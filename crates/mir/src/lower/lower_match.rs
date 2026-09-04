use super::lower_expr::*;
use super::lower_type::*;
use super::*;

/// switch case 体 `break` 消解（[RFC 037 M-CE1 对齐 C# 语义]）。
///
/// switch 语句脱糖为 if-else 链后无循环帧，`to_cfg` 的 `loop_stack` 为空，
/// 残留的 `MirStatement::Break` 会触发 `break outside loop`。C# 中 case 内
/// `break` 跳出 switch：脱糖链中其等价于「终止当前分支」——分支落入链合并点，
/// break 本身及其后（不可达）语句可安全丢弃。
///
/// 递归穿透嵌套 `If` 分支：C# 允许 `case X: { if (c) { break; } ... }`
/// （case 体花括号块 + 嵌套 if-break），分支内 Break 同样表示跳出 switch，
/// 等价于截断所在 then/else 分支；分支截断点之后的兄弟语句对另一路径
/// 仍可达，必须保留。循环体（`While`/`LinqForeach`）**不穿透**：其中裸
/// `break` 语义是跳出循环（C#「最近封闭循环或 switch」规则），由 `to_cfg`
/// 循环分支的 `loop_stack` 正确解析。
fn truncate_at_switch_break(stmts: Vec<MirStatement>) -> Vec<MirStatement> {
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        if matches!(s, MirStatement::Break) {
            break;
        }
        match s {
            MirStatement::If {
                cond,
                then_body,
                else_body,
            } => {
                let then_body = truncate_at_switch_break(then_body);
                let else_body = truncate_at_switch_break(else_body);
                out.push(MirStatement::If {
                    cond,
                    then_body,
                    else_body,
                });
            }
            other => out.push(other),
        }
    }
    out
}

/// RFC 045 P3：scrut 是否为 object 槽（值可能是 ArcStringBox / 基元 ArcBox）。
fn is_object_boxed_scrut(ty: &TypeId) -> bool {
    match ty {
        TypeId::Object => true,
        TypeId::Nullable { inner } => **inner == TypeId::Object,
        // infer_scrut_ty 对非局部（字段/复合）返回 mangle 名形式。
        TypeId::Named(n) => n.as_str() == "object",
        _ => false,
    }
}

/// 绑定目标是否为需拆箱的值类型（基元；struct/enum 暂不在此拆箱）。
fn is_value_binding_ty(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
            | TypeId::Bool
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    )
}

fn lower_match_scrutinee(
    builder: &mut MirBuilder,
    expr: &Expr,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    // Field 快路径仅用于真字段：custom-accessor 属性（`get_X` 且无同名 backing field）
    // 必须走 rvalue → MethodCall，否则 `switch (obj.Prop)` 会把属性名当成 FieldGet，
    // codegen `field_info` 找不到字段时回退 `(16, int)`，对 variant 载荷 load i32 崩 IR。
    let field_is_plain = if let Expr::Field { receiver, field } = expr {
        let recv_class = class_from_expr(&receiver.node, ctx);
        !is_custom_accessor_property(ctx.registry, &recv_class, field)
    } else {
        false
    };
    if matches!(
        expr,
        Expr::IntLit(_) | Expr::BoolLit(_) | Expr::StringLit(_) | Expr::Ident(_)
    ) || field_is_plain
    {
        return (vec![], operand_from_expr(expr, ctx));
    }
    let ty = infer_type_from_expr(expr, ctx);
    let (mut prep, rvalue) = lower_expr_to_rvalue_with_binary(expr, builder, ctx);
    let tmp = builder.fresh_local(&"_scrut".into(), ty, ctx.locals);
    prep.push(MirStatement::Assign { place: tmp, rvalue });
    (prep, MirOperand::Local(tmp))
}

