use super::lower_call::*;
use super::lower_linq::is_primitive_numeric_type;
use super::lower_type::*;
use super::*;
use crate::types::ArrayLitElement;
use ast::{CollectionElement, ExpressionTree};

pub(super) fn lower_cond(
    builder: &mut MirBuilder,
    expr: &Expr,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    lower_expr_to_operand(builder, expr, ctx)
}

/// 无目标上下文的 lambda（表达式位置实参 / 委托实例化 / 裸赋值）的形参类型
/// 来源：P0 表中 typeck bind 时按形参槽推断出的 `Func` 形态——单一事实源。
/// 无标注形参不得回退 Int（object/Func 形参槽 i32 化 → 闭包 ABI 错位，
/// `'t' defined with type 'i32' but expected 'ptr'）。
fn lambda_expected_params<'a>(ctx: &'a LowerCtx, span: ast::Span) -> Option<&'a [TypeId]> {
    match ctx.expr_types.get(span) {
        Some(TypeId::Func { params, .. }) => Some(params),
        _ => None,
    }
}

/// 为 `new C(args...)` 的实参装箱：形参为接口类型时，把具体类实参包装为接口
/// 胖指针（`MirOperand::Iface`）。与 `maybe_box_iface`（方法实参）对偶——
/// 否则 ctor 形参收到裸对象指针，存储进接口类型字段后 dispatch 按 `{ptr,ptr}`
/// 解引用（obj 槽误读为 itable 地址）→ ACCESS_VIOLATION（AICodeAct._provider 实测）。
fn box_ctor_args(
    class: &Ident,
    args: &[Spanned<Expr>],
    ops: Vec<MirOperand>,
    ctx: &LowerCtx,
) -> Vec<MirOperand> {
    let ctor = ctx
        .registry
        .ctor_signatures(class)
        .iter()
        .find(|c| c.param_types.len() == args.len());
    let Some(ctor) = ctor else {
        return ops;
    };
    ops.into_iter()
        .zip(args.iter())
        .zip(ctor.param_types.iter())
        .map(|((op, a), param_ty)| {
            let arg_class = type_name_from_operand(&op, &a.node, ctx);
            maybe_box_iface(op, &arg_class, &TypeId::Named(param_ty.clone()), ctx)
        })
        .collect()
}

