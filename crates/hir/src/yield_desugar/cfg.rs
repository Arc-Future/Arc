//! RFC 044：迭代器方法体 → 微型 CFG。
//!
//! 结构化控制流（if / while / for / switch / break / continue / return;）
//! 按块拆分：每个 `yield return` 是挂起点，其后继块即恢复点（状态号 = 块 id，
//! 入口块恒为 0）。不可达块在发射阶段过滤。构建同时完成标识符改写
//!（参数/局部 → 提升字段）与 M1 边界拒绝（try/using/lock/foreach/lambda 等）。

use ast::*;

use super::entry::{body_has_throw, body_has_yield};
use super::rename::Renamer;

#[derive(Debug)]
pub(crate) enum Term {
    Unset,
    Jump(usize),
    Branch(Spanned<Expr>, usize, usize),
    /// 用户 switch 原样保留分派（模式/守卫不改语义），各臂落入目标块。
    Switch {
        scrutinee: Spanned<Expr>,
        arms: Vec<SwitchArmCfg>,
    },
    /// 挂起：写入 `__current`，登记恢复点，`return true`。
    Yield(Spanned<Expr>, usize),
    /// 终结（含 `yield break` / 落出方法尾 / `return;`）。
    Finish,
}

#[derive(Debug)]
pub(crate) struct SwitchArmCfg {
    pub(crate) pattern: Option<Pattern>,
    pub(crate) when: Option<Spanned<Expr>>,
    pub(crate) target: usize,
}

#[derive(Debug)]
pub(crate) struct CfgBlock {
    pub(crate) stmts: Vec<Spanned<Stmt>>,
    pub(crate) term: Term,
}

enum Breakable {
    Loop { brk: usize, cont: usize },
    Switch { after: usize },
}

/// foreach 展开辅助：方法调用表达式（RFC 044 M2）。
fn cfg_method_call(receiver: Expr, method: &str) -> Spanned<Expr> {
    Spanned::new(
        Expr::MethodCall {
            receiver: Box::new(Spanned::new(receiver, Span::DUMMY)),
            method: method.into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        },
        Span::DUMMY,
    )
}

/// foreach 展开辅助：字段访问表达式（`__e.Current`）。
fn cfg_field_access(receiver: Expr, field: &str) -> Spanned<Expr> {
    Spanned::new(
        Expr::Field {
            receiver: Box::new(Spanned::new(receiver, Span::DUMMY)),
            field: field.into(),
        },
        Span::DUMMY,
    )
}

pub(crate) struct CfgBuilder {
    blocks: Vec<CfgBlock>,
    breakables: Vec<Breakable>,
    renamer: Renamer,
}

impl CfgBuilder {
    pub(crate) fn new(renamer: Renamer) -> Self {
        Self {
            blocks: Vec::new(),
            breakables: Vec::new(),
            renamer,
        }
    }

    pub(crate) fn build(mut self, body: Block) -> (Vec<CfgBlock>, Vec<String>, bool) {
        let entry = self.new_block();
        debug_assert_eq!(entry, 0);
        if body.tail.is_some() {
            self.renamer
                .errors
                .push("迭代器方法体不支持尾表达式（RFC 044）；请用 `yield return` 产出元素".into());
        }
        let end = self.seq(body.stmts, entry);
        if matches!(self.blocks[end].term, Term::Unset) {
            self.blocks[end].term = Term::Finish;
        }
        let host_captured = self.renamer.host_captured;
        (self.blocks, self.renamer.errors, host_captured)
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push(CfgBlock {
            stmts: Vec::new(),
            term: Term::Unset,
        });
        self.blocks.len() - 1
    }

    fn push(&mut self, cur: usize, stmt: Spanned<Stmt>) {
        self.blocks[cur].stmts.push(stmt);
    }

    fn set_term(&mut self, cur: usize, term: Term) {
        self.blocks[cur].term = term;
    }

    /// 顺序 lowering 一段语句；返回尾端开放块 id。
    fn seq(&mut self, stmts: Vec<Spanned<Stmt>>, mut cur: usize) -> usize {
        for stmt in stmts {
            cur = self.one(stmt, cur);
        }
        cur
    }