/// 计算 switch case 的匹配条件。
///
/// 返回 `(cond_prep, cond, body_prep)`：
/// - `cond_prep`：计算条件所需的预处理语句（如 tag 读取 / rt_obj_isa）
/// - `cond`：布尔条件操作数（true 时进入 then_body）
/// - `body_prep`：进入 then_body 前需执行的语句（payload / 类型绑定）
fn match_arm_cond(
    builder: &mut MirBuilder,
    pattern: &Pattern,
    scrut: &MirOperand,
    enum_name: Option<&Ident>,
    variant_name: Option<&Ident>,
    registry: &TypeRegistry,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand, Vec<MirStatement>) {
    match pattern {
        Pattern::Wildcard => (vec![], MirOperand::ConstBool(true), vec![]),
        Pattern::Var(name) => {
            let ty = infer_scrut_ty(scrut, ctx);
            let binding_local = builder.fresh_local(name, ty, ctx.locals);
            ctx.bind(name, binding_local);
            (
                vec![],
                MirOperand::ConstBool(true),
                vec![MirStatement::Assign {
                    place: binding_local,
                    rvalue: MirRvalue::Use(scrut.clone()),
                }],
            )
        }
        Pattern::Null => {
            let (prep, cond) = compare_scrut(builder, scrut, MirOperand::ConstNull, ctx);
            (prep, cond, vec![])
        }
        Pattern::Literal(lit) => {
            let rhs = match &lit.node {
                Expr::IntLit(n) => MirOperand::ConstInt(*n),
                Expr::BoolLit(b) => MirOperand::ConstBool(*b),
                Expr::CharLit(c) => MirOperand::ConstInt(*c as u32 as i64),
                Expr::StringLit(s) => MirOperand::ConstString(s.clone()),
                _ => MirOperand::ConstInt(0),
            };
            let (prep, cond) = compare_scrut(builder, scrut, rhs, ctx);
            (prep, cond, vec![])
        }
        Pattern::Type { ty, binding } => {
            let type_name = typeck::resolve_instantiated_type_name(&ty.node)
                .unwrap_or_else(|| type_id_name(&lower_type_name(&ty.node)).to_string());
            // RFC 018 §D1：值类型（int/bool/double/...）无 vtable，rt_obj_isa 会读取
            // obj+8 的 vtable 指针——对值类型 scrutinee 而言是垃圾内存，导致访问违规
            // (exit 0xc0000005)。值类型 static type 已由 typeck 确认与 pattern 类型
            // 兼容，运行时检查恒真，编译期折叠为 ConstBool(true)。
            let scrut_ty = infer_scrut_ty(scrut, ctx);
            let is_value_type = matches!(
                scrut_ty,
                TypeId::Int
                    | TypeId::Long
                    | TypeId::Short
                    | TypeId::Byte
                    | TypeId::Char
                    | TypeId::Float
                    | TypeId::Double
                    | TypeId::Bool
                    | TypeId::UInt
                    | TypeId::ULong
                    | TypeId::UShort
                    | TypeId::SByte
                    | TypeId::Vector { .. }
            );
            // RFC 045 P3：Nullable（int?/object?）不折叠——槽值可能是盒指针
            // （ArcBox/ArcStringBox），须走 rt_obj_isa 判定（折叠恒真会使
            // 首个 case 误匹配一切，`case int` 永不可达）。
            let (cond_prep, cond) = if is_value_type {
                (vec![], MirOperand::ConstBool(true))
            } else {
                // CD-14/D3：接口静态类型 scrutinee 是胖指针盒，rt_obj_isa 须
                // 底层对象指针（vtable slot0 取 typeinfo）——与 Expr::Is 路径
                // 的 isa_operand 一致，先 UnboxIface 取盒首槽。
                let isa_scrut = match &scrut_ty {
                    TypeId::Named(n) if registry.is_interface(n) => MirOperand::UnboxIface {
                        object: Box::new(scrut.clone()),
                        class: n.to_string(),
                    },
                    _ => scrut.clone(),
                };
                let isa_local = builder.fresh_local(&"_isa".into(), TypeId::Bool, ctx.locals);
                let prep = vec![MirStatement::Assign {
                    place: isa_local,
                    rvalue: MirRvalue::Call {
                        func: "rt_obj_isa".into(),
                        args: vec![
                            isa_scrut,
                            MirOperand::TypeInfoPtr {
                                type_name: type_name.clone(),
                            },
                        ],
                    },
                }];
                (prep, MirOperand::Local(isa_local))
            };
            let mut body_prep = vec![];
            if let Some(binding_name) = binding {
                let binding_ty = lower_type_name(&ty.node);
                // RFC 044 M2：绑定名是合成类提升字段（迭代器场景，HIR 已改写为
                // `__loc_*`）时写入字段——状态机各 case 块之间 scope 隔离，局部
                // 绑定在 yield 块不可见（实测读回字段旧值 → Length=0）；字段跨块
                // 存活。非字段绑定（普通 switch）保持局部。
                let binding_is_field = ctx.is_class_field(binding_name);
                let binding_local =
                    builder.fresh_local(binding_name, binding_ty.clone(), ctx.locals);
                if !binding_is_field {
                    ctx.bind(binding_name, binding_local);
                }
                // 接口绑定须 MakeIface / MakeIfaceDyn（基类静态类型走动态 itable）。
                let wrap = match &binding_ty {
                    TypeId::Named(iface_name) if ctx.registry.is_interface(iface_name) => {
                        iface_wrap_rvalue(ctx.registry, &scrut_ty, iface_name, scrut.clone())
                    }
                    _ => None,
                };
                let place_rvalue = if is_object_boxed_scrut(&scrut_ty)
                    && (binding_ty == TypeId::String || is_value_binding_ty(&binding_ty))
                {
                    // RFC 045 P3：object 槽的 string/值类型绑定须拆箱——`case
                    // string s:` 的 scrut 是 ArcStringBox / 基元 ArcBox，透传绑定
                    // 使 `s.Length` 读盒头（实测 Length=1）、`case int n:` 匹配
                    // 后 n 为盒指针（实测 0）。与 `(string)obj` 的 Cast→Unbox
                    // 同款：rt_string_unbox / rt_box_unbox。
                    MirRvalue::Unbox {
                        src: scrut.clone(),
                        target_ty: binding_ty,
                    }
                } else {
                    MirRvalue::Use(scrut.clone())
                };
                if binding_is_field {
                    // RFC 044 M2：合成类提升字段写入（跨状态机块存活）。
                    let this_op = ctx
                        .lookup(&"this".into())
                        .map(MirOperand::Local)
                        .unwrap_or(MirOperand::ConstNull);
                    let class_name = ctx.owner.clone().unwrap_or_default();
                    body_prep.push(MirStatement::FieldSet {
                        object: this_op,
                        class: class_name.to_string(),
                        field: binding_name.to_string(),
                        value: place_rvalue,
                    });
                } else if let Some(rv) = wrap {
                    body_prep.push(MirStatement::Assign {
                        place: binding_local,
                        rvalue: rv,
                    });
                } else {
                    body_prep.push(MirStatement::Assign {
                        place: binding_local,
                        rvalue: place_rvalue,
                    });
                }
            }
            (cond_prep, cond, body_prep)
        }
        Pattern::Ident(name) => {
            if let Some(en) = enum_name {
                if let Some(v) = registry.enum_variant(en, name) {
                    let (prep, cond) = compare_scrut(
                        builder,
                        scrut,
                        MirOperand::ConstInt(v.discriminant as i64),
                        ctx,
                    );
                    return (prep, cond, vec![]);
                }
            }
            // 无绑定类型模式：`case Dog:`（typeck 已确认为类型名）
            // RFC 018 §D1：值类型 scrutinee 跳过 rt_obj_isa（无 vtable，避免访问违规）
            let scrut_ty = infer_scrut_ty(scrut, ctx);
            let is_value_type = matches!(
                scrut_ty,
                TypeId::Int
                    | TypeId::Long
                    | TypeId::Short
                    | TypeId::Byte
                    | TypeId::Char
                    | TypeId::Float
                    | TypeId::Double
                    | TypeId::Bool
                    | TypeId::UInt
                    | TypeId::ULong
                    | TypeId::UShort
                    | TypeId::SByte
                    | TypeId::Vector { .. }
            );
            // RFC 045 P3：同上——Nullable 不折叠，isa 判定。
            if is_value_type {
                return (vec![], MirOperand::ConstBool(true), vec![]);
            }
            let isa_local = builder.fresh_local(&"_isa".into(), TypeId::Bool, ctx.locals);
            // CD-14/D3：接口静态类型 scrutinee 是胖指针盒，rt_obj_isa 须
            // 底层对象指针（UnboxIface 取盒首槽），否则 obj+8 读到 itable。
            let isa_scrut = match &scrut_ty {
                TypeId::Named(n) if registry.is_interface(n) => MirOperand::UnboxIface {
                    object: Box::new(scrut.clone()),
                    class: n.to_string(),
                },
                _ => scrut.clone(),
            };
            let cond_prep = vec![MirStatement::Assign {
                place: isa_local,
                rvalue: MirRvalue::Call {
                    func: "rt_obj_isa".into(),
                    args: vec![
                        isa_scrut,
                        MirOperand::TypeInfoPtr {
                            type_name: name.to_string(),
                        },
                    ],
                },
            }];
            (cond_prep, MirOperand::Local(isa_local), vec![])
        }
        Pattern::Variant {
            path,
            type_args: _,
            case,
            binding,
        } => {
            let vname = match (variant_name, path.last()) {
                (Some(n), _) => n.clone(),
                (None, Some(p)) => p.clone(),
                _ => {
                    return (vec![], MirOperand::ConstBool(false), vec![]);
                }
            };
            // enum 变体模式 `EnumType.VariantName`：解析器与 variant 共用 `Pattern::Variant`
            // AST，但 enum 不走 `variant_case` 路径——查 `enum_variant` 取 discriminant 比较。
            // typeck `check_match_pattern` 已对 enum 分流校验，此处为 MIR 镜像处理。
            if let Some(v) = registry.enum_variant(&vname, case) {
                let (prep, cond) = compare_scrut(
                    builder,
                    scrut,
                    MirOperand::ConstInt(v.discriminant as i64),
                    ctx,
                );
                // enum 变体模式不支持 payload binding（typeck 已校验）。
                return (prep, cond, vec![]);
            }
            let case_info = match registry.variant_case(&vname, case) {
                Some(info) => info,
                None => {
                    return (vec![], MirOperand::ConstBool(false), vec![]);
                }
            };
            let tag_local = builder.fresh_local(&"_vtag".into(), TypeId::Int, ctx.locals);
            let mut cond_prep = vec![MirStatement::Assign {
                place: tag_local,
                rvalue: MirRvalue::VariantTag {
                    scrutinee: scrut.clone(),
                    variant_name: vname.to_string(),
                },
            }];
            let (cmp_prep, cond) = compare_scrut(
                builder,
                &MirOperand::Local(tag_local),
                MirOperand::ConstInt(case_info.discriminant as i64),
                ctx,
            );
            cond_prep.extend(cmp_prep);
            let mut body_prep = vec![];
            if let (Some(binding_name), Some(_payload_ty)) = (binding, &case_info.payload) {
                // 基元类型 payload（double/int/bool 等）须映射为对应 TypeId 变体，
                // 不能统一包装为 TypeId::Named——否则 codegen 的 named_type 不认识
                // 基元名，回退为 ptr，导致 variant payload 提取类型不匹配
                // （如 double payload 被 load 为 ptr → ret ptr from double-returning fn）。
                let payload_ty = match case_info.payload.as_ref().unwrap().as_str() {
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
                    other => TypeId::Named(other.into()),
                };
                let binding_local =
                    builder.fresh_local(binding_name, payload_ty.clone(), ctx.locals);
                ctx.bind(binding_name, binding_local);
                body_prep.push(MirStatement::Assign {
                    place: binding_local,
                    rvalue: MirRvalue::VariantExtract {
                        scrutinee: scrut.clone(),
                        variant_name: vname.to_string(),
                        case_name: case.to_string(),
                        payload_ty,
                    },
                });
            }
            (cond_prep, cond, body_prep)
        }
        Pattern::Positional(_) => {
            panic!(
                "MIR lower: Pattern::Positional reached match_arm_cond; \
                 typeck must rewrite positional switch arms (RFC 004 M3)"
            )
        }
    }
}