/// 解析 `new C(args...)` 的构造器形参类型名（mangle 就绪，CD-10/D1 同源）。
///
/// 供 `MirRvalue::New.ctor_params` 填充：codegen 按签名 mangle ctor 符号
/// （`__ctor::C_1` 仅按参数个数，同参数量不同类型参数的 ctor 重载会符号碰撞）。
/// 与 `box_ctor_args` 同源：按参数个数锁定候选，存在同参数量重载时用实际
/// 实参类型名精确匹配消歧；无法匹配时回退首个同参数量候选。
/// **仅当存在同参数量碰撞（`same_arity.len() > 1`）时返回非空列表**，否则
/// 返回空列表（codegen 保持旧 arity mangle）。
fn resolve_ctor_params(
    class: &Ident,
    args: &[Spanned<Expr>],
    ops: &[MirOperand],
    ctx: &LowerCtx,
) -> Vec<String> {
    let same_arity: Vec<_> = ctx
        .registry
        .ctor_signatures(class)
        .iter()
        .filter(|c| c.param_types.len() == args.len())
        .collect();
    if same_arity.len() <= 1 {
        // 无碰撞：arity 唯一，无需消歧。
        return vec![];
    }
    let arg_names: Vec<Ident> = ops
        .iter()
        .zip(args.iter())
        .map(|(op, a)| type_name_from_operand(op, &a.node, ctx))
        .collect();
    // RFC 045（di_decorate 崩溃根因）：内联 lambda 实参（`new ServiceDescriptor(
    // typeof(T), (sp) => ..., lifetime)`）的类型名经 type_name_from_operand 不可得
    // （Closure operand → "unknown"）——匹配时允许 unknown 实参命中 Func/Action
    // 形参（lambda 实参与委托形参的唯一天然匹配）；其余实参仍须精确相等。
    let ctor = same_arity
        .iter()
        .find(|c| {
            c.param_types == arg_names
                || (arg_names.len() == c.param_types.len()
                    && arg_names.iter().zip(&c.param_types).all(|(a, p)| {
                        a == p
                            || (*a == "unknown"
                                && (p.starts_with("Func") || p.starts_with("Action")))
                    }))
        })
        .or_else(|| same_arity.first());
    ctor.map(|c| c.param_types.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

pub(super) fn lower_expr_to_operand(
    builder: &mut MirBuilder,
    expr: &Expr,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    if matches!(
        expr,
        Expr::IntLit(_)
            | Expr::BoolLit(_)
            | Expr::StringLit(_)
            | Expr::Ident(_)
            | Expr::This
            | Expr::Null
    ) {
        return (vec![], operand_from_expr(expr, ctx));
    }
    match expr {
        Expr::Binary { op, left, right } => {
            // `&&`/`||` 必须短路（C#/Arc 语义）：`if (e == null || e.Kind != X)`
            // 中右操作数在左操作数为 true 时不得求值——否则对 null 指针做字段
            // 读取（GEP null + load）→ ACCESS_VIOLATION。lower_arg_operand 已接入
            // lower_short_circuit_binary，但条件路径（if/while/for）走本函数，
            // 此前漏接导致急切求值。此处与 lower_arg_operand 对齐。
            if matches!(op, BinOp::And | BinOp::Or) {
                let (prep, rv) =
                    lower_short_circuit_binary(*op, &left.node, &right.node, builder, ctx);
                let op = match rv {
                    MirRvalue::Use(op) => op,
                    _ => unreachable!("lower_short_circuit_binary returns Use(_sc local)"),
                };
                return (prep, op);
            }
            let (mut prep, left_op) = lower_expr_to_operand(builder, &left.node, ctx);
            let (prep_r, right_op) = lower_expr_to_operand(builder, &right.node, ctx);
            prep.extend(prep_r);
            let result_ty = infer_type_from_expr(expr, ctx);
            let tmp = builder.fresh_local(&"_bin".into(), result_ty, ctx.locals);
            prep.push(MirStatement::Assign {
                place: tmp,
                rvalue: MirRvalue::Binary {
                    op: *op,
                    left: left_op,
                    right: right_op,
                },
            });
            (prep, MirOperand::Local(tmp))
        }
        Expr::Unary { op, expr } => {
            let (mut prep, inner_op) = lower_expr_to_operand(builder, &expr.node, ctx);
            let result_ty = match op {
                UnaryOp::Not => TypeId::Bool,
                // RFC 009 P1-F #8：`in`/`ref`/`out` 参数类型为 `TypeId::Ref`；
                // 一元求负结果应为解引用后的内部类型（如 `int`），否则临时
                // local 被分配为 `ptr` 槽，codegen 物化 `-v` 时会从未初始化的
                // ptr 槽加载地址并存储，导致运行时崩溃。
                UnaryOp::Neg => match infer_type_from_spanned(expr, ctx) {
                    TypeId::Ref { inner, .. } => *inner,
                    other => other,
                },
                UnaryOp::BitNot => match infer_type_from_spanned(expr, ctx) {
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
                // `~x == x ^ -1`（二补码恒等），复用 BitXor 现有 codegen。
                UnaryOp::BitNot => MirRvalue::Binary {
                    op: BinOp::BitXor,
                    left: MirOperand::ConstInt(-1),
                    right: inner_op,
                },
            };
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            (prep, MirOperand::Local(tmp))
        }
        _ => {
            let ty = infer_type_from_expr(expr, ctx);
            let (mut prep, rvalue) = lower_expr_to_rvalue_with_binary(expr, builder, ctx);
            let tmp = builder.fresh_local(&"_tmp".into(), ty, ctx.locals);
            prep.push(MirStatement::Assign { place: tmp, rvalue });
            (prep, MirOperand::Local(tmp))
        }
    }
}

pub(super) fn lower_expr_to_rvalue_with_binary(
    expr: &Expr,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirRvalue) {
    // RFC 004 M2：variant 构造（`Value.Null` / `Value.Int(42)` / `Shape.Rect(new Point{...})`）
    // M2 struct payload 走 variant_construct_rvalue_with_prep 进行栈分配。
    if let Some((prep, rv)) = variant_construct_rvalue_with_prep(expr, builder, ctx) {
        return (prep, rv);
    }
    if let Expr::SwitchForm(s) = expr {
        let (prep, op) = super::lower_match::lower_switch_form(builder, s, ctx);
        return (prep, MirRvalue::Use(op));
    }
    // RFC 006 G2：Lambda 作为通用右值（字段/局部直接赋值 `_cb = () => this.OnX()`）
    // 走闭包物化，避免落入 operand_from_expr 的 `Discriminant(Lambda)` panic。
    if let Expr::Lambda(l) = expr {
        let op = builder.lower_lambda_to_fnptr(l, ctx, None);
        return (vec![], MirRvalue::Use(op));
    }
    // RFC 004 M4：表达式块（位置模式 switch 臂脱糖体）——先降级 stmts，再取 tail 为值。
    if let Expr::Block(b) = expr {
        return lower_block_expr(b, builder, ctx);
    }
    if let Expr::CollectionExpr { elements } = expr {
        return lower_collection_expr(elements, builder, ctx);
    }
    // `new T[n]` — 运行时长度零初始化堆数组分配。
    if let Expr::NewArray { elem_type, length } = expr {
        let (mut prep, len_op) = lower_arg_operand(builder, &length.node, ctx);
        let elem_ty = lower_type_name(&elem_type.node);
        let arr_ty = TypeId::Array {
            elem: Box::new(elem_ty.clone()),
        };
        let tmp = builder.fresh_local(&"_newarr".into(), arr_ty, ctx.locals);
        prep.push(MirStatement::Assign {
            place: tmp,
            rvalue: MirRvalue::NewArray {
                elem_type: elem_ty,
                length: len_op,
            },
        });
        return (prep, MirRvalue::Use(MirOperand::Local(tmp)));
    }
    // RFC 005 params@Span：栈缓冲脱糖（元素先物化为 operand）。
    if let Expr::StackSpanLit {
        elements,
        mutable,
        elem,
    } = expr
    {
        let mut prep = Vec::new();
        let mut ops = Vec::with_capacity(elements.len());
        for e in elements {
            let (mut p, op) = lower_arg_operand(builder, &e.node, ctx);
            prep.append(&mut p);
            ops.push(op);
        }
        return (
            prep,
            MirRvalue::SpanFromStack {
                elements: ops,
                elem_type: elem.clone(),
                mutable: *mutable,
            },
        );
    }
    if let Expr::Binary { op, left, right } = expr {
        if matches!(op, BinOp::And | BinOp::Or) {
            return lower_short_circuit_binary(*op, &left.node, &right.node, builder, ctx);
        }
        let (mut prep_l, l) = lower_arg_operand(builder, &left.node, ctx);
        let (prep_r, r) = lower_arg_operand(builder, &right.node, ctx);
        prep_l.extend(prep_r);
        return (
            prep_l,
            MirRvalue::Binary {
                op: *op,
                left: l,
                right: r,
            },
        );
    }
    if let Expr::Unary { op, expr: inner } = expr {
        let (prep, inner_op) = lower_arg_operand(builder, &inner.node, ctx);
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
            // `~x == x ^ -1`（二补码恒等）。
            UnaryOp::BitNot => MirRvalue::Binary {
                op: BinOp::BitXor,
                left: MirOperand::ConstInt(-1),
                right: inner_op,
            },
        };
        return (prep, rvalue);
    }
    // RFC 036 M2: `is` 表达式——内层须经 lower_arg_operand 物化，禁止
    // lower_expr_to_rvalue_simple → operand_from_expr（复杂内层 Binary 会 panic）。
    if let Expr::Is {
        expr: inner,
        pattern,
    } = expr
    {
        let (mut prep, inner_op) = lower_arg_operand(builder, &inner.node, ctx);
        let expr_ty = infer_type_from_spanned(inner, ctx);
        // 内层静态类型为接口时，inner_op 是接口胖指针 `{ ptr obj, ptr itable }`；
        // rt_obj_isa 需要底层对象指针（vtable slot0 取 typeinfo），须先
        // UnboxIface 取首槽，否则把胖指针盒当类对象 → 0xC0000005。
        let isa_operand = match &expr_ty {
            TypeId::Named(n) if ctx.registry.is_interface(n) => MirOperand::UnboxIface {
                object: Box::new(inner_op.clone()),
                class: n.to_string(),
            },
            _ => inner_op.clone(),
        };
        // C# 9 逻辑组合（and/or/not）在 typeck 校验后于此处递归 lower：
        // 组合子结果物化为临时 bool 后按 `&&`/`||`/`!` 组合成 rvalue。
        let (mut rprep, rv) =
            lower_is_pattern_rvalue(builder, &inner_op, &isa_operand, &expr_ty, pattern, ctx);
        prep.append(&mut rprep);
        return (prep, rv);
    }
    // For Field with potentially complex receivers (e.g. `list[0].Value`
    // where the receiver is itself a MethodCall), use `lower_arg_operand`
    // which properly lowers sub-expressions to temp locals.
    // NOTE: Do NOT route Index here — `lower_arg_operand`'s final fallback
    // calls this function, creating infinite recursion.
    if let Expr::Field { receiver, field } = expr {
        // RFC 005：`Span<T>.Empty` / `ReadOnlySpan<T>.Empty` → 空栈 Span。
        if let Expr::Call {
            func,
            args: ref ca,
            type_args,
            ..
        } = &receiver.node
        {
            if ca.is_empty() && type_args.len() == 1 && field.as_str() == "Empty" {
                if let Expr::Ident(name) = &func.node {
                    if name.as_str() == "Span" || name.as_str() == "ReadOnlySpan" {
                        let elem = lower_type::lower_type_name(&type_args[0].node);
                        return (
                            vec![],
                            MirRvalue::SpanFromStack {
                                elements: vec![],
                                elem_type: elem,
                                mutable: name.as_str() == "Span",
                            },
                        );
                    }
                }
            }
        }
        let (prep, op) = lower_arg_operand(builder, expr, ctx);
        return (prep, MirRvalue::Use(op));
    }
    // MethodCall needs prep-statement propagation so that complex argument
    // expressions (e.g. `people.Add(new Person() { ... })` as a sub-expression
    // of `var x = obj.Foo(people.Add(...))`) are materialized to temp locals
    // with their prep statements preserved. `method_call_rvalue_with_prep`
    // internally uses `lower_arg_operand`, whose fallback routes non-leaf
    // args back through this function — recursion terminates because each
    // recursive call processes strictly smaller sub-expressions.
    if let Expr::MethodCall {
        receiver,
        method,
        args,
        type_args,
        params_span,
    } = expr
    {
        // RFC 004 M2：variant 有 payload case 构造（`Value.Int(42)`）。
        // Parser 将 `Type.Case(payload)` 解析为 MethodCall，需在通用 MethodCall
        // lowering 之前拦截，避免生成 `@unknown_Case` 函数调用。
        if let Some((prep, rv)) = variant_construct_rvalue_with_prep(expr, builder, ctx) {
            return (prep, rv);
        }
        // LINQ 终端：`Any` / `Count` / `First`（数组 + List；MIR 编译期展开）。
        // 须先于 facade / 实例方法路径——否则会落入「未知方法」或错误 Call。
        if let Some((chain, kind)) = lower_linq::try_parse_linq_terminal(expr, ctx) {
            if let Some((prep, local)) = builder.lower_linq_terminal(kind, chain, ctx) {
                return (prep, MirRvalue::Use(MirOperand::Local(local)));
            }
        }
        // Facade builtin 拦截优先：`File.ReadAllText` / `Console.WriteLine` /
        // `Task.FromResult` / `Assert.Equal` 等 stub 类的静态方法必须先于此处的
        // `user_type_static_method_func` 检查——这些 stub 类在 registry 注册为
        // 普通 `Class`，但其方法体为空（行为靠 codegen 拦截器发射 ABI 实现）。
        // 若 `user_type_static_method_func` 先返回 `Class::Method`（mangled
        // 用户函数），会绕过 codegen 拦截器，生成空 stub 调用（如
        // `@File_ReadAllText`），导致 `var x = File.ReadAllText(...)` 等表达式路径
        // 拿到错误结果。与 `lower.rs` MethodCall 语句路径的检查顺序对齐。
        if let Some(func) = lower_linq::builtin_static_method(&receiver.node, method) {
            let mut prep: Vec<MirStatement> = Vec::new();
            let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
            for a in args {
                let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
                prep.append(&mut p);
                call_args.push(op);
            }
            return (
                prep,
                MirRvalue::Call {
                    func,
                    args: call_args,
                },
            );
        }
        // RFC 005：数组 → Span / Span.Slice。
        {
            let recv_ty = lower_type::infer_type_from_spanned(receiver, ctx);
            match (method.as_str(), &recv_ty) {
                ("AsSpan", TypeId::Array { .. }) => {
                    let (mut prep, array) = lower_arg_operand(builder, &receiver.node, ctx);
                    let (start, length) = match args.len() {
                        0 => (None, None),
                        2 => {
                            let (p0, s) = lower_arg_operand(builder, &args[0].node, ctx);
                            let (p1, l) = lower_arg_operand(builder, &args[1].node, ctx);
                            prep.extend(p0);
                            prep.extend(p1);
                            (Some(s), Some(l))
                        }
                        _ => (None, None),
                    };
                    return (
                        prep,
                        MirRvalue::SpanFromArray {
                            array,
                            start,
                            length,
                            mutable: true,
                        },
                    );
                }
                ("AsReadOnlySpan", TypeId::Array { .. }) => {
                    let (prep, array) = lower_arg_operand(builder, &receiver.node, ctx);
                    return (
                        prep,
                        MirRvalue::SpanFromArray {
                            array,
                            start: None,
                            length: None,
                            mutable: false,
                        },
                    );
                }
                ("Slice", TypeId::Span { mutable, .. }) => {
                    let (mut prep, span) = lower_arg_operand(builder, &receiver.node, ctx);
                    let (p0, start) = lower_arg_operand(builder, &args[0].node, ctx);
                    prep.extend(p0);
                    // `Slice(start)` 单参 = 切片到末尾（length: None，codegen 计算 len-start）。
                    let length = if args.len() >= 2 {
                        let (p1, length) = lower_arg_operand(builder, &args[1].node, ctx);
                        prep.extend(p1);
                        Some(length)
                    } else {
                        None
                    };
                    return (
                        prep,
                        MirRvalue::SpanSlice {
                            span,
                            start,
                            length,
                            mutable: *mutable,
                        },
                    );
                }
                ("AsReadOnly", TypeId::Span { mutable: true, .. }) => {
                    let (prep, span) = lower_arg_operand(builder, &receiver.node, ctx);
                    return (prep, MirRvalue::Use(span));
                }
                ("CopyTo", TypeId::Span { elem, .. }) => {
                    let (mut prep, src) = lower_arg_operand(builder, &receiver.node, ctx);
                    let (p0, dest) = lower_arg_operand(builder, &args[0].node, ctx);
                    prep.extend(p0);
                    return (
                        prep,
                        MirRvalue::SpanCopyTo {
                            src,
                            dest,
                            elem_type: *elem.clone(),
                        },
                    );
                }
                (
                    "Fill",
                    TypeId::Span {
                        elem,
                        mutable: true,
                        ..
                    },
                ) => {
                    let (mut prep, span) = lower_arg_operand(builder, &receiver.node, ctx);
                    let (p0, value) = lower_arg_operand(builder, &args[0].node, ctx);
                    prep.extend(p0);
                    return (
                        prep,
                        MirRvalue::SpanFill {
                            span,
                            value,
                            elem_type: *elem.clone(),
                        },
                    );
                }
                (
                    "Clear",
                    TypeId::Span {
                        elem,
                        mutable: true,
                        ..
                    },
                ) => {
                    let (prep, span) = lower_arg_operand(builder, &receiver.node, ctx);
                    return (
                        prep,
                        MirRvalue::SpanClear {
                            span,
                            elem_type: *elem.clone(),
                        },
                    );
                }
                ("TryCopyTo", TypeId::Span { elem, .. }) => {
                    let (mut prep, src) = lower_arg_operand(builder, &receiver.node, ctx);
                    let (p0, dest) = lower_arg_operand(builder, &args[0].node, ctx);
                    prep.extend(p0);
                    return (
                        prep,
                        MirRvalue::SpanTryCopyTo {
                            src,
                            dest,
                            elem_type: *elem.clone(),
                        },
                    );
                }
                ("ToArray", TypeId::Span { elem, .. }) => {
                    let (prep, span) = lower_arg_operand(builder, &receiver.node, ctx);
                    return (
                        prep,
                        MirRvalue::SpanToArray {
                            span,
                            elem_type: *elem.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        // RFC 004 M2：用户类型静态方法调用（如 `Vector2.Add(a, b)`）——
        // 优先于实例方法路径，避免 receiver 被物化为 `ConstInt(0)` 充当 `this`。
        // 静态方法无 `this` 参数，降级为 `MirRvalue::Call`（无 receiver）。
        let stripped_type_args: Vec<ast::Type> = type_args.iter().map(|t| t.node.clone()).collect();
        if let Some((func, params)) = lower_call::user_type_static_method_sig(
            &receiver.node,
            method,
            &stripped_type_args,
            args,
            ctx,
        ) {
            let mut prep: Vec<MirStatement> = Vec::new();
            let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
            for (i, a) in args.iter().enumerate() {
                let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
                prep.append(&mut p);
                // RFC 039：静态方法接口形参须包装 class 实参为接口胖指针。
                let arg_ty = type_name_from_operand(&op, &a.node, ctx);
                let op = if let Some(pt) = params.get(i) {
                    lower_call::maybe_box_iface(op, &arg_ty, &TypeId::Named(pt.clone()), ctx)
                } else {
                    op
                };
                call_args.push(op);
            }
            return (
                prep,
                MirRvalue::Call {
                    func,
                    args: call_args,
                },
            );
        }
        // Use `lower_arg_operand` (not `operand_from_expr`) for the receiver so
        // that chained calls (e.g. `sb.Append("a").Append("b")`) materialize
        // the inner MethodCall to a temp local instead of collapsing to
        // `ConstInt(0)`. `lower_arg_operand`'s fallback routes MethodCall back
        // through this function — recursion terminates because each recursive
        // call processes strictly smaller sub-expressions.
        let (mut recv_prep, recv) = lower_arg_operand(builder, &receiver.node, ctx);
        // 委托接收者调用（`d.Invoke(5)` / 委托字段 `g.Convert(5)`）：receiver 是
        // 委托 → 间接调用 `IndirectCall`，禁止视为 MethodCall 静态分派——否则
        // `Convert` 非 `Gp5` 的方法，按方法名 mangle 产出未定义符号 `@g_Convert`
        // （arc-prune-001）。与 `try_lower_delegate_invoke` 的接收者物化对
        //（非 Local 字段须暂存到临时，便于 codegen `closure_locals` 与 ABI）。
        let recv_ty_id = lower_type::infer_type_from_spanned(receiver, ctx);
        if lower_type::is_delegate_type(&recv_ty_id) {
            let func_local = match recv {
                MirOperand::Local(id) => id,
                other => {
                    let id = builder.fresh_local(&"_dlg".into(), recv_ty_id, ctx.locals);
                    recv_prep.push(MirStatement::Assign {
                        place: id,
                        rvalue: MirRvalue::Use(other),
                    });
                    id
                }
            };
            let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
            for a in args {
                let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
                recv_prep.append(&mut p);
                call_args.push(op);
            }
            return (
                recv_prep,
                MirRvalue::IndirectCall {
                    func: MirOperand::Local(func_local),
                    args: call_args,
                },
            );
        }
        let recv_ty = type_name_from_operand(&recv, &receiver.node, ctx);
        let (mut prep, rvalue) = lower_call::method_call_rvalue_with_prep(
            builder,
            receiver,
            method,
            args,
            &stripped_type_args,
            params_span.as_ref(),
            ctx,
            recv,
            &recv_ty,
        );
        recv_prep.append(&mut prep);
        return (recv_prep, rvalue);
    }
    // Free-function / static Call：实参须经 `lower_arg_operand` 物化，禁止
    // `lower_call_args_simple` → `operand_from_expr` 静默 0/null。
    if let Expr::Call {
        func,
        args,
        type_args,
        params_span,
    } = expr
    {
        if let Expr::Ident(name) = &func.node {
            if let Some(local_id) = ctx.lookup(name) {
                let is_func_local = ctx
                    .locals
                    .get(&local_id)
                    .map(|(_, ty)| lower_type::is_delegate_type(ty))
                    .unwrap_or(false);
                if is_func_local {
                    let mut prep: Vec<MirStatement> = Vec::new();
                    let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
                    for a in args {
                        let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
                        prep.append(&mut p);
                        call_args.push(op);
                    }
                    return (
                        prep,
                        MirRvalue::IndirectCall {
                            func: MirOperand::Local(local_id),
                            args: call_args,
                        },
                    );
                }
            }
            // 实例委托字段（`_f(x)` 裸调用）→ IndirectCall；禁止自由函数 Call("_f")。
            // 与 lower.rs 语句上下文对称，见 lower_call::try_lower_delegate_invoke。
            if let Some((dprep, drv)) = try_lower_delegate_invoke(builder, func, args, ctx) {
                return (dprep, drv);
            }
            let func_name = if !type_args.is_empty() {
                resolve_instantiated_type_name_from_args(name, type_args)
            } else if !ctx.fn_sigs.contains_key(name.as_str()) {
                resolve_class_static_method(name, args, ctx).unwrap_or_else(|| name.to_string())
            } else {
                name.to_string()
            };
            // 裸实例方法调用（`_bump()` → `this._bump()`）：与 typeck 对齐，
            // C# 允许在实例方法内省略 `this.` 直接调用同 class（含基类）实例
            // 方法。若名字未命中自由函数/静态方法，且当前类存在 arity 匹配的
            // 实例方法，则重写为 `Expr::MethodCall { receiver: this }` 递归
            // 降级，确保 `target_fn` 为 `Owner::Method`（而非裸名 `_bump`）。
            if func_name == *name && type_args.is_empty() {
                if let Some(owner) = ctx.owner.clone() {
                    if mir_has_instance_method(ctx.registry, &owner, name, args.len())
                        && ctx.lookup(name).is_none()
                    {
                        let mc = Expr::MethodCall {
                            receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                            method: name.clone(),
                            args: args.clone(),
                            type_args: type_args.clone(),
                            params_span: None,
                        };
                        return lower_expr_to_rvalue_with_binary(&mc, builder, ctx);
                    }
                }
            }
            let (prep, call_args) =
                lower_call::lower_call_args(builder, name, args, params_span.as_ref(), ctx);
            return (
                prep,
                MirRvalue::Call {
                    func: func_name,
                    args: call_args,
                },
            );
        }
        // 委托字段调用（typeck 已把 `g.Convert(5)` 改写为 `Call{func: Field}`）：
        // 委托类型字段 → IndirectCall。须先于下方「`recv_name::field` 静态方法」
        // 回退——否则实例局部 `g` 会被视作类型名，mangle 出 `g::Convert` →
        // 未定义符号 `@g_Convert`（arc-prune-001）。与语句级路径 `lower.rs`
        // `Expr::Call`（`try_lower_delegate_invoke`）对称。
        if let Some((dprep, drv)) = try_lower_delegate_invoke(builder, func, args, ctx) {
            return (dprep, drv);
        }
        if let Expr::Field { receiver, field } = &func.node {
            if let Expr::Ident(recv_name) = &receiver.node {
                let class: Ident = class_from_expr(&receiver.node, ctx).into();
                if ctx.registry.is_class(&class) {
                    let func = format!("{recv_name}::{field}");
                    let (prep, call_args) = lower_call::lower_call_args(
                        builder,
                        field,
                        args,
                        params_span.as_ref(),
                        ctx,
                    );
                    return (
                        prep,
                        MirRvalue::Call {
                            func,
                            args: call_args,
                        },
                    );
                }
            }
        }
        let (mut prep, func_op) = lower_arg_operand(builder, &func.node, ctx);
        let mut call_args: Vec<MirOperand> = Vec::with_capacity(args.len());
        for a in args {
            let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
            prep.append(&mut p);
            call_args.push(op);
        }
        return (
            prep,
            MirRvalue::IndirectCall {
                func: func_op,
                args: call_args,
            },
        );
    }
    // 赋值表达式（值语义）：RHS 先求值并物化到伪局部——赋值表达式的值 =
    // 写入后的 RHS，且写入分派不得对 RHS 二次求值（Call/属性 getter 会双
    // 触发）。写入复用语句级 `TypedStmt::Assign` 分派（局部/静态/实例字段、
    // SoA 融合、属性 setter、索引器、NullCond 单一事实源）：value 以伪局部
    // 标识传入，语句级各分支经 ctx.lookup 命中该局部，零重复降级。
    if let Expr::Assign { target, value } = expr {
        let (mut prep, rv) = lower_expr_to_rvalue_with_binary(&value.node, builder, ctx);
        let vty = lower_type::infer_type_from_spanned(value, ctx);
        let tmp = builder.fresh_local(&"_assign_val".into(), vty, ctx.locals);
        prep.push(MirStatement::Assign {
            place: tmp,
            rvalue: rv,
        });
        let pseudo: Ident = Ident::from(format!("_assign_val_{}", tmp.0).as_str());
        ctx.bind(&pseudo, tmp);
        let tmp_value = Spanned::new(Expr::Ident(pseudo), value.span);
        let block = typeck::TypedBlock {
            stmts: vec![typeck::TypedStmt::Assign {
                target: (**target).clone(),
                value: tmp_value,
            }],
            tail: None,
        };
        let mut writes = builder.lower_typed_block(&block, ctx);
        prep.append(&mut writes);
        return (prep, MirRvalue::Use(MirOperand::Local(tmp)));
    }
    // C# 委托实例化（`new Action(() => {...})` / `new Func<...>(lambda)`）：
    // 委托不是类——lambda 闭包值即委托值，禁走 `MirRvalue::New`（不存在
    // `__ctor_Action_*` 可发射 → arc-prune-001）。非 lambda 实参（方法组）
    // 维持类路径，由完整性门响亮报错（避免静默错误 ABI）。
    if let Expr::New { ty, args, .. } = expr {
        if lower_type::is_delegate_type(&lower_type_name(&ty.node)) {
            if let [arg] = args.as_slice() {
                if let Expr::Lambda(l) = &arg.node {
                    let op = builder.lower_lambda_to_fnptr(l, ctx, None);
                    return (vec![], MirRvalue::Use(op));
                }
            }
        }
    }
    // Class object construction（含 `new X(...)` 与 `new X(...) { Field = v }`）：
    // args 经 `lower_arg_operand` 物化，使内层 `new`（如 `new Outer(new Inner())`）
    // 正确降级到临时 local，而非被 `operand_from_expr` 兜底为 ConstInt(0)/ConstNull。
    // Struct-with-obj_init 仍走 `lower_expr_to_rvalue_simple` 的 StructLit 路径。
    if let Expr::New { ty, args, obj_init } = expr {
        let class_name =
            typeck::resolve_instantiated_type_name(&ty.node).unwrap_or_else(|| match &ty.node {
                Type::Named { path, .. } => path
                    .last()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                _ => "unknown".into(),
            });
        let class_ident: Ident = class_name.clone().into();
        let is_struct_with_init = matches!(
            obj_init,
            Some(fields) if !fields.is_empty() && ctx.registry.is_struct(&class_ident)
        );
        if !is_struct_with_init {
            let mut prep: Vec<MirStatement> = Vec::new();
            let mut arg_ops: Vec<MirOperand> = Vec::with_capacity(args.len());
            for a in args {
                let (mut p, op) = lower_arg_operand(builder, &a.node, ctx);
                prep.append(&mut p);
                arg_ops.push(op);
            }
            // 形参为接口时把具体类实参装箱为接口胖指针（见 box_ctor_args）。
            arg_ops = box_ctor_args(&class_ident, args, arg_ops, ctx);
            if let Some(fields) = obj_init {
                if !fields.is_empty() {
                    let class_ty = TypeId::Named(class_ident.clone());
                    let tmp = builder.fresh_local(&"_new".into(), class_ty, ctx.locals);
                    let ctor_params = resolve_ctor_params(&class_ident, args, &arg_ops, ctx);
                    prep.push(MirStatement::Assign {
                        place: tmp,
                        rvalue: MirRvalue::New {
                            class: class_name.clone(),
                            args: arg_ops,
                            ctor_params,
                        },
                    });
                    for (fname, fexpr) in fields {
                        // RFC 006 M5：自定义 get/init|set 属性走 set_*，auto-property 仍 FieldSet。
                        if is_custom_accessor_property(ctx.registry, &class_name, fname) {
                            let (mut fprep, fop) =
                                lower_call::lower_arg_operand(builder, &fexpr.node, ctx);
                            prep.append(&mut fprep);
                            let setter = format!("set_{fname}");
                            let set_params =
                                vec![lower_type::type_name_from_operand(&fop, &fexpr.node, ctx)
                                    .to_string()];
                            let (impl_class, target_fn) = resolve_method_target(
                                ctx.registry,
                                &class_ident,
                                &setter.clone().into(),
                                ctx.owner.clone(),
                            );
                            let is_virtual =
                                is_virtual_member(ctx.layouts, &class_name, &setter, &set_params);
                            let place =
                                builder.fresh_local(&"_setprop".into(), TypeId::Void, ctx.locals);
                            prep.push(MirStatement::Assign {
                                place,
                                rvalue: MirRvalue::MethodCall {
                                    receiver: MirOperand::Local(tmp),
                                    method: setter,
                                    args: vec![fop],
                                    receiver_type: class_name.clone(),
                                    impl_class,
                                    target_fn,
                                    is_virtual,
                                    params: set_params,
                                },
                            });
                        } else {
                            let (mut fprep, frv) =
                                lower_expr_to_rvalue_with_binary(&fexpr.node, builder, ctx);
                            prep.append(&mut fprep);
                            prep.push(MirStatement::FieldSet {
                                object: MirOperand::Local(tmp),
                                class: class_name.clone(),
                                field: fname.to_string(),
                                value: frv,
                            });
                        }
                    }
                    return (prep, MirRvalue::Use(MirOperand::Local(tmp)));
                }
            }
            // 普通构造（无 obj_init 或空 obj_init）：直接返回 New rvalue，由调用方物化。
            let ctor_params = resolve_ctor_params(&class_ident, args, &arg_ops, ctx);
            return (
                prep,
                MirRvalue::New {
                    class: class_name,
                    args: arg_ops,
                    ctor_params,
                },
            );
        }
    }
    // Cast：内层可能是 Binary / New / MethodCall，须经 lower_arg_operand 物化。
    if let Expr::Cast { ty, expr: inner } = expr {
        let (prep, op) = lower_arg_operand(builder, &inner.node, ctx);
        if let Some(class) = iface_to_class_cast_target(&ty.node, &inner.node, ctx) {
            return (
                prep,
                MirRvalue::Use(MirOperand::UnboxIface {
                    object: Box::new(op),
                    class,
                }),
            );
        }
        // RFC 045 P2：object→string/数值拆箱——is string / is int 收窄产生的
        // 窄化 Cast（typeck Ident 窄化重写，不经 Cast→Unbox 转换路径）。与 typeck
        // 转的 Expr::Unbox 同语义（rt_string_unbox / rt_box_unbox）；须先于数值
        // 转换分支——窄化目标 int 时值是 ArcBox，按标量物化会直读盒头。
        let target_ty = lower_type_name(&ty.node);
        if is_object_typed(&inner.node, ctx)
            && (target_ty == TypeId::String || is_numeric_type_id(&target_ty))
        {
            return (prep, MirRvalue::Unbox { src: op, target_ty });
        }
        // 数值类型转换（(long)a / (double)i 等）：物化为目标类型 temp local，
        // 保留转换语义。否则 Cast 被丢弃，codegen 按内层操作数实际类型推断
        // binary 运算——(long)a * 16777216 被按 i32 乘法（溢出）→ 高位符号
        // 扩展失真（barcode argb 打包错位 / 数值表达式溢出的根因）。
        if is_numeric_type_id(&target_ty) {
            let tmp = builder.fresh_local(&"_cast".into(), target_ty, ctx.locals);
            let mut prep = prep;
            prep.push(MirStatement::Assign {
                place: tmp,
                rvalue: MirRvalue::Use(op),
            });
            return (prep, MirRvalue::Use(MirOperand::Local(tmp)));
        }
        // 泛型型参 cast：`(T)boxed` 的 T 为型参时（模板 lowering 阶段未知具体
        // 类型），保留型参名经单态化替换，codegen 按具体类型发射拆箱——与
        // operand_from_expr 的 UnboxGeneric 同机制。须在数值转换之后（型参
        // 不是数值 type id，不会提前落入）；否则落入下面 `Use(op)` 透传，
        // 值类型 T 单态化后发射 `ret ptr` 与函数结果类型不匹配。
        if let TypeId::Named(name) = &target_ty {
            // `NativePtr` 是内建 OpaquePtr FFI 类型（非 ARC 引用类，天然不在
            // registry.types）。`(NativePtr)longValue` 是整型↔指针 reinterpret，
            // 须透传为 `Use(op)` 交由 codegen `coerce_value` 发射 `inttoptr`；
            // 若落到 UnboxGeneric 会把 long 当盒对象解引用（返回 ptr 存 i64 值
            // → LLVM 「defined with type i64 but expected ptr」）。NativePtr 恒
            // 非型参，明确排除。
            if name.as_str() != "NativePtr" && !ctx.registry.types.contains_key(name.as_str()) {
                return (
                    prep,
                    MirRvalue::Use(MirOperand::UnboxGeneric {
                        object: Box::new(op),
                        type_name: name.to_string(),
                    }),
                );
            }
        }
        return (prep, MirRvalue::Use(op));
    }
    // Index：receiver / index 均可能含 Binary / MethodCall，须经 lower_arg_operand
    // 物化。不在此对整个 Index 调 lower_arg_operand（其 fallback 会回到本函数）。
    if let Expr::Index { receiver, index } = expr {
        let (mut prep, recv_op) = lower_arg_operand(builder, &receiver.node, ctx);
        let (prep_i, idx_op) = lower_arg_operand(builder, &index.node, ctx);
        prep.extend(prep_i);
        // Builtin `string` 索引：`s[i]` → get_Chars → rt_str_char_at（与 C# Chars 对齐）。
        if lower_type::infer_type_from_spanned(receiver, ctx) == TypeId::String {
            return (
                prep,
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: "get_Chars".into(),
                    args: vec![idx_op],
                    receiver_type: "string".into(),
                    impl_class: Some("string".into()),
                    target_fn: Some("string::get_Chars".into()),
                    is_virtual: false,
                    params: vec!["int".into()],
                },
            );
        }
        let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
        if let Some(ix) = lower_type::resolve_indexer(&recv_class, &index.node, ctx) {
            return (
                prep,
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: ix.get.into(),
                    args: vec![idx_op],
                    receiver_type: recv_class.clone(),
                    impl_class: Some(recv_class.clone()),
                    target_fn: Some(format!("{recv_class}::{}", ix.get)),
                    is_virtual: false,
                    params: vec![],
                },
            );
        }
        return (
            prep,
            MirRvalue::IndexGet {
                array: recv_op,
                index: idx_op,
                elem_type: index_elem_type_non_indexer(receiver, ctx),
            },
        );
    }
    // RFC 009 M6：async lambda body 形如 `async () => await Foo()` —— await 作为
    // 表达式（非 let-init RHS、非 statement）出现。此处显式降级为 Await 语句 +
    // 临时 local，使状态机 lowering 能识别 entry 块中的 await（满足
    // `can_lower_as_state_machine` 条件 2），否则 fallback 到 ConstInt(0) 导致
    // async lambda 无效化为 `return 0`。
    if let Expr::Await(inner) = expr {
        // 推断 await 结果类型：inner 是 Task<T>，await 结果是 T。
        let result_ty = match lower_type::infer_type_from_spanned(inner, ctx) {
            TypeId::Task { inner } => *inner,
            other => other,
        };
        let place = builder.fresh_local(&"_await".into(), result_ty, ctx.locals);
        let (mut prep, task_rv) = lower_expr_to_rvalue_with_binary(&inner.node, builder, ctx);
        prep.push(MirStatement::Await {
            place,
            task: task_rv,
        });
        return (prep, MirRvalue::Use(MirOperand::Local(place)));
    }
    // Ternary: all three sub-expressions (condition, then-branch, else-branch)
    // must be lowered through lower_arg_operand.  operand_from_expr only handles
    // simple forms (Ident, BoolLit, IntLit, …) and falls back to ConstInt(0)
    // for anything non-trivial (Binary, MethodCall, nested Ternary, …).  Using
    // it for the condition would silently turn "a > b ? …" into "false ? …".
    if let Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    } = expr
    {
        let (mut prep, c) = lower_arg_operand(builder, &cond.node, ctx);
        let (prep_t, t) = lower_arg_operand(builder, &then_branch.node, ctx);
        let (prep_e, e) = lower_arg_operand(builder, &else_branch.node, ctx);
        prep.extend(prep_t);
        prep.extend(prep_e);
        return (
            prep,
            MirRvalue::Ternary {
                cond: c,
                then_val: t,
                else_val: e,
            },
        );
    }
    // Coalesce 与 Ternary 同理：`a ?? b` 两侧可为任意表达式（静态自定义属性
    // `culture ?? CultureInfo.CurrentUICulture`、方法调用、嵌套 Coalesce …），
    // 必须经 `lower_arg_operand` 物化（prep 语句 + 临时 local）。若落入
    // `lower_expr_to_rvalue_simple` 的 `operand_from_expr` 叶子路径，静态自定义
    // 属性（registry 中是 `get_X` 方法、非字段）会回退实例字段物化，receiver
    // `Ident(类名)` 解析失败 → "unresolved ident" ICE（typeck 已放行的合法表达式）。
    if let Expr::Coalesce { left, right } = expr {
        let (mut prep, l) = lower_arg_operand(builder, &left.node, ctx);
        let (prep_r, r) = lower_arg_operand(builder, &right.node, ctx);
        prep.extend(prep_r);
        return (prep, MirRvalue::Coalesce { left: l, right: r });
    }
    // RFC 006 M3：`(string)obj` / object→string 装箱的 src 可能是 custom accessor
    // property（如 `lv.SelectedItem`，getter 调 `GetValue<object>`）。必须经
    // `lower_arg_operand` 物化（发射 getter 调用 + 绑定临时 local），而非
    // `operand_from_expr` 的直接字段访问——后者会绕过 getter，把 object 属性
    // 误读为 i32 字段并传入 `rt_string_unbox(ptr)` 导致 clang 类型错误。
    if let Expr::Box { expr, value_ty } = expr {
        let (prep, src) = lower_arg_operand(builder, &expr.node, ctx);
        let src_ty = lower_type_name(&value_ty.node);
        return (prep, MirRvalue::Box { src, src_ty });
    }
    if let Expr::Unbox { expr, value_ty } = expr {
        let (prep, src) = lower_arg_operand(builder, &expr.node, ctx);
        let target_ty = lower_type_name(&value_ty.node);
        return (prep, MirRvalue::Unbox { src, target_ty });
    }
    // RFC 009 L2：嵌套 NullCond/ForceDeref 的 receiver 物化。`a?.B?.C` 的外层
    // receiver（`a?.B`）不是 operand_from_expr 可直接表达的简单形式，若直接落入
    // simple 层会触发 "unhandled expression Discriminant(37)" panic。此处将复杂
    // receiver 经 with_binary 降为前置 Assign + 临时 local，并把 receiver AST
    // 重写为伪 Ident（ctx.bind 进 scopes），使 simple 层既有 NullCond/ForceDeref
    // 路径（operand_from_expr → ctx.lookup、class_from_expr → type_id_to_name
    // 剥 Nullable）无需感知嵌套结构。
    if let Expr::NullCond { access } | Expr::ForceDeref { access } = expr {
        // access 非 Field/MethodCall 的形状（typeck 已放行）→ 防御性原样交回。
        let recv_expr = match &access.node {
            Expr::Field { receiver, .. } | Expr::MethodCall { receiver, .. } => receiver,
            _ => return (vec![], lower_expr_to_rvalue_simple(expr, builder, ctx)),
        };
        // 简单 receiver（operand_from_expr 快速通道覆盖的形式）→ 原样交回，
        // 保持既有直通路径（零回归）。
        if is_simple_null_cond_receiver(&recv_expr.node) {
            return (vec![], lower_expr_to_rvalue_simple(expr, builder, ctx));
        }
        let recv_ty = lower_type::infer_type_from_spanned(recv_expr, ctx);
        let (mut prep, recv_rv) = lower_expr_to_rvalue_with_binary(&recv_expr.node, builder, ctx);
        let pseudo: Ident = "_nullcond_recv".into();
        let tmp = builder.fresh_local(&pseudo, recv_ty, ctx.locals);
        prep.push(MirStatement::Assign {
            place: tmp,
            rvalue: recv_rv,
        });
        ctx.bind(&pseudo, tmp);
        // receiver AST 重写为伪 Ident：Clone access 后原地替换 receiver 节点。
        let mut access_clone = (**access).clone();
        if let Expr::Field { receiver, .. } | Expr::MethodCall { receiver, .. } =
            &mut access_clone.node
        {
            **receiver = Spanned::new(Expr::Ident(pseudo.clone()), Span::DUMMY);
        }
        let rewritten = match expr {
            Expr::NullCond { .. } => Expr::NullCond {
                access: Box::new(access_clone),
            },
            _ => Expr::ForceDeref {
                access: Box::new(access_clone),
            },
        };
        return (prep, lower_expr_to_rvalue_simple(&rewritten, builder, ctx));
    }
    (vec![], lower_expr_to_rvalue_simple(expr, builder, ctx))
}

/// RFC 009 L2：NullCond/ForceDeref receiver 是否为 operand_from_expr 快速通道
/// 覆盖的简单形式。与 operand_from_expr 的处理列表对齐（Unary 的常量折叠仅
/// 覆盖数值/bool 结果，不可能是引用 receiver，故不列入）。简单 receiver 保留
/// 既有直通路径（零回归）；复杂 receiver 由 with_binary 层物化为临时 local。
fn is_simple_null_cond_receiver(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::StringLit(_)
            | Expr::BoolLit(_)
            | Expr::CharLit(_)
            | Expr::Ident(_)
            | Expr::This
            | Expr::Base
            | Expr::RefArg { .. }
            | Expr::Null
            | Expr::Default { .. }
            | Expr::TypeOf(_)
            | Expr::Field { .. }
            | Expr::Cast { .. }
    )
}

/// Lower `a && b` / `a || b` with short-circuit evaluation via `MirStatement::If`.
/// `&&`: if a then result=b else result=false
/// `||`: if a then result=true else result=b
fn lower_short_circuit_binary(
    op: BinOp,
    left: &Expr,
    right: &Expr,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirRvalue) {
    let (mut prep, l_operand) = lower_arg_operand(builder, left, ctx);
    let sc_name: Ident = "_sc".into();
    let result_local = builder.fresh_local(&sc_name, TypeId::Bool, ctx.locals);
    let (then_body, else_body) = if matches!(op, BinOp::And) {
        let (mut prep_r, r_operand) = lower_arg_operand(builder, right, ctx);
        prep_r.push(MirStatement::Assign {
            place: result_local,
            rvalue: MirRvalue::Use(r_operand),
        });
        let else_body = vec![MirStatement::Assign {
            place: result_local,
            rvalue: MirRvalue::Use(MirOperand::ConstBool(false)),
        }];
        (prep_r, else_body)
    } else {
        let then_body = vec![MirStatement::Assign {
            place: result_local,
            rvalue: MirRvalue::Use(MirOperand::ConstBool(true)),
        }];
        let (mut prep_r, r_operand) = lower_arg_operand(builder, right, ctx);
        prep_r.push(MirStatement::Assign {
            place: result_local,
            rvalue: MirRvalue::Use(r_operand),
        });
        (then_body, prep_r)
    };
    prep.push(MirStatement::If {
        cond: l_operand,
        then_body,
        else_body,
    });
    (prep, MirRvalue::Use(MirOperand::Local(result_local)))
}

fn lower_collection_expr_simple(
    elements: &[ast::CollectionElement],
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> MirRvalue {
    // `ArrayLit.elem_type` 约定为**完整数组类型** `T[]`（与 let 覆写路径一致），
    // 以便 codegen 解包一层得到元素存储类型；嵌套 `[[…]]` 时元素亦为 `U[]`。
    let elem_type = collection_array_type(elements, ctx);
    MirRvalue::ArrayLit {
        elem_type,
        elements: elements
            .iter()
            .map(|el| match el {
                CollectionElement::Element(e) => {
                    ArrayLitElement::Value(lower_expr_to_rvalue_simple(&e.node, builder, ctx))
                }
                CollectionElement::Spread(e) => {
                    ArrayLitElement::Spread(operand_from_expr(&e.node, ctx))
                }
            })
            .collect(),
    }
}

fn lower_collection_expr(
    elements: &[ast::CollectionElement],
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirRvalue) {
    let elem_type = collection_array_type(elements, ctx);
    let mut prep = Vec::new();
    let mut out = Vec::new();
    for el in elements {
        match el {
            CollectionElement::Element(e) => {
                let (p, rv) = lower_expr_to_rvalue_with_binary(&e.node, builder, ctx);
                prep.extend(p);
                out.push(ArrayLitElement::Value(rv));
            }
            CollectionElement::Spread(e) => {
                let (p, op) = lower_expr_to_operand(builder, &e.node, ctx);
                prep.extend(p);
                out.push(ArrayLitElement::Spread(op));
            }
        }
    }
    (
        prep,
        MirRvalue::ArrayLit {
            elem_type,
            elements: out,
        },
    )
}

/// 集合表达式元素类型 `T`（spread 操作数解包一层）。
fn collection_elem_type(elements: &[ast::CollectionElement], ctx: &mut LowerCtx) -> TypeId {
    elements
        .iter()
        .map(|el| match el {
            CollectionElement::Element(e) => infer_type_from_spanned(e, ctx),
            CollectionElement::Spread(e) => match infer_type_from_spanned(e, ctx) {
                TypeId::Array { elem } => *elem,
                other => other,
            },
        })
        .next()
        .unwrap_or(TypeId::Named("object".into()))
}

/// `ArrayLit.elem_type`：完整 `T[]`（空集合默认 `object[]`）。
fn collection_array_type(elements: &[ast::CollectionElement], ctx: &mut LowerCtx) -> TypeId {
    TypeId::Array {
        elem: Box::new(collection_elem_type(elements, ctx)),
    }
}

/// C# 9 逻辑组合：`is` 模式 lower 为 bool rvalue。
///
/// - 单模式（叶子）直接产出 rvalue（无 prep）；
/// - `And` / `Or`：子结果物化为临时 bool 后按 `&&` / `||` 组合；
/// - `Not`：内层结果与 `false` 比较取反。
///
/// 内层 scrutinee 已由调用方物化（`inner_op`），组合子共享同一操作数，
/// 不重复求值。
fn lower_is_pattern_rvalue(
    builder: &mut MirBuilder,
    inner_op: &MirOperand,
    isa_operand: &MirOperand,
    expr_ty: &TypeId,
    pattern: &IsPattern,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirRvalue) {
    match pattern {
        // 单模式（叶子）
        IsPattern::Type { .. }
        | IsPattern::Var(_)
        | IsPattern::Null
        | IsPattern::Positional(_)
        | IsPattern::Constant(_) => {
            let rv = lower_is_leaf_rvalue(inner_op, isa_operand, expr_ty, pattern, ctx);
            (vec![], rv)
        }
        IsPattern::And { left, right } => {
            let (p1, lv) = lower_is_pattern_to_operand(
                builder,
                inner_op,
                isa_operand,
                expr_ty,
                &left.node,
                ctx,
            );
            let (p2, rv2) = lower_is_pattern_to_operand(
                builder,
                inner_op,
                isa_operand,
                expr_ty,
                &right.node,
                ctx,
            );
            let mut prep = p1;
            prep.extend(p2);
            (
                prep,
                MirRvalue::Binary {
                    op: BinOp::And,
                    left: lv,
                    right: rv2,
                },
            )
        }
        IsPattern::Or { left, right } => {
            let (p1, lv) = lower_is_pattern_to_operand(
                builder,
                inner_op,
                isa_operand,
                expr_ty,
                &left.node,
                ctx,
            );
            let (p2, rv2) = lower_is_pattern_to_operand(
                builder,
                inner_op,
                isa_operand,
                expr_ty,
                &right.node,
                ctx,
            );
            let mut prep = p1;
            prep.extend(p2);
            (
                prep,
                MirRvalue::Binary {
                    op: BinOp::Or,
                    left: lv,
                    right: rv2,
                },
            )
        }
        IsPattern::Not { inner } => {
            let (p, v) = lower_is_pattern_to_operand(
                builder,
                inner_op,
                isa_operand,
                expr_ty,
                &inner.node,
                ctx,
            );
            (
                p,
                MirRvalue::Binary {
                    op: BinOp::Eq,
                    left: v,
                    right: MirOperand::ConstBool(false),
                },
            )
        }
    }
}

/// 将 `is` 模式 lower 为 bool operand（物化为临时 local）。
fn lower_is_pattern_to_operand(
    builder: &mut MirBuilder,
    inner_op: &MirOperand,
    isa_operand: &MirOperand,
    expr_ty: &TypeId,
    pattern: &IsPattern,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    let (mut prep, rv) =
        lower_is_pattern_rvalue(builder, inner_op, isa_operand, expr_ty, pattern, ctx);
    let tmp = builder.fresh_local(&"_ispat".into(), TypeId::Bool, ctx.locals);
    prep.push(MirStatement::Assign {
        place: tmp,
        rvalue: rv,
    });
    (prep, MirOperand::Local(tmp))
}

/// `is` 单模式（叶子）lower 为 bool rvalue（不产生 prep 语句）。
///
/// 基元类型编译期折叠（同类型 true / 不同基元 false），避免对无 vtable 的
/// 基元调用 rt_obj_isa 崩溃；引用类型静态关系折叠（D8）；否则运行时 rt_obj_isa。
///
/// `isa_operand`：内层为接口时调用方传入 UnboxIface 取首槽的操作数
/// （否则与 `inner_op` 相同），用于 target 非具名类型分支。
fn lower_is_leaf_rvalue(
    inner_op: &MirOperand,
    isa_operand: &MirOperand,
    expr_ty: &TypeId,
    pattern: &IsPattern,
    ctx: &LowerCtx,
) -> MirRvalue {
    match pattern {
        IsPattern::Type { ty, .. } => {
            let target_type_name = lower_type_name(&ty.node);
            let target_name_str = type_id_name(&target_type_name);
            if is_primitive_type(expr_ty) && is_primitive_name(&target_name_str) {
                let same = type_id_name(expr_ty) == target_name_str;
                MirRvalue::Use(MirOperand::ConstBool(same))
            } else if let TypeId::Named(ref expr_name) = expr_ty {
                // RFC 018 D8 扩展：引用类型（class/interface）静态关系折叠。
                // 运行时 `is` 依赖 vtable slot0 的 typeinfo（rt_obj_isa）；
                // 无虚方法类 has_vtable=false，运行时无法判断类型，因此在
                // 静态类型关系已确定的场景直接编译期折叠：
                //   - expr 静态类型是测试类型的子类/相同 → 恒 true
                //   - 两者无继承关系 → 恒 false
                //   - 测试类型是 expr 静态类型的真子类 → 仍须运行时判断
                if let TypeId::Named(ref target_name) = target_type_name {
                    let up = ctx.registry.is_subtype(expr_name, target_name);
                    let down = ctx.registry.is_subtype(target_name, expr_name);
                    if up {
                        MirRvalue::Use(MirOperand::ConstBool(true))
                    } else if !down
                        && !ctx.registry.is_interface(target_name)
                        // 泛型型参目标不可折叠：registry 未注册「T」——它不是
                        // 具体类，单态化后才落定（TryGetComponent<T> 的
                        // `item is T`，T : IComponent 约束下运行期可能命中）。
                        // 须走 rt_obj_isa 运行期判定（TypeInfoPtr 随单态化替换，
                        // RFC 040 ConstDefault 同模式）；静态折叠恒 false 会使
                        // 泛型约束下的组件查找永不命中。
                        && ctx.registry.types.contains_key(target_name.as_str())
                    {
                        // 目标为具体类且与表达式静态类型无继承关系 → 恒 false。
                        // 目标为接口时不可折叠为 false：运行时子类可能实现该接口
                        //（`Widget w = new Impl(); w is IMarker`，Impl 实现 IMarker），
                        // 须走 rt_obj_isa 运行时接口遍历。
                        MirRvalue::Use(MirOperand::ConstBool(false))
                    } else {
                        let type_name = typeck::resolve_instantiated_type_name(&ty.node)
                            .unwrap_or_else(|| type_id_name(&target_type_name).to_string());
                        MirRvalue::Call {
                            func: "rt_obj_isa".into(),
                            args: vec![
                                // CD-14/D3：内层静态类型为接口时，`inner_op` 是
                                // fat pointer 盒地址，rt_obj_isa 需要底层对象指针
                                //（vtable slot0 取 typeinfo）。`isa_operand` 已由
                                // 调用方在接口静态类型时包 UnboxIface（取盒首槽），
                                // 此处必须用它而非 inner_op，否则 obj+8 读到 itable
                                // 指针当 vtable → 垃圾 typeinfo → is 结果错误/AV。
                                isa_operand.clone(),
                                MirOperand::TypeInfoPtr { type_name },
                            ],
                        }
                    }
                } else {
                    let type_name = typeck::resolve_instantiated_type_name(&ty.node)
                        .unwrap_or_else(|| type_id_name(&target_type_name).to_string());
                    MirRvalue::Call {
                        func: "rt_obj_isa".into(),
                        args: vec![isa_operand.clone(), MirOperand::TypeInfoPtr { type_name }],
                    }
                }
            } else {
                let type_name = typeck::resolve_instantiated_type_name(&ty.node)
                    .unwrap_or_else(|| type_id_name(&target_type_name).to_string());
                MirRvalue::Call {
                    func: "rt_obj_isa".into(),
                    args: vec![inner_op.clone(), MirOperand::TypeInfoPtr { type_name }],
                }
            }
        }
        IsPattern::Null => MirRvalue::Binary {
            op: BinOp::Eq,
            left: inner_op.clone(),
            right: MirOperand::ConstNull,
        },
        IsPattern::Var(_) => MirRvalue::Use(MirOperand::ConstBool(true)),
        // RFC 004 常量模式：`==` 值相等（string 经 codegen `rt_str_equals`，
        // 数值经 `icmp`，char 按 u32 判别值比较——对齐 switch 字面量臂）。
        IsPattern::Constant(c) => {
            let rhs = match &c.node {
                Expr::IntLit(n) => MirOperand::ConstInt(*n),
                Expr::BoolLit(b) => MirOperand::ConstBool(*b),
                Expr::CharLit(ch) => MirOperand::ConstInt(*ch as u32 as i64),
                Expr::StringLit(s) => MirOperand::ConstString(s.clone()),
                _ => MirOperand::ConstInt(0),
            };
            MirRvalue::Binary {
                op: BinOp::Eq,
                left: inner_op.clone(),
                right: rhs,
            }
        }
        IsPattern::Positional(_) => {
            panic!(
                "MIR lower: IsPattern::Positional reached MIR; \
                 typeck must desugar positional patterns (RFC 004 M3)"
            )
        }
        // 组合子由 lower_is_pattern_rvalue 处理，不会到达此处。
        IsPattern::And { .. } | IsPattern::Or { .. } | IsPattern::Not { .. } => {
            unreachable!("composite IsPattern must be handled by lower_is_pattern_rvalue")
        }
    }
}

fn lower_expr_to_rvalue_simple(
    expr: &Expr,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> MirRvalue {
    if let Some(op) = enum_variant_operand(expr, ctx.registry) {
        return MirRvalue::Use(op);
    }
    // RFC 004 M1：variant 构造（无 payload `Value.Null` / 有 payload `Value.Int(42)`）
    // M2 struct payload 走 lower_expr_to_rvalue_with_binary 路径（有 prep 语句）。
    if let Some(rv) = variant_construct_rvalue(expr, ctx) {
        return rv;
    }
    match expr {
        Expr::Call {
            func,
            args,
            type_args,
            params_span,
        } => {
            if let Expr::Ident(name) = &func.node {
                if let Some(local_id) = ctx.lookup(name) {
                    // RFC 037 M1: 局部变量持有委托（Func/Action）时，需间接调用。
                    // typeck 限制 #1：类方法级泛型参数未 push 到 type_param_scope，
                    // 导致 `Func<T, T, bool>` 等被 mangle 为 `Named("Func_T_T_bool")`
                    // 而非 `TypeId::Func { .. }`。识别 mangled 委托名以路由 IndirectCall。
                    let is_func_local = ctx
                        .locals
                        .get(&local_id)
                        .map(|(_, ty)| lower_type::is_delegate_type(ty))
                        .unwrap_or(false);
                    if is_func_local {
                        let func_operand = MirOperand::Local(local_id);
                        let args: Vec<MirOperand> = args
                            .iter()
                            .map(|a| operand_from_expr(&a.node, ctx))
                            .collect();
                        return MirRvalue::IndirectCall {
                            func: func_operand,
                            args,
                        };
                    }
                }
                let func_name = if !type_args.is_empty() {
                    resolve_instantiated_type_name_from_args(name, type_args)
                } else {
                    // If the bare name is not a free function, try resolving it
                    // as a static method on the current owning class.
                    // e.g. `Compare(5, 3)` inside `Program` → `Program::Compare`.
                    if !ctx.fn_sigs.contains_key(name.as_str()) {
                        resolve_class_static_method(name, args, ctx)
                            .unwrap_or_else(|| name.to_string())
                    } else {
                        name.to_string()
                    }
                };
                return MirRvalue::Call {
                    func: func_name,
                    args: lower_call_args_simple(name, args, params_span.as_ref(), ctx),
                };
            }
            if let Expr::Field { receiver, field } = &func.node {
                // Native module / static class method call: resolve to direct Call.
                // e.g. `rt_resources.rt_os_now_ticks()` → Call { func: "rt_resources.rt_os_now_ticks" }
                if let Expr::Ident(recv_name) = &receiver.node {
                    let class: Ident = class_from_expr(&receiver.node, ctx).into();
                    if ctx.registry.is_class(&class) {
                        let func = format!("{recv_name}::{field}");
                        return MirRvalue::Call {
                            func,
                            args: lower_call_args_simple(field, args, params_span.as_ref(), ctx),
                        };
                    }
                }
            }
            let func_operand = operand_from_expr(&func.node, ctx);
            let args: Vec<MirOperand> = args
                .iter()
                .map(|a| operand_from_expr(&a.node, ctx))
                .collect();
            MirRvalue::IndirectCall {
                func: func_operand,
                args,
            }
        }
        Expr::Binary { op, left, right } => MirRvalue::Binary {
            op: *op,
            left: operand_from_expr(&left.node, ctx),
            right: operand_from_expr(&right.node, ctx),
        },
        Expr::Unary { op, expr } => {
            let inner = operand_from_expr(&expr.node, ctx);
            match op {
                UnaryOp::Not => MirRvalue::Binary {
                    op: BinOp::Eq,
                    left: inner,
                    right: MirOperand::ConstBool(false),
                },
                UnaryOp::Neg => MirRvalue::Binary {
                    op: BinOp::Sub,
                    left: MirOperand::ConstInt(0),
                    right: inner,
                },
                // `~x == x ^ -1`（二补码恒等）。
                UnaryOp::BitNot => MirRvalue::Binary {
                    op: BinOp::BitXor,
                    left: MirOperand::ConstInt(-1),
                    right: inner,
                },
            }
        }
        Expr::New { ty, args, obj_init } => {
            // 委托实例化与 with_binary 层同语义：lambda 闭包值即委托值。
            if obj_init.is_none()
                && lower_type::is_delegate_type(&lower_type_name(&ty.node))
            {
                if let [arg] = args.as_slice() {
                    if let Expr::Lambda(l) = &arg.node {
                        let op = builder
                            .lower_lambda_to_fnptr(l, ctx, lambda_expected_params(ctx, arg.span));
                        return MirRvalue::Use(op);
                    }
                }
            }
            let class_name =
                typeck::resolve_instantiated_type_name(&ty.node).unwrap_or_else(|| {
                    match &ty.node {
                        Type::Named { path, .. } => path
                            .last()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "unknown".into()),
                        _ => "unknown".into(),
                    }
                });
            let class: Ident = class_name.clone().into();
            if let Some(fields) = obj_init {
                if ctx.registry.is_struct(&class) {
                    return MirRvalue::StructLit {
                        struct_name: class.to_string(),
                        fields: fields
                            .iter()
                            .map(|(n, v)| (n.to_string(), operand_from_expr(&v.node, ctx)))
                            .collect(),
                    };
                }
            }
            let ops: Vec<MirOperand> = args
                .iter()
                .map(|a| operand_from_expr(&a.node, ctx))
                .collect();
            let ctor_params = resolve_ctor_params(&class, args, &ops, ctx);
            MirRvalue::New {
                class: class_name,
                args: ops,
                ctor_params,
            }
        }
        Expr::CollectionExpr { elements } => lower_collection_expr_simple(elements, builder, ctx),
        Expr::StackSpanLit {
            elements,
            mutable,
            elem,
        } => MirRvalue::SpanFromStack {
            elements: elements
                .iter()
                .map(|e| operand_from_expr(&e.node, ctx))
                .collect(),
            elem_type: elem.clone(),
            mutable: *mutable,
        },
        Expr::Index { receiver, index } => {
            // Builtin `string` 索引：`s[i]` → get_Chars → rt_str_char_at。
            if lower_type::infer_type_from_spanned(receiver, ctx) == TypeId::String {
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let idx_op = operand_from_expr(&index.node, ctx);
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: "get_Chars".into(),
                    args: vec![idx_op],
                    receiver_type: "string".into(),
                    impl_class: Some("string".into()),
                    target_fn: Some("string::get_Chars".into()),
                    is_virtual: false,
                    params: vec!["int".into()],
                }
            } else {
                let recv_class = lower_type::class_from_expr(&receiver.node, ctx);
                // C# 索引器读：`obj[i]` → MethodCall get_Item，codegen 内联为 rt_*。
                if let Some(ix) = lower_type::resolve_indexer(&recv_class, &index.node, ctx) {
                    let recv_op = operand_from_expr(&receiver.node, ctx);
                    let idx_op = operand_from_expr(&index.node, ctx);
                    MirRvalue::MethodCall {
                        receiver: recv_op,
                        method: ix.get.into(),
                        args: vec![idx_op],
                        receiver_type: recv_class.clone(),
                        impl_class: Some(recv_class.clone()),
                        target_fn: Some(format!("{recv_class}::{}", ix.get)),
                        is_virtual: false,
                        params: vec![],
                    }
                } else {
                    MirRvalue::IndexGet {
                        array: operand_from_expr(&receiver.node, ctx),
                        index: operand_from_expr(&index.node, ctx),
                        elem_type: index_elem_type_non_indexer(receiver, ctx),
                    }
                }
            }
        }
        Expr::ExpressionLit(e) => {
            // Collect visible outer-scope variables as captures (name, local_id, ty).
            // These let codegen emit value-snapshot code for CaptureExpression nodes.
            let captures: Vec<(Ident, i32, SmolStr)> = ctx
                .scopes
                .iter()
                .flat_map(|s| s.iter())
                .map(|(name, id)| {
                    let ty = ctx
                        .locals
                        .get(id)
                        .map(|(_, ty)| type_id_name(ty))
                        .unwrap_or_else(|| "unknown".into());
                    (name.clone(), id.0 as i32, ty)
                })
                .collect();
            // 与 `Expression<T> = lambda` 赋值路径一致：根为 LambdaExpression。
            // 定位公理：树化失败须硬错误，禁止静默 Constant(true)。
            let mut tree = ExpressionTree::from_lambda(&e.lambda, &captures).unwrap_or_else(|| {
                panic!(
                    "MIR lower: ExpressionTree::from_lambda failed \
                     (silent Constant(true) fallback is forbidden)"
                )
            });
            // ExpressionLit 无声明类型时仅靠源码标注形参；字段类型仍尽量经 registry 解析。
            annotate_expression_tree(&mut tree, &TypeId::Infer, ctx);
            MirRvalue::ExpressionTreeConst {
                name: "rt_expr_tree_summary_0".into(),
                tree,
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            type_args,
            params_span,
        } => {
            // Facade builtin 优先（同 `lower_expr_to_rvalue` 路径）：
            // 防止 stub 类（File/Console/Task/Assert 等）的静态方法被
            // `user_type_static_method_func` 错误降级为 `Class::Method` 用户函数。
            // LINQ 终端（Any/Count/First）在 `lower_expr_to_rvalue_with_binary`
            // 中拦截（带 prep）；本 simple 路径不展开终端。
            if let Some(func) = lower_linq::builtin_static_method(&receiver.node, method) {
                let args: Vec<MirOperand> = args
                    .iter()
                    .map(|a| operand_from_expr(&a.node, ctx))
                    .collect();
                return MirRvalue::Call { func, args };
            }
            // RFC 005：数组 → Span / Span.Slice 降为专用 rvalue（codegen 胖指针）。
            {
                let recv_ty = lower_type::infer_type_from_spanned(receiver, ctx);
                match (method.as_str(), &recv_ty) {
                    ("AsSpan", TypeId::Array { .. }) => {
                        let array = operand_from_expr(&receiver.node, ctx);
                        let (start, length) = match args.len() {
                            0 => (None, None),
                            2 => (
                                Some(operand_from_expr(&args[0].node, ctx)),
                                Some(operand_from_expr(&args[1].node, ctx)),
                            ),
                            _ => (None, None),
                        };
                        return MirRvalue::SpanFromArray {
                            array,
                            start,
                            length,
                            mutable: true,
                        };
                    }
                    ("AsReadOnlySpan", TypeId::Array { .. }) => {
                        return MirRvalue::SpanFromArray {
                            array: operand_from_expr(&receiver.node, ctx),
                            start: None,
                            length: None,
                            mutable: false,
                        };
                    }
                    ("Slice", TypeId::Span { mutable, .. }) => {
                        // `Slice(start)` 单参 = 切片到末尾（length: None，codegen 计算 len-start）。
                        let length = if args.len() >= 2 {
                            Some(operand_from_expr(&args[1].node, ctx))
                        } else {
                            None
                        };
                        return MirRvalue::SpanSlice {
                            span: operand_from_expr(&receiver.node, ctx),
                            start: operand_from_expr(&args[0].node, ctx),
                            length,
                            mutable: *mutable,
                        };
                    }
                    ("AsReadOnly", TypeId::Span { mutable: true, .. }) => {
                        // 只读视图：复用同一胖指针（可变→只读仅为类型层）。
                        return MirRvalue::Use(operand_from_expr(&receiver.node, ctx));
                    }
                    ("CopyTo", TypeId::Span { elem, .. }) => {
                        return MirRvalue::SpanCopyTo {
                            src: operand_from_expr(&receiver.node, ctx),
                            dest: operand_from_expr(&args[0].node, ctx),
                            elem_type: *elem.clone(),
                        };
                    }
                    (
                        "Fill",
                        TypeId::Span {
                            elem,
                            mutable: true,
                            ..
                        },
                    ) => {
                        return MirRvalue::SpanFill {
                            span: operand_from_expr(&receiver.node, ctx),
                            value: operand_from_expr(&args[0].node, ctx),
                            elem_type: *elem.clone(),
                        };
                    }
                    (
                        "Clear",
                        TypeId::Span {
                            elem,
                            mutable: true,
                            ..
                        },
                    ) => {
                        return MirRvalue::SpanClear {
                            span: operand_from_expr(&receiver.node, ctx),
                            elem_type: *elem.clone(),
                        };
                    }
                    ("TryCopyTo", TypeId::Span { elem, .. }) => {
                        return MirRvalue::SpanTryCopyTo {
                            src: operand_from_expr(&receiver.node, ctx),
                            dest: operand_from_expr(&args[0].node, ctx),
                            elem_type: *elem.clone(),
                        };
                    }
                    ("ToArray", TypeId::Span { elem, .. }) => {
                        return MirRvalue::SpanToArray {
                            span: operand_from_expr(&receiver.node, ctx),
                            elem_type: *elem.clone(),
                        };
                    }
                    _ => {}
                }
            }
            // RFC 004 M2：用户类型静态方法调用（如 `Vector2.Add(a, b)`）——
            // 优先于实例方法路径，避免 receiver 被物化为 `ConstInt(0)` 充当 `this`。
            let stripped_type_args: Vec<ast::Type> =
                type_args.iter().map(|t| t.node.clone()).collect();
            if let Some((func, params)) = lower_call::user_type_static_method_sig(
                &receiver.node,
                method,
                &stripped_type_args,
                args,
                ctx,
            ) {
                // RFC 039：静态方法接口形参须包装 class 实参为接口胖指针。
                // 简单路径无法返回 prep，但 maybe_box_iface 仅包裹 operand，
                // 无需 prep 语句，可直接应用。
                let call_args: Vec<MirOperand> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let op = operand_from_expr(&a.node, ctx);
                        let arg_ty = type_name_from_operand(&op, &a.node, ctx);
                        if let Some(pt) = params.get(i) {
                            lower_call::maybe_box_iface(
                                op,
                                &arg_ty,
                                &TypeId::Named(pt.clone()),
                                ctx,
                            )
                        } else {
                            op
                        }
                    })
                    .collect();
                return MirRvalue::Call {
                    func,
                    args: call_args,
                };
            }
            let recv = operand_from_expr(&receiver.node, ctx);
            // C#：`d.Invoke(args)` ≡ `d(args)`。typeck 通常已改写；此处兜底。
            if method.as_str() == "Invoke" {
                let recv_ty_id = match &recv {
                    MirOperand::Local(id) => ctx.locals.get(id).map(|(_, ty)| ty.clone()),
                    _ => None,
                };
                if recv_ty_id
                    .as_ref()
                    .is_some_and(lower_type::is_delegate_type)
                {
                    let args: Vec<MirOperand> = args
                        .iter()
                        .map(|a| operand_from_expr(&a.node, ctx))
                        .collect();
                    return MirRvalue::IndirectCall { func: recv, args };
                }
            }
            let recv_ty = type_name_from_operand(&recv, &receiver.node, ctx);
            method_call_rvalue(
                builder,
                receiver,
                method,
                args,
                &stripped_type_args,
                params_span.as_ref(),
                ctx,
                recv,
                &recv_ty,
            )
        }
        Expr::Field { receiver, field } => {
            let recv_class = class_from_expr(&receiver.node, ctx);
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
                            let elem = lower_type::lower_type_name(&type_args[0].node);
                            return MirRvalue::SpanFromStack {
                                elements: vec![],
                                elem_type: elem,
                                mutable: name.as_str() == "Span",
                            };
                        }
                    }
                }
            }
            // RFC 004 M1：基元类型 static abstract 属性拦截。
            // 单态化后 `T.Zero` / `T.One` 已被 substitute_expr 替换为
            // `int.Zero` / `double.One` 等。lower 在此转为 Call { func: "int.Zero" }，
            // codegen try_emit_primitive_static 拦截发射常量（零运行时开销）。
            // （facade stub 静态属性不在此列举——`user_type_static_property_func`
            //   已统一还原源码形 `Class.Prop` 供 codegen 分派。）
            if let Expr::Ident(name) = &receiver.node {
                if matches!(field.as_str(), "Zero" | "One") && is_primitive_numeric_type(name) {
                    return MirRvalue::Call {
                        func: format!("{name}.{field}"),
                        args: vec![],
                    };
                }
            }
            // RFC 004 M2：用户类型静态属性访问（如 `Vector2.Zero`）。
            // 优先于实例属性路径，避免 receiver 被物化为 `ConstInt(0)` 充当 `this`。
            // 静态 getter 无 `this` 参数，降级为 `MirRvalue::Call`（无 receiver），
            // codegen `mangle_fn_name` 将 `Vector2::get_Zero` mangle 为 `@Vector2_get_Zero`。
            if let Some(func) =
                lower_call::user_type_static_property_func(&receiver.node, field, ctx)
            {
                return MirRvalue::Call { func, args: vec![] };
            }
            // Task facade (RFC 009 M1): Task<T> 的属性访问转为 MethodCall，
            // 以便 codegen 通过 expected 类型选择正确的 rt_task_result_* ABI。
            // TypeId::Task 是内建类型，不走 registry；若不在此拦截，会生成
            // MirOperand::Field，codegen 无法获取 inner 类型信息。
            // 泛型单态化后局部类型可能是 mangled 名 `Task_<T>`（Named 而非
            // TypeId::Task），receiver_type 须归一化为 "Task"，否则 codegen
            // classify_builtin_facade("Task_...") 匹配不到 → 裸字段读取运行时
            // RtTask 对象偏移 → inttoptr 悬垂解引用 AV
            // （web_mb_host_route_bind_e2e `task.Result` 实测）。
            if (recv_class == "Task" || recv_class.starts_with("Task_"))
                && task_facade_instance_property(field.as_str())
            {
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let getter = format!("get_{field}");
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: getter,
                    args: vec![],
                    receiver_type: "Task".into(),
                    impl_class: None,
                    target_fn: None,
                    is_virtual: false,
                    params: vec![],
                }
            } else if recv_class == "CancellationTokenSource"
                && cts_facade_instance_property(field.as_str())
            {
                // CTS facade (RFC 009 M4): CTS 属性访问转为 MethodCall。
                // stub 将 Token/IsCancellationRequested 注册为 auto-property（含 backing field），
                // 导致 is_custom_accessor_property 返回 false（field 在 fields map 中）。
                // 此处显式拦截，转为 get_Token/get_IsCancellationRequested，
                // codegen try_emit_cts_method 拦截发射 rt_cts_* ABI。
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let getter = format!("get_{field}");
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: getter,
                    args: vec![],
                    receiver_type: recv_class,
                    impl_class: None,
                    target_fn: None,
                    is_virtual: false,
                    params: vec![],
                }
            } else if is_custom_accessor_property(ctx.registry, &recv_class, field) {
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let getter = format!("get_{field}");
                let (impl_class, target_fn) = resolve_method_target(
                    ctx.registry,
                    &recv_class.clone().into(),
                    &getter.clone().into(),
                    ctx.owner.clone(),
                );
                let is_virtual = is_virtual_member(ctx.layouts, &recv_class, &getter, &[]);
                MirRvalue::MethodCall {
                    receiver: recv_op,
                    method: getter,
                    args: vec![],
                    receiver_type: recv_class,
                    impl_class,
                    target_fn,
                    is_virtual,
                    params: vec![],
                }
            } else {
                MirRvalue::Use(operand_from_expr(expr, ctx))
            }
        }
        Expr::Null => MirRvalue::Use(MirOperand::ConstNull),
        Expr::Coalesce { left, right } => {
            let l = operand_from_expr(&left.node, ctx);
            let r = operand_from_expr(&right.node, ctx);
            MirRvalue::Coalesce { left: l, right: r }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = operand_from_expr(&cond.node, ctx);
            let t = operand_from_expr(&then_branch.node, ctx);
            let e = operand_from_expr(&else_branch.node, ctx);
            MirRvalue::Ternary {
                cond: c,
                then_val: t,
                else_val: e,
            }
        }
        Expr::NullCond { access } => {
            // RFC 009 L2：`?.` 结果始终为 `T?`（typeck 已校验）。
            // - 引用类型 `T?`：null = null ptr（既有路径）
            // - 值类型 `T?`：null = null ptr；非 null = ptr to alloca'd T
            //   （codegen 在 emit_null_cond_field/method 装箱）
            // 因此 default 一律为 `ConstNull`，与 access_ty 解耦——
            // 旧实现按 access_ty 取 `ConstInt(0)` 等价值零值，与 `T?` 的 ptr
            // 表示错配，导致 codegen 生成 `phi i32 [0, ...], [loaded, ...]`
            // 后被 coerce_value 误转 ptr，丢失 null 语义。
            let _access_ty = infer_type_from_spanned(access, ctx);
            let default = MirOperand::ConstNull;
            match &access.node {
                Expr::Field { receiver, field } => {
                    let recv_op = operand_from_expr(&receiver.node, ctx);
                    let class = class_from_expr(&receiver.node, ctx);
                    MirRvalue::NullCondField {
                        receiver: recv_op,
                        class,
                        field: field.to_string(),
                        default,
                    }
                }
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    params_span,
                    ..
                } => {
                    let recv_op = operand_from_expr(&receiver.node, ctx);
                    let recv_ty = type_name_from_operand(&recv_op, &receiver.node, ctx);
                    let method_rv = method_call_rvalue(
                        builder,
                        receiver,
                        method,
                        args,
                        &[],
                        params_span.as_ref(),
                        ctx,
                        recv_op.clone(),
                        &recv_ty,
                    );
                    if let MirRvalue::MethodCall {
                        method,
                        args,
                        receiver_type,
                        impl_class,
                        target_fn,
                        is_virtual,
                        params,
                        ..
                    } = method_rv
                    {
                        MirRvalue::NullCondMethod {
                            receiver: recv_op,
                            method,
                            args,
                            receiver_type,
                            impl_class,
                            target_fn,
                            is_virtual,
                            default,
                            params,
                        }
                    } else {
                        method_rv
                    }
                }
                _ => MirRvalue::Use(operand_from_expr(expr, ctx)),
            }
        }
        Expr::ForceDeref { access } => match &access.node {
            Expr::Field { receiver, field } => {
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let class = class_from_expr(&receiver.node, ctx);
                MirRvalue::ForceDerefField {
                    receiver: recv_op,
                    class,
                    field: field.to_string(),
                    span: access.span,
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                params_span,
                ..
            } => {
                let recv_op = operand_from_expr(&receiver.node, ctx);
                let recv_ty = type_name_from_operand(&recv_op, &receiver.node, ctx);
                let method_rv = method_call_rvalue(
                    builder,
                    receiver,
                    method,
                    args,
                    &[],
                    params_span.as_ref(),
                    ctx,
                    recv_op.clone(),
                    &recv_ty,
                );
                if let MirRvalue::MethodCall {
                    method,
                    args,
                    receiver_type,
                    impl_class,
                    target_fn,
                    is_virtual,
                    params,
                    ..
                } = method_rv
                {
                    MirRvalue::ForceDerefMethod {
                        receiver: recv_op,
                        method,
                        args,
                        receiver_type,
                        impl_class,
                        target_fn,
                        is_virtual,
                        span: access.span,
                        params,
                    }
                } else {
                    method_rv
                }
            }
            _ => MirRvalue::Use(operand_from_expr(expr, ctx)),
        },
        // FFI Marshal 装箱（RFC 016 v2 M2 / RFC 016 M3）：
        // value_ty 提供源值类型的 size/align，由 codegen 发射 rt_box_create + memcpy。
        Expr::Box { expr, value_ty } => {
            let src = operand_from_expr(&expr.node, ctx);
            let src_ty = lower_type_name(&value_ty.node);
            MirRvalue::Box { src, src_ty }
        }
        // FFI Marshal 拆箱（RFC 016 v2 M2 / RFC 016 M3）：
        // value_ty 提供目标值类型的 expected_size/out_size，由 codegen 发射 rt_box_unbox。
        Expr::Unbox { expr, value_ty } => {
            let src = operand_from_expr(&expr.node, ctx);
            let target_ty = lower_type_name(&value_ty.node);
            MirRvalue::Unbox { src, target_ty }
        }
        // RFC 036 M1: `expr is pattern` — 类型测试，返回 bool。
        //
        // C# 9 逻辑组合（and/or/not）需物化子结果（prep 语句），本 simple 路径
        // 无法携带 prep（仅用于数组字面量元素等平凡上下文），故组合模式在此
        // 报错提示使用常规表达式上下文；单模式（叶子）直接产出 rvalue。
        Expr::Is { expr, pattern } => {
            let inner_op = operand_from_expr(&expr.node, ctx);
            let expr_ty = infer_type_from_spanned(expr, ctx);
            // CD-14/D3：与常规表达式路径一致，接口静态类型须 UnboxIface 取
            // 底层对象指针供 rt_obj_isa（否则把胖指针盒当类对象 → AV）。
            let isa_operand = match &expr_ty {
                TypeId::Named(n) if ctx.registry.is_interface(n) => MirOperand::UnboxIface {
                    object: Box::new(inner_op.clone()),
                    class: n.to_string(),
                },
                _ => inner_op.clone(),
            };
            let (prep, rv) =
                lower_is_pattern_rvalue(builder, &inner_op, &isa_operand, &expr_ty, pattern, ctx);
            if !prep.is_empty() {
                panic!(
                    "MIR lower: composite IsPattern (`is A and B` / `A or B` / `not A`) \
                     reached the trivial expression context (e.g. array literal element); \
                     it requires prep statements, use a general expression context"
                );
            }
            rv
        }
        _ => MirRvalue::Use(operand_from_expr(expr, ctx)),
    }
}