    fn one(&mut self, stmt: Spanned<Stmt>, cur: usize) -> usize {
        let span = stmt.span;
        match stmt.node {
            Stmt::YieldReturn { value } => {
                let mut value = value;
                self.renamer.expr(&mut value);
                let resume = self.new_block();
                self.set_term(cur, Term::Yield(value, resume));
                resume
            }
            Stmt::YieldBreak | Stmt::Return(None) => {
                self.set_term(cur, Term::Finish);
                self.new_block()
            }
            Stmt::Return(Some(_)) => {
                self.renamer.errors.push(
                    "迭代器方法体内 `return expr;` 非法（RFC 044，对齐 C#）；请用 `yield return`，或 `return;` 终结序列".into(),
                );
                self.new_block()
            }
            Stmt::Break => {
                let target = match self.breakables.last() {
                    Some(Breakable::Loop { brk, .. }) | Some(Breakable::Switch { after: brk }) => {
                        *brk
                    }
                    None => {
                        self.renamer
                            .errors
                            .push("`break` 不在循环或 switch 内".into());
                        return self.new_block();
                    }
                };
                self.set_term(cur, Term::Jump(target));
                self.new_block()
            }
            Stmt::Continue => {
                let target = self.breakables.iter().rev().find_map(|b| match b {
                    Breakable::Loop { cont, .. } => Some(*cont),
                    Breakable::Switch { .. } => None,
                });
                match target {
                    Some(t) => {
                        self.set_term(cur, Term::Jump(t));
                        self.new_block()
                    }
                    None => {
                        self.renamer.errors.push("`continue` 不在循环内".into());
                        self.new_block()
                    }
                }
            }
            Stmt::Let {
                name, ty: _, init, ..
            } => {
                let field = self.renamer.field_of(&name);
                if let Some(mut value) = init {
                    self.renamer.expr(&mut value);
                    self.push(
                        cur,
                        Spanned::new(
                            Stmt::Assign {
                                target: Spanned::new(Expr::Ident(field), span),
                                value,
                            },
                            span,
                        ),
                    );
                }
                cur
            }
            Stmt::Assign {
                mut target,
                mut value,
            } => {
                self.renamer.expr(&mut target);
                self.renamer.expr(&mut value);
                self.push(cur, Spanned::new(Stmt::Assign { target, value }, span));
                cur
            }
            Stmt::Expr(expr) => self.expr_stmt(expr, cur, span),
            Stmt::Throw { mut expr } => {
                self.renamer.expr(&mut expr);
                self.push(cur, Spanned::new(Stmt::Throw { expr }, span));
                cur
            }
            Stmt::While { mut cond, body } => {
                self.renamer.expr(&mut cond);
                // 循环头必须独立成块：若把 Branch 终结在 `cur`（可能已含
                // 前置语句，如 `let i = 1`），回边 Jump(cur) 会随每次迭代
                // 重新执行这些前置语句（i 被恒重置 → 死循环）。
                let head = self.new_block();
                let body_blk = self.new_block();
                let after = self.new_block();
                self.set_term(cur, Term::Jump(head));
                self.set_term(head, Term::Branch(cond, body_blk, after));
                self.breakables.push(Breakable::Loop {
                    brk: after,
                    cont: head,
                });
                let end = self.block_into(body, body_blk, head);
                debug_assert_eq!(end, body_blk);
                self.breakables.pop();
                after
            }
            Stmt::ForC {
                init,
                cond,
                inc,
                body,
            } => {
                let mut cur = cur;
                if let Some(s) = init {
                    cur = self.one(s.map(|b| *b), cur);
                }
                // 同 While：cond 求值点独立成块，latch 回边跳 head 而非
                // 含 init 的 `cur`，否则 init（`i = 0`）每圈重执行。
                let head = self.new_block();
                let body_blk = self.new_block();
                let latch = self.new_block();
                let after = self.new_block();
                self.set_term(cur, Term::Jump(head));
                match cond {
                    Some(mut c) => {
                        self.renamer.expr(&mut c);
                        self.set_term(head, Term::Branch(c, body_blk, after));
                    }
                    None => self.set_term(head, Term::Jump(body_blk)),
                }
                self.breakables.push(Breakable::Loop {
                    brk: after,
                    cont: latch,
                });
                let end = self.block_into(body, body_blk, latch);
                debug_assert_eq!(end, body_blk);
                self.breakables.pop();
                if let Some(s) = inc {
                    self.one(s.map(|b| *b), latch);
                }
                self.set_term(latch, Term::Jump(head));
                after
            }
            Stmt::For { var, iter, body } => {
                // RFC 044 M2：foreach 展开为枚举器协议（GetEnumerator → MoveNext →
                // Current）——迭代变量（__loc_<var>）与枚举器（__enum_<var>）为
                // 无类型提升字段，类型由 typeck 从 Current/GetEnumerator 返回类型
                // 后置推断（合成类字段类型后置解析）。
                let mut iter = iter;
                self.renamer.expr(&mut iter);
                let e_field: Ident = format!("__enum_{}", var).into();
                let x_field = self.renamer.field_of(&var);
                self.push(
                    cur,
                    Spanned::new(
                        Stmt::Assign {
                            target: Spanned::new(Expr::Ident(e_field.clone()), span),
                            value: cfg_method_call(iter.node, "GetEnumerator"),
                        },
                        span,
                    ),
                );
                let head = self.new_block();
                let body_blk = self.new_block();
                let after = self.new_block();
                self.set_term(cur, Term::Jump(head));
                self.set_term(
                    head,
                    Term::Branch(
                        cfg_method_call(Expr::Ident(e_field.clone()), "MoveNext"),
                        body_blk,
                        after,
                    ),
                );
                self.push(
                    body_blk,
                    Spanned::new(
                        Stmt::Assign {
                            target: Spanned::new(Expr::Ident(x_field), span),
                            value: cfg_field_access(Expr::Ident(e_field), "Current"),
                        },
                        span,
                    ),
                );
                self.breakables.push(Breakable::Loop {
                    brk: after,
                    cont: head,
                });
                let end = self.block_into(body, body_blk, head);
                debug_assert_eq!(end, body_blk);
                self.breakables.pop();
                after
            }
            Stmt::DeconstructAssign {
                declare,
                mut targets,
                mut value,
            } => {
                // RFC 044 M2：解构保留节点（typeck 的 check_deconstruct_assign 展开
                // 为字段赋值）；目标绑定名改写为提升字段（类型后置推断）。
                self.renamer.expr(&mut value);
                self.renamer.deconstruct_targets(&mut targets);
                self.push(
                    cur,
                    Spanned::new(
                        Stmt::DeconstructAssign {
                            declare,
                            targets,
                            value,
                        },
                        span,
                    ),
                );
                cur
            }
            Stmt::TryCatch { .. } => {
                self.renamer.errors.push(
                    "迭代器方法体内暂不支持 try/catch（RFC 044 M2：catch 跨挂起点机制后置）；try/finally 已支持".into(),
                );
                self.new_block()
            }
            Stmt::TryFinally { body, finally } => {
                // RFC 044 M2：try/finally 内 yield——finally 内容内联到 try 区域
                // 每个终止/落出块（Finish = yield break / 落出方法尾 → 返回 false
                // 前执行；落出 → 跳 after 前执行）。挂起点（yield return）不执行
                // finally（C# 语义：挂起时 finally 延迟到序列完成/Dispose——
                // Dispose 链由 emit 阶段按挂起点活动 finally 集合合成）。
                // finally 内不允许 yield（C# 同样禁止）；throw 在区域内暂不支持
                // （异常路径 finally 需 EH 栈，状态机无——M1 边界）。
                if body_has_yield(&finally.stmts) {
                    self.renamer
                        .errors
                        .push("finally 块内不允许 `yield`（RFC 044 M2，对齐 C#）".into());
                    return self.new_block();
                }
                if body_has_throw(&finally.stmts) || body_has_throw(&body.stmts) {
                    self.renamer.errors.push(
                        "迭代器方法体内 try/finally 区域暂不支持 `throw`（RFC 044 M2：异常路径 finally 机制后置）".into(),
                    );
                    return self.new_block();
                }
                // 区域边界：after 之后、拆块期间新建的块 = try body 区域
                // （region_start 必须在 after 之后取值，否则区间为空——实测
                // finally 内联丢失、FIN_RAN 不入 IR）。
                let after = self.new_block();
                let region_start = self.blocks.len();
                let end = self.block_into(body, cur, after);
                debug_assert_eq!(end, cur);
                // finally 内联：区域 [region_start, blocks.len()) 内的终止/落出块——
                // Finish（yield break / 落出方法尾）与**跳出区域**的 Jump（落出 try）。
                // 循环内跳转（回边/区域内 Jump 目标）不是落出——若内联会使 finally
                // 每轮循环执行（实测 FIN_RAN × 7）。
                for id in region_start..self.blocks.len() {
                    let terminates = match &self.blocks[id].term {
                        Term::Finish | Term::Unset => true,
                        Term::Jump(target) => *target < region_start,
                        _ => false,
                    };
                    if !terminates {
                        continue;
                    }
                    let mut fin = finally.stmts.clone();
                    for f in fin.iter_mut() {
                        self.renamer.stmt(&mut f.node);
                    }
                    self.blocks[id].stmts.extend(fin);
                }
                after
            }
            Stmt::Using { .. }
            | Stmt::UsingVar { .. }
            | Stmt::AwaitUsing { .. }
            | Stmt::AwaitUsingVar { .. } => {
                self.renamer.errors.push(
                    "迭代器方法体内暂不支持 using / await using（RFC 044 M1：Dispose 跨挂起点语义后置）".into(),
                );
                self.new_block()
            }
            Stmt::Lock { .. } => {
                self.renamer
                    .errors
                    .push("迭代器方法体内暂不支持 lock（RFC 044 M1）".into());
                self.new_block()
            }
        }
    }