fn infer_scrut_ty(scrut: &MirOperand, ctx: &LowerCtx) -> TypeId {
    match scrut {
        MirOperand::Local(id) => ctx
            .locals
            .get(id)
            .map(|(_, ty)| ty.clone())
            .unwrap_or(TypeId::Named("object".into())),
        _ => TypeId::Named("object".into()),
    }
}

fn compare_scrut(
    builder: &mut MirBuilder,
    left: &MirOperand,
    right: MirOperand,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    let tmp = builder.fresh_local(&"_match_cond".into(), TypeId::Bool, ctx.locals);
    (
        vec![MirStatement::Assign {
            place: tmp,
            rvalue: MirRvalue::Binary {
                op: BinOp::Eq,
                left: left.clone(),
                right,
            },
        }],
        MirOperand::Local(tmp),
    )
}

pub(super) fn lower_switch(
    builder: &mut MirBuilder,
    s: &SwitchExpr,
    ctx: &mut LowerCtx,
) -> Vec<MirStatement> {
    let (mut prep, scrut) = lower_match_scrutinee(builder, &s.scrutinee.node, ctx);
    let enum_name: Option<Ident> = if let Expr::Ident(name) = &s.scrutinee.node {
        ctx.lookup(name)
            .and_then(|id| ctx.locals.get(&id))
            .and_then(|(_, ty)| {
                if let TypeId::Named(n) = ty {
                    if ctx.registry.is_enum(n) {
                        return Some(n.clone());
                    }
                }
                None
            })
    } else {
        match infer_type_from_spanned(&s.scrutinee, ctx) {
            TypeId::Named(n) if ctx.registry.is_enum(&n) => Some(n),
            _ => None,
        }
    };
    let variant_name: Option<Ident> = if let Expr::Ident(name) = &s.scrutinee.node {
        ctx.lookup(name)
            .and_then(|id| ctx.locals.get(&id))
            .and_then(|(_, ty)| {
                if let TypeId::Named(n) = ty {
                    if ctx.registry.is_variant(n) {
                        return Some(n.clone());
                    }
                }
                None
            })
    } else {
        match infer_type_from_spanned(&s.scrutinee, ctx) {
            TypeId::Named(n) if ctx.registry.is_variant(&n) => Some(n),
            _ => None,
        }
    };

    let mut chain: Vec<MirStatement> = Vec::new();
    for case in s.cases.iter().rev() {
        if case.pattern.is_none() {
            chain = truncate_at_switch_break(builder.lower_block(&case.body, ctx));
            continue;
        }
        let pattern = case.pattern.as_ref().unwrap();
        let (cond_prep, pat_cond, body_prep) = match_arm_cond(
            builder,
            pattern,
            &scrut,
            enum_name.as_ref(),
            variant_name.as_ref(),
            ctx.registry,
            ctx,
        );
        let next = chain;
        // body_prep 先声明绑定 local，供 when / body 使用
        let mut then_body = body_prep;
        if let Some(when) = &case.when {
            let (when_prep, when_op) = lower_cond(builder, &when.node, ctx);
            then_body.extend(when_prep);
            then_body.push(MirStatement::If {
                cond: when_op,
                then_body: truncate_at_switch_break(builder.lower_block(&case.body, ctx)),
                else_body: next.clone(),
            });
        } else {
            then_body.extend(truncate_at_switch_break(
                builder.lower_block(&case.body, ctx),
            ));
        }
        chain = cond_prep;
        chain.push(MirStatement::If {
            cond: pat_cond,
            then_body,
            else_body: next,
        });
    }
    prep.extend(chain);
    prep
}