/// 判定「源为接口、目标为具体类」的显式转型，返回目标类名。
///
/// `(SqliteConnection)raw`（raw: IDbConnection）在 MIR 中是接口胖指针
/// `{ ptr obj, ptr itable }` 盒；调用方据此生成 `MirOperand::UnboxIface`，
/// codegen 发射 `load ptr, ptr 盒` 取出底层对象指针（而非把盒当类对象拷贝）。
/// 目标为接口 / object → class（RFC 037 M1 FFI 拆箱）等情况返回 None。
fn iface_to_class_cast_target(ty: &ast::Type, inner: &Expr, ctx: &LowerCtx) -> Option<String> {
    let target: String = typeck::resolve_instantiated_type_name(ty)
        .unwrap_or_else(|| type_id_name(&lower_type_name(ty)).to_string());
    let target_ident: Ident = target.as_str().into();
    if ctx.registry.is_interface(&target_ident) {
        return None;
    }
    match infer_type_from_expr(inner, ctx) {
        TypeId::Named(name) if ctx.registry.is_interface(&name) => Some(target),
        _ => None,
    }
}

pub(super) fn operand_from_expr(expr: &Expr, ctx: &LowerCtx) -> MirOperand {
    if let Some(op) = enum_variant_operand(expr, ctx.registry) {
        return op;
    }
    // RFC 004 M1：variant 构造不能折叠为 operand（需 alloca + tag/payload store）。
    // 安全网：若调用方误将 variant 构造传入此函数，返回 ConstNull 避免被
    // `Expr::Field` 路径误降为字段访问。若触发此分支，说明上游调用方未正确
    // 预处理 variant 构造（正解见 `lower_arg_operand` 的提前拦截
    // 与 `lower_expr_to_rvalue_simple` 的 `variant_construct_rvalue` 处理）。
    if variant_construct_rvalue(expr, ctx).is_some() {
        panic!(
            "MIR lower: variant_construct reached operand_from_expr; \
             caller must intercept in lower_arg_operand or lower_expr_to_rvalue_with_binary \
             (silent null is forbidden)"
        );
    }
    match expr {
        Expr::IntLit(n) => MirOperand::ConstInt(*n),
        Expr::FloatLit(ast::FloatLitValue::Double(f)) => MirOperand::ConstFloat(*f),
        Expr::FloatLit(ast::FloatLitValue::Float(f)) => MirOperand::ConstFloat(*f as f64),
        Expr::StringLit(s) => MirOperand::ConstString(s.clone()),
        Expr::BoolLit(b) => MirOperand::ConstBool(*b),
        Expr::CharLit(c) => MirOperand::ConstInt(*c as u32 as i64),
        Expr::Ident(name) => {
            if let Some(owner) = &ctx.owner {
                if let Some(op) = try_const_operand(owner, name, ctx) {
                    return op;
                }
            }
            // Locals / params shadow fields (C#；primary ctor `this.x = x` 右侧须绑参数)。
            if let Some(id) = ctx.lookup(name) {
                return MirOperand::Local(id);
            }
            if let Some(owner) = &ctx.owner {
                if ctx.is_static_field_of(owner, name) {
                    return MirOperand::StaticField {
                        class: owner.to_string(),
                        field: name.to_string(),
                    };
                }
            }
            if ctx.is_class_field(name) {
                if let Some(this) = ctx.lookup(&"this".into()) {
                    return MirOperand::Field {
                        object: Box::new(MirOperand::Local(this)),
                        class: ctx.owner.as_ref().unwrap().to_string(),
                        field: name.to_string(),
                    };
                }
            }
            panic!(
                "MIR lower: unresolved ident `{name}` in operand_from_expr \
                 (typeck should have failed; silent 0 is forbidden)"
            )
        }
        Expr::This => ctx
            .lookup(&"this".into())
            .map(MirOperand::Local)
            .unwrap_or_else(|| {
                panic!(
                    "MIR lower: `this` not in scope in operand_from_expr \
                 (silent 0 is forbidden)"
                )
            }),
        // CD-15/D4：`base` 与 `this` 指向同一对象（C# base 仅改变分派/静态类型，
        // 不产生独立实例）。接收者物化为 this 局部；非虚基类分派由
        // method_call_rvalue 按接收者静态类型 + is_virtual=false 实现。
        Expr::Base => ctx
            .lookup(&"this".into())
            .map(MirOperand::Local)
            .unwrap_or_else(|| {
                panic!(
                    "MIR lower: `base` not in scope in operand_from_expr \
                 (silent 0 is forbidden)"
                )
            }),
        Expr::RefArg { is_out: _, expr } => {
            let inner = operand_from_expr(&expr.node, ctx);
            if let MirOperand::Local(id) = inner {
                MirOperand::AddrOf(id)
            } else {
                inner
            }
        }
        Expr::Null => MirOperand::ConstNull,
        Expr::Default { ty } => {
            let type_id = lower_type_name(&ty.node);
            // RFC 040：`default(T)` 的 T 为泛型型参时（模板 lowering 阶段未知
            // 具体类型），保留类型名经单态化替换，codegen 按具体类型发射默认值。
            // 型参判定：基元类型已被 lower_type_name 映射为 TypeId::Int/Bool/...，
            // 故 `TypeId::Named(name)` 中 name 若非 registry 已注册类型即型参。
            // 否则落入 default_operand_for_type 的 `_ => ConstNull` 会丢失类型，
            // 导致 `default(bool)` 单态后发射 `ret ptr null` 与 i1 结果不匹配。
            if let TypeId::Named(name) = &type_id {
                if !ctx.registry.types.contains_key(name.as_str()) {
                    return MirOperand::ConstDefault {
                        type_name: name.to_string(),
                    };
                }
            }
            default_operand_for_type(&type_id)
        }
        Expr::TypeOf(ty) => {
            let type_name = typeck::resolve_instantiated_type_name(&ty.node)
                .unwrap_or_else(|| type_id_name(&lower_type_name(&ty.node)).to_string());
            MirOperand::TypeId { type_name }
        }
        Expr::Unary { op, expr: inner } => {
            let inner_op = operand_from_expr(&inner.node, ctx);
            match (op, &inner_op) {
                (UnaryOp::Neg, MirOperand::ConstInt(n)) => MirOperand::ConstInt(-n),
                (UnaryOp::Neg, MirOperand::ConstFloat(f)) => MirOperand::ConstFloat(-f),
                (UnaryOp::BitNot, MirOperand::ConstInt(n)) => MirOperand::ConstInt(!n),
                _ => inner_op,
            }
        }
        Expr::Field { receiver, field } => {
            let class = class_from_expr(&receiver.node, ctx);
            let class_ident: Ident = class.as_str().into();
            if let Some(op) = try_const_operand(&class_ident, field, ctx) {
                return op;
            }
            if ctx.is_static_field_of(&class_ident, field) {
                return MirOperand::StaticField {
                    class: class_ident.to_string(),
                    field: field.to_string(),
                };
            }
            MirOperand::Field {
                object: Box::new(operand_from_expr(&receiver.node, ctx)),
                class,
                field: field.to_string(),
            }
        }
        Expr::Cast { ty, expr: inner } => {
            let inner_op = operand_from_expr(&inner.node, ctx);
            if let Some(class) = iface_to_class_cast_target(&ty.node, &inner.node, ctx) {
                MirOperand::UnboxIface {
                    object: Box::new(inner_op),
                    class,
                }
            } else if lower_type_name(&ty.node) == TypeId::String
                && is_object_typed(&inner.node, ctx)
            {
                // RFC 045 P2：is string 收窄（Ident 窄化重写产生 Cast(object→string)）
                // 的叶子路径拆箱——ArcStringBox → char*（rt_string_unbox，vtable
                // 校验 + null 安全）。无 prep 通道，故用操作数级 UnboxString。
                MirOperand::UnboxString {
                    object: Box::new(inner_op),
                }
            } else if let TypeId::Named(name) = lower_type_name(&ty.node) {
                // 泛型型参 cast：`(T)boxed` 的 T 为型参时（模板 lowering 阶段未知
                // 具体类型），保留型参名经单态化替换，codegen 按具体类型发射拆箱。
                // 与 `Expr::Default` 的 `ConstDefault` 同机制（型参判定：基元类型
                // 已被 lower_type_name 映射为 TypeId::Int/Bool/...，故 Named 中 name
                // 若非 registry 已注册类型即型参）。否则落入 else 直接透传 object，
                // 值类型 T 单态化后发射 `ret ptr` 与结果类型不匹配。
                if !ctx.registry.types.contains_key(name.as_str()) {
                    MirOperand::UnboxGeneric {
                        object: Box::new(inner_op),
                        type_name: name.to_string(),
                    }
                } else {
                    inner_op
                }
            } else {
                inner_op
            }
        }
        Expr::New { ty, .. } => {
            panic!(
                "MIR lower: Expr::New `{}` reached operand_from_expr without prep \
                 (silent null is forbidden); use lower_expr_to_rvalue_with_binary / lower_arg_operand",
                lower_type_name(&ty.node)
            );
        }
        Expr::Binary { .. } => {
            panic!(
                "MIR lower: Expr::Binary reached operand_from_expr without prep \
                 (silent 0 is forbidden); use lower_expr_to_rvalue_with_binary / lower_arg_operand"
            );
        }
        other => {
            panic!(
                "MIR lower: unhandled expression {:?} in operand_from_expr \
                 (silent 0 is forbidden); materialize via lower_arg_operand",
                std::mem::discriminant(other)
            );
        }
    }
}

