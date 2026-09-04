//! RFC 044：迭代器状态机类合成。
//!
//! CFG → `while (true) { switch (__state) { case N: … } }` 驱动 + 实现枚举
//! 接口的合成类（字段提升 / 构造器 / MoveNext(|Async) / Current /
//! Get(Async)Enumerator）。下游管线只见普通类——零新机制。

use ast::*;

use super::cfg::{CfgBlock, SwitchArmCfg, Term};

/// 迭代器方法类别（由声明返回类型决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IterKind {
    /// 返回 `IEnumerable<T>`：合成类同时实现 IEnumerable<T> + IEnumerator<T>。
    SyncEnumerable,
    /// 返回 `IEnumerator<T>`：仅实现 IEnumerator<T>。
    SyncEnumerator,
    /// 返回 `IAsyncEnumerable<T>`：实现 IAsyncEnumerable<T> + IAsyncEnumerator<T>。
    AsyncEnumerable,
}

/// 提升字段（参数或局部）。
/// `ty` 为 None 表示 var 推断局部（foreach 迭代变量 / 解构目标 / `var x`）：
/// 合成类字段发射为 `Type::Infer`，由 typeck 在状态机方法体首次赋值时后置推断
/// 回填（RFC 044 M2 合成类字段类型后置解析）。
pub(crate) struct HoistedField {
    pub(crate) name: Ident,
    pub(crate) ty: Option<Spanned<Type>>,
}

/// 合成一个状态机类所需的全量输入。
pub(crate) struct SmPlan {
    pub(crate) kind: IterKind,
    pub(crate) class_name: Ident,
    pub(crate) elem_ty: Spanned<Type>,
    pub(crate) fields: Vec<HoistedField>,
    /// ctor 形参（原方法形参克隆）。
    pub(crate) ctor_params: Vec<Param>,
    /// ctor 形参 → 提升字段赋值对。
    pub(crate) param_captures: Vec<(Ident, Ident)>,
    /// RFC 044 M2：方法体是否使用 `this`（决定宿主引用字段 `__host` 的捕获）。
    pub(crate) host_captured: bool,
    /// 宿主捕获的 ctor 形参（`__this: <Host>`）；None 表示未捕获。
    pub(crate) host_param: Option<Param>,
    /// 宿主类名：类方法脱糖时为宿主类，顶层函数为 None。typeck 据此放行
    /// 合成类对宿主 private 成员的访问（C# 嵌套状态机的等价可见性语义）。
    pub(crate) host: Option<Ident>,
    /// RFC 044 M3：合成类泛型形参 = 宿主类泛型 + 方法泛型（顺序即模板形参
    /// 顺序，与重写体/Get(Async)Enumerator 的实参顺序一致）。
    pub(crate) generics: Vec<GenericParam>,
    /// 合成类 where 约束（克隆自迭代器方法，泛型实例化约束校验用）。
    pub(crate) where_clause: Vec<TypeConstraint>,
}

const STATE: &str = "__state";
const CURRENT: &str = "__current";

fn dummy_stmt(node: Stmt) -> Spanned<Stmt> {
    Spanned::new(node, Span::DUMMY)
}

fn dummy_expr(node: Expr) -> Spanned<Expr> {
    Spanned::new(node, Span::DUMMY)
}

fn ident(name: &str) -> Spanned<Expr> {
    dummy_expr(Expr::Ident(name.into()))
}

fn assign(target: &str, value: Expr) -> Spanned<Stmt> {
    dummy_stmt(Stmt::Assign {
        target: ident(target),
        value: dummy_expr(value),
    })
}

fn generic_ty(name: &str, elem: &Spanned<Type>) -> Spanned<Type> {
    Spanned::new(
        Type::Named {
            path: vec![name.into()],
            generics: vec![elem.clone()],
        },
        Span::DUMMY,
    )
}

