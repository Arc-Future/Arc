//! RFC 044：yield 脱糖入口——程序遍历、迭代器方法分类与合成类注入。
//!
//! 挂接于 LINQ 脱糖之后（pipeline/inspect/overview/core_arc 四处调用点），
//! 先于 hir lower。产出的合成类是普通 Arc 源码级构造，下游零感知。

use ast::*;

use super::cfg::CfgBuilder;
use super::emit::{emit_state_machine, rewritten_method_body, HoistedField, IterKind, SmPlan};
use super::rename::Renamer;

/// 遍历整个程序，把含 yield 的方法脱糖为状态机类 + 实例化桩。
/// 返回精确诊断（空 = 成功）。
pub fn desugar_yield_program(program: &mut Program) -> Vec<String> {
    let mut errors = Vec::new();
    let mut counter = 0u32;
    walk_items(&mut program.items, &mut errors, &mut counter);
    errors
}

/// `errors` 全局累计；`counter` 生成全局唯一合成类名后缀。
fn walk_items(items: &mut Vec<Spanned<Item>>, errors: &mut Vec<String>, counter: &mut u32) {
    let mut injected: Vec<Spanned<Item>> = Vec::new();
    for item in items.iter_mut() {
        match &mut item.node {
            Item::Namespace(ns) => {
                walk_items(&mut ns.items, errors, counter);
            }
            Item::Class(class) => {
                for method in class.methods.iter_mut() {
                    let MethodDef { sig, body, .. } = &mut method.node;
                    let Some(body) = body.as_mut() else {
                        continue;
                    };
                    let plan_args = FnSig {
                        name: sig.name.clone(),
                        generics: &sig.generics,
                        where_clause: &sig.where_clause,
                        params: &sig.params,
                        ret: sig.ret.as_ref(),
                        is_async: sig.is_async,
                        is_static: sig.modifier == MethodModifier::Static,
                        host: Some(class.name.clone()),
                        host_generics: class.generics.iter().map(|g| g.name.clone()).collect(),
                    };
                    if let Some(sm) = transform_fn(plan_args, body, errors, counter) {
                        sig.is_async = false;
                        injected.push(Spanned::new(Item::Class(sm), item.span));
                    }
                }
            }
            Item::Fn(f) => {
                let FnDef {
                    name,
                    generics,
                    where_clause,
                    params,
                    ret,
                    body,
                    is_async,
                    ..
                } = f;
                let Some(body) = body.as_mut() else {
                    continue;
                };
                let plan_args = FnSig {
                    name: name.clone(),
                    generics,
                    where_clause,
                    params,
                    ret: ret.as_ref(),
                    is_async: *is_async,
                    is_static: true,
                    host: None,
                    host_generics: vec![],
                };
                if let Some(sm) = transform_fn(plan_args, body, errors, counter) {
                    *is_async = false;
                    injected.push(Spanned::new(Item::Class(sm), item.span));
                }
            }
            Item::Struct(_)
            | Item::Interface(_)
            | Item::Enum(_)
            | Item::Variant(_)
            | Item::Delegate(_)
            | Item::Use(_)
            | Item::Native(_) => {}
        }
    }
    items.extend(injected);
}

/// 方法签名只读视图（类方法与顶层自由函数共用转换）。
struct FnSig<'a> {
    name: Ident,
    generics: &'a [GenericParam],
    where_clause: &'a [TypeConstraint],
    params: &'a [Param],
    ret: Option<&'a Spanned<Type>>,
    is_async: bool,
    /// 方法是否 static（static 方法内 `this` 非法，由本层精确诊断）。
    is_static: bool,
    /// 宿主类名（类方法）；顶层函数为 None。
    host: Option<Ident>,
    /// 宿主类泛型形参名（RFC 044 M3：合成类泛型 = 宿主类泛型 + 方法泛型，
    /// 使泛型类内迭代器方法与 `this` 捕获均可在模板上静态命名）。
    host_generics: Vec<Ident>,
}

