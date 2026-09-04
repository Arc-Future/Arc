//! RFC 044：提升变量改名器 + 表达式位置合法性校验。
//!
//! 迭代器方法的参数与局部变量提升为状态机字段（`__prm_*` / `__loc_*`），
//! 方法体内所有标识符引用同步改写。本模块同时承担表达式位置的最后防线：
//! lambda / `this` / `base` / 表达式块内的声明与 yield 在此精确拒绝。

use std::collections::HashMap;

use ast::*;

pub(crate) struct Renamer {
    /// 源名 → 提升字段名。扁平映射；遮蔽声明在绑定收集阶段已拒绝。
    map: HashMap<Ident, Ident>,
    /// RFC 044 M2：方法体是否使用 `this`（触发宿主引用字段 `__host` 捕获）。
    pub host_captured: bool,
    pub errors: Vec<String>,
}

impl Renamer {
    pub(crate) fn new() -> Self {
        Self {
            map: HashMap::new(),
            host_captured: false,
            errors: Vec::new(),
        }
    }

    /// 登记一处提升绑定；同名重复声明（遮蔽）精确报错。
    pub(crate) fn bind(&mut self, name: &Ident, field: Ident) {
        if self.map.contains_key(name) {
            self.errors.push(format!(
                "迭代器方法体内局部变量 `{name}` 重复声明（遮蔽）；RFC 044 M1 要求提升绑定名唯一"
            ));
            return;
        }
        self.map.insert(name.clone(), field);
    }

    /// 已登记绑定的提升字段名（语句位置 Let 的声明点使用）。
    pub(crate) fn field_of(&self, name: &Ident) -> Ident {
        self.map.get(name).cloned().unwrap_or_else(|| name.clone())
    }

    /// RFC 044 M2：解构目标改写——Bind(Some) 绑定名替换为提升字段名。
    pub(crate) fn deconstruct_targets(&mut self, targets: &mut [DeconstructTarget]) {
        for t in targets {
            match t {
                DeconstructTarget::Bind(Some(name)) => {
                    *name = self.field_of(name);
                }
                DeconstructTarget::Bind(None) => {}
                DeconstructTarget::Nested(subs) => self.deconstruct_targets(subs),
            }
        }
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    pub(crate) fn stmts(&mut self, stmts: &mut [Spanned<Stmt>]) {
        for stmt in stmts {
            self.stmt(&mut stmt.node);
        }
    }

    /// 表达式位置块内语句的改写——声明与 yield 在此非法（M1 边界）。
    pub(crate) fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { .. } => self
                .error("迭代器方法体内的局部声明必须处于语句位置（RFC 044 M1 拒绝表达式块内声明）"),
            Stmt::YieldReturn { .. } | Stmt::YieldBreak => {
                self.error("yield 语句必须处于方法体语句位置（RFC 044 M1 拒绝表达式块内 yield）")
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Return(Some(e)) => self.expr(e),
            Stmt::Return(None) => {}
            Stmt::Throw { expr } => self.expr(expr),
            Stmt::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::ForC {
                init,
                cond,
                inc,
                body,
            } => {
                if let Some(s) = init {
                    self.stmt(&mut s.node);
                }
                if let Some(c) = cond {
                    self.expr(c);
                }
                if let Some(s) = inc {
                    self.stmt(&mut s.node);
                }
                self.block(body);
            }
            Stmt::For { iter, body, .. } => {
                self.expr(iter);
                self.block(body);
            }
            Stmt::Break | Stmt::Continue => {}
            Stmt::TryCatch {
                try_body,
                when_cond,
                catch_body,
                finally,
                ..
            } => {
                self.block(try_body);
                if let Some(w) = when_cond {
                    self.expr(w);
                }
                self.block(catch_body);
                if let Some(f) = finally {
                    self.block(f);
                }
            }
            Stmt::TryFinally { body, finally } => {
                self.block(body);
                self.block(finally);
            }
            Stmt::Using { init, body, .. } | Stmt::AwaitUsing { init, body, .. } => {
                self.expr(init);
                self.block(body);
            }
            Stmt::UsingVar { init, .. } | Stmt::AwaitUsingVar { init, .. } => {
                self.expr(init);
            }
            Stmt::Lock { expr, body } => {
                self.expr(expr);
                self.block(body);
            }
            Stmt::DeconstructAssign { targets, value, .. } => {
                self.deconstruct_targets(targets);
                self.expr(value);
            }
        }
    }

