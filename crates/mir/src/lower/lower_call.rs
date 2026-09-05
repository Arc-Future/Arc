use super::lower_expr::*;
use super::lower_linq::*;
use super::lower_type::*;
use super::*;

/// Func/Action 值调用（局部、实例字段、显式 `this.f` 等）→ `IndirectCall`。
///
/// 须在「裸 Ident → 自由函数 `Call`」与「`recv.field` → 静态方法 `Call`」之前拦截；
/// 否则字段名被 mangle 成 `@_f` 直调，链接失败或半物化 AV（高阶委托半物化缺口）。
pub(super) fn try_lower_delegate_invoke(
    builder: &mut MirBuilder,
    func: &Spanned<Expr>,
    args: &[Spanned<Expr>],
    ctx: &mut LowerCtx,
) -> Option<(Vec<MirStatement>, MirRvalue, TypeId)> {
    let callee_ty = infer_type_from_spanned(func, ctx);
    if !is_delegate_type(&callee_ty) {
        return None;
    }
    // 委托返回类型：结果临时须按真实返回类型建（否则 Void 本地 → codegen 以
    // i32 存储，`_disposer = _callback()` 的 IDisposable 指针被 ptrtoint 截断
    // x64 高位 → 0xC0000005，chord Provide/Revert 链路实测）。
    let ret_ty = delegate_return_type(&callee_ty, &|s| ctx.registry.types.contains_key(s))
        .unwrap_or_else(|| TypeId::Named("object".into()));
    // RFC 039：委托形参为接口时，class 实参须包装为接口胖指针
    //（如 `Action<IServiceCollection>` 收到 `ServiceCollection` 实参）。
    let params = delegate_params_of(&callee_ty, args.len(), &|s| {
        ctx.registry.types.contains_key(s)
    });
    let mut prep = Vec::new();
    let (mut p, func_op) = lower_arg_operand(builder, &func.node, ctx);
    prep.append(&mut p);
    // Field/StaticField 等非 Local 须先物化到临时，便于 codegen
    // `closure_locals` 追踪与 IndirectCall ABI。
    let func_local = match func_op {
        MirOperand::Local(id) => id,
        other => {
            let id = builder.fresh_local(&"_dlg".into(), callee_ty, ctx.locals);
            prep.push(MirStatement::Assign {
                place: id,
                rvalue: MirRvalue::Use(other),
            });
            id
        }
    };
    let mut call_args = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
        prep.append(&mut p);
        let op = if let Some(pt) = params.as_ref().and_then(|ps| ps.get(i)) {
            let arg_ty = type_name_from_operand(&op, &a.node, ctx);
            maybe_box_iface(op, &arg_ty, pt, ctx)
        } else {
            op
        };
        call_args.push(op);
    }
    Some((
        prep,
        MirRvalue::IndirectCall {
            func: MirOperand::Local(func_local),
            args: call_args,
        },
        ret_ty,
    ))
}

/// string.Split 重载分派名（供 codegen）：
/// - `Split` — 单分隔符 / +Options
/// - `SplitMulti` — params char / char[]
/// - `SplitMultiOpts` — 多分隔符 + Options
/// - `SplitCount` — (sep, count, options)
/// - `SplitMultiCount` — (char[], count, options)
fn rewrite_string_split_method(
    recv_ty: &Ident,
    method: &Ident,
    args: &[Spanned<Expr>],
    ctx: &LowerCtx,
) -> String {
    if recv_ty.as_str() != "string" || method.as_str() != "Split" {
        return method.to_string();
    }
    let tys: Vec<TypeId> = args
        .iter()
        .map(|a| infer_type_from_spanned(a, ctx))
        .collect();
    let is_opts =
        |ty: &TypeId| matches!(ty, TypeId::Named(n) if n.as_str() == "StringSplitOptions");
    let is_char = |ty: &TypeId| matches!(ty, TypeId::Char);
    let is_char_arr =
        |ty: &TypeId| matches!(ty, TypeId::Array { elem } if matches!(elem.as_ref(), TypeId::Char));
    let is_sep = |ty: &TypeId| matches!(ty, TypeId::String | TypeId::Char) || is_char_arr(ty);
    match tys.as_slice() {
        [a] if is_char_arr(a) => "SplitMulti".into(),
        [_, b] if is_opts(b) && is_char_arr(&tys[0]) => "SplitMultiOpts".into(),
        [_, b] if is_opts(b) => "Split".into(),
        chars if chars.len() >= 2 && chars.iter().all(&is_char) => "SplitMulti".into(),
        chars
            if chars.len() >= 3
                && is_opts(chars.last().unwrap())
                && chars[..chars.len() - 1].iter().all(is_char) =>
        {
            "SplitMultiOpts".into()
        }
        [a, mid, last] if is_opts(last) && is_sep(a) && matches!(mid, TypeId::Int) => {
            if is_char_arr(a) {
                "SplitMultiCount".into()
            } else {
                "SplitCount".into()
            }
        }
        _ => "Split".into(),
    }
}

/// RFC 005：按调用点 params 标注把实参切分为「固定前缀」与「尾随可变实参」。
fn split_params_args<'a>(
    args: &'a [Spanned<Expr>],
    params_span: Option<&ParamsSpanInfo>,
) -> (&'a [Spanned<Expr>], &'a [Spanned<Expr>]) {
    match params_span {
        Some(info) => (&args[..info.fixed], &args[info.fixed..]),
        None => (args, &[]),
    }
}

/// RFC 005 单一物化点：把调用点的尾随可变实参收集为栈 span。
///
/// 这是**唯一**把 params 尾随实参打包为 `MirRvalue::SpanFromStack` 的地方——
/// 所有调用形态（builtin 静态 / 用户方法 / 扩展 / 泛型 / 自由函数）都经
/// `lower_call_args` / `method_call_rvalue_with_prep` 汇聚到这里。物化到临时
/// local 后作为末位实参 operand 传入，codegen `emit_span_from_stack` 发射
/// `{ptr,len}` 胖指针。
///
/// 单尾随实参且本身已是 Span/ROS（C# 语义，如 `this.Sum(existingSpan)`）时
/// 直接透传，不二次打包。
pub(super) fn materialize_params_span(
    builder: &mut MirBuilder,
    info: &ParamsSpanInfo,
    trailing: &[Spanned<Expr>],
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    if trailing.len() == 1 {
        let ty = infer_type_from_spanned(&trailing[0], ctx);
        if matches!(&ty, TypeId::Span { .. }) {
            return lower_arg_operand(builder, &trailing[0].node, ctx);
        }
    }
    let mut prep = Vec::new();
    let mut ops = Vec::with_capacity(trailing.len());
    for e in trailing {
        let (mut p, op) = lower_arg_operand(builder, &e.node, ctx);
        prep.append(&mut p);
        ops.push(op);
    }
    let span_ty = TypeId::Span {
        elem: Box::new(info.elem.clone()),
        mutable: info.mutable,
    };
    let tmp = builder.fresh_local(&"_params_span".into(), span_ty, ctx.locals);
    prep.push(MirStatement::Assign {
        place: tmp,
        rvalue: MirRvalue::SpanFromStack {
            elements: ops,
            elem_type: info.elem.clone(),
            mutable: info.mutable,
        },
    });
    (prep, MirOperand::Local(tmp))
}

pub(super) fn lower_call_args(
    builder: &mut MirBuilder,
    fname: &Ident,
    args: &[Spanned<Expr>],
    params_span: Option<&ParamsSpanInfo>,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, Vec<MirOperand>) {
    let sig = ctx.fn_sigs.get(fname.as_str());
    let (fixed_args, trailing) = split_params_args(args, params_span);
    let mut prep = Vec::new();
    let mut out = Vec::new();
    for (i, a) in fixed_args.iter().enumerate() {
        // 形参为 Func/Action 时解析委托形参类型，供未标注 lambda 形参推断
        //（否则 `TypeId::Int` 默认值会截断 double/string 载荷）。
        let expected_lambda: Option<Vec<TypeId>> = match (&a.node, sig) {
            (Expr::Lambda(l), Some((params, _))) => params.get(i).and_then(|p| {
                delegate_params_of(p, l.params.len(), &|s| ctx.registry.types.contains_key(s))
            }),
            _ => None,
        };
        // 委托契约返回类型：lambda 实参须按形参 Func/Action 的返回类型提升
        //（接口返回 → 闭包产出 fat pointer，见 lower_lambda_to_fnptr）。
        let expected_ret: Option<TypeId> = match (&a.node, sig) {
            (Expr::Lambda(_), Some((params, _))) => params.get(i).and_then(|p| {
                if lower_type::is_delegate_type(p) {
                    lower_type::delegate_return_type(p, &|s| ctx.registry.types.contains_key(s))
                } else {
                    None
                }
            }),
            _ => None,
        };
        let (mut stmts, op) = lower_arg_operand_with_expected(
            builder,
            &a.node,
            ctx,
            expected_lambda.as_deref(),
            expected_ret.as_ref(),
        );
        prep.append(&mut stmts);
        if let Some((params, _)) = sig {
            if let Some(param_ty) = params.get(i) {
                let arg_ty = type_name_from_operand(&op, &a.node, ctx);
                out.push(maybe_box_iface(op, &arg_ty, param_ty, ctx));
                continue;
            }
        }
        out.push(op);
    }
    if let Some(info) = params_span {
        let (mut p, span_op) = materialize_params_span(builder, info, trailing, ctx);
        prep.append(&mut p);
        out.push(span_op);
    }
    (prep, out)
}