/// 单个函数/方法转换：返回 Some(合成类) 表示已脱糖（调用方负责改写标记与注入）。
fn transform_fn(
    sig: FnSig<'_>,
    body: &mut Block,
    errors: &mut Vec<String>,
    counter: &mut u32,
) -> Option<ClassDef> {
    if !body_has_yield(&body.stmts) {
        return None;
    }

    let kind = classify(&sig, errors)?;

    if sig
        .params
        .iter()
        .any(|p| p.is_ref || p.is_out || p.is_in || p.is_params)
    {
        errors.push(format!(
            "迭代器方法 `{}` 的参数不允许 ref/out/in/params 修饰（RFC 044）",
            sig.name
        ));
        return None;
    }

    let mut renamer = Renamer::new();
    let mut hoisted: Vec<HoistedField> = Vec::new();
    let mut captures: Vec<(Ident, Ident)> = Vec::new();
    for param in sig.params {
        let field: Ident = format!("__prm_{}", param.name).into();
        renamer.bind(&param.name, field.clone());
        captures.push((param.name.clone(), field.clone()));
        hoisted.push(HoistedField {
            name: field,
            ty: Some(param.ty.clone()),
        });
    }
    collect_locals(
        &body.stmts,
        &mut renamer,
        &mut hoisted,
        sig.name.as_str(),
        errors,
    );
    if !renamer.errors.is_empty() {
        errors.extend(renamer.errors);
        return None;
    }

    let class_name: Ident = format!("__Yield_{}_{}", sanitize(&sig.name), *counter).into();
    *counter += 1;

    let builder = CfgBuilder::new(renamer);
    let (blocks, build_errors, host_captured) = builder.build(std::mem::replace(
        body,
        Block {
            stmts: Vec::new(),
            tail: None,
        },
    ));
    if !build_errors.is_empty() {
        errors.extend(build_errors);
        return None;
    }

    // RFC 044 M2/M3：this 捕获（body 内显式 `this.X`，由 CFG 构建期登记）。
    // 注入宿主引用字段 `__host`（类型 = 宿主类 [+ 宿主类泛型实参名]，均可在
    // 合成类模板上静态命名）；static 方法 / 顶层函数内 `this` 非法，精确拒绝。
    let mut host_param: Option<Param> = None;
    if host_captured {
        if sig.host.is_none() || sig.is_static {
            errors.push(format!(
                "迭代器方法 `{}` 内 `this` 仅能在实例方法中使用（RFC 044 M2）",
                sig.name
            ));
            return None;
        }
        let host = sig.host.clone().unwrap();
        let host_ty = Spanned::new(
            Type::Named {
                path: vec![host],
                generics: sig.host_generics.iter().map(generic_type_arg).collect(),
            },
            Span::DUMMY,
        );
        hoisted.push(HoistedField {
            name: "__host".into(),
            ty: Some(host_ty.clone()),
        });
        captures.push(("__this".into(), "__host".into()));
        host_param = Some(Param {
            name: "__this".into(),
            ty: host_ty,
            attributes: vec![],
            is_extension_receiver: false,
            is_ref: false,
            is_out: false,
            is_in: false,
            is_params: false,
            default: None,
        });
    }

    // RFC 044 M3：合成类泛型 = 宿主类泛型 + 方法泛型（顺序即模板形参顺序，
    // 与重写体/Get(Async)Enumerator 的实参顺序一致；宿主类泛型由 enclosing
    // 类的单态化替换自动实例化，方法泛型由泛型方法单态化替换）。
    let mut sm_generics: Vec<GenericParam> = sig
        .host_generics
        .iter()
        .map(|n| GenericParam::new(n.clone()))
        .collect();
    sm_generics.extend(sig.generics.iter().cloned());

    let elem_ty = match &sig.ret.unwrap().node {
        Type::Named { generics, .. } => generics[0].clone(),
        _ => unreachable!("classify 已确保 Named 泛型形式"),
    };
    let plan = SmPlan {
        kind,
        class_name,
        elem_ty,
        fields: hoisted,
        ctor_params: sig.params.to_vec(),
        param_captures: captures,
        host: sig.host,
        host_captured: host_param.is_some(),
        host_param,
        generics: sm_generics,
        where_clause: sig.where_clause.to_vec(),
    };
    let sm = emit_state_machine(&plan, &blocks);
    *body = rewritten_method_body(&plan);
    Some(sm)
}

/// 泛型实参形态：`T` → `Named { path: [T], generics: [] }`。
fn generic_type_arg(name: &Ident) -> Spanned<Type> {
    Spanned::new(
        Type::Named {
            path: vec![name.clone()],
            generics: vec![],
        },
        Span::DUMMY,
    )
}

