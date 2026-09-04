//! RFC 004 §D9 / RFC 037 M2：隐式 variant 构造（typeck AST 重写）。
//!
//! 当值类型 T 与期望的 variant 类型 V 不直接兼容时，typeck 尝试将值
//! 包装为 `V.Case(value)` 形式（`Expr::MethodCall`）。这使
//! `button.Content = "Click"` 能被自动重写为
//! `button.Content = ContentVariant.Text("Click")`，保持 WPF 风格开发
//! 体验同时维持零装箱架构（variant 为栈分配标签联合，见 RFC 004）。
//!
//! ## 重写策略
//!
//! 1. 仅当期望类型 `expected_ty` 为 `TypeId::Named(V)` 且 `V` 在 registry
//!    中注册为 `TypeKind::Variant` 时尝试。
//! 2. 遍历 V 的所有 case，对有 payload 的 case 检查 `types_compatible(payload, found_ty)`。
//! 3. **歧义消解**：若恰好一个 case 匹配，返回 `V.Case(value)`；若多个
//!    case 匹配，拒绝隐式转换（要求用户显式构造），避免语义二义性。
//! 4. 零匹配时返回 `None`，调用方按原逻辑报类型不匹配错误。
//!
//! ## 与 MIR lower 的衔接
//!
//! 重写后的 `Expr::MethodCall { receiver: Expr::Ident(V), method: Case, args: [value] }`
//! 会被 `mir::lower::lower_type::variant_construct_rvalue_with_prep` 识别并发射
//! `MirRvalue::VariantConstruct`，无需 MIR 层额外修改。

use crate::checker::TypeChecker;
use crate::type_id::TypeId;
use ast::{Expr, Ident, Span, Spanned};

/// 将 payload 标识符（如 `"string"`、`"int"`、`"Element"`）映射为规范的 TypeId。
///
/// Registry 中 variant payload 存储为纯标识符（如 `"string"`），而 typeck 中
/// 字面量 `Expr::StringLit` 返回 `TypeId::String`——两者不相等。此函数将
/// 标识符解析为规范 TypeId，使 `types_compatible` 能正确匹配。
/// 框架封闭 variant 在 payload 类型歧义时的隐式构造首选 case。
///
/// 仅覆盖 RFC 037 已承诺的 UI variant；其余 variant 仍按 §D9 拒绝歧义。
fn preferred_implicit_case(
    variant_name: &Ident,
    found_ty: &TypeId,
    first_match: &Option<Ident>,
    second_match: &Ident,
) -> Option<Ident> {
    if !matches!(found_ty, TypeId::String) {
        return None;
    }
    let first = first_match.as_ref()?;
    match variant_name.as_str() {
        // Content.as 声明顺序：None, Text, Element, Binding, Resource
        "Content" if first.as_str() == "Text" && second_match.as_str() == "Resource" => {
            Some(first.clone())
        }
        _ => None,
    }
}

fn canonical_payload_type(name: &ast::Ident) -> TypeId {
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
        _ => TypeId::Named(name.clone()),
    }
}

impl TypeChecker {
    /// 尝试将值隐式包装为 variant case 构造。
    ///
    /// 入参：
    /// - `value`: 已 typeck 处理的值表达式（可能含 FFI 装箱等重写）
    /// - `found_ty`: 值的实际类型
    /// - `expected_ty`: 期望的 variant 类型
    ///
    /// 返回：
    /// - `Some(Expr)`: 重写为 `Variant.Case(value)` 的 MethodCall 表达式
    /// - `None`: 无法隐式转换（expected 非 variant / 无匹配 case / 多匹配歧义）
    pub(crate) fn coerce_to_variant(
        &self,
        value: Expr,
        found_ty: &TypeId,
        expected_ty: &TypeId,
    ) -> Option<Expr> {
        // 仅对 Named variant 类型尝试隐式构造。
        // 基元类型 / Infer / 复合类型不参与 variant 隐式构造。
        let variant_name: Ident = match expected_ty {
            TypeId::Named(n) if self.registry.is_variant(n) => n.clone(),
            _ => return None,
        };

        let cases = self.registry.variant_cases(&variant_name);
        let mut matched: Option<Ident> = None;
        for case in cases {
            // 无 payload case（如 `Value.Null`）不能通过隐式构造触发——
            // 用户应显式书写 `Value.Null`。
            let Some(payload_ident) = &case.payload else {
                continue;
            };
            // 使用规范的 TypeId 进行比较——registry 存储的是纯标识符
            // （如 `"string"`），而 typeck 字面量使用规范变体（如
            // `TypeId::String`），二者必须通过此映射统一才能匹配。
            let payload_ty = canonical_payload_type(payload_ident);
            if self.types_compatible(&payload_ty, found_ty) {
                if matched.is_some() {
                    // 框架封闭 variant 的 string 歧义消解（RFC 037 D2 / RFC 004 §D9）：
                    // `Content` 同时有 `Text of string` 与 `Resource of string`，
                    // 裸字面量语义为文本内容；资源引用须显式 `Content.Resource(key)`
                    // 或 ARML MarkupExtension（StaticResource 等）。
                    if let Some(preferred) =
                        preferred_implicit_case(&variant_name, found_ty, &matched, &case.name)
                    {
                        matched = Some(preferred);
                        break;
                    }
                    // 其他 variant：多 case 同 payload → 拒绝隐式构造
                    return None;
                }
                matched = Some(case.name.clone());
            }
        }

        matched.map(|case_name| {
            // 构造 `Variant.Case(value)` 表达式。
            // Parser 将 `Type.Case(payload)` 解析为 MethodCall，此处与之对齐，
            // 使 check_expr 的 variant 构造路径与 MIR lower 均能识别。
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::Ident(variant_name), Span::DUMMY)),
                method: case_name,
                args: vec![Spanned::new(value, Span::DUMMY)],
                type_args: vec![],
                params_span: None,
            }
        })
    }
}