pub(super) fn lower_arg_operand(
    builder: &mut MirBuilder,
    expr: &Expr,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    lower_arg_operand_with_expected(builder, expr, ctx, None, None)
}

/// `lower_arg_operand` 的扩展变体：`expected_lambda` 携带调用点形参类型解析出的
/// 委托形参类型（见 `demangle_delegate_params_with`），供未标注类型的 lambda 形参
/// 推断实际类型——替代 `TypeId::Int` 默认值，修复 `Signal<T>` 单态化中
/// `double`/`string` 载荷经 `(_, newValue) => handler(newValue)` 包装 lambda
/// 被截断为 i32 的 ABI 缺陷（C1/C2 同一根因）。
pub(super) fn lower_arg_operand_with_expected(
    builder: &mut MirBuilder,
    expr: &Expr,
    ctx: &mut LowerCtx,
    expected_lambda: Option<&[TypeId]>,
    expected_ret: Option<&TypeId>,
) -> (Vec<MirStatement>, MirOperand) {
    // 裸自定义属性 Ident（如 `Value`）→ `this.Value`，复用下方 Field 访问器分派。
    // typeck 在复合表达式（Binary 等）中可能未重写子树；与 C# 实例成员查找对齐。
    if let Expr::Ident(name) = expr {
        if let Some(owner) = ctx.owner.clone() {
            if is_custom_accessor_property(ctx.registry, owner.as_str(), name.as_str()) {
                let field_expr = Expr::Field {
                    receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                    field: name.clone(),
                };
                return lower_arg_operand(builder, &field_expr, ctx);
            }
        }
    }
    if matches!(
        expr,
        Expr::IntLit(_)
            | Expr::BoolLit(_)
            | Expr::StringLit(_)
            | Expr::Ident(_)
            | Expr::This
            // CD-15/D4：`base` 与 `this` 同对象，按简单操作数直接物化。
            | Expr::Base
            | Expr::RefArg { .. }
            | Expr::Null
            // RFC 018 M2 step 2: `typeof(T)` 编译期常量，直接 inline 为
            // `MirOperand::TypeId`，避免物化到 local 丢失编译期信息
            // （codegen `try_emit_typeinfo_from_typeof` 依赖此 operand 形态）。
            | Expr::TypeOf(_)
    ) {
        return (vec![], operand_from_expr(expr, ctx));
    }
    if let Expr::Lambda(l) = expr {
        let op = builder.lower_lambda_to_fnptr(l, ctx, expected_lambda, expected_ret);
        return (vec![], op);
    }
    // Enum variant access (e.g. `ExpressionType.Unary`) and other const
    // operands must be resolved before the general `Expr::Field` path,
    // mirroring `operand_from_expr`'s opening `enum_variant_operand` check.
    // Without this, enum variants fall through to the direct-field-access
    // path and produce `ptr 0` in IR.
    if let Some(op) = enum_variant_operand(expr, ctx.registry) {
        return (vec![], op);
    }
    // RFC 004 M1：variant 构造（`Value.Null` / `Value.Int(42)`）必须走 rvalue 路径
    // 物化到临时 local，不能走 `operand_from_expr` 的字段访问路径（会把 case 名
    // 误判为 class 字段）。此处提前拦截，路由到 fallback 的 rvalue 物化。
    if variant_construct_rvalue(expr, ctx).is_some() {
        let (mut prep, rvalue) = lower_expr_to_rvalue_with_binary(expr, builder, ctx);
        let ty = infer_type_from_expr(expr, ctx);
        let tmp = builder.fresh_local(&"_variant_arg".into(), ty, ctx.locals);
        prep.push(MirStatement::Assign { place: tmp, rvalue });
        return (prep, MirOperand::Local(tmp));
    }
    if let Expr::Field { receiver, field } = expr {
        // RFC 009 D3：`soaArr[i].field` → `SoaFieldGet`，避免 IndexGet 物化 AoS 临时
        // 后 codegen 无法回溯 SoA 字段数组。
        if let Expr::Index {
            receiver: arr,
            index,
        } = &receiver.node
        {
            if let Some(struct_name) = soa_array_elem_struct(arr, ctx) {
                let (mut prep, arr_op) = lower_arg_operand(builder, &arr.node, ctx);
                let (prep_i, idx_op) = lower_arg_operand(builder, &index.node, ctx);
                prep.extend(prep_i);
                let ty = infer_type_from_expr(expr, ctx);
                let tmp = builder.fresh_local(&"_soa_field".into(), ty, ctx.locals);
                prep.push(MirStatement::Assign {
                    place: tmp,
                    rvalue: MirRvalue::SoaFieldGet {
                        array: arr_op,
                        index: idx_op,
                        class: struct_name,
                        field: field.to_string(),
                    },
                });
                return (prep, MirOperand::Local(tmp));
            }
        }
        let recv_class = class_from_expr(&receiver.node, ctx);
        let recv_class_ident: Ident = recv_class.as_str().into();
        if let Some(op) = try_const_operand(&recv_class_ident, field, ctx) {
            return (vec![], op);
        }
        // RFC 006 M3：跨类静态字段访问（`Config.DefaultValue`）——
        // const 已被 try_const_operand 折叠，此处处理非 const 静态字段。
        // 必须在 receiver 物化之前拦截，否则 `lower_arg_operand(receiver)`
        // 会对 `Expr::Ident("Config")` 触发 `operand_from_expr` 的 locals 查找，
        // 而 Config 是类名非变量，导致 "unresolved ident" panic。
        // 与 `operand_from_expr` 的 Expr::Field 分支（lower_expr.rs:976）对齐。
        if ctx.is_static_field_of(&recv_class_ident, field) {
            return (
                vec![],
                MirOperand::StaticField {
                    class: recv_class,
                    field: field.to_string(),
                },
            );
        }
        // RFC 004 M1：基元 `T.Zero`/`T.One` 静态抽象属性（源码形 Call，见下）。
        // facade 类静态属性不在此列举——`user_type_static_property_func` 已统一
        // 还原源码形 `Class.Prop` 供 codegen 分派。
        if let Expr::Ident(name) = &receiver.node {
            if matches!(field.as_str(), "Zero" | "One") && is_primitive_numeric_type(name) {
                let ty = infer_type_from_expr(expr, ctx);
                let tmp = builder.fresh_local(&"_prim_prop".into(), ty, ctx.locals);
                return (
                    vec![MirStatement::Assign {
                        place: tmp,
                        rvalue: MirRvalue::Call {
                            func: format!("{name}.{field}"),
                            args: vec![],
                        },
                    }],
                    MirOperand::Local(tmp),
                );
            }
        }
        // RFC 004 M2：用户类型静态属性访问（如 `Vector2.Zero`）。
        // 优先于实例属性路径与直接字段访问，避免 receiver 被物化为
        // `ConstInt(0)` 充当 `this`。静态 getter 无 `this` 参数，
        // 降级为 `MirRvalue::Call`（无 receiver）。
        if let Some(func) = user_type_static_property_func(&receiver.node, field, ctx) {
            let ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_static_prop".into(), ty, ctx.locals);
            return (
                vec![MirStatement::Assign {
                    place: tmp,
                    rvalue: MirRvalue::Call { func, args: vec![] },
                }],
                MirOperand::Local(tmp),
            );
        }
        // CTS facade (RFC 009 M4): CTS 属性访问转为 MethodCall（与 lower_expr.rs 对齐）。
        // stub 将 Token/IsCancellationRequested 注册为 auto-property（含 backing field），
        // 导致 is_custom_accessor_property 返回 false；若不在此拦截，
        // `Task.Delay(ms, cts.Token)` 的 CT 参数会走直接字段访问路径，
        // 生成 getelementptr+load i32 而非 rt_cts_* ABI。
        if recv_class == "CancellationTokenSource" && cts_facade_instance_property(field.as_str()) {
            let (mut prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
            let getter = format!("get_{field}");
            let rvalue = MirRvalue::MethodCall {
                receiver: recv_op,
                method: getter,
                args: vec![],
                receiver_type: recv_class,
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            };
            let ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_cts_prop".into(), ty, ctx.locals);
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            return (prep, MirOperand::Local(tmp));
        }
        // Task facade (RFC 009 M1/M5.7): Task<T> 实例属性访问转为 MethodCall。
        // 泛型单态化后局部类型是 mangled 名 `Task_<T>`（Named 而非 TypeId::Task），
        // recv_class 为 "Task_UserDto"；若不在此拦截，直接字段访问生成
        // MirOperand::Field { class: "Task_UserDto" }，codegen emit_field_get 仅匹配
        // class == "Task" → 落到偏移 16 的 i32 load + inttoptr，运行时 AV
        // （web_mb_host_route_bind_e2e `task.Result` 实测）。
        if (recv_class == "Task" || recv_class.starts_with("Task_"))
            && task_facade_instance_property(field.as_str())
        {
            let (mut prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
            let getter = format!("get_{field}");
            let rvalue = MirRvalue::MethodCall {
                receiver: recv_op,
                method: getter,
                args: vec![],
                receiver_type: "Task".into(),
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            };
            let ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_task_prop".into(), ty, ctx.locals);
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            return (prep, MirOperand::Local(tmp));
        }
        if ctx.registry.is_interface(&recv_class_ident) {
            let (mut prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
            let getter = format!("get_{field}");
            let rvalue = MirRvalue::MethodCall {
                receiver: recv_op,
                method: getter,
                args: vec![],
                receiver_type: recv_class,
                impl_class: None,
                target_fn: None,
                is_virtual: false,
                params: vec![],
            };
            let ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_prop".into(), ty, ctx.locals);
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            return (prep, MirOperand::Local(tmp));
        }
        if is_custom_accessor_property(ctx.registry, &recv_class, field) {
            let (mut prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
            let getter = format!("get_{field}");
            let (impl_class, target_fn) = resolve_method_target(
                ctx.registry,
                &recv_class_ident,
                &getter.clone().into(),
                ctx.owner.clone(),
            );
            let is_virtual = is_virtual_member(ctx.layouts, &recv_class, &getter, &[]);
            let rvalue = MirRvalue::MethodCall {
                receiver: recv_op,
                method: getter,
                args: vec![],
                receiver_type: recv_class.clone(),
                impl_class,
                target_fn,
                is_virtual,
                params: vec![],
            };
            let ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_prop".into(), ty, ctx.locals);
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            return (prep, MirOperand::Local(tmp));
        }
        // Direct field access — lower the receiver first so that complex
        // receivers (e.g. `list[0].Value` where the receiver is a
        // MethodCall) are properly lowered to a temp local instead of
        // collapsing to `ConstInt(0)` via `operand_from_expr`.
        let (prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
        // For complex receivers, `class_from_expr` returns "unknown".
        // Resolve the class from the lowered operand's type instead.
        let field_class = if recv_class == "unknown" {
            type_name_from_operand(&recv_op, &receiver.node, ctx).to_string()
        } else {
            recv_class
        };
        let field_op = MirOperand::Field {
            object: Box::new(recv_op),
            class: field_class,
            field: field.to_string(),
        };
        return (prep, field_op);
    }
    if let Expr::Unary { op, expr: inner } = expr {
        let (mut prep, inner_op) = lower_expr_to_operand(builder, &inner.node, ctx);
        let result_ty = match op {
            UnaryOp::Not => TypeId::Bool,
            UnaryOp::Neg => match infer_type_from_spanned(inner, ctx) {
                TypeId::Ref { inner, .. } => *inner,
                other => other,
            },
            UnaryOp::BitNot => match infer_type_from_spanned(inner, ctx) {
                TypeId::Ref { inner, .. } => *inner,
                other => other,
            },
        };
        let tmp = builder.fresh_local(&"_unary".into(), result_ty, ctx.locals);
        let rvalue = match op {
            UnaryOp::Not => MirRvalue::Binary {
                op: BinOp::Eq,
                left: inner_op,
                right: MirOperand::ConstBool(false),
            },
            UnaryOp::Neg => MirRvalue::Binary {
                op: BinOp::Sub,
                left: MirOperand::ConstInt(0),
                right: inner_op,
            },
            UnaryOp::BitNot => MirRvalue::Binary {
                op: BinOp::BitXor,
                left: MirOperand::ConstInt(-1),
                right: inner_op,
            },
        };
        prep.push(MirStatement::Assign { place: tmp, rvalue });
        return (prep, MirOperand::Local(tmp));
    }
    let (mut prep, rvalue) = lower_expr_to_rvalue_with_binary(expr, builder, ctx);
    // `(IFoo)classExpr` 实参：Cast 在此路径折叠为裸对象指针，若直接以接口
    // 形参类型透传，被调方按 `{ptr,ptr}` 胖指针解引用 → ACCESS_VIOLATION
    // （web_mb_host_route_bind_e2e `(IJsonDeserializable)req` 实测；json_xml_e2e
    // 注「concrete 直传可编译但静默未填充」同根因）。与 lower.rs Let/FieldSet
    // 装箱路径一致：源类型临时 + `iface_wrap_rvalue` 物化接口胖指针。
    if let Expr::Cast {
        ty: cast_ty,
        expr: inner,
    } = expr
    {
        if let TypeId::Named(iface_name) = lower_type_name(&cast_ty.node) {
            if ctx.registry.is_interface(&iface_name) {
                let src_ty = infer_type_from_spanned(inner, ctx);
                let stmp = builder.fresh_local(&"_arg".into(), src_ty.clone(), ctx.locals);
                prep.push(MirStatement::Assign {
                    place: stmp,
                    rvalue,
                });
                if let Some(wrap) =
                    iface_wrap_rvalue(ctx.registry, &src_ty, &iface_name, MirOperand::Local(stmp))
                {
                    let itmp = builder.fresh_local(
                        &"_iface".into(),
                        lower_type_name(&cast_ty.node),
                        ctx.locals,
                    );
                    prep.push(MirStatement::Assign {
                        place: itmp,
                        rvalue: wrap,
                    });
                    return (prep, MirOperand::Local(itmp));
                }
                return (prep, MirOperand::Local(stmp));
            }
        }
    }
    // RFC 017 #16：Block 脱糖结束时 pop scope，`infer_type_from_expr(tail Ident)`
    // 会回退成 Int；仅对 Block 从已物化 Local 取真实类型。其它表达式（含 Cast→接口）
    // 仍走 infer，避免把源类类型误当成接口形参类型而直调 `Tests_Setup`。
    let ty = if matches!(expr, Expr::Block(_)) {
        type_of_materialized_rvalue(&rvalue, expr, ctx)
    } else {
        infer_type_from_expr(expr, ctx)
    };
    let tmp = builder.fresh_local(&"_arg".into(), ty, ctx.locals);
    prep.push(MirStatement::Assign { place: tmp, rvalue });
    (prep, MirOperand::Local(tmp))
}

fn type_of_materialized_rvalue(rvalue: &MirRvalue, expr: &Expr, ctx: &LowerCtx) -> TypeId {
    if let MirRvalue::Use(MirOperand::Local(id)) = rvalue {
        if let Some((_, ty)) = ctx.locals.get(id) {
            return ty.clone();
        }
    }
    infer_type_from_expr(expr, ctx)
}

pub(super) fn lower_call_args_simple(
    fname: &Ident,
    args: &[Spanned<Expr>],
    params_span: Option<&ParamsSpanInfo>,
    ctx: &LowerCtx,
) -> Vec<MirOperand> {
    // RFC 005：params 调用须物化 SpanFromStack（需 prep 语句），simple 路径（无 prep）
    // 无法承载 → 显式不可达。上游 `Expr::Call` 含 params 标注时必须走 with-prep 路径。
    if params_span.is_some() {
        panic!(
            "MIR lower: call `{fname}` is a `params Span` call in a simple-operand context; \
             it requires prep (SpanFromStack materialization) — use lower_call_args / \
             with-prep lowering"
        );
    }
    // 仅允许已是 operand 形态的实参。复杂实参必须走 `lower_call_args`（带 prep）。
    // 禁止静默 ConstInt(0)/ConstNull（前言定位公理 / 4.5）。
    for a in args {
        if !is_simple_operand_expr(&a.node, ctx) {
            panic!(
                "MIR lower: call `{fname}` argument cannot be materialized without prep \
                 (silent 0/null is forbidden); use lower_call_args / lower_arg_operand"
            );
        }
    }
    let sig = ctx.fn_sigs.get(fname.as_str());
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let op = operand_from_expr(&a.node, ctx);
            if let Some((params, _)) = sig {
                if let Some(param_ty) = params.get(i) {
                    let arg_ty = type_name_from_operand(&op, &a.node, ctx);
                    return maybe_box_iface(op, &arg_ty, param_ty, ctx);
                }
            }
            op
        })
        .collect()
}

fn is_simple_operand_expr(expr: &Expr, ctx: &LowerCtx) -> bool {
    if enum_variant_operand(expr, ctx.registry).is_some() {
        return true;
    }
    match expr {
        Expr::IntLit(_)
        | Expr::BoolLit(_)
        | Expr::FloatLit(_)
        | Expr::StringLit(_)
        | Expr::CharLit(_)
        | Expr::Ident(_)
        | Expr::This
        | Expr::Null
        | Expr::TypeOf(_)
        | Expr::Default { .. } => true,
        Expr::RefArg { expr: inner, .. }
        | Expr::Cast { expr: inner, .. }
        | Expr::Unary { expr: inner, .. } => is_simple_operand_expr(&inner.node, ctx),
        Expr::Field { receiver, .. } => is_simple_operand_expr(&receiver.node, ctx),
        _ => false,
    }
}

pub(super) fn maybe_box_iface(
    op: MirOperand,
    arg_class: &Ident,
    param_ty: &TypeId,
    ctx: &LowerCtx,
) -> MirOperand {
    let TypeId::Named(iface) = param_ty else {
        return op;
    };
    if !ctx.registry.is_interface(iface) {
        return op;
    }
    if let Some(impl_cls) = ctx.registry.interface_impl_class(arg_class, iface) {
        let itable_iface = ctx.registry.interface_itable_name(&impl_cls, iface);
        return MirOperand::Iface {
            object: Box::new(op),
            class: impl_cls.to_string(),
            iface: itable_iface.to_string(),
        };
    }
    op
}

/// raw/λ（模板克隆体 raw 重降级）调用点补齐 string→object 实参装箱。
///
/// typed 路径由 typeck 在 AST 插入 `Expr::Box`（`MirRvalue::Box` → codegen
/// `rt_string_box`，null 保留）；raw 体无此节点——`object`/`object?` 形参直收
/// rodata/堆裸串。裸串无 ArcHeader：入 object 槽后按对象消费（cast/vtable 判别）
/// 或未来参与 ARC 计数即损坏（Reload 路径 0xC0000005 实证：inc 把字符串内容当
/// refcount 原子写）。此处按形参类型补装箱；已带 Box/Unbox 节点或静态类型非
/// string 时不重复装箱（null 由 emit_box 保留为 null）。
pub(super) fn maybe_box_string_to_object(
    builder: &mut MirBuilder,
    arg: &Spanned<Expr>,
    op: MirOperand,
    param_ty: Option<&str>,
    ctx: &mut LowerCtx,
    prep: &mut Vec<MirStatement>,
) -> MirOperand {
    let Some(pt) = param_ty else {
        return op;
    };
    if pt != "object" && pt != "object?" {
        return op;
    }
    if matches!(arg.node, Expr::Box { .. } | Expr::Unbox { .. }) {
        return op;
    }
    if infer_type_from_spanned(arg, ctx) != TypeId::String {
        return op;
    }
    let tmp = builder.fresh_local(&"_obj_arg".into(), TypeId::Object, ctx.locals);
    prep.push(MirStatement::Assign {
        place: tmp,
        rvalue: MirRvalue::Box {
            src: op,
            src_ty: TypeId::String,
        },
    });
    MirOperand::Local(tmp)
}

/// 泛型方法实例化（`g.M<int>(…)`）调用目标的符号基底。
///
/// 与静态路径 `user_type_static_method_sig` 一致：基底必须取**模板** link 名
///（形参保留泛型占位符，如 `Class::M_T`），再追加 `__{type_args}`。若误用
/// **替换后**签名作基底（`Class::M_int`），则 (1) 与同名非泛型重载 `M(int)`
/// 的 mangle `Class::M_int` 撞名；(2) `split_mono_name`/`get_method_generics`
/// 反查不到模板 → mono body 不克隆 → LLVM `use of undefined value`。
/// 模板唯一匹配失败时回退到替换后基底（与静态路径回退语义一致，不改变
/// 既有非泛型/非重载场景的符号）。
fn generic_instantiation_target(
    registry: &TypeRegistry,
    declaring: &Ident,
    method: &Ident,
    sig: &typeck::OopMethodSig,
    arg_types: &[Ident],
    type_arg_names: &[Ident],
    ctx: &AccessContext,
) -> String {
    let base = registry
        .method_generic_template_link_name(declaring, method, arg_types, type_arg_names, ctx)
        .or_else(|| {
            // 模板唯一匹配（替换后形参 vs 实参名）可能因未绑定 lambda
            //（`Func_Infer_*`）或调用面类型差异失配——按「泛型参数个数 +
            // 实参个数」窄匹配模板本体（实参类型无关），唯一命中即用其
            // **占位符** link 基底（如 `Provide_Func_T`），避免回退到替换后
            // 签名基底（`Provide_Func_Greeter`）与 mono body 命名分叉
            //（arc-prune-001：call 引用 `Provide_Func_Greeter__Greeter`，
            // 模板克隆体名为 `Provide_Func_T__Greeter`，符号缺失）。
            registry.method_generic_template_link_name_by_arity(
                declaring,
                method,
                arg_types.len(),
                type_arg_names.len(),
                ctx,
            )
        })
        .unwrap_or_else(|| registry.method_link_name_for(declaring, sig));
    format!("{base}__{}", type_arg_names.join("__"))
}

/// CD-15/D4：`base.M()` 的静态分派目标——**直接基类**对 M 的实现。
///
/// C# base 语义：非虚调用，跳过派生类覆写。`recv_ty` 已由调用方解析为直接基类；
/// 沿其 `method_impl`（名+形参签名键）取实现类：基类若有 override 命中覆写体
///（三层链 `A→B→C` 中 `C` 内 `base.Greet()` 命中 B 的覆写），未覆写则继承原声明
/// 类（命中 A）。`is_virtual` 由调用方置 false，codegen 据此直接调用
/// `@Impl_Greet` 而非 vtable 分派。泛型 `base.M<int>(…)` 经
/// [`generic_instantiation_target`] 取模板基底，避免与定参重载撞名。
fn base_call_target(
    registry: &TypeRegistry,
    layouts: &ProgramLayouts,
    recv_ty: &Ident,
    method: &Ident,
    declaring: &Ident,
    sig: &typeck::OopMethodSig,
    type_args: &[ast::Type],
    arg_types: &[Ident],
    ctx: &AccessContext,
) -> (Option<String>, Option<String>) {
    let params: Vec<Ident> = sig.params.iter().map(|p| p.ty.clone()).collect();
    let impl_class = layouts
        .classes
        .get(recv_ty.as_str())
        .and_then(|c| c.method_impl.get(&(method.clone(), params)).cloned())
        .unwrap_or_else(|| declaring.clone());
    let target = if !type_args.is_empty() {
        let type_arg_names: Vec<Ident> = type_args
            .iter()
            .map(|t| type_id_name(&lower_type_name(t)))
            .collect();
        generic_instantiation_target(
            registry,
            &impl_class,
            method,
            sig,
            arg_types,
            &type_arg_names,
            ctx,
        )
    } else {
        registry.method_link_name_for(&impl_class, sig)
    };
    (Some(impl_class.to_string()), Some(target))
}

pub(super) fn method_call_rvalue(
    builder: &mut MirBuilder,
    receiver: &Spanned<Expr>,
    method: &Ident,
    args: &[Spanned<Expr>],
    type_args: &[ast::Type],
    params_span: Option<&ParamsSpanInfo>,
    ctx: &LowerCtx,
    recv: MirOperand,
    recv_ty: &Ident,
) -> MirRvalue {
    // RFC 005：params 调用须物化 SpanFromStack（需 prep 语句），simple 路径（无 prep）
    // 无法承载 → 显式不可达。上游 `Expr::MethodCall` 含 params 标注时必须走
    // `method_call_rvalue_with_prep`。
    if params_span.is_some() {
        panic!(
            "MIR lower: params method call `{method}` in a simple-operand context requires \
             with-prep lowering (SpanFromStack materialization needs prep statements)"
        );
    }
    let arg_types: Vec<Ident> = args
        .iter()
        .map(|a| type_id_name(&infer_type_from_spanned(a, ctx)))
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
        .map(|t| type_id_name(&lower_type_name(t)))
        .collect();
    // RFC 005 M2b λ 对齐（typeck 同阶梯）：strict 重载解析无法匹配未绑定
    // lambda（实参推断为 `Func_Infer_*`，与形参 mangle 名不严格相等）时，
    // 先按 λ 软匹配（元数 + Func/Infer 兼容）回落实例重载——否则 MIR 会
    // 跳过实例候选直接选中同名扩展方法（`app.InjectReactive([...], ctx => …)`
    // 错绑 `ChordContextExtensions::InjectReactive` 的 string 形参），或
    // 落到 declaring 首签名/替换后基底——与 typeck 已校验的实例绑定分叉，
    // λ 形参类型（Func 槽解构）随之丢失。
    // 仅无显式 type_args 时启用：显式泛型实参下实例泛型候选由 strict
    // type-args 解析（含 λ 返回具体类型）或模板匹配路径处理，soft 不得
    // 抢先命中非泛型实例重载（`On<string>` 会错绑实例 `On(string, …)`）。
    let has_lambda = args.iter().any(|a| matches!(a.node, Expr::Lambda(_)));
    let resolved = {
        let strict = if !type_args.is_empty() {
            ctx.registry.resolve_method_with_type_args(
                recv_ty,
                method,
                &arg_types,
                &type_arg_names,
                &overload_ctx,
            )
        } else {
            ctx.registry
                .resolve_method_overload(recv_ty, method, &arg_types, &overload_ctx)
        };
        if strict.is_err() && has_lambda && type_args.is_empty() {
            strict.or_else(|_| {
                ctx.registry.resolve_method_overload_lambda_soft(
                    recv_ty,
                    method,
                    &arg_types,
                    &overload_ctx,
                )
            })
        } else {
            strict
        }
    };
    // Capture formal parameter types before `resolved` is consumed by the
    // if-let below. These are used to wrap class-typed arguments in interface
    // fat pointers (MirOperand::Iface) when the parameter type is an interface
    // — e.g. `expr.EvalBool(row)` where `row: DataRow` is passed as
    // `IEvalContext`. Without this, codegen passes a bare object pointer
    // where the callee expects a { obj, vtable } fat pointer, causing ABI
    // mismatch and runtime crashes.
    //
    // 严格重载失败（lambda 实参推断为 `Func_Infer_*` 与单态化形参
    // `Func_double_*` 不严格匹配）时回退到按名查找的声明类签名——与下方
    // `impl_class`/`target_fn` 的回退路径一致——仍能提供 Func/Action 形参
    // 类型供未标注 lambda 形参推断（否则 Signal<T> 单态化包装 lambda 的
    // double/string 载荷被截断为 i32）。
    let param_types: Vec<Ident> = resolved
        .as_ref()
        .ok()
        .map(|(_, sig)| sig.params.iter().map(|p| p.ty.clone()).collect())
        .or_else(|| {
            ctx.registry
                .resolve_method_with_declaring(recv_ty, method, &overload_ctx)
                .ok()
                .map(|(_, sig)| sig.params.iter().map(|p| p.ty.clone()).collect())
        })
        .unwrap_or_default();
    // Lambda 实参：从形参 Func/Action 类型解析委托形参类型，供未标注形参推断
    //（否则 `TypeId::Int` 默认值截断 double/string 载荷——Signal<T> 单态化
    // `(_, newValue) => handler(newValue)` 包装 lambda ABI 缺陷）。
    let expected_lambda_params: Vec<Option<Vec<TypeId>>> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if let Expr::Lambda(l) = &a.node {
                param_types.get(i).and_then(|p| {
                    demangle_delegate_params_with(p.as_str(), l.params.len(), &|s| {
                        ctx.registry.types.contains_key(s)
                    })
                })
            } else {
                None
            }
        })
        .collect();
    // Lambda 实参的委托契约返回类型（`Func<R>` 形参名 → R；接口 R 时闭包
    // 须产出 fat pointer，见 lower_lambda_to_fnptr）。
    let expected_lambda_rets: Vec<Option<TypeId>> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if let Expr::Lambda(_) = &a.node {
                param_types.get(i).and_then(|p| {
                    let pty = TypeId::Named(p.clone());
                    if lower_type::is_delegate_type(&pty) {
                        lower_type::delegate_return_type(&pty, &|s| {
                            ctx.registry.types.contains_key(s)
                        })
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .collect();
    let lower_arg = |b: &mut MirBuilder, a: &Spanned<Expr>, i: usize| -> MirOperand {
        if let Expr::Lambda(l) = &a.node {
            b.lower_lambda_to_fnptr(
                l,
                ctx,
                expected_lambda_params[i].as_deref(),
                expected_lambda_rets[i].as_ref(),
            )
        } else {
            operand_from_expr(&a.node, ctx)
        }
    };
    // RFC 027 M5 / RFC 007 M2a：primitive 实例有参 ToString——
    // `x.ToString(format)` / `x.ToString(format, provider)` 降级为静态调用
    // `double.ToString(x, format, provider)`（receiver 作为首参并入 args）。
    // 无参 `x.ToString()` 走下方 `builtin_static_method`（receiver 命中类名），
    // 但字面量 receiver（如 `(1234.5).ToString(...)`）无法命中类名，故在此
    // 按 receiver 推断类型显式降级，避免参数被丢弃退化回无参调用。
    if method.as_str() == "ToString" && !args.is_empty() {
        let recv_type = infer_type_from_spanned(receiver, ctx);
        if matches!(
            recv_type,
            TypeId::Int
                | TypeId::Long
                | TypeId::Short
                | TypeId::Byte
                | TypeId::Float
                | TypeId::Double
                | TypeId::UInt
                | TypeId::ULong
                | TypeId::UShort
                | TypeId::SByte
        ) {
            let static_name = format!("{}.ToString", type_id_name(&recv_type));
            let mut call_args = vec![recv];
            for (i, a) in args.iter().enumerate() {
                call_args.push(lower_arg(builder, a, i));
            }
            return MirRvalue::Call {
                func: static_name,
                args: call_args,
            };
        }
    }
    if let Some(func) = builtin_static_method(&receiver.node, method) {
        return MirRvalue::Call {
            func,
            args: args
                .iter()
                .enumerate()
                .map(|(i, a)| lower_arg(builder, a, i))
                .collect(),
        };
    }
    let (impl_class, target_fn) = if let Ok((declaring, sig)) = resolved {
        // CD-15/D4：`base.M()` → 直接基类实现的**非虚**静态分派（C# base 语义）。
        // `recv_ty` 已由 type_name_from_operand 解析为直接基类；base_call_target
        // 沿直接基类的 method_impl 取实现类并生成目标链接名。
        if matches!(receiver.node, Expr::Base) {
            base_call_target(
                ctx.registry,
                ctx.layouts,
                recv_ty,
                method,
                &declaring,
                &sig,
                type_args,
                &arg_types,
                &overload_ctx,
            )
        } else {
            let target = if !type_args.is_empty() {
                generic_instantiation_target(
                    ctx.registry,
                    &declaring,
                    method,
                    &sig,
                    &arg_types,
                    &type_arg_names,
                    &overload_ctx,
                )
            } else {
                ctx.registry.method_link_name_for(&declaring, &sig)
            };
            let impl_cls = if ctx.registry.is_interface(recv_ty) {
                // Fat pointer already stored in the interface receiver; codegen
                // must not rebuild itable from a guessed concrete class.
                None
            } else {
                Some(declaring.to_string())
            };
            (impl_cls, Some(target))
        }
    } else if let Ok(Some(ext)) = ctx.registry.resolve_extension_with_arg_types(
        recv_ty,
        method,
        args.len(),
        &type_arg_names,
        &arg_types,
        &overload_ctx,
    ) {
        // 决策 #7（RFC 010）：泛型扩展方法使用 mangled call_name（如 `FooExt::Id_int`）。
        // 须传实参类型消歧：`AddSingleton<T>(T)` vs `AddSingleton<T>(Func<…>)`
        // 同 arity 并列时仅靠接收者无法区分（web_mb `AddSingleton<IConfiguration>(cfg)`）。
        // 接收者装箱：扩展方法 `this` 形参可能是接口（如
        // `AddTransient<T>(this IServiceCollection)`）而实参接收者是具体类对象——
        // 须包装为接口胖指针（MirOperand::Iface）。否则 callee 把裸对象指针当
        // `{ ptr, ptr }` 解引用（refcount 槽误读为 obj/itable）→ ACCESS_VIOLATION。
        // `ext.sig.params` 不含 this（注册时已 remove(0)），故用 `ext.this_ty`。
        let recv = maybe_box_iface(recv, recv_ty, &TypeId::Named(ext.this_ty.clone()), ctx);
        let mut call_args = vec![recv];
        call_args.extend(
            args.iter()
                .enumerate()
                .map(|(i, a)| lower_arg(builder, a, i)),
        );
        return MirRvalue::Call {
            func: ext.call_name,
            args: call_args,
        };
    } else {
        // 严格重载匹配失败（如泛型方法 `GetValue<T>(DependencyProperty<T>)`
        // 在 `arg_types` 为单态化 `DependencyProperty_string` 时不匹配 `T`）。
        // 回退到 `resolve_method_with_declaring` 沿继承链查找声明类，
        // 确保 `Window.GetValue<T>`（实现在 `Element`）被 mangle 为
        // `@Element_GetValue` 而非 `@Window_GetValue`。
        let fallback = ctx
            .registry
            .resolve_method_with_declaring(recv_ty, method, &overload_ctx);
        // CD-15/D4：base 调用的回退路径同样命中直接基类实现（非虚）。
        if matches!(receiver.node, Expr::Base) {
            fallback
                .as_ref()
                .ok()
                .map(|(declaring, sig)| {
                    base_call_target(
                        ctx.registry,
                        ctx.layouts,
                        recv_ty,
                        method,
                        declaring,
                        sig,
                        type_args,
                        &arg_types,
                        &overload_ctx,
                    )
                })
                .unwrap_or((None, None))
        } else {
            let impl_cls = if ctx.registry.is_interface(recv_ty) {
                None
            } else if let Ok((declaring, _)) = &fallback {
                Some(declaring.to_string())
            } else {
                None
            };
            let target_fn = fallback.as_ref().ok().map(|(declaring, sig)| {
                if !type_args.is_empty() {
                    generic_instantiation_target(
                        ctx.registry,
                        declaring,
                        method,
                        sig,
                        &arg_types,
                        &type_arg_names,
                        &overload_ctx,
                    )
                } else {
                    ctx.registry.method_link_name_for(declaring, sig)
                }
            });
            (impl_cls, target_fn)
        }
    };
    let param_type_strs: Vec<String> = param_types.iter().map(|p| p.as_str().to_string()).collect();
    // CD-15/D4：`base.M()` 恒为非虚调用（C# 语义：跳过派生覆写，静态分派到
    // 直接基类实现）。`recv_ty` 已为直接基类，但其虚槽仍会命中（基类方法带
    // virtual 修饰时 is_virtual_member 为 true），必须强制 false。
    let is_base_call = matches!(receiver.node, Expr::Base);
    let is_virtual =
        !is_base_call && is_virtual_member(ctx.layouts, recv_ty, method, &param_type_strs);
    // Build args with interface boxing applied based on formal param types.
    let boxed_args: Vec<MirOperand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let op = lower_arg(builder, a, i);
            if let Some(param_ty) = param_types.get(i) {
                let arg_ty = type_name_from_operand(&op, &a.node, ctx);
                let param_type_id = TypeId::Named(param_ty.clone());
                return maybe_box_iface(op, &arg_ty, &param_type_id, ctx);
            }
            op
        })
        .collect();
    // string.Split 重载：MIR 擦除 enum/char/int 后均为 i32，按实参 TypeId 改写方法名供 codegen 分派。
    let method_str = rewrite_string_split_method(recv_ty, method, args, ctx);
    MirRvalue::MethodCall {
        receiver: recv,
        method: method_str,
        args: boxed_args,
        receiver_type: recv_ty.to_string(),
        impl_class,
        target_fn,
        is_virtual,
        params: param_type_strs,
    }
}

/// Variant of `method_call_rvalue` that materializes each argument via
/// `lower_arg_operand` so that complex argument expressions (e.g.
/// `people.Add(new Person() { Age = 25 })` where the arg is an object
/// initializer) are lowered to temp locals with their prep statements
/// preserved. Returns `(prep, rvalue)` so callers can emit prep before
/// the rvalue assignment.
///
/// Mirrors `method_call_rvalue`'s overload resolution and interface
/// boxing logic, but uses pre-materialized operands instead of
/// `operand_from_expr` (which collapses `Expr::New` to `ConstInt(0)`).
pub(super) fn method_call_rvalue_with_prep(
    builder: &mut MirBuilder,
    receiver: &Spanned<Expr>,
    method: &Ident,
    args: &[Spanned<Expr>],
    type_args: &[ast::Type],
    params_span: Option<&ParamsSpanInfo>,
    ctx: &mut LowerCtx,
    recv: MirOperand,
    recv_ty: &Ident,
) -> (Vec<MirStatement>, MirRvalue) {
    let mut prep: Vec<MirStatement> = Vec::new();
    // RFC 005：按调用点标注把实参切分为「固定前缀」与「尾随可变实参」，尾随
    // 部分经单一物化点 `materialize_params_span` 收集为 SpanFromStack（末位实参）。
    // 重载解析所需的有效实参类型为「固定类型 + params 槽的 Span 类型」。
    let (fixed_args, trailing) = split_params_args(args, params_span);
    let mut arg_types: Vec<Ident> = fixed_args
        .iter()
        .map(|a| type_id_name(&infer_type_from_spanned(a, ctx)))
        .collect();
    if let Some(info) = params_span {
        let span_ty = TypeId::Span {
            elem: Box::new(info.elem.clone()),
            mutable: info.mutable,
        };
        arg_types.push(type_id_name(&span_ty));
    }
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
        .map(|t| type_id_name(&lower_type_name(t)))
        .collect();
    // RFC 005 M2b λ 对齐（与 `method_call_rvalue` 同阶梯）：strict 失败且含
    // 未绑定 lambda 时先按 λ 软匹配回落实例重载，避免与 typeck 绑定分叉
    //（扩展误选 / 替换后基底 / λ 形参类型丢失）。仅无显式 type_args 时启用
    //（显式泛型实参须走 type-args/模板路径，soft 不得抢先命中非泛型重载）。
    let has_lambda = args.iter().any(|a| matches!(a.node, Expr::Lambda(_)));
    let resolved = {
        let strict = if !type_args.is_empty() {
            ctx.registry.resolve_method_with_type_args(
                recv_ty,
                method,
                &arg_types,
                &type_arg_names,
                &overload_ctx,
            )
        } else {
            ctx.registry
                .resolve_method_overload(recv_ty, method, &arg_types, &overload_ctx)
        };
        if strict.is_err() && has_lambda && type_args.is_empty() {
            strict.or_else(|_| {
                ctx.registry.resolve_method_overload_lambda_soft(
                    recv_ty,
                    method,
                    &arg_types,
                    &overload_ctx,
                )
            })
        } else {
            strict
        }
    };
    // 严格重载失败（lambda 实参推断为 `Func_Infer_*` 与单态化形参
    // `Func_double_*` 不严格匹配）时回退到按名查找的声明类签名——与下方
    // `impl_class`/`target_fn` 的回退路径一致——仍能提供 Func/Action 形参
    // 类型供未标注 lambda 形参推断（否则 Signal<T> 单态化包装 lambda 的
    // double/string 载荷被截断为 i32）。
    let param_types: Vec<Ident> = resolved
        .as_ref()
        .ok()
        .map(|(_, sig)| sig.params.iter().map(|p| p.ty.clone()).collect())
        .or_else(|| {
            ctx.registry
                .resolve_method_with_declaring(recv_ty, method, &overload_ctx)
                .ok()
                .map(|(_, sig)| sig.params.iter().map(|p| p.ty.clone()).collect())
        })
        .unwrap_or_default();
    let expected_lambda_params: Vec<Option<Vec<TypeId>>> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if let Expr::Lambda(l) = &a.node {
                param_types.get(i).and_then(|p| {
                    demangle_delegate_params_with(p.as_str(), l.params.len(), &|s| {
                        ctx.registry.types.contains_key(s)
                    })
                })
            } else {
                None
            }
        })
        .collect();
    // Lambda 实参的委托契约返回类型（`Func<R>` 形参名 → R；接口 R 时闭包
    // 须产出 fat pointer，见 lower_lambda_to_fnptr）。
    let expected_lambda_rets: Vec<Option<TypeId>> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            if let Expr::Lambda(_) = &a.node {
                param_types.get(i).and_then(|p| {
                    let pty = TypeId::Named(p.clone());
                    if lower_type::is_delegate_type(&pty) {
                        lower_type::delegate_return_type(&pty, &|s| {
                            ctx.registry.types.contains_key(s)
                        })
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .collect();
    // Materialize each argument to a temp local via `lower_arg_operand`,
    // preserving prep statements. Lambda args go through the lambda path
    // inside `lower_arg_operand` (it has an `Expr::Lambda` early return).
    // RFC 005：固定前缀逐参物化；尾随 params 实参经单一物化点收集为栈 Span。
    let mut arg_ops: Vec<MirOperand> = Vec::with_capacity(fixed_args.len() + 1);
    for (i, a) in fixed_args.iter().enumerate() {
        let (mut p, op) = lower_arg_operand_with_expected(
            builder,
            &a.node,
            ctx,
            expected_lambda_params[i].as_deref(),
            expected_lambda_rets[i].as_ref(),
        );
        prep.append(&mut p);
        // raw/λ 路径 string→object 实参装箱（typed 路径由 typeck 插 Box 节点）。
        let op = maybe_box_string_to_object(
            builder,
            a,
            op,
            param_types.get(i).map(|s| s.as_str()),
            ctx,
            &mut prep,
        );
        arg_ops.push(op);
    }
    if let Some(info) = params_span {
        let (mut p, span_op) = materialize_params_span(builder, info, trailing, ctx);
        prep.append(&mut p);
        arg_ops.push(span_op);
    }
    // RFC 034 M5 / RFC 007 M2a：primitive 实例有参 ToString——降级为静态调用
    // `double.ToString(x, format, provider)`（receiver 作为首参并入 args）。
    // `builtin_static_method` 仅对类名 receiver（如 `double.ToString(...)`）命中，
    // 字面量 receiver（如 `(1234.5).ToString(...)`）无法命中，故在此显式降级，
    // 避免参数被丢弃退化回无参调用。
    if method.as_str() == "ToString" && !args.is_empty() {
        let recv_type = infer_type_from_spanned(receiver, ctx);
        if matches!(
            recv_type,
            TypeId::Int
                | TypeId::Long
                | TypeId::Short
                | TypeId::Byte
                | TypeId::Float
                | TypeId::Double
                | TypeId::UInt
                | TypeId::ULong
                | TypeId::UShort
                | TypeId::SByte
        ) {
            let static_name = format!("{}.ToString", type_id_name(&recv_type));
            let mut call_args = vec![recv];
            call_args.extend(arg_ops);
            return (
                prep,
                MirRvalue::Call {
                    func: static_name,
                    args: call_args,
                },
            );
        }
    }
    if let Some(func) = builtin_static_method(&receiver.node, method) {
        return (
            prep,
            MirRvalue::Call {
                func,
                args: arg_ops,
            },
        );
    }
    let (impl_class, target_fn) = if let Ok((declaring, sig)) = resolved {
        // CD-15/D4：`base.M()` → 直接基类实现的**非虚**静态分派（同
        // method_call_rvalue 路径，见 base_call_target）。
        if matches!(receiver.node, Expr::Base) {
            base_call_target(
                ctx.registry,
                ctx.layouts,
                recv_ty,
                method,
                &declaring,
                &sig,
                type_args,
                &arg_types,
                &overload_ctx,
            )
        } else {
            let target = if !type_args.is_empty() {
                // 基底用模板 link 名（含重载参数占位），与 MIR 模板名对齐。
                generic_instantiation_target(
                    ctx.registry,
                    &declaring,
                    method,
                    &sig,
                    &arg_types,
                    &type_arg_names,
                    &overload_ctx,
                )
            } else {
                ctx.registry.method_link_name_for(&declaring, &sig)
            };
            let impl_cls = if ctx.registry.is_interface(recv_ty) {
                None
            } else {
                Some(declaring.to_string())
            };
            (impl_cls, Some(target))
        }
    } else if let Ok(Some(ext)) = ctx.registry.resolve_extension_with_arg_types(
        recv_ty,
        method,
        args.len(),
        &type_arg_names,
        &arg_types,
        &overload_ctx,
    ) {
        // 决策 #7（RFC 010）：扩展方法首参为接收者；泛型扩展使用 mangled call_name。
        // 须传实参类型消歧（同 method_call_rvalue：AddSingleton 实例/工厂并列）。
        // 接收者装箱：与 method_call_rvalue 扩展路径一致——`this` 形参为接口时
        // 须把具体类接收者包装为接口胖指针（MirOperand::Iface）。
        // `ext.sig.params` 不含 this（注册时已 remove(0)），故用 `ext.this_ty`。
        let recv = maybe_box_iface(recv, recv_ty, &TypeId::Named(ext.this_ty.clone()), ctx);
        let mut call_args = vec![recv];
        call_args.extend(arg_ops);
        return (
            prep,
            MirRvalue::Call {
                func: ext.call_name,
                args: call_args,
            },
        );
    } else {
        // 严格重载匹配失败时，回退到 `resolve_method_with_declaring`
        // 沿继承链查找声明类（与 `method_call_rvalue` 对称）。
        let fallback = ctx
            .registry
            .resolve_method_with_declaring(recv_ty, method, &overload_ctx);
        // CD-15/D4：base 调用的回退路径同样命中直接基类实现（非虚）。
        if matches!(receiver.node, Expr::Base) {
            fallback
                .as_ref()
                .ok()
                .map(|(declaring, sig)| {
                    base_call_target(
                        ctx.registry,
                        ctx.layouts,
                        recv_ty,
                        method,
                        declaring,
                        sig,
                        type_args,
                        &arg_types,
                        &overload_ctx,
                    )
                })
                .unwrap_or((None, None))
        } else {
            let impl_cls = if ctx.registry.is_interface(recv_ty) {
                None
            } else if let Ok((declaring, _)) = &fallback {
                Some(declaring.to_string())
            } else {
                None
            };
            let target_fn = fallback.as_ref().ok().map(|(declaring, sig)| {
                if !type_args.is_empty() {
                    generic_instantiation_target(
                        ctx.registry,
                        declaring,
                        method,
                        sig,
                        &arg_types,
                        &type_arg_names,
                        &overload_ctx,
                    )
                } else {
                    ctx.registry.method_link_name_for(declaring, sig)
                }
            });
            (impl_cls, target_fn)
        }
    };
    let param_type_strs: Vec<String> = param_types.iter().map(|p| p.as_str().to_string()).collect();
    // CD-15/D4：`base.M()` 恒为非虚调用（C# 语义：跳过派生覆写）。
    let is_base_call = matches!(receiver.node, Expr::Base);
    let is_virtual =
        !is_base_call && is_virtual_member(ctx.layouts, recv_ty, method, &param_type_strs);
    // Apply interface boxing based on formal param types.
    let boxed_args: Vec<MirOperand> = arg_ops
        .into_iter()
        .enumerate()
        .map(|(i, op)| {
            if let Some(param_ty) = param_types.get(i) {
                let param_type_id = TypeId::Named(param_ty.clone());
                // Arg class 推断与 `method_call_rvalue` 同源（AST 推断优先）：
                // 仅查 `ctx.locals` 会漏（`arg=unknown` → impl 判定失败 → 裸
                // 对象指针传接口形参 → 分派解引用垃圾 itable AV；l3 sim 批
                // `_simulation.Update(delta, _registry)` 实测）。
                let arg_ty = args
                    .get(i)
                    .map(|a| type_name_from_operand(&op, &a.node, ctx))
                    .unwrap_or_else(|| "unknown".into());
                return maybe_box_iface(op, &arg_ty, &param_type_id, ctx);
            }
            op
        })
        .collect();
    (
        prep,
        MirRvalue::MethodCall {
            receiver: recv,
            method: rewrite_string_split_method(recv_ty, method, args, ctx),
            args: boxed_args,
            receiver_type: recv_ty.to_string(),
            impl_class,
            target_fn,
            is_virtual,
            params: param_type_strs,
        },
    )
}

pub(super) fn try_const_operand(
    class: &Ident,
    field: &Ident,
    ctx: &LowerCtx,
) -> Option<MirOperand> {
    let nom = ctx.registry.types.get(class)?;
    let finfo = nom.fields.get(field)?;
    if !finfo.is_const {
        return None;
    }
    let cv = nom.const_values.get(field)?;
    Some(match cv {
        ConstValue::Int(n) => MirOperand::ConstInt(*n),
        ConstValue::Float(f) => MirOperand::ConstFloat(*f),
        ConstValue::String(s) => MirOperand::ConstString(s.clone()),
        ConstValue::Bool(b) => MirOperand::ConstBool(*b),
        ConstValue::Null => MirOperand::ConstNull,
    })
}

/// RFC 004 M2：检测用户类型的静态方法调用（如 `Vector2.Add(a, b)`）。
///
/// 当 receiver 是类型名（非变量）且该类型注册了同名 `static` 方法时，
/// 返回 mangled 函数名（`"Vector2::Add"`，codegen mangle 为 `@Vector2_Add`）。
/// 静态方法无 `this` 参数，应降级为 `MirRvalue::Call` 而非 `MethodCall`。
///
/// 解析用户类型静态方法调用，返回 `(link 名, 已消歧形参类型)`。
///
/// 形参类型用于调用点把 class 实参包装为接口胖指针（`maybe_box_iface`）——
/// 静态方法（如 `Reg.RegA(sc)` 中形参为 `IServiceCollection`）缺此包装时，
/// callee 把裸对象指针当 `{ptr,ptr}` 解引用 itable → ACCESS_VIOLATION。
pub(super) fn user_type_static_method_sig(
    receiver: &Expr,
    method: &Ident,
    type_args: &[ast::Type],
    args: &[Spanned<Expr>],
    ctx: &LowerCtx,
) -> Option<(String, Vec<Ident>)> {
    // 泛型类静态方法调用（`Holder<Thing>.Get(...)` / `EntityMap<User>.FindKey`）：
    // receiver 为 `Call { Ident, [], type_args }`，typeck 已单态化并注册 mangle 名
    // （`Holder_Thing`）。与 `Expr::Ident` 路径统一，避免 receiver 被物化为
    // `call @Holder_Thing()`（类名被误当构造调用）充当 this。
    let (name, receiver_type_args) = match receiver {
        Expr::Ident(name) => (name, None),
        Expr::Call {
            func,
            args: ca,
            type_args: rta,
            ..
        } if ca.is_empty() && !rta.is_empty() => match &func.node {
            Expr::Ident(name) => (name, Some(rta)),
            _ => return None,
        },
        _ => return None,
    };
    let class_ident: Ident = if let Some(rta) = receiver_type_args {
        let arg_tys: Vec<TypeId> = rta.iter().map(|t| lower_type_name(&t.node)).collect();
        let mangled = mangle_generic(name.as_str(), &arg_tys);
        let mangled_ident: Ident = mangled.as_str().into();
        if ctx.registry.types.contains_key(&mangled_ident) {
            mangled_ident
        } else {
            return None;
        }
    } else {
        name.clone()
    };
    let nom = match ctx.registry.types.get(&class_ident) {
        Some(n) => n,
        None => {
            return None;
        }
    };
    if !matches!(
        nom.kind,
        typeck::TypeKind::Class | typeck::TypeKind::Struct | typeck::TypeKind::StaticClass
    ) {
        return None;
    }
    let sigs = nom.methods.get(method)?;
    let has_static = sigs.iter().any(|s| s.modifier == MethodModifier::Static);
    if !has_static {
        return None;
    }
    let sig = match resolve_static_overload(&class_ident, method, type_args, args, ctx) {
        Some(s) => s,
        None => {
            return None;
        }
    };
    // 使用 `{Class}::{Method}` 格式，codegen `mangle_fn_name` 将 `::` 替换为 `_`。
    if type_args.is_empty() {
        let func = ctx.registry.method_link_name_for(&class_ident, &sig);
        return Some((func, sig.params.iter().map(|p| p.ty.clone()).collect()));
    }
    // resolve_* 返回的 sig 已把 param 里的 T 换成 concrete；link 名须用模板。
    let type_arg_names: Vec<Ident> = type_args
        .iter()
        .map(|t| type_id_name(&lower_type_name(t)))
        .collect();
    let arg_types: Vec<Ident> = args
        .iter()
        .map(|a| type_id_name(&infer_type_from_spanned(a, ctx)))
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
    let base = ctx
        .registry
        .method_generic_template_link_name(
            &class_ident,
            method,
            &arg_types,
            &type_arg_names,
            &overload_ctx,
        )
        .unwrap_or_else(|| ctx.registry.method_link_name_for(&class_ident, &sig));
    let suffix: String = type_arg_names
        .iter()
        .map(|t| t.as_str().to_string())
        .collect::<Vec<_>>()
        .join("__");
    let full = format!("{base}__{suffix}");
    Some((full, sig.params.iter().map(|p| p.ty.clone()).collect()))
}

/// 按实参类型在 `ty.method` 的静态重载中选唯一候选。
/// 推断失败时的窄回退：唯一静态签名，或同 arity 唯一静态签名。
/// 多个同 arity 候选时返回 None（须由 `infer_type_from_expr` 给出正确实参类型）。
fn resolve_static_overload(
    ty: &Ident,
    method: &Ident,
    type_args: &[ast::Type],
    args: &[Spanned<Expr>],
    ctx: &LowerCtx,
) -> Option<typeck::OopMethodSig> {
    let arg_types: Vec<Ident> = args
        .iter()
        .map(|a| type_id_name(&infer_type_from_spanned(a, ctx)))
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
    let resolved = if !type_args.is_empty() {
        let type_arg_names: Vec<Ident> = type_args
            .iter()
            .map(|t| type_id_name(&lower_type_name(t)))
            .collect();
        ctx.registry.resolve_method_with_type_args(
            ty,
            method,
            &arg_types,
            &type_arg_names,
            &overload_ctx,
        )
    } else {
        ctx.registry
            .resolve_method_overload(ty, method, &arg_types, &overload_ctx)
    };
    if let Ok((_, sig)) = resolved {
        if sig.modifier == MethodModifier::Static {
            return Some(sig);
        }
    }
    // 推断失败时的窄回退：仅当存在唯一静态签名（无重载歧义）才取之。
    let nom = ctx.registry.types.get(ty)?;
    let sigs = nom.methods.get(method)?;
    let static_sigs: Vec<_> = sigs
        .iter()
        .filter(|s| s.modifier == MethodModifier::Static)
        .cloned()
        .collect();
    match static_sigs.as_slice() {
        [sig] => Some(sig.clone()),
        _ => {
            // Narrow fallback: only when a unique same-arity static signature
            // exists. Nested Field chains must be typed by `infer_type_from_expr`
            // (see lower_type Field arm); do not pick the first of several
            // same-arity overloads (that hid `b.Value.Value` → Int failures).
            let by_arity: Vec<_> = static_sigs
                .iter()
                .filter(|s| s.params.len() == args.len())
                .cloned()
                .collect();
            match by_arity.as_slice() {
                [sig] => Some(sig.clone()),
                _ => None,
            }
        }
    }
}

/// Resolve a bare function name to a static method on the current owning class.
///
/// When `Program.Main()` calls `Compare(5, 3)` (no `Program.` prefix), the
/// call site in MIR would get `Call { func: "Compare" }` while the typed fn
/// is registered as `Program::Compare`.  This function bridges that gap by
/// returning `"Program::Compare"` when `name` is a static method on `ctx.owner`.
///
/// 多静态重载时按 `args` 类型解析，与 [`user_type_static_method_func`] 一致。
///
/// Returns `None` if `ctx.owner` is not set, the class is not in the registry,
/// or the method is not a static method of the owning class.
pub(super) fn resolve_class_static_method(
    name: &Ident,
    args: &[Spanned<Expr>],
    ctx: &LowerCtx,
) -> Option<String> {
    let owner = ctx.owner.as_ref()?;
    let nom = ctx.registry.types.get(owner)?;
    let sigs = nom.methods.get(name)?;
    if !sigs.iter().any(|s| s.modifier == MethodModifier::Static) {
        return None;
    }
    let sig = resolve_static_overload(owner, name, &[], args, ctx)?;
    Some(ctx.registry.method_link_name_for(owner, &sig))
}

/// 判断 `owner` 类（含基类链）是否存在 `name` 的实例方法且参数个数匹配。
/// 供 MIR 裸实例方法调用（`_bump()` → `this._bump()`）重写使用。
pub(super) fn mir_has_instance_method(
    registry: &typeck::TypeRegistry,
    owner: &Ident,
    name: &Ident,
    argc: usize,
) -> bool {
    let mut current = Some(owner.clone());
    while let Some(cn) = current {
        let Some(nom) = registry.types.get(&cn) else {
            break;
        };
        if let Some(sigs) = nom.methods.get(name) {
            if sigs.iter().any(|s| {
                s.modifier != MethodModifier::Static
                    && !s.is_static_abstract
                    && s.params.len() == argc
            }) {
                return true;
            }
        }
        current = nom.bases.iter().find(|b| registry.is_class(b)).cloned();
    }
    false
}

/// 按名判定 `owner` 类（含基类链）是否存在名为 `name` 的实例方法（不限 arity）。
///
/// lambda 捕获分析（compute_captures）只持有标识符名、拿不到调用点 arity，
/// 按名保守判定：命中即视作隐式 `this.` 调用、触发 this 捕获。局部遮蔽由
/// 调用方先行排除，与调用点重写（mir_has_instance_method）的解析优先级一致。
pub(super) fn mir_has_instance_method_named(
    registry: &typeck::TypeRegistry,
    owner: &Ident,
    name: &Ident,
) -> bool {
    let mut current = Some(owner.clone());
    while let Some(cn) = current {
        let Some(nom) = registry.types.get(&cn) else {
            break;
        };
        if let Some(sigs) = nom.methods.get(name) {
            if sigs
                .iter()
                .any(|s| s.modifier != MethodModifier::Static && !s.is_static_abstract)
            {
                return true;
            }
        }
        current = nom.bases.iter().find(|b| registry.is_class(b)).cloned();
    }
    false
}

/// 按名判定 `owner` 类自身是否存在名为 `name` 的静态成员
/// （静态方法 / 静态属性 getter `get_{name}` / 静态·常量字段）。
///
/// lambda 体内裸引用静态成员不捕获 this（静态成员不经 this 访问），但裸名 →
/// 限定符号解析（resolve_class_static_method / StaticField operand）依赖 owner，
/// 捕获分析据此标记 owner 传播。与消费端一致仅查 owner 自身：静态成员经类名
/// 限定访问，不沿基类链隐式解析。
pub(super) fn mir_class_has_static_member_named(
    registry: &typeck::TypeRegistry,
    owner: &Ident,
    name: &Ident,
) -> bool {
    let Some(nom) = registry.types.get(owner) else {
        return false;
    };
    let has_static_method = |method: &Ident| {
        nom.methods
            .get(method)
            .is_some_and(|sigs| sigs.iter().any(|s| s.modifier == MethodModifier::Static))
    };
    if has_static_method(name) {
        return true;
    }
    // 静态属性：custom getter 注册为静态方法 `get_{name}`。
    let getter: Ident = format!("get_{name}").into();
    if has_static_method(&getter) {
        return true;
    }
    registry
        .field_info(owner, name)
        .is_some_and(|f| f.is_static || f.is_const)
}

/// RFC 004 M2：检测用户类型的静态属性访问（如 `Vector2.Zero`）。
///
/// 当 receiver 是类型名且该类型注册了 `static get_{field}` 方法时，
/// 返回 mangled getter 函数名（`"Vector2::get_Zero"`，codegen mangle 为
/// `@Vector2_get_Zero`）。静态属性无 `this` 参数，应降级为
/// `MirRvalue::Call` 而非 `MethodCall`（避免 `ptr 0` 作为 `this`）。
///
/// 返回 `None` 的情形：
/// - receiver 不是 `Expr::Ident`。
/// - name 不是注册的 class/struct/static_class 类型。
/// - 该类型未声明 `static` getter `get_{field}`。
pub(super) fn user_type_static_property_func(
    receiver: &Expr,
    field: &Ident,
    ctx: &LowerCtx,
) -> Option<String> {
    let Expr::Ident(name) = receiver else {
        return None;
    };
    // 根因修复（Builtin 静态自动属性）：
    // `[Builtin]` 静态自动属性的 getter 无真实方法体——语义完全在 codegen
    // `try_emit_builtin_static` 按**源码形** `"Class.Prop"` 分派。无论该类是否
    // 已注册进 registry（facade 类可能因 `using` 非递归加载而缺席，如 Task/
    // CancellationToken），一律还原源码形。若走下方普通路径返回 mangled
    // `Class::get_Prop`，codegen 无法命中源码形分派表 → `undefined value`。
    //
    // 判定：**真实静态 getter**（registry 有该类、`get_X` 已注册、且属性非
    // `[Builtin]` 静态自动属性）→ 走真实函数符号（如
    // `Path.DirectorySeparatorChar { get { return "/"; } }`——一律还原曾致
    // `@Path.DirectorySeparatorChar` undefined value）；否则（缺席 /
    // `[Builtin]` 静态自动属性，如 `Task.CompletedTask`）→ 源码形。
    if typeck::is_builtin_facade(name) {
        let getter: Ident = format!("get_{field}").into();
        let real_getter = ctx.registry.types.get(name).is_some_and(|nom| {
            nom.methods
                .get(&getter)
                .is_some_and(|sigs| sigs.iter().any(|s| s.modifier == MethodModifier::Static))
                && !ctx.registry.is_builtin_static_prop(name, field)
        });
        if !real_getter {
            return Some(format!("{name}.{field}"));
        }
        // 有真实 getter → 继续走下方普通路径。
    }
    let nom = ctx.registry.types.get(name)?;
    if !matches!(
        nom.kind,
        typeck::TypeKind::Class | typeck::TypeKind::Struct | typeck::TypeKind::StaticClass
    ) {
        return None;
    }
    let getter_name: Ident = format!("get_{field}").into();
    let sigs = nom.methods.get(&getter_name)?;
    let sig = sigs.iter().find(|s| s.modifier == MethodModifier::Static)?;
    Some(ctx.registry.method_link_name_for(name, sig))
}

/// RFC 004 M2 对称路径：检测用户类型静态属性**赋值**（如 `CultureInfo.CurrentUICulture = v`）。
///
/// 当 receiver 是类型名且该类型注册了 `static set_{field}` 方法时，返回
/// mangled setter 函数名。静态 setter 无 `this` 参数，赋值应降级为
/// `MirRvalue::Call { args: [value] }` 而非 `MethodCall`（receiver 类名
/// 不是表达式，物化会 ICE）。返回 `None` 的情形同 getter 版本。
pub(super) fn user_type_static_property_setter_func(
    receiver: &Expr,
    field: &Ident,
    ctx: &LowerCtx,
) -> Option<String> {
    let Expr::Ident(name) = receiver else {
        return None;
    };
    let nom = ctx.registry.types.get(name)?;
    if !matches!(
        nom.kind,
        typeck::TypeKind::Class | typeck::TypeKind::Struct | typeck::TypeKind::StaticClass
    ) {
        return None;
    }
    let setter_name: Ident = format!("set_{field}").into();
    let sigs = nom.methods.get(&setter_name)?;
    let sig = sigs.iter().find(|s| s.modifier == MethodModifier::Static)?;
    Some(ctx.registry.method_link_name_for(name, sig))
}