/// 迭代器类别：返回类型必须是 IEnumerable<T>/IEnumerator<T>/IAsyncEnumerable<T>。
fn classify(sig: &FnSig<'_>, errors: &mut Vec<String>) -> Option<IterKind> {
    let Some(ret) = sig.ret else {
        errors.push(format!(
            "含 yield 的方法 `{}` 必须声明 IEnumerable<T>/IEnumerator<T>/IAsyncEnumerable<T> 返回类型（RFC 044）",
            sig.name
        ));
        return None;
    };
    let Type::Named { path, generics } = &ret.node else {
        errors.push(format!(
            "含 yield 的方法 `{}` 返回类型必须是 IEnumerable<T>/IEnumerator<T>/IAsyncEnumerable<T>（RFC 044）",
            sig.name
        ));
        return None;
    };
    if generics.len() != 1 || path.len() != 1 {
        errors.push(format!(
            "含 yield 的方法 `{}` 返回类型必须是单段单实参泛型接口（RFC 044）",
            sig.name
        ));
        return None;
    }
    let kind = match path[0].as_str() {
        "IEnumerable" => IterKind::SyncEnumerable,
        "IEnumerator" => IterKind::SyncEnumerator,
        "IAsyncEnumerable" => IterKind::AsyncEnumerable,
        other => {
            errors.push(format!(
                "含 yield 的方法 `{}` 返回类型 `{other}` 非法；须为 IEnumerable<T>/IEnumerator<T>/IAsyncEnumerable<T>（RFC 044）",
                sig.name
            ));
            return None;
        }
    };
    if sig.is_async && kind != IterKind::AsyncEnumerable {
        errors.push(format!(
            "async 迭代器方法 `{}` 必须返回 IAsyncEnumerable<T>（RFC 044）",
            sig.name
        ));
        return None;
    }
    Some(kind)
}

/// 结构化语句树中是否存在 yield（含表达式位置——由 cfg 阶段精确拒绝）。
fn stmt_has_throw(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Throw { .. } => true,
        Stmt::While { body, .. } | Stmt::For { body, .. } => body_has_throw(&body.stmts),
        Stmt::Expr(e) => expr_has_throw(e),
        _ => false,
    }
}

/// RFC 044 M2：语句序列是否含 `throw`（try/finally 区域 M1 边界判定）。
pub(super) fn body_has_throw(stmts: &[Spanned<Stmt>]) -> bool {
    stmts.iter().any(|s| match &s.node {
        Stmt::Throw { .. } => true,
        Stmt::While { body, .. } | Stmt::For { body, .. } => body_has_throw(&body.stmts),
        Stmt::ForC {
            init,
            cond: _,
            inc,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_has_throw(&s.node))
                || body_has_throw(&body.stmts)
                || inc.as_ref().is_some_and(|s| stmt_has_throw(&s.node))
        }
        Stmt::Expr(e) => expr_has_throw(e),
        _ => false,
    })
}

fn expr_has_throw(e: &Spanned<Expr>) -> bool {
    match &e.node {
        Expr::Block(b) => body_has_throw(&b.stmts) || b.tail.as_deref().is_some_and(expr_has_throw),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_throw(cond)
                || body_has_throw(&then_branch.stmts)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| body_has_throw(&b.stmts))
        }
        Expr::Switch(SwitchExpr { scrutinee, cases }) => {
            expr_has_throw(scrutinee) || cases.iter().any(|c| body_has_throw(&c.body.stmts))
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => expr_has_throw(cond) || expr_has_throw(then_branch) || expr_has_throw(else_branch),
        _ => false,
    }
}

pub(super) fn body_has_yield(stmts: &[Spanned<Stmt>]) -> bool {
    stmts.iter().any(|s| stmt_has_yield(&s.node))
}