    /// 表达式语句：`if` / `switch` 语句形态走结构化拆分，其余原样落块。
    fn expr_stmt(&mut self, mut expr: Spanned<Expr>, cur: usize, span: Span) -> usize {
        match expr.node {
            Expr::If {
                mut cond,
                then_branch,
                else_branch,
            } => {
                self.renamer.expr(&mut cond);
                let then_blk = self.new_block();
                let after = self.new_block();
                let else_blk = if else_branch.is_some() {
                    self.new_block()
                } else {
                    after
                };
                self.set_term(cur, Term::Branch(*cond, then_blk, else_blk));
                let end = self.block_into(then_branch, then_blk, after);
                debug_assert_eq!(end, then_blk);
                if let Some(else_branch) = else_branch {
                    let end = self.block_into(else_branch, else_blk, after);
                    debug_assert_eq!(end, else_blk);
                }
                after
            }
            Expr::Switch(SwitchExpr {
                mut scrutinee,
                cases,
            }) => {
                // 驱动分派要求 switch 全覆盖（无 default 且无臂命中时状态
                // 不变 → continue → 死循环），M1 拒绝无 default 的 switch。
                if !matches!(cases.last(), Some(c) if c.pattern.is_none()) {
                    self.renamer.errors.push(
                        "迭代器方法体内的 switch 必须含 default 臂（RFC 044 M1：驱动分派需全覆盖）"
                            .into(),
                    );
                }
                self.renamer.expr(&mut scrutinee);
                let after = self.new_block();
                let mut arms = Vec::new();
                let mut bodies = Vec::new();
                for case in cases {
                    let SwitchCase {
                        mut pattern,
                        mut when,
                        body,
                    } = case;
                    if let Some(w) = when.as_mut() {
                        self.renamer.expr(w);
                    }
                    // RFC 044 M2：绑定模式放行——绑定名改写为提升字段
                    // （`case T n` 显式类型；`case var n` 由 typeck 后置推断）。
                    if let Some(p) = pattern.as_mut() {
                        self.renamer.pattern(p);
                    }
                    let body_blk = self.new_block();
                    arms.push(SwitchArmCfg {
                        pattern,
                        when: when.clone(),
                        target: body_blk,
                    });
                    bodies.push((body, body_blk));
                }
                self.breakables.push(Breakable::Switch { after });
                for (body, body_blk) in bodies {
                    let end = self.block_into(body, body_blk, after);
                    debug_assert_eq!(end, body_blk);
                }
                self.breakables.pop();
                self.set_term(
                    cur,
                    Term::Switch {
                        scrutinee: *scrutinee,
                        arms,
                    },
                );
                after
            }
            other => {
                expr.node = other;
                self.renamer.expr(&mut expr);
                self.push(cur, Spanned::new(Stmt::Expr(expr), span));
                cur
            }
        }
    }

    /// 块体 lowering 进 `blk`；若尾端仍开放则接 `exit`，返回 `blk`。
    fn block_into(&mut self, block: Block, blk: usize, exit: usize) -> usize {
        let Block { stmts, tail } = block;
        let mut end = self.seq(stmts, blk);
        if let Some(tail) = tail {
            // 尾表达式与语句位置同路径：`else if` 链经 parser 表示为 else 分支块
            // 的 tail（`Block { tail: If }`），若在此仅按普通表达式落块，分支内
            // 标识符不被改写（undefined name）、挂起点不被拆分（错误状态机）。
            end = self.one(Spanned::new(Stmt::Expr(*tail), Span::DUMMY), end);
        }
        if matches!(self.blocks[end].term, Term::Unset) {
            self.set_term(end, Term::Jump(exit));
        }
        blk
    }
}
