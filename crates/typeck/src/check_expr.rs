use ast::*;
use indexmap::IndexMap;
use std::cell::Cell;

use crate::checker::check_native::{box_to_object, type_id_to_ast_type};
use crate::checker::TypeChecker;
use crate::collection_expr_list::contains_collection_expr;
use crate::error::TypeError;
use crate::generics::{
    substitute_type_ast, substitute_type_name, substitution_map, type_id_to_field_name,
};
use crate::match_pat::MatchPat;
use crate::target_typed_new::contains_target_typed_new;
use crate::type_id::{LinqPath, TypeId};
use crate::typed::{TypedBlock, TypedExpr, TypedStmt};

// 统一高阈值背板：仅兜 `RecursionGuard` 遗漏导致的编译器自身栈溢出（0xC00000FD）。
//
// 挂在 `check_expr_inner` 入口——表达式检查递归树最深、且是
// `check_class_inner`/`check_fn_inner` 的公共汇聚点。阈值 4096 远高于任何
// 合法嵌套（原 `CHECK_EXPR_DEPTH` 阈值 40 的 100 倍），不误伤合法深表达式。
thread_local! {
    static TYPE_CHECK_RECURSION_DEPTH: Cell<u32> = const { Cell::new(0) };
}
// RAII：离开 `check_expr_inner` 时递减统一背板计数。
struct TypeCheckRecursionGuard;
impl Drop for TypeCheckRecursionGuard {
    fn drop(&mut self) {
        TYPE_CHECK_RECURSION_DEPTH.with(|d| d.set(d.get() - 1));
    }
}

/// Convert a registry field/property type name back to the canonical TypeId.
///
/// Registry stores field types as plain identifiers (e.g., `"int"`, `"string"`).
/// When resolving field access (`obj.Field`), the registry returns the type as an
/// `Ident`, and the type checker wraps it as `TypeId::Named("int")`. This is
/// problematic because `TypeId::Named("int")` ≠ `TypeId::Int`, causing primitive
/// method interception (e.g., `intValue.ToString()`) to miss. This function
/// maps primitive type names back to their proper `TypeId` variants.
pub(crate) fn resolve_named_type_id(name: Ident) -> TypeId {
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
        "object" => TypeId::Object,
        _ => TypeId::Named(name),
    }
}

impl TypeChecker {
    pub(crate) fn check_expr(&mut self, expr: &Expr) -> Result<TypedExpr, TypeError> {
        self.check_expr_inner(expr)
    }

    /// P0 双引擎收敛：带 span 的表达式检查入口。
    ///
    /// 由持有 `Spanned<Expr>` 的调用位置调用（`self.check_expr_at(e.span, &e.node)` →
    /// `self.check_expr_at(e.span, &e.node)`），检查成功后在出口向
    /// `expr_type_table` 记录 (span → ty)。MIR lower 优先查表，命中即采用
    /// typeck 结论，未命中回落旧推断（`infer_type_from_expr`），消除双引擎
    /// 对 builtin 知识的重复维护与静默漂移。
    pub(crate) fn check_expr_at(
        &mut self,
        span: ast::Span,
        expr: &Expr,
    ) -> Result<TypedExpr, TypeError> {
        let typed = self.check_expr_inner(expr)?;
        self.expr_type_table.record(span, typed.ty.clone());
        Ok(typed)
    }