/// RFC 036 M4：switch 表达式 → 结果 local + if-else 链。
pub(super) fn lower_switch_form(
    builder: &mut MirBuilder,
    s: &SwitchExprForm,
    ctx: &mut LowerCtx,
) -> (Vec<MirStatement>, MirOperand) {
    let (mut prep, scrut) = lower_match_scrutinee(builder, &s.scrutinee.node, ctx);
    let scrut_ty = infer_type_from_spanned(&s.scrutinee, ctx);
    let enum_name: Option<Ident> = match scrut_ty.clone() {
        TypeId::Named(n) if ctx.registry.is_enum(&n) => Some(n),
        _ => None,
    };
    let variant_name: Option<Ident> = match scrut_ty {
        TypeId::Named(n) if ctx.registry.is_variant(&n) => Some(n),
        _ => None,
    };

    // Pass 0：为所有 arm 建立 pattern binding，使 `infer_type_from_expr`
    // 能解析 binding 变量。例如 `Content.Text(s) => s`，先建立 `s: string`
    // 的绑定，后续推断 arm body `s` 时就能得到正确的 `TypeId::String`。
    // 修复前：`s` 未绑定 → `infer_type_from_expr` fallback 到 `TypeId::Int`，
    // 导致 `_sw` result local 被 alloca 为 `i32`，赋值 string (ptr) 时触发
    // `ptrtoint ptr to i32` 截断 64 位指针，造成 0xc0000005 崩溃。
    //
    // Pass 0 创建 temp local 后**不恢复** next_local：若恢复会导致 Pass 1
    // 复用相同 LocalId 但不同 arm 顺序下类型错配（Pass 0 正向 → Pass 1 逆向），
    // 使 alloca 类型与赋值类型不一致，产生 corrupted IR。
    // Pass 0 的 temp local 在 codegen 阶段是 dead alloca（从未被读写），
    // 不产生运行时开销，仅略增栈帧大小。
    ctx.scopes.push(IndexMap::new());
    for arm in &s.arms {
        // RFC 004：位置模式脱糖为 Block{Let…; tail}——Pass 0 只预绑定 Let 类型，
        // 不跑 match_arm_cond（避免同函数多 switch 时堆积未初始化 pattern local → AV）。
        // 其它模式（variant payload 等）仍需 match_arm_cond 建立 binding 类型。
        if let Expr::Block(b) = &arm.body.node {
            for stmt in &b.stmts {
                if let Stmt::Let { name, ty, .. } = &stmt.node {
                    let local_ty = ty
                        .as_ref()
                        .map(|t| lower_type_name(&t.node))
                        .unwrap_or(TypeId::Int);
                    let id = builder.fresh_local(name, local_ty, ctx.locals);
                    ctx.bind(name, id);
                }
            }
        } else {
            let _ = match_arm_cond(
                builder,
                &arm.pattern,
                &scrut,
                enum_name.as_ref(),
                variant_name.as_ref(),
                ctx.registry,
                ctx,
            );
        }
    }
    // 此时 binding 变量已在 ctx 作用域中，可正确推断 arm body 类型。
    let result_ty = s
        .arms
        .first()
        .map(|a| infer_type_from_spanned(&a.body, ctx))
        .unwrap_or(TypeId::Int);
    let result = builder.fresh_local(&"_sw".into(), result_ty, ctx.locals);
    // 撤销 Pass 0 scope（绑定从作用域移除），但保留 locals 中的
    // temp 条目（避免与 Pass 1 的类型冲突，见上方注释）。
    ctx.scopes.pop();

    let mut chain: Vec<MirStatement> = Vec::new();
    for arm in s.arms.iter().rev() {
        // 每臂独立作用域，避免 binding / `__pos_scrut_*` 泄漏到后续臂或
        // 同函数内后续 switch 表达式（多 switch 连环 → 错 LocalId → AV）。
        ctx.scopes.push(IndexMap::new());
        // 先降级 pattern（match_arm_cond）以注册 variant binding，
        // 再降级 arm body——否则 arm body 引用 binding 时 ctx.lookup 失败。
        let (cond_prep, pat_cond, body_prep) = match_arm_cond(
            builder,
            &arm.pattern,
            &scrut,
            enum_name.as_ref(),
            variant_name.as_ref(),
            ctx.registry,
            ctx,
        );
        // body_prep 含 variant payload 提取（MirRvalue::VariantExtract + ctx.bind），
        // 必须在 arm body 降级前执行，使 binding local 进入 ctx 作用域。
        // binding 也必须在 `when` 守卫求值前执行——when 表达式引用 binding
        // （如 `case int x when x > 0`），若 binding 在 when_then 内则 when_op
        // 读取未初始化 local 导致 UB。
        let mut then_body = body_prep;
        let (body_prep_assign, body_rv) =
            lower_expr_to_rvalue_with_binary(&arm.body.node, builder, ctx);
        // body 赋值单独收集：有 when 时仅在 when 为真时执行；无 when 时直接追加
        let mut body_assign = body_prep_assign;
        body_assign.push(MirStatement::Assign {
            place: result,
            rvalue: body_rv,
        });
        let next = chain;
        if let Some(when) = &arm.when {
            let (when_prep, when_op) = lower_cond(builder, &when.node, ctx);
            // 顺序：binding（已在 then_body）→ when_prep → If(when_op, body_assign, next)
            then_body.extend(when_prep);
            then_body.push(MirStatement::If {
                cond: when_op,
                then_body: body_assign,
                else_body: next.clone(),
            });
        } else {
            then_body.extend(body_assign);
        }
        chain = cond_prep;
        chain.push(MirStatement::If {
            cond: pat_cond,
            then_body,
            else_body: next,
        });
        ctx.scopes.pop();
    }
    prep.extend(chain);
    (prep, MirOperand::Local(result))
}
