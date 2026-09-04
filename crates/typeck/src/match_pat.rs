use crate::TypeId;
use ast::Ident;

/// switch case 模式分类结果（RFC 036 M2）。
pub(crate) enum MatchPat {
    Variant {
        case: Ident,
        /// RFC 004 M1：variant case 的 payload 绑定（None = 无 payload 或 enum variant）。
        binding: Option<(Ident, TypeId)>,
    },
    /// `_` / 字面量常量模式（穷尽性上视为兜底或已覆盖常量）。
    Wildcard,
    /// `case var name:` — 永远匹配，绑定 scrutinee 类型。
    Binding(Ident),
    /// 类型模式：`case T:` / `case T name:` / 解析为类型的裸 Ident。
    Type { ty: TypeId, binding: Option<Ident> },
    /// `case null:`。
    Null,
}
