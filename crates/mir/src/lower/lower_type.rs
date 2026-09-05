use super::lower_linq::*;
use super::*;
// `operand_from_expr` 通过全路径 `lower_expr::operand_from_expr(...)` 调用。

/// Build an AccessContext from the LowerCtx, using ctx.owner as current_type
/// so that private field/method access is allowed.
fn access_ctx(ctx: &LowerCtx) -> AccessContext {
    AccessContext {
        current_type: ctx.owner.clone(),
        extension_scope: ExtensionScope::default(),
        enclosing_namespace: vec![],
        current_package: None,
        skip_type_visibility: false,
    }
}

/// CD-15/D4：当前 owner 类的**直接基类**（沿 `bases` 找第一个 class）。
///
/// `base` 引用（`base.Method()` / `base.Field`）的静态类型即直接基类；派生类
/// 访问 base 成员始终从直接基类出发解析（C# 语义：跳过派生覆写，命中基类实现）。
pub(super) fn direct_base_class(ctx: &LowerCtx) -> Option<Ident> {
    let owner = ctx.owner.as_ref()?;
    let class_ty = ctx.registry.get(owner)?;
    class_ty
        .bases
        .iter()
        .find(|b| ctx.registry.is_class(b))
        .cloned()
}

pub(super) fn is_class_type(ty: &TypeId, registry: &TypeRegistry) -> bool {
    match ty {
        TypeId::Named(n) => registry.is_class(n),
        _ => false,
    }
}

/// RFC 008：`Func`/`Action`（含 mangled `Func_*`/`Action_*`）委托类型。
///
/// 可空委托（`Func<...>?`/`Action<...>?` 形参与局部）与委托同形（closure
/// 指针）——解包 `Nullable` 后判别，否则裸标识符调用（`apply(ctx)`）漏路由
/// `IndirectCall`，按名直调产出未定义符号（arc-prune-001）。
pub(super) fn is_delegate_type(ty: &TypeId) -> bool {
    let inner = match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    };
    matches!(inner, TypeId::Func { .. })
        || matches!(
            inner,
            TypeId::Named(n) if n.as_str() == "Action"
                || n.as_str() == "Func"
                || n.starts_with("Func_")
                || n.starts_with("Action_")
        )
}

/// 若 `ty` 是 `delegate` 关键字声明（`public delegate int Converter(int);`）的
/// 别名，展开为 registry 记录的 `Func` 类型，使下游 `is_delegate_type` /
/// `delegate_params_of` / `delegate_return_type` 走委托语义；否则原样返回。
/// 与 typeck 委托别名字段读取的展开行为对齐（check_expr.rs / check_stmt.rs）。
fn expand_delegate_alias(ctx: &LowerCtx, ty: TypeId) -> TypeId {
    if let TypeId::Named(n) = &ty {
        if let Some(func_ty) = ctx.registry.delegate_aliases.get(n.as_str()) {
            return func_ty.clone();
        }
    }
    ty
}

pub(super) fn type_name_from_operand(op: &MirOperand, expr: &Expr, ctx: &LowerCtx) -> Ident {
    // CD-15/D4：`base` 接收者的静态类型 = 直接基类。必须**先于** `MirOperand::Local`
    // 分支——base 物化为 this 局部，其声明类型是当前派生类；方法解析须从直接基类
    // 出发（`base.Greet()` 静态分派到基类实现）。
    if matches!(expr, Expr::Base) {
        return direct_base_class(ctx).unwrap_or_else(|| "unknown".into());
    }
    match op {
        MirOperand::Local(id) => ctx
            .locals
            .get(id)
            .map(|(_, ty)| type_id_to_name(ty))
            .unwrap_or_else(|| "unknown".into()),
        // RFC 006 M4 对称性：实例字段与静态字段都以字段声明类型作为
        // 方法调用的 receiver 类型。缺 StaticField 分支时 `Stats.Fired.ToString()`
        // 的 receiver_type 下沉为 "unknown"，codegen 在 emit_call.rs 断言 panic。
        MirOperand::Field { class, field, .. } | MirOperand::StaticField { class, field } => {
            let class_ident: Ident = class.as_str().into();
            let field_ident: Ident = field.as_str().into();
            if let Some(f) = ctx.registry.field_info(&class_ident, &field_ident) {
                return f.ty.clone();
            }
            // RFC 045（di_decorate 崩溃根因）：枚举成员引用（`ServiceLifetime.Transient`）
            // 不是字段——返回枚举类型名（ctor 实参名匹配需要：形态 2 形参表
            // [Type, Func_..., ServiceLifetime]；旧实现落 "unknown" → 匹配失败 →
            // fallback 首候选（实现类型形态）→ Factory 未设置 → 解析崩溃）。
            if ctx.registry.is_enum(&class_ident) {
                return class_ident;
            }
            // 内置类型字段（`string.Length` / `Span.Length` / `Task.Result` 等）不在
            // registry.types 表；field_info 查不到时对齐 infer_type_from_expr 的内置
            // 字段推断（其 Expr::Field 分支有 String.Length→Int / Span.Length 等特判），
            // 否则 receiver_type 下沉 "unknown"，codegen 在 emit_call.rs 断言 panic
            // （`string.Length.ToString()` 复现，DeepSeekModelProvider 请求构造触发）。
            type_id_to_name(&infer_type_from_expr(expr, ctx))
        }
        MirOperand::UnboxIface { class, .. } => class.as_str().into(),
        _ => match expr {
            // RFC 016 M1：native 契约模块注册为 StaticClass（如 `libc`），
            // 需与普通 Class 一样识别为类型名，使 `libc.puts(...)` 的
            // receiver_type 解析为 "libc" 而非 "unknown"，否则 codegen
            // 的 native 符号表查找 `libc.puts` 会失败。
            Expr::Ident(name)
                if ctx.registry.is_class(name) || ctx.registry.is_static_class(name) =>
            {
                name.clone()
            }
            Expr::New { ty, .. } => match &ty.node {
                Type::Named { path, .. } => {
                    path.last().cloned().unwrap_or_else(|| "unknown".into())
                }
                _ => "unknown".into(),
            },
            // Literal receivers: method calls on literals (e.g. `" hi ".Trim()`)
            // must resolve the receiver type so codegen dispatches to the
            // correct rt_str_* function instead of `unknown_<Method>`.
            Expr::StringLit(_) => "string".into(),
            Expr::IntLit(_) => "int".into(),
            Expr::BoolLit(_) => "bool".into(),
            Expr::FloatLit(ast::FloatLitValue::Float(_)) => "float".into(),
            Expr::FloatLit(ast::FloatLitValue::Double(_)) => "double".into(),
            Expr::CharLit(_) => "char".into(),
            // RFC 045（di_decorate 崩溃根因）：`typeof(T)` 实参的类型名须为
            // "Type"（ctor 形参表 `Type service` 的 type_path_name）——旧实现落
            // `_ => "unknown"` 使 `new ServiceDescriptor(typeof(X), Func, Lifetime)`
            // 的实参名 [unknown, Func_..., ...] 匹配不到工厂形态 ctor（3 参重载）
            // → fallback 首候选（实现类型形态）→ Factory 未设置 → 解析崩溃。
            Expr::TypeOf(_) => "Type".into(),
            // RFC 045：枚举成员引用（`ServiceLifetime.Transient`）的 operand 已
            // 折叠为判别值（非 MirOperand::Field），此处按表达式形态返回枚举
            // 类型名（ctor 实参名匹配）。
            Expr::Field { receiver, .. } => {
                if let Expr::Ident(name) = &receiver.node {
                    if ctx.registry.is_enum(name) {
                        return name.clone();
                    }
                }
                "unknown".into()
            }
            _ => "unknown".into(),
        },
    }
}

fn type_id_to_name(ty: &TypeId) -> Ident {
    match ty {
        TypeId::Nullable { inner } => type_id_to_name(inner),
        // 借用包装（ref/out/in 参数、struct this）：运行时类名取被引类型，
        // 与 Nullable 同构。缺此臂时 record 脱糖 ctor 的 `this.X = x`（receiver
        // 为 Ident("this") 局部，类型 Ref{Named,kind}）被映射为 "unknown"，
        // FieldSet class 失真 → codegen field_info 兜底写偏移 16，而读链按
        // 真实 struct 布局取偏移 0 → record 位置属性值恒 0（探针 P6）。
        TypeId::Ref { inner, .. } => type_id_to_name(inner),
        TypeId::Named(n) => n.clone(),
        TypeId::Int => "int".into(),
        TypeId::Long => "long".into(),
        TypeId::Short => "short".into(),
        TypeId::Byte => "byte".into(),
        TypeId::Char => "char".into(),
        TypeId::Float => "float".into(),
        TypeId::Double => "double".into(),
        TypeId::Bool => "bool".into(),
        TypeId::UInt => "uint".into(),
        TypeId::ULong => "ulong".into(),
        TypeId::UShort => "ushort".into(),
        TypeId::SByte => "sbyte".into(),
        TypeId::String => "string".into(),
        TypeId::Void => "void".into(),
        // Task<T> 在 typeck 中是内建类型，但 MIR lower 需要将其映射为 "Task"
        // 类名，以便 class_from_expr 返回 "Task"，codegen 拦截 Task facade 调用。
        TypeId::Task { .. } => "Task".into(),
        // Expression<T> 是编译期树化类型；运行时物化为 `LambdaExpression`（ExpressionTree
        // 根节点，C# `Expression<TDelegate> : LambdaExpression` 对齐；RFC 022 Sprint 2d）。
        // 若不映射，`expr.Body` 的 FieldGet class 会退回 "Expression"，codegen 读错偏移。
        // 与 typeck member 解析（check_expr.rs LambdaExpression 定向）保持一致。
        TypeId::Expression { .. } => "LambdaExpression".into(),
        TypeId::Object => "object".into(),
        // RFC 045（di_decorate 崩溃根因）：Func/Action 委托 → 单态名
        // （`Func_IServiceProvider_object`）——`type_name_from_operand` 供 ctor
        // 重载消歧（resolve_ctor_params）按实参类型名匹配形参表；旧实现落
        // `_ => "unknown"` 使 `new ServiceDescriptor(Type, Func, Lifetime)`
        // 的实参名 [Type, unknown, Lifetime] 匹配不到工厂形态 ctor → fallback
        // 首候选（实现类型形态）→ Factory 未设置 → 解析路径崩溃。
        TypeId::Func { .. } => type_id_to_field_name(ty),
        // Builtin `IEnumerable<T>` / `IQueryable<T>` → std 单态名（方法分派 / itable）。
        TypeId::IEnumerable { .. } | TypeId::IQueryable { .. } => type_id_to_field_name(ty),
        // RFC 005：Span Length / 索引路径依赖类名分流。
        TypeId::Span { mutable: true, .. } => "Span".into(),
        TypeId::Span { mutable: false, .. } => "ReadOnlySpan".into(),
        // RFC 004 刀 2 收尾：泛型参数类型 → 参数名。`T t = new T(); t.Create()`
        // 的 receiver_type 此前烘死 "unknown"；单态化克隆把 `T → IntFactory`
        // 文本替换后即解析为具体类，codegen 正常分派。约束检查（check_constraints
        // 于调用点验证）确保合法约束才编译——非法接口约束被拒，防静默误编译。
        TypeId::Generic(n) => n.clone(),
        // 类型推断失败哨兵：显式映射为 "unknown" 以命中 codegen emit_call.rs 的
        // 「method call on unresolved receiver type "unknown"」panic，让索引器等
        // 无法推断的接收者类型在 codegen 早期清晰失败，而非静默退化为 i32。
        TypeId::Error => "unknown".into(),
        // 数组 / SIMD 向量：与 Func、IEnumerable/IQueryable 同构走单态 mangle
        // 名（`{elem}_arr` / `Vector_{elem}_{n}`）——registry 签名表即以该名
        // 流转（TypeId::enumerable_elem 的逆向解码一致）。
        TypeId::Array { .. } | TypeId::Vector { .. } => type_id_to_field_name(ty),
        // 推断哨兵与 Error 同语义：显式 "unknown" 命中 codegen 对未解析
        // receiver 的清晰失败，禁止静默退化。本函数不设通配臂——新增
        // TypeId 变体必须显式决定运行时类名（exhaustive match 治理）。
        TypeId::Infer => "unknown".into(),
    }
}

pub(super) fn enum_variant_operand(expr: &Expr, registry: &TypeRegistry) -> Option<MirOperand> {
    if let Expr::Field { receiver, field } = expr {
        if let Expr::Ident(enum_name) = &receiver.node {
            if let Some(v) = registry.enum_variant(enum_name, field) {
                return Some(MirOperand::ConstInt(v.discriminant as i64));
            }
        }
    }
    None
}