    /// RFC 037 M-D0：观察者入口 `ObserveProperty("Name")` 的 typeck 识别与校验。
    ///
    /// 编译器在含 `[Observable]` auto-property 的类上合成实例方法
    /// `Signal<T> ObserveProperty(string symbol)`（§5.3）——该符号**无实体
    /// 方法表条目**，typeck 在此识别调用形态并给出返回类型 `Signal_<PropType>`；
    /// codegen 将调用展开为隐藏通道字段的静态定址直访（GEP 常量偏移 +
    /// 惰性 `new Signal<T>()`），无运行期字符串查找（§16 非目标 1）。
    ///
    /// 形态要求（违反即编译错误）：
    /// - 接收者类含 `[Observable]` auto-property（隐藏通知通道存在）；
    /// - 实参恰为 1 个**编译期字符串字面量**，命名接收者类上的某 `[Observable]`
    ///   auto-property（通道属性类型由命名属性编译期确定，不接受泛型实参）。
    ///
    /// 非本入口形态（如接收者类无观察通道 / 实参非常量字符串）在此被编译期
    /// 拒绝——与 codegen 的防御性降级（`try_emit_observable_observe` 返回
    /// `None`）对偶：typeck 面向用户源码，codegen 面向 MIR 层调用方。
    fn check_observable_observe_call(
        &mut self,
        expr: &Expr,
        args: &[Spanned<Expr>],
        type_args: &[Spanned<Type>],
        tname: &Ident,
    ) -> Result<TypedExpr, TypeError> {
        if !type_args.is_empty() {
            return Err(TypeError::Oop(
                "`ObserveProperty` 不接受泛型实参（通道属性类型由命名属性编译期确定）".into(),
            ));
        }
        if args.len() != 1 {
            return Err(TypeError::Mismatch {
                expected: "1 个字符串字面量实参（`ObserveProperty(\"PropName\")`）".into(),
                found: format!("{} 个实参", args.len()),
            });
        }
        let Expr::StringLit(prop_name) = &args[0].node else {
            return Err(TypeError::Oop(
                "`ObserveProperty` 实参须为编译期字符串字面量（命名 `[Observable]` 属性）".into(),
            ));
        };
        // 仅 `[Observable]` auto-property 有合成隐藏通道，可订阅；否则编译错误。
        let has_observable = match self.member_def_id(tname.as_str(), prop_name.as_str()) {
            Some(def_id) => self.attribute_table.has_attr(def_id, "Observable"),
            None => false,
        };
        if !has_observable {
            return Err(TypeError::Oop(format!(
                "`ObserveProperty(\"{prop_name}\")` 失败：`{tname}` 上无 `[Observable]` \
                 auto-property `{prop_name}`（仅编译器合成隐藏通道的属性可订阅）"
            )));
        }
        let prop_ident: Ident = Ident::from(prop_name.as_str());
        // 属性类型从 `declared_properties` 解析（覆盖 auto-property 与
        // custom-accessor）：属性名 → 类型名。`resolve_field` 仅命中 auto-property
        // 的同名 backing field，custom-accessor（如 `private int _x;` + 属性 `X`）
        // 无此字段会解析失败 → 误报 `Signal_unknown`。
        let prop_ty = self
            .registry
            .declared_properties
            .get(tname)
            .and_then(|props| props.iter().find(|p| p.name == prop_ident))
            .map(|p| p.ty.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let signal_ty = TypeId::Named(format!("Signal_{prop_ty}").into());
        Ok(TypedExpr {
            ty: signal_ty,
            expr: expr.clone(),
            linq_path: None,
            expression_tree: None,
        })
    }

    /// RFC 037 M-D0：通知侧入口 `NotifyPropertyChanged("Name")` 的 typeck 识别与校验。
    ///
    /// 编译器在含 `[Observable]` 属性（auto 或 custom-accessor）的类上合成实例
    /// 方法 `void NotifyPropertyChanged(string symbol)`（§5.3 场景 6）——该符号
    /// **无实体方法表条目**，typeck 在此识别调用形态并给出返回类型 `void`；
    /// codegen 将调用展开为隐藏通道的**显式 raise**（`Signal<T>.Set(当前值)`，
    /// 见 `try_emit_observable_notify`）。
    ///
    /// 形态要求（违反即编译错误）：
    /// - 接收者类含 `[Observable]` 属性（隐藏通知通道存在）；
    /// - 实参恰为 1 个**编译期字符串字面量**，命名接收者类上的某 `[Observable]`
    ///   属性（auto 或 custom-accessor，均分配隐藏通道槽）；
    /// - 属性须**可读**——显式 raise 需读取当前属性值（setter-only
    ///   custom-accessor 无读取路径 = 编译错误）；
    /// - 不接受泛型实参。
    ///
    /// 非本入口形态在此被编译期拒绝——与 codegen 的防御性降级
    /// （`try_emit_observable_notify` 返回 `None`）对偶。
    fn check_observable_notify_call(
        &mut self,
        expr: &Expr,
        args: &[Spanned<Expr>],
        type_args: &[Spanned<Type>],
        tname: &Ident,
    ) -> Result<TypedExpr, TypeError> {
        if !type_args.is_empty() {
            return Err(TypeError::Oop(
                "`NotifyPropertyChanged` 不接受泛型实参（通道属性类型由命名属性编译期确定）".into(),
            ));
        }
        if args.len() != 1 {
            return Err(TypeError::Mismatch {
                expected: "1 个字符串字面量实参（`NotifyPropertyChanged(\"PropName\")`）".into(),
                found: format!("{} 个实参", args.len()),
            });
        }
        let Expr::StringLit(prop_name) = &args[0].node else {
            return Err(TypeError::Oop(
                "`NotifyPropertyChanged` 实参须为编译期字符串字面量（命名 `[Observable]` 属性）"
                    .into(),
            ));
        };
        // 属性须存在且标注 `[Observable]`（接收者类含该属性的隐藏通知通道）。
        let has_observable = match self.member_def_id(tname.as_str(), prop_name.as_str()) {
            Some(def_id) => self.attribute_table.has_attr(def_id, "Observable"),
            None => false,
        };
        if !has_observable {
            return Err(TypeError::Oop(format!(
                "`NotifyPropertyChanged(\"{prop_name}\")` 失败：`{tname}` 上无 `[Observable]` \
                 属性 `{prop_name}`（仅编译器分配隐藏通道的属性可显式通知）"
            )));
        }
        // RFC 037 §5.3：属性须可读——显式 raise 读取当前值（auto-property 读
        // backing field、custom-accessor 调属性 getter）；setter-only 属性无
        // 当前值读取路径 = 编译错误。
        let readable = self
            .registry
            .declared_properties
            .get(tname)
            .and_then(|props| props.iter().find(|p| p.name.as_str() == prop_name.as_str()))
            .map(|p| p.can_read)
            .unwrap_or(true);
        if !readable {
            return Err(TypeError::Oop(format!(
                "`NotifyPropertyChanged(\"{prop_name}\")` 失败：`{tname}` 的 `{prop_name}` \
                 只写不可读，无当前值可发送"
            )));
        }
        Ok(TypedExpr {
            ty: TypeId::Void,
            expr: expr.clone(),
            linq_path: None,
            expression_tree: None,
        })
    }

    /// 将已检查（重写）的 `TypedBlock` 还原为 AST `Block`，供 MIR `lower_block`
    /// 重新下降。`Expr::If`/`Switch` 等控制流在 typeck 中把 then/else 分支存为
    /// AST `Block`；若直接沿用原始块，则块内 typeck 重写（如 `object→string`
    /// 拆箱 `Expr::Unbox`、装箱 `Expr::Box`）会在 MIR 重下降时丢失（2026-08-06
    /// `listview_single_object_items_source` 根因）。本函数用重写后的
    /// `TypedStmt` 重建 `Block.stmts`，并保留原始块尾（`else if` 链等未被
    /// `check_block` 重写的 tail，保持既有行为）。
    fn typed_block_to_block(&self, tb: &TypedBlock, tail: Option<Box<Spanned<Expr>>>) -> Block {
        // RFC 045 P3：优先使用 TypedBlock 自带的已重写 tail（check_block 保留）；
        // 参数仅作调用方未走 check_block 重建时的后备（历史行为）。
        let stmts = tb
            .stmts
            .iter()
            .map(|s| {
                let node = match s {
                    TypedStmt::Let { name, ty, init } => Stmt::Let {
                        mutable: false,
                        name: name.clone(),
                        ty: Some(Spanned::new(type_id_to_ast_type(ty), Span::DUMMY)),
                        init: init.clone(),
                    },
                    TypedStmt::Expr(e) => Stmt::Expr(e.clone()),
                    TypedStmt::Return(v) => Stmt::Return(v.clone()),
                    TypedStmt::While { cond, body } => Stmt::While {
                        cond: cond.clone(),
                        body: self.typed_block_to_block(body, None),
                    },
                    TypedStmt::For {
                        var, iter, body, ..
                    } => Stmt::For {
                        var: var.clone(),
                        iter: iter.clone(),
                        body: self.typed_block_to_block(body, None),
                    },
                    TypedStmt::ForC {
                        init,
                        cond,
                        inc,
                        body,
                    } => Stmt::ForC {
                        init: init
                            .as_ref()
                            .map(|s| Spanned::new(Box::new(s.node.clone()), s.span)),
                        cond: cond.clone(),
                        inc: inc
                            .as_ref()
                            .map(|s| Spanned::new(Box::new(s.node.clone()), s.span)),
                        body: self.typed_block_to_block(body, None),
                    },
                    TypedStmt::Assign { target, value } => Stmt::Assign {
                        target: target.clone(),
                        value: value.clone(),
                    },
                    TypedStmt::Break => Stmt::Break,
                    TypedStmt::Continue => Stmt::Continue,
                    TypedStmt::Throw { expr } => Stmt::Throw { expr: expr.clone() },
                    TypedStmt::TryCatch {
                        try_body,
                        catch_ty,
                        catch_name,
                        when_cond,
                        catch_body,
                        finally,
                    } => Stmt::TryCatch {
                        try_body: self.typed_block_to_block(try_body, None),
                        catch_ty: Spanned::new(type_id_to_ast_type(catch_ty), Span::DUMMY),
                        catch_name: catch_name.clone(),
                        when_cond: when_cond.clone(),
                        catch_body: self.typed_block_to_block(catch_body, None),
                        finally: finally.as_ref().map(|f| self.typed_block_to_block(f, None)),
                    },
                    TypedStmt::TryFinally { body, finally } => Stmt::TryFinally {
                        body: self.typed_block_to_block(body, None),
                        finally: self.typed_block_to_block(finally, None),
                    },
                    TypedStmt::Using {
                        name,
                        ty,
                        init,
                        body,
                    } => Stmt::Using {
                        name: name.clone(),
                        ty: Some(Spanned::new(type_id_to_ast_type(ty), Span::DUMMY)),
                        init: init.clone(),
                        body: self.typed_block_to_block(body, None),
                    },
                    TypedStmt::UsingVar { name, ty, init } => Stmt::UsingVar {
                        name: name.clone(),
                        ty: Some(Spanned::new(type_id_to_ast_type(ty), Span::DUMMY)),
                        init: init.clone(),
                    },
                    TypedStmt::AwaitUsing {
                        name,
                        ty,
                        init,
                        body,
                    } => Stmt::AwaitUsing {
                        name: name.clone(),
                        ty: Some(Spanned::new(type_id_to_ast_type(ty), Span::DUMMY)),
                        init: init.clone(),
                        body: self.typed_block_to_block(body, None),
                    },
                    TypedStmt::AwaitUsingVar { name, ty, init } => Stmt::AwaitUsingVar {
                        name: name.clone(),
                        ty: Some(Spanned::new(type_id_to_ast_type(ty), Span::DUMMY)),
                        init: init.clone(),
                    },
                };
                Spanned::new(node, Span::DUMMY)
            })
            .collect();
        Block {
            stmts,
            tail: tb.tail.clone().or_else(|| tail.clone()),
        }
    }

    pub(crate) fn check_expr_inner(&mut self, expr: &Expr) -> Result<TypedExpr, TypeError> {
        // 统一背板：仅兜 RecursionGuard 遗漏导致的编译器自身栈溢出，不误伤合法深嵌套。
        TYPE_CHECK_RECURSION_DEPTH.with(|d| {
            let n = d.get() + 1;
            if n > 4096 {
                panic!(
                    "TYPE_CHECK_RECURSION_DEPTH overflow: depth={}; class_path={}; iface_path={}",
                    n,
                    self.recursion_class.render_path(),
                    self.recursion_iface.render_path(),
                );
            }
            d.set(n);
        });
        let _guard = TypeCheckRecursionGuard;
        // RFC 016 v2 M2 / RFC 016 M3：当 Cast 源为 `object`、目标为值类型时，
        // typeck 将 Cast 转换为 `Expr::Unbox`（FFI Marshal 拆箱节点），以便
        // 后续 MIR lower 与 codegen 发射 `rt_box_unbox` ABI。
        let mut override_expr: Option<Expr> = None;
        let (ty, linq_path, expression_tree) = match expr {
            Expr::IntLit(_) => (TypeId::Int, None, None),
            Expr::FloatLit(ast::FloatLitValue::Float(_)) => (TypeId::Float, None, None),
            Expr::FloatLit(ast::FloatLitValue::Double(_)) => (TypeId::Double, None, None),
            Expr::BoolLit(_) => (TypeId::Bool, None, None),
            Expr::StringLit(_) => (TypeId::String, None, None),
            // RFC 012：comptime 有限子集——编译期常量求值。
            // 折叠内部表达式为编译期常量；不可折叠即报编译错误。折叠成功后
            // 把节点替换为折叠后的字面量表达式，递归重查（保证类型统一与后续
            // MIR lower 拿到字面量）。
            Expr::Comptime(inner) => {
                let value = crate::comptime::eval_comptime(&inner.node).ok_or_else(|| {
                    TypeError::Oop(
                        "`comptime` 表达式必须为可编译期折叠的常量（整型/bool/string 字面量运算；RFC 012）"
                            .into(),
                    )
                })?;
                let folded = crate::call_args::const_to_expr(&value);
                let checked = self.check_expr(&folded)?;
                override_expr = Some(checked.expr);
                (checked.ty, None, None)
            }
            Expr::InterpolatedString { parts } => {
                let desugared = self.desugar_interpolated_string(parts)?;
                return self.check_expr_inner(&desugared);
            }
            Expr::CharLit(_) => (TypeId::Char, None, None),
            Expr::Ident(name) => {
                // 构造器初始化器 `: base(args)` 注入的 `__ctor::Base` 调用：
                // typeck 不 resolve 该符号（基类 ctor 已在基类 check 时注册到
                // typed_fns，但不进入 fn_defs / scopes）。识别 `__ctor::` 前缀
                // 直接返回 Void，由 mir lowering / codegen 处理实际调用。
                if name.as_str().starts_with("__ctor::") {
                    (TypeId::Void, None, None)
                } else {
                    // RFC 006 M2：静态方法内禁止访问实例字段。
                    // `check_class_inner` 在类作用域（scopes[1]）中放入了所有字段
                    // （含实例字段），`resolve_value_name` 会命中实例字段——
                    // 此处先于解析拦截。若 name 被方法作用域（scopes.last()）内
                    // 的局部变量/参数遮蔽，则不视为字段访问，跳过检查。
                    if self.current_fn_is_static {
                        if let Some(class_name) = &self.current_class {
                            let in_method_scope = self
                                .scopes
                                .last()
                                .map(|s| s.contains_key(name))
                                .unwrap_or(false);
                            if !in_method_scope && self.is_instance_field_of(class_name, name) {
                                return Err(TypeError::Oop(format!(
                                    "static method cannot access instance field `{name}`"
                                )));
                            }
                        }
                    }
                    // Bare instance field shorthand: `_field` → `this._field`.
                    // 仅当 name 未被方法作用域内的局部变量/参数遮蔽时重写
                    // （C# 名称查找：局部 > 字段；primary ctor `this.x = x` 的
                    // 右侧 `x` 是形参，不得重写为 `this.x`）。
                    let in_method_scope = self
                        .scopes
                        .last()
                        .map(|s| s.contains_key(name))
                        .unwrap_or(false);
                    if !in_method_scope {
                        // RFC 045（ContentPresenter 构造崩溃根因）：裸标识若同时是
                        // 类字段与**类型名**（`public Content Content;`——字段 Content
                        // 与 variant 类型 Content 同名），`Content.None` 的 receiver
                        // 须解析为类型名（variant case 构造）；旧实现先做裸字段重写
                        // → `(this.Content).None` 错降为字段读取 → 运行时崩溃。
                        // C# 名称查找：类型名优先于实例字段（`Content.None` 无歧义）。
                        if !self
                            .registry
                            .lookup_type(name, &self.enclosing_namespace)
                            .is_some()
                        {
                            if let Some(field_expr) = self.rewrite_bare_instance_field(name) {
                                return self.check_expr_inner(&field_expr);
                            }
                        }
                    }
                    let ty = self.resolve_value_name(name).or_else(|| {
                        // Fallback: static class/type name (e.g., `Parallel.ForAsync(...)`).
                        // `resolve_value_name` only searches scoped variables; class names
                        // must be resolved from the type registry.
                        // CD-30：沿调用点 namespace 链消歧——`Arc.Drawing` 内
                        // `ImageNative` 命中本包类型（shadowed_types FQN），
                        // 而非被入口包遮蔽后的全局 stub 类。
                        if self
                            .registry
                            .lookup_type(name, &self.enclosing_namespace)
                            .is_some()
                        {
                            Some(TypeId::Named(name.clone()))
                        } else {
                            None
                        }
                    });
                    let ty = match ty {
                        Some(t) => t,
                        None => {
                            // 自定义属性访问器无后备字段，不在 scopes；裸 `Value` 脱糖为
                            // `this.Value`（与 C# 实例成员查找一致），供表达式体方法等使用。
                            if let Some(class_name) = self.current_class.clone() {
                                if !self.current_fn_is_static {
                                    let getter: Ident = format!("get_{name}").into();
                                    if self
                                        .registry
                                        .resolve_method(&class_name, &getter, &self.access_ctx())
                                        .is_ok()
                                    {
                                        let field_expr = Expr::Field {
                                            receiver: Box::new(Spanned::new(
                                                Expr::This,
                                                Span::DUMMY,
                                            )),
                                            field: name.clone(),
                                        };
                                        return self.check_expr_inner(&field_expr);
                                    }
                                }
                            }
                            return Err(TypeError::Undefined(name.to_string()));
                        }
                    };
                    let ty = match ty {
                        TypeId::Ref { inner, .. } => *inner,
                        other => other,
                    };
                    // RFC 045 P2：is 模式收窄（narrowed 类型 ≠ 声明类型）时把 Ident
                    // 重写为 Cast 节点——MIR 据此按窄化类型下降：class 目标值透传
                    // （ArcBox 即对象本身），string/值类型目标经 MIR Cast 折叠兜底
                    // unbox（rt_string_unbox / rt_box_unbox）。C# `if (o is string) {
                    // o.Length }` 语义由此成立；不重写则 MIR 按 locals 原始 object
                    // 类型下降，成员访问发射 @object_Member 未定义符号或直读 ArcBox。
                    let declared_ty = self.scopes.iter().rev().find_map(|s| s.get(name).cloned());
                    if let Some(declared) = declared_ty {
                        let declared = match declared {
                            TypeId::Ref { inner, .. } => *inner,
                            other => other,
                        };
                        if declared != ty {
                            override_expr = Some(Expr::Cast {
                                expr: Box::new(Spanned::new(expr.clone(), Span::DUMMY)),
                                ty: Spanned::new(
                                    crate::checker::check_native::type_id_to_ast_type(&ty),
                                    Span::DUMMY,
                                ),
                            });
                        }
                    }
                    (ty, None, None)
                }
            }
            Expr::Path(path) => {
                let name = path
                    .last()
                    .ok_or_else(|| TypeError::Undefined("empty path".into()))?;
                let ty = self
                    .resolve_value_name(name)
                    .ok_or_else(|| TypeError::Undefined(name.to_string()))?;
                let ty = match ty {
                    TypeId::Ref { inner, .. } => *inner,
                    other => other,
                };
                (ty, None, None)
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.check_expr_at(left.span, &left.node)?;
                let right_ty = self.check_short_circuit_right(*op, right, &left_ty)?;
                // RFC 006 M2：record 值相等 —— 重写为 null 安全 Equals 后递归检查
                if let Some(desugared) =
                    self.desugar_record_equality(*op, left, right, &left_ty.ty, &right_ty.ty)
                {
                    return self.check_expr_inner(&desugared);
                }
                // RFC 003：用户运算符重载 → Type.op_*(…)
                if let Some(desugared) =
                    self.desugar_user_binary_operator(*op, left, right, &left_ty.ty, &right_ty.ty)
                {
                    return self.check_expr_inner(&desugared);
                }
                let ty = match op {
                    BinOp::Eq | BinOp::NotEq => {
                        let left_canon = self.canonical_type(&left_ty.ty);
                        let right_canon = self.canonical_type(&right_ty.ty);
                        if left_canon == TypeId::String || right_canon == TypeId::String {
                            // Allow comparison with `null` (Nullable(Infer)) — any
                            // reference type (including string) is comparable with null.
                            let other = if left_canon == TypeId::String {
                                &right_canon
                            } else {
                                &left_canon
                            };
                            let is_null = matches!(other, TypeId::Nullable { inner } if matches!(**inner, TypeId::Infer));
                            let is_ref = self.is_reference_type(other);
                            if !is_null && !is_ref {
                                return Err(TypeError::Mismatch {
                                    expected: format!("string (comparing at {:?})", left.span),
                                    found: if left_canon == TypeId::String {
                                        right_ty.ty.display()
                                    } else {
                                        left_ty.ty.display()
                                    },
                                });
                            }
                        }
                        TypeId::Bool
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::And | BinOp::Or => {
                        TypeId::Bool
                    }
                    BinOp::Add
                        if self.canonical_type(&left_ty.ty) == TypeId::String
                            || self.canonical_type(&right_ty.ty) == TypeId::String =>
                    {
                        // String concatenation: `string + X` or `X + string` →
                        // always string. The non-string operand can be any type;
                        // codegen emits `rt_str_concat` / `rt_int_to_str` etc.
                        TypeId::String
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        let left_canon = self.canonical_type(&left_ty.ty);
                        let right_canon = self.canonical_type(&right_ty.ty);
                        // 枚举位运算（RFC 004 枚举能力增强）：同枚举 `E | E` → E；
                        // 移位 `E << n` → E（右操作数为 int）。枚举底层为 int32，
                        // codegen 直接对 i32 发射 and/or/xor/shl。
                        if self.is_enum_type(&left_canon) {
                            let is_shift = matches!(op, BinOp::Shl | BinOp::Shr);
                            let right_ok = if is_shift {
                                right_canon == TypeId::Int
                            } else {
                                self.is_enum_type(&right_canon) && left_canon == right_canon
                            };
                            if !right_ok {
                                return Err(TypeError::Mismatch {
                                    expected: if is_shift {
                                        "int shift count".into()
                                    } else {
                                        format!(
                                            "same enum type `{}` for {:?}",
                                            left_ty.ty.display(),
                                            op
                                        )
                                    },
                                    found: right_ty.ty.display(),
                                });
                            }
                            left_canon
                        } else if self.is_enum_type(&right_canon) {
                            return Err(TypeError::Mismatch {
                                expected: format!("numeric or same enum operands for {:?}", op),
                                found: format!(
                                    "{} {:?} {}",
                                    left_ty.ty.display(),
                                    op,
                                    right_ty.ty.display()
                                ),
                            });
                        } else if !is_arithmetic_numeric(&left_canon)
                            || !is_arithmetic_numeric(&right_canon)
                        {
                            return Err(TypeError::Mismatch {
                                expected: format!("numeric operands or user operator for {:?}", op),
                                found: format!(
                                    "{} {:?} {}",
                                    left_ty.ty.display(),
                                    op,
                                    right_ty.ty.display()
                                ),
                            });
                        } else {
                            numeric_promote(&left_canon, &right_canon)
                        }
                    }
                    _ => {
                        let left_canon = self.canonical_type(&left_ty.ty);
                        let right_canon = self.canonical_type(&right_ty.ty);
                        if !is_arithmetic_numeric(&left_canon)
                            || !is_arithmetic_numeric(&right_canon)
                        {
                            return Err(TypeError::Mismatch {
                                expected: format!("numeric operands or user operator for {:?}", op),
                                found: format!(
                                    "{} {:?} {}",
                                    left_ty.ty.display(),
                                    op,
                                    right_ty.ty.display()
                                ),
                            });
                        }
                        numeric_promote(&left_canon, &right_canon)
                    }
                };
                // 泛型方法缺陷 B：子表达式若被改写（如泛型方法调用推断出 type_args
                // 后 override_expr 注入显式实参 `Identity<int>(42)`），必须用改写后的
                // left_ty.expr / right_ty.expr 重建 Binary 节点，否则 MIR 收到未标注
                // type_args 的模板调用 → 非单态化符号 + 错误指针（嵌套
                // `h.Identity(42) + ...` 0xC0000005）。子表达式未改写时 expr 即原始
                // 节点，重建是无损等价，可无条件重组。
                let left_expr = left_ty.expr;
                let right_expr = right_ty.expr;
                if left_expr != left.node || right_expr != right.node {
                    override_expr = Some(Expr::Binary {
                        op: *op,
                        left: Box::new(Spanned::new(left_expr, left.span)),
                        right: Box::new(Spanned::new(right_expr, right.span)),
                    });
                }
                (ty, None, None)
            }
            Expr::Call {
                func,
                args,
                type_args,
                params_span,
            } => {
                if let Expr::Lambda(l) = &func.node {
                    if crate::call_args::lambda_has_defaults(&l.params) {
                        return self.check_lambda_iife_call(l, args, type_args);
                    }
                }
                // RFC 007：自由函数可选/命名实参绑定（无显式 type_args 时）。
                if type_args.is_empty() {
                    if let Expr::Ident(name) = &func.node {
                        if let Some(te) = self.try_bind_free_fn_call(name, args)? {
                            return Ok(te);
                        }
                    }
                }
                if let Expr::Ident(name) = &func.node {
                    if !type_args.is_empty() {
                        let targs: Vec<TypeId> = type_args
                            .iter()
                            .map(|t| self.lower_type(&t.node))
                            .collect::<Result<_, _>>()?;
                        // Task<T> 是内建泛型类型，不是 fn_templates 条目。
                        if name.as_str() == "Task" && targs.len() == 1 {
                            return Ok(TypedExpr {
                                ty: TypeId::Task {
                                    inner: Box::new(targs[0].clone()),
                                },
                                expr: expr.clone(),
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        // 泛型类模板出现在表达式位置（如 `EqualityComparer<string>.Default` /
                        // `Holder<Thing>.Cache`，receiver 为 `Call { Ident, [], type_args }`）。
                        // 此前无条件走 `instantiate_generic_fn`，把泛型类型引用当泛型函数
                        // 解析，`fn_templates` 未命中 → `undefined name <Class>`（ORM 热路径
                        // 「泛型字段 mono」缺口）。此处检测到泛型类模板时按泛型类实例化，
                        // 返回其单态化命名类型，静态字段/方法解析随后在 Named 类型上进行。
                        if self.class_templates.contains_key(name) {
                            let ty = self.instantiate_generic_class(name, &targs)?;
                            return Ok(TypedExpr {
                                ty,
                                expr: expr.clone(),
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        self.instantiate_generic_fn(name, &targs)?;
                        let template = self.fn_templates.get(name).cloned().unwrap();
                        let map = substitution_map(&template.generics, &targs);
                        // RFC 037 M1: 修复泛型方法调用类型替换——先 substitute AST 再 lower。
                        // 旧实现先 `lower_type` 把 `DependencyProperty<T>` mangle 为
                        // `Named("DependencyProperty_T")`，再 `substitute_type` 尝试替换
                        // mangled 名中的 T——但 mangled 名是单一标识符，不匹配 map 键 "T"，
                        // 导致 `RegisterProperty<double>(...)` 返回类型仍为
                        // `DependencyProperty_T` 而非 `DependencyProperty_double`。
                        // 改为先 `substitute_type_ast` 在 AST 层面替换 `T` → `double`，
                        // 再 `lower_type` 生成正确的 mangled `Named`。
                        let mut params = Vec::new();
                        for p in &template.params {
                            let p_ast = substitute_type_ast(&p.ty.node, &map);
                            let pty = self.lower_type(&p_ast)?;
                            params.push(pty);
                        }
                        let ret = match template.ret.as_ref() {
                            Some(t) => {
                                let ret_ast = substitute_type_ast(&t.node, &map);
                                self.lower_type(&ret_ast)?
                            }
                            None if template.is_async => TypeId::Task {
                                inner: Box::new(TypeId::Void),
                            },
                            None => TypeId::Void,
                        };
                        if args.len() != params.len() {
                            return Err(TypeError::Mismatch {
                                expected: format!("{} arguments", params.len()),
                                found: format!("{} arguments", args.len()),
                            });
                        }
                        // RFC 006：泛型自由函数实参目标类型 `new()`。
                        // RFC 017：实参 `[…]` → `List<T>` 目标脱糖。
                        let mut prepared_args = Vec::with_capacity(args.len());
                        for (a, expected) in args.iter().zip(params.iter()) {
                            let prepared = self.prepare_target_expr(&a.node, expected, a.span)?;
                            prepared_args.push(Spanned::new(prepared, a.span));
                        }
                        let mut arg_tys = Vec::new();
                        for (a, expected) in prepared_args.iter().zip(params.iter()) {
                            // RFC 017 残余补全：集合表达式实参对 `T[]` 形参——同
                            // Func 路径的目标化短路（防元素类型独立推断）。
                            if let (Expr::CollectionExpr { .. }, TypeId::Array { .. }) =
                                (&a.node, expected)
                            {
                                if self.try_bind_collection_array_target(&a.node, expected)? {
                                    arg_tys.push(expected.clone());
                                    continue;
                                }
                            }
                            arg_tys.push(self.check_expr_at(a.span, &a.node)?.ty);
                        }
                        for (found, expected) in arg_tys.iter().zip(params.iter()) {
                            if !self.types_compatible(expected, found) {
                                return Err(TypeError::Mismatch {
                                    expected: expected.display(),
                                    found: found.display(),
                                });
                            }
                        }
                        return Ok(TypedExpr {
                            ty: ret,
                            expr: Expr::Call {
                                func: func.clone(),
                                args: prepared_args,
                                type_args: type_args.clone(),
                                params_span: params_span.clone(),
                            },
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 004 M1：variant 有 payload case 构造（`Value.Int(42)`）。
                // 语法：`Type.Case(payload)` → Expr::Call { func: Expr::Field { receiver: Expr::Ident(Type), field: Case }, args: [payload] }
                if let Expr::Field { receiver, field } = &func.node {
                    if let Expr::Ident(variant_name) = &receiver.node {
                        if self.registry.is_variant(variant_name) {
                            let case_info = self
                                .registry
                                .variant_case(variant_name, field)
                                .ok_or_else(|| {
                                    TypeError::Oop(format!(
                                        "variant `{}` has no case `{}`",
                                        variant_name, field
                                    ))
                                })?;
                            let payload_ty = case_info.payload.as_ref().ok_or_else(|| TypeError::Oop(format!(
                                "variant case `{}.{}` has no payload; use `{}.{}` without arguments",
                                variant_name, field, variant_name, field
                            )))?;
                            let expected_ty = TypeId::Named(payload_ty.clone());
                            if args.len() != 1 {
                                return Err(TypeError::Mismatch {
                                    expected: "1 payload argument".into(),
                                    found: format!("{} arguments", args.len()),
                                });
                            }
                            let arg_ty = self.check_expr_at(args[0].span, &args[0].node)?.ty;
                            if !self.types_compatible(&expected_ty, &arg_ty) {
                                return Err(TypeError::Mismatch {
                                    expected: expected_ty.display(),
                                    found: arg_ty.display(),
                                });
                            }
                            return Ok(TypedExpr {
                                // CD-30：构造返回类型与模式/注解侧同一 FQN 规范化（见
                                // check_builtin Pattern::Variant `pattern_variant_ty`）。
                                // `Content c = Content.Text("Click")` 中 RHS 返回短名
                                // `Content`、注解 `Content` 为 FQN，直接短名会让赋值失配
                                // （batch 跨 case 同名 variant 实测）。
                                ty: self
                                    .resolve_type_path(std::slice::from_ref(variant_name))
                                    .unwrap_or_else(|| TypeId::Named(variant_name.clone())),
                                expr: expr.clone(),
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                    }
                }
                // RFC 007: same-class static method call. When a bare method name
                // is called and no free function exists, resolve it as a static
                // method on the enclosing class (C# allows omitting the class name
                // for same-class static calls).
                if let Expr::Ident(name) = &func.node {
                    if let Some(ref class_name) = self.current_class {
                        // Collect candidate static methods; match by argument count.
                        // Extract ret and param types before the mutable self borrow below.
                        let candidate: Option<(TypeId, Vec<Ident>)> = self
                            .registry
                            .types
                            .get(class_name)
                            .and_then(|nom| nom.methods.get(name))
                            .and_then(|sigs| {
                                sigs.iter()
                                    .filter(|s| {
                                        s.modifier == MethodModifier::Static
                                            && s.params.len() == args.len()
                                    })
                                    .map(|s| {
                                        let param_tys: Vec<Ident> =
                                            s.params.iter().map(|p| p.ty.clone()).collect();
                                        (demangle_type_part(&s.ret), param_tys)
                                    })
                                    .next()
                            });
                        if let Some((ret, param_tys)) = candidate {
                            // RFC 006 M2：同 class 静态方法实参目标上下文。
                            // RFC 017：实参 `[…]` → `List<T>` 目标脱糖。
                            let mut prepared_args = Vec::with_capacity(args.len());
                            for (a, pty) in args.iter().zip(param_tys.iter()) {
                                let expected = self.param_sig_type_id(pty);
                                let prepared =
                                    self.prepare_target_expr(&a.node, &expected, a.span)?;
                                prepared_args.push(Spanned::new(prepared, a.span));
                            }
                            let mut arg_tys = Vec::new();
                            for a in &prepared_args {
                                arg_tys.push(self.check_expr_at(a.span, &a.node)?.ty);
                            }
                            // Check argument type compatibility.
                            for (arg_ty, param_ty) in arg_tys.iter().zip(param_tys.iter()) {
                                let expected = TypeId::Named(param_ty.clone());
                                if !self.types_compatible(&expected, arg_ty) {
                                    return Err(TypeError::Mismatch {
                                        expected: expected.display(),
                                        found: arg_ty.display(),
                                    });
                                }
                            }
                            return Ok(TypedExpr {
                                ty: ret,
                                expr: Expr::Call {
                                    func: func.clone(),
                                    args: prepared_args,
                                    type_args: type_args.clone(),
                                    params_span: params_span.clone(),
                                },
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                    }
                }
                // RFC 007+：裸实例方法调用（`_bump()` → `this._bump()`）。C# 允许
                // 在实例方法内省略 `this.` 直接调用同 class（含基类）实例方法。
                // 与 `rewrite_bare_instance_field` 对齐：static 方法内不重写、
                // 局部变量/参数遮蔽时不重写、无匹配实例方法时不重写。
                if let Expr::Ident(name) = &func.node {
                    if !self.current_fn_is_static {
                        if let Some(ref class_name) = self.current_class {
                            let shadowed = self
                                .scopes
                                .last()
                                .map(|s| s.contains_key(name))
                                .unwrap_or(false);
                            if !shadowed && self.has_instance_method(class_name, name, args.len()) {
                                let call_expr = Expr::MethodCall {
                                    receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                                    method: name.clone(),
                                    args: args.to_vec(),
                                    type_args: type_args.clone(),
                                    params_span: None,
                                };
                                return self.check_expr_inner(&call_expr);
                            }
                        }
                    }
                }
                // 先解析 callee 类型（裸实例字段 `_cb` 在此重写为 `this._cb`），
                // 再按 Func 形参填目标类型 `new()` 并保留重写。
                let func_ty = self.check_expr_at(func.span, &func.node)?;
                let resolve_func_id = |ty_id: &TypeId| -> Option<TypeId> {
                    // 可空委托（`Func<...>? _f` / `Action<...>? _a`）调用：解包
                    // Nullable 后再解析签名，否则 `_authenticator(ctx)` 落入无
                    // Func 签名路径被误判返回 void（WebApplication 鉴权现场）。
                    let unwrapped = match ty_id {
                        TypeId::Nullable { inner } => inner.as_ref(),
                        other => other,
                    };
                    match unwrapped {
                        TypeId::Func { params, ret } => Some(TypeId::Func {
                            params: params.clone(),
                            ret: ret.clone(),
                        }),
                        TypeId::Named(n) => demangle_func_type_with(n.as_str(), args.len(), &|s| {
                            self.registry.types.contains_key(s)
                        }),
                        _ => None,
                    }
                };
                let func_ty_id = if let Expr::Ident(name) = &func.node {
                    let scoped_ty = self.scopes.iter().rev().find_map(|s| s.get(name)).cloned();
                    match scoped_ty {
                        Some(TypeId::Nullable { inner }) => resolve_func_id(&inner),
                        Some(TypeId::Func { params, ret }) => Some(TypeId::Func { params, ret }),
                        Some(TypeId::Named(n)) => {
                            demangle_func_type_with(n.as_str(), args.len(), &|s| {
                                self.registry.types.contains_key(s)
                            })
                        }
                        // 裸实例字段委托（`_cb(x)` → `this._cb(x)`）：作用域无此符号，
                        // 回退到字段访问解析类型（Func mangled 名 → demangle），
                        // 修复 `_authenticator(ctx)` 被误判返回 void 的问题。
                        None => resolve_func_id(&func_ty.ty),
                        _ => None,
                    }
                } else {
                    resolve_func_id(&func_ty.ty)
                };
                if let Some(TypeId::Func { params, ret }) = func_ty_id {
                    if args.len() != params.len() {
                        return Err(TypeError::Mismatch {
                            expected: format!("{} arguments", params.len()),
                            found: format!("{} arguments", args.len()),
                        });
                    }
                    let mut prepared_args = Vec::with_capacity(args.len());
                    for (a, expected) in args.iter().zip(params.iter()) {
                        // RFC 004 M1：嵌套 Func 实参上的方法组。
                        let after_mg = self.maybe_coerce_method_group(&a.node, expected)?;
                        // RFC 017：实参 `[…]` → `List<T>` 目标脱糖。
                        let prepared = self.prepare_target_expr(&after_mg, expected, a.span)?;
                        prepared_args.push(Spanned::new(prepared, a.span));
                    }
                    let mut arg_tys = Vec::new();
                    let mut rewritten = Vec::with_capacity(prepared_args.len());
                    for a in &prepared_args {
                        let te = self.check_expr_at(a.span, &a.node)?;
                        arg_tys.push(te.ty);
                        rewritten.push(Spanned::new(te.expr, a.span));
                    }
                    for (found, expected) in arg_tys.iter().zip(params.iter()) {
                        if !self.types_compatible(expected, found) {
                            return Err(TypeError::Mismatch {
                                expected: expected.display(),
                                found: found.display(),
                            });
                        }
                    }
                    return Ok(TypedExpr {
                        ty: *ret,
                        expr: Expr::Call {
                            func: func.clone(),
                            args: rewritten,
                            type_args: type_args.clone(),
                            params_span: params_span.clone(),
                        },
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                // 无 Func 形参信息：仍禁止 Infer `new` 落入 MIR（check 时硬错误）。
                let mut rewritten_args = Vec::with_capacity(args.len());
                let mut args_rewritten = false;
                for a in args {
                    let te = self.check_expr_at(a.span, &a.node)?;
                    if te.expr != a.node {
                        args_rewritten = true;
                    }
                    rewritten_args.push(Spanned::new(te.expr, a.span));
                }
                if args_rewritten {
                    override_expr = Some(Expr::Call {
                        func: func.clone(),
                        args: rewritten_args,
                        type_args: type_args.clone(),
                        params_span: params_span.clone(),
                    });
                }
                (TypeId::Void, None, None)
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                type_args,
                params_span,
            } => {
                // Parser 将 `Type.Case(payload)` 解析为 MethodCall（而非 Call+Field），
                // 因此需在此分支检测：receiver 是 variant 类型名 → 走 variant 构造路径。
                // RFC 004 M2：支持泛型 variant `Option<int>.Some(42)`，
                // receiver 为 `Call { func: Ident, args: [], type_args }`。
                if let Some(vn) = match &receiver.node {
                    Expr::Ident(name) if self.registry.is_variant(name) => Some(name.to_string()),
                    Expr::Call {
                        func,
                        args: ref ca,
                        type_args,
                        ..
                    } if ca.is_empty() && !type_args.is_empty() => {
                        if let Expr::Ident(name) = &func.node {
                            if self.registry.is_variant(name)
                                && self.registry.is_generic_template(name)
                            {
                                let arg_tys: Vec<TypeId> = type_args
                                    .iter()
                                    .filter_map(|t| self.lower_type(&t.node).ok())
                                    .collect();
                                let mangled =
                                    crate::generics::mangle_generic(name.as_str(), &arg_tys);
                                if !self.registry.is_variant(&mangled.as_str().into()) {
                                    // 首次引用时按需实例化泛型 variant
                                    if let Some(tmpl) = self.registry.types.get(name).cloned() {
                                        let map: IndexMap<Ident, TypeId> = tmpl
                                            .generic_params
                                            .iter()
                                            .zip(arg_tys.iter())
                                            .map(|(p, t)| (p.clone(), t.clone()))
                                            .collect();
                                        let inst_cases: Vec<_> = tmpl
                                            .variants
                                            .iter()
                                            .map(|c| {
                                                let mut copy = c.clone();
                                                if let Some(ref p) = c.payload {
                                                    copy.payload =
                                                        Some(substitute_type_name(p, &map));
                                                }
                                                copy
                                            })
                                            .collect();
                                        let inst = crate::oop_types::NominalType {
                                            name: mangled.as_str().into(),
                                            kind: crate::oop_types::TypeKind::Variant,
                                            variants: inst_cases,
                                            ..tmpl.clone()
                                        };
                                        self.registry.types.insert(mangled.as_str().into(), inst);
                                    }
                                }
                                if self.registry.is_variant(&mangled.as_str().into()) {
                                    Some(mangled)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                } {
                    let vn_ident: Ident = vn.as_str().into();
                    let case_info =
                        self.registry
                            .variant_case(&vn_ident, method)
                            .ok_or_else(|| {
                                TypeError::Oop(format!("variant `{}` has no case `{}`", vn, method))
                            })?;
                    let payload_ty = case_info.payload.as_ref().ok_or_else(|| {
                        TypeError::Oop(format!(
                            "variant case `{}.{}` has no payload; use `{}.{}` without arguments",
                            vn, method, vn, method
                        ))
                    })?;
                    let expected_ty = TypeId::Named(payload_ty.clone());
                    if args.len() != 1 {
                        return Err(TypeError::Mismatch {
                            expected: "1 payload argument".into(),
                            found: format!("{} arguments", args.len()),
                        });
                    }
                    let arg_ty = self.check_expr_at(args[0].span, &args[0].node)?.ty;
                    if !self.types_compatible(&expected_ty, &arg_ty) {
                        return Err(TypeError::Mismatch {
                            expected: expected_ty.display(),
                            found: arg_ty.display(),
                        });
                    }
                    return Ok(TypedExpr {
                        // CD-30：构造返回类型与注解/模式侧同一 FQN 规范化（见
                        // check_expr Expr::Call 变体构造分支注释）。parser 把
                        // `Content.Text("Click")` 落为 MethodCall，此处返回短名
                        // `Content` 会让 `Content c = Content.Text(..)` 的 init 校验
                        // `types_compatible(declared=FQN, found=short)` 失配。
                        ty: self
                            .resolve_type_path(std::slice::from_ref(&vn_ident))
                            .unwrap_or_else(|| TypeId::Named(vn_ident.clone())),
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                // RFC 016 v2 M2 / RFC 016 M3：FFI `object` 形参的自动装箱在
                // `check_native_method` 内完成（就地修改 args 副本）。调用返回后
                // 用（可能被包装的）args 重建 MethodCall 表达式作为 TypedExpr.expr，
                // 保证后续 MIR lower 能看到 `Expr::Box` 节点。
                let mut args_vec: Vec<Spanned<Expr>> = args.to_vec();
                // RFC 004 M1：优先尝试 static abstract 接口调用（`T.Method()`）。
                // 若 receiver 是当前作用域的泛型参数且有 `where T : IFace<T>` 约束
                // 含 `static abstract Method`，走 static abstract 单态化分派路径。
                if let Some(ty) =
                    self.check_static_abstract_call(&receiver.node, method, &mut args_vec)?
                {
                    return Ok(TypedExpr {
                        ty,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                if let Some(ty) = self.check_builtin_static_method(
                    &receiver.node,
                    method,
                    &mut args_vec,
                    type_args,
                )? {
                    // CD-16/D5：builtin/native 静态方法调用点必须携带 **typed 实参**。
                    // 嵌套 params 调用（`Console.WriteLine("A:" + c.Sum(1,2,3))` /
                    // `Array.Clear(a, 0, c.Sum(1,2))`）的 `ParamsSpanInfo` 标注只存在于
                    // typeck 绑定 params 槽时产生的 typed 节点——parser 产出的 raw AST
                    // 恒为 `None`。复用 `expr.clone()` 会把嵌套 params 调用降级为定参
                    // 调用（MIR 按 `[obj, 1, 2]` 传实参，callee 签名 `(ptr, span)`）
                    // → ABI 错位 → 0xC0000005。此处以「已检查实参」重建调用表达式
                    // （check_expr_inner 幂等，二次检查安全），保留所有嵌套标注。
                    // builtin/native 静态方法自身无 `params`，故重建时标注为 None。
                    let mut checked_args = Vec::with_capacity(args_vec.len());
                    for a in &args_vec {
                        let te = self.check_expr_at(a.span, &a.node)?;
                        checked_args.push(Spanned::new(te.expr, a.span));
                    }
                    let final_expr = Expr::MethodCall {
                        receiver: receiver.clone(),
                        method: method.clone(),
                        args: checked_args,
                        type_args: type_args.clone(),
                        params_span: None,
                    };
                    return Ok(TypedExpr {
                        ty,
                        expr: final_expr,
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                // RFC 045 P1：receiver 检查可能重写 (string)obj → Expr::Unbox；
                // 以重建的 MethodCall 幂等重检（与 Field 分支同款），否则 MIR 收到
                // 原始 Cast 折叠丢失拆箱——((string)box).Substring(...) 把 ArcBox
                // 当 string 直传（实测乱码）。params_span 由重检的绑定路径重新标注。
                if recv.expr != receiver.node {
                    let rebuilt = Expr::MethodCall {
                        receiver: Box::new(Spanned::new(recv.expr, receiver.span)),
                        method: method.clone(),
                        args: args.clone(),
                        type_args: type_args.clone(),
                        params_span: None,
                    };
                    return self.check_expr_inner(&rebuilt);
                }
                let recv = if recv.ty.is_nullable() {
                    if let Expr::Ident(name) = &receiver.node {
                        if !self.null_flow.as_ref().is_some_and(|f| f.is_non_null(name)) {
                            return Err(TypeError::NullableMemberAccess {
                                var: name.to_string(),
                                member: method.to_string(),
                            });
                        }
                    } else if self.null_guard_depth == 0 {
                        return Err(TypeError::Oop(format!(
                            "cannot call method `{method}` on nullable expression; use `?.` or `!.`"
                        )));
                    }
                    let inner = recv
                        .ty
                        .nullable_inner()
                        .cloned()
                        .unwrap_or_else(|| recv.ty.clone());
                    TypedExpr {
                        ty: inner,
                        expr: recv.expr,
                        linq_path: None,
                        expression_tree: None,
                    }
                } else {
                    recv
                };
                // RFC 004 修复刀 2：泛型参数实例接口方法分派。
                // `where T : IFace<T>` 约束下对泛型局部（`TypeId::Generic`）的实例
                // 接口方法调用（如 `t.ReadJson(reader)` / `t.Create()`）。此前静默
                // 落 Void，MIR 烘死 receiver "unknown" → codegen panic。此处分派
                // 到约束接口的实例方法，返回类型保留泛型参数（单态化替换）。
                if matches!(recv.ty, TypeId::Generic(_)) {
                    if let Some(iface_ret) =
                        self.check_generic_constraint_method_call(&recv.ty, method, args.len())?
                    {
                        return Ok(TypedExpr {
                            ty: iface_ret,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // C# 惯用：`delegate.Invoke(args)` ≡ `delegate(args)`（RFC 008 间接调用）。
                // Func/Action 无 registry 实例方法；若不改写，MethodCall 落 Void 且 MIR
                // `type_id_to_name(Func)` → "unknown" → codegen `@unknown_Invoke` panic。
                // 改写为 Call 后复用上方 Func 形参校验 + MIR IndirectCall 路径。
                if method.as_str() == "Invoke" {
                    let is_delegate = matches!(&recv.ty, TypeId::Func { .. })
                        || matches!(
                            &recv.ty,
                            TypeId::Named(n)
                                if n.starts_with("Func_") || n.starts_with("Action_")
                        );
                    if is_delegate {
                        let call_expr = Expr::Call {
                            func: receiver.clone(),
                            args: args.to_vec(),
                            type_args: vec![],
                            params_span: None,
                        };
                        return self.check_expr_inner(&call_expr);
                    }
                }
                // C#：`obj._delegate(args)` ≡ `_delegate.Invoke(args)` when `_delegate`
                // is a Func/Action field. Parser desugars `receiver.field(...)` to
                // MethodCall { method: field } — rewrite to Call for IndirectCall.
                if let Some(tname) = self.type_name_of(&recv.ty) {
                    if let Ok(field_ty_name) =
                        self.registry
                            .resolve_field(&tname, method, &self.access_ctx())
                    {
                        let field_ty = resolve_named_type_id(field_ty_name.clone());
                        let is_delegate = matches!(&field_ty, TypeId::Func { .. })
                            || matches!(
                                &field_ty,
                                TypeId::Named(n)
                                    if n.starts_with("Func_") || n.starts_with("Action_")
                            )
                            || self
                                .registry
                                .delegate_aliases
                                .contains_key(field_ty_name.as_str());
                        if is_delegate {
                            let call_expr = Expr::Call {
                                func: Box::new(Spanned::new(
                                    Expr::Field {
                                        receiver: receiver.clone(),
                                        field: method.clone(),
                                    },
                                    receiver.span,
                                )),
                                args: args.to_vec(),
                                type_args: vec![],
                                params_span: None,
                            };
                            return self.check_expr_inner(&call_expr);
                        }
                    }
                }
                // Builtin `string` instance methods (P2): Split/Replace/Substring/
                // Contains/IndexOf/StartsWith/EndsWith/Trim/ToUpper/ToLower.
                if recv.ty == TypeId::String {
                    if let Some(ty) = self.check_builtin_string_method(method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 005：`T[]`.AsSpan / AsReadOnlySpan。
                if matches!(recv.ty, TypeId::Array { .. }) {
                    if let Some(ty) =
                        self.check_builtin_array_span_method(&recv.ty, method, args)?
                    {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 005 M2：`List<T>`.AsSpan / AsReadOnlySpan。
                if matches!(&recv.ty, TypeId::Named(n) if n.starts_with("List_")) {
                    if let Some(ty) = self.check_builtin_list_span_method(&recv.ty, method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 005：`Span`/`ReadOnlySpan` 实例方法。
                if matches!(recv.ty, TypeId::Span { .. }) {
                    if let Some(ty) = self.check_builtin_span_method(&recv.ty, method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // Task facade (RFC 009 M1): Task<T>/Task 实例方法拦截。
                // Wait/Cancel/GetResult 同步路径；GetAwaiter 留 M2。
                if matches!(recv.ty, TypeId::Task { .. }) {
                    if let Some(ty) = self.check_builtin_task_method(&recv.ty, method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // CancellationTokenSource facade (RFC 009 M4): CTS 实例方法拦截。
                // Cancel/CancelAfter/Token/IsCancellationRequested；codegen 发射 rt_cts_* ABI。
                if matches!(&recv.ty, TypeId::Named(n) if n.as_str() == "CancellationTokenSource") {
                    if let Some(ty) = self.check_builtin_cts_method(method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // CancellationToken facade (RFC 009 M4): CT 实例方法拦截。
                // ThrowIfCancellationRequested/Register/IsCancellationRequested。
                if matches!(&recv.ty, TypeId::Named(n) if n.as_str() == "CancellationToken") {
                    if let Some(ty) = self.check_builtin_ct_method(method, args)? {
                        return Ok(TypedExpr {
                            ty,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // Primitive instance methods (int/long/float/double/bool/char/string).
                // ToString() is a virtual method on all primitives; typeck
                // intercepts it here because primitives are not registered in
                // the TypeRegistry type table (they are TypeId enum variants),
                // and the registry-based method resolution path would fail with
                // "undefined type `int`". `string` 同属内置 TypeId 变体（不注册
                // type table）：`s.ToString()`/`s.Equals(...)`/`s.CompareTo(...)`
                // 走同一拦截，避免落入 registry 报 `undefined type `string``。
                if matches!(
                    recv.ty,
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
                        | TypeId::SByte
                        | TypeId::String
                ) {
                    if method.as_str() == "ToString" {
                        // RFC 027 M5 / RFC 007 M2a：数值族支持
                        //   ToString(format) / ToString(format, provider) 文化感知格式化。
                        // string/bool/char 无格式重载，仅 0 参。
                        let numeric = matches!(
                            recv.ty,
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
                        );
                        match args.len() {
                            0 => {}
                            2 if numeric => {
                                self.require_string_arg(&args[..], 0)?;
                                self.require_format_provider_arg(&args[..], 1)?;
                            }
                            n => {
                                return Err(TypeError::Mismatch {
                                    expected: format!(
                                        "{} for primitive ToString",
                                        if numeric {
                                            "0, 2 or 3 arguments"
                                        } else {
                                            "0 arguments"
                                        }
                                    ),
                                    found: format!("{n} argument(s)"),
                                })
                            }
                        }
                        return Ok(TypedExpr {
                            ty: TypeId::String,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    if method.as_str() == "GetHashCode" {
                        if !args.is_empty() {
                            return Err(TypeError::Mismatch {
                                expected: "0 arguments for primitive GetHashCode".into(),
                                found: format!("{} argument(s)", args.len()),
                            });
                        }
                        return Ok(TypedExpr {
                            ty: TypeId::Int,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    if method.as_str() == "Equals" {
                        if args.len() != 1 {
                            return Err(TypeError::Mismatch {
                                expected: "1 argument for primitive Equals".into(),
                                found: format!("{} argument(s)", args.len()),
                            });
                        }
                        return Ok(TypedExpr {
                            ty: TypeId::Bool,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    if method.as_str() == "CompareTo" {
                        if args.len() != 1 {
                            return Err(TypeError::Mismatch {
                                expected: "1 argument for primitive CompareTo".into(),
                                found: format!("{} argument(s)", args.len()),
                            });
                        }
                        return Ok(TypedExpr {
                            ty: TypeId::Int,
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    // 基元类型没有用户定义的方法。尝试扩展方法脱糖（同命名空间
                    // 或 using 导入的扩展方法可见时，`42.Bar()` 脱糖为
                    // `FooExt.Bar(42)`）。这是 C# 扩展方法语义在基元类型上的
                    // 正常路径——基元类型本身无法添加方法，只能通过扩展方法扩展。
                    let prim_tname: Ident = match recv.ty {
                        TypeId::Int => "int".into(),
                        TypeId::Long => "long".into(),
                        TypeId::Short => "short".into(),
                        TypeId::Byte => "byte".into(),
                        TypeId::Float => "float".into(),
                        TypeId::Double => "double".into(),
                        TypeId::Bool => "bool".into(),
                        TypeId::Char => "char".into(),
                        TypeId::UInt => "uint".into(),
                        TypeId::ULong => "ulong".into(),
                        TypeId::UShort => "ushort".into(),
                        TypeId::SByte => "sbyte".into(),
                        TypeId::String => "string".into(),
                        _ => {
                            return Err(TypeError::Oop(format!(
                                "no method `{method}` on primitive type `{}`",
                                recv.ty.display()
                            )));
                        }
                    };
                    match self.registry.resolve_extension(
                        &prim_tname,
                        method,
                        args.len(),
                        &[],
                        &self.access_ctx(),
                    ) {
                        Ok(Some(ext_res)) => {
                            let msig = &ext_res.sig;
                            // 尾随 `params` 槽（RFC 005）接受可变个数：仅要求
                            // `args.len() >= 固定参数个数`；非 params 要求严格相等。
                            let is_params = msig.params.last().is_some_and(|p| p.is_params);
                            let fixed = if is_params {
                                msig.params.len() - 1
                            } else {
                                msig.params.len()
                            };
                            if (is_params && args.len() < fixed)
                                || (!is_params && args.len() != fixed)
                            {
                                return Err(TypeError::Mismatch {
                                    expected: format!("{} argument(s)", fixed),
                                    found: format!("{} argument(s)", args.len()),
                                });
                            }
                            for (i, (arg, param)) in args.iter().zip(msig.params.iter()).enumerate()
                            {
                                if is_params && i >= fixed {
                                    // params 槽逐元素类型校验交由 check_call_bind
                                    // 的 span 打包处理（此处仅算返回类型）。
                                    break;
                                }
                                let aty = self.check_expr_at(arg.span, &arg.node)?.ty;
                                let expected = TypeId::Named(param.ty.clone());
                                if !self.types_compatible(&expected, &aty) {
                                    return Err(TypeError::Mismatch {
                                        expected: expected.display(),
                                        found: aty.display(),
                                    });
                                }
                            }
                            // 决策 #7（RFC 010）：泛型扩展方法触发单态化。
                            if let Some(inferred_arg) = &ext_res.inferred_arg {
                                self.instantiate_generic_extension_fn_by_key(
                                    &ext_res.template_key,
                                    std::slice::from_ref(inferred_arg),
                                )?;
                            } else if !ext_res.type_args.is_empty() {
                                self.instantiate_generic_extension_fn_by_key(
                                    &ext_res.template_key,
                                    &ext_res.type_args,
                                )?;
                            }
                            // RFC 005：基元接收者扩展方法 `params Span/ReadOnlySpan`
                            // 调用点**纯标注**。此前本路径仅校验实参并返回 `expr.clone()`
                            //（不重写），MIR 把尾随 string 实参当裸指针传 → 访问
                            // Span.Length/元素时 0xC0000005。此处经
                            // `bind_extension_args` 把尾随实参保留为独立实参并返回
                            // `ParamsSpanInfo` 标注——由 MIR 单一物化点 SpanFromStack
                            // 收集发射胖指针，与用户方法/通用扩展路径统一。
                            //
                            // 实参统一绑定（params 与否一律经 `bind_extension_args`）：
                            // 非 params 扩展此前返回 `expr.clone()` 不重写实参，装箱
                            // （string/基元 → object 形参，RFC 004 P0）与隐式 variant
                            // 构造（RFC 037 M2）被跳过——`AddKeyedSingleton<T>(key)`
                            // 的 string key 未装箱即入 object? 槽，对侧 `(string)` 拆箱
                            // 恒 NULL（keyed DI 解析全灭，实测回归）。
                            let (bound_args, params_span) =
                                self.bind_extension_args(&msig.params, args)?;
                            let final_expr = Expr::MethodCall {
                                receiver: receiver.clone(),
                                method: method.clone(),
                                args: bound_args,
                                type_args: type_args.clone(),
                                params_span,
                            };
                            let ret_ty = self.canonical_type(&TypeId::Named(msig.ret.clone()));
                            return Ok(TypedExpr {
                                ty: ret_ty,
                                expr: final_expr,
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        Ok(None) => {
                            return Err(TypeError::Oop(format!(
                                "no method `{method}` on primitive type `{}`",
                                recv.ty.display()
                            )));
                        }
                        Err(oop_err) => {
                            // 决策 #8：扩展方法歧义（多个并列候选）报错
                            return Err(TypeError::Oop(oop_err.to_string()));
                        }
                    }
                }
                // LINQ method resolution is only triggered for the recognized
                // operator set. Stream ops: Where/Select/OrderBy/OrderByDescending. Terminals:
                // Any/Count/First/FirstOrDefault（MIR 编译期展开；非泛型扩展方法体）。
                // Other method calls on IEnumerable/List<T> (e.g. `Get`, `Add`)
                // must fall through to ordinary OOP resolution. Without this
                // guard, `nums.Get(0)` on a `List<int>` would be misclassified
                // as a LINQ operator and resolve to `Void`.
                // Note: `list.Count`（属性）走 Field；`list.Count()` / `Count(pred)`
                // 才是 LINQ 终端，与属性无冲突。
                let is_linq_stream = matches!(
                    method.as_str(),
                    "Where" | "Select" | "OrderBy" | "OrderByDescending"
                );
                let is_linq_terminal = matches!(
                    method.as_str(),
                    "Any" | "Count" | "First" | "FirstOrDefault"
                );
                // RFC 007：泛型物化终端 `ToList`/`ToArray`（MIR 编译期展开，复用
                // `materialize_linq_chain_to_list`）。`ToList` 对任意可枚举源
                // （List / 数组 / 查询链）成立；`ToArray` 仅查询链形态走 LINQ——
                // 裸 `List<T>.ToArray()` 已有 facade 方法（rt_list_to_array），
                // 须放行给 OOP 路径避免劫持。二者均要求 0 值实参。
                let is_linq_materialize_call = matches!(method.as_str(), "ToList" | "ToArray")
                    && (method.as_str() == "ToList"
                        || matches!(
                            &receiver.node,
                            Expr::MethodCall { method, .. }
                                if matches!(
                                    method.as_str(),
                                    "Where" | "Select" | "OrderBy" | "OrderByDescending"
                                )
                        ))
                    && args.is_empty();
                let path = if recv.ty.is_iqueryable() && is_linq_stream {
                    for a in args {
                        if matches!(&a.node, Expr::Lambda(l) if l.is_expression_tree) {
                            return Err(TypeError::QueryableRequiresExpression);
                        }
                        if matches!(&a.node, Expr::ExpressionLit(_)) {
                            return Err(TypeError::QueryableRequiresExpression);
                        }
                    }
                    Some(LinqPath::Queryable)
                } else if recv.ty.is_ienumerable() && (is_linq_stream || is_linq_terminal) {
                    if is_linq_terminal {
                        // Enumerable 诚实子集：0 参或单谓词 lambda；非 Queryable。
                        let arity_ok = match args.len() {
                            0 => true,
                            1 => matches!(&args[0].node, Expr::Lambda(_)),
                            _ => false,
                        };
                        if !arity_ok {
                            return Err(TypeError::Oop(format!(
                                "LINQ `{method}` expects 0 arguments or a single predicate lambda"
                            )));
                        }
                    }
                    Some(LinqPath::Enumerable)
                } else if recv.ty.is_ienumerable() && is_linq_materialize_call {
                    Some(LinqPath::Enumerable)
                } else {
                    None
                };
                // RFC 007：命名实参或候选含默认值时，走绑定脱糖路径。
                // RFC 006：实参含目标类型 `new()` 时亦走绑定，按形参槽填类型
                // （禁止等长普通路径先 check 到 Infer → 硬错误或 MIR unknown）。
                // RFC 017 #16：实参含 `[…]` 时亦走绑定，按形参槽做 `List<T>` 目标脱糖
                // （否则先 check 成 `T[]`，`Consume(List<int>)` 重载匹配失败）。
                let has_ttnew = args.iter().any(|a| contains_target_typed_new(&a.node));
                let has_collexpr = args.iter().any(|a| contains_collection_expr(&a.node));
                // RFC 008 M3/S0：含方法组形态实参的等长调用走槽绑定路径——方法组脱糖
                // 需要目标形参类型（`bind_args_to_slots` → `maybe_coerce_method_group`），
                // 普通路径先 check 实参会在方法组上失败（报「no field or property」）。
                let has_method_group = args
                    .iter()
                    .any(|a| self.looks_like_method_group_shape(&a.node));
                if path.is_none() && type_args.is_empty() {
                    let has_named = args
                        .iter()
                        .any(|a| matches!(&a.node, Expr::NamedArg { .. }));
                    let tname_opt: Option<Ident> = match &recv.ty {
                        TypeId::Named(n) => Some(n.clone()),
                        TypeId::String => Some("string".into()),
                        TypeId::Object => Some("object".into()),
                        _ => None,
                    };
                    if let Some(tname) = tname_opt {
                        // 命名/默认/短实参，或候选形参含 variant（RFC 004 §D9 实例
                        // 方法隐式构造）时走绑定脱糖。不可对所有等长调用一律
                        // try_bind——会抢先匹配 `Task<T>` 模板签名，破坏
                        // `Task_int` 等单态化重载。含 ttnew / 集合表达式时例外。
                        let cands = self.registry.collect_method_overloads(
                            &tname,
                            method,
                            &self.access_ctx(),
                        );
                        let try_bind = has_named
                            || has_ttnew
                            || has_collexpr
                            || has_method_group
                            || cands
                                .as_ref()
                                .map(|c| {
                                    c.iter().any(|(_, sig)| {
                                        sig.params.iter().any(|p| p.default.is_some())
                                            || sig.params.iter().any(|p| p.is_params)
                                            || args.len() < sig.params.len()
                                            || (sig.params.last().is_some_and(|p| p.is_params)
                                                && args.len() > sig.params.len())
                                            || sig
                                                .params
                                                .iter()
                                                .any(|p| self.registry.is_variant(&p.ty))
                                    })
                                })
                                .unwrap_or(false);
                        if try_bind {
                            match self.resolve_bind_method_call(
                                &tname,
                                method,
                                args,
                                &self.access_ctx(),
                            ) {
                                Ok((_decl, sig, bound, params_span)) => {
                                    // 返回类型须归一化：非泛型 `Task`/`Task_AIReply` 之类
                                    // mangle 名 -> `Task { inner }`，否则 await 时
                                    // 「expected Task<T>, found Task_XXX」。
                                    let ret =
                                        self.canonical_type(&self.param_sig_type_id(&sig.ret));
                                    return Ok(TypedExpr {
                                        ty: ret,
                                        expr: Expr::MethodCall {
                                            receiver: receiver.clone(),
                                            method: method.clone(),
                                            args: bound,
                                            type_args: vec![],
                                            params_span,
                                        },
                                        linq_path: None,
                                        expression_tree: None,
                                    });
                                }
                                Err(e) => {
                                    // 短参数 / 命名 / ttnew / 集合表达式 / params 调用必须能绑定；
                                    // 失败则直接报错，禁止落入旧路径。
                                    let has_params = cands
                                        .as_ref()
                                        .map(|c| {
                                            c.iter().any(|(_, sig)| {
                                                sig.params.iter().any(|p| p.is_params)
                                            })
                                        })
                                        .unwrap_or(false);
                                    if has_named
                                        || has_ttnew
                                        || has_collexpr
                                        || has_params
                                        || args.len()
                                            < cands
                                                .as_ref()
                                                .map(|c| {
                                                    c.iter()
                                                        .map(|(_, s)| s.params.len())
                                                        .min()
                                                        .unwrap_or(0)
                                                })
                                                .unwrap_or(0)
                                    {
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    }
                }
                // RFC 006：显式 type_args 的 MethodCall 含 ttnew 时，按 arity+generics
                // 唯一定位签名、替换形参后 bind（不可落入下方先 check Infer）。
                if path.is_none() && has_ttnew && !type_args.is_empty() {
                    let Some(tname) = self.type_name_of(&recv.ty) else {
                        return Err(TypeError::Oop(
                            "target-typed `new()` requires a concrete type context \
                             (e.g. `T x = new(...)`; `var x = new()` is not allowed)"
                                .into(),
                        ));
                    };
                    let type_arg_names: Vec<Ident> = type_args
                        .iter()
                        .map(|t| {
                            let ty = self.lower_type(&t.node).unwrap_or(TypeId::Infer);
                            type_id_to_field_name(&ty)
                        })
                        .collect();
                    let cands = self
                        .registry
                        .collect_method_overloads(&tname, method, &self.access_ctx())
                        .map_err(|e| TypeError::Oop(e.to_string()))?;
                    let matching: Vec<_> = cands
                        .iter()
                        .filter(|(_, sig)| {
                            sig.generics.len() == type_arg_names.len()
                                && sig.params.len() == args.len()
                        })
                        .collect();
                    match matching.len() {
                        1 => {
                            let sig_template = matching[0].1.clone();
                            let generics = sig_template.generics.clone();
                            let mut sig = sig_template;
                            sig.ret = crate::registry::substitute_generic_in_ty_name(
                                &sig.ret,
                                &generics,
                                &type_arg_names,
                            )
                            .into();
                            sig.params = sig
                                .params
                                .iter()
                                .map(|p| {
                                    let mut p = p.clone();
                                    p.ty = crate::registry::substitute_generic_in_ty_name(
                                        &p.ty,
                                        &generics,
                                        &type_arg_names,
                                    )
                                    .into();
                                    p
                                })
                                .collect();
                            let slots = self.param_slots_from_sigs(&sig.params);
                            let (bound, params_span) = self.bind_args_to_slots(&slots, args)?;
                            let ret = self.canonical_type(&self.param_sig_type_id(&sig.ret));
                            return Ok(TypedExpr {
                                ty: ret,
                                expr: Expr::MethodCall {
                                    receiver: receiver.clone(),
                                    method: method.clone(),
                                    args: bound,
                                    type_args: type_args.clone(),
                                    params_span,
                                },
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        0 => {
                            return Err(TypeError::Oop(format!(
                                "no matching overload for `{tname}.{method}`"
                            )));
                        }
                        _ => {
                            return Err(TypeError::Oop(format!(
                                "ambiguous overload for `{tname}.{method}`"
                            )));
                        }
                    }
                }
                let mut lambda_body_ty: Option<TypeId> = None;
                let mut arg_types: Vec<TypeId> = Vec::new();
                let mut rewritten_args: Vec<Spanned<Expr>> = Vec::with_capacity(args.len());
                let mut args_rewritten = false;
                for a in args {
                    if let Expr::Lambda(l) = &a.node {
                        if let Some(elem) = recv.ty.enumerable_elem() {
                            // RFC 037 M1: 当 list 元素类型本身是 Func/Action mangled
                            // 名时（如 `List<Func<T, T, void>>` 单态化为
                            // `List_Func_T_T_void`），lambda 是元素值本身（不是元素上
                            // 的函数）。需解构 elem 取出 Func 的参数类型绑定到 lambda
                            // 参数。
                            //
                            // 例：`_changedHandlers.Add((_, newValue) => handler(newValue))`
                            // 其中 `_changedHandlers: List<Func<T, T, void>>`：
                            //   - elem = Named("Func_T_T_void")
                            //   - 旧实现：lambda 的 _ 和 newValue 都绑定到 elem
                            //     → handler(newValue) 中 newValue 类型错误
                            //       （handler 期望 Named("T")，实际 Named("Func_T_T_void")）
                            //   - 新实现：demangle elem 得
                            //     Func { params: [Named("T"), Named("T")], ret: Void }
                            //     → 绑定 _ 和 newValue 到 Named("T")，类型匹配
                            let lambda_param_tys: Vec<TypeId> = if let TypeId::Named(name) = &elem {
                                match demangle_func_type_with(name, l.params.len(), &|s| {
                                    self.registry.types.contains_key(s)
                                }) {
                                    Some(TypeId::Func { params, .. }) => params,
                                    _ => vec![elem.clone(); l.params.len()],
                                }
                            } else {
                                vec![elem.clone(); l.params.len()]
                            };
                            self.scopes.push(IndexMap::new());
                            for (p, ty) in l.params.iter().zip(lambda_param_tys.iter()) {
                                self.scopes
                                    .last_mut()
                                    .unwrap()
                                    .insert(p.name.clone(), ty.clone());
                            }
                            let body_ty = match &l.body {
                                LambdaBody::Expr(e) => self.check_expr_at(e.span, &e.node)?.ty,
                                LambdaBody::Block(b) => {
                                    for stmt in &b.stmts {
                                        self.check_stmt(&stmt.node)?;
                                    }
                                    if let Some(tail) = &b.tail {
                                        self.check_expr_at(tail.span, &tail.node)?.ty
                                    } else {
                                        TypeId::Void
                                    }
                                }
                            };
                            lambda_body_ty = Some(body_ty.clone());
                            self.scopes.pop();
                            arg_types.push(TypeId::Func {
                                params: lambda_param_tys.clone(),
                                ret: Box::new(body_ty),
                            });
                            rewritten_args.push(a.clone());
                            continue;
                        }
                    }
                    // RFC 006 M2：保留嵌套目标类型 `new()` 等实参重写，避免 MIR 见到 Infer。
                    let te = self.check_expr_at(a.span, &a.node)?;
                    if te.expr != a.node {
                        args_rewritten = true;
                    }
                    rewritten_args.push(Spanned::new(te.expr, a.span));
                    arg_types.push(te.ty);
                }
                let ret = if let Some(linq_path) = &path {
                    match (method.as_str(), linq_path) {
                        ("Where", LinqPath::Queryable) | ("Where", LinqPath::Enumerable) => {
                            recv.ty.clone()
                        }
                        ("Select", LinqPath::Queryable) | ("Select", LinqPath::Enumerable) => {
                            lambda_body_ty
                                .map(|elem| recv.ty.with_enumerable_elem(elem))
                                .unwrap_or_else(|| recv.ty.clone())
                        }
                        ("OrderBy", _) | ("OrderByDescending", _) => recv.ty.clone(),
                        ("Any", LinqPath::Enumerable) => TypeId::Bool,
                        ("Count", LinqPath::Enumerable) => TypeId::Int,
                        ("First", LinqPath::Enumerable)
                        | ("FirstOrDefault", LinqPath::Enumerable) => {
                            recv.ty.enumerable_elem().unwrap_or(TypeId::Infer)
                        }
                        // RFC 007：泛型物化终端返回具体集合类型——`ToList` →
                        // `List_<T>`（mangle 名，供 MIR materializer 直建）、`ToArray` →
                        // `T[]`。元素类型取投影后（Select 链已由 `with_enumerable_elem`
                        // 改写接收者类型）的 `enumerable_elem()`。
                        ("ToList", LinqPath::Enumerable) => {
                            let elem = recv.ty.enumerable_elem().unwrap_or(TypeId::Infer);
                            TypeId::Named(
                                crate::generics::mangle_generic(
                                    "List",
                                    std::slice::from_ref(&elem),
                                )
                                .into(),
                            )
                        }
                        ("ToArray", LinqPath::Enumerable) => TypeId::Array {
                            elem: Box::new(recv.ty.enumerable_elem().unwrap_or(TypeId::Infer)),
                        },
                        _ => TypeId::Void,
                    }
                } else if let Some(tname) = self.type_name_of(&recv.ty) {
                    // RFC 038 OOP 路径：静态 `Enum.*<E>()` 调用点发射特化
                    // 方法体（零反射、编译期烘焙枚举数据/位组合判断）。须在方法
                    // 解析与 MIR 克隆之前发射，使 MIR 发现 `Enum::*__E` 已存在而
                    // 跳过从模板克隆（否则回退到返回 stub 的模板体）。
                    // 显式 `<E>`（GetOptions/GetNames/GetValues）与实参推断
                    // （HasFlag/IsDefined 泛型方法）两类形态统一在此发射。
                    self.maybe_emit_enum_baked_method(&tname, method, type_args, &arg_types);
                    // RFC 037 M-D0：观察者入口 `ObserveProperty("Name")`——编译器
                    // 在含 `[Observable]` auto-property 的类上合成的实例方法
                    // （§5.3），该符号无实体方法表条目。typeck 在此识别调用形态
                    // 并给出返回类型 `Signal_<PropType>`（codegen 将调用展开为
                    // 隐藏通道字段的静态定址直访，见 `try_emit_observable_observe`）。
                    if method.as_str() == "ObserveProperty" {
                        return self.check_observable_observe_call(expr, args, type_args, &tname);
                    }
                    // RFC 037 M-D0：通知侧入口 `NotifyPropertyChanged("Name")`——
                    // 编译器在含 `[Observable]` 属性的类上合成的实例方法（§5.3
                    // 场景 6，与 ObserveProperty 对偶：订阅侧 vs 通知侧），该符号
                    // 无实体方法表条目。typeck 在此识别调用形态并给出返回类型
                    // `void`（codegen 将调用展开为隐藏通道的显式 raise——
                    // `Signal<T>.Set(当前值)`，见 `try_emit_observable_notify`）。
                    if method.as_str() == "NotifyPropertyChanged" {
                        return self.check_observable_notify_call(expr, args, type_args, &tname);
                    }
                    let has_lambda = args.iter().any(|a| matches!(a.node, Expr::Lambda(_)));
                    // 无显式 type_args 时，从实参推断方法级泛型（`Assert.Empty(xs)`）。
                    let mut inferred_type_args: Option<Vec<Ident>> = None;
                    let resolve_result = if has_lambda {
                        // Lambda arguments to user-defined methods target
                        // Expression<T>/Func<...> parameters. When the parameter is
                        // Func<...>, the lambda's inferred TypeId::Func mangles to the
                        // same name as the parameter type, so normal overload resolution
                        // can match. Try it first so the correct arity overload is picked
                        // (e.g., `Sort(Func<T,T,int>)` over `Sort()`). Fall back to the
                        // name-only resolution for Expression<T> parameters where the
                        // lambda type is not directly assignable. (Queryable path:
                        // typeck records the expression-tree IR; translation is
                        // runtime-side via IQueryProvider, RFC 022.)
                        let arg_type_names: Vec<ast::Ident> =
                            arg_types.iter().map(type_id_to_field_name).collect();
                        // RFC 006 M4：带 lambda 实参的泛型方法若携带**显式** type_args
                        //（如 `BindCollection<int>(host, sig, (args) => …)`），必须优先按
                        // 显式 type_args 替换签名——否则下方 λ 推断/逃逸路径会回落到
                        // name-only 解析，返回未替换的 `Action_CollectionChangedEventArgs_T`，
                        // 报 `expected _T, found _int`。此路径与无 λ 的显式 type_args
                        // 分支（`resolve_method_with_type_args`）对齐，替换后再做 λ 校验。
                        if !type_args.is_empty() {
                            let type_arg_names: Vec<ast::Ident> = type_args
                                .iter()
                                .map(|t| {
                                    let ty = self.lower_type(&t.node).unwrap_or(TypeId::Infer);
                                    type_id_to_field_name(&ty)
                                })
                                .collect();
                            if let Ok((d, s)) = self.registry.resolve_method_with_type_args(
                                &tname,
                                method,
                                &arg_type_names,
                                &type_arg_names,
                                &self.access_ctx(),
                            ) {
                                Ok((d, s))
                            } else {
                                self.registry.resolve_method_overload(
                                    &tname,
                                    method,
                                    &arg_type_names,
                                    &self.access_ctx(),
                                )
                            }
                        } else {
                            self.registry
                                .resolve_method_overload(
                                    &tname,
                                    method,
                                    &arg_type_names,
                                    &self.access_ctx(),
                                )
                                .or_else(|_| {
                                    // `Assert.All(xs, x => …)`：lambda 使本支跳过无 lambda
                                    // 的 infer 路径；须在此推断 `All<T>`，否则回落到
                                    // 未替换的 `List_T` → `expected List_T, found List_int`。
                                    match self.registry.resolve_method_infer_type_args(
                                        &tname,
                                        method,
                                        &arg_type_names,
                                        &self.access_ctx(),
                                    ) {
                                        Ok((d, s, inferred)) => {
                                            inferred_type_args = Some(inferred);
                                            Ok((d, s))
                                        }
                                        Err(e) => Err(e),
                                    }
                                })
                                .or_else(|_| {
                                    // `Func_*_Infer` ↔ `Action`/`Func_void`：仅唯一软匹配时采用
                                    // （避免 Run(Action)/Run<T> 双命中歧义）。
                                    self.registry.resolve_method_overload_lambda_soft(
                                        &tname,
                                        method,
                                        &arg_type_names,
                                        &self.access_ctx(),
                                    )
                                })
                                .or_else(|_| {
                                    // Expression<T> 等仍可能需要 name-only 首签名（历史路径）。
                                    self.registry
                                        .resolve_method(&tname, method, &self.access_ctx())
                                        .map(|sig| (tname.clone(), sig))
                                })
                        }
                    } else {
                        let arg_type_names: Vec<ast::Ident> =
                            arg_types.iter().map(type_id_to_field_name).collect();
                        // RFC 037 M3：显式 type_args 的泛型方法调用（如
                        // `this.GetValue<double>(WidthProperty)`）需走
                        // `resolve_method_with_type_args` 路径——将方法签名中的
                        // 泛型占位符 T 替换为具体类型实参后再做参数匹配。
                        // 否则 `DependencyProperty_T`（占位符）无法匹配
                        // `DependencyProperty_double`（实参类型），导致重载解析失败。
                        if !type_args.is_empty() {
                            let type_arg_names: Vec<ast::Ident> = type_args
                                .iter()
                                .map(|t| {
                                    let ty = self.lower_type(&t.node).unwrap_or(TypeId::Infer);
                                    type_id_to_field_name(&ty)
                                })
                                .collect();
                            self.registry.resolve_method_with_type_args(
                                &tname,
                                method,
                                &arg_type_names,
                                &type_arg_names,
                                &self.access_ctx(),
                            )
                        } else {
                            match self.registry.resolve_method_overload(
                                &tname,
                                method,
                                &arg_type_names,
                                &self.access_ctx(),
                            ) {
                                Ok(r) => Ok(r),
                                Err(e) => match self.registry.resolve_method_infer_type_args(
                                    &tname,
                                    method,
                                    &arg_type_names,
                                    &self.access_ctx(),
                                ) {
                                    Ok((d, s, inferred)) => {
                                        inferred_type_args = Some(inferred);
                                        Ok((d, s))
                                    }
                                    Err(_) => Err(e),
                                },
                            }
                        }
                    };
                    match resolve_result {
                        Ok((_declaring, msig)) => {
                            // 方法级泛型 + 无显式 type_args + 未走推断路径：即使
                            // overload **平凡匹配**成功（形参 T ≡ 实参 T，同为当前
                            // 方法型参——泛型方法转调泛型方法，如
                            // `World.TryGetComponent<T>` 转调
                            // `_registry.TryGetComponent(actorId, out component)`），
                            // 也须推断出类型实参并回写显式 type_args。否则 lower 的
                            // target mangle 无 `__T` 后缀，单态化传播断裂——外层
                            // 单态化体（`World_TryGetComponent__HealthComponent`）
                            // 转调仍指向模板名 `@ActorRegistry_TryGetComponent`，
                            // 模板体内 `item is T` 的 TypeInfoPtr 型参名不替换 →
                            // rt_obj_isa 收 null typeinfo 恒 false（l3
                            // component_store 实测）。
                            if !msig.generics.is_empty()
                                && type_args.is_empty()
                                && inferred_type_args.is_none()
                            {
                                let arg_type_names: Vec<ast::Ident> =
                                    arg_types.iter().map(type_id_to_field_name).collect();
                                if let Ok((_d2, _s2, inferred)) =
                                    self.registry.resolve_method_infer_type_args(
                                        &tname,
                                        method,
                                        &arg_type_names,
                                        &self.access_ctx(),
                                    )
                                {
                                    inferred_type_args = Some(inferred);
                                }
                            }
                            if arg_types.len() != msig.params.len() {
                                if std::env::var("ARC_DEBUG_BIND").is_ok() {
                                    eprintln!(
                                        "[ARITY] .{} expects {} got {} | sig={:?}",
                                        method,
                                        msig.params.len(),
                                        arg_types.len(),
                                        msig.params
                                    );
                                }
                                return Err(TypeError::Mismatch {
                                    expected: format!("{} argument(s)", msig.params.len()),
                                    found: format!("{} argument(s)", arg_types.len()),
                                });
                            }
                            // RFC 004 刀 2 约束修复（Step 2）：static class / 实例
                            // 泛型方法调用点验证 where 约束（Step 1 已把泛型方法
                            // 模板注册到 fn_templates）。此前 `Maker.Make<IntFactory>`
                            // 走 OOP resolve_method_with_type_args，where_clause 被
                            // OopMethodSig 丢弃 → 约束检查被跳过（`IntFactory :
                            // IFactory<int>` 被错误接受用于 `where T : IFactory<T>`）。
                            // 此处分派到 fn_templates 的 where_clause 验证。
                            if !msig.generics.is_empty() && !type_args.is_empty() {
                                let arg_type_names: Vec<ast::Ident> =
                                    arg_types.iter().map(type_id_to_field_name).collect();
                                let type_arg_names: Vec<ast::Ident> = type_args
                                    .iter()
                                    .map(|t| {
                                        let ty = self.lower_type(&t.node).unwrap_or(TypeId::Infer);
                                        type_id_to_field_name(&ty)
                                    })
                                    .collect();
                                if let Some(template_link) =
                                    self.registry.method_generic_template_link_name(
                                        &tname,
                                        method,
                                        &arg_type_names,
                                        &type_arg_names,
                                        &self.access_ctx(),
                                    )
                                {
                                    // 先 clone 出 where_clause/generics，释放 fn_templates
                                    // 的 immutable borrow，再 check_constraints（需 mutable self）。
                                    let template_data = self
                                        .fn_templates
                                        .get(template_link.as_str())
                                        .map(|t| (t.where_clause.clone(), t.generics.clone()));
                                    if let Some((where_clause, generics)) = template_data {
                                        let targs: Vec<TypeId> = type_args
                                            .iter()
                                            .map(|t| self.lower_type(&t.node))
                                            .collect::<Result<_, _>>()?;
                                        self.check_constraints(&where_clause, &generics, &targs)?;
                                    }
                                }
                            }
                            for (i, (aty, param)) in
                                arg_types.iter().zip(msig.params.iter()).enumerate()
                            {
                                // Expression<T> 参数仍跳过：表达式树被捕获为 AST，
                                // 由 IQueryProvider 在运行时翻译（RFC 022），此处不
                                // 对 lambda 体做签名校验。type_path_name 剥掉泛型实参，
                                // 因此 "Expression" 同时匹配 Expression<T> 与
                                // Expression<Func<U, bool>>。
                                if matches!(aty, TypeId::Func { .. })
                                    && (param.ty.as_str() == "Expression"
                                        || param.ty.as_str().starts_with("Expression_"))
                                {
                                    continue;
                                }
                                // M5 事件签名匹配：Func/Action 参数收到内联 lambda 时，
                                // 解构出形参类型并对 lambda 体做完整校验（arity + 方法
                                // 解析 + 返回类型）。此前一律 continue 跳过，导致 .arml
                                // `Click="OnRefresh"` 生成的 `OnClick(_ => this.OnRefresh())`
                                // 中 handler 签名错误或缺失无法在 typeck 阶段捕获，拖到
                                // LLVM 才报 `undefined value`。表达式树仍按上方规则跳过。
                                if matches!(aty, TypeId::Func { .. })
                                    && (param.ty.as_str() == "Func"
                                        || param.ty.as_str().starts_with("Func_")
                                        || param.ty.as_str() == "Action"
                                        || param.ty.as_str().starts_with("Action_"))
                                {
                                    if let Some(arg_expr) = args.get(i) {
                                        if let Expr::Lambda(l) = &arg_expr.node {
                                            if let Some(TypeId::Func { params, ret }) =
                                                demangle_func_type_with(
                                                    &param.ty,
                                                    l.params.len(),
                                                    &|s| self.registry.types.contains_key(s),
                                                )
                                            {
                                                self.check_func_lambda(l, &params, &ret)?;
                                            }
                                        }
                                    }
                                    continue;
                                }
                                let expected = TypeId::Named(param.ty.clone());
                                if !self.types_compatible(&expected, aty) {
                                    // RFC 017 残余补全：集合表达式实参对 `T[]`
                                    // 形参——按目标元素类型检查后放行（实参 AST
                                    // 保持集合形态，MIR 按元素推断数组元素类型）。
                                    // 否则单元素集合被独立推断为元素类型
                                    //（`app.Inject(["dep"], cb)` → string）。
                                    if let Expr::CollectionExpr { .. } = &rewritten_args[i].node {
                                        let expected_tid =
                                            resolve_named_type_id(param.ty.clone());
                                        if matches!(&expected_tid, TypeId::Array { .. })
                                            && self.try_bind_collection_array_target(
                                                &rewritten_args[i].node,
                                                &expected_tid,
                                            )?
                                        {
                                            continue;
                                        }
                                    }
                                    return Err(TypeError::Mismatch {
                                        expected: expected.display(),
                                        found: aty.display(),
                                    });
                                }
                            }
                            // RFC 004 P0 Phase 1：object 形参 + 基元实参 → 装箱
                            // （统一入口，string 亦经此装箱）。
                            for (i, (param, aty)) in
                                msig.params.iter().zip(arg_types.iter()).enumerate()
                            {
                                let a = &rewritten_args[i];
                                let boxed = box_to_object(
                                    &self.registry,
                                    a.node.clone(),
                                    aty,
                                    param.ty.as_str(),
                                    a.span,
                                );
                                if boxed != a.node {
                                    rewritten_args[i] = Spanned::new(boxed, a.span);
                                    args_rewritten = true;
                                }
                            }
                            if let Some(inferred) = &inferred_type_args {
                                let rewritten_targs: Vec<Spanned<ast::Type>> = inferred
                                    .iter()
                                    .map(|n| {
                                        Spanned::new(
                                            ast::Type::Named {
                                                path: vec![n.clone()],
                                                generics: vec![],
                                            },
                                            Span::DUMMY,
                                        )
                                    })
                                    .collect();
                                override_expr = Some(Expr::MethodCall {
                                    receiver: receiver.clone(),
                                    method: method.clone(),
                                    args: if args_rewritten {
                                        rewritten_args.clone()
                                    } else {
                                        args.to_vec()
                                    },
                                    type_args: rewritten_targs,
                                    params_span: params_span.clone(),
                                });
                            }
                            self.canonical_type(&TypeId::Named(msig.ret.clone()))
                        }
                        Err(oop_err) => {
                            // 显式 type_args 的泛型扩展方法（如 `AddTransient<TService,TImpl>`）：
                            // 按泛型参数个数过滤候选 + mangle 调用名 + 触发方法体单态化。
                            // 保留 OOP 解析的原始错误：若 instance 解析命中「跨包泛型模板缺失」
                            //（RFC 038 M2-G3b），须在扩展方法也未命中时原样透出，而非被
                            // 掩盖成误导性的「无匹配重载」（报错 > 静默推断）。
                            let ext_type_arg_names: Vec<ast::Ident> = type_args
                                .iter()
                                .map(|t| {
                                    let ty = self.lower_type(&t.node).unwrap_or(TypeId::Infer);
                                    type_id_to_field_name(&ty)
                                })
                                .collect();
                            match self.registry.resolve_extension_with_arg_types(
                                &tname,
                                method,
                                args.len(),
                                &ext_type_arg_names,
                                &arg_types
                                    .iter()
                                    .map(type_id_to_field_name)
                                    .collect::<Vec<ast::Ident>>(),
                                &self.access_ctx(),
                            ) {
                                Ok(Some(ext_res)) => {
                                    let msig = &ext_res.sig;
                                    // 尾随 `params` 槽（RFC 005）接受可变个数：仅要求
                                    // `args.len() >= 固定参数个数`；非 params 要求严格相等。
                                    let is_params = msig.params.last().is_some_and(|p| p.is_params);
                                    let fixed = if is_params {
                                        msig.params.len() - 1
                                    } else {
                                        msig.params.len()
                                    };
                                    if (is_params && args.len() < fixed)
                                        || (!is_params && args.len() != fixed)
                                    {
                                        return Err(TypeError::Mismatch {
                                            expected: format!("{} argument(s)", fixed),
                                            found: format!("{} argument(s)", args.len()),
                                        });
                                    }
                                    for (i, (arg, param)) in
                                        args.iter().zip(msig.params.iter()).enumerate()
                                    {
                                        if is_params && i >= fixed {
                                            // params 槽逐元素类型校验交由 check_call_bind
                                            // 的 span 打包处理（此处仅算返回类型）。
                                            break;
                                        }
                                        let pname = &param.name;
                                        let pty = &param.ty;
                                        let aty = self.check_expr_at(arg.span, &arg.node)?.ty;
                                        let expected = TypeId::Named(pty.clone());
                                        if !self.types_compatible(&expected, &aty) {
                                            return Err(TypeError::Mismatch {
                                                expected: expected.display(),
                                                found: aty.display(),
                                            });
                                        }
                                        let _ = pname;
                                    }
                                    // RFC 005：扩展方法 `params Span/ReadOnlySpan` 调用点
                                    // **纯标注**。此前扩展路径仅校验实参并返回 `expr.clone()`
                                    // （不重写），MIR 把尾随 string 实参当裸指针传 → 访问
                                    // Span.Length/元素时 0xC0000005。此处经
                                    // `bind_extension_args` 把尾随实参保留为独立实参并返回
                                    // `ParamsSpanInfo` 标注（由 MIR 单一物化点 SpanFromStack
                                    // 收集发射），与用户方法/自由函数路径统一。
                                    //
                                    // 实参统一绑定（params 与否一律经 `bind_extension_args`）：
                                    // 非 params 扩展此前不重写实参，装箱（string/基元 →
                                    // object 形参，RFC 004 P0）与隐式 variant 构造（RFC 037
                                    // M2）被跳过——与基元接收者路径同一缺陷（见上方注释）。
                                    let (bound_args, params_span) =
                                        self.bind_extension_args(&msig.params, args)?;
                                    override_expr = Some(Expr::MethodCall {
                                        receiver: receiver.clone(),
                                        method: method.clone(),
                                        args: bound_args,
                                        type_args: type_args.clone(),
                                        params_span,
                                    });
                                    // 决策 #7（RFC 010）：泛型扩展方法触发单态化，
                                    // 生成 `Container::Method_<arg>` 方法体供 MIR/codegen 调用。
                                    // 显式 type_args 走多参实例化（`AddTransient<A,B>`），
                                    // 否则按接收者推断单参（`Id<T>(this T)`）。
                                    if let Some(inferred_arg) = &ext_res.inferred_arg {
                                        self.instantiate_generic_extension_fn_by_key(
                                            &ext_res.template_key,
                                            std::slice::from_ref(inferred_arg),
                                        )?;
                                    } else if !ext_res.type_args.is_empty() {
                                        self.instantiate_generic_extension_fn_by_key(
                                            &ext_res.template_key,
                                            &ext_res.type_args,
                                        )?;
                                    }
                                    self.canonical_type(&TypeId::Named(msig.ret.clone()))
                                }
                                Ok(None) => {
                                    // 扩展方法也未命中：透出 OOP 解析的原始诊断。
                                    // 若为 M2-G3b 的缺失模板诊断，原样上抛（报错 > 静默推断）；
                                    // 否则为标准的「无匹配重载」。
                                    return Err(TypeError::Oop(oop_err.to_string()));
                                }
                                Err(oop_err) => {
                                    // 决策 #8：扩展方法歧义（多个并列候选）报错
                                    return Err(TypeError::Oop(oop_err.to_string()));
                                }
                            }
                        }
                    }
                } else {
                    TypeId::Void
                };
                if args_rewritten && override_expr.is_none() {
                    override_expr = Some(Expr::MethodCall {
                        receiver: receiver.clone(),
                        method: method.clone(),
                        args: rewritten_args,
                        type_args: type_args.clone(),
                        params_span: params_span.clone(),
                    });
                }
                (ret, path, None)
            }
            Expr::Lambda(l) => {
                if l.is_expression_tree {
                    return Err(TypeError::QueryableRequiresExpression);
                }
                // RFC 007 M2c：默认值仅 IIFE；赋值/传参等路径在此拒绝。
                Self::reject_lambda_defaults_outside_iife(&l.params)?;
                // RFC 009 M6: async lambda 的返回类型是 `Task<T>`，T 由 body 推断。
                let ret_ty = if l.is_async {
                    TypeId::Task {
                        inner: Box::new(TypeId::Infer),
                    }
                } else {
                    TypeId::Infer
                };
                (
                    TypeId::Func {
                        params: l.params.iter().map(|_| TypeId::Infer).collect(),
                        ret: Box::new(ret_ty),
                    },
                    None,
                    None,
                )
            }
            Expr::ExpressionLit(_) => {
                return Err(TypeError::QueryableRequiresExpression);
            }
            Expr::Await(inner) => {
                if !self.in_async {
                    return Err(TypeError::AwaitOutsideAsync);
                }
                // 传播内层重写：`await Run(Fetch)` 中 `Fetch` 方法组经 typeck 脱糖为
                // lambda（`() => Fetch()`），若只取 `.ty` 而丢弃 `.expr`，MIR 会把
                // 未重写的 `Fetch` 裸 ident 当操作数 → "unresolved ident" 内部错误。
                let checked_inner = self.check_expr_at(inner.span, &inner.node)?;
                // 先 canonical_type 归一：method_group/绑定返回的 `Task<...>` 以
                // registry mangle 名（如 `Named("Task_string[]")`）到达此处置于
                // `other` 分支报 "expected Task<T>, found Task_string[]"（Arc.IO
                // `Task<string[]>` 可达时触发）。归一为结构化 `TypeId::Task` 后再匹配。
                let awaited_ty = self.canonical_type(&checked_inner.ty);
                match awaited_ty {
                    TypeId::Task { inner: task_inner } => {
                        if checked_inner.expr != inner.node {
                            override_expr = Some(Expr::Await(Box::new(Spanned::new(
                                checked_inner.expr,
                                inner.span,
                            ))));
                        }
                        (*task_inner, None, None)
                    }
                    other => {
                        return Err(TypeError::Mismatch {
                            expected: "Task<T>".into(),
                            found: other.display(),
                        });
                    }
                }
            }
            Expr::Query(q) => {
                let mut from_elem = TypeId::Infer;
                let mut from_ident: Option<ast::Ident> = None;
                let mut src_ty = TypeId::Infer;
                // 单次 in-order 遍历：`let` / `join` 引入的变量与 `groupby` 对
                // range var 的重绑必须按子句顺序入作用域，select 才能解析
                // （分两遍会丢失中间绑定）。Where/OrderBy 保持只检查表达式。
                for clause in &q.clauses {
                    match clause {
                        QueryClause::From { ident, source } => {
                            from_ident = Some(ident.clone());
                            src_ty = self.check_expr_at(source.span, &source.node)?.ty;
                            from_elem =
                                match &src_ty {
                                    TypeId::IQueryable { inner } => (**inner).clone(),
                                    _ => src_ty.enumerable_elem().ok_or_else(|| {
                                        TypeError::Mismatch {
                                            expected: "IEnumerable<T>, IQueryable<T>, or T[]"
                                                .into(),
                                            found: src_ty.display(),
                                        }
                                    })?,
                                };
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(ident.clone(), from_elem.clone());
                        }
                        QueryClause::Let { ident, value } => {
                            let vty = self.check_expr_at(value.span, &value.node)?.ty;
                            self.scopes.last_mut().unwrap().insert(ident.clone(), vty);
                        }
                        QueryClause::Where(e) => {
                            self.check_expr_at(e.span, &e.node)?;
                        }
                        QueryClause::OrderBy { key, .. } => {
                            self.check_expr_at(key.span, &key.node)?;
                        }
                        QueryClause::Join {
                            ident,
                            source,
                            on_left,
                            on_right,
                        } => {
                            let inner_src_ty = self.check_expr_at(source.span, &source.node)?.ty;
                            let inner_elem = inner_src_ty.enumerable_elem().ok_or_else(|| {
                                TypeError::Mismatch {
                                    expected: "IEnumerable<T>, IQueryable<T>, or T[]".into(),
                                    found: inner_src_ty.display(),
                                }
                            })?;
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(ident.clone(), inner_elem);
                            self.check_expr_at(on_left.span, &on_left.node)?;
                            self.check_expr_at(on_right.span, &on_right.node)?;
                        }
                        QueryClause::GroupBy {
                            key,
                            element,
                            into_ident,
                        } => {
                            let key_ty = self.check_expr_at(key.span, &key.node)?.ty;
                            let item_ty = if let Some(el) = element {
                                self.check_expr_at(el.span, &el.node)?.ty
                            } else {
                                from_elem.clone()
                            };
                            // range var 重绑为 `Grouping<K, T>`（对标 C# IGrouping）。
                            // 命名采用 mangled 泛型名，MIR 物化 `new Grouping_<K>_<T>`
                            // 与之保持一致；select 对 `g.Key` / `g.Count` 经 registry
                            // 解析到 `std/Arc/Linq/Grouping.as` 的公开成员。
                            // `force_instantiate_generic_class` 保证即使 select 未引用
                            // 任何 Grouping 成员（如 `select 0`），单态化类也已注册，
                            // MIR `resolve_method_target` 才能解析 get_Key/Add。
                            let _ = self.force_instantiate_generic_class(
                                &ast::Ident::from("Grouping"),
                                &[key_ty.clone(), item_ty.clone()],
                            );
                            let group_ty = TypeId::Named(
                                crate::generics::mangle_generic("Grouping", &[key_ty, item_ty])
                                    .into(),
                            );
                            let group_ident = into_ident.clone().unwrap_or_else(|| {
                                from_ident.clone().unwrap_or_else(|| ast::Ident::from("x"))
                            });
                            self.scopes
                                .last_mut()
                                .unwrap()
                                .insert(group_ident, group_ty);
                        }
                    }
                }
                self.check_expr_at(q.select.span, &q.select.node)?;
                let select_ty = match &q.select.node {
                    Expr::Lambda(l) => match &l.body {
                        LambdaBody::Expr(e) => self.check_expr_at(e.span, &e.node)?.ty,
                        LambdaBody::Block(b) => b
                            .tail
                            .as_ref()
                            .map(|t| self.check_expr_at(t.span, &t.node).map(|x| x.ty))
                            .transpose()?
                            .unwrap_or(TypeId::Infer),
                    },
                    _ => self.check_expr_at(q.select.span, &q.select.node)?.ty,
                };
                let path = if src_ty.is_iqueryable() {
                    LinqPath::Queryable
                } else {
                    LinqPath::Enumerable
                };
                (
                    TypeId::IEnumerable {
                        inner: Box::new(select_ty),
                    },
                    Some(path),
                    None,
                )
            }
            Expr::Field { receiver, field } => {
                // RFC 004 M1：非泛型 variant 无 payload case 构造（`Content.None`）。
                // receiver 为类型名（Ident）且 field 为 variant case 时**优先于字段解析**——
                // `ContentPresenter.Content` 字段与 `Content` variant 类型同名场景下，
                // `this.Content = Content.None` 的 RHS `Content` 必须是类型名
                //（C# 类型/字段命名空间分离语义）。缺此拦截会把 receiver 解析为
                // 隐式 `this.Content`（字段）并重写为 `(this.Content).None`，MIR 收到
                // 错误形态 → 生成读 this.Content 的垃圾代码（0xC0000005 实测）。
                if let Expr::Ident(name) = &receiver.node {
                    if self.registry.is_variant(name) {
                        if let Some(case_info) = self.registry.variant_case(name, field) {
                            if case_info.payload.is_none() {
                                return Ok(TypedExpr {
                                    ty: TypeId::Named(name.clone()),
                                    expr: expr.clone(),
                                    linq_path: None,
                                    expression_tree: None,
                                });
                            }
                        }
                    }
                }
                // Task facade (RFC 009 M4): Task.CompletedTask 静态属性拦截。
                // Task 已在全局 scope 注册为 TypeId::Named("Task")，但 stub 不暴露
                // registry 字段/getter；typeck 在此直接返回 Task<Void>，
                // codegen try_emit_task_static 拦截发射 rt_task_void。
                if let Expr::Ident(name) = &receiver.node {
                    if name == "Task" && field == "CompletedTask" {
                        return Ok(TypedExpr {
                            ty: TypeId::Task {
                                inner: Box::new(TypeId::Void),
                            },
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    // CancellationToken facade (RFC 009 M4): `CancellationToken.None`
                    // 静态属性拦截（与 Task.CompletedTask 同型）。语义对齐 .NET：
                    // None 是永不可取消的空令牌；typeck 直接返回 CT 类型，
                    // codegen 在属性 getter 拦截中发射 rt_cts_create（不可取消令牌）。
                    if name == "CancellationToken" && field == "None" {
                        return Ok(TypedExpr {
                            ty: TypeId::Named("CancellationToken".into()),
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 005：`Span<T>.Empty` / `ReadOnlySpan<T>.Empty`（须在 check receiver 前拦截）。
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
                                let elem = self.lower_type(&type_args[0].node)?;
                                return Ok(TypedExpr {
                                    ty: TypeId::Span {
                                        elem: Box::new(elem),
                                        mutable: name.as_str() == "Span",
                                    },
                                    expr: expr.clone(),
                                    linq_path: None,
                                    expression_tree: None,
                                });
                            }
                        }
                    }
                }
                // RFC 004 M2：泛型 variant 无 payload 构造 `Option<int>.None`。
                // receiver 为 `Call { Ident, [], type_args }`（与 MethodCall 路径对称）。
                if let Expr::Call {
                    func,
                    args: ref ca,
                    type_args,
                    ..
                } = &receiver.node
                {
                    if ca.is_empty() && !type_args.is_empty() {
                        if let Expr::Ident(name) = &func.node {
                            if self.registry.is_variant(name)
                                && self.registry.is_generic_template(name)
                            {
                                let arg_tys: Vec<TypeId> = type_args
                                    .iter()
                                    .filter_map(|t| self.lower_type(&t.node).ok())
                                    .collect();
                                let mangled =
                                    crate::generics::mangle_generic(name.as_str(), &arg_tys);
                                if !self.registry.is_variant(&mangled.as_str().into()) {
                                    if let Some(tmpl) = self.registry.types.get(name).cloned() {
                                        let map: IndexMap<Ident, TypeId> = tmpl
                                            .generic_params
                                            .iter()
                                            .zip(arg_tys.iter())
                                            .map(|(p, t)| (p.clone(), t.clone()))
                                            .collect();
                                        let inst_cases: Vec<_> = tmpl
                                            .variants
                                            .iter()
                                            .map(|c| {
                                                let mut copy = c.clone();
                                                if let Some(ref p) = c.payload {
                                                    copy.payload =
                                                        Some(substitute_type_name(p, &map));
                                                }
                                                copy
                                            })
                                            .collect();
                                        let inst = crate::oop_types::NominalType {
                                            name: mangled.as_str().into(),
                                            kind: crate::oop_types::TypeKind::Variant,
                                            variants: inst_cases,
                                            ..tmpl.clone()
                                        };
                                        self.registry.types.insert(mangled.as_str().into(), inst);
                                    }
                                }
                                let mangled_ident: Ident = mangled.as_str().into();
                                if self.registry.is_variant(&mangled_ident) {
                                    let case_info = self
                                        .registry
                                        .variant_case(&mangled_ident, field)
                                        .ok_or_else(|| {
                                            TypeError::Oop(format!(
                                                "variant `{}` has no case `{}`",
                                                mangled, field
                                            ))
                                        })?;
                                    if case_info.payload.is_some() {
                                        return Err(TypeError::Oop(format!(
                                            "variant case `{}.{}` requires payload; use `{}.{}(payload)`",
                                            mangled, field, mangled, field
                                        )));
                                    }
                                    return Ok(TypedExpr {
                                        ty: TypeId::Named(mangled_ident),
                                        expr: expr.clone(),
                                        linq_path: None,
                                        expression_tree: None,
                                    });
                                }
                            }
                        }
                    }
                }
                // RFC 004 M1：`T.Prop` 形式的 static abstract 属性访问
                // （如 `T.Zero` / `T.One`）。若 receiver 是当前作用域的泛型参数
                // 且 where 约束含 static abstract 属性，走单态化分派路径。
                // codegen 拦截器对基元类型直接返回常量；用户类型发射
                // `Type_get_Prop` 静态函数符号。
                if let Some(ty) = self.check_static_abstract_field(&receiver.node, field)? {
                    return Ok(TypedExpr {
                        ty,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                // RFC 045 P1：receiver 检查可能重写 (string)obj → Expr::Unbox；
                // 以重建的 Field 幂等重检（CD-16 同款二次检查），否则 MIR 收到原始
                // Cast 折叠丢失拆箱语义——((string)box).Length 把 ArcBox 当 string
                // 直读（rt_str_length(ArcBox*)）→ 数据损坏（实测 Length=1）。
                if recv.expr != receiver.node {
                    let rebuilt = Expr::Field {
                        receiver: Box::new(Spanned::new(recv.expr, receiver.span)),
                        field: field.clone(),
                    };
                    return self.check_expr_inner(&rebuilt);
                }
                // When receiver is nullable but narrowed to non-null (via null_flow),
                // use the inner type for member access.
                let recv_ty = if recv.ty.is_nullable() {
                    if let Expr::Ident(name) = &receiver.node {
                        if !self.null_flow.as_ref().is_some_and(|f| f.is_non_null(name)) {
                            return Err(TypeError::NullableMemberAccess {
                                var: name.to_string(),
                                member: field.to_string(),
                            });
                        }
                    } else if self.null_guard_depth == 0 {
                        return Err(TypeError::Oop(format!(
                            "cannot access member `{field}` on nullable expression; use `?.` or `!.`"
                        )));
                    }
                    recv.ty
                        .nullable_inner()
                        .cloned()
                        .unwrap_or_else(|| recv.ty.clone())
                } else {
                    recv.ty.clone()
                };
                // registry 以 mangle 名存储字段/形参类型（`int[]` → `int_arr`），
                // receiver 类型经其读回时归一为结构化 TypeId，否则数组成员
                // （`arr.Length` / `arr[i]`）等结构化分支全部漏匹配。仅在此
                // 消费点归一——全局 canonical 归一会破坏 types_compatible 的
                // `Array ↔ Named("{elem}_arr")` 字符串互认（泛型 `T[]` 依赖）。
                // 排除已注册 nominal：泛型实例 mangle 同样以 `_arr` 结尾
                // （`List<string[]>` → `List_string_arr`），误归一为数组会使
                // `.Count` 等成员解析落入 `type_name_of(Array) = None` → Infer。
                // 数组 mangle 名不是 nominal 类型、不进 registry.types，可判别。
                let recv_ty = match &recv_ty {
                    TypeId::Named(n)
                        if n.as_str().ends_with("_arr") && !self.registry.types.contains_key(n) =>
                    {
                        let elem = self.canonical_type(&TypeId::Named(
                            n.as_str()[..n.as_str().len() - 4].into(),
                        ));
                        TypeId::Array {
                            elem: Box::new(elem),
                        }
                    }
                    other => other.clone(),
                };
                if recv_ty == TypeId::String && field.as_str() == "Length" {
                    return Ok(TypedExpr {
                        ty: TypeId::Int,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                if matches!(recv_ty, TypeId::Array { .. }) && field.as_str() == "Length" {
                    return Ok(TypedExpr {
                        ty: TypeId::Int,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                // RFC 005：`Span`/`ReadOnlySpan`.Length / IsEmpty
                if matches!(recv_ty, TypeId::Span { .. }) && field.as_str() == "Length" {
                    return Ok(TypedExpr {
                        ty: TypeId::Int,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                if matches!(recv_ty, TypeId::Span { .. }) && field.as_str() == "IsEmpty" {
                    return Ok(TypedExpr {
                        ty: TypeId::Bool,
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                // Task facade (RFC 009 M1): Task<T>/Task 实例属性拦截。
                // TypeId::Task 是内建类型，不走 registry；typeck 在此直接返回属性类型，
                // codegen 拦截后发射 rt_task_status/rt_task_result_*/rt_task_is_canceled ABI。
                if let TypeId::Task { inner } = &recv_ty {
                    return Ok(TypedExpr {
                        ty: match field.as_str() {
                            "Status" => TypeId::Named("TaskStatus".into()),
                            "Result" => (**inner).clone(),
                            "IsCompleted" => TypeId::Bool,
                            "IsCanceled" => TypeId::Bool,
                            "IsFaulted" => TypeId::Bool,
                            "Exception" => TypeId::Named("Exception".into()),
                            _ => {
                                return Err(TypeError::Oop(format!(
                                    "no property `{field}` on `Task<T>`"
                                )))
                            }
                        },
                        expr: expr.clone(),
                        linq_path: None,
                        expression_tree: None,
                    });
                }
                // CancellationTokenSource facade (RFC 009 M4): CTS 实例属性拦截。
                // CTS stub 在 `std/Arc/Tasks/CancellationTokenSource.as`，但 `using Arc;`
                // 不递归加载 `std/Arc/Tasks/` 子目录，导致 registry 中无 CTS 类注册。
                // typeck 在此直接返回属性类型，与 MethodCall 拦截器
                // (`check_builtin_cts_method` 的 `get_Token`/`get_IsCancellationRequested`)
                // 协同；codegen 拦截后发射 rt_cts_is_canceled / 共享指针读取 ABI。
                if let TypeId::Named(n) = &recv_ty {
                    if n.as_str() == "CancellationTokenSource" {
                        return Ok(TypedExpr {
                            ty: match field.as_str() {
                                "Token" => TypeId::Named("CancellationToken".into()),
                                "IsCancellationRequested" => TypeId::Bool,
                                _ => {
                                    return Err(TypeError::Oop(format!(
                                        "no property `{field}` on `CancellationTokenSource`"
                                    )))
                                }
                            },
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    // CancellationToken facade (RFC 009 M4): CT 实例属性拦截。
                    // CT 与 CTS 共享 RtCts* 指针，IsCancellationRequested 直接读取。
                    if n.as_str() == "CancellationToken" {
                        return Ok(TypedExpr {
                            ty: match field.as_str() {
                                "IsCancellationRequested" => TypeId::Bool,
                                "CanBeCanceled" => TypeId::Bool,
                                _ => {
                                    return Err(TypeError::Oop(format!(
                                        "no property `{field}` on `CancellationToken`"
                                    )))
                                }
                            },
                            expr: expr.clone(),
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                }
                // RFC 045（animation_callback 崩溃根因）：target-typed 赋值
                // （`Content c = Content.None;` 的 RHS 经 prepare_target_expr 包装
                // 为 `(Content)Content.None`）使 receiver 呈 Cast 形态——枚举/
                // variant case 判定须解包一层 Cast 的 Ident 内层，否则落
                // member_lookup 的 resolve_field 报「no field or property」。
                let recv_ident = match &receiver.node {
                    Expr::Ident(name) => Some(name.clone()),
                    Expr::Cast { expr, .. } => match &expr.node {
                        Expr::Ident(name) => Some(name.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let (Some(enum_name), Some(tname)) =
                    (recv_ident.as_ref(), self.type_name_of(&recv_ty))
                {
                    if self.registry.is_enum(&tname) {
                        if self.registry.enum_variant(&tname, field).is_some() {
                            return Ok(TypedExpr {
                                ty: TypeId::Named(tname),
                                expr: expr.clone(),
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        return Err(TypeError::UnknownEnumVariant {
                            enum_name: enum_name.to_string(),
                            variant: field.to_string(),
                        });
                    }
                }
                // RFC 004 M1：variant 无 payload case 构造（`Value.Null`）。
                // 有 payload case（`Value.Int(42)`）在 Expr::Call 分支处理。
                if let (Some(_), Some(tname)) = (recv_ident.as_ref(), self.type_name_of(&recv_ty)) {
                    if self.registry.is_variant(&tname) {
                        if let Some(case_info) = self.registry.variant_case(&tname, field) {
                            if case_info.payload.is_some() {
                                return Err(TypeError::Oop(format!(
                                    "variant case `{}.{}` requires payload; use `{}.{}(payload)`",
                                    tname, field, tname, field
                                )));
                            }
                            return Ok(TypedExpr {
                                ty: TypeId::Named(tname),
                                expr: expr.clone(),
                                linq_path: None,
                                expression_tree: None,
                            });
                        }
                        return Err(TypeError::Oop(format!(
                            "variant `{}` has no case `{}`",
                            tname, field
                        )));
                    }
                }
                // C# 对齐（RFC 022 Sprint 2d `Expression<TDelegate>`）：`Expression<T>`
                // 的运行时对象是 `LambdaExpression`（ExpressionTree 根节点），成员解析
                // 基于 `LambdaExpression` 类层次（`Body`/`Parameters` 在 LambdaExpression，
                // `NodeType`/`Type`/`TypeName` 继承自 Expression 基类）。`type_name_of`
                // 保持返回 "Expression" 供重载匹配，此处仅把成员查找定向到 LambdaExpression。
                let member_lookup = match &recv_ty {
                    TypeId::Expression { .. } => Some("LambdaExpression".into()),
                    _ => self.type_name_of(&recv_ty),
                };
                if let Some(tname) = member_lookup {
                    let fty = self
                        .registry
                        .resolve_field(&tname, field, &self.access_ctx());
                    match fty {
                        Ok(fty) => {
                            // RFC 006 A1：auto-property 读访问（getter）看 `get_vis`
                            // （比属性自身可见性更严格时可拦截外部读取）。
                            if let Some(finfo) = self.registry.field_info(&tname, field) {
                                if let Some(gv) = finfo.get_vis {
                                    if !self.registry.can_access(gv, &tname, &self.access_ctx()) {
                                        return Err(TypeError::Oop(format!(
                                            "getter of property `{field}` on `{tname}` is not accessible from this context"
                                        )));
                                    }
                                }
                            }
                            // RFC 044 M2：`__infer__` 哨兵字段在读取前必须已由
                            // 赋值推断回填（yield 状态机提升字段；读先于写属错误
                            // 状态机或脱糖缺陷）。
                            if fty == "__infer__" {
                                return Err(TypeError::Oop(format!(
                                    "field `{tname}.{field}` has no inferred type yet; \
                                     reads must follow an assignment (RFC 044 hoisted field)"
                                )));
                            }
                            // RFC 008 AsyncStream：字段/属性类型经 registry 以
                            // mangle 名存储（如泛型类成员 `Task<T>` → "Task_string"）；
                            // 过 canonical_type 归一化为结构化 TypeId::Task——否则
                            // `await tcs.Task` 等直接消费字段类型的下游（Await 内层
                            // 提取、泛型 inner 解构）拿到 Named("Task_string") 报
                            // "expected Task<T>, found Task_string"。
                            // 委托别名字段（`public Converter Convert;`）：registry 存
                            // 原始别名名（如 "Converter"），读取时须展开为 `Func`——否则
                            // `Named("Converter")` 非 `Func`，字段调用 `g.Convert(5)` 无
                            // Invoke 定址返回 void（"expected int, found void"）。
                            let field_tid = self
                                .registry
                                .delegate_aliases
                                .get(fty.as_str())
                                .cloned()
                                .unwrap_or_else(|| self.canonical_type(&TypeId::Named(fty)));
                            (field_tid, None, None)
                        }
                        Err(_) => {
                            // RFC 045（animation_callback 崩溃根因）：任意 receiver
                            // 形态的 variant case 引用（如 `this.Content.None`——
                            // Field 链 receiver 未走上方的 Ident/Cast 分支）——
                            // resolve_field 找不到（case 非字段），getter 兜底前补
                            // variant case 检查（无 payload case 引用）。
                            if self.registry.is_variant(&tname)
                                && self
                                    .registry
                                    .variant_case(&tname, field)
                                    .is_some_and(|c| c.payload.is_none())
                            {
                                return Ok(TypedExpr {
                                    ty: TypeId::Named(tname),
                                    expr: expr.clone(),
                                    linq_path: None,
                                    expression_tree: None,
                                });
                            }
                            let getter: Ident = format!("get_{field}").into();
                            match self
                                .registry
                                .resolve_method(&tname, &getter, &self.access_ctx())
                            {
                                Ok(sig) => (
                                    self.canonical_type(&TypeId::Named(sig.ret.clone())),
                                    None,
                                    None,
                                ),
                                Err(_) => {
                                    return Err(TypeError::Oop(format!(
                                        "no field or property `{field}` on `{tname}`"
                                    )));
                                }
                            }
                        }
                    }
                } else {
                    (TypeId::Infer, None, None)
                }
            }
            Expr::New { ty, args, obj_init } => {
                if matches!(ty.node, Type::Infer) {
                    return Err(TypeError::Oop(
                        "target-typed `new()` requires a concrete type context \
                         (e.g. `T x = new(...)`; `var x = new()` is not allowed)"
                            .into(),
                    ));
                }
                let lowered = self.lower_type(&ty.node)?;
                // RFC 012 M4-1：abstract class 不可直接实例化。
                // `GenerateToAttribute<T>` 基类标记为 abstract，强制用户派生；
                // 派生类通过调用 `: base(expr)` 复用基类构造函数。
                if let TypeId::Named(name) = &lowered {
                    if let Some(nominal) = self.registry.types.get(name) {
                        if nominal.is_abstract {
                            return Err(TypeError::Oop(format!(
                                "cannot create instance of abstract class `{name}`; derive a subclass instead"
                            )));
                        }
                    }
                }
                // RFC 007 M2：`new T(...)` 可选/命名实参绑定；RFC 006：ttnew 按槽填类型。
                // 脱糖后写入完整位置实参，MIR/codegen 按 arity 选 `__ctor::T_N`。
                let mut prepared_args = args.clone();
                if let TypeId::Named(ref name) = lowered {
                    let has_named = args
                        .iter()
                        .any(|a| matches!(&a.node, Expr::NamedArg { .. }));
                    let has_ttnew = args.iter().any(|a| contains_target_typed_new(&a.node));
                    // RFC 045（closure_method_group 崩溃根因）：实参含方法组形态
                    // （`new Thread(this.WatchExit)`）时须走绑定路径——方法组脱糖
                    // 需要目标形参类型（`bind_args_to_slots` →
                    // `maybe_coerce_method_group`），等长快速路径先 check 实参会在
                    // 方法组上失败（`no field or property WatchExit on Probe`）。
                    let has_method_group = args
                        .iter()
                        .any(|a| self.looks_like_method_group_shape(&a.node));
                    let ctors = self.registry.ctor_signatures(name);
                    // ctors 为空时：仅无参走合成默认 ctor；有实参则勿硬失败（泛型实例可能尚未填构造表）。
                    let try_bind = has_named
                        || has_ttnew
                        || has_method_group
                        || ctors.iter().any(|c| {
                            c.params.iter().any(|p| p.default.is_some())
                                || args.len() != c.param_types.len()
                        })
                        || (ctors.is_empty() && (args.is_empty() || has_named));
                    if try_bind {
                        let (_sig, bound, params_span) = self.resolve_bind_ctor(name, args)?;
                        // RFC 005：构造点无 `params_span` 字段，params 尾随实参重新
                        // 打包为 `StackSpanLit`（复用集合字面量路径），与旧行为一致。
                        prepared_args = if let Some(info) = params_span {
                            self.rewrap_params_trailing_as_stack_span(bound, &info)?
                        } else {
                            bound
                        };
                    } else if let Some(ctor) =
                        ctors.iter().find(|c| c.param_types.len() == args.len())
                    {
                        // 等长、无命名/默认：仅做 ttnew 目标类型填充（既有快速路径）。
                        let mut rewritten = Vec::with_capacity(args.len());
                        for (a, pty) in args.iter().zip(ctor.param_types.iter()) {
                            let expected = self.param_sig_type_id(pty);
                            let prepared = self.prepare_target_expr(&a.node, &expected, a.span)?;
                            rewritten.push(Spanned::new(prepared, a.span));
                        }
                        prepared_args = rewritten;
                    }
                }
                let mut first_arg_ty: Option<TypeId> = None;
                for (i, a) in prepared_args.iter().enumerate() {
                    // 绑定路径已 check；等长快速路径仍需。
                    if !matches!(&a.node, Expr::NamedArg { .. }) {
                        let typed = self.check_expr_at(a.span, &a.node)?;
                        if i == 0 {
                            first_arg_ty = Some(typed.ty);
                        }
                    }
                }
                // RFC 024 M7：`new BlockingCollection<T>(collection, capacity)` 的第一实参
                // 必须是 ConcurrentQueue/Bag/Stack 三种具体集合。该约束在 codegen 不可
                // 表达（emit 层 infallible），违规只能以 ICE panic 暴露——前移到 typeck
                // 报诊断；codegen 侧 panic 保留为内部不变量防线。
                // typeck 层类型名为裸泛型名（单态化 mangle 在 mir lower 阶段发生，
                // 此处 lowered == "BlockingCollection"），故白名单须同时接受裸名与
                // `ConcurrentQueue_` 单态化前缀两种形态。
                if prepared_args.len() == 2
                    && matches!(
                        &lowered,
                        TypeId::Named(n)
                            if n.as_str() == "BlockingCollection"
                                || n.starts_with("BlockingCollection_")
                    )
                {
                    if let Some(first_ty) = first_arg_ty {
                        let supported = match &first_ty {
                            TypeId::Named(n) => {
                                n.as_str() == "ConcurrentQueue"
                                    || n.as_str() == "ConcurrentBag"
                                    || n.as_str() == "ConcurrentStack"
                                    || n.starts_with("ConcurrentQueue_")
                                    || n.starts_with("ConcurrentBag_")
                                    || n.starts_with("ConcurrentStack_")
                            }
                            // 内部推断不确定态：防御性放行（codegen panic 兜底）。
                            TypeId::Infer | TypeId::Error => true,
                            _ => false,
                        };
                        if !supported {
                            return Err(TypeError::Oop(format!(
                                "`BlockingCollection` constructor requires the first argument \
                                 to be a `ConcurrentQueue<T>`, `ConcurrentBag<T>` or \
                                 `ConcurrentStack<T>`; found `{}`",
                                first_ty.display()
                            )));
                        }
                    }
                }
                if let Some(fields) = obj_init {
                    let TypeId::Named(ref name) = lowered else {
                        return Err(TypeError::Mismatch {
                            expected: "named type for object initializer".into(),
                            found: lowered.display(),
                        });
                    };
                    if self.registry.is_struct(name) {
                        for (_, v) in fields {
                            self.check_expr_at(v.span, &v.node)?;
                        }
                    } else if self.registry.is_class(name) {
                        for (field, v) in fields {
                            let checked = self.check_expr_at(v.span, &v.node)?;
                            // RFC 006 M5：对象初始化器可写字段或可 set/init 的属性（含自定义 init 体）。
                            if self
                                .registry
                                .resolve_field(name, field, &self.access_ctx())
                                .is_ok()
                            {
                                // RFC 006 A1：对象初始化器写属性也看 setter 可见性
                                // （private set 等更严格访问器在外部 init 中不可写）。
                                if let Some(finfo) = self.registry.field_info(name, field) {
                                    if let Some(sv) = finfo.set_vis {
                                        if !self.registry.can_access(sv, name, &self.access_ctx()) {
                                            return Err(TypeError::Oop(format!(
                                                "setter of property `{field}` on `{name}` is not accessible from this context"
                                            )));
                                        }
                                    }
                                }
                                continue;
                            }
                            let setter: Ident = format!("set_{field}").into();
                            let sig = self
                                .registry
                                .resolve_method(name, &setter, &self.access_ctx())
                                .map_err(|_| {
                                    TypeError::Oop(format!(
                                        "no field or settable property `{field}` on `{name}` in object initializer"
                                    ))
                                })?;
                            if let Some(param) = sig.params.first() {
                                let expected = TypeId::Named(param.ty.clone());
                                if !self.types_compatible(&expected, &checked.ty) {
                                    return Err(TypeError::Mismatch {
                                        expected: expected.display(),
                                        found: checked.ty.display(),
                                    });
                                }
                            }
                        }
                    } else {
                        return Err(TypeError::Oop(format!("`{name}` is not a struct or class")));
                    }
                }
                // RFC 006 M3/M4：required 须由对象初始化器或选中 ctor 体赋值（SetsRequiredMembers）。
                if let TypeId::Named(ref name) = lowered {
                    let ctor_sets = self
                        .registry
                        .types
                        .get(name)
                        .and_then(|ty| {
                            ty.constructors
                                .iter()
                                .find(|c| c.param_types.len() == prepared_args.len())
                        })
                        .map(|c| c.sets_required_members.clone())
                        .unwrap_or_default();
                    self.check_required_members(name, obj_init.as_deref(), &ctor_sets)?;
                }
                if prepared_args != *args {
                    override_expr = Some(Expr::New {
                        ty: ty.clone(),
                        args: prepared_args,
                        obj_init: obj_init.clone(),
                    });
                }
                (lowered, None, None)
            }
            Expr::CollectionExpr { elements } => {
                if elements.is_empty() {
                    (
                        TypeId::Array {
                            elem: Box::new(TypeId::Named("object".into())),
                        },
                        None,
                        None,
                    )
                } else {
                    let mut elem = TypeId::Infer;
                    for item in elements {
                        match item {
                            CollectionElement::Element(e) => {
                                let t = self.check_expr_at(e.span, &e.node)?;
                                if matches!(elem, TypeId::Infer) {
                                    elem = t.ty;
                                } else if !self.types_compatible(&elem, &t.ty) {
                                    return Err(TypeError::Mismatch {
                                        expected: elem.display(),
                                        found: t.ty.display(),
                                    });
                                }
                            }
                            CollectionElement::Spread(e) => {
                                let t = self.check_expr_at(e.span, &e.node)?;
                                let spread_elem = match &t.ty {
                                    TypeId::Array { elem: inner } => inner.as_ref().clone(),
                                    other => {
                                        return Err(TypeError::Mismatch {
                                            expected: "T[] (spread operand)".into(),
                                            found: other.display(),
                                        });
                                    }
                                };
                                if matches!(elem, TypeId::Infer) {
                                    elem = spread_elem;
                                } else if !self.types_compatible(&elem, &spread_elem) {
                                    return Err(TypeError::Mismatch {
                                        expected: format!("{}[]", elem.display()),
                                        found: t.ty.display(),
                                    });
                                }
                            }
                        }
                    }
                    (
                        TypeId::Array {
                            elem: Box::new(elem),
                        },
                        None,
                        None,
                    )
                }
            }
            Expr::NewArray { elem_type, length } => {
                // `new T[n]` — C# 数组分配。元素类型为 elem_type（不含 `[]` 后缀），
                // 长度表达式须为 int。结果类型为 `T[]`（带 RtArrayHeader 的堆数组）。
                let elem_ty = self.lower_type(&elem_type.node)?;
                let len = self.check_expr_at(length.span, &length.node)?;
                if !self.types_compatible(&TypeId::Int, &len.ty) {
                    return Err(TypeError::Mismatch {
                        expected: "int (array length)".into(),
                        found: len.ty.display(),
                    });
                }
                // 泛型方法缺陷 B 推广：长度表达式改写传播。
                if len.expr != length.node {
                    override_expr = Some(Expr::NewArray {
                        elem_type: elem_type.clone(),
                        length: Box::new(Spanned::new(len.expr, length.span)),
                    });
                }
                (
                    TypeId::Array {
                        elem: Box::new(elem_ty),
                    },
                    None,
                    None,
                )
            }
            Expr::Index { receiver, index } => {
                let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                let idx = self.check_expr_at(index.span, &index.node)?;
                // 泛型方法缺陷 B 推广：接收者/索引表达式改写传播（recv.ty / idx.ty
                // 为独立字段，部分移动后仍可读）。
                if recv.expr != receiver.node || idx.expr != index.node {
                    override_expr = Some(Expr::Index {
                        receiver: Box::new(Spanned::new(recv.expr, receiver.span)),
                        index: Box::new(Spanned::new(idx.expr, index.span)),
                    });
                }
                // Builtin `string` 只读索引：`s[i]` → `char`（UTF-8 码元，与 Length/ToCharArray 对齐）。
                if recv.ty == TypeId::String {
                    if !self.types_compatible(&TypeId::Int, &idx.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: idx.ty.display(),
                        });
                    }
                    (TypeId::Char, None, None)
                } else if let TypeId::Span { elem, .. } = &recv.ty {
                    // RFC 005：Span / ReadOnlySpan 索引读。
                    if !self.types_compatible(&TypeId::Int, &idx.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: idx.ty.display(),
                        });
                    }
                    (elem.as_ref().clone(), None, None)
                } else if let Some(tname) = self.type_name_of(&recv.ty) {
                    // C# 索引器：`obj[i]` → `get_Item`；否则数组 GEP。
                    // 将索引实参类型传入重载解析，使 `List<string>` 等索引器
                    // 正确命中 `get_Item(int)` 并返回元素类型。此前经
                    // `resolve_method` 以空实参列表解析，先打印
                    // `[OVL] fail ... args=[]` 噪声再回落到「首候选」——
                    // 既无法按索引类型区分重载，又依赖脆弱的首候选兜底。
                    let get_item: Ident = "get_Item".into();
                    let index_arg: Ident = type_id_to_field_name(&idx.ty);
                    if let Ok((_declaring, sig)) = self.registry.resolve_method_overload(
                        &tname,
                        &get_item,
                        &[index_arg],
                        &self.access_ctx(),
                    ) {
                        let ret = self.canonical_type(&TypeId::Named(sig.ret.clone()));
                        (ret, None, None)
                    } else {
                        if !self.types_compatible(&TypeId::Int, &idx.ty) {
                            return Err(TypeError::Mismatch {
                                expected: "int".into(),
                                found: idx.ty.display(),
                            });
                        }
                        let elem =
                            recv.ty
                                .enumerable_elem()
                                .ok_or_else(|| TypeError::Mismatch {
                                    expected: "array".into(),
                                    found: recv.ty.display(),
                                })?;
                        (elem, None, None)
                    }
                } else {
                    if !self.types_compatible(&TypeId::Int, &idx.ty) {
                        return Err(TypeError::Mismatch {
                            expected: "int".into(),
                            found: idx.ty.display(),
                        });
                    }
                    let elem = recv
                        .ty
                        .enumerable_elem()
                        .ok_or_else(|| TypeError::Mismatch {
                            expected: "array".into(),
                            found: recv.ty.display(),
                        })?;
                    (elem, None, None)
                }
            }
            Expr::This => {
                // RFC 006 M2：静态方法内禁止使用 `this`（无实例上下文）。
                if self.current_fn_is_static {
                    return Err(TypeError::Oop(
                        "`this` is not valid in static method context".into(),
                    ));
                }
                let cn = self
                    .current_class
                    .clone()
                    .ok_or_else(|| TypeError::Oop("`this` used outside class method".into()))?;
                (TypeId::Named(cn), None, None)
            }
            Expr::Base => {
                // RFC 006 M2：静态方法内禁止使用 `base`（无实例上下文）。
                if self.current_fn_is_static {
                    return Err(TypeError::Oop(
                        "`base` is not valid in static method context".into(),
                    ));
                }
                let cn = self
                    .current_class
                    .as_ref()
                    .ok_or_else(|| TypeError::Oop("`base` used outside class method".into()))?;
                let class_ty = self
                    .registry
                    .get(cn)
                    .ok_or_else(|| TypeError::Oop(format!("unknown class `{cn}`")))?;
                let base = class_ty
                    .bases
                    .iter()
                    .find(|b| self.registry.is_class(b))
                    .cloned()
                    .ok_or_else(|| TypeError::Oop("no base class".into()))?;
                (TypeId::Named(base), None, None)
            }
            Expr::Cast { expr: src_expr, ty } => {
                let src = self.check_expr_at(src_expr.span, &src_expr.node)?;
                let src_ty = src.ty;
                let target_ty = self.lower_type(&ty.node)?;
                // RFC 016 v2 M2 / RFC 016 M3：FFI Marshal 拆箱。
                // Cast 从 `object` 到值类型 → 转换为 `Expr::Unbox`（非用户书写），
                // 由 codegen 发射 `rt_box_unbox` ABI（含 size 校验，不匹配 panic）。
                // RFC 006 M3：Cast 从 `object` 到 `string` 同样转 `Expr::Unbox`，
                // 由 codegen 发射 `rt_string_unbox`（从 string box 提取 char*）。
                // string 非值类型（RFC 037 is_value_type 不含 string），故单独并入。
                //
                // RFC 037 M1: typeck 限制 #3 修复——使用 `is_value_type` 精确判定
                // （仅基元/struct/enum 为值类型），避免误将 `(Signal<T>)box` 转为 Unbox。
                // `Signal<T>` 是 class 引用类型，Cast 应保持指针重解释语义（零开销）。
                // 此修复使 RFC 037 零装箱依赖属性存储可行——
                // `Dictionary<long, object>` 存 `Signal<T>` 引用，GetValue 取回时仅 cast
                // 不 unbox。
                //
                // 可空剥离：`(string)obj?` 与 `(string)obj` 同走 Unbox——`object?` 是
                // Nullable(Object)，此前仅判 `TypeId::Object` 导致 `(string)object?`
                // 走裸 Cast 指针重解释（string box 未解包，首字节 \x01 即串终止符，
                // 所有 box 值比较恒相等——DI keyed KeysEqual 全灭）。unbox ABI 对
                // null 入参安全（返回 null），无需额外空判。
                let src_deref = match &src_ty {
                    TypeId::Nullable { inner } => (**inner).clone(),
                    other => other.clone(),
                };
                if src_deref == TypeId::Object
                    && (self.is_value_type(&target_ty) || target_ty == TypeId::String)
                {
                    override_expr = Some(Expr::Unbox {
                        expr: Box::new(Spanned::new(src.expr, src_expr.span)),
                        value_ty: ty.clone(),
                    });
                } else if let Expr::Cast {
                    ty: inner_ty,
                    expr: _inner_expr,
                } = &src.expr
                {
                    // RFC 045 P2：同目标嵌套 Cast 折叠——Ident 窄化重写（scop 引用
                    // 包 Cast）与 P1 receiver 重检（重建 Field 再检查）组合时，重写
                    // 后的 Cast 再检查会再套一层同目标 Cast（窄化状态持续存在），
                    // 不折叠则 recv.expr != receiver.node 恒成立 → 重检无限递归。
                    // (T)(T)x 对同目标恒等（C# 语义），折叠安全且收敛。
                    if inner_ty.node == ty.node {
                        override_expr = Some(src.expr.clone());
                    } else {
                        override_expr = Some(Expr::Cast {
                            expr: Box::new(Spanned::new(src.expr, src_expr.span)),
                            ty: ty.clone(),
                        });
                    }
                } else if src.expr != src_expr.node {
                    // 泛型方法缺陷 B 推广：Cast 子表达式被改写（如泛型方法调用注入
                    // 显式 type_args）时须传播，否则 MIR 收到未标注的模板调用。
                    override_expr = Some(Expr::Cast {
                        expr: Box::new(Spanned::new(src.expr, src_expr.span)),
                        ty: ty.clone(),
                    });
                }
                (target_ty, None, None)
            }
            // FFI Marshal 装箱节点：typeck 自动插入，结果恒为 object（FFI Marshal 根类型）。
            // value_ty 仅用于 codegen 推导 size/align，不参与表达式类型。
            Expr::Box { expr, value_ty } => {
                let inner = self.check_expr_at(expr.span, &expr.node)?;
                if inner.expr != expr.node {
                    // 泛型方法缺陷 B 推广：Box 子表达式改写传播。
                    override_expr = Some(Expr::Box {
                        expr: Box::new(Spanned::new(inner.expr, expr.span)),
                        value_ty: value_ty.clone(),
                    });
                }
                (TypeId::Object, None, None)
            }
            // FFI Marshal 拆箱节点：typeck 自动插入，结果为目标值类型。
            Expr::Unbox { expr, value_ty } => {
                let inner = self.check_expr_at(expr.span, &expr.node)?;
                if inner.expr != expr.node {
                    // 泛型方法缺陷 B 推广：Unbox 子表达式改写传播。
                    override_expr = Some(Expr::Unbox {
                        expr: Box::new(Spanned::new(inner.expr, expr.span)),
                        value_ty: value_ty.clone(),
                    });
                }
                (self.lower_type(&value_ty.node)?, None, None)
            }
            Expr::Default { ty } => (self.lower_type(&ty.node)?, None, None),
            Expr::TypeOf(ty) => {
                // RFC 018 M2 step 4: typeof(T) 表达式类型为 Type 抽象基类（公共 API 面）。
                // 运行时的具体实现是 RuntimeType（internal 子类，见 RuntimeType.as），
                // codegen 发射 RuntimeType 实例（指向 @.typeinfo.{T} 全局常量）。
                // 类型面用公共基类 `Type` 而非 internal 的 `RuntimeType`：
                //   - 与 C# 对齐（typeof 返回 System.Type）；
                //   - 跨程序集成员访问（typeof(T).TypeId / .Name / .FullName）经
                //     `Type` 上的抽象成员解析，不受 RuntimeType internal 可见性限制；
                //   - RuntimeType 保持 internal 实现细节，对用户屏蔽。
                // codegen 以 receiver_type ∈ {RuntimeType, Type} 对偶拦截同一 getter
                // （builtin_dispatch.rs），故此处改标 Type 不影响代码生成。
                self.lower_type(&ty.node)?;
                (TypeId::Named("Type".into()), None, None)
            }
            // RFC 036 M1/M2 + RFC 004 M3: `expr is pattern` — 返回 bool；绑定在外层 If/switch 脱糖。
            Expr::Is { expr, pattern } => {
                let checked = self.check_expr_at(expr.span, &expr.node)?;
                match pattern {
                    IsPattern::Type { ty, .. } => {
                        self.lower_type(&ty.node)?;
                    }
                    IsPattern::Var(_) | IsPattern::Null => {}
                    // RFC 004 常量模式：字面量类型须与 scrutinee 类型兼容。
                    IsPattern::Constant(lit) => {
                        let lit_ty = match &lit.node {
                            Expr::IntLit(_) => TypeId::Int,
                            Expr::BoolLit(_) => TypeId::Bool,
                            Expr::CharLit(_) => TypeId::Char,
                            Expr::StringLit(_) => TypeId::String,
                            _ => TypeId::Error,
                        };
                        if !self.types_compatible(&checked.ty, &lit_ty) {
                            return Err(TypeError::Mismatch {
                                expected: checked.ty.display(),
                                found: lit_ty.display(),
                            });
                        }
                    }
                    // C# 9 逻辑组合：校验绑定约束后作为布尔表达式。
                    IsPattern::And { .. } | IsPattern::Or { .. } | IsPattern::Not { .. } => {
                        self.validate_is_pattern_bindings(pattern)?;
                    }
                    IsPattern::Positional(elems) => {
                        let has_bind = elems.iter().any(|e| {
                            matches!(
                                e,
                                PositionalSubpattern::Var(_)
                                    | PositionalSubpattern::Typed { .. }
                                    | PositionalSubpattern::Const(_)
                                    | PositionalSubpattern::Nested(_)
                            )
                        });
                        if has_bind {
                            return Err(TypeError::Oop(
                                "positional pattern with bindings/constants/nested is only supported in `if` / `switch` (RFC 004 M6)"
                                    .into(),
                            ));
                        }
                        // 全弃元：class → !(e is null)；struct → true
                        let rewritten = self.positional_non_null_cond(
                            &Spanned::new(checked.expr, expr.span),
                            &checked.ty,
                            expr.span,
                        )?;
                        return self.check_expr_inner(&rewritten);
                    }
                }
                // 泛型方法缺陷 B 推广：`expr is pattern` 子表达式改写传播（Positional
                // 已整树重查返回，不达此处）。
                if checked.expr != expr.node {
                    override_expr = Some(Expr::Is {
                        expr: Box::new(Spanned::new(checked.expr, expr.span)),
                        pattern: pattern.clone(),
                    });
                }
                (TypeId::Bool, None, None)
            }
            // RFC 006 M2：`with` → New 脱糖后递归检查
            // RFC 006 M5+：clone 可读私有 backing 字段（升 current_class ≡ 合成拷贝可见性）
            Expr::With { receiver, inits } => {
                let recv = self.check_expr_at(receiver.span, &receiver.node)?;
                let desugared =
                    self.desugar_record_with(receiver, inits, &recv.ty, receiver.span)?;
                let prev_class = self.current_class.clone();
                if let TypeId::Named(ref name) = recv.ty {
                    self.current_class = Some(name.clone());
                }
                let result = self.check_expr_inner(&desugared);
                self.current_class = prev_class;
                return result;
            }
            // 赋值表达式（C# assignment）：检查复用语句级 `Stmt::Assign` 路径
            //（目标形态校验、const/readonly/init-only/setter、类型兼容 + variant
            // 构造 + 装箱单一事实源），out/null flow 窄化一并由该路径维护。
            // 表达式类型 = 目标类型（C# 语义：赋值表达式的值即写入后的 RHS）。
            Expr::Assign { target, value } => {
                let stmt = Stmt::Assign {
                    target: (**target).clone(),
                    value: (**value).clone(),
                };
                match self.check_stmt(&stmt)? {
                    TypedStmt::Assign { target, value } => {
                        let target_ty = self.check_expr_at(target.span, &target.node)?.ty;
                        return Ok(TypedExpr {
                            ty: target_ty,
                            expr: Expr::Assign {
                                target: Box::new(target),
                                value: Box::new(value),
                            },
                            linq_path: None,
                            expression_tree: None,
                        });
                    }
                    _ => unreachable!("Stmt::Assign check always yields TypedStmt::Assign"),
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                // RFC 036 M2：`if (x is T n)` → then 注入 `T n = (T)x`，条件剥绑定。
                // RFC 004 M6：常量子模式将 else 折入 then 内层 if；`else_out` 为外层 else。
                let (cond_node, then_branch, is_narrow, mut else_out) =
                    self.desugar_is_bindings(cond, then_branch, else_branch)?;
                let cond_span = cond.span;
                // RFC 006 M2：保留条件上的 AST 重写（record `==` → Equals 三元式等）
                let checked_cond = self.check_expr_at(cond_span, &cond_node)?;
                let cond = Spanned::new(checked_cond.expr, cond_span);
                let (then_nn, else_nn) = analyze_null_condition(&cond.node);
                let snap = self.out_flow.as_ref().map(|f| f.snapshot());
                let null_snap = self.null_flow.as_ref().map(|f| f.snapshot());
                if let Some(flow) = &mut self.null_flow {
                    for name in &then_nn {
                        flow.mark_non_null(name);
                    }
                    if let Some((scrut_name, narrow_ty)) = &is_narrow {
                        flow.narrow(scrut_name, narrow_ty.clone());
                        if narrow_ty.nullable_inner().is_none() {
                            flow.mark_non_null(scrut_name);
                        }
                    }
                }
                // 保存 then/else 分支的 AST 重写结果（`Expr::Unbox` 等），供 MIR
                // 重下降时保留——否则 `if (o is string) { ... (string)o ... }`
                // 内的 object→string 拆箱在 MIR 时丢失（2026-08-06 根因）。
                let then_tail = then_branch.tail.clone();
                let typed_then = self.check_block(&then_branch, &TypeId::Void)?;
                let then_branch = self.typed_block_to_block(&typed_then, then_tail);
                let then_assigned = self.out_flow.as_ref().map(|f| f.snapshot());
                let then_null = self.null_flow.as_ref().map(|f| f.snapshot());
                let then_exits = block_definitely_exits(&then_branch);
                if let (Some(flow), Some(s)) = (&mut self.out_flow, snap.clone()) {
                    flow.restore(s);
                }
                if let (Some(flow), Some(s)) = (&mut self.null_flow, null_snap.clone()) {
                    flow.restore(s);
                }
                if let Some(else_b) = &else_out {
                    if let Some(flow) = &mut self.null_flow {
                        for name in &else_nn {
                            flow.mark_non_null(name);
                        }
                    }
                    let else_tail = else_b.tail.clone();
                    let typed_else = self.check_block(else_b, &TypeId::Void)?;
                    let else_b = self.typed_block_to_block(&typed_else, else_tail);
                    let else_null = self.null_flow.as_ref().map(|f| f.snapshot());
                    let else_exits = block_definitely_exits(&else_b);
                    if let (Some(flow), Some(s)) = (&mut self.out_flow, snap) {
                        flow.restore(s);
                    }
                    if let (Some(flow), Some(then_a)) = (&mut self.out_flow, then_assigned) {
                        flow.merge_intersect(&then_a);
                    }
                    if let Some(flow) = &mut self.null_flow {
                        if then_exits && !else_exits {
                            if let Some(en) = else_null {
                                flow.restore(en);
                            }
                        } else if else_exits && !then_exits {
                            if let Some(tn) = then_null {
                                flow.restore(tn);
                            }
                        } else if !then_exits && !else_exits {
                            if let Some(tn) = then_null {
                                flow.merge_intersect(&tn);
                            }
                        }
                    }
                    else_out = Some(else_b);
                } else {
                    if let (Some(flow), Some(s)) = (&mut self.out_flow, snap) {
                        flow.restore(s);
                    }
                    if then_exits {
                        if let Some(flow) = &mut self.null_flow {
                            for name in &else_nn {
                                flow.mark_non_null(name);
                            }
                        }
                    }
                }
                override_expr = Some(Expr::If {
                    cond: Box::new(cond),
                    then_branch,
                    else_branch: else_out,
                });
                (TypeId::Void, None, None)
            }
            Expr::Switch(s) => {
                let scrut = self.check_expr_at(s.scrutinee.span, &s.scrutinee.node)?;
                let enum_name = self
                    .type_name_of(&scrut.ty)
                    .filter(|n| self.registry.is_enum(n));
                let variant_name = self
                    .type_name_of(&scrut.ty)
                    .filter(|n| self.registry.is_variant(n));
                let mut covered = std::collections::HashSet::new();
                let mut has_default = false;
                let mut rewritten_cases = Vec::with_capacity(s.cases.len());
                for case in &s.cases {
                    if let Some(pattern) = &case.pattern {
                        let (pattern, when, body) = if let Some((p, w, b)) = self
                            .rewrite_positional_switch_arm(
                                pattern,
                                &case.when,
                                &case.body,
                                &s.scrutinee,
                                &scrut.ty,
                                case.body.span(),
                            )? {
                            (p, w, b)
                        } else {
                            (pattern.clone(), case.when.clone(), case.body.clone())
                        };
                        let pat_kind =
                            self.check_match_pattern(&pattern, &scrut.ty, enum_name.as_ref())?;
                        let null_snap = self.null_flow.as_ref().map(|f| f.snapshot());
                        let mut scope_pushed = false;
                        match &pat_kind {
                            MatchPat::Variant { case: c, binding } => {
                                covered.insert(c.clone());
                                if let Some((name, payload_ty)) = binding {
                                    self.scopes
                                        .push(IndexMap::from([(name.clone(), payload_ty.clone())]));
                                    scope_pushed = true;
                                }
                            }
                            MatchPat::Wildcard => {
                                has_default = true;
                            }
                            MatchPat::Null => {}
                            MatchPat::Binding(name) => {
                                has_default = true;
                                self.scopes
                                    .push(IndexMap::from([(name.clone(), scrut.ty.clone())]));
                                scope_pushed = true;
                            }
                            MatchPat::Type { ty, binding } => {
                                if let Some(name) = binding {
                                    self.scopes
                                        .push(IndexMap::from([(name.clone(), ty.clone())]));
                                    scope_pushed = true;
                                }
                                if let Expr::Ident(scrut_name) = &s.scrutinee.node {
                                    if let Some(flow) = &mut self.null_flow {
                                        // RFC 006 M3：object→string 不做收窄（同
                                        // desugar_is_bindings；ArcStringBox 表示，见上）。
                                        if *ty != TypeId::String {
                                            flow.narrow(scrut_name, ty.clone());
                                        }
                                        if ty.nullable_inner().is_none() {
                                            flow.mark_non_null(scrut_name);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(when) = &when {
                            let wty = self.check_expr_at(when.span, &when.node)?.ty;
                            if !matches!(self.canonical_type(&wty), TypeId::Bool) {
                                return Err(TypeError::Mismatch {
                                    expected: "bool".into(),
                                    found: wty.display(),
                                });
                            }
                        }
                        // switch case 体内 `break` 合法跳出 switch（C# 语义）；
                        // 临时提升 loop_depth 使 check_stmt 的 Break 校验通过。
                        self.loop_depth += 1;
                        self.check_block(&body, &TypeId::Void)?;
                        self.loop_depth -= 1;
                        if scope_pushed {
                            self.scopes.pop();
                        }
                        if let (Some(flow), Some(s)) = (&mut self.null_flow, null_snap) {
                            flow.restore(s);
                        }
                        rewritten_cases.push(SwitchCase {
                            pattern: Some(pattern),
                            when,
                            body,
                        });
                    } else {
                        has_default = true;
                        if case.when.is_some() {
                            return Err(TypeError::Oop(
                                "`when` is not allowed on `default` switch arm".into(),
                            ));
                        }
                        // 同 pattern 分支：default 体内 `break` 合法跳出 switch。
                        self.loop_depth += 1;
                        self.check_block(&case.body, &TypeId::Void)?;
                        self.loop_depth -= 1;
                        rewritten_cases.push(case.clone());
                    }
                }
                if let Some(en) = enum_name {
                    if !has_default {
                        let missing: Vec<_> = self
                            .registry
                            .enum_variants(&en)
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|n| !covered.contains(n))
                            .map(|n| n.to_string())
                            .collect();
                        if !missing.is_empty() {
                            return Err(TypeError::NonExhaustiveMatch {
                                ty: en.to_string(),
                                missing: missing.join(", "),
                            });
                        }
                    }
                }
                if let Some(vn) = variant_name {
                    if !has_default {
                        let missing: Vec<_> = self
                            .registry
                            .variant_cases(&vn)
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|n| !covered.contains(n))
                            .map(|n| n.to_string())
                            .collect();
                        if !missing.is_empty() {
                            return Err(TypeError::NonExhaustiveMatch {
                                ty: vn.to_string(),
                                missing: missing.join(", "),
                            });
                        }
                    }
                }
                override_expr = Some(Expr::Switch(SwitchExpr {
                    scrutinee: s.scrutinee.clone(),
                    cases: rewritten_cases,
                }));
                (TypeId::Void, None, None)
            }
            Expr::SwitchForm(s) => {
                let scrut = self.check_expr_at(s.scrutinee.span, &s.scrutinee.node)?;
                let enum_name = self
                    .type_name_of(&scrut.ty)
                    .filter(|n| self.registry.is_enum(n));
                let variant_name = self
                    .type_name_of(&scrut.ty)
                    .filter(|n| self.registry.is_variant(n));
                let mut covered = std::collections::HashSet::new();
                let mut has_default = false;
                let mut result_ty: Option<TypeId> = None;
                let mut rewritten_arms = Vec::with_capacity(s.arms.len());
                for arm in &s.arms {
                    let was_positional = matches!(arm.pattern, Pattern::Positional(_));
                    let is_ref_class = match &scrut.ty {
                        TypeId::Named(n) => self.registry.is_class(n),
                        TypeId::Nullable { .. } => true,
                        _ => false,
                    };
                    let (pattern, when, body) = if let Some((p, w, b)) = self
                        .rewrite_positional_switch_expr_arm(
                            &arm.pattern,
                            &arm.when,
                            &arm.body,
                            &s.scrutinee,
                            &scrut.ty,
                            arm.body.span,
                        )? {
                        (p, w, b)
                    } else {
                        (arm.pattern.clone(), arm.when.clone(), arm.body.clone())
                    };
                    let pat_kind =
                        self.check_match_pattern(&pattern, &scrut.ty, enum_name.as_ref())?;
                    let null_snap = self.null_flow.as_ref().map(|f| f.snapshot());
                    let mut scope_pushed = false;
                    match &pat_kind {
                        MatchPat::Variant { case: c, binding } => {
                            covered.insert(c.clone());
                            if let Some((name, payload_ty)) = binding {
                                self.scopes
                                    .push(IndexMap::from([(name.clone(), payload_ty.clone())]));
                                scope_pushed = true;
                            }
                        }
                        MatchPat::Wildcard => {
                            // RFC 004 M4：class 位置模式脱糖为 Wildcard+when，不计入穷尽默认臂
                            if !was_positional || !is_ref_class {
                                has_default = true;
                            }
                        }
                        MatchPat::Null => {}
                        MatchPat::Binding(name) => {
                            if !was_positional || !is_ref_class {
                                has_default = true;
                            }
                            self.scopes
                                .push(IndexMap::from([(name.clone(), scrut.ty.clone())]));
                            scope_pushed = true;
                        }
                        MatchPat::Type { ty, binding } => {
                            if let Some(name) = binding {
                                self.scopes
                                    .push(IndexMap::from([(name.clone(), ty.clone())]));
                                scope_pushed = true;
                            }
                            if let Expr::Ident(scrut_name) = &s.scrutinee.node {
                                if let Some(flow) = &mut self.null_flow {
                                    // RFC 006 M3：object→string 不做收窄（ArcStringBox 表示）。
                                    if *ty != TypeId::String {
                                        flow.narrow(scrut_name, ty.clone());
                                    }
                                    if ty.nullable_inner().is_none() {
                                        flow.mark_non_null(scrut_name);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(when) = &when {
                        let wty = self.check_expr_at(when.span, &when.node)?.ty;
                        if !matches!(self.canonical_type(&wty), TypeId::Bool) {
                            return Err(TypeError::Mismatch {
                                expected: "bool".into(),
                                found: wty.display(),
                            });
                        }
                    }
                    let body_ty = self.check_expr_at(body.span, &body.node)?.ty;
                    match &result_ty {
                        None => result_ty = Some(body_ty),
                        Some(expected) => {
                            if !self.types_compatible(expected, &body_ty) {
                                return Err(TypeError::Mismatch {
                                    expected: expected.display(),
                                    found: body_ty.display(),
                                });
                            }
                        }
                    }
                    if scope_pushed {
                        self.scopes.pop();
                    }
                    if let (Some(flow), Some(snap)) = (&mut self.null_flow, null_snap) {
                        flow.restore(snap);
                    }
                    rewritten_arms.push(SwitchExprArm {
                        pattern,
                        when,
                        body,
                    });
                }
                if let Some(ref en) = enum_name {
                    if !has_default {
                        let missing: Vec<_> = self
                            .registry
                            .enum_variants(en)
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|n| !covered.contains(n))
                            .map(|n| n.to_string())
                            .collect();
                        if !missing.is_empty() {
                            return Err(TypeError::NonExhaustiveMatch {
                                ty: en.to_string(),
                                missing: missing.join(", "),
                            });
                        }
                    }
                }
                if let Some(ref vn) = variant_name {
                    if !has_default {
                        let missing: Vec<_> = self
                            .registry
                            .variant_cases(vn)
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|n| !covered.contains(n))
                            .map(|n| n.to_string())
                            .collect();
                        if !missing.is_empty() {
                            return Err(TypeError::NonExhaustiveMatch {
                                ty: vn.to_string(),
                                missing: missing.join(", "),
                            });
                        }
                    }
                }
                if !has_default && enum_name.is_none() && variant_name.is_none() {
                    return Err(TypeError::Oop(
                        "non-exhaustive switch expression: add `_ => ...` arm".into(),
                    ));
                }
                override_expr = Some(Expr::SwitchForm(SwitchExprForm {
                    scrutinee: s.scrutinee.clone(),
                    arms: rewritten_arms,
                }));
                let ty = result_ty.unwrap_or(TypeId::Void);
                (ty, None, None)
            }
            Expr::Block(b) => {
                // 表达式块：语句副作用后取 tail 类型（RFC 004 M4 位置模式脱糖体）。
                self.scopes.push(IndexMap::new());
                let block_result = (|| -> Result<TypeId, TypeError> {
                    for stmt in &b.stmts {
                        match &stmt.node {
                            Stmt::DeconstructAssign {
                                declare,
                                targets,
                                value,
                            } => {
                                let _ = self.check_deconstruct_assign(
                                    *declare, targets, value, stmt.span,
                                )?;
                            }
                            Stmt::Lock { expr, body } => {
                                let _ = self.check_lock_stmt(expr, body, stmt.span)?;
                            }
                            other => {
                                let _ = self.check_stmt(other)?;
                            }
                        }
                    }
                    if let Some(tail) = &b.tail {
                        Ok(self.check_expr_at(tail.span, &tail.node)?.ty)
                    } else {
                        Ok(TypeId::Void)
                    }
                })();
                self.scopes.pop();
                (block_result?, None, None)
            }
            Expr::RefArg { is_out, expr } => {
                let inner = self.check_expr_at(expr.span, &expr.node)?;
                let ty = inner.ty;
                // C#：将 `out` 参数再作为 `out` 实参传递视为已定值。
                if *is_out {
                    if let (Some(flow), Expr::Ident(name)) = (&mut self.out_flow, &inner.expr) {
                        flow.mark_assigned(name);
                    }
                }
                // 泛型方法缺陷 B 推广：ref/out 实参改写传播。
                if inner.expr != expr.node {
                    override_expr = Some(Expr::RefArg {
                        is_out: *is_out,
                        expr: Box::new(Spanned::new(inner.expr, expr.span)),
                    });
                }
                (ty, None, None)
            }
            Expr::NamedArg { .. } => {
                return Err(TypeError::Oop(
                    "named arguments are only valid in call argument lists".into(),
                ));
            }
            // RFC 005：params / `[…]`→Span 脱糖产物；元素已检查，整式类型为 Span/ROS。
            Expr::StackSpanLit {
                elements,
                mutable,
                elem,
            } => {
                for e in elements {
                    let et = self.check_expr_at(e.span, &e.node)?.ty;
                    if !self.types_compatible(elem, &et) {
                        return Err(TypeError::Mismatch {
                            expected: elem.display(),
                            found: et.display(),
                        });
                    }
                }
                (
                    TypeId::Span {
                        elem: Box::new(elem.clone()),
                        mutable: *mutable,
                    },
                    None,
                    None,
                )
            }
            Expr::Null => (
                TypeId::Nullable {
                    inner: Box::new(TypeId::Infer),
                },
                None,
                None,
            ),
            Expr::Coalesce { left, right } => {
                let lt = self.check_expr_at(left.span, &left.node)?;
                let rt = self.check_expr_at(right.span, &right.node)?;
                let inner = match lt.ty.nullable_inner().cloned() {
                    Some(inner) => inner,
                    None => {
                        return Err(TypeError::Oop(format!(
                            "`??` requires nullable left side, found `{}`",
                            lt.ty.display()
                        )))
                    }
                };
                if !self.types_compatible(&inner, &rt.ty) {
                    return Err(TypeError::Mismatch {
                        expected: inner.display(),
                        found: rt.ty.display(),
                    });
                }
                // 泛型方法缺陷 B 推广：`??` 两侧改写传播。
                if lt.expr != left.node || rt.expr != right.node {
                    override_expr = Some(Expr::Coalesce {
                        left: Box::new(Spanned::new(lt.expr, left.span)),
                        right: Box::new(Spanned::new(rt.expr, right.span)),
                    });
                }
                (self.canonical_type(&inner), None, None)
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let ct = self.check_expr_at(cond.span, &cond.node)?;
                if ct.ty != TypeId::Bool {
                    return Err(TypeError::Mismatch {
                        expected: "bool".into(),
                        found: ct.ty.display(),
                    });
                }
                let tt = self.check_expr_at(then_branch.span, &then_branch.node)?;
                let et = self.check_expr_at(else_branch.span, &else_branch.node)?;
                // 泛型方法缺陷 B 推广：三目各分支改写传播。
                if ct.expr != cond.node
                    || tt.expr != then_branch.node
                    || et.expr != else_branch.node
                {
                    override_expr = Some(Expr::Ternary {
                        cond: Box::new(Spanned::new(ct.expr, cond.span)),
                        then_branch: Box::new(Spanned::new(tt.expr, then_branch.span)),
                        else_branch: Box::new(Spanned::new(et.expr, else_branch.span)),
                    });
                }
                if self.types_compatible(&tt.ty, &et.ty) {
                    (self.canonical_type(&tt.ty), None, None)
                } else if self.types_compatible(&et.ty, &tt.ty) {
                    (self.canonical_type(&et.ty), None, None)
                } else {
                    return Err(TypeError::Mismatch {
                        expected: tt.ty.display(),
                        found: et.ty.display(),
                    });
                }
            }
            Expr::NullCond { access } => {
                let recv_expr = match &access.node {
                    Expr::Field { receiver, .. } => receiver,
                    Expr::MethodCall { receiver, .. } => receiver,
                    _ => {
                        return Err(TypeError::Oop(
                            "`?.` must be followed by field or method access".into(),
                        ))
                    }
                };
                let recv = self.check_expr_at(recv_expr.span, &recv_expr.node)?;
                if !recv.ty.is_nullable() {
                    return Err(TypeError::Oop(format!(
                        "`?.` requires nullable receiver, found `{}`",
                        recv.ty.display()
                    )));
                }
                let narrowed_name = if let Expr::Ident(name) = &recv_expr.node {
                    if let Some(flow) = &mut self.null_flow {
                        flow.mark_non_null(name);
                    }
                    Some(name.clone())
                } else {
                    None
                };
                self.null_guard_depth += 1;
                let access_checked = self.check_expr_at(access.span, &access.node);
                self.null_guard_depth -= 1;
                let access_ty = self.canonical_type(&access_checked?.ty);
                if let Some(name) = &narrowed_name {
                    if let Some(flow) = &mut self.null_flow {
                        flow.un_narrow(name);
                    }
                }
                // RFC 009 L2：`?.` 的结果类型始终为 `T?`（无论 T 是引用类型还是值类型）。
                // - 引用类型 `string`：`s?.ToString()` → `string?`（null = null ptr）
                // - 值类型 `int`：`s?.Length` → `int?`（null = null ptr；有值 = boxed int ptr）
                // 若 access_ty 已经是 nullable（如 `s?.Child?.Name`），不再二次包装。
                if !access_ty.is_nullable() {
                    (
                        TypeId::Nullable {
                            inner: Box::new(access_ty),
                        },
                        None,
                        None,
                    )
                } else {
                    (access_ty, None, None)
                }
            }
            Expr::ForceDeref { access } => {
                let recv_expr = match &access.node {
                    Expr::Field { receiver, .. } => receiver,
                    Expr::MethodCall { receiver, .. } => receiver,
                    _ => {
                        return Err(TypeError::Oop(
                            "`!.` must be followed by field or method access".into(),
                        ))
                    }
                };
                let recv = self.check_expr_at(recv_expr.span, &recv_expr.node)?;
                if !recv.ty.is_nullable() {
                    return Err(TypeError::Oop(format!(
                        "`!.` requires nullable receiver, found `{}`",
                        recv.ty.display()
                    )));
                }
                let narrowed_name = if let Expr::Ident(name) = &recv_expr.node {
                    if let Some(flow) = &mut self.null_flow {
                        flow.mark_non_null(name);
                    }
                    Some(name.clone())
                } else {
                    None
                };
                self.null_guard_depth += 1;
                let access_checked = self.check_expr_at(access.span, &access.node);
                self.null_guard_depth -= 1;
                let access_ty = self.canonical_type(&access_checked?.ty);
                if let Some(name) = &narrowed_name {
                    if let Some(flow) = &mut self.null_flow {
                        flow.un_narrow(name);
                    }
                }
                (access_ty, None, None)
            }
            Expr::Unary { op, expr } => {
                let inner = self.check_expr_at(expr.span, &expr.node)?;
                let inner_canon = self.canonical_type(&inner.ty);
                let ty = match op {
                    UnaryOp::Not => {
                        if inner_canon != TypeId::Bool {
                            return Err(TypeError::Mismatch {
                                expected: "bool".into(),
                                found: inner.ty.display(),
                            });
                        }
                        TypeId::Bool
                    }
                    UnaryOp::Neg => {
                        if let Some(desugared) = self.desugar_user_unary_neg(expr, &inner.ty) {
                            return self.check_expr_inner(&desugared);
                        }
                        if !is_arithmetic_numeric(&inner_canon) {
                            return Err(TypeError::Mismatch {
                                expected: "numeric or user operator -".into(),
                                found: inner.ty.display(),
                            });
                        }
                        inner_canon
                    }
                    UnaryOp::BitNot => {
                        // 枚举位取反（RFC 004 枚举能力增强）：`~E` → E。
                        if !is_arithmetic_numeric(&inner_canon) && !self.is_enum_type(&inner_canon)
                        {
                            return Err(TypeError::Mismatch {
                                expected: "numeric (integer) or enum operand for ~".into(),
                                found: inner.ty.display(),
                            });
                        }
                        inner_canon
                    }
                };
                // 保留内层 AST 重写（如 record `==` → Equals），否则 `!(a == b)` 仍降低为原始 Binary。
                override_expr = Some(Expr::Unary {
                    op: *op,
                    expr: Box::new(Spanned::new(inner.expr, expr.span)),
                });
                (ty, None, None)
            }
        };
        let final_expr = override_expr.unwrap_or_else(|| expr.clone());
        Ok(TypedExpr {
            ty,
            expr: final_expr,
            linq_path,
            expression_tree,
        })
    }

    /// CD-8：短路布尔运算符（`&&`/`||`）右操作数的可空收窄（对齐 C#/Roslyn
    /// NullableWalker 的 two-state 条件流：右操作数以「左操作数的分支状态」为
    /// 入口分析）。
    ///
    /// - `a || b`：`b` 仅在 `a` 为假时求值 → 注入「`a` 为假 ⇒ 变量非空」集合
    ///   （`x == null` 为假 ⇒ `x` 非空）后再检查 `b`；
    /// - `a && b`：`b` 仅在 `a` 为真时求值 → 注入「`a` 为真 ⇒ 变量非空」集合
    ///   （`x != null` 为真 ⇒ `x` 非空）后再检查 `b`。
    ///
    /// 检查后恢复快照：`a || b` 整体之后变量仍可能为空（左真短路路径），收窄
    /// 不得泄漏到后续表达式——分支注入由 `Expr::If` 的 `analyze_null_condition`
    /// 负责。非短路运算符（`&`/`|`）不适用（Roslyn 同样不支持，见
    /// dotnet/roslyn#53255）。
    fn check_short_circuit_right(
        &mut self,
        op: BinOp,
        right: &Spanned<Expr>,
        left_ty: &TypedExpr,
    ) -> Result<TypedExpr, TypeError> {
        let injected = match op {
            BinOp::Or => analyze_null_condition(&left_ty.expr).1,
            BinOp::And => analyze_null_condition(&left_ty.expr).0,
            _ => return self.check_expr_at(right.span, &right.node),
        };
        if injected.is_empty() {
            return self.check_expr_at(right.span, &right.node);
        }
        let snap = self.null_flow.as_ref().map(|f| f.snapshot());
        if let Some(flow) = &mut self.null_flow {
            for name in &injected {
                flow.mark_non_null(name);
            }
        }
        let result = self.check_expr_at(right.span, &right.node);
        if let (Some(flow), Some(s)) = (&mut self.null_flow, snap) {
            flow.restore(s);
        }
        result
    }

    /// RFC 036 M2：将 `if (e is T n)` / `if (e is var n)` 脱糖为 then 块内 `let`。
    ///
    /// 返回 `(剥绑定后的条件, 注入 let 的 then 块, 可选的 scrutinee 窄化, 外层 else)`。
    /// RFC 004 M6：常量子模式时将原 else 折入 then 内层 `if (guards)`，外层 else 仍保留
    ///（覆盖 null / 非匹配路径）。
    fn desugar_is_bindings(
        &mut self,
        cond: &Spanned<Expr>,
        then_branch: &Block,
        else_branch: &Option<Block>,
    ) -> Result<(Expr, Block, Option<(Ident, TypeId)>, Option<Block>), TypeError> {
        let Expr::Is { expr, pattern } = &cond.node else {
            return Ok((
                cond.node.clone(),
                then_branch.clone(),
                None,
                else_branch.clone(),
            ));
        };
        let scrut_name = match &expr.node {
            Expr::Ident(n) => Some(n.clone()),
            _ => None,
        };
        match pattern {
            IsPattern::Type {
                ty,
                binding: Some(name),
            } => {
                let pat_ty = self.lower_type(&ty.node)?;
                let cast = Expr::Cast {
                    expr: expr.clone(),
                    ty: ty.clone(),
                };
                let let_stmt = Spanned::new(
                    Stmt::Let {
                        mutable: false,
                        name: name.clone(),
                        ty: Some(ty.clone()),
                        init: Some(Spanned::new(cast, cond.span)),
                    },
                    cond.span,
                );
                let mut stmts = vec![let_stmt];
                stmts.extend(then_branch.stmts.clone());
                let then = Block {
                    stmts,
                    tail: then_branch.tail.clone(),
                };
                let new_cond = Expr::Is {
                    expr: expr.clone(),
                    pattern: IsPattern::Type {
                        ty: ty.clone(),
                        binding: None,
                    },
                };
                // RFC 045 P2：string 亦收窄（C# 语义 is string 后 scrut 按 string
                // 使用）。运行时 string box 的拆箱由 Ident 窄化重写产生的 Cast
                // (object→string) 承担：typeck 经 Cast 分支转 Expr::Unbox、MIR 折叠
                // 兜底发射 rt_string_unbox（见 Cast 分支注释与 lower_expr）。
                let narrow = scrut_name.map(|n| (n, pat_ty));
                Ok((new_cond, then, narrow, else_branch.clone()))
            }
            IsPattern::Type { ty, binding: None } => {
                let pat_ty = self.lower_type(&ty.node)?;
                // RFC 045 P2：string 亦收窄。运行时值仍是 ArcStringBox（object 槽
                // 语义不变）；Ident 窄化重写把 scrut 使用点包为 Cast(object→string)，
                // typeck 转 Expr::Unbox / MIR 折叠兜底发射 rt_string_unbox 真正解箱，
                // 成员访问 o.Length 与显式 (string)o 同语义（见 Ident 分支）。
                let narrow = scrut_name.map(|n| (n, pat_ty));
                Ok((
                    cond.node.clone(),
                    then_branch.clone(),
                    narrow,
                    else_branch.clone(),
                ))
            }
            IsPattern::Var(name) => {
                let let_stmt = Spanned::new(
                    Stmt::Let {
                        mutable: false,
                        name: name.clone(),
                        ty: None,
                        init: Some((**expr).clone()),
                    },
                    cond.span,
                );
                let mut stmts = vec![let_stmt];
                stmts.extend(then_branch.stmts.clone());
                let then = Block {
                    stmts,
                    tail: then_branch.tail.clone(),
                };
                let new_cond = Expr::Is {
                    expr: expr.clone(),
                    pattern: IsPattern::Var(name.clone()),
                };
                Ok((new_cond, then, None, else_branch.clone()))
            }
            IsPattern::Null => {
                if let Some(n) = &scrut_name {
                    let _ = n;
                }
                Ok((
                    cond.node.clone(),
                    then_branch.clone(),
                    None,
                    else_branch.clone(),
                ))
            }
            // RFC 004 常量模式：无绑定、不收窄，条件原样保留（值相等由 MIR 降）。
            IsPattern::Constant(_) => Ok((
                cond.node.clone(),
                then_branch.clone(),
                None,
                else_branch.clone(),
            )),
            IsPattern::Positional(elems) => {
                let checked = self.check_expr_at(expr.span, &expr.node)?;
                let value = Spanned::new(checked.expr.clone(), expr.span);
                let (expand, guard) =
                    self.expand_positional_pattern(elems, &value, cond.span, false)?;
                let mut stmts = expand;
                let then = if let Some(g) = guard {
                    let inner_else = else_branch.clone();
                    let inner_if = Spanned::new(
                        Expr::If {
                            cond: Box::new(g),
                            then_branch: then_branch.clone(),
                            else_branch: inner_else,
                        },
                        cond.span,
                    );
                    stmts.push(Spanned::new(Stmt::Expr(inner_if), cond.span));
                    Block { stmts, tail: None }
                } else {
                    stmts.extend(then_branch.stmts.clone());
                    Block {
                        stmts,
                        tail: then_branch.tail.clone(),
                    }
                };
                let new_cond = self.positional_non_null_cond(
                    &Spanned::new(checked.expr, expr.span),
                    &checked.ty,
                    cond.span,
                )?;
                Ok((new_cond, then, None, else_branch.clone()))
            }
            // C# 9 逻辑组合：`or` / `not` 内禁止绑定（已由 validate 拒绝），条件原样保留。
            IsPattern::Or { .. } | IsPattern::Not { .. } => Ok((
                cond.node.clone(),
                then_branch.clone(),
                None,
                else_branch.clone(),
            )),
            // C# 9 `and`：允许绑定。将 and 树内的声明绑定注入 then 分支并剥除
            // Type 绑定（保留纯类型测试）；var 模式恒真、保留即可（其绑定另行注入）。
            IsPattern::And { .. } => {
                let bindings = collect_is_and_bindings(pattern);
                if bindings.is_empty() {
                    return Ok((
                        cond.node.clone(),
                        then_branch.clone(),
                        None,
                        else_branch.clone(),
                    ));
                }
                let scrut_name = match &expr.node {
                    Expr::Ident(n) => Some(n.clone()),
                    _ => None,
                };
                let mut stmts = Vec::new();
                let mut narrow = None;
                for (name, ty, is_var) in bindings {
                    if is_var {
                        stmts.push(Spanned::new(
                            Stmt::Let {
                                mutable: false,
                                name: name.clone(),
                                ty: None,
                                init: Some((**expr).clone()),
                            },
                            cond.span,
                        ));
                    } else if let Some(ty) = ty {
                        let cast = Expr::Cast {
                            expr: expr.clone(),
                            ty: ty.clone(),
                        };
                        stmts.push(Spanned::new(
                            Stmt::Let {
                                mutable: false,
                                name: name.clone(),
                                ty: Some(ty.clone()),
                                init: Some(Spanned::new(cast, cond.span)),
                            },
                            cond.span,
                        ));
                        // 取首个类型绑定用于收窄（与单模式行为一致）
                        if narrow.is_none() {
                            if let Ok(pat_ty) = self.lower_type(&ty.node) {
                                if pat_ty != TypeId::String {
                                    narrow = scrut_name.clone().map(|n| (n, pat_ty));
                                }
                            }
                        }
                    }
                }
                stmts.extend(then_branch.stmts.clone());
                let then = Block {
                    stmts,
                    tail: then_branch.tail.clone(),
                };
                let new_cond = Expr::Is {
                    expr: expr.clone(),
                    pattern: strip_is_and_bindings(pattern.clone()),
                };
                Ok((new_cond, then, narrow, else_branch.clone()))
            }
        }
    }

    /// C# 9 逻辑组合绑定约束校验。
    ///
    /// - `not` / `or` 内部**禁止**声明绑定（`T name`）与 `var x` 绑定——只允许
    ///   纯类型（`binding: None`）/ Null / 位置（无绑定）子模式。
    /// - `and` 允许绑定（typeck 后续在 if 收窄时取首个/一致性处理）。
    fn validate_is_pattern_bindings(&self, pattern: &IsPattern) -> Result<(), TypeError> {
        self.validate_is_pattern_bindings_inner(pattern, false)
    }

    fn validate_is_pattern_bindings_inner(
        &self,
        pattern: &IsPattern,
        forbid_binding: bool,
    ) -> Result<(), TypeError> {
        match pattern {
            IsPattern::Type {
                binding: Some(_), ..
            }
            | IsPattern::Var(_) => {
                if forbid_binding {
                    return Err(TypeError::Oop(
                        "binding is not allowed inside `not`/`or` pattern combinators (C# 9 rule); \
                         only pure type / null / positional (binding-free) subpatterns are permitted"
                            .into(),
                    ));
                }
                Ok(())
            }
            IsPattern::Type { binding: None, .. } | IsPattern::Null | IsPattern::Constant(_) => {
                Ok(())
            }
            IsPattern::Positional(elems) => {
                if forbid_binding
                    && elems.iter().any(|e| {
                        matches!(
                            e,
                            PositionalSubpattern::Var(_) | PositionalSubpattern::Typed { .. }
                        )
                    })
                {
                    return Err(TypeError::Oop(
                        "binding is not allowed inside `not`/`or` pattern combinators (C# 9 rule); \
                         only pure type / null / positional (binding-free) subpatterns are permitted"
                            .into(),
                    ));
                }
                Ok(())
            }
            IsPattern::And { left, right } => {
                self.validate_is_pattern_bindings_inner(&left.node, forbid_binding)?;
                self.validate_is_pattern_bindings_inner(&right.node, forbid_binding)
            }
            IsPattern::Or { left, right } => {
                // `or` 内部禁止一切绑定（即使嵌套在 and 中）
                self.validate_is_pattern_bindings_inner(&left.node, true)?;
                self.validate_is_pattern_bindings_inner(&right.node, true)
            }
            IsPattern::Not { inner } => self.validate_is_pattern_bindings_inner(&inner.node, true),
        }
    }

    /// RFC 004 M3：位置模式匹配守卫——class 为 `!(e is null)`，struct/值类型为 `true`。
    fn positional_non_null_cond(
        &self,
        expr: &Spanned<Expr>,
        ty: &TypeId,
        span: Span,
    ) -> Result<Expr, TypeError> {
        let is_ref_class = match ty {
            TypeId::Named(n) => self.registry.is_class(n),
            TypeId::Nullable { .. } => true,
            _ => false,
        };
        if is_ref_class {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(Spanned::new(
                    Expr::Is {
                        expr: Box::new(expr.clone()),
                        pattern: IsPattern::Null,
                    },
                    span,
                )),
            })
        } else {
            Ok(Expr::BoolLit(true))
        }
    }

    /// RFC 004 M3：将 `case (var x, …)` 改写为既有 Wildcard/Var + Let/Deconstruct MethodCall。
    /// RFC 004 M6：常量子模式 → when 内二次 Deconstruct + 相等守卫（臂体再绑一次）。
    fn rewrite_positional_switch_arm(
        &mut self,
        pattern: &Pattern,
        when: &Option<Spanned<Expr>>,
        body: &Block,
        scrutinee: &Spanned<Expr>,
        scrut_ty: &TypeId,
        span: Span,
    ) -> Result<Option<(Pattern, Option<Spanned<Expr>>, Block)>, TypeError> {
        let Pattern::Positional(elems) = pattern else {
            return Ok(None);
        };
        let (new_pat, value_expr, combined_when) =
            self.positional_switch_prelude(elems, when, scrutinee, scrut_ty, span)?;

        let scope_pushed = matches!(&value_expr.node, Expr::Ident(_));
        if scope_pushed {
            if let Expr::Ident(tmp) = &value_expr.node {
                self.scopes
                    .push(IndexMap::from([(tmp.clone(), scrut_ty.clone())]));
            }
        }
        let (expand, _) = self.expand_positional_pattern(elems, &value_expr, span, false)?;
        if scope_pushed {
            self.scopes.pop();
        }
        let mut stmts = expand;
        stmts.extend(body.stmts.clone());
        let new_body = Block {
            stmts,
            tail: body.tail.clone(),
        };
        Ok(Some((new_pat, combined_when, new_body)))
    }

    /// RFC 004 M4：switch 表达式位置模式 → Wildcard/Var + when + `Block{ Lets; Deconstruct; tail }`。
    fn rewrite_positional_switch_expr_arm(
        &mut self,
        pattern: &Pattern,
        when: &Option<Spanned<Expr>>,
        body: &Spanned<Expr>,
        scrutinee: &Spanned<Expr>,
        scrut_ty: &TypeId,
        span: Span,
    ) -> Result<Option<(Pattern, Option<Spanned<Expr>>, Spanned<Expr>)>, TypeError> {
        let Pattern::Positional(elems) = pattern else {
            return Ok(None);
        };
        let (new_pat, value_expr, combined_when) =
            self.positional_switch_prelude(elems, when, scrutinee, scrut_ty, span)?;

        let scope_pushed = matches!(&value_expr.node, Expr::Ident(_));
        if scope_pushed {
            if let Expr::Ident(tmp) = &value_expr.node {
                self.scopes
                    .push(IndexMap::from([(tmp.clone(), scrut_ty.clone())]));
            }
        }
        let (expand, _) = self.expand_positional_pattern(elems, &value_expr, span, false)?;
        if scope_pushed {
            self.scopes.pop();
        }
        let new_body = Spanned::new(
            Expr::Block(Block {
                stmts: expand,
                tail: Some(Box::new(body.clone())),
            }),
            span,
        );
        Ok(Some((new_pat, combined_when, new_body)))
    }

    /// 位置模式 switch 公共前奏：scrut 绑定 / null when / 常量匹配 when。
    fn positional_switch_prelude(
        &mut self,
        elems: &[PositionalSubpattern],
        when: &Option<Spanned<Expr>>,
        scrutinee: &Spanned<Expr>,
        scrut_ty: &TypeId,
        span: Span,
    ) -> Result<(Pattern, Spanned<Expr>, Option<Spanned<Expr>>), TypeError> {
        let is_ref_class = match scrut_ty {
            TypeId::Named(n) => self.registry.is_class(n),
            TypeId::Nullable { .. } => true,
            _ => false,
        };
        let has_const = Self::positional_has_const(elems);
        let (new_pat, value_expr, null_when) = if is_ref_class {
            // 临时名序号按编译单元隔离（非进程全局）：并行编译各成员互不干扰。
            self.pos_scrut_seq += 1;
            let seq = self.pos_scrut_seq;
            let tmp: Ident = format!("__pos_scrut_{}_{}", span.start, seq).into();
            let null_guard = Spanned::new(
                Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(Spanned::new(
                        Expr::Is {
                            expr: Box::new(Spanned::new(Expr::Ident(tmp.clone()), span)),
                            pattern: IsPattern::Null,
                        },
                        span,
                    )),
                },
                span,
            );
            (
                Pattern::Var(tmp.clone()),
                Spanned::new(Expr::Ident(tmp), span),
                Some(null_guard),
            )
        } else {
            (Pattern::Wildcard, scrutinee.clone(), None)
        };

        let mut combined_when = match (null_when, when) {
            (None, w) => w.clone(),
            (Some(n), None) => Some(n),
            (Some(n), Some(w)) => Some(Spanned::new(
                Expr::Binary {
                    op: BinOp::And,
                    left: Box::new(n),
                    right: Box::new(w.clone()),
                },
                span,
            )),
        };

        if has_const {
            let scope_pushed = matches!(&value_expr.node, Expr::Ident(_));
            if scope_pushed {
                if let Expr::Ident(tmp) = &value_expr.node {
                    self.scopes
                        .push(IndexMap::from([(tmp.clone(), scrut_ty.clone())]));
                }
            }
            let (guard_stmts, guard) =
                self.expand_positional_pattern(elems, &value_expr, span, true)?;
            if scope_pushed {
                self.scopes.pop();
            }
            let Some(g) = guard else {
                return Err(TypeError::Oop(
                    "internal: const positional pattern produced no guard".into(),
                ));
            };
            let match_block = Spanned::new(
                Expr::Block(Block {
                    stmts: guard_stmts,
                    tail: Some(Box::new(g)),
                }),
                span,
            );
            combined_when = Some(match combined_when {
                None => match_block,
                Some(w) => Spanned::new(
                    Expr::Binary {
                        op: BinOp::And,
                        left: Box::new(w),
                        right: Box::new(match_block),
                    },
                    span,
                ),
            });
        }

        Ok((new_pat, value_expr, combined_when))
    }

    fn positional_has_const(elems: &[PositionalSubpattern]) -> bool {
        elems.iter().any(|e| match e {
            PositionalSubpattern::Const(_) => true,
            PositionalSubpattern::Nested(inner) => Self::positional_has_const(inner),
            _ => false,
        })
    }

    /// RFC 004 M6：展开位置模式为 `Let` + `Deconstruct`（可嵌套）+ 可选常量相等守卫。
    ///
    /// `temps_only`：全部槽位用临时名（供 switch `when` 匹配侧二次展开）。
    fn expand_positional_pattern(
        &mut self,
        elems: &[PositionalSubpattern],
        value: &Spanned<Expr>,
        span: Span,
        temps_only: bool,
    ) -> Result<(Vec<Spanned<Stmt>>, Option<Spanned<Expr>>), TypeError> {
        if elems.len() < 2 {
            return Err(TypeError::Oop(
                "positional pattern requires at least two subpatterns".into(),
            ));
        }
        let checked_value = self.check_expr_at(value.span, &value.node)?;
        let tname = self.type_name_of(&checked_value.ty).ok_or_else(|| {
            TypeError::Oop(format!(
                "cannot deconstruct value of type `{}`",
                checked_value.ty.display()
            ))
        })?;
        let method: Ident = "Deconstruct".into();
        let candidates = self
            .registry
            .collect_method_overloads(&tname, &method, &self.access_ctx())
            .map_err(|e| TypeError::Oop(e.to_string()))?;
        let matching: Vec<_> = candidates
            .iter()
            .filter(|(_, sig)| {
                sig.params.len() == elems.len()
                    && sig.params.iter().all(|p| p.is_out)
                    && !sig.params.iter().any(|p| p.is_ref || p.is_in)
            })
            .collect();
        let (_declaring, sig) = match matching.len() {
            1 => matching[0],
            0 => {
                return Err(TypeError::Oop(format!(
                    "no matching `Deconstruct` with {} out parameter(s) on `{}`",
                    elems.len(),
                    tname
                )));
            }
            _ => {
                return Err(TypeError::Oop(format!(
                    "ambiguous `Deconstruct` overload on `{}`",
                    tname
                )));
            }
        };

        let mut out_stmts = Vec::new();
        let mut args: Vec<Spanned<Expr>> = Vec::with_capacity(elems.len());
        let mut guards: Vec<Spanned<Expr>> = Vec::new();
        let mut nested_work: Vec<(Ident, Vec<PositionalSubpattern>, TypeId)> = Vec::new();
        let mut discard_i = 0u32;

        for (elem, param) in elems.iter().zip(sig.params.iter()) {
            let expected = self.param_sig_type_id(&param.ty);
            let fresh_temp = |i: &mut u32| -> Ident {
                let name: Ident =
                    format!("__pos_{}_{}_{}", span.start, *i, temps_only as u8).into();
                *i += 1;
                name
            };
            let (bind_name, const_lit, nested) = match elem {
                PositionalSubpattern::Discard => (fresh_temp(&mut discard_i), None, None),
                PositionalSubpattern::Var(n) if !temps_only => (n.clone(), None, None),
                PositionalSubpattern::Var(_) => (fresh_temp(&mut discard_i), None, None),
                PositionalSubpattern::Typed { ty, name } => {
                    let declared = self.lower_type(&ty.node)?;
                    if !self.types_compatible(&expected, &declared)
                        || !self.types_compatible(&declared, &expected)
                    {
                        return Err(TypeError::Mismatch {
                            expected: format!(
                                "positional type `{}` (Deconstruct out)",
                                expected.display()
                            ),
                            found: declared.display(),
                        });
                    }
                    if temps_only {
                        (fresh_temp(&mut discard_i), None, None)
                    } else {
                        (name.clone(), None, None)
                    }
                }
                PositionalSubpattern::Const(lit) => {
                    let lit_ty = self.check_expr_at(lit.span, &lit.node)?.ty;
                    if !self.types_compatible(&expected, &lit_ty) {
                        return Err(TypeError::Mismatch {
                            expected: format!(
                                "positional constant type `{}` (Deconstruct out)",
                                expected.display()
                            ),
                            found: lit_ty.display(),
                        });
                    }
                    (fresh_temp(&mut discard_i), Some(lit.clone()), None)
                }
                PositionalSubpattern::Nested(inner) => {
                    let tmp = fresh_temp(&mut discard_i);
                    (
                        tmp.clone(),
                        None,
                        Some((tmp, inner.clone(), expected.clone())),
                    )
                }
            };

            let ty_ast = crate::generics::type_id_to_ast(&expected);
            out_stmts.push(Spanned::new(
                Stmt::Let {
                    mutable: false,
                    name: bind_name.clone(),
                    ty: Some(Spanned::new(ty_ast, span)),
                    init: None,
                },
                span,
            ));
            if let Some(lit) = const_lit {
                guards.push(Spanned::new(
                    Expr::Binary {
                        op: BinOp::Eq,
                        left: Box::new(Spanned::new(Expr::Ident(bind_name.clone()), span)),
                        right: Box::new(lit),
                    },
                    span,
                ));
            }
            if let Some(n) = nested {
                nested_work.push(n);
            }
            args.push(Spanned::new(
                Expr::RefArg {
                    is_out: true,
                    expr: Box::new(Spanned::new(Expr::Ident(bind_name), span)),
                },
                span,
            ));
        }

        let call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(checked_value.expr, value.span)),
                method,
                args,
                type_args: Vec::new(),
                params_span: None,
            },
            span,
        );
        out_stmts.push(Spanned::new(Stmt::Expr(call), span));

        for (tmp, inner, nested_ty) in nested_work {
            self.scopes.push(IndexMap::from([(tmp.clone(), nested_ty)]));
            let nested_val = Spanned::new(Expr::Ident(tmp), span);
            let nested_result =
                self.expand_positional_pattern(&inner, &nested_val, span, temps_only);
            self.scopes.pop();
            let (nested_stmts, nested_guard) = nested_result?;
            out_stmts.extend(nested_stmts);
            if let Some(g) = nested_guard {
                guards.push(g);
            }
        }

        let guard = guards.into_iter().reduce(|a, b| {
            Spanned::new(
                Expr::Binary {
                    op: BinOp::And,
                    left: Box::new(a),
                    right: Box::new(b),
                },
                span,
            )
        });
        Ok((out_stmts, guard))
    }

    /// RFC 006 M3/M4：`required` 须由对象初始化器或选中 ctor 体赋值（SetsRequiredMembers 等价）。
    fn check_required_members(
        &self,
        type_name: &Ident,
        obj_init: Option<&[(Ident, Spanned<Expr>)]>,
        ctor_sets: &indexmap::IndexSet<Ident>,
    ) -> Result<(), TypeError> {
        let Some(ty) = self.registry.types.get(type_name) else {
            return Ok(());
        };
        if ty.required_props.is_empty() {
            return Ok(());
        }
        let mut set: indexmap::IndexSet<_> = obj_init
            .unwrap_or(&[])
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        for n in ctor_sets {
            set.insert(n.clone());
        }
        let missing: Vec<_> = ty
            .required_props
            .iter()
            .filter(|p| !set.contains(*p))
            .map(|p| p.as_str().to_string())
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        Err(TypeError::Oop(format!(
            "missing required member(s) on `{type_name}`: {}; set them in the object initializer or a constructor that assigns them (RFC 006 M4)",
            missing.join(", ")
        )))
    }

    /// RFC 007：将插值 parts 脱糖为 StringBuilder 路径。
    ///
    /// 旧实现为 `StringLit` / `T.ToString(x[,fmt])` / `PadLeft|PadRight` / `+` 链——
    /// 每次 `+` 拼接都整串拷贝，N 段为 O(n²)。现改为
    /// `new StringBuilder()` → `.Append(piece)…` → `.ToString()`：分段追加摊还 O(n)。
    ///
    /// 每段 piece 已由 `desugar_interp_hole` 规范为 `string`（含格式/对齐），统一走
    /// `Append(string)`；stub facade 方法签名在 registry 中注册，MIR / codegen 沿既有
    /// `new StringBuilder()` / `sb.Append` 路径分发。零洞（纯字面量）直接折叠为常量串，
    /// 不引入构造开销。
    fn desugar_interpolated_string(
        &mut self,
        parts: &[ast::InterpPart],
    ) -> Result<Expr, TypeError> {
        use ast::InterpPart;
        let mut pieces: Vec<Expr> = Vec::with_capacity(parts.len().max(1));
        for part in parts {
            match part {
                InterpPart::Lit(s) => pieces.push(Expr::StringLit(s.clone())),
                InterpPart::Expr(hole) => {
                    pieces.push(self.desugar_interp_hole(hole)?);
                }
            }
        }
        // 单段：直接返回该段（零洞时为常量串，避免无谓的 StringBuilder 构造）。
        if pieces.len() == 1 {
            return Ok(pieces.pop().unwrap());
        }
        // 多段：`new StringBuilder().Append(p0).Append(p1)…ToString()`。
        let mut cur = Expr::New {
            ty: ast::Type::named("StringBuilder"),
            args: vec![],
            obj_init: None,
        };
        for piece in pieces {
            cur = Expr::MethodCall {
                receiver: Box::new(Spanned::new(cur, Span::DUMMY)),
                method: "Append".into(),
                args: vec![Spanned::new(piece, Span::DUMMY)],
                type_args: vec![],
                params_span: None,
            };
        }
        Ok(Expr::MethodCall {
            receiver: Box::new(Spanned::new(cur, Span::DUMMY)),
            method: "ToString".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        })
    }

    /// RFC 007 M2a：单洞 → 可选格式化 ToString + 可选对齐 Pad。
    fn desugar_interp_hole(&mut self, hole: &ast::InterpHole) -> Result<Expr, TypeError> {
        let te = self.check_expr_at(hole.expr.span, &hole.expr.node)?;
        let canon = self.canonical_type(&te.ty);
        let span = hole.expr.span;

        if let Some(fmt) = &hole.format {
            validate_interp_format(fmt, &canon)?;
        }

        let mut piece = if let Some(fmt) = &hole.format {
            if is_datetime_type(&canon) {
                // 日期模式 → 实例 `dt.ToString(fmt)`（std DateTime 诚实子集）。
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(te.expr, span)),
                    method: "ToString".into(),
                    args: vec![Spanned::new(Expr::StringLit(fmt.clone()), Span::DUMMY)],
                    type_args: vec![],
                    params_span: None,
                }
            } else {
                let Some(prim) = primitive_tostring_type_name(&canon) else {
                    return Err(TypeError::Mismatch {
                        expected: "numeric primitive or DateTime for interpolation format specifier (RFC 007)"
                            .into(),
                        found: canon.display(),
                    });
                };
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident(prim.into()), Span::DUMMY)),
                    method: "ToString".into(),
                    args: vec![
                        Spanned::new(te.expr, span),
                        Spanned::new(Expr::StringLit(fmt.clone()), Span::DUMMY),
                    ],
                    type_args: vec![],
                    params_span: None,
                }
            }
        } else if canon == TypeId::String {
            te.expr
        } else if let Some(prim) = primitive_tostring_type_name(&canon) {
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::Ident(prim.into()), Span::DUMMY)),
                method: "ToString".into(),
                args: vec![Spanned::new(te.expr, span)],
                type_args: vec![],
                params_span: None,
            }
        } else {
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(te.expr, span)),
                method: "ToString".into(),
                args: vec![],
                type_args: vec![],
                params_span: None,
            }
        };

        if let Some(align) = hole.alignment {
            if align != 0 {
                let width = align.unsigned_abs() as i64;
                let method = if align > 0 { "PadLeft" } else { "PadRight" };
                piece = Expr::MethodCall {
                    receiver: Box::new(Spanned::new(piece, Span::DUMMY)),
                    method: method.into(),
                    args: vec![Spanned::new(Expr::IntLit(width), Span::DUMMY)],
                    type_args: vec![],
                    params_span: None,
                };
            }
        }

        Ok(piece)
    }
}

/// RFC 007 M2a–M2g：校验格式说明符。
///
/// - 标准：`D`/`X`/`F`/`G`/`N`/`C`/`E`/`P`（大小写见下）+ 可选十进制精度
/// - M2c–M2g 自定义数值子集：`0`/`#`、分组 `,`、缩放逗号、`;` 节、后缀 `%`、前缀/后缀/占位间引号/`\` 字面量
/// - 日期诚实子集（仅 `DateTime`）：`yyyy`/`yy`/`MMMM`/`MMM`/`MM`/`M`/`dddd`/`ddd`/`dd`/`HH`/`hh`/`mm`/`ss`/`fff`/`tt`/`zzz` + 字面分隔符
/// - `FormattableString` / 文化感知 → **立宪硬拒绝**（RFC 006）
/// - `D`/`X`：仅整数族；其余标准与自定义：整数族 + `float`/`double`
/// - `bool`/`char`/引用类型：拒绝格式说明符
fn validate_interp_format(fmt: &str, ty: &TypeId) -> Result<(), TypeError> {
    let bytes = fmt.as_bytes();
    if bytes.is_empty() {
        return Err(TypeError::Mismatch {
            expected: "non-empty format specifier (D/X/F/G/N/C/E/P or custom 0/#/,/%/;/'/\\)"
                .into(),
            found: "empty".into(),
        });
    }
    let is_int = matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::UInt
            | TypeId::ULong
            | TypeId::UShort
            | TypeId::SByte
    );
    let is_float = matches!(ty, TypeId::Float | TypeId::Double);

    // 日期诚实子集（仅 DateTime）——须在自定义数值之前。
    if is_simple_date_format(fmt) {
        if !is_datetime_type(ty) {
            return Err(TypeError::Mismatch {
                expected: "DateTime for date format pattern (RFC 007)".into(),
                found: ty.display(),
            });
        }
        return Ok(());
    }

    // M2c–M2f：自定义数值（先于字母路径；可含 `#` / `,` / `%` / `;` / 引号字面量）
    if is_custom_numeric_format(fmt) {
        if !is_int && !is_float {
            return Err(TypeError::Mismatch {
                expected: "numeric type for custom numeric format (RFC 007)".into(),
                found: ty.display(),
            });
        }
        return Ok(());
    }

    let spec = bytes[0] as char;
    let precision = &fmt[1..];
    if !precision.is_empty() && !precision.chars().all(|c| c.is_ascii_digit()) {
        return Err(TypeError::Mismatch {
            expected: "standard D5/N2/E/P, custom 0/#/,/%/;/'/\\, or DateTime yyyy/yy/MMMM/MMM/MM/M/dddd/ddd/dd/HH/hh/mm/ss/fff/tt/zzz (RFC 007); FormattableString/culture rejected (RFC 016)"
                .into(),
            found: fmt.into(),
        });
    }
    if !precision.is_empty() && precision.parse::<u32>().is_err() {
        return Err(TypeError::Mismatch {
            expected: "format precision fitting u32".into(),
            found: precision.into(),
        });
    }
    match spec {
        'D' | 'd' | 'X' | 'x' => {
            if !is_int {
                return Err(TypeError::Mismatch {
                    expected: "integer type for D/X format".into(),
                    found: ty.display(),
                });
            }
        }
        'F' | 'f' | 'G' | 'g' | 'N' | 'n' | 'C' | 'c' | 'E' | 'e' | 'P' | 'p' => {
            if !is_int && !is_float {
                return Err(TypeError::Mismatch {
                    expected: "numeric type for F/G/N/C/E/P format".into(),
                    found: ty.display(),
                });
            }
        }
        _ => {
            return Err(TypeError::Mismatch {
                expected: "standard D/X/F/G/N/C/E/P, custom 0/#/,/%/;/'/\\, or DateTime yyyy/yy/MMMM/MMM/MM/M/dddd/ddd/dd/HH/hh/mm/ss/fff/tt/zzz (RFC 007); FormattableString/culture rejected (RFC 016)"
                    .into(),
                found: fmt.into(),
            });
        }
    }
    Ok(())
}

/// RFC 007 M2c–M2g：解析自定义数值（含前缀/后缀/整数段占位间字面量）；各节独立 `sec(;sec){0,2}`。
fn is_datetime_type(ty: &TypeId) -> bool {
    matches!(ty, TypeId::Named(n) if n.as_str() == "DateTime")
}

/// RFC 007 日期诚实子集：`yyyy`/`yy`/`MMMM`/`MMM`/`MM`/`M`/`dddd`/`ddd`/`dd`/
/// `HH`/`hh`/`mm`/`ss`/`fff`/`tt`/`zzz` + 字面分隔符。
/// 不含单字符 `d`（避免与数值标准说明符 `d`/`D` 冲突）。
fn is_simple_date_format(fmt: &str) -> bool {
    if fmt.is_empty() {
        return false;
    }
    let b = fmt.as_bytes();
    let mut i = 0;
    let mut saw_token = false;
    while i < b.len() {
        if b[i..].starts_with(b"yyyy") || b[i..].starts_with(b"MMMM") || b[i..].starts_with(b"dddd")
        {
            i += 4;
            saw_token = true;
        } else if b[i..].starts_with(b"MMM")
            || b[i..].starts_with(b"ddd")
            || b[i..].starts_with(b"fff")
            || b[i..].starts_with(b"zzz")
        {
            i += 3;
            saw_token = true;
        } else if b[i..].starts_with(b"yy")
            || b[i..].starts_with(b"MM")
            || b[i..].starts_with(b"dd")
            || b[i..].starts_with(b"HH")
            || b[i..].starts_with(b"hh")
            || b[i..].starts_with(b"mm")
            || b[i..].starts_with(b"ss")
            || b[i..].starts_with(b"tt")
        {
            i += 2;
            saw_token = true;
        } else if b[i] == b'M' {
            i += 1;
            saw_token = true;
        } else if matches!(b[i], b'-' | b':' | b'/' | b' ' | b'.' | b'T') {
            i += 1;
        } else {
            return false;
        }
    }
    saw_token
}

fn is_custom_numeric_format(fmt: &str) -> bool {
    if fmt.is_empty() {
        return false;
    }
    if fmt.bytes().any(|b| b == b'"') {
        return false;
    }
    let sections = split_custom_sections(fmt);
    if sections.len() > 3 {
        return false;
    }
    sections
        .iter()
        .all(|sec| sec.is_empty() || is_custom_numeric_section(sec))
}

/// 按 `;` 分割，忽略引号内与 `\` 转义后的分号。
fn split_custom_sections(fmt: &str) -> Vec<&str> {
    let b = fmt.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_quote = false;
    while i < b.len() {
        if in_quote {
            if b[i] == b'\'' {
                if i + 1 < b.len() && b[i + 1] == b'\'' {
                    i += 2;
                } else {
                    in_quote = false;
                    i += 1;
                }
            } else {
                i += 1;
            }
            continue;
        }
        if b[i] == b'\\' {
            i = (i + 2).min(b.len());
            continue;
        }
        if b[i] == b'\'' {
            in_quote = true;
            i += 1;
            continue;
        }
        if b[i] == b';' {
            out.push(std::str::from_utf8(&b[start..i]).unwrap_or(""));
            start = i + 1;
        }
        i += 1;
    }
    out.push(std::str::from_utf8(&b[start..]).unwrap_or(""));
    out
}

/// 单节自定义数值模式（无顶层 `;`）。允许前缀/后缀 `'…'` / `\c`，以及整数段占位间字面量（M2g）。
fn is_custom_numeric_section(fmt: &str) -> bool {
    let bytes = fmt.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut n = bytes.len();
    let mut percent = false;
    if bytes[n - 1] == b'%' {
        // 尾 `%` 是否在引号/`\` 内：与 runtime 一致，未加引号才剥
        let mut in_q = false;
        let mut pct_lit = false;
        let mut t = 0usize;
        while t < n {
            if in_q {
                if bytes[t] == b'\'' {
                    if t + 1 < n && bytes[t + 1] == b'\'' {
                        t += 2;
                    } else {
                        in_q = false;
                        t += 1;
                    }
                } else {
                    t += 1;
                }
                if t == n {
                    pct_lit = true;
                }
                continue;
            }
            if bytes[t] == b'\\' {
                if t + 1 < n {
                    t += 1;
                }
                t += 1;
                if t == n {
                    pct_lit = true;
                }
                continue;
            }
            if bytes[t] == b'\'' {
                in_q = true;
            }
            t += 1;
        }
        if !pct_lit && !in_q {
            percent = true;
            n -= 1;
            if n == 0 {
                return false;
            }
        }
    }
    let _ = percent;

    let body = &bytes[..n];
    let mut i = 0usize;
    let mut pending_lit = false;
    let mut seen_digit = false;
    let mut in_frac = false;
    let mut need_digit = false;
    let mut scale_done = false;
    let mut int_len = 0usize;
    let mut frac_len = 0usize;

    while i < body.len() {
        match body[i] {
            b'\'' => {
                i += 1;
                let mut closed = false;
                while i < body.len() {
                    if body[i] == b'\'' {
                        if i + 1 < body.len() && body[i + 1] == b'\'' {
                            i += 2;
                        } else {
                            i += 1;
                            closed = true;
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                if !closed {
                    return false;
                }
                pending_lit = true;
            }
            b'\\' => {
                if i + 1 >= body.len() {
                    return false;
                }
                i += 2;
                pending_lit = true;
            }
            b'0' | b'#' => {
                if scale_done {
                    return false;
                }
                if pending_lit && seen_digit {
                    /* M2g：整数段占位间字面量；M2i：小数段占位间字面量 */
                }
                pending_lit = false;
                if in_frac {
                    frac_len += 1;
                } else {
                    int_len += 1;
                }
                seen_digit = true;
                need_digit = false;
                i += 1;
            }
            b',' => {
                if in_frac || !seen_digit || need_digit || pending_lit || scale_done {
                    return false;
                }
                let mut j = i;
                while j < body.len() && body[j] == b',' {
                    j += 1;
                }
                let next = body.get(j).copied();
                if j == body.len() || matches!(next, Some(b'.' | b'\'' | b'\\')) {
                    scale_done = true;
                    i = j;
                    need_digit = false;
                } else if matches!(next, Some(b'0' | b'#')) {
                    need_digit = true;
                    i += 1;
                } else {
                    return false;
                }
            }
            b'.' => {
                if in_frac || !seen_digit || need_digit || pending_lit {
                    return false;
                }
                in_frac = true;
                scale_done = false;
                need_digit = true;
                i += 1;
            }
            _ => return false,
        }
    }
    if !seen_digit || need_digit {
        return false;
    }
    if in_frac && frac_len == 0 {
        return false;
    }
    if int_len == 0 {
        return false;
    }
    true
}

/// RFC 007：基元类型名（用于静态 `T.ToString(x)` 脱糖）。
fn primitive_tostring_type_name(ty: &TypeId) -> Option<&'static str> {
    match ty {
        TypeId::Int => Some("int"),
        TypeId::Long => Some("long"),
        TypeId::Short => Some("short"),
        TypeId::Byte => Some("byte"),
        TypeId::Char => Some("char"),
        TypeId::Float => Some("float"),
        TypeId::Double => Some("double"),
        TypeId::Bool => Some("bool"),
        TypeId::UInt => Some("uint"),
        TypeId::ULong => Some("ulong"),
        TypeId::UShort => Some("ushort"),
        TypeId::SByte => Some("sbyte"),
        _ => None,
    }
}

fn is_arithmetic_numeric(ty: &TypeId) -> bool {
    matches!(
        ty,
        TypeId::Int
            | TypeId::Long
            | TypeId::Short
            | TypeId::Byte
            | TypeId::Char
            | TypeId::Float
            | TypeId::Double
    )
}

/// Numeric promotion for arithmetic binary ops (Add/Sub/Mul/Div/Mod on numerics).
/// Mirrors C# semantics: if either operand is `double` → `double`; else if either is
/// `float` → `float`; else if either is `long` → `long`; else `int` (short/byte promote
/// to int for arithmetic, matching C#).
fn numeric_promote(left: &TypeId, right: &TypeId) -> TypeId {
    if *left == TypeId::Double || *right == TypeId::Double {
        TypeId::Double
    } else if *left == TypeId::Float || *right == TypeId::Float {
        TypeId::Float
    } else if *left == TypeId::Long || *right == TypeId::Long {
        TypeId::Long
    } else {
        TypeId::Int
    }
}

fn analyze_null_condition(cond: &Expr) -> (Vec<Ident>, Vec<Ident>) {
    match cond {
        Expr::Binary {
            op: BinOp::NotEq,
            left,
            right,
        } => {
            let name = extract_null_comparison_ident(&left.node, &right.node);
            match name {
                Some(n) => (vec![n], vec![]),
                None => (vec![], vec![]),
            }
        }
        Expr::Binary {
            op: BinOp::Eq,
            left,
            right,
        } => {
            let name = extract_null_comparison_ident(&left.node, &right.node);
            match name {
                Some(n) => (vec![], vec![n]),
                None => (vec![], vec![]),
            }
        }
        Expr::Binary {
            op: BinOp::And,
            left,
            right,
        } => {
            let (l_then, _l_else) = analyze_null_condition(&left.node);
            let (r_then, _r_else) = analyze_null_condition(&right.node);
            let then: Vec<_> = l_then.into_iter().chain(r_then).collect();
            (then, vec![])
        }
        Expr::Binary {
            op: BinOp::Or,
            left,
            right,
        } => {
            let (_l_then, l_else) = analyze_null_condition(&left.node);
            let (_r_then, r_else) = analyze_null_condition(&right.node);
            let else_: Vec<_> = l_else.into_iter().chain(r_else).collect();
            (vec![], else_)
        }
        _ => (vec![], vec![]),
    }
}

fn extract_null_comparison_ident(left: &Expr, right: &Expr) -> Option<Ident> {
    match (left, right) {
        (Expr::Ident(name), Expr::Null) => Some(name.clone()),
        (Expr::Null, Expr::Ident(name)) => Some(name.clone()),
        _ => None,
    }
}

/// RFC 037 M1: 把 `Func_<p1>_<p2>_..._<ret>` 或 `Action_<p1>_<p2>_...` mangled
/// 名还原为 `TypeId::Func`。
///
/// 用 lambda 参数数量 `arity` 作为提示，决定如何分割 mangled 串：
/// - `Func_<p1>_..._<pN>_<ret>`：`arity + 1` 个 `_`-分隔组件（前 N 个是参数，最后一个是返回类型）
/// - `Action_<p1>_..._<pN>`：`arity` 个组件（全部是参数，返回类型恒为 void）
///
/// 嵌套 `Func_`/`Action_` 类型（如 `Func_object_Func_object_object_object`）
/// 由 [`demangle_func_type_depth`] 递归解析——与 `mangle_type_suffix` 互逆的
/// 单一事实源，不再回退 `None`。
///
/// **使用场景**：当 `List<Func<...>>` 被单态化为 `List_Func_<...>` 时，
/// `enumerable_elem` 返回 `Named("Func_<...>")`。对 `Add(lambda)` 而言，
/// lambda 是元素值（一个 Func 值），而非元素上的函数。需把 lambda 参数
/// 绑定到 Func 的参数类型，而非 elem 本身。
///
/// 复合类型参数（单态化泛型如 `ObservableCollection_int`，本身含 `_`）可通过
/// `is_known` 在类型注册表中识别后按组切分，而非要求每个参数恰好一个 `_`-原子。
pub fn demangle_func_type_with(
    name: &str,
    arity: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<TypeId> {
    demangle_func_type_depth(name, Some(arity), 0, is_known)
}

/// 递归 demangle：`arity=None` 服务嵌套组（元数未知），在全部可行组数上
/// 回溯，「组数恰好 + 原子用尽」的切分胜出；嵌套委托名作为子文法递归解析。
/// 深度上限防御病态输入。
pub fn demangle_func_type_depth(
    name: &str,
    arity: Option<usize>,
    depth: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<TypeId> {
    if depth > 8 {
        return None;
    }
    let (is_action, rest) = if let Some(r) = name.strip_prefix("Action_") {
        (true, r)
    } else {
        (false, name.strip_prefix("Func_")?)
    };
    if rest.is_empty() {
        return None;
    }
    let parts: Vec<&str> = rest.split('_').collect();
    let counts: Box<dyn Iterator<Item = usize>> = match arity {
        Some(a) => Box::new(std::iter::once(if is_action { a } else { a + 1 })),
        None => Box::new(1..=parts.len()),
    };
    for count in counts {
        if let Some(mut groups) = split_type_groups_typed(&parts, count, depth, is_known) {
            let ret = if is_action {
                TypeId::Void
            } else {
                groups.pop().unwrap_or(TypeId::Void)
            };
            return Some(TypeId::Func {
                params: groups,
                ret: Box::new(ret),
            });
        }
    }
    None
}

/// 把 `_`-分割片段回溯切分为 `count` 个类型组（产出已还原的 [`TypeId`]）。
/// 每组是单个原子（原语/占位符恒合法）、多原子组成的已注册类型名
/// （如 `ObservableCollection_int`），或可完整解析的嵌套委托 mangle 名
/// （如 `Func_object_object`）。`Func`/`Action` 为保留前缀原子（mangle
/// 生成端不产出裸名），排除以免嵌套名被错误截断为独立组。
fn split_type_groups_typed(
    parts: &[&str],
    count: usize,
    depth: usize,
    is_known: &dyn Fn(&str) -> bool,
) -> Option<Vec<TypeId>> {
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
    let max_atoms = parts.len() - (count - 1);
    for end in 1..=max_atoms {
        let candidate = parts[..end].join("_");
        let head = if end == 1 {
            if candidate == "Func" || candidate == "Action" {
                continue;
            }
            demangle_type_part(&candidate)
        } else if is_known(&candidate) {
            TypeId::Named(candidate.into())
        } else {
            match demangle_func_type_depth(&candidate, None, depth + 1, is_known) {
                Some(t) => t,
                None => continue,
            }
        };
        if let Some(mut rest) = split_type_groups_typed(&parts[end..], count - 1, depth, is_known) {
            rest.insert(0, head);
            return Some(rest);
        }
    }
    None
}

fn demangle_type_part(s: &str) -> TypeId {
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

fn block_definitely_exits(block: &Block) -> bool {
    block
        .stmts
        .last()
        .map(|s| matches!(s.node, Stmt::Return(_) | Stmt::Throw { .. }))
        .unwrap_or(false)
}

/// C# 9 `and` 组合：收集树内声明绑定（Type 绑定与 var 绑定）。
///
/// 每个元组 `(name, ty, is_var)`：
/// - `Type { binding: Some(name) }` → `(name, Some(ty), false)`
/// - `Var(name)` → `(name, None, true)`
///
/// `not` / `or` 内禁止绑定（已由校验阶段拒绝），此处不会遇到。
fn collect_is_and_bindings(pattern: &IsPattern) -> Vec<(Ident, Option<Spanned<Type>>, bool)> {
    match pattern {
        IsPattern::Type {
            ty,
            binding: Some(name),
        } => {
            vec![(name.clone(), Some(ty.clone()), false)]
        }
        IsPattern::Var(name) => vec![(name.clone(), None, true)],
        IsPattern::And { left, right } => {
            let mut v = collect_is_and_bindings(&left.node);
            v.extend(collect_is_and_bindings(&right.node));
            v
        }
        // 其它 primary（纯类型 / Null / 位置）与 Or / Not 均无绑定
        _ => vec![],
    }
}

/// C# 9 `and` 组合：剥除 Type 绑定（保留纯类型测试），其余结构原样保留。
/// var 模式恒真、保留即可（其绑定已由 desugar 注入 then 分支）。
fn strip_is_and_bindings(pattern: IsPattern) -> IsPattern {
    match pattern {
        IsPattern::Type {
            ty,
            binding: Some(_),
        } => IsPattern::Type { ty, binding: None },
        IsPattern::And { left, right } => IsPattern::And {
            left: Box::new(Spanned::new(strip_is_and_bindings(left.node), left.span)),
            right: Box::new(Spanned::new(strip_is_and_bindings(right.node), right.span)),
        },
        // 其余（Var / Null / Positional / Or / Not）原样保留
        other => other,
    }
}