    fn block(&mut self, block: &mut Block) {
        self.stmts(&mut block.stmts);
        if let Some(tail) = block.tail.as_deref_mut() {
            self.expr(tail);
        }
    }

    pub(crate) fn expr(&mut self, spanned: &mut Spanned<Expr>) {
        match &mut spanned.node {
            Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::BoolLit(_)
            | Expr::StringLit(_)
            | Expr::CharLit(_)
            | Expr::Null => {}
            Expr::This => {
                // RFC 044 M2：this 捕获——改写为宿主引用字段 `__host`。
                // 仅支撑显式 `this.X` 成员访问（公开成员 `this.` 前缀约定）；
                // 裸私有字段引用在脱糖层无名字解析，无法与 `Console`/`Math`
                // 等外部裸标识符区分，仍为边界（见 entry.rs 守卫）。
                self.host_captured = true;
                spanned.node = Expr::Ident("__host".into());
            }
            Expr::Base => {
                self.error("迭代器方法体内暂不支持 `base`（RFC 044）");
            }
            Expr::Ident(name) => {
                if let Some(renamed) = self.map.get(name).cloned() {
                    *name = renamed;
                }
            }
            Expr::Path(segments) => {
                // 单段路径等价标识符引用；多段限定名指向外部作用域，不改写。
                if segments.len() == 1 {
                    if let Some(renamed) = self.map.get(&segments[0]).cloned() {
                        segments[0] = renamed;
                    }
                }
            }
            Expr::Comptime(inner) => self.expr(inner),
            Expr::InterpolatedString { parts } => {
                for part in parts {
                    if let InterpPart::Expr(hole) = part {
                        self.expr(&mut hole.expr);
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                self.expr(left);
                self.expr(right);
            }
            // 赋值表达式：目标与值均递归重命名（迭代器提升变量可作目标）。
            Expr::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            Expr::Unary { expr, .. } => self.expr(expr),
            Expr::Call { func, args, .. } => {
                self.expr(func);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.expr(receiver);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::Field { receiver, .. } => self.expr(receiver),
            Expr::Index { receiver, index } => {
                self.expr(receiver);
                self.expr(index);
            }
            Expr::Lambda(_) => self.error(
                "迭代器方法体内暂不支持 lambda（RFC 044 M1，含 LINQ 查询脱糖产物）；如需谓词请展开为显式循环",
            ),
            Expr::ExpressionLit(_) => self.error(
                "迭代器方法体内暂不支持表达式树字面量（RFC 044 M1）",
            ),
            Expr::Await(inner) => self.expr(inner),
            Expr::Block(block) => self.block(block),
            // if / switch 的分支块是**语句性上下文**：cfg 构建对其做结构化
            // 拆分（block_into），块内声明提升与挂起点均完整支持（与
            // entry 的 collect_locals_expr 提升范围一致）。本层只改写
            // 条件/模式表达式，分支语句交由 cfg 遍历——若在此走 block()
            // 会把分支内 let/yield 误判为「表达式块内」而拒绝。
            Expr::If {
                cond,
                then_branch: _,
                else_branch: _,
            } => {
                self.expr(cond);
            }
            Expr::Switch(SwitchExpr { scrutinee, cases }) => {
                self.expr(scrutinee);
                for case in cases {
                    if let Some(p) = &mut case.pattern {
                        self.pattern(p);
                    }
                    if let Some(w) = &mut case.when {
                        self.expr(w);
                    }
                }
            }
            Expr::SwitchForm(SwitchExprForm { scrutinee, arms }) => {
                self.expr(scrutinee);
                for arm in arms {
                    self.pattern(&mut arm.pattern);
                    if let Some(w) = &mut arm.when {
                        self.expr(w);
                    }
                    self.expr(&mut arm.body);
                }
            }
            Expr::CollectionExpr { elements } => {
                for el in elements {
                    match el {
                        CollectionElement::Element(e) | CollectionElement::Spread(e) => {
                            self.expr(e);
                        }
                    }
                }
            }
            Expr::Cast { expr, .. } => self.expr(expr),
            Expr::Box { expr, .. } | Expr::Unbox { expr, .. } => self.expr(expr),
            Expr::New { args, obj_init, .. } => {
                for a in args {
                    self.expr(a);
                }
                if let Some(inits) = obj_init {
                    for (_, v) in inits {
                        self.expr(v);
                    }
                }
            }
            Expr::NewArray { length, .. } => self.expr(length),
            Expr::Query(QueryExpr { clauses, select }) => {
                for clause in clauses {
                    match clause {
                        QueryClause::From { source, .. } => self.expr(source),
                        QueryClause::Let { value, .. } => self.expr(value),
                        QueryClause::Where(p) => self.expr(p),
                        QueryClause::OrderBy { key, .. } => self.expr(key),
                        QueryClause::Join {
                            source, on_left, on_right, ..
                        } => {
                            self.expr(source);
                            self.expr(on_left);
                            self.expr(on_right);
                        }
                        QueryClause::GroupBy { key, element, .. } => {
                            self.expr(key);
                            if let Some(el) = element {
                                self.expr(el);
                            }
                        }
                    }
                }
                self.expr(select);
            }
            Expr::RefArg { expr, .. } => self.expr(expr),
            Expr::NamedArg { expr, .. } => self.expr(expr),
            Expr::StackSpanLit { elements, .. } => {
                for e in elements {
                    self.expr(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond);
                self.expr(then_branch);
                self.expr(else_branch);
            }
            Expr::Coalesce { left, right } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::NullCond { access } | Expr::ForceDeref { access } => self.expr(access),
            Expr::Default { .. } | Expr::TypeOf(_) => {}
            Expr::Is { expr, pattern } => {
                self.expr(expr);
                self.is_pattern(pattern);
            }
            Expr::With { receiver, inits } => {
                self.expr(receiver);
                for (_, v) in inits {
                    self.expr(v);
                }
            }
        }
    }

    /// switch `case` 模式改写：常量表达式递归 + 绑定名替换为提升字段
    /// （RFC 044 M2：`case T n` / `case var n` / variant / 位置绑定）。
    pub(crate) fn pattern(&mut self, pattern: &mut Pattern) {
        match pattern {
            Pattern::Literal(e) => self.expr(e),
            Pattern::Positional(subs) => self.positional_subs(subs),
            Pattern::Type {
                binding: Some(name),
                ..
            } => *name = self.field_of(name),
            Pattern::Var(name) => *name = self.field_of(name),
            Pattern::Variant {
                binding: Some(name),
                ..
            } => *name = self.field_of(name),
            Pattern::Wildcard
            | Pattern::Ident(_)
            | Pattern::Type { binding: None, .. }
            | Pattern::Null
            | Pattern::Variant { binding: None, .. } => {}
        }
    }

    /// 位置子模式改写：常量递归、绑定名替换（RFC 044 M2）。
    fn positional_subs(&mut self, subs: &mut [PositionalSubpattern]) {
        for sub in subs {
            match sub {
                PositionalSubpattern::Const(e) => self.expr(e),
                PositionalSubpattern::Var(name) => *name = self.field_of(name),
                PositionalSubpattern::Typed { name, .. } => *name = self.field_of(name),
                PositionalSubpattern::Nested(inner) => self.positional_subs(inner),
                PositionalSubpattern::Discard => {}
            }
        }
    }

    fn is_pattern(&mut self, pattern: &mut IsPattern) {
        match pattern {
            IsPattern::Constant(e) => self.expr(e),
            IsPattern::Positional(subs) => {
                for sub in subs {
                    if let PositionalSubpattern::Const(e) = sub {
                        self.expr(e);
                    }
                }
            }
            IsPattern::And { left, right } | IsPattern::Or { left, right } => {
                self.is_pattern(&mut left.node);
                self.is_pattern(&mut right.node);
            }
            IsPattern::Not { inner } => self.is_pattern(&mut inner.node),
            IsPattern::Type { .. } | IsPattern::Var(_) | IsPattern::Null => {}
        }
    }
}