/// RFC 004 M4：将表达式块降级为 prep 语句 + tail 右值。
/// RFC 017 #16：`List<T>` 集合目标脱糖块含 `While` / `Assign`（数组中转 + 索引 Add）。
fn lower_block_expr(
    block: &Block,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirRvalue) {
    ctx.scopes.push(IndexMap::new());
    let mut prep = Vec::new();
    for stmt in &block.stmts {
        lower_block_expr_stmt(&stmt.node, builder, ctx, &mut prep);
    }
    let rv = if let Some(tail) = &block.tail {
        let (mut p, r) = lower_expr_to_rvalue_with_binary(&tail.node, builder, ctx);
        prep.append(&mut p);
        r
    } else {
        MirRvalue::Use(MirOperand::ConstInt(0))
    };
    ctx.scopes.pop();
    (prep, rv)
}

fn lower_block_expr_stmt(
    stmt: &Stmt,
    builder: &mut MirBuilder,
    ctx: &mut LowerCtx,
    prep: &mut Vec<MirStatement>,
) {
    match stmt {
        Stmt::Let { name, ty, init, .. } => {
            let local_ty = ty
                .as_ref()
                .map(|t| lower_type_name(&t.node))
                .or_else(|| init.as_ref().map(|i| infer_type_from_spanned(i, ctx)))
                .unwrap_or(TypeId::Int);
            let id = builder.fresh_local(name, local_ty, ctx.locals);
            ctx.bind(name, id);
            if let Some(init) = init {
                let (mut p, rv) = lower_expr_to_rvalue_with_binary(&init.node, builder, ctx);
                prep.append(&mut p);
                prep.push(MirStatement::Assign {
                    place: id,
                    rvalue: rv,
                });
            }
        }
        Stmt::Expr(e) => {
            let (mut p, rv) = lower_expr_to_rvalue_with_binary(&e.node, builder, ctx);
            prep.append(&mut p);
            let tmp = builder.fresh_local(&"_blk".into(), TypeId::Void, ctx.locals);
            prep.push(MirStatement::Assign {
                place: tmp,
                rvalue: rv,
            });
        }
        Stmt::Assign { target, value } => {
            let Expr::Ident(name) = &target.node else {
                panic!(
                    "MIR lower: expr Block Assign target must be Ident, got {:?}",
                    std::mem::discriminant(&target.node)
                );
            };
            let Some(place) = ctx.lookup(name) else {
                panic!("MIR lower: expr Block Assign unknown local `{name}`");
            };
            let (mut p, rv) = lower_expr_to_rvalue_with_binary(&value.node, builder, ctx);
            prep.append(&mut p);
            prep.push(MirStatement::Assign { place, rvalue: rv });
        }
        Stmt::While { cond, body } => {
            let mut body_stmts = Vec::new();
            ctx.enter_loop_body();
            for s in &body.stmts {
                lower_block_expr_stmt(&s.node, builder, ctx, &mut body_stmts);
            }
            ctx.exit_loop_body();
            builder.lower_while_with_cond(&cond.node, body_stmts, ctx, prep);
        }
        other => {
            panic!(
                "MIR lower: unsupported stmt in expr Block: {:?}",
                std::mem::discriminant(other)
            );
        }
    }
}

/// RFC 045 P2：表达式推断类型是否为 object（含 object?）。
/// 供窄化 Cast（object→string/数值）的 MIR 折叠判定——源必须是 object 槽
/// （值可能是 ArcStringBox / 基元 ArcBox），class 目标的 Cast 应保持透传。
fn is_object_typed(expr: &Expr, ctx: &LowerCtx) -> bool {
    match infer_type_from_expr(expr, ctx) {
        TypeId::Object => true,
        TypeId::Nullable { inner } => *inner == TypeId::Object,
        _ => false,
    }
}

/// Cast 目标类型是否为数值类型（int/float 族）。数值 Cast 必须在 MIR 物化为
/// 目标类型 temp（保留转换语义），否则 codegen 按内层操作数实际类型推断
/// binary 运算，跨类型数值表达式（(long)a * big）错在窄域溢出。
fn is_numeric_type_id(tid: &TypeId) -> bool {
    matches!(
        tid,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    )
}
