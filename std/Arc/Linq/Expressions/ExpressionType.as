namespace Arc.Linq.Expressions;

/// 表达式节点类型枚举
public enum ExpressionType {
    // ── L1 查询子集（ORM 翻译用，纯函数式）──
    Constant,
    Parameter,
    Capture,
    Member,
    Index,
    Binary,
    Unary,
    Conditional,
    Call,
    New,
    Lambda,
    Cast,

    // ── C# per-operator 节点类型（RFC 022 Sprint 2d Slice 0 · 2026-08-02）──
    // 追加于 L1 之后（0-11 稳定），供 Binary/Unary 的 Op 字段移除后按 NodeType
    // 分派（C# System.Linq.Expressions 对齐）。当前为 Slice 0 预留；迁移完成后
    // Binary/Unary 节点以其 NodeType 标识具体运算。
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    AndAlso,
    OrElse,
    And,
    Or,
    Not,
    Negate,

    // ── L1 扩展（RFC 022 §2.2.9，RFC 032 QIF 路径触发）──
    // Class/Method 节点仅供 GenerateToAttribute<T> 派生类构造函数体编译期
    // 解释器（D10.6）使用，不进入运行时查询翻译路径，不被 codegen 发射。
    Class,
    Method,

    // ── L2 表达式扩展（RFC 022 §2.2.10，覆盖 Arc 所有表达式语法）──
    // L2 节点用于编译期扩展（D10.6 解释器/Source Generator），
    // 不进入 ORM 翻译路径——SqlTranslator 遇到 L2/L3 节点报『不可翻译』错误。
    This,               // 当前实例引用
    Base,               // 基类实例引用
    Null,               // null 字面量
    Path,               // 路径访问 A.B.C
    If,                 // if-else 表达式
    Switch,             // switch 表达式
    Coalesce,           // a ?? b 空合并
    NullConditional,    // a?.b 空条件访问
    ForceDeref,         // a!.b 强制解引用
    Is,                 // e is T 类型测试
    TypeOf,             // typeof(T) 类型标识
    Default,            // default(T) 默认值
    Await,              // await expr 异步等待
    Block,              // 语句块（含 L3 语句序列 + 可选 tail）
    Collection,         // [1,2,3] 集合表达式
    Box,                // 装箱（值类型→object）
    Unbox,              // 拆箱（object→值类型）
    Query,              // LINQ comprehension（from/where/select）

    // ── L3 语句层（RFC 022 §2.2.10，在 BlockExpression 内承载）──
    // L3 节点表示 Arc 语句，仅在 BlockExpression.Statements 中出现。
    Let,                // 局部变量声明
    Assign,             // 赋值
    Return,             // 返回
    Break,              // 循环中断
    Throw,              // 抛异常
    While,              // while 循环
    For,                // for/foreach 循环
    TryCatch,           // try-catch
    TryFinally,         // try-finally
    Using,              // using 语句

    // ── 位运算 / 移位（RFC 036 M1b 编译器夯实；追加于末尾以保持既有判别值稳定）──
    ExclusiveOr,        // a ^ b
    LeftShift,          // a << b
    RightShift,         // a >> b
}