/// RFC 004 M1/M2：检测 variant 构造表达式，返回 VariantConstruct rvalue。
/// M2 扩展：struct payload 通过 prep 语句先物化到临时 local；
/// 简单 payload（int/string/bool/null）仍走 operand_from_expr 路径。
///
/// 支持两种形式：
/// - 无 payload：`Value.Null` → `Expr::Field { receiver: Expr::Ident("Value"), field: "Null" }`
/// - 有 payload：`Value.Int(42)` → `Expr::MethodCall { receiver: Expr::Ident("Value"), method: "Int", args: [42] }`
///   （Parser 将 `Type.Case(payload)` 解析为 MethodCall，而非 Call+Field）
///
/// 返回 `Some((Vec<MirStatement>, MirRvalue::VariantConstruct))` 当表达式是 variant 构造；否则 None。
pub(super) fn variant_construct_rvalue_with_prep(
    expr: &Expr,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> Option<(Vec<MirStatement>, MirRvalue)> {
    // 辅助：lower payload arg，处理 Expr::New（struct 需要栈分配）
    // 返回 (prep_statements, payload_operand)
    // payload 可能是任意复杂表达式（如 `b.ToHex()` MethodCall），
    // 须经 lower_arg_operand 物化到临时 local，禁止 operand_from_expr
    // （其对 MethodCall/Call 等复杂表达式会 panic）。
    let lower_payload = |arg: &Expr,
                         builder: &mut MirBuilder,
                         ctx: &mut LowerCtx|
     -> (Vec<MirStatement>, MirOperand) {
        if let Expr::New { ty, .. } = arg {
            // Struct payload: 需要栈分配结构体，物化到临时 local
            let (mut prep, rv) =
                crate::lower::lower_expr::lower_expr_to_rvalue_with_binary(arg, builder, ctx);
            let payload_name = lower_type_name(&ty.node);
            let tmp = builder.fresh_local(
                &"_variant_struct".into(),
                TypeId::Named(payload_name.to_string().into()),
                ctx.locals,
            );
            prep.push(MirStatement::Assign {
                place: tmp,
                rvalue: rv,
            });
            (prep, MirOperand::Local(tmp))
        } else {
            super::lower_call::lower_arg_operand(builder, arg, ctx)
        }
    };

    match expr {
        // 无 payload case：`Value.Null` / `Option<int>.None`
        Expr::Field { receiver, field } => {
            let (variant_name, type_args): (Option<&Ident>, &[Spanned<ast::Type>]) =
                extract_generic_receiver(receiver);
            if let Some(vn) = variant_name {
                let resolved_name = if type_args.is_empty() {
                    vn.to_string()
                } else {
                    let arg_tys: Vec<TypeId> =
                        type_args.iter().map(|t| lower_type_name(&t.node)).collect();
                    typeck::mangle_generic(vn.as_str(), &arg_tys)
                };
                if ctx.registry.is_variant(&resolved_name.as_str().into()) {
                    if let Some(case_info) = ctx
                        .registry
                        .variant_case(&resolved_name.as_str().into(), field)
                    {
                        if case_info.payload.is_none() {
                            return Some((
                                vec![],
                                MirRvalue::VariantConstruct {
                                    variant_name: resolved_name,
                                    case_name: field.to_string(),
                                    payload: None,
                                },
                            ));
                        }
                    }
                }
            }
            None
        }
        // 有 payload case：`Value.Int(42)`（Parser 产物：MethodCall）
        // RFC 004 M2：通用 variant receiver 检测（含泛型 `Option<int>.Some(42)`）
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // 提取 variant 名与泛型实参
            let (variant_name, type_args): (Option<&Ident>, &[Spanned<ast::Type>]) =
                extract_generic_receiver(receiver);
            if let Some(vn) = variant_name {
                // 泛型 variant：构造 mangle 名（如 `Option_int`）
                let resolved_name = if type_args.is_empty() {
                    vn.to_string()
                } else {
                    let arg_tys: Vec<TypeId> =
                        type_args.iter().map(|t| lower_type_name(&t.node)).collect();
                    typeck::mangle_generic(vn.as_str(), &arg_tys)
                };
                if ctx.registry.is_variant(&resolved_name.as_str().into()) {
                    if let Some(case_info) = ctx
                        .registry
                        .variant_case(&resolved_name.as_str().into(), method)
                    {
                        if case_info.payload.is_some() && args.len() == 1 {
                            let (prep, payload_op) = lower_payload(&args[0].node, builder, ctx);
                            return Some((
                                prep,
                                MirRvalue::VariantConstruct {
                                    variant_name: resolved_name,
                                    case_name: method.to_string(),
                                    payload: Some(payload_op),
                                },
                            ));
                        }
                    }
                }
            }
            None
        }
        // 兼容旧路径：Expr::Call + Expr::Field（部分内联构造场景）
        Expr::Call { func, args, .. } => {
            if let Expr::Field { receiver, field } = &func.node {
                if let Expr::Ident(variant_name) = &receiver.node {
                    if ctx.registry.is_variant(variant_name) {
                        if let Some(case_info) = ctx.registry.variant_case(variant_name, field) {
                            if case_info.payload.is_some() && args.len() == 1 {
                                let (prep, payload_op) = lower_payload(&args[0].node, builder, ctx);
                                return Some((
                                    prep,
                                    MirRvalue::VariantConstruct {
                                        variant_name: variant_name.to_string(),
                                        case_name: field.to_string(),
                                        payload: Some(payload_op),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// RFC 004 M2：从 receiver 表达式中提取 variant 名与泛型类型实参。
///
/// 支持两种形式：
/// - 无泛型：`Expr::Ident("Value")` → `("Value", [])`
/// - 泛型限定：`Expr::Call { func: Ident("Option"), args: [], type_args: [int] }`
///   → `("Option", [int])`（parser 为 `Option<int>.Some(42)` 产生的 AST）
fn extract_generic_receiver(
    receiver: &ast::Spanned<Expr>,
) -> (Option<&Ident>, &[Spanned<ast::Type>]) {
    match &receiver.node {
        Expr::Ident(name) => (Some(name), &[]),
        Expr::Call {
            func,
            args,
            type_args,
            params_span: _,
        } if args.is_empty() => match &func.node {
            Expr::Ident(name) => (Some(name), type_args.as_slice()),
            _ => (None, &[]),
        },
        _ => (None, &[]),
    }
}

/// 判断表达式是否足够简单，可被 `operand_from_expr` 直接处理（无需 prep 语句）。
/// 用于 `variant_construct_rvalue` 的 payload 检测：复杂 payload（MethodCall/Call/
/// Binary/New 等）须由 `variant_construct_rvalue_with_prep` 经 `lower_arg_operand`
/// 物化到临时 local，禁止 `operand_from_expr` panic。
fn is_simple_operand_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::BoolLit(_)
            | Expr::StringLit(_)
            | Expr::CharLit(_)
            | Expr::Ident(_)
            | Expr::Path(_)
            | Expr::This
            | Expr::Null
            | Expr::Field { .. }
            | Expr::Cast { .. }
    )
}

/// RFC 004 M1 兼容检测：仅检测是否为 variant 构造（不生成 prep 语句）。
/// 用于 `operand_from_expr` 安全网和 `lower_arg_operand` 的提前拦截检测。
///
/// 有 payload 的 case：仅在 payload 为简单表达式（`is_simple_operand_expr`）时
/// 返回 `Some`；复杂 payload 返回 `None`，交由 `variant_construct_rvalue_with_prep`
/// 经 `lower_arg_operand` 物化处理，避免 `operand_from_expr` 对 MethodCall/Call
/// 等复杂表达式 panic。
pub(super) fn variant_construct_rvalue(expr: &Expr, ctx: &LowerCtx) -> Option<MirRvalue> {
    match expr {
        Expr::Field { receiver, field } => {
            let (variant_name, type_args): (Option<&Ident>, &[Spanned<ast::Type>]) =
                extract_generic_receiver(receiver);
            if let Some(vn) = variant_name {
                let resolved_name = if type_args.is_empty() {
                    vn.to_string()
                } else {
                    let arg_tys: Vec<TypeId> =
                        type_args.iter().map(|t| lower_type_name(&t.node)).collect();
                    typeck::mangle_generic(vn.as_str(), &arg_tys)
                };
                if ctx.registry.is_variant(&resolved_name.as_str().into()) {
                    if let Some(case_info) = ctx
                        .registry
                        .variant_case(&resolved_name.as_str().into(), field)
                    {
                        if case_info.payload.is_none() {
                            return Some(MirRvalue::VariantConstruct {
                                variant_name: resolved_name,
                                case_name: field.to_string(),
                                payload: None,
                            });
                        }
                    }
                }
            }
            None
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let (variant_name, type_args): (Option<&Ident>, &[Spanned<ast::Type>]) =
                extract_generic_receiver(receiver);
            if let Some(vn) = variant_name {
                let resolved_name = if type_args.is_empty() {
                    vn.to_string()
                } else {
                    let arg_tys: Vec<TypeId> =
                        type_args.iter().map(|t| lower_type_name(&t.node)).collect();
                    typeck::mangle_generic(vn.as_str(), &arg_tys)
                };
                if ctx.registry.is_variant(&resolved_name.as_str().into()) {
                    if let Some(case_info) = ctx
                        .registry
                        .variant_case(&resolved_name.as_str().into(), method)
                    {
                        if case_info.payload.is_some() && args.len() == 1 {
                            // 复杂 payload（MethodCall/Call 等）须由 with_prep 版本处理。
                            if !is_simple_operand_expr(&args[0].node) {
                                return None;
                            }
                            let payload_op = lower_expr::operand_from_expr(&args[0].node, ctx);
                            return Some(MirRvalue::VariantConstruct {
                                variant_name: resolved_name,
                                case_name: method.to_string(),
                                payload: Some(payload_op),
                            });
                        }
                    }
                }
            }
            None
        }
        Expr::Call { func, args, .. } => {
            if let Expr::Field { receiver, field } = &func.node {
                if let Expr::Ident(variant_name) = &receiver.node {
                    if ctx.registry.is_variant(variant_name) {
                        if let Some(case_info) = ctx.registry.variant_case(variant_name, field) {
                            if case_info.payload.is_some() && args.len() == 1 {
                                // 复杂 payload（MethodCall/Call 等）须由 with_prep 版本处理。
                                if !is_simple_operand_expr(&args[0].node) {
                                    return None;
                                }
                                let payload_op = lower_expr::operand_from_expr(&args[0].node, ctx);
                                return Some(MirRvalue::VariantConstruct {
                                    variant_name: variant_name.to_string(),
                                    case_name: field.to_string(),
                                    payload: Some(payload_op),
                                });
                            }
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// 检测 variant 构造表达式（`Content.None` 无 payload / `Content.Element(x)` 有
/// payload / `Option<int>.Some(42)` 泛型）并返回解析后的 variant 类型名（含泛型
/// mangle，如 `Option_int`）。非 variant 构造返回 None。
///
/// 供 `infer_type_from_expr` 复用 `variant_construct_rvalue` 的判别逻辑，避免无
/// payload case 的 variant 构造被误推断为 Int（ptrtoint 截断 64 位指针 → 崩溃）。
fn variant_construct_type_name(expr: &Expr, ctx: &LowerCtx) -> Option<Ident> {
    let (receiver, case_name) = match expr {
        Expr::Field { receiver, field } => (receiver, Some(field)),
        Expr::MethodCall {
            receiver, method, ..
        } => (receiver, Some(method)),
        _ => return None,
    };
    let (variant_name, type_args) = extract_generic_receiver(receiver);
    let vn = variant_name?;
    let resolved = if type_args.is_empty() {
        vn.to_string()
    } else {
        let arg_tys: Vec<TypeId> = type_args.iter().map(|t| lower_type_name(&t.node)).collect();
        typeck::mangle_generic(vn.as_str(), &arg_tys)
    };
    if !ctx.registry.is_variant(&resolved.as_str().into()) {
        return None;
    }
    let name = case_name?;
    if ctx
        .registry
        .variant_case(&resolved.as_str().into(), name)
        .is_some()
    {
        Some(resolved.into())
    } else {
        None
    }
}

pub(super) fn class_from_expr(expr: &Expr, ctx: &LowerCtx) -> String {
    match expr {
        Expr::Ident(name) => {
            if ctx.registry.is_enum(name) {
                return name.to_string();
            }
            // Static class / native module name (e.g. rt_resources, Console, File).
            // 必须同时覆盖 StaticClass（如 QIFReporting、native 模块）：
            // `QIFReporting.StatusLabel(x)` 若被 parser 解析为
            // `Call { func: Field { receiver: Ident("QIFReporting"), ... } }`，
            // `infer_type_from_expr` 的 Expr::Call 分支通过 `class_from_expr`
            // 解析 receiver 类名；缺 is_static_class 会导致返回 "unknown"，
            // resolve_method 失败，返回类型误判为 Int → ptrtoint 截断。
            // RFC 006 V4：struct（值类型）同样须识别——`P.Z`（P 为 struct）静态字段
            // 读取在 `operand_from_expr` 的 Expr::Field 分支经 `class_from_expr` 取
            // receiver 类名后再 `is_static_field_of` 判定；缺 is_struct 会返回
            // "unknown" → 判定失败 → 回退实例 Field 访问 → `operand_from_expr(Ident(P))`
            // 解析失败 → `unresolved ident P` panic。
            if ctx.registry.is_class(name)
                || ctx.registry.is_static_class(name)
                || ctx.registry.is_struct(name)
            {
                return name.to_string();
            }
            if let Some(id) = ctx.lookup(name) {
                if let Some((_, ty)) = ctx.locals.get(&id) {
                    let r = type_id_to_name(ty).to_string();
                    return r;
                }
            }
            // 隐式 `this.field`：方法体内裸字段名访问。与 infer_type_from_expr
            // 的 Expr::Ident 分支对齐，从 owner 类解析字段声明类型，使
            // `Columns.Count`（Columns 为 this.Columns）在 FindKey 内能识别为
            // List_ColumnMap 而非 "unknown"，从而触发属性→getter 翻译。
            if ctx.is_class_field(name) {
                if let Some(owner) = &ctx.owner {
                    let access = access_ctx(ctx);
                    if let Ok(fty) = ctx.registry.resolve_field(owner, name, &access) {
                        return fty.to_string();
                    }
                    let getter: Ident = format!("get_{name}").into();
                    if let Ok(sig) = ctx.registry.resolve_method(owner, &getter, &access) {
                        return sig.ret.to_string();
                    }
                }
            }
            "unknown".into()
        }
        Expr::This => ctx
            .owner
            .as_ref()
            .map(|o| o.to_string())
            .unwrap_or_else(|| "unknown".into()),
        // CD-15/D4：`base` 引用的类名 = 直接基类（`base.Field` / `base.Prop` 的
        // 字段/属性解析从基类出发，与 typeck `Expr::Base` 静态类型一致）。
        Expr::Base => direct_base_class(ctx)
            .map(|b| b.to_string())
            .unwrap_or_else(|| "unknown".into()),
        // 解析字段声明类型而非递归 receiver 的类名。例如 `userMap.Columns.Count`
        // 中 `Columns` 字段类型为 `List_ColumnMap`，`is_custom_accessor_property`
        // 需在 `List_ColumnMap` 上查找 `get_Count`，而非在 `EntityMap_User` 上查找
        // （后者会失败，回退到直接字段访问，绕过 rt_list_size 拦截）。
        Expr::Field { receiver, field } => {
            // Task facade：`TypeId::Task` 是内建类型，不在 registry。
            // `t.Exception.Message` 若此处回退 `"unknown"`，codegen `field_info`
            // 默认 `(16,"int")` → `load i32` 当 string → clang/0xC0000005。
            // 与 `infer_type_from_expr` Field 分支对齐。
            let recv_ty = infer_type_from_spanned(receiver, ctx);
            if let TypeId::Task { inner } = &recv_ty {
                return match field.as_str() {
                    "Status" => "TaskStatus".into(),
                    "Result" => type_id_to_name(inner).to_string(),
                    "IsCompleted" | "IsCanceled" | "IsFaulted" => "bool".into(),
                    "Exception" => "Exception".into(),
                    _ => "unknown".into(),
                };
            }
            if let TypeId::Named(n) = &recv_ty {
                match n.as_str() {
                    "CancellationTokenSource" => {
                        return match field.as_str() {
                            "Token" => "CancellationToken".into(),
                            "IsCancellationRequested" => "bool".into(),
                            _ => "unknown".into(),
                        };
                    }
                    "CancellationToken" => {
                        return match field.as_str() {
                            "IsCancellationRequested" => "bool".into(),
                            _ => "unknown".into(),
                        };
                    }
                    _ => {}
                }
            }
            let recv_class = class_from_expr(&receiver.node, ctx);
            let recv_ident: Ident = recv_class.as_str().into();
            let access = access_ctx(ctx);
            if let Ok(fty) = ctx.registry.resolve_field(&recv_ident, field, &access) {
                fty.to_string()
            } else {
                // 属性 getter 返回类型兜底（与 infer_type_from_expr 的 Expr::Field 分支对齐）。
                let getter: Ident = format!("get_{field}").into();
                ctx.registry
                    .resolve_method(&recv_ident, &getter, &access)
                    .map(|sig| sig.ret.to_string())
                    .unwrap_or_else(|_| "unknown".into())
            }
        }
        Expr::StringLit(_) => "string".into(),
        // RFC 018 M2 step 4: typeof(T) 类型为公共基类 `Type`（与 typeck 对齐）。
        // 使 `typeof(T).TypeId` / `.Name` 等属性访问能被 is_custom_accessor_property
        // 正确识别为 custom accessor（get_TypeId 在 Type 上抽象存在），走
        // MethodCall 路径触发 codegen try_emit_runtime_type_getter 拦截器。
        Expr::TypeOf(_) => "Type".into(),
        // RFC 018 M3+：`list[i].Name` 等索引接收者须解析为元素类型类名，
        // 否则 is_custom_accessor_property("unknown", …) 失败，抽象属性被降为
        // FieldGet（读到错误偏移的 i32，与 string 比较触发 LLVM 类型错误）。
        Expr::Index { receiver, index } => {
            type_id_to_name(&infer_index_elem_type(receiver, &index.node, ctx)).to_string()
        }
        // 索引器常脱糖为 `get_Item` MethodCall——同样需要元素/返回类型类名。
        Expr::MethodCall {
            receiver,
            method,
            type_args,
            ..
        } => {
            let recv_class = class_from_expr(&receiver.node, ctx);
            let recv_ident: Ident = recv_class.as_str().into();
            let access = access_ctx(ctx);
            let type_arg_names: Vec<Ident> = type_args
                .iter()
                .map(|t| type_id_name(&lower_type_name(&t.node)))
                .collect();
            ctx.registry
                .resolve_method(&recv_ident, method, &access)
                .map(|sig| {
                    if !type_arg_names.is_empty() && !sig.generics.is_empty() {
                        typeck::registry::substitute_generic_in_ty_name(
                            &sig.ret,
                            &sig.generics,
                            &type_arg_names,
                        )
                    } else {
                        sig.ret.to_string()
                    }
                })
                .unwrap_or_else(|_| "unknown".into())
        }
        // 泛型类型限定成员访问的 receiver：`Holder<Thing>.Cache` /
        // `Holder<Thing>.Get(...)`。typeck 已单态化并注册 mangle 名
        // （`Holder_Thing`）；此处按 type_args 构造 mangle 名解析 receiver
        // 类名，否则回退 "unknown" 导致静态字段/方法解析失败（ORM 热路径
        // 「泛型字段 mono」缺口）。
        Expr::Call {
            func,
            args: ca,
            type_args,
            ..
        } => {
            if !ca.is_empty() || type_args.is_empty() {
                return "unknown".into();
            }
            if let Expr::Ident(name) = &func.node {
                let mangled = resolve_instantiated_type_name_from_args(name, type_args);
                let mangled_ident: Ident = mangled.as_str().into();
                if ctx.registry.is_class(&mangled_ident)
                    || ctx.registry.is_static_class(&mangled_ident)
                    || ctx.registry.is_struct(&mangled_ident)
                {
                    return mangled;
                }
            }
            "unknown".into()
        }
        _ => "unknown".into(),
    }
}

pub(super) fn resolve_instantiated_type_name_from_args(
    name: &Ident,
    type_args: &[Spanned<Type>],
) -> String {
    typeck::resolve_instantiated_type_name(&Type::Named {
        path: vec![name.clone()],
        generics: type_args.to_vec(),
    })
    .unwrap_or_else(|| name.to_string())
}

pub(super) fn default_operand_for_type(ty: &TypeId) -> MirOperand {
    match ty {
        TypeId::Int
        | TypeId::Long
        | TypeId::Short
        | TypeId::Byte
        | TypeId::Char
        | TypeId::UInt
        | TypeId::ULong
        | TypeId::UShort
        | TypeId::SByte => MirOperand::ConstInt(0),
        TypeId::Float | TypeId::Double => MirOperand::ConstFloat(0.0),
        TypeId::Bool => MirOperand::ConstBool(false),
        // RFC 012 S6 A1（struct default）：Named 类型统一携带类型名，codegen 按
        // layouts.structs 分派——struct → 栈 zeroinit 存储地址（struct 值 =
        // ptr 指向存储，null 不是合法 struct 值，作 this 传入方法即解引用崩溃）；
        // class/接口/泛型实例 → null（与旧行为一致）。
        TypeId::Named(n) => MirOperand::ConstDefault {
            type_name: n.to_string(),
        },
        _ => MirOperand::ConstNull,
    }
}

/// RFC 009 D3：若 `arr` 的元素类型为 `[SoA]` struct，返回该 struct 名；否则 `None`。
pub(super) fn soa_array_elem_struct(arr: &Spanned<Expr>, ctx: &LowerCtx) -> Option<String> {
    let elem = index_elem_type_non_indexer(arr, ctx);
    match elem {
        TypeId::Named(name) => {
            if ctx
                .layouts
                .structs
                .get(name.as_str())
                .is_some_and(|s| s.soa)
            {
                Some(name.to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// RFC 009 D3：SoA struct 的字段索引（`StructLayout.fields` 声明顺序）与字段类型。
///
/// 供 SoA 字段写融合使用：`rt_soa_field_ptr(arr, field_idx)` 按字段索引取
/// 字段数组首指针，`IndexSet` 按字段类型 GEP + store。调用方须先经
/// [`soa_array_elem_struct`] 确认 struct 为 `[SoA]`，否则 panic 属内部错误。
pub(super) fn soa_field_idx_ty(
    ctx: &LowerCtx,
    struct_name: &str,
    field: &Ident,
) -> (usize, TypeId) {
    let layout =
        ctx.layouts.structs.get(struct_name).unwrap_or_else(|| {
            panic!("MIR lower: SoA struct `{struct_name}` missing from layouts")
        });
    let (idx, f) = layout
        .fields
        .iter()
        .enumerate()
        .find(|(_, f)| f.name == *field)
        .unwrap_or_else(|| panic!("MIR lower: SoA struct `{struct_name}` has no field `{field}`"));
    let ty = type_name_to_type_id(&f.ty);
    (idx, ty)
}

/// RFC 007：C# 索引器 `obj[i]` 消解结果（读/写方法名 + 元素类型）。
///
/// `set` 为 `Some("set_Item")` 当且仅当接收者同时声明 `set_Item(index, elem)`
/// （读写索引器）；只读索引器（`IReadOnlyList`/`IReadOnlyDictionary` 等仅有
/// `get_Item`）为 `None`，写路径据此在编译期拒绝，而非发射不存在的符号。
pub(super) struct IndexerResolution {
    pub get: &'static str,
    pub set: Option<&'static str>,
    pub elem: TypeId,
}

/// RFC 007 权威路径：C# 索引器消解的唯一入口。
///
/// 显式推导索引实参类型，经 [`TypeRegistry::resolve_method_overload`] 按实参
/// 命中 `get_Item(index_ty)` 重载。**禁止**旧 `resolve_method`（空实参 + "first
/// overload" 回退）——那是 `[OVL] fail args=[]` 噪声与返回类型误判（NativePtr/
/// Int）的根源：空实参永不匹配 `get_Item(int)`，仅靠回退取「第一个重载」，一旦
/// 首个重载非索引重载或返回类型未单态化即错。
///
/// 命中则返回 `get`/`set` 方法名与元素类型；未命中返回 `None`（走数组 GEP）。
pub(super) fn resolve_indexer(
    recv_class: &str,
    index: &Expr,
    ctx: &LowerCtx,
) -> Option<IndexerResolution> {
    // RFC 005：`Span` / `ReadOnlySpan` 虽有契约面 `this[]`，索引必须走 `IndexGet`/
    // `IndexSet` → codegen 胖指针 GEP（禁止落到空 stub `Span_get_Item`）。
    if recv_class == "Span" || recv_class == "ReadOnlySpan" {
        return None;
    }
    let recv_ident: Ident = recv_class.into();
    let index_ty = type_id_to_name(&infer_type_from_expr(index, ctx));
    let get_item: Ident = "get_Item".into();
    let (_, sig) = ctx
        .registry
        .resolve_method_overload(
            &recv_ident,
            &get_item,
            std::slice::from_ref(&index_ty),
            &access_ctx(ctx),
        )
        .ok()?;
    let elem = type_name_to_type_id(&sig.ret);
    // 只读索引器二次校验：`get_Item` 命中不保证 `set_Item` 存在（如
    // `IReadOnlyList`/`IReadOnlyDictionary` 仅声明 getter）。写路径必须据此在
    // 编译期拒绝，禁止发射不存在的 `set_Item` 符号直到链接期才失败。
    // `resolve_method_overload` 对「方法不存在」经 `collect_method_overloads` 的
    // `UnknownMethod` 静默返回 Err（无 `[OVL] fail` 噪声），仅当签名真存在但
    // 实参不匹配才打印噪声——此处实参由 get_Item 返回类型推导，理应精确匹配。
    let set_item: Ident = "set_Item".into();
    let set_exists = ctx
        .registry
        .resolve_method_overload(
            &recv_ident,
            &set_item,
            &[index_ty, type_id_to_name(&elem)],
            &access_ctx(ctx),
        )
        .is_ok();
    Some(IndexerResolution {
        get: "get_Item",
        set: set_exists.then_some("set_Item"),
        elem,
    })
}

/// 非索引器（内建 `string` / `Span` / 原生数组 / `List_*` mangle 名）的元素类型。
pub(super) fn index_elem_type_non_indexer(receiver: &Spanned<Expr>, ctx: &LowerCtx) -> TypeId {
    let recv_ty = infer_type_from_spanned(receiver, ctx);
    // Builtin `string` 索引 → char（与 Length 同为 UTF-8 码元）。
    if recv_ty == TypeId::String {
        return TypeId::Char;
    }
    // RFC 005：Span / ReadOnlySpan 索引 → 元素类型。
    if let TypeId::Span { elem, .. } = recv_ty {
        return *elem;
    }
    // 无法推断接收者的可枚举元素类型（既非 string/Span，也非 IEnumerable/Array/
    // List_*/arr mangle，且 `resolve_indexer` 未命中 C# 索引器）→ 失败哨兵，
    // 禁止静默兜底 Int：否则 codegen 把 ptr 元素按 i32 发射 `ptrtoint` 截断，
    // 直到 LLVM 才崩。经 `type_id_to_name(Error) → "unknown"` 命中 codegen
    // emit_call.rs 的 panic 哨兵，在 receiver 路径提前清晰暴露。
    recv_ty.enumerable_elem().unwrap_or(TypeId::Error)
}

/// 索引读的元素类型：优先 C# 索引器返回类型（`resolve_indexer`），否则内建/数组。
pub(super) fn infer_index_elem_type(
    receiver: &Spanned<Expr>,
    index: &Expr,
    ctx: &LowerCtx,
) -> TypeId {
    let recv_class = class_from_expr(&receiver.node, ctx);
    if let Some(ix) = resolve_indexer(&recv_class, index, ctx) {
        return ix.elem;
    }
    index_elem_type_non_indexer(receiver, ctx)
}

/// 与 typeck `numeric_promote` 对齐的 MIR 算术结果类型推断。
fn mir_numeric_promote(left: &TypeId, right: &TypeId) -> TypeId {
    if *left == TypeId::Double || *right == TypeId::Double {
        TypeId::Double
    } else if *left == TypeId::Float || *right == TypeId::Float {
        TypeId::Float
    } else if *left == TypeId::Long || *right == TypeId::Long {
        TypeId::Long
    } else if *left == TypeId::ULong || *right == TypeId::ULong {
        TypeId::ULong
    } else {
        TypeId::Int
    }
}

fn type_name_to_type_id(name: &Ident) -> TypeId {
    match name.as_str() {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "string" => TypeId::String,
        "uint" => TypeId::UInt,
        "ulong" => TypeId::ULong,
        "ushort" => TypeId::UShort,
        "sbyte" => TypeId::SByte,
        "void" => TypeId::Void,
        other => {
            // 泛型 `Task<T>` 名字符串（如 registry 静态方法
            // `File.WriteAllTextAsync` 的 sig.ret → "Task<bool>"）须还原为
            // `TypeId::Task { inner }`，而非 Named("Task<bool>")→ptr。否则
            // `return await`/`await` 的结果局部被推断为 ptr 类型，await 提取走
            // `rt_task_result_ptr` 而非值类型对应的 `rt_task_result_int`，
            // 值类型（bool/int/long/enum…）Task 结果错位返回默认值（B3 根因）。
            if let Some(task) = parse_task_generic_name(other) {
                task
            } else {
                TypeId::Named(other.into())
            }
        }
    }
}

/// 把 `Task<X>` 或单态名 `Task_X` 泛型名字符串解析为
/// `TypeId::Task { inner: X }`。仅处理表示 Task 泛型实例化的命名串；`X` 经
/// `type_name_to_type_id` 解析（基元直接命中；嵌套泛型/类名退化为 Named——对
/// class 元素本就映射 ptr）。
fn parse_task_generic_name(name: &str) -> Option<TypeId> {
    let s = name.trim();
    let inner = if s.starts_with("Task<") && s.ends_with('>') {
        Some(&s[5..s.len() - 1])
    } else {
        s.strip_prefix("Task_")
    };
    inner.map(|i| TypeId::Task {
        inner: Box::new(type_name_to_type_id(&i.into())),
    })
}

pub(super) fn type_name_from_expr(expr: &Expr, ctx: &LowerCtx) -> Ident {
    match expr {
        Expr::Ident(name) => {
            if let Some(id) = ctx.lookup(name) {
                if let Some((_, ty)) = ctx.locals.get(&id) {
                    // RFC 018 M2 step 3: Nullable<T> 归约为 inner 类型名
                    // （`RuntimeType?` → "RuntimeType"），与 typeck
                    // `type_name_of` / `registry::type_path_name` 行为一致。
                    // 否则 resolve_field/resolve_method 用变量名 "rt" 查找失败，
                    // fallback 到 TypeId::Int 导致指针被截断为 i32。
                    let effective_ty = match ty {
                        TypeId::Nullable { inner } => inner.as_ref(),
                        other => other,
                    };
                    if let TypeId::Named(n) = effective_ty {
                        return n.clone();
                    }
                    if matches!(effective_ty, TypeId::String) {
                        return "string".into();
                    }
                }
            }
            // 裸实例字段（`_finalReply.Text`）：Ident 不是局部时，不可把字段名
            // 当类型名。否则 resolve_field("_finalReply","Text") 失败 →
            // fallback Int → ptrtoint → `icmp eq i32, null`（tip baa92700 红）。
            if let Some(owner) = &ctx.owner {
                let access = access_ctx(ctx);
                if let Ok(fty) = ctx.registry.resolve_field(owner, name, &access) {
                    return fty;
                }
                let getter: Ident = format!("get_{name}").into();
                if let Ok(sig) = ctx.registry.resolve_method(owner, &getter, &access) {
                    return sig.ret.clone();
                }
            }
            name.clone()
        }
        Expr::This => ctx.owner.clone().unwrap_or_else(|| "unknown".into()),
        // Nested chains (`a.B.C`): type of this Field expr, not the inner
        // receiver class. `class_from_expr` on the Field resolves `B` then
        // returns C's declaring type; using only `receiver` broke `b.Value.Value`.
        Expr::Field { .. } => class_from_expr(expr, ctx).into(),
        Expr::New { ty, .. } => match &ty.node {
            Type::Named { path, .. } => path.last().cloned().unwrap_or_else(|| "unknown".into()),
            _ => "unknown".into(),
        },
        _ => "unknown".into(),
    }
}

/// 泛型方法缺陷 A：判断 receiver 是否「内建 object 方法」的适用类型——
/// 基元（int/long/...）或泛型类型参数占位。`TypeId::Generic(name)` 是泛型参数
/// 的权威表示；未被 registry 注册的 `Named(name)` 是其在 MIR 局部表中的等价
/// 表示（`lower_type_name` 对未知类型名回退 `Named`）。这两类类型上的
/// `ToString()`/`GetHashCode()`/`Equals()`/`CompareTo()` 返回类型是确定的
/// （Object 根 + 基元内置），可安全短路推断；否则 fallback 把接收者类型误判
/// 为返回类型（Int），引发 alloca i32 + ptrtoint 截断字符串指针（泛型方法体
/// 单态化前 `value.ToString()` 打印垃圾值根因）。
fn is_object_method_receiver(ty: &TypeId, ctx: &LowerCtx) -> bool {
    match ty {
        TypeId::Generic(_) => true,
        TypeId::Named(n) => {
            !(ctx.registry.is_class(n)
                || ctx.registry.is_static_class(n)
                || ctx.registry.is_struct(n)
                || ctx.registry.is_enum(n)
                || ctx.registry.is_variant(n)
                || ctx.registry.is_interface(n))
        }
        TypeId::Int
        | TypeId::Long
        | TypeId::Short
        | TypeId::Byte
        | TypeId::Float
        | TypeId::Double
        | TypeId::Bool
        | TypeId::Char
        | TypeId::UInt
        | TypeId::ULong
        | TypeId::UShort
        | TypeId::SByte => true,
        _ => false,
    }
}

/// RFC 044：判断 `foreach` 迭代源是否为 `IEnumerable<T>` 接口类型（含其
/// 单态 mangle 名 `IEnumerable_int` 等）。接口值在 codegen 以胖指针
/// `{ ptr obj, ptr itable }` 表示，走 `GetEnumerator()`/`MoveNext()`/`Current`
/// 协议分派；非索引路径（`get_Count`/`get_Item`）。List_* / 数组 / Span 等
/// 具体集合不在此列（各走自身索引快路径）。
pub(super) fn is_enumerable_iface(ty: &TypeId) -> bool {
    match ty {
        TypeId::IEnumerable { .. } => true,
        TypeId::Named(n) => n.starts_with("IEnumerable_"),
        _ => false,
    }
}

/// P0 双引擎收敛：带 span 的表达式类型推断入口。
///
/// 优先查 typeck 下达的 span 键表达式类型表（`ctx.expr_types`）：typeck 的
/// `check_expr_at` 在检查出口记录了每个表达式节点的结论，MIR 侧命中即直接
/// 采用——两套推断引擎对 builtin 成员/方法知识的重复维护收敛为单一事实源。
/// 未命中（合成节点 / borrow 侧 / typeck 未覆盖路径）或 Ambiguous（宏展开 /
/// 泛型单态化克隆共享模板 span，typeck 冲突降级）回落旧推断
/// [`infer_type_from_expr`]，行为与改造前一致。
pub(super) fn infer_type_from_spanned(spanned: &Spanned<Expr>, ctx: &LowerCtx) -> TypeId {
    if let Some(ty) = ctx.expr_types.get(spanned.span) {
        return ty.clone();
    }
    infer_type_from_expr(&spanned.node, ctx)
}

pub(super) fn infer_type_from_expr(expr: &Expr, ctx: &LowerCtx) -> TypeId {
    // RFC 004 M1：variant 构造（`Content.None` 无 payload / `Content.Element(x)`
    // 有 payload）的类型即该 variant 的命名类型。若不在此拦截，fallback 到 Int：
    // 临时 local 被 alloca 为 i32，codegen 对 variant 指针做 `ptrtoint ... to i32`
    // 截断 64 位指针 → 0xC0000005（visualhost_data_context_isolation_e2e 根因）。
    if let Some(vname) = variant_construct_type_name(expr, ctx) {
        return TypeId::Named(vname);
    }
    match expr {
        Expr::Ident(name) => {
            // First check locals (parameters, variables)
            if let Some(id) = ctx.lookup(name) {
                if let Some((_, ty)) = ctx.locals.get(&id) {
                    return ty.clone();
                }
            }
            // Implicit `this.field` in method body: resolve field type, then
            // try the matching property getter (`get_{field}`).
            if ctx.is_class_field(name) {
                if let Some(owner) = &ctx.owner {
                    let access = access_ctx(ctx);
                    if let Ok(fty) = ctx.registry.resolve_field(owner, name, &access) {
                        return expand_delegate_alias(ctx, type_name_to_type_id(&fty));
                    }
                    let getter: Ident = format!("get_{name}").into();
                    if let Ok(sig) = ctx.registry.resolve_method(owner, &getter, &access) {
                        return type_name_to_type_id(&sig.ret);
                    }
                }
            }
            // Static class name (e.g. File, Console, Math).
            // 必须同时覆盖 StaticClass（如 QIFReporting、native 模块）：
            // 否则 receiver_type 推断 fallback 到 Int，导致 MethodCall
            // 的 resolve_method_overload 查找 "int".method 失败，返回类型
            // 误判为 Int，codegen 生成 ptrtoint ptr to i32 截断字符串指针。
            // struct / enum 类型名同理：`DateTime._dateToTicks` / `DateTime._pad4`
            // 的 receiver 若 fallback 到 Int，long 返回被 trunc 为 i32、string
            // 返回被 ptrtoint，表现为 ToString 部件损坏（RFC 007 日期阻塞根因）。
            if ctx.registry.is_class(name)
                || ctx.registry.is_static_class(name)
                || ctx.registry.is_struct(name)
                || ctx.registry.is_enum(name)
            {
                return TypeId::Named(name.clone());
            }
            // RFC 004：variant 类型名（如 Content、SetterValue）——variant 是值类型，
            // 命名类型映射到 ptr（llvm_type_of 对 variant 返回 "ptr"）。
            // 缺失此分支会导致 MethodCall receiver（如 Content.Text(...)）的
            // 推断 fallback 到 Int，使临时 local 被 alloca 为 i32，ptrtoint
            // truncate 64 位指针，造成 0xc0000005 崩溃。
            if ctx.registry.is_variant(name) {
                return TypeId::Named(name.clone());
            }
            // Interface name (e.g. IComparable, IDisposable) — 返回命名类型 ptr，
            // 使 interface dispatch 的临时 local 类型正确。
            if ctx.registry.is_interface(name) {
                return TypeId::Named(name.clone());
            }
            TypeId::Int
        }
        Expr::This => ctx
            .owner
            .as_ref()
            .map(|o| TypeId::Named(o.clone()))
            .unwrap_or(TypeId::Int),
        // CD-15/D4：`base` 表达式的静态类型 = 直接基类（与 typeck 一致）。
        Expr::Base => direct_base_class(ctx)
            .map(TypeId::Named)
            .unwrap_or(TypeId::Int),
        // 未在 expr_types 表命中的 lambda（typeck 已校验、span 重写后错位或
        // 无捕获零开销路径）：不得回退 Int（会把 λ 参数/闭包当 i32 截断，
        // receiver 变成 int → `int_SetConfig` 之类错绑）。返回与 typeck 同构的
        // `Func{params: Infer…, ret: Infer}`——调用点 resolution 的 λ 软匹配
        // 按元数对齐目标 Func 槽，形参最终类型由 `expected_lambda_params`
        //（形参 Func/Action 解构）定向。
        Expr::Lambda(l) => TypeId::Func {
            params: l.params.iter().map(|_| TypeId::Infer).collect(),
            ret: Box::new(TypeId::Infer),
        },
        Expr::Call {
            func,
            type_args,
            args,
            params_span: _,
        } => {
            if let Expr::Ident(name) = &func.node {
                // 委托局部调用（`fac(x)`）：按委托返回类型推断结果，避免回退
                // Int 把返回的引用指针（object/string/class）截断为 i32。
                if let Some(local_id) = ctx.lookup(name) {
                    if let Some((_, lty)) = ctx.locals.get(&local_id) {
                        if is_delegate_type(lty) {
                            if let Some(ret) =
                                delegate_return_type(lty, &|s| ctx.registry.types.contains_key(s))
                            {
                                return ret;
                            }
                        }
                    }
                }
                // 实例委托字段裸调用（`_callback()`）：字段非局部、非自由函数——
                // 按字段委托返回类型推断，否则回落 Int 会把返回的引用指针物化为
                // i32 临时（chord `EffectEntry.Run: _disposer = _callback()` 的
                // ptrtoint→inttoptr 截断实证；与 func=Field 对称路径同源）。
                if ctx.is_class_field(name) {
                    if let Some(owner) = &ctx.owner {
                        let access = access_ctx(ctx);
                        if let Ok(fty) = ctx.registry.resolve_field(owner, name, &access) {
                            let fty_id = expand_delegate_alias(ctx, type_name_to_type_id(&fty));
                            if is_delegate_type(&fty_id) {
                                if let Some(ret) = delegate_return_type(&fty_id, &|s| {
                                    ctx.registry.types.contains_key(s)
                                }) {
                                    return ret;
                                }
                            }
                        }
                    }
                }
                if name.as_str() == "rt_expr_tree_summary" {
                    return TypeId::String;
                }
                if let Some((_, ret)) = ctx.fn_sigs.get(name.as_str()) {
                    return ret.clone();
                }
                // RFC 004 M1: 泛型函数单态化调用（如 `Same<int>(...)`）。
                // `fn_sigs` 中以 mangled 名（`Same_int`）注册，原始名（`Same`）查不到。
                // 用 `type_args` 构造 mangled 名再查，回填真实返回类型。
                // 修复前：fallback 到 `TypeId::Int`，导致返回 bool/string/long 等的
                // 泛型函数被错误地物化为 i32 临时 local，与 codegen 的真实返回类型
                //（如 i1）错配，引发 LLVM verifier "defined with type i32 but expected i1"。
                if !type_args.is_empty() {
                    let arg_tys: Vec<TypeId> =
                        type_args.iter().map(|t| lower_type_name(&t.node)).collect();
                    let mangled = typeck::mangle_generic(name.as_str(), &arg_tys);
                    if let Some((_, ret)) = ctx.fn_sigs.get(mangled.as_str()) {
                        return ret.clone();
                    }
                    // 泛型类模板在表达式位置（`Holder<Thing>` 作为静态成员 receiver，
                    // 如 `Holder<Thing>.Cache` / `Holder<Thing>.Get(...)`）。typeck 已
                    // 单态化并注册 mangle 名；registry 中存在该类则返回其命名类型，
                    // 否则回退（避免静默错型/ptrtoint 截断）。
                    let mangled_ident: Ident = mangled.as_str().into();
                    if ctx.registry.is_class(&mangled_ident)
                        || ctx.registry.is_static_class(&mangled_ident)
                        || ctx.registry.is_struct(&mangled_ident)
                    {
                        return TypeId::Named(mangled.into());
                    }
                }
                // Bare name not in fn_sigs: resolve as a method on the current
                // owning class (same-class call like `Eval(80)`). 与 MIR
                // lower_call 的裸实例调用重写（`_bump()` → `this._bump()`）对齐：
                // 静态（`Eval(80)`）与实例（`FindStream(sid)`）方法都须正确推断
                // 返回类型，否则临时 local 被 alloca 为 i32，codegen 对 ptr 返回
                // 值做 `ptrtoint` 截断 → LLVM verifier 拒收 / 指针损坏。
                if let Some(ref owner) = ctx.owner {
                    if let Some(nom) = ctx.registry.types.get(owner) {
                        if let Some(sigs) = nom.methods.get(name) {
                            // 优先取 arity 匹配的重载；否则取首个。返回类型即推断结果。
                            if let Some(ret_name) = sigs
                                .iter()
                                .find(|s| s.params.len() == args.len())
                                .or_else(|| sigs.first())
                                .map(|s| s.ret.clone())
                            {
                                let ret = type_name_to_type_id(&ret_name);
                                if !matches!(ret, TypeId::Named(ref n) if n.as_str() == "void") {
                                    return ret;
                                }
                            }
                        }
                    }
                }
            } else if let Expr::Field { receiver, field } = &func.node {
                // Native module / static class / struct / enum 静态方法。
                let recv_class = class_from_expr(&receiver.node, ctx);
                let class_ident: Ident = recv_class.clone().into();
                if ctx.registry.is_class(&class_ident)
                    || ctx.registry.is_static_class(&class_ident)
                    || ctx.registry.is_struct(&class_ident)
                    || ctx.registry.is_enum(&class_ident)
                {
                    let access = access_ctx(ctx);
                    if let Ok(sig) = ctx.registry.resolve_method(&class_ident, field, &access) {
                        let ret = type_name_to_type_id(&sig.ret);
                        if !matches!(ret, TypeId::Named(ref n) if n.as_str() == "void") {
                            return ret;
                        }
                    }
                    if let Some(ret_name) = ctx
                        .registry
                        .types
                        .get(class_ident.as_str())
                        .and_then(|n| n.methods.get(field))
                        .and_then(|sigs| sigs.first())
                        .map(|s| s.ret.clone())
                    {
                        let ret = type_name_to_type_id(&ret_name);
                        if !matches!(ret, TypeId::Named(ref n) if n.as_str() == "void") {
                            return ret;
                        }
                    }
                }
                // 委托实例字段调用（`this._f(...)` / `obj._f(...)`，func 为
                // Field）：静态方法解析未命中时，若字段类型是委托则按委托返回
                // 类型推断——否则回落 Int 把返回的引用指针物化为 i32 截断
                //（与 func=Ident 的裸 `_f()` 字段路径同源，chord 实证）。
                let access = access_ctx(ctx);
                if let Ok(fty) = ctx.registry.resolve_field(&class_ident, field, &access) {
                    let fty_id = expand_delegate_alias(ctx, type_name_to_type_id(&fty));
                    if is_delegate_type(&fty_id) {
                        if let Some(ret) =
                            delegate_return_type(&fty_id, &|s| ctx.registry.types.contains_key(s))
                        {
                            return ret;
                        }
                    }
                }
            }
            TypeId::Int
        }
        Expr::IntLit(_) => TypeId::Int,
        Expr::FloatLit(ast::FloatLitValue::Float(_)) => TypeId::Float,
        Expr::FloatLit(ast::FloatLitValue::Double(_)) => TypeId::Double,
        Expr::BoolLit(_) => TypeId::Bool,
        Expr::CharLit(_) => TypeId::Char,
        Expr::StringLit(_) => TypeId::String,
        // RFC 007：未脱糖的插值（防御）；正常路径 typeck 已脱糖为 `+`。
        Expr::InterpolatedString { .. } => TypeId::String,
        // `null` 字面量类型推断为 `Infer` 哨兵（而非 String）：重载解析的
        // `param_assignable` 对 `Infer` 放行——可匹配任意引用类型形参（含
        // `Dictionary<...>` 等），使 `ExecuteAsync(code, null, ct)` 能正确绑定
        // 3-参重载，避免回退到同名 2-参重载导致自递归。操作数发射仍走
        // `MirOperand::ConstNull`（类型无关），不影响 null 指针代码生成。
        Expr::Null => TypeId::Infer,
        Expr::Default { ty } => lower_type_name(&ty.node),
        // RFC 018 M2 step 4: typeof(T) 的类型用公共基类 `Type`（与 typeck 对齐，
        // 见 check_expr.rs Expr::TypeOf）。RuntimeType 是 internal 实现细节，子库
        // publish 时不在外部符号中，若此处推断为 RuntimeType 会导致
        // `param_assignable("Type","RuntimeType")` 失败（RuntimeType 未注册，无法
        // is_subtype）——`sp.GetService(typeof(X))` 误解析到扩展方法。codegen 对
        // receiver_type ∈ {RuntimeType, Type} 对偶拦截，改标 Type 不影响代码生成。
        Expr::TypeOf(_) => TypeId::Named("Type".into()),
        // RFC 036 M1: `expr is pattern` 返回 bool。
        Expr::Is { .. } => TypeId::Bool,
        // RFC 006 M2：`with` 结果类型 = 接收者类型（正常路径已脱糖）。
        Expr::With { receiver, .. } => infer_type_from_spanned(receiver, ctx),
        // 使用 resolve_instantiated_type_name 保留泛型实参，使
        // `new EntityMap<User>(...)` 被推断为 `EntityMap_User` 而非 `EntityMap`。
        // 否则泛型类型的 fluent chain 方法调用会被 mangle 为
        // `EntityMap_Column` 而非 `EntityMap_User_Column`，链接失败。
        Expr::New { ty, .. } => typeck::resolve_instantiated_type_name(&ty.node)
            .map(|name| TypeId::Named(name.into()))
            .unwrap_or_else(|| lower_type_name(&ty.node)),
        // `new string[0]` 等数组分配：推断为数组类型——否则回落 Int 会把数组指针
        // 经 i32 本地往返截断（`ptrtoint ... to i32` 丢 x64 高位 → 0xC0000005）。
        Expr::NewArray { elem_type, .. } => TypeId::Array {
            elem: Box::new(lower_type_name(&elem_type.node)),
        },
        Expr::CollectionExpr { elements } => {
            let elem = elements
                .first()
                .map(|el| match el {
                    CollectionElement::Element(e) => infer_type_from_spanned(e, ctx),
                    CollectionElement::Spread(e) => match infer_type_from_spanned(e, ctx) {
                        TypeId::Array { elem } => *elem,
                        other => other,
                    },
                })
                .unwrap_or(TypeId::Named("object".into()));
            TypeId::Array {
                elem: Box::new(elem),
            }
        }
        Expr::StackSpanLit { mutable, elem, .. } => TypeId::Span {
            elem: Box::new(elem.clone()),
            mutable: *mutable,
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span: _,
        } => {
            // RFC 007 / 基元静态 `int.ToString(x)` → string。
            // builtin_static_method 能识别，但 registry 无 `int` 类型条目，
            // 若不在此短路，fallback 会把返回类型误判为 `int`，导致
            // alloca i32 + ptrtoint，进而破坏插值 `+` 的 rt_str_concat。
            if method.as_str() == "ToString" {
                if let Some(func) = builtin_static_method(&receiver.node, method) {
                    if func.ends_with(".ToString") {
                        return TypeId::String;
                    }
                }
            }
            // Instance ToString(): x.ToString() where x is int/long/... or a
            // generic type parameter.
            // Same root cause as the static path above — primitives are not in
            // the registry, so the fallback would wrongly infer the receiver
            // type (int/long/...) as the return type, causing alloca i32 + ptrtoint.
            // 泛型参数（`TypeId::Generic` 或未注册 `Named` 占位）在单态化前代表
            // 任意类型：`U.ToString()` 恒为 string（Object 根 + 基元内置），否则
            // fallback 把返回类型误判为 Int，泛型方法模板体 lower 时
            // `rt_int_to_string` 结果被 ptrtoint 截断 → 打印指针垃圾值。
            if method.as_str() == "ToString" {
                let recv_ty = infer_type_from_spanned(receiver, ctx);
                if is_object_method_receiver(&recv_ty, ctx) {
                    return TypeId::String;
                }
            }
            // Instance GetHashCode/Equals/CompareTo on primitives/generic params
            // → same pattern. Without this, the fallback infers the receiver
            // type (e.g. int) as the return type, causing alloca i32 where
            // i1/i32 is expected.
            if matches!(method.as_str(), "GetHashCode" | "CompareTo") {
                let recv_ty = infer_type_from_spanned(receiver, ctx);
                if is_object_method_receiver(&recv_ty, ctx) || matches!(recv_ty, TypeId::String) {
                    return TypeId::Int;
                }
            }
            if method.as_str() == "Equals" {
                let recv_ty = infer_type_from_spanned(receiver, ctx);
                if is_object_method_receiver(&recv_ty, ctx) {
                    return TypeId::Bool;
                }
            }
            // RFC 005：`T[]`.AsSpan / AsReadOnlySpan；`Span`.Slice / AsReadOnly。
            // RFC 005 M2：`List<T>`.AsSpan / AsReadOnlySpan。
            {
                let recv_ty = infer_type_from_spanned(receiver, ctx);
                match (method.as_str(), &recv_ty) {
                    ("AsSpan", TypeId::Array { elem }) => {
                        return TypeId::Span {
                            elem: elem.clone(),
                            mutable: true,
                        };
                    }
                    ("AsReadOnlySpan", TypeId::Array { elem }) => {
                        return TypeId::Span {
                            elem: elem.clone(),
                            mutable: false,
                        };
                    }
                    ("AsSpan", TypeId::Named(n)) if n.starts_with("List_") => {
                        if let Some(elem) = recv_ty.enumerable_elem() {
                            return TypeId::Span {
                                elem: Box::new(elem),
                                mutable: true,
                            };
                        }
                    }
                    ("AsReadOnlySpan", TypeId::Named(n)) if n.starts_with("List_") => {
                        if let Some(elem) = recv_ty.enumerable_elem() {
                            return TypeId::Span {
                                elem: Box::new(elem),
                                mutable: false,
                            };
                        }
                    }
                    ("Slice", TypeId::Span { elem, mutable }) => {
                        return TypeId::Span {
                            elem: elem.clone(),
                            mutable: *mutable,
                        };
                    }
                    (
                        "AsReadOnly",
                        TypeId::Span {
                            elem,
                            mutable: true,
                        },
                    ) => {
                        return TypeId::Span {
                            elem: elem.clone(),
                            mutable: false,
                        };
                    }
                    ("CopyTo", TypeId::Span { .. }) => {
                        return TypeId::Void;
                    }
                    ("TryCopyTo", TypeId::Span { .. }) => {
                        return TypeId::Bool;
                    }
                    ("ToArray", TypeId::Span { elem, .. }) => {
                        return TypeId::Array { elem: elem.clone() };
                    }
                    _ => {}
                }
            }
            // struct/class/enum 类型名静态方法（如 DateTime.FromYMD）：直接查 registry。
            if let Expr::Ident(name) = &receiver.node {
                if ctx.registry.is_struct(name)
                    || ctx.registry.is_class(name)
                    || ctx.registry.is_enum(name)
                    || ctx.registry.is_static_class(name)
                {
                    if let Some(sigs) = ctx
                        .registry
                        .types
                        .get(name)
                        .and_then(|n| n.methods.get(method))
                    {
                        let arity = args.len();
                        // RFC 007 静态同元数重载分裂修复：推断须与实际发射一致的
                        // 重载解析（按实参类型）——此前仅「static + 元数」取**首个**
                        // 候选，`Color.Lerp(Color,Color,double)`（公开）与私有
                        // `Lerp(double,double,double)` 同元数时命中公开载 → 返回类型
                        // 误判 Color；发射侧按实参类型选中 double 载 → 实参按 struct
                        // 物化（`load %struct.Color, ptr` 直读 double）→ clang IR
                        // 校验失败。strict 解析失败（未绑定 λ 等）再回落元数匹配。
                        let access = access_ctx(ctx);
                        let arg_type_names: Vec<Ident> = args
                            .iter()
                            .map(|a| type_id_name(&infer_type_from_spanned(a, ctx)))
                            .collect();
                        let type_aware = ctx.registry.resolve_method_overload(
                            name,
                            method,
                            &arg_type_names,
                            &access,
                        );
                        let sig_opt: Option<typeck::OopMethodSig> = type_aware
                            .ok()
                            .map(|(_, s)| s)
                            .or_else(|| {
                                sigs.iter()
                                    .find(|s| {
                                        s.modifier == MethodModifier::Static
                                            && s.params.len() == arity
                                    })
                                    .cloned()
                            })
                            .or_else(|| {
                                sigs.iter()
                                    .find(|s| s.modifier == MethodModifier::Static)
                                    .cloned()
                            })
                            .or_else(|| sigs.first().cloned());
                        if let Some(sig) = sig_opt {
                            let ret = type_name_to_type_id(&sig.ret);
                            if !matches!(ret, TypeId::Named(ref n) if n.as_str() == "void") {
                                return ret;
                            }
                        }
                    }
                }
            }
            if let Some(func) = builtin_static_method(&receiver.node, method) {
                // Stub facade 类静态方法的返回类型从 registry 查询，避免硬编码
                // 每个方法（Console.Write/Read/ReadLine、Path.Combine、
                // File.Exists 等几十个方法）。`is_builtin_facade` 命中的类
                // 方法体为空 stub——返回类型签名已由 typeck Pass 2 注册到
                // registry，可直接查询。
                //
                // `string.Compare` 走 primitive 路径但 codegen `try_emit_primitive_static`
                // 返回 i1（bool）而非 i32；typeck 注册的 sig.ret 是 "int"
                // （C# `IComparable<T>.Compare` 契约返回 int）。两者一致。
                if let Expr::Ident(name) = &receiver.node {
                    if let Some(nom) = ctx.registry.types.get(name) {
                        if let Some(sigs) = nom.methods.get(method) {
                            if let Some(sig) =
                                sigs.iter().find(|s| s.modifier == MethodModifier::Static)
                            {
                                return type_name_to_type_id(&sig.ret);
                            }
                        }
                    }
                }
                // 特殊情况：无法从 registry sig 直接推断的 facade 方法。
                match func.as_str() {
                    // Task.FromResult 的返回类型 Task<T> 中 T 由 arg 推断
                    // （registry sig.ret 是 "Task<T>" 泛型占位，需运行时推断）。
                    "Task.FromResult" => {
                        let inner = infer_type_from_spanned(&args[0], ctx);
                        return TypeId::Task {
                            inner: Box::new(inner),
                        };
                    }
                    // Task.WhenAll/WhenAny/CompletedTask/Delay 的 sig.ret 是
                    // "Task"（非泛型），但 codegen 拦截器返回 Task<void>。
                    "Task.WhenAll" | "Task.WhenAny" | "Task.CompletedTask" | "Task.Delay" => {
                        return TypeId::Task {
                            inner: Box::new(TypeId::Void),
                        };
                    }
                    // string.Join/Concat/Format 等静态方法返回 string；
                    // "string" 是 builtin 类型，不在 registry 中注册为 class。
                    "string.Join" | "string.Concat" | "string.Format" => {
                        return TypeId::String;
                    }
                    _ => {}
                }
            }
            // Builtin static methods with codegen-specialized emission
            if let Expr::Ident(name) = &receiver.node {
                if ctx.registry.is_class(name) {
                    match (name.as_str(), method.as_str()) {
                        ("File", "ReadAllText") => return TypeId::String,
                        ("File", "WriteAllText") => return TypeId::Bool,
                        ("Console", "WriteLine") => return TypeId::Void,
                        ("Math", _) => {
                            // 根据方法名与参数类型推断返回类型（支持重载）。
                            // 默认返回 double（Sqrt/Sin/Cos 等数学函数）；
                            // Abs/Min/Max 返回与参数相同的类型；
                            // Sign 始终返回 int。
                            match method.as_str() {
                                "Abs" | "Min" | "Max" => {
                                    if let Some(first_arg) = args.first() {
                                        let arg_ty = infer_type_from_spanned(first_arg, ctx);
                                        if arg_ty == TypeId::Int || arg_ty == TypeId::Long {
                                            return arg_ty;
                                        }
                                    }
                                    return TypeId::Double;
                                }
                                "Sign" => return TypeId::Int,
                                _ => return TypeId::Double,
                            }
                        }
                        // RFC 032 M1: QIF Assert facade 静态方法均返回 void。
                        // codegen emit_call_typed 拦截后发射 rt_qif_assert_* ABI，
                        // 失败时 runtime 调用 rt_panic_at 终止进程。
                        (
                            "Assert",
                            "Equal" | "True" | "False" | "Null" | "NotNull" | "Fail" | "Skip",
                        ) => {
                            return TypeId::Void;
                        }
                        _ => {}
                    }
                }
            }
            // Builtin `string` instance methods (P2): Split/Replace/Substring/
            // Contains/IndexOf/StartsWith/EndsWith/Trim/ToUpper/ToLower.
            // These are not registered in the type registry, so overload
            // resolution always fails and the fallback returns TypeId::Int.
            // Returning the correct type here ensures the temp local alloca
            // matches the rvalue's LLVM type (e.g. `i1` for bool methods,
            // `ptr` for string-returning methods).
            let recv_type_id = infer_type_from_spanned(receiver, ctx);
            if recv_type_id == TypeId::String {
                match method.as_str() {
                    "Split" => {
                        return TypeId::Array {
                            elem: Box::new(TypeId::String),
                        }
                    }
                    // `string.ToString()` 返回 string（自身）。缺此分支时 fallback
                    // 把返回类型误判为 Int，临时 local 物化为 i32，codegen 对 string
                    // 指针做 ptrtoint 再 rt_int_to_string，打印出指针垃圾值。
                    "ToString" | "ToString_" => return TypeId::String,
                    "Replace" | "Substring" | "Trim" | "TrimStart" | "TrimEnd" | "ToUpper"
                    | "ToLower" | "PadLeft" | "PadRight" | "Insert" | "Remove" => {
                        return TypeId::String
                    }
                    "Contains" | "StartsWith" | "EndsWith" => return TypeId::Bool,
                    "IndexOf" | "LastIndexOf" | "Compare" => return TypeId::Int,
                    "get_Chars" => return TypeId::Char,
                    "ToCharArray" => {
                        return TypeId::Array {
                            elem: Box::new(TypeId::Char),
                        }
                    }
                    // RFC 005 M2：UTF-8 诚实 → ReadOnlySpan<byte>
                    "AsSpan" => {
                        return TypeId::Span {
                            elem: Box::new(TypeId::Byte),
                            mutable: false,
                        };
                    }
                    _ => {}
                }
            }
            // StringBuilder (Arc.Text facade, RFC 021 §4.3 M4): Append/AppendLine
            // return the builder (for chaining), ToString returns string, and
            // get_Length returns int. Without this, `var s = sb.ToString()`
            // infers `int` and the alloca type mismatches the `ptr` returned by
            // `rt_text_sb_to_string`.
            if let TypeId::Named(n) = &recv_type_id {
                if n.as_str() == "StringBuilder" {
                    match method.as_str() {
                        "Append" | "AppendLine" => {
                            return TypeId::Named("StringBuilder".into());
                        }
                        "ToString" => return TypeId::String,
                        "get_Length" => return TypeId::Int,
                        _ => {}
                    }
                }
            }
            let recv_ty = type_id_to_name(&recv_type_id);
            // RFC 037 M-D0：`ObserveProperty("Name")` 是编译器合成入口（无实体
            // 方法表条目），返回 `Signal_<PropType>`（与 typeck
            // `check_observable_observe_call` 一致）。缺此分支时 infer fallback
            // 到 Int，链式 `vm.ObserveProperty("Count").Subscribe(...)` 的中间
            // 临时被 alloca 为 i32，与 codegen 返回的 ptr（Signal 句柄）错配
            // → LLVM "defined with type i32 but expected ptr"。
            if method.as_str() == "ObserveProperty" {
                if let Some(prop_ty) = args
                    .first()
                    .map(|a| &a.node)
                    .and_then(|n| match n {
                        Expr::StringLit(p) => Some(p.as_str().to_string()),
                        _ => None,
                    })
                    .and_then(|prop_name| {
                        ctx.registry
                            .declared_properties
                            .get(recv_ty.as_str())
                            .and_then(|props| props.iter().find(|p| p.name.as_str() == prop_name))
                            .map(|p| p.ty.as_str().to_string())
                    })
                {
                    return TypeId::Named(format!("Signal_{prop_ty}").into());
                }
            }
            // LINQ 终端（MIR 编译期展开，无 registry 方法体）：先于 overload 解析。
            if recv_type_id.is_ienumerable() {
                match method.as_str() {
                    "Any" => return TypeId::Bool,
                    "Count" => return TypeId::Int,
                    "First" | "FirstOrDefault" => {
                        return recv_type_id.enumerable_elem().unwrap_or(TypeId::Infer);
                    }
                    _ => {}
                }
            }
            let elem_opt = recv_type_id.enumerable_elem();
            let arg_types: Vec<Ident> = args
                .iter()
                .map(|a| {
                    if let Expr::Lambda(l) = &a.node {
                        if let Some(elem) = &elem_opt {
                            let body_ty = match &l.body {
                                LambdaBody::Expr(e) => infer_type_from_spanned(e, ctx),
                                LambdaBody::Block(b) => {
                                    if let Some(tail) = &b.tail {
                                        infer_type_from_spanned(tail, ctx)
                                    } else {
                                        TypeId::Void
                                    }
                                }
                            };
                            let params: Vec<TypeId> =
                                l.params.iter().map(|_| elem.clone()).collect();
                            return type_id_name(&TypeId::Func {
                                params,
                                ret: Box::new(body_ty),
                            });
                        }
                    }
                    type_id_name(&infer_type_from_spanned(a, ctx))
                })
                .collect();
            let overload_ctx = AccessContext {
                current_type: ctx.owner.clone(),
                extension_scope: ExtensionScope {
                    imported: ctx.registry.extension_namespace_paths(),
                    enclosing: vec![],
                },
                enclosing_namespace: vec![],
                current_package: None,
                skip_type_visibility: false,
            };
            let type_arg_names: Vec<Ident> = type_args
                .iter()
                .map(|t| type_id_name(&lower_type_name(&t.node)))
                .collect();
            // 带显式类型实参的泛型方法调用（如 `a.Get<Seed>(s)`）必须走
            // `resolve_method_with_type_args`——其返回类型已按实参替换
            //（`T → Seed`），与 typeck 该解析路径 / substitute ret 对齐。
            // 否则 `resolve_method_overload` 对泛型形参（`T seed`）无法匹配，
            // fallback 到下方 `registry.types[...].methods` 取泛型方法**声明
            // 返回类型 `T`**（未替换），链式 `a.Get<Seed>(s).Value()` 接收者
            // 被误判为 `T` → 发射 `@T_Value` undefined（e2e 实测）。
            if !type_arg_names.is_empty() {
                if let Ok((_, sig)) = ctx.registry.resolve_method_with_type_args(
                    &recv_ty,
                    method,
                    &arg_types,
                    &type_arg_names,
                    &overload_ctx,
                ) {
                    return type_name_to_type_id(&sig.ret);
                }
            }
            if let Ok((_, sig)) =
                ctx.registry
                    .resolve_method_overload(&recv_ty, method, &arg_types, &overload_ctx)
            {
                let ret_name: Ident = if !type_arg_names.is_empty() && !sig.generics.is_empty() {
                    typeck::registry::substitute_generic_in_ty_name(
                        &sig.ret,
                        &sig.generics,
                        &type_arg_names,
                    )
                    .into()
                } else {
                    sig.ret.clone()
                };
                return type_name_to_type_id(&ret_name);
            }
            if let Ok(Some(ext)) = ctx.registry.resolve_extension_with_arg_types(
                &recv_ty,
                method,
                args.len(),
                &type_arg_names,
                &arg_types,
                &overload_ctx,
            ) {
                return type_name_to_type_id(&ext.sig.ret);
            }
            // Fallback: look up the method's return type from the vtable layout.
            // This handles virtual methods where overload resolution fails due to
            // namespace-qualified receiver types or other resolution gaps.
            // 同名多槽（重载）时取首槽——仅用于类型推断兜底，非分派。
            // RFC 009 M2（泛型调用损坏修复）：两个 fallback 取的都是方法
            // **声明** ret，泛型方法的 `T` 不随显式类型实参替换。对齐上方
            // resolve_method_overload 路径的 `substitute_generic_in_ty_name`
            // 替换：`a.Get<Seed>(s)` 若因 AccessContext（私有方法）/重载匹配
            // 失败落入 fallback，声明 `T` 会让接收者被推断为未定义的 `T` →
            // 后续 `@T_Value` undefined / Int 兜底损坏（e2e 实测）。
            let subst_ret = |ret: &Ident, generics: &[Ident]| -> Ident {
                if !type_arg_names.is_empty() && !generics.is_empty() {
                    typeck::registry::substitute_generic_in_ty_name(ret, generics, &type_arg_names)
                        .into()
                } else {
                    ret.clone()
                }
            };
            if let Some(ret_name) = ctx
                .layouts
                .classes
                .get(&recv_ty)
                .and_then(|c| c.virtual_slots.iter().find(|s| s.name.as_str() == method))
                .map(|s| s.ret.clone())
            {
                // 槽位只存 name/ret/params，不带泛型形参名——从 registry 按
                // 接收者类补取（私有泛型方法必然声明在接收者类上，可命中；
                // 继承链上的泛型虚方法取不到时保持声明 ret 原行为）。
                let generics = ctx
                    .registry
                    .types
                    .get(recv_ty.as_str())
                    .and_then(|n| n.methods.get(method))
                    .and_then(|sigs| sigs.first())
                    .map(|s| s.generics.clone())
                    .unwrap_or_default();
                return type_name_to_type_id(&subst_ret(&ret_name, &generics));
            }
            // Fallback: 直接从 registry.types[class].methods[method] 取首个
            // 重载的 ret 类型。`resolve_method_overload` 受 AccessContext 限制，
            // 对 private 方法（如 `WgpuRender.default_wgsl_source`）在 enclosing
            // 为空的 MIR 推断上下文中会失败，但方法本身确实注册在 registry。
            // 仅用于类型推断（决定临时 local 的 alloca 类型），不影响实际
            // 访问控制——访问合法性已由 typeck 阶段保证。
            if let Some(sig) = ctx
                .registry
                .types
                .get(recv_ty.as_str())
                .and_then(|n| n.methods.get(method))
                .and_then(|sigs| sigs.first())
            {
                return type_name_to_type_id(&subst_ret(&sig.ret, &sig.generics));
            }
            // Builtin monomorphized collection method return types.
            // List_T.Get(i) → T; List_T.Find(...) → T; etc.
            // These are inlined by codegen and not registered in the type
            // registry, so overload resolution always fails.
            if let Some(elem) = recv_ty.strip_prefix("List_") {
                match method.as_str() {
                    "get_Item" | "Find" => return type_name_to_type_id(&elem.into()),
                    "FindAll" | "AddRange" => return recv_type_id.clone(),
                    "GetEnumerator" => {
                        return TypeId::Named(format!("ListEnumerator_{elem}").into())
                    }
                    "Contains" | "Exists" | "TrueForAll" | "Remove" | "RemoveAt" | "Clear"
                    | "Sort" | "Reverse" | "Insert" | "set_Item" | "Add" | "ForEach"
                    | "RemoveAll" => return TypeId::Int,
                    "Count" | "IndexOf" | "FindIndex" | "FindLastIndex" | "LastIndexOf" => {
                        return TypeId::Int;
                    }
                    _ => {}
                }
            }
            // RFC 004：variant 构造表达式（如 Content.Text("hello")）。
            // Variant case 不在 registry.methods 中（存在于 variants 列表），
            // 常规方法解析均失败。variant 构造的返回类型即 variant 自身，
            // 直接使用 receiver 类型即可。缺失此分支 → fallback TypeId::Int，
            // 导致 _variant_arg 临时 local alloca 为 i32，ptrtoint 截断
            // variant 指针，造成 0xc0000005 崩溃。
            if let TypeId::Named(ref n) = recv_type_id {
                if ctx.registry.is_variant(n) {
                    return recv_type_id;
                }
            }
            TypeId::Int
        }
        Expr::ExpressionLit(_) => TypeId::String,
        Expr::Field { receiver, field } => {
            let recv_ty = infer_type_from_spanned(receiver, ctx);
            if recv_ty == TypeId::String && field.as_str() == "Length" {
                return TypeId::Int;
            }
            // RFC 005：Span / ReadOnlySpan.Length / IsEmpty
            if matches!(recv_ty, TypeId::Span { .. }) && field.as_str() == "Length" {
                return TypeId::Int;
            }
            if matches!(recv_ty, TypeId::Span { .. }) && field.as_str() == "IsEmpty" {
                return TypeId::Bool;
            }
            // RFC 005：`Span<T>.Empty` / `ReadOnlySpan<T>.Empty`
            if let Expr::Call {
                func,
                args: ref ca,
                type_args,
                params_span: _,
            } = &receiver.node
            {
                if ca.is_empty() && type_args.len() == 1 && field.as_str() == "Empty" {
                    if let Expr::Ident(name) = &func.node {
                        if name.as_str() == "Span" || name.as_str() == "ReadOnlySpan" {
                            let elem = lower_type_name(&type_args[0].node);
                            return TypeId::Span {
                                elem: Box::new(elem),
                                mutable: name.as_str() == "Span",
                            };
                        }
                    }
                }
            }
            // Task facade (RFC 005 M4): Task.CompletedTask 静态属性类型推断。
            // typeck 已返回 Task<Void>；MIR lower 转为 Call { func: "Task.CompletedTask" }，
            // codegen try_emit_task_static 要求 expected 为 Task，否则返回 None 走用户函数路径。
            if let Expr::Ident(name) = &receiver.node {
                if name == "Task" && field == "CompletedTask" {
                    return TypeId::Task {
                        inner: Box::new(TypeId::Void),
                    };
                }
                // CancellationToken facade (RFC 009 M4): CancellationToken.None 类型推断。
                // typeck 已返回 Named("CancellationToken")；此处与 lower 拦截对齐，
                // 否则 Call { func: "CancellationToken.None" } 的 rvalue 类型推断失败。
                if name == "CancellationToken" && field == "None" {
                    return TypeId::Named("CancellationToken".into());
                }
                // Thread facade（RFC 009 M5.5）：静态属性类型推断，与 lower 拦截对齐。
                if name == "Thread" && field == "ManagedThreadId" {
                    return TypeId::Int;
                }
                if name == "Thread" && field == "CurrentThread" {
                    return TypeId::Named("Thread".into());
                }
                // RFC 004 M1：基元类型 static abstract 属性类型推断。
                // `int.Zero` / `double.One` 等返回基元类型本身。
                // typeck 已通过 check_primitive_static_field 校验仅数值类型有 Zero/One。
                if matches!(field.as_str(), "Zero" | "One") {
                    match name.as_str() {
                        "int" => return TypeId::Int,
                        "long" => return TypeId::Long,
                        "short" => return TypeId::Short,
                        "byte" => return TypeId::Byte,
                        "float" => return TypeId::Float,
                        "double" => return TypeId::Double,
                        "uint" => return TypeId::UInt,
                        "ulong" => return TypeId::ULong,
                        "ushort" => return TypeId::UShort,
                        "sbyte" => return TypeId::SByte,
                        _ => {}
                    }
                }
            }
            // Task facade (RFC 009 M1): Task<T> 属性类型推断。
            // TypeId::Task 是内建类型，不走 registry；必须在此返回正确类型，
            // 否则 `var r = t.Result;` 的 local_ty 推断为 Int，codegen
            // try_emit_task_method 用 rt_task_result_int 而非 rt_task_result_ptr，
            // 导致 string/引用类型结果读取崩溃。
            // 泛型单态化后局部类型是 mangled 名 `Named("Task_UserDto")` 而非
            // TypeId::Task；用 parse_task_generic_name 还原为 TypeId::Task 后再
            // 按字段返回 inner 类型，否则 `TResponse resp = task.Result;` 的 temp
            // local_ty 回退 Int → codegen 发射 rt_task_result_int + inttoptr，
            // 引用类型结果被截断（web_mb_host_route_bind_e2e 实测）。
            let task_ty = match &recv_ty {
                TypeId::Task { .. } => Some(recv_ty.clone()),
                TypeId::Named(n) => parse_task_generic_name(n.as_str()),
                _ => None,
            };
            if let Some(TypeId::Task { inner }) = task_ty {
                return match field.as_str() {
                    "Status" => TypeId::Named("TaskStatus".into()),
                    "Result" => (*inner).clone(),
                    "IsCompleted" | "IsCanceled" | "IsFaulted" => TypeId::Bool,
                    "Exception" => TypeId::Named("Exception".into()),
                    _ => TypeId::Int,
                };
            }
            // CTS/CT facade (RFC 009 M4): 属性类型推断。
            // `using Arc;` 不递归加载 `std/Arc/Tasks/` 子目录，registry 中无 CTS/CT
            // 类注册；infer_type_from_expr 的 registry fallback 会返回 Int，
            // 导致 `var ct = cts.Token;` 的 ct 类型误判为 Int，
            // `ct.ThrowIfCancellationRequested()` 的 receiver_type 解析为 "int"
            // 而非 "CancellationToken"，codegen 拦截器不触发，调用不存在的用户函数崩溃。
            // 与 typeck check_expr.rs 的 CTS/CT Field access 拦截对齐。
            if let TypeId::Named(n) = &recv_ty {
                match n.as_str() {
                    "CancellationTokenSource" => {
                        return match field.as_str() {
                            "Token" => TypeId::Named("CancellationToken".into()),
                            "IsCancellationRequested" => TypeId::Bool,
                            _ => TypeId::Int,
                        };
                    }
                    "CancellationToken" => {
                        return match field.as_str() {
                            "IsCancellationRequested" => TypeId::Bool,
                            _ => TypeId::Int,
                        };
                    }
                    _ => {}
                }
            }
            // Nested Field chains (`b.Value.Value` / `list[i].Name`): use recursively
            // inferred receiver TypeId. Bare Ident still goes through type_name_from_expr
            // so enum names like `Color.Red` keep working (infer falls back to Int).
            // RFC 018 M3+：Index / get_Item MethodCall 接收者必须走 type_id_to_name，
            // 否则 `methods[i].Name` 的 get_Name 解析失败 → fallback Int → ptrtoint 截断。
            // RFC 009 M2（?. 链式类型推断修复）：NullCond / ForceDeref 同样是
            // 「接收者表达式」，类型须取递归推断的 recv_ty。缺失此分支时落入
            // type_name_from_expr 的 `_ => "unknown"`（两形态无分支）→
            // resolve_field 失败 → TypeId::Int 兜底 → 单行
            // `a?.Next?.Tag ?? "none"` 的 Coalesce 左物化 alloca i32，codegen
            // 对 ptr 做 ptrtoint → LLVM "defined with type i32 but expected
            // ptr"（l2_null_safety_batch Case0_Run 实测；拆两行写法因左操作数
            // 是 Ident 不触发此路径，故 chain_probe 未暴露）。
            let recv_ty_name = match &receiver.node {
                Expr::Field { .. }
                | Expr::Index { .. }
                | Expr::MethodCall { .. }
                | Expr::TypeOf(_)
                | Expr::NullCond { .. }
                | Expr::ForceDeref { .. } => type_id_to_name(&recv_ty),
                _ => type_name_from_expr(&receiver.node, ctx),
            };
            if ctx.registry.is_enum(&recv_ty_name) {
                return TypeId::Named(recv_ty_name);
            }
            let access = access_ctx(ctx);
            if let Ok(fty) = ctx.registry.resolve_field(&recv_ty_name, field, &access) {
                let fty = type_name_to_type_id(&fty);
                // 委托别名字段（`public Converter Convert;`）：registry 按别名
                // 存取字段声明类型（如 `Named("Converter")`），须展开为 `Func`
                // 使 `is_delegate_type` 成立，否则 `g.Convert(5)` 走不到
                // `try_lower_delegate_invoke`，被 mangle 成静态方法直调
                // `@g_Convert` → 链接/完整性（arc-prune-001）失败。与 typeck
                // check_expr.rs 的委托字段读取展开对齐。
                return expand_delegate_alias(ctx, fty);
            }
            // Try property getter: get_{field}
            let getter: Ident = format!("get_{field}").into();
            if let Ok(sig) = ctx.registry.resolve_method(&recv_ty_name, &getter, &access) {
                return type_name_to_type_id(&sig.ret);
            }
            TypeId::Int
        }
        Expr::Binary { op, left, right } => match op {
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => TypeId::Bool,
            BinOp::Add => {
                let lt = infer_type_from_spanned(left, ctx);
                let rt = infer_type_from_spanned(right, ctx);
                if lt == TypeId::String || rt == TypeId::String {
                    TypeId::String
                } else {
                    // 与 typeck numeric_promote 对齐：long+long → long。
                    mir_numeric_promote(&lt, &rt)
                }
            }
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr => {
                let lt = infer_type_from_spanned(left, ctx);
                let rt = infer_type_from_spanned(right, ctx);
                mir_numeric_promote(&lt, &rt)
            }
        },
        Expr::Unary { op, expr: inner } => match op {
            UnaryOp::Not => TypeId::Bool,
            UnaryOp::Neg => match infer_type_from_spanned(inner, ctx) {
                TypeId::Ref { inner, .. } => *inner,
                other => other,
            },
            UnaryOp::BitNot => match infer_type_from_spanned(inner, ctx) {
                TypeId::Ref { inner, .. } => *inner,
                other => other,
            },
        },
        // Index access (e.g. `arr[i]`): element type must be inferred from the
        // receiver's enumerable element type, otherwise the temp local is
        // allocated as `i32` and codegen emits `alloca i32` for a `ptr` value.
        Expr::Index { receiver, index } => infer_index_elem_type(receiver, &index.node, ctx),
        // RFC 016 v2 M2 / RFC 016 M3：FFI Marshal 装箱 → object 引用类型。
        // temp local 必须是 ptr（object），否则 native call 的 arg 类型不匹配。
        Expr::Box { .. } => TypeId::Object,
        // RFC 016 v2 M2 / RFC 016 M3：FFI Marshal 拆箱 → 目标值类型。
        // value_ty 提供拆箱后的类型（int/double/struct 等）。
        Expr::Unbox { value_ty, .. } => lower_type_name(&value_ty.node),
        // Expr::Cast：目标类型由 ty 字段决定（typeck 已将 object→value 转为 Unbox）。
        // RFC 037 M1: 使用 resolve_instantiated_type_name 保留泛型实参，
        // 使 `(Signal<T>)box` 推断为 `Signal_T`（mangled）而非 `Signal`（裸类名）。
        // 否则 `signal.Set(value)` 会被 mangle 为 `Signal_Set`（无 _T 后缀），
        // 与已注册的 `Signal_T_Set` 符号不匹配，导致链接错误。
        Expr::Cast { ty, .. } => typeck::resolve_instantiated_type_name(&ty.node)
            .map(|name| TypeId::Named(name.into()))
            .unwrap_or_else(|| lower_type_name(&ty.node)),
        // 三元表达式：类型取两个分支的公共类型。
        // 两个分支类型通常一致（typeck 已确保），取非兜底分支的任意一个即可。
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            let then_ty = infer_type_from_spanned(then_branch, ctx);
            if then_ty != TypeId::Int && then_ty != TypeId::Error && then_ty != TypeId::Infer {
                then_ty
            } else {
                let else_ty = infer_type_from_spanned(else_branch, ctx);
                if else_ty != TypeId::Int && else_ty != TypeId::Error && else_ty != TypeId::Infer {
                    else_ty
                } else {
                    TypeId::Int
                }
            }
        }
        // RFC 004 M4：表达式块类型 = tail
        Expr::Block(b) => b
            .tail
            .as_ref()
            .map(|t| infer_type_from_spanned(t, ctx))
            .unwrap_or(TypeId::Void),
        // RFC 009 L2：`?.`/`!.` 整体类型 = access 类型（typeck 语义：`?.` 结果
        // 为 `T?`）。receiver 物化临时 local 需正确类型——若无此分支，`a?.B?.C`
        // 外层 receiver（`a?.B`）的临时 local 会 fallback TypeId::Int，codegen
        // 对 ptr 值 alloca i32 → 类型错配崩溃。
        Expr::NullCond { access } => infer_type_from_spanned(access, ctx),
        Expr::ForceDeref { access } => infer_type_from_spanned(access, ctx),
        _ => TypeId::Int,
    }
}

pub(super) fn lower_type_name(ty: &Type) -> TypeId {
    match ty {
        Type::Named { path, generics } => {
            // RFC 037 M1: 泛型实例化类型必须 mangle 以匹配 codegen 符号表。
            // 否则 `Signal<T> signal = ...` 中 signal 的 local 类型为 `Named("Signal")`，
            // 导致 `signal.Set(value)` 被 mangle 为 `@Signal_Set`（缺少 `_T` 后缀），
            // 与已注册的 `@Signal_T_Set` 符号不匹配，链接失败。
            if !generics.is_empty() {
                if let Some(name) = typeck::resolve_instantiated_type_name(ty) {
                    return TypeId::Named(name.into());
                }
            }
            let name = path.last().map(|s| s.as_str()).unwrap_or("int");
            match name {
                "int" => TypeId::Int,
                "long" => TypeId::Long,
                "short" => TypeId::Short,
                "byte" => TypeId::Byte,
                "char" => TypeId::Char,
                "bool" => TypeId::Bool,
                "string" => TypeId::String,
                "void" => TypeId::Void,
                "float" => TypeId::Float,
                "double" => TypeId::Double,
                "uint" => TypeId::UInt,
                "ulong" => TypeId::ULong,
                "ushort" => TypeId::UShort,
                "sbyte" => TypeId::SByte,
                // 与 typeck check_type 对齐：裸 `Task` ≡ `Task<void>` 内建类型。
                // `typed_block_to_block` 还原后的 try 块局部依赖此映射，避免
                // `Named("Task")` 与 facade 分派分叉。
                "Task" if generics.is_empty() => TypeId::Task {
                    inner: Box::new(TypeId::Void),
                },
                other => TypeId::Named(other.into()),
            }
        }
        Type::Array { inner } => TypeId::Array {
            elem: Box::new(lower_type_name(&inner.node)),
        },
        Type::Nullable { inner } => lower_type_name(&inner.node),
        // RFC 037 M1 配套：`typed_block_to_block` 重建的 if 分支把委托声明还原为
        // `ast::Type::Func`，此处须同理降回 `TypeId::Func`，使 `is_delegate_type`
        // 识别 `Action a` 走 IndirectCall（否则 `a()` 被误判为直接函数调用
        // `call void @a()` → LLVM undefined symbol）。
        Type::Func { params, ret } => TypeId::Func {
            params: params.iter().map(|p| lower_type_name(&p.node)).collect(),
            ret: Box::new(lower_type_name(&ret.node)),
        },
        _ => TypeId::Int,
    }
}

pub(super) fn type_id_name(ty: &TypeId) -> Ident {
    type_id_to_field_name(ty)
}

/// 从 mangled `Func_<p1>_..._<pN>_<ret>` / `Action_<p1>_..._<pN>` 名解析形参类型
/// 列表（与 typeck `demangle_func_type` 对称）。用于为**未标注类型**的 lambda 形参
/// 推断委托形参类型——替代 `lower_lambda_to_fnptr` 的 `TypeId::Int` 默认值，修复
/// `Signal<T>` 单态化中 `double`/`string` 载荷经未标注形参被截断为 i32 的 ABI 缺陷。
///
/// `arity` 为 lambda 形参数；`Func_` 尾部的返回类型据此剥离。嵌套委托无法解析，
/// 返回 `None` 使调用方回退到原有 `Int` 默认行为。
///
/// 复合类型参数（单态化泛型如 `ObservableCollection_int`，本身含 `_`）可通过
/// `is_known` 在类型注册表中识别后按组切分（与 typeck `demangle_func_type_with`
/// 对称）。
pub(super) fn demangle_delegate_params_with(
    name: &str,
    arity: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<Vec<TypeId>> {
    // 单一事实源：委托 typeck 的递归 demangle（支持嵌套 Func/Action 组）。
    // 本地旧实现遇嵌套即弃权（`return None`）→ lambda 形参 expected=None
    // → Int 回退 → object/Func 槽 i32 化闭包 ABI 错位。
    if let Some(TypeId::Func { params, .. }) =
        typeck::demangle_func_type_with(name, arity, is_known)
    {
        return Some(params);
    }
    None
}

/// 把 `_`-分割片段回溯切分为 `count` 个类型组。每组是单个原子（原语/占位符
/// 恒合法），或多原子组成的已注册类型名（如 `ObservableCollection_int`）。
/// 组数约束与注册表识别共同消除歧义。
fn split_type_groups(
    parts: &[&str],
    count: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<Vec<String>> {
    if count == 0 {
        return if parts.is_empty() {
            Some(Vec::new())
        } else {
            None
        };
    }
    if parts.len() < count {
        return None;
    }
    // 每组至少占用 1 个原子；为剩余 count-1 组预留最少原子数。
    let max_atoms = parts.len() - (count - 1);
    for end in 1..=max_atoms {
        let candidate = parts[..end].join("_");
        if end > 1 && !is_known(&candidate) {
            continue;
        }
        if let Some(mut rest) = split_type_groups(&parts[end..], count - 1, is_known) {
            rest.insert(0, candidate);
            return Some(rest);
        }
    }
    None
}

fn delegate_param_type_id(s: &str) -> TypeId {
    match s {
        "int" => TypeId::Int,
        "long" => TypeId::Long,
        "short" => TypeId::Short,
        "byte" => TypeId::Byte,
        "char" => TypeId::Char,
        "float" => TypeId::Float,
        "double" => TypeId::Double,
        "bool" => TypeId::Bool,
        "void" => TypeId::Void,
        "string" => TypeId::String,
        "object" => TypeId::Object,
        other => TypeId::Named(other.into()),
    }
}

/// 从委托 `TypeId` 解析返回类型：`TypeId::Func` 直接取 ret；mangled
/// `Named("Func_...")` 取尾部返回类型组；`Named("Action_...")` → Void；
/// 其余返回 `None`。用于委托局部调用（`fac(x)`）的结果临时 local 类型推断，
/// 避免 `infer_type_from_expr` 回退到 `TypeId::Int` 把返回的 `object` 指针
/// 截断为 i32（ServiceScope 解析 Scoped 服务 0xC0000005 根因）。
pub(super) fn delegate_return_type(ty: &TypeId, is_known: &dyn Fn(&str) -> bool) -> Option<TypeId> {
    let inner = match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    };
    if let TypeId::Named(n) = inner {
        // 单一事实源：mangle 名形态委托 typeck 递归 demangle（嵌套组支持），
        // 取返回类型组；本地旧实现遇嵌套弃权。arity 未知 → None 回溯全组数。
        if n.starts_with("Func_") || n.starts_with("Action_") {
            return typeck::demangle_func_type_depth(n, None, 0, is_known)
                .as_ref()
                .and_then(|f| match f {
                    TypeId::Func { ret, .. } => Some((**ret).clone()),
                    _ => None,
                });
        }
    }
    match inner {
        TypeId::Func { ret, .. } => Some(ret.as_ref().clone()),
        TypeId::Named(n) => {
            let name = n.as_str();
            if name.starts_with("Action_") {
                return Some(TypeId::Void);
            }
            if name.starts_with("Func_") {
                let rest = name.strip_prefix("Func_")?;
                if rest.contains("Func_") || rest.contains("Action_") {
                    return None; // 嵌套委托不支持
                }
                let parts: Vec<&str> = rest.split('_').collect();
                let group_count = parts.len();
                let groups = split_type_groups(&parts, group_count, is_known)?;
                return groups.last().map(|g| delegate_param_type_id(g));
            }
            None
        }
        _ => None,
    }
}

/// 从调用点形参 `TypeId` 解析委托形参类型列表：`TypeId::Func` 直接取 params；
/// mangled 名（`Named("Func_...")` / `Named("Action_...")`）走
/// `demangle_delegate_params_with`（`is_known` 用于识别含 `_` 的复合类型参数）；
/// 其余返回 `None`。
pub(super) fn delegate_params_of(
    ty: &TypeId,
    arity: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<Vec<TypeId>> {
    let inner = match ty {
        TypeId::Nullable { inner } => inner.as_ref(),
        other => other,
    };
    match inner {
        TypeId::Func { params, .. } => Some(params.clone()),
        TypeId::Named(n) => demangle_delegate_params_with(n.as_str(), arity, is_known),
        _ => None,
    }
}

/// 从 `Expression<Func<T1,...,Tn, R>>` 提取 Func 形参类型名列表。
fn expression_func_param_tys(ty: &TypeId) -> Vec<SmolStr> {
    match ty {
        TypeId::Expression { inner } => match inner.as_ref() {
            TypeId::Func { params, .. } => params.iter().map(type_id_name).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// 填充 ExpressionTree 中 Parameter / MemberAccess 的类型名（RFC 022 §9.4.6）。
///
/// `expr_ty` 为 `Expression<Func<...>>` 声明类型时注入未标注 Lambda 形参类型；
/// 成员类型经 TypeRegistry.resolve_field / get_ 属性解析。
pub(super) fn annotate_expression_tree(
    tree: &mut ExpressionTree,
    expr_ty: &TypeId,
    ctx: &LowerCtx,
) {
    let param_tys = expression_func_param_tys(expr_ty);
    let access = access_ctx(ctx);
    tree.annotate_types(&param_tys, |owner, member| {
        let owner_id: Ident = owner.into();
        let member_id: Ident = member.into();
        if let Ok(fty) = ctx.registry.resolve_field(&owner_id, &member_id, &access) {
            return Some(fty);
        }
        let getter: Ident = format!("get_{member}").into();
        ctx.registry
            .resolve_method(&owner_id, &getter, &access)
            .ok()
            .map(|sig| sig.ret)
    });
}

/// Check if a type name (as string) is a non-pointer primitive.
/// Used for compile-time folding of `is` expressions.
pub(super) fn is_primitive_name(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "long"
            | "short"
            | "byte"
            | "char"
            | "uint"
            | "ulong"
            | "ushort"
            | "sbyte"
            | "float"
            | "double"
            | "bool"
    )
}

/// Check if TypeId is a non-pointer primitive type (int/double/bool/float etc).
/// These types have no vtable and rt_obj_isa would crash on them.
pub(super) fn is_primitive_type(ty: &TypeId) -> bool {
    match ty {
        TypeId::Int
        | TypeId::Long
        | TypeId::Short
        | TypeId::Byte
        | TypeId::Char
        | TypeId::UInt
        | TypeId::ULong
        | TypeId::UShort
        | TypeId::SByte
        | TypeId::Float
        | TypeId::Double
        | TypeId::Bool => true,
        TypeId::Named(n) => is_primitive_name(n),
        _ => false,
    }
}