/// 合成类类型：`__Yield_X_0` [+ `<形参…>`]（RFC 044 M3 泛型实例化形态）。
fn sm_type(plan: &SmPlan) -> Spanned<Type> {
    Spanned::new(
        Type::Named {
            path: vec![plan.class_name.clone()],
            generics: plan
                .generics
                .iter()
                .map(|g| {
                    Spanned::new(
                        Type::Named {
                            path: vec![g.name.clone()],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    )
                })
                .collect(),
        },
        Span::DUMMY,
    )
}

fn block(stmts: Vec<Spanned<Stmt>>) -> Block {
    Block { stmts, tail: None }
}

fn method(
    name: &str,
    params: Vec<Param>,
    ret: Option<Spanned<Type>>,
    is_async: bool,
    body: Block,
) -> Spanned<MethodDef> {
    Spanned::new(
        MethodDef {
            sig: MethodSig {
                vis: Visibility::Public,
                name: name.into(),
                generics: vec![],
                where_clause: vec![],
                params,
                ret,
                is_async,
                modifier: MethodModifier::None,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(body),
            doc: None,
        },
        Span::DUMMY,
    )
}

/// 重写后的原方法体：`return new <SM>(<实参…>);`
pub(crate) fn rewritten_method_body(plan: &SmPlan) -> Block {
    let mut args = plan
        .ctor_params
        .iter()
        .map(|p| ident(p.name.as_str()))
        .collect::<Vec<_>>();
    // 宿主捕获：实例方法内 `this` 即为宿主引用，作为 ctor 末参传入。
    if plan.host_captured {
        args.push(dummy_expr(Expr::This));
    }
    block(vec![dummy_stmt(Stmt::Return(Some(dummy_expr(
        Expr::New {
            ty: sm_type(plan),
            args,
            obj_init: None,
        },
    ))))])
}

/// 合成状态机类。
pub(crate) fn emit_state_machine(plan: &SmPlan, blocks: &[CfgBlock]) -> ClassDef {
    let mut fields = vec![
        HoistedField {
            name: STATE.into(),
            ty: Some(Type::named("int")),
        },
        HoistedField {
            name: CURRENT.into(),
            ty: Some(plan.elem_ty.clone()),
        },
    ];
    fields.extend(plan.fields.iter().map(|f| HoistedField {
        name: f.name.clone(),
        ty: f.ty.clone(),
    }));

    let mut methods = Vec::new();
    match plan.kind {
        IterKind::SyncEnumerable => {
            methods.push(get_enumerator_method(
                "GetEnumerator",
                "IEnumerator",
                &plan.elem_ty,
                plan,
            ));
        }
        IterKind::SyncEnumerator => {}
        IterKind::AsyncEnumerable => {
            methods.push(get_enumerator_method(
                "GetAsyncEnumerator",
                "IAsyncEnumerator",
                &plan.elem_ty,
                plan,
            ));
        }
    }

    let driver = driver_body(blocks);
    match plan.kind {
        IterKind::AsyncEnumerable => {
            let task_bool = Spanned::new(
                Type::Named {
                    path: vec!["Task".into()],
                    generics: vec![Type::named("bool")],
                },
                Span::DUMMY,
            );
            methods.push(method(
                "MoveNextAsync",
                vec![],
                Some(task_bool),
                true,
                driver,
            ));
        }
        IterKind::SyncEnumerable | IterKind::SyncEnumerator => {
            methods.push(method(
                "MoveNext",
                vec![],
                Some(Type::named("bool")),
                false,
                driver,
            ));
        }
    }

    // Current：接口契约为属性（对齐 C# IEnumerator<T>.Current / IAsyncEnumerator<T>.Current）。
    let current_prop = Spanned::new(
        PropertyDef {
            vis: Visibility::Public,
            name: "Current".into(),
            ty: plan.elem_ty.clone(),
            has_get: true,
            has_set: false,
            has_init: false,
            is_required: false,
            get_body: Some(block(vec![dummy_stmt(Stmt::Return(Some(ident(CURRENT))))])),
            set_body: None,
            get_vis: None,
            set_vis: None,
            modifier: MethodModifier::None,
            attributes: vec![],
            is_static_abstract: false,
            index_params: vec![],
            init: None,
            doc: None,
        },
        Span::DUMMY,
    );

    let bases: Vec<Type> = match plan.kind {
        IterKind::SyncEnumerable => vec![
            generic_ty("IEnumerable", &plan.elem_ty).node,
            generic_ty("IEnumerator", &plan.elem_ty).node,
        ],
        IterKind::SyncEnumerator => vec![generic_ty("IEnumerator", &plan.elem_ty).node],
        IterKind::AsyncEnumerable => vec![
            generic_ty("IAsyncEnumerable", &plan.elem_ty).node,
            generic_ty("IAsyncEnumerator", &plan.elem_ty).node,
        ],
    };

    ClassDef {
        vis: Visibility::Internal,
        is_static: false,
        is_abstract: false,
        is_partial: false,
        is_record: false,
        name: plan.class_name.clone(),
        generics: plan.generics.clone(),
        where_clause: plan.where_clause.clone(),
        bases,
        fields: fields
            .into_iter()
            .map(|f| FieldDef {
                vis: Visibility::Private,
                name: f.name,
                // RFC 044 M2：无类型提升字段发射 `Type::Infer`，typeck 后置推断。
                ty: f
                    .ty
                    .unwrap_or_else(|| Spanned::new(Type::Infer, Span::DUMMY)),
                is_readonly: false,
                is_const: false,
                is_static: false,
                init: None,
                attributes: vec![],
                doc: None,
            })
            .collect(),
        properties: vec![current_prop.node],
        methods,
        constructors: vec![Spanned::new(
            ConstructorDef {
                vis: Visibility::Public,
                params: ctor_params(plan),
                body: ctor_body(plan),
                base_args: None,
                doc: None,
            },
            Span::DUMMY,
        )],
        attributes: vec![],
        doc: None,
        synthesized_host: plan.host.clone(),
    }
}

/// ctor 形参：原方法形参 + 宿主捕获形参（`__this`，若有）。
fn ctor_params(plan: &SmPlan) -> Vec<Param> {
    let mut params = plan.ctor_params.clone();
    if let Some(hp) = &plan.host_param {
        params.push(hp.clone());
    }
    params
}

/// ctor：`__state = 0;` + 逐参数捕获赋值。
fn ctor_body(plan: &SmPlan) -> Block {
    let mut stmts = vec![assign(STATE, Expr::IntLit(0))];
    for (param, field) in &plan.param_captures {
        stmts.push(dummy_stmt(Stmt::Assign {
            target: ident(field.as_str()),
            value: ident(param.as_str()),
        }));
    }
    block(stmts)
}

/// GetEnumerator / GetAsyncEnumerator：返回新鲜实例（可重复枚举契约）。
fn get_enumerator_method(
    name: &str,
    ret_iface: &str,
    elem_ty: &Spanned<Type>,
    plan: &SmPlan,
) -> Spanned<MethodDef> {
    let mut params = vec![];
    if name == "GetAsyncEnumerator" {
        params.push(Param {
            name: "cancellationToken".into(),
            ty: Type::named("CancellationToken"),
            attributes: vec![],
            is_extension_receiver: false,
            is_ref: false,
            is_out: false,
            is_in: false,
            is_params: false,
            default: None,
        });
    }
    let fresh_args = plan
        .param_captures
        .iter()
        .map(|(_, field)| ident(field.as_str()))
        .collect();
    method(
        name,
        params,
        Some(generic_ty(ret_iface, elem_ty)),
        false,
        block(vec![dummy_stmt(Stmt::Return(Some(dummy_expr(
            Expr::New {
                ty: sm_type(plan),
                args: fresh_args,
                obj_init: None,
            },
        ))))]),
    )
}

/// 驱动体：`while (true) { if (__state == N) { …; continue; } … }` + 尾部 default。
///
/// 分派刻意不用嵌套 if-else 链与 switch：
/// - Arc 的 `break` 仅绑定循环（switch case 的语法 `break` 由 parser 消费，
///   AST 级 `Stmt::Break` 会跳出本驱动循环）；
/// - MIR 对 else 深链（else Block 内再嵌 if）的降级不完整，深支会被静默丢弃；
/// - 链式 switch lowering 对以 break 结尾的 then 体存在丢臂风险。
///
/// 故每状态一个**顶层** if，转移统一「设状态 + `continue`」——continue 在
/// MIR 语义稳定（Goto 最近循环头，即分派回边），并列支不互相穿透。
fn driver_body(blocks: &[CfgBlock]) -> Block {
    let reachable = reachable_set(blocks);
    let mut ids: Vec<usize> = (0..blocks.len())
        .filter(|id| reachable.contains(id))
        .collect();
    ids.sort_unstable();
    let mut stmts = Vec::new();
    for id in ids {
        let test = dummy_expr(Expr::Binary {
            op: BinOp::Eq,
            left: Box::new(ident(STATE)),
            right: Box::new(dummy_expr(Expr::IntLit(id as i64))),
        });
        stmts.push(dummy_stmt(Stmt::Expr(dummy_expr(Expr::If {
            cond: Box::new(test),
            then_branch: case_body(&blocks[id]),
            else_branch: None,
        }))));
    }
    // default：终结态（-1）与未知状态统一收敛为序列结束。
    stmts.push(assign(STATE, Expr::IntLit(-1)));
    stmts.push(dummy_stmt(Stmt::Return(Some(dummy_expr(Expr::BoolLit(
        false,
    ))))));
    block(vec![dummy_stmt(Stmt::While {
        cond: dummy_expr(Expr::BoolLit(true)),
        body: block(stmts),
    })])
}

/// 单个基本块 → 驱动中该状态的 then 体（语句序列 + 终结边语句）。
///
/// 非终结转移（Jump/Branch/Switch）以 `continue` 收尾回到分派头；
/// 挂起/终结以 `return` 结束本次 MoveNext 调用。
fn case_body(blk: &CfgBlock) -> Block {
    let mut stmts = blk.stmts.clone();
    match &blk.term {
        Term::Unset => {
            // 构建阶段保证所有可达块终结边已设；防御性按 Finish 处理。
            stmts.push(assign(STATE, Expr::IntLit(-1)));
            stmts.push(dummy_stmt(Stmt::Return(Some(dummy_expr(Expr::BoolLit(
                false,
            ))))));
        }
        Term::Jump(t) => {
            stmts.push(assign(STATE, Expr::IntLit(*t as i64)));
            stmts.push(dummy_stmt(Stmt::Continue));
        }
        Term::Branch(cond, t, f) => {
            // 三元选状态：避免嵌套 if（else 深链降级不完整）。
            stmts.push(dummy_stmt(Stmt::Assign {
                target: ident(STATE),
                value: dummy_expr(Expr::Ternary {
                    cond: Box::new(cond.clone()),
                    then_branch: Box::new(dummy_expr(Expr::IntLit(*t as i64))),
                    else_branch: Box::new(dummy_expr(Expr::IntLit(*f as i64))),
                }),
            }));
            stmts.push(dummy_stmt(Stmt::Continue));
        }
        Term::Switch { scrutinee, arms } => {
            stmts.push(dummy_stmt(Stmt::Expr(dummy_expr(Expr::Switch(
                SwitchExpr {
                    scrutinee: Box::new(scrutinee.clone()),
                    cases: arms.iter().map(switch_dispatch_case).collect::<Vec<_>>(),
                },
            )))));
            stmts.push(dummy_stmt(Stmt::Continue));
        }
        Term::Yield(value, resume) => {
            stmts.push(dummy_stmt(Stmt::Assign {
                target: ident(CURRENT),
                value: value.clone(),
            }));
            stmts.push(assign(STATE, Expr::IntLit(*resume as i64)));
            stmts.push(dummy_stmt(Stmt::Return(Some(dummy_expr(Expr::BoolLit(
                true,
            ))))));
        }
        Term::Finish => {
            stmts.push(assign(STATE, Expr::IntLit(-1)));
            stmts.push(dummy_stmt(Stmt::Return(Some(dummy_expr(Expr::BoolLit(
                false,
            ))))));
        }
    }
    block(stmts)
}

/// 用户 switch 的分派 case：模式/守卫原样保留，体为「设状态」——
/// 体完即出 switch 链（Arc 无 fallthrough），落链尾后由 while 头重新分派。
fn switch_dispatch_case(arm: &SwitchArmCfg) -> SwitchCase {
    SwitchCase {
        pattern: arm.pattern.clone(),
        when: arm.when.clone(),
        body: block(vec![assign(STATE, Expr::IntLit(arm.target as i64))]),
    }
}

/// 从入口块（0）沿终结边标记可达块。
fn reachable_set(blocks: &[CfgBlock]) -> std::collections::HashSet<usize> {
    use std::collections::HashSet;

    let mut reachable = HashSet::new();
    let mut work = vec![0usize];
    while let Some(id) = work.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(blk) = blocks.get(id) {
            match &blk.term {
                Term::Unset | Term::Finish => {}
                // 挂起点的恢复块是下一次 MoveNext 的入口状态，必须可达。
                Term::Yield(_, resume) => work.push(*resume),
                Term::Jump(t) => work.push(*t),
                Term::Branch(_, a, b) => {
                    work.push(*a);
                    work.push(*b);
                }
                Term::Switch { arms, .. } => {
                    for arm in arms {
                        work.push(arm.target);
                    }
                }
            }
        }
    }
    reachable
}