fn stmt_has_yield(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::YieldReturn { .. } | Stmt::YieldBreak => true,
        Stmt::Let { init, .. } => init.as_ref().is_some_and(expr_has_yield),
        Stmt::Expr(e) => expr_has_yield(e),
        Stmt::Assign { target, value } => expr_has_yield(target) || expr_has_yield(value),
        Stmt::Return(v) => v.as_ref().is_some_and(expr_has_yield),
        Stmt::Throw { expr } => expr_has_yield(expr),
        Stmt::While { cond, body } => expr_has_yield(cond) || body_has_yield(&body.stmts),
        Stmt::ForC {
            init,
            cond,
            inc,
            body,
        } => {
            init.as_ref().is_some_and(|s| stmt_has_yield(&s.node))
                || cond.as_ref().is_some_and(expr_has_yield)
                || inc.as_ref().is_some_and(|s| stmt_has_yield(&s.node))
                || body_has_yield(&body.stmts)
        }
        Stmt::For { iter, body, .. } => expr_has_yield(iter) || body_has_yield(&body.stmts),
        Stmt::TryCatch {
            try_body,
            catch_body,
            finally,
            ..
        } => {
            body_has_yield(&try_body.stmts)
                || body_has_yield(&catch_body.stmts)
                || finally.as_ref().is_some_and(|f| body_has_yield(&f.stmts))
        }
        Stmt::TryFinally { body, finally } => {
            body_has_yield(&body.stmts) || body_has_yield(&finally.stmts)
        }
        Stmt::Using { init, body, .. } | Stmt::AwaitUsing { init, body, .. } => {
            expr_has_yield(init) || body_has_yield(&body.stmts)
        }
        Stmt::UsingVar { init, .. } | Stmt::AwaitUsingVar { init, .. } => expr_has_yield(init),
        Stmt::Lock { expr, body } => expr_has_yield(expr) || body_has_yield(&body.stmts),
        Stmt::DeconstructAssign { value, .. } => expr_has_yield(value),
        Stmt::Break | Stmt::Continue => false,
    }
}

fn expr_has_yield(e: &Spanned<Expr>) -> bool {
    match &e.node {
        Expr::Block(b) => body_has_yield(&b.stmts) || b.tail.as_deref().is_some_and(expr_has_yield),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_yield(cond)
                || body_has_yield(&then_branch.stmts)
                || else_branch
                    .as_ref()
                    .is_some_and(|b| body_has_yield(&b.stmts))
        }
        Expr::Switch(SwitchExpr { scrutinee, cases }) => {
            expr_has_yield(scrutinee) || cases.iter().any(|c| body_has_yield(&c.body.stmts))
        }
        _ => false,
    }
}

/// 语句位置局部声明收集：`var`（无类型）拒绝，显式类型提升。
fn collect_locals(
    stmts: &[Spanned<Stmt>],
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    for stmt in stmts {
        match &stmt.node {
            Stmt::Let { name, ty, .. } => bind_local(name, ty, renamer, hoisted, fn_name, errors),
            Stmt::While { body, .. } => {
                collect_locals(&body.stmts, renamer, hoisted, fn_name, errors);
            }
            Stmt::For { var, iter, body } => {
                // RFC 044 M2：foreach 迭代变量与枚举器字段均提升（无类型，
                // 由 typeck 后置推断）；枚举器字段名 `__enum_<var>` 与 cfg 展开一致。
                bind_local(var, &None, renamer, hoisted, fn_name, errors);
                hoisted.push(HoistedField {
                    name: format!("__enum_{}", var).into(),
                    ty: None,
                });
                collect_locals_expr(iter, renamer, hoisted, fn_name, errors);
                collect_locals(&body.stmts, renamer, hoisted, fn_name, errors);
            }
            Stmt::DeconstructAssign { targets, value, .. } => {
                // RFC 044 M2：解构目标（Bind）提升为无类型字段（后置推断）。
                collect_deconstruct_targets(targets, renamer, hoisted, fn_name, errors);
                collect_locals_expr(value, renamer, hoisted, fn_name, errors);
            }
            Stmt::ForC { init, body, .. } => {
                if let Some(s) = init {
                    if let Stmt::Let { name, ty, .. } = &*s.node {
                        bind_local(name, ty, renamer, hoisted, fn_name, errors);
                    }
                }
                collect_locals(&body.stmts, renamer, hoisted, fn_name, errors);
            }
            Stmt::TryCatch {
                try_body,
                catch_body,
                finally,
                ..
            } => {
                collect_locals(&try_body.stmts, renamer, hoisted, fn_name, errors);
                collect_locals(&catch_body.stmts, renamer, hoisted, fn_name, errors);
                if let Some(f) = finally {
                    collect_locals(&f.stmts, renamer, hoisted, fn_name, errors);
                }
            }
            Stmt::TryFinally { body, finally } => {
                collect_locals(&body.stmts, renamer, hoisted, fn_name, errors);
                collect_locals(&finally.stmts, renamer, hoisted, fn_name, errors);
            }
            Stmt::Using { body, .. } | Stmt::AwaitUsing { body, .. } | Stmt::Lock { body, .. } => {
                collect_locals(&body.stmts, renamer, hoisted, fn_name, errors);
            }
            Stmt::Expr(e) => {
                collect_locals_expr(e, renamer, hoisted, fn_name, errors);
            }
            _ => {}
        }
    }
}

