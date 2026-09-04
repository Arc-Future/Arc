use crate::{Block, Ident, Spanned, Type, TypeId};

/// RFC 005：调用点 `params Span<T>` / `params ReadOnlySpan<T>` 标注。
///
/// typeck 在解析调用时附着到 `Expr::Call` / `Expr::MethodCall` 节点（**纯标注**，
/// 不注入 `Expr::StackSpanLit`）；尾随可变实参保留为调用节点上的独立实参
/// `args[fixed..]`。MIR 的**单一物化点**读取本标注，把这些尾随实参收集为
/// `MirRvalue::SpanFromStack`（栈缓冲胖指针 `{ptr,len}`）。
#[derive(Clone, Debug, PartialEq)]
pub struct ParamsSpanInfo {
    /// params 槽之前的固定形参数（尾随实参自 `args[fixed..]` 收集）。
    pub fixed: usize,
    /// params 元素类型 `T`。
    pub elem: TypeId,
    /// `true` = `Span<T>`；`false` = `ReadOnlySpan<T>`。
    pub mutable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FloatLitValue {
    Float(f32),
    Double(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    IntLit(i64),
    FloatLit(FloatLitValue),
    BoolLit(bool),
    StringLit(String),
    /// RFC 012：comptime 有限子集——编译期常量求值表达式。
    /// typeck 在编译期把内部表达式折叠为常量（int/bool/string 字面量运算）；
    /// 运行期不产生任何求值（零开销）。见 `crates/typeck/src/comptime.rs`。
    Comptime(Box<Spanned<Expr>>),
    /// RFC 007：`$"...{expr}..."` 插值字符串（typeck 脱糖为 `string +` 链）。
    InterpolatedString {
        parts: Vec<InterpPart>,
    },
    CharLit(char),
    Ident(Ident),
    Path(Vec<Ident>),
    Binary {
        op: BinOp,
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
    Call {
        func: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
        type_args: Vec<Spanned<Type>>,
        /// RFC 005：末参为 `params Span<T>` 时附着；否则 `None`。
        params_span: Option<ParamsSpanInfo>,
    },
    MethodCall {
        receiver: Box<Spanned<Expr>>,
        method: Ident,
        args: Vec<Spanned<Expr>>,
        type_args: Vec<Spanned<Type>>,
        /// RFC 005：末参为 `params Span<T>` 时附着；否则 `None`。
        params_span: Option<ParamsSpanInfo>,
    },
    Field {
        receiver: Box<Spanned<Expr>>,
        field: Ident,
    },
    Index {
        receiver: Box<Spanned<Expr>>,
        index: Box<Spanned<Expr>>,
    },
    Lambda(LambdaExpr),
    ExpressionLit(ExpressionLit),
    Await(Box<Spanned<Expr>>),
    Block(Block),
    If {
        cond: Box<Spanned<Expr>>,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    Switch(SwitchExpr),
    /// RFC 036 M4：C# 8 switch 表达式 `e switch { pat => expr, ... }`（求值形式）。
    SwitchForm(SwitchExprForm),
    /// `[e1, e2, ...]` / `[..a, b]` — C# 12 collection expression (RFC 017)。
    /// 目标类型由声明上下文或元素类型推导；`..` 为 spread 元素（#8）。
    CollectionExpr {
        elements: Vec<CollectionElement>,
    },
    Cast {
        expr: Box<Spanned<Expr>>,
        ty: Spanned<Type>,
    },
    /// FFI Marshal 装箱节点（RFC 016 v2 M2 / RFC 016 M3）。
    ///
    /// 值类型 → object 引用类型的隐式转换。由 typeck 在 FFI `extern` 函数调用的
    /// `void*` 形参处自动插入（非用户书写）。通用赋值/参数/返回值装箱已永久剔除
    /// （RFC 016 v2 §6，由 RFC 004 variant 承担）。
    ///
    /// `value_ty` 为被装箱的源值类型（codegen 据此推导 size/align）。
    /// 表达式整体类型为 `object`（由 typeck 写入 TypedExpr.ty，与 value_ty 无关）。
    ///
    /// codegen 发射 `call ptr @rt_box_create(size, align)` + `@llvm.memcpy` + `@rt_arc_inc`。
    Box {
        expr: Box<Spanned<Expr>>,
        value_ty: Spanned<Type>,
    },
    /// FFI Marshal 拆箱节点（RFC 016 v2 M2 / RFC 016 M3）。
    ///
    /// object 引用类型 → 值类型的转换。由 typeck 在 FFI `extern` 函数 `void*` 返回值
    /// 处自动插入。unboxing 类型不匹配（expected_size != payload_size）触发 panic。
    ///
    /// `value_ty` 为拆箱后的目标值类型（codegen 据此推导 expected_size/out_size）。
    ///
    /// codegen 发射 `call i32 @rt_box_unbox(ptr, expected_size, out_ptr, out_size)` + size 校验。
    Unbox {
        expr: Box<Spanned<Expr>>,
        value_ty: Spanned<Type>,
    },
    /// `new T(args)` / `new T() { ... }`；RFC 006 目标类型形式下 `ty` 为 `Type::Infer`。
    New {
        ty: Spanned<Type>,
        args: Vec<Spanned<Expr>>,
        /// `new Point() { X = 1 }` — C# object initializer after constructor.
        obj_init: Option<Vec<(Ident, Spanned<Expr>)>>,
    },
    /// `new T[n]` — C# 数组分配。堆分配 n 个 T 的零初始化数组
    /// （codegen → `rt_array_create(n, elem_size)`；带 RtArrayHeader，`Length` 可读）。
    ///
    /// `elem_type` 为元素类型（不含数组后缀，如 `byte`）；`length` 为数组长度表达式
    /// （须为 `int`）。区别于 `New`（对象构造）：`new T[n]` 无参数/对象初始化器。
    NewArray {
        elem_type: Spanned<Type>,
        length: Box<Spanned<Expr>>,
    },
    This,
    Base,
    Query(QueryExpr),
    /// `ref expr` / `out expr` argument in call position only.
    RefArg {
        is_out: bool,
        expr: Box<Spanned<Expr>>,
    },
    /// RFC 007：`name: expr` 命名实参（仅调用实参位置）。
    NamedArg {
        name: Ident,
        expr: Box<Spanned<Expr>>,
    },
    /// RFC 005 params@Span / `[…]`→Span：脱糖产物（非用户书写）。
    ///
    /// 元素已按 `elem` 类型检查；codegen 发射栈上 `[N x T]` + `{ptr,len}` 胖指针，
    /// **禁止**经 `rt_array_create` 堆分配冒充零成本。
    StackSpanLit {
        elements: Vec<Spanned<Expr>>,
        /// `true` = `Span<T>`；`false` = `ReadOnlySpan<T>`。
        mutable: bool,
        elem: TypeId,
    },
    /// `cond ? then : else` — ternary conditional. cond must be `bool`.
    /// Both branches are expressions (not blocks); their types must be
    /// compatible (unified by typeck).
    Ternary {
        cond: Box<Spanned<Expr>>,
        then_branch: Box<Spanned<Expr>>,
        else_branch: Box<Spanned<Expr>>,
    },
    /// `null` literal — type is `Nullable { inner: Infer }`, resolved by context.
    Null,
    /// `left ?? right` — null-coalescing. left must be `T?`, right must be `T` or `T?`.
    Coalesce {
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    /// `receiver?.field` or `receiver?.method(args)` — null-conditional access.
    /// `access` is `Field` or `MethodCall` (receiver already set).
    NullCond {
        access: Box<Spanned<Expr>>,
    },
    /// `receiver!.field` or `receiver!.method(args)` — force dereference.
    /// `access` is `Field` or `MethodCall` (receiver already set).
    ForceDeref {
        access: Box<Spanned<Expr>>,
    },
    /// `default(T)` — type-typed default value.
    /// Numeric → 0, bool → false, reference types (string/class/interface) → null,
    /// struct → zero-initialized.
    Default {
        ty: Spanned<Type>,
    },
    /// `typeof(T)` — compile-time type identifier (RFC 023 M1).
    /// Returns a `TypeId` struct whose `Value` field is a globally unique int
    /// assigned by codegen. Enables DI container's `GetService<T>()` extension
    /// methods to produce `TypeId` without runtime reflection.
    TypeOf(Spanned<Type>),
    /// RFC 004: `expr is pattern` — 类型测试 + 可选绑定，返回 `bool`。
    ///
    /// M1 支持的 pattern 形式见 [`IsPattern`]（Type/Var/Null）。
    /// 编译期折叠（D8）由 typeck 在静态类型已知时直接产出常量；
    /// 运行时通过 `rt_obj_isa(obj, typeinfo)` 实现 class 层级测试。
    Is {
        expr: Box<Spanned<Expr>>,
        pattern: IsPattern,
    },
    /// RFC 006 M2：`recv with { Member = value, … }` — record 浅拷贝并覆盖成员。
    /// typeck 脱糖为 `new R(recv.F1, …) { … }`；MIR/codegen 不可见本节点。
    With {
        receiver: Box<Spanned<Expr>>,
        inits: Vec<(Ident, Spanned<Expr>)>,
    },
    /// 赋值表达式（`target = value`）——C# assignment 是表达式，值即写入的
    /// RHS。语句位置由 parser 提取为 `Stmt::Assign`（下游全链路不变）；
    /// 表达式位置（lambda 表达式体、三元分支、实参等）走 typeck/MIR 的
    /// 表达式链路。复合赋值（`+=` 族）与 `??=` 在 parser 就地脱糖为本变体
    /// （RFC 076 契约延续：无复合赋值 AST 变体）。
    Assign {
        target: Box<Spanned<Expr>>,
        value: Box<Spanned<Expr>>,
    },
}

/// RFC 007：插值字符串的一段——字面量或 `{expr[,align][:format]}` 洞。
#[derive(Clone, Debug, PartialEq)]
pub enum InterpPart {
    Lit(String),
    Expr(InterpHole),
}

/// RFC 007 M2a：插值洞——表达式 + 可选对齐/标准格式说明符。
#[derive(Clone, Debug, PartialEq)]
pub struct InterpHole {
    pub expr: Spanned<Expr>,
    /// `{expr,N}`：正数右对齐（左填空格），负数左对齐（右填空格）；`None` 表示无对齐。
    pub alignment: Option<i32>,
    /// `{expr:format}`：标准数字格式（如 `"D5"` / `"X"` / `"F2"` / `"G"`）；`None` 表示默认 `ToString`。
    pub format: Option<String>,
}

impl InterpHole {
    pub fn plain(expr: Spanned<Expr>) -> Self {
        Self {
            expr,
            alignment: None,
            format: None,
        }
    }
}

/// RFC 017 #8：集合表达式元素——普通元素或 `..spread`。
#[derive(Clone, Debug, PartialEq)]
pub enum CollectionElement {
    /// `e` — 单个元素。
    Element(Spanned<Expr>),
    /// `..e` — 展开 `e`（静态类型须为 `T[]`）。
    Spread(Spanned<Expr>),
}

impl CollectionElement {
    pub fn expr(&self) -> &Spanned<Expr> {
        match self {
            Self::Element(e) | Self::Spread(e) => e,
        }
    }

    pub fn expr_mut(&mut self) -> &mut Spanned<Expr> {
        match self {
            Self::Element(e) | Self::Spread(e) => e,
        }
    }

    pub fn is_spread(&self) -> bool {
        matches!(self, Self::Spread(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// `&` — 位与（int/long/byte 等整数）。
    BitAnd,
    /// `|` — 位或。
    BitOr,
    /// `^` — 位异或。
    BitXor,
    /// `<<` — 左移。
    Shl,
    /// `>>` — 右移（有符号算术右移）。
    Shr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Neg,
    /// `~` — 位取反（整数）。
    BitNot,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LambdaExpr {
    pub params: Vec<LambdaParam>,
    pub body: LambdaBody,
    pub is_expression_tree: bool,
    /// RFC 009 M6: `async () => ...` / `async (x) => ...` / `async x => ...`
    /// 异步 lambda 编译为状态机；返回类型为 `Task<T>`，T 为 body 类型。
    /// 捕获变量与跨 await 存活的 locals 一起提升为状态机 env 字段
    /// （env struct 合并 async env 与 lambda capture env，见 RFC 009 §8.3）。
    pub is_async: bool,
    /// Captured outer variables (filled by typeck).
    /// Empty = no-capture lambda (zero overhead, env = NULL).
    pub captures: Vec<LambdaCapture>,
}

/// A captured outer variable in a lambda closure (RFC 008).
#[derive(Clone, Debug, PartialEq)]
pub struct LambdaCapture {
    pub name: Ident,
    pub ty: TypeId,
    pub mode: CaptureMode,
}

/// Capture mode: class/string by-reference, value/primitive by-value (RFC 008).
#[derive(Clone, Debug, PartialEq)]
pub enum CaptureMode {
    ByRef,
    ByValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LambdaParam {
    pub name: Ident,
    pub ty: Option<Spanned<Type>>,
    /// RFC 007 M2c：可选形参默认值（编译期常量）。仅立即调用（IIFE）脱糖；
    /// 赋值给 `Func`/`Action`、作实参/返回等硬拒绝（D7：委托类型不携带默认槽，须独立 RFC）。
    pub default: Option<Spanned<Expr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LambdaBody {
    Expr(Box<Spanned<Expr>>),
    Block(Block),
}

/// Legacy AST node for expression-tree lambdas (parser no longer emits; use `Expression<T>` + `=>`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionLit {
    pub lambda: LambdaExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchExpr {
    pub scrutinee: Box<Spanned<Expr>>,
    pub cases: Vec<SwitchCase>,
}

/// RFC 036 M4：switch 表达式（与语句形式 [`SwitchExpr`] 分离：arm body 是 `Expr`）。
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchExprForm {
    pub scrutinee: Box<Spanned<Expr>>,
    pub arms: Vec<SwitchExprArm>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchExprArm {
    pub pattern: Pattern,
    pub when: Option<Spanned<Expr>>,
    pub body: Spanned<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    /// `None` = `default:` branch.
    pub pattern: Option<Pattern>,
    /// RFC 036 M2：`case pat when cond:` — 在 pattern 绑定作用域内求值的 bool 守卫。
    pub when: Option<Spanned<Expr>>,
    pub body: Block,
}

/// RFC 004 M3/M5/M6：位置模式子模式。
///
/// M3：`var name` / `_`；M5：`T name`；**M6**：常量子模式 / 嵌套位置模式。
#[derive(Clone, Debug, PartialEq)]
pub enum PositionalSubpattern {
    /// 弃元 `_`。
    Discard,
    /// `var name` — 绑定到对应 `Deconstruct` out 形参类型。
    Var(Ident),
    /// RFC 004 M5：`T name` — 类型须与对应 `Deconstruct` out 形参一致，再绑定。
    Typed { ty: Spanned<Type>, name: Ident },
    /// RFC 004 M6：常量子模式（字面量）；匹配时 `Deconstruct` out 值须 `==` 该常量。
    Const(Spanned<Expr>),
    /// RFC 004 M6：嵌套位置模式 `(…, (…), …)`；对对应 out 值再 `Deconstruct`。
    Nested(Vec<PositionalSubpattern>),
}

/// switch `case` 模式（RFC 036 M2 + RFC 004 variant + RFC 004 M3）。
///
/// 与 [`IsPattern`] 分离：`Pattern::Ident` 在 enum switch 中表示变体名；
/// 类型声明模式走 [`Pattern::Type`]（可带绑定）。
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    /// 弃元 `_`：永远匹配，无绑定。
    Wildcard,
    /// enum 变体名，或无绑定的简单类型名（typeck 再区分）。
    Ident(Ident),
    Literal(Spanned<Expr>),
    /// 类型模式：`case T:` / `case T name:`。
    Type {
        ty: Spanned<Type>,
        binding: Option<Ident>,
    },
    /// `case null:`。
    Null,
    /// `case var name:` — 永远匹配并绑定到 scrutinee 类型。
    Var(Ident),
    /// RFC 004 M1：variant case 模式——`Value.Int(n)` / `Value.Null`。
    ///
    /// `path` 为 variant 类型路径（如 `["Value"]` 或 `["Arc", "Value"]`）；
    /// `type_args` 为泛型实参（如 `Option<int>.Some(n)` 中的 `[int]`；非泛型为空）；
    /// `case` 为 case 名称（如 `Int`）；`binding` 为 payload 绑定名（`Some(n)`）
    /// 或无 payload case / 通配（`None`）。
    Variant {
        path: Vec<Ident>,
        type_args: Vec<Spanned<Type>>,
        case: Ident,
        binding: Option<Ident>,
    },
    /// RFC 004 M3：位置模式 `case (var x, var y)` / `case (_, _)`（arity ≥ 2）。
    /// typeck 脱糖为非 null 守卫 + `DeconstructAssign`；不进入 MIR 裸形式。
    Positional(Vec<PositionalSubpattern>),
}

/// RFC 036 M1: `is` 表达式的模式（与 `Pattern` 分离以避免与 switch 的
/// `Pattern::Ident`（enum variant）歧义）。
///
/// 支持形式：
/// - `expr is T` / `expr is T name` — 类型模式 + 可选声明绑定
/// - `expr is var name` — 永远匹配，绑定到原类型
/// - `expr is null` — null 测试
/// - RFC 004：`expr is <literal>` — 常量模式（`is 5` / `is "a"` / `is true` /
///   `is 'c'`），匹配语义为值相等 `==`（对齐 C# 常量模式）
/// - RFC 004 M3：`expr is (var x, var y)` / `expr is (_, _)` — 位置模式
/// - C# 9 逻辑组合：`A and B` / `A or B` / `not A`（优先级 `not` > `and` > `or`）
#[derive(Clone, Debug, PartialEq)]
pub enum IsPattern {
    /// 类型模式：`is T` 或 `is T name`（声明模式）。
    /// `binding = None` 表示纯类型测试；`Some(name)` 表示同时绑定到该名字。
    Type {
        ty: Spanned<Type>,
        binding: Option<Ident>,
    },
    /// `var` 模式：永远匹配，绑定到原类型（不进行类型测试）。
    Var(Ident),
    /// `null` 模式：测试对象是否为 null。
    Null,
    /// RFC 004 常量模式：`is 5` / `is "a"` / `is true` / `is 'c'`。
    /// 承载字面量表达式；typeck 校验字面量类型与 scrutinee 类型兼容，
    /// MIR 降为 `==` 值相等（string 走 `rt_str_equals`，数值走 `icmp`）。
    /// Box 打破 `Expr::Is { pattern: IsPattern }` ↔ `Constant(Expr)` 的递归环。
    Constant(Box<Spanned<Expr>>),
    /// RFC 004 M3：位置模式（arity ≥ 2）；typeck 脱糖。
    Positional(Vec<PositionalSubpattern>),
    /// C# 9 `and` 组合：左右两个模式都匹配。允许绑定（typeck 校验绑定名一致性）。
    And {
        left: Box<Spanned<IsPattern>>,
        right: Box<Spanned<IsPattern>>,
    },
    /// C# 9 `or` 组合：左右任一匹配。内部**禁止**声明绑定（编译期错误）。
    Or {
        left: Box<Spanned<IsPattern>>,
        right: Box<Spanned<IsPattern>>,
    },
    /// C# 9 `not` 组合：内层模式取反。内部**禁止**声明绑定（编译期错误）。
    Not { inner: Box<Spanned<IsPattern>> },
}

/// LINQ query comprehension: `from x in xs where ... select ...`
#[derive(Clone, Debug, PartialEq)]
pub struct QueryExpr {
    pub clauses: Vec<QueryClause>,
    pub select: Box<Spanned<Expr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueryClause {
    From {
        ident: Ident,
        source: Spanned<Expr>,
    },
    Let {
        ident: Ident,
        value: Spanned<Expr>,
    },
    Where(Spanned<Expr>),
    OrderBy {
        key: Spanned<Expr>,
        descending: bool,
    },
    Join {
        ident: Ident,
        source: Spanned<Expr>,
        on_left: Spanned<Expr>,
        on_right: Spanned<Expr>,
    },
    GroupBy {
        key: Spanned<Expr>,
        element: Option<Spanned<Expr>>,
        /// C# `group … by … into g`：`into` 之后 range var 由元素重绑为分组。
        /// 缺省时沿用 `from` range var（该名随后在 select 中表示分组）。
        into_ident: Option<Ident>,
    },
}
