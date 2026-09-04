use ast::Span;
use thiserror::Error;

/// RFC 005 里程碑④：编译期 warning 通道（与 [`TypeError`] 语义分离）。
///
/// warning **不**阻断编译（不进入 `check_module` 的 `Err` 路径）；由 pipeline
/// 打印到 stderr。`arc-cycle-001` 为声明级字段环检测码（warning-by-default，
/// 无 error 档）。
#[derive(Debug, Clone)]
pub struct TypeWarning {
    /// 诊断码（如 `arc-cycle-001`）。
    pub code: &'static str,
    /// 人类可读消息（P3 友好措辞，不暴露借用/生命周期类术语）。
    pub message: String,
    /// 关联源码位置（类声明 span）。
    pub span: Span,
}

impl TypeWarning {
    /// 渲染为 `warning[<code>]: <message>` 单行。
    pub fn render(&self) -> String {
        format!("warning[{}]: {}", self.code, self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Error)]
pub enum TypeError {
    #[error("type mismatch: expected {expected}, found {found}")]
    Mismatch { expected: String, found: String },
    #[error("undefined name `{0}`")]
    Undefined(String),
    #[error("async fn must return Task<T>, found `{0}`")]
    AsyncReturn(String),
    #[error("`await` is only allowed inside async functions")]
    AwaitOutsideAsync,
    #[error("void function cannot return a value (found `{0}`)")]
    VoidReturnWithValue(String),
    #[error("function must return `{expected}`, bare return is invalid")]
    MissingReturnValue { expected: String },
    #[error("IQueryable requires an expression-tree lambda; use `Expression<Func<...>>` or pass `x => ...` to Where/Select on IQueryable<T>")]
    QueryableRequiresExpression,
    #[error("expression tree lambda cannot capture mutable variables")]
    ExpressionCaptureMut,
    #[error("OOP: {0}")]
    Oop(String),
    #[error("unknown enum variant `{variant}` on `{enum_name}`")]
    UnknownEnumVariant { enum_name: String, variant: String },
    #[error("generic arity mismatch on `{name}`: expected {expected}, found {found}")]
    GenericArity {
        name: String,
        expected: usize,
        found: usize,
    },
    #[error("type `{0}` requires type arguments")]
    GenericTypeNeedsArgs(String),
    #[error("generic constraint not satisfied: `{arg}` does not satisfy `{bound}`")]
    ConstraintNotSatisfied {
        param: String,
        arg: String,
        bound: String,
    },
    /// 泛型约束批量违约哨兵（DiagnosticBag 模式）。
    ///
    /// `check_constraints` 按错误恢复语义完整遍历约束表，将全部违约逐条推入
    /// 错误池（[`TypeError::ConstraintNotSatisfied`]），随后返回本哨兵沿
    /// `?` 冒泡链传播——单 `TypeError` 冒泡链保持不变，调用方以「有错误
    /// 即短路」中止当前单态化/表达式检查（违约实参不得参与单态化，否则
    /// 下游级联错误），无需感知批量结构。本哨兵为该实例化点的中止定位
    /// 信息，违约明细以逐条独立诊断呈现。
    #[error("{count} type constraint violation(s) at this instantiation site (each reported separately)")]
    ConstraintBatchViolated { count: usize },
    #[error("non-exhaustive match on `{ty}`: missing {missing}")]
    NonExhaustiveMatch { ty: String, missing: String },
    #[error("unsupported type: {0} is not yet supported")]
    Unsupported(String),
    #[error("cannot assign `null` to non-nullable type `{0}`")]
    NullToNonNullable(String),
    #[error("cannot assign nullable `{found}` to non-nullable `{expected}`; use `??`, `!.`, or null check")]
    NullableToNonNullable { expected: String, found: String },
    #[error("cannot access member `{member}` on nullable `{var}`; use `?.`, `!.`, or null check")]
    NullableMemberAccess { var: String, member: String },
    #[error("type `{0}` is not a nullable reference type; only reference types can be nullable")]
    NotNullableType(String),
    #[error("non-nullable variable `{0}` must be initialized")]
    UninitializedNonNull(String),
    /// RFC 028 M4-8 D12.4: 宏体系编译期错误（arc-macro-XXX）。
    ///
    /// `code` 是 RFC 028 D12.4 定义的错误码（如 `arc-macro-010`），
    /// `message` 是人类可读的诊断消息。由 `check_cyclic_macro_dependencies`
    /// 等宏体系校验方法产生。
    #[error("error[{code}]: {message}")]
    Macro { code: &'static str, message: String },
    #[error("undefined type parameter `{0}` in where clause")]
    UndefinedTypeParameter(String),
    #[error("invalid variance: `{param}` is `{variance}` but appears in {position} position")]
    InvalidVariance {
        param: String,
        variance: String,
        position: String,
    },
    #[error("variance modifiers (`in`/`out`) are only allowed on interface type parameters")]
    VarianceNotOnInterface,
    #[error("`break` is only allowed inside a loop")]
    BreakOutsideLoop,
    #[error("`continue` is only allowed inside a loop")]
    ContinueOutsideLoop,
    #[error("{0}")]
    Generic(String),
}