/// 表达式位置的 if / switch / 块语句内的声明：同样提升（yield 在 cfg 阶段拒绝）。
/// 块的尾表达式递归：`else if` 链经 parser 表示为 else 分支块 tail 的嵌套 If，
/// 仅收集 stmts 会漏掉链上更深分支的声明 → 提升缺字段 → 下游 undefined name。
fn collect_locals_expr(
    e: &Spanned<Expr>,
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    match &e.node {
        Expr::Block(b) => {
            collect_block_locals(b, renamer, hoisted, fn_name, errors);
        }
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_block_locals(then_branch, renamer, hoisted, fn_name, errors);
            if let Some(eb) = else_branch {
                collect_block_locals(eb, renamer, hoisted, fn_name, errors);
            }
        }
        Expr::Switch(SwitchExpr { cases, .. }) => {
            for c in cases {
                // RFC 044 M2：case 绑定模式（`case T n` / `case var n` / variant /
                // 位置绑定）提升为字段（类型后置推断）。
                collect_pattern_bindings(&c.pattern, renamer, hoisted, fn_name, errors);
                collect_block_locals(&c.body, renamer, hoisted, fn_name, errors);
            }
        }
        _ => {}
    }
}

/// RFC 044 M2：switch case 模式的绑定名提升（`T n` 带类型；`var n`/variant/
/// 位置绑定无类型 → typeck 后置推断）。
fn collect_pattern_bindings(
    pattern: &Option<Pattern>,
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    match pattern {
        Some(Pattern::Type {
            ty,
            binding: Some(name),
        }) => bind_local(name, &Some(ty.clone()), renamer, hoisted, fn_name, errors),
        Some(Pattern::Var(name))
        | Some(Pattern::Variant {
            binding: Some(name),
            ..
        }) => bind_local(name, &None, renamer, hoisted, fn_name, errors),
        Some(Pattern::Positional(subs)) => {
            collect_positional_bindings(subs, renamer, hoisted, fn_name, errors)
        }
        _ => {}
    }
}

fn collect_positional_bindings(
    subs: &[PositionalSubpattern],
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    for s in subs {
        match s {
            PositionalSubpattern::Var(name) => {
                bind_local(name, &None, renamer, hoisted, fn_name, errors)
            }
            PositionalSubpattern::Typed { ty, name } => {
                bind_local(name, &Some(ty.clone()), renamer, hoisted, fn_name, errors)
            }
            PositionalSubpattern::Nested(inner) => {
                collect_positional_bindings(inner, renamer, hoisted, fn_name, errors)
            }
            _ => {}
        }
    }
}

/// 块级声明收集：语句序列 + 尾表达式（嵌套 if/switch 链）。
fn collect_block_locals(
    block: &Block,
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    collect_locals(&block.stmts, renamer, hoisted, fn_name, errors);
    if let Some(tail) = &block.tail {
        collect_locals_expr(tail, renamer, hoisted, fn_name, errors);
    }
}

fn bind_local(
    name: &Ident,
    ty: &Option<Spanned<Type>>,
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    _fn_name: &str,
    _errors: &mut Vec<String>,
) {
    // RFC 044 M2：`var` 局部（ty=None）放行——提升字段类型后置推断
    // （合成类字段发射 `Type::Infer`，typeck 从状态机方法体首次赋值推断回填）。
    let field: Ident = format!("__loc_{}", name).into();
    renamer.bind(name, field.clone());
    hoisted.push(HoistedField {
        name: field,
        ty: ty.clone(),
    });
}

/// 递归收集解构目标绑定（Bind(Some) 提升；discard `_` 与嵌套递归）。
fn collect_deconstruct_targets(
    targets: &[DeconstructTarget],
    renamer: &mut Renamer,
    hoisted: &mut Vec<HoistedField>,
    fn_name: &str,
    errors: &mut Vec<String>,
) {
    for t in targets {
        match t {
            DeconstructTarget::Bind(Some(name)) => {
                bind_local(name, &None, renamer, hoisted, fn_name, errors);
            }
            DeconstructTarget::Bind(None) => {}
            DeconstructTarget::Nested(subs) => {
                collect_deconstruct_targets(subs, renamer, hoisted, fn_name, errors);
            }
        }
    }
}

/// 合成类名净化：标识符安全字符白名单。
fn sanitize(name: &Ident) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
