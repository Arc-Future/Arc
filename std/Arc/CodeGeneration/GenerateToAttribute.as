// RFC 012 M4-1: GenerateToAttribute 宏特性代码注入体系根基类型（v1.0 修订）。
//
// 本文件定义 M4 宏特性的根基类型：
//   - GenerateToAttribute<T>（泛型，abstract）：宏特性派生基类，关联宏容器类 T
//
// **设计说明（RFC 012 v1.0 / RFC 012 v0.12，2026-07-19 修订）**：
//   - GenerateToAttribute<T> 为 abstract class，仅作为派生基类，不可直接实例化
//   - 泛型参数 T 关联一个「宏容器」类——T 是普通业务类即可，约束 `where T : class`
//     （v0.11 放宽，原 `where T : Attribute`），不再强制容器派生自 Attribute
//   - 容器识别完全通过 features 反向推断：扫描所有 `GenerateToAttribute<T>` 派生类
//     的 T 参数，T 即为容器类——容器类无需任何特殊标注（如 [GenerateTo]）
//   - 派生类（如 QIF 的 FactAttribute、DI 的 TransientAttribute）构造函数必须接收
//     一个 Expression 参数（表示「完整的类型定义」，即 typeof(实现类型)），并通过
//     `: base(expr)` 转发到基类构造函数；派生类自行处理该 Expression
//   - 派生类构造函数体由 typeck D10.6 编译期解释器执行，通过单一 API 注册展开：
//     * Build(Action<StringBuilder>)——向容器类 Build() 方法体前置追加代码
//
// **v1.0 简化（撤销 v0.11 方案 A 埋点配对）**：
//   - v0.11 曾存在双方案：方案 A `GenerateTo(name, code)` 命名埋点配对 + 方案 B `Build`
//   - v1.0 撤销方案 A，仅保留方案 B `Build(Action<StringBuilder>)`——单一 API，降低
//     复杂度，避免埋点配对的命名冲突与跨容器错配风险
//   - 容器类必须提供 `Build()` 方法作为唯一展开槽位（由编译器在 Pass 3 splice 注入）
//
// **abstract class 设计动机**：
//   - 强制派生类显式声明构造函数并处理 Expression，避免直接
//     `new GenerateToAttribute<T>()` 的无意义实例化
//   - 派生类各自决定 Expression 的语义（QIF 用 ClassExpression.Methods 生成
//     注册代码；DI 用 TypeName 生成注册代码；其他场景可读取接口列表、方法签名等）
//   - typeck 在 Expr::New 检查 is_abstract 字段，禁止直接实例化 abstract class
//
// **QIF marker attribute 派生自此基类（RFC 032 QIF v1.0 路径）**：
//   - QIF 的 FactAttribute/TheoryAttribute/InlineDataAttribute 派生自
//     `GenerateToAttribute<QIFRegistryBuilder>`——QIFRegistryBuilder 是普通业务类
//     （不派生 Attribute，不标 [GenerateTo]），通过 v0.11 放宽的 `where T : class`
//     约束合法关联
//   - 派生类构造函数接收 `Expression expression` 参数（实际为 ClassExpression，
//     由 typeck 在扫描到「类上标了 [Fact]」时构造并注入）
//   - 派生类构造函数体含 `if (expression is ClassExpression classDef)
//     { foreach (var m in classDef.Methods) { this.Build(s => { ... }); } }`
//     等控制流嵌套——D10.6 构造函数体编译期解释器负责识别此类结构
//
// **架构红线**（RFC 009 D9.1 / RFC 012 D2.1）：
//   - M4 宏逻辑以普通 Arc 代码（委托 + StringBuilder 拼接）编写
//   - 不要求独立 proc-macro crate
//   - D10.6 解释器仅服务于 GenerateToAttribute<T> 派生类构造函数体解释
//   - 不扩展为通用 comptime / CTFE
//   - typeck 零 QIF 专用代码——解释器是 typeck 通用机制

namespace Arc.CodeGeneration;

using Arc.Linq.Expressions;
using Arc.Text;

/// <summary>
/// 宏特性派生基类（RFC 009 D9.3，v1.0 修订）。
///
/// 泛型参数 T 关联一个「宏容器」类——T 是普通业务类即可（约束
/// <c>where T : class</c>，v0.11 放宽，原 <c>where T : Attribute</c>）。
/// 本类为 abstract class，不可直接实例化——派生类必须：
///   1. 定义自己的构造函数，接收一个 <c>Expression expression</c> 参数
///      （表示「完整的类型定义」，即 typeof(实现类型)）
///   2. 通过 <c>: base(expression)</c> 调用基类构造函数
///   3. 自行处理 expression（如读取 TypeName 注册展开委托）
///
/// 派生类构造函数体由 typeck D10.6 编译期解释器执行，通过单一 API 注册展开：
///   - <see cref="Build"/>：向容器类 Build() 方法体前置追加代码
///
/// v1.0 撤销 v0.11 方案 A `GenerateTo(name, code)` 埋点配对，仅保留方案 B
/// `Build(Action&lt;StringBuilder&gt;)`——单一 API 降低复杂度，避免埋点配对
/// 的命名冲突与跨容器错配风险。容器类必须提供 `Build()` 方法作为唯一展开槽位。
///
/// 容器识别完全通过 features 反向推断（v0.11 修订）：扫描所有
/// <c>GenerateToAttribute&lt;T&gt;</c> 派生类的 T 参数，T 即为容器类——
/// 容器类无需任何特殊标注。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public abstract class GenerateToAttribute<T> : Attribute where T : class {
    /// <summary>
    /// 受保护构造函数——派生类必须通过 <c>: base(expression)</c> 调用。
    ///
    /// <c>expression</c> 表示「完整的类型定义」（即 typeof(实现类型)），由派生类
    /// 自行处理其语义。基类仅持有引用，不做处理——实际处理逻辑由派生类
    /// 在自身构造函数中完成（如调用 Build 注册展开）。
    /// </summary>
    /// <param name="expression">派生类要处理的类型定义表达式。</param>
    protected GenerateToAttribute(Expression expression) {}

    /// <summary>
    /// 向容器类 Build() 方法体前置追加代码（RFC 009 M4 + RFC 012 v1.0 单一 API）。
    ///
    /// 编译器识别此调用模式：<c>this.Build(s =&gt; { s.AppendLine(code); })</c>，
    /// D10.6 解释器执行 lambda 体中的 StringBuilder 操作，收集 AppendLine 调用
    /// 的字符串作为代码片段，在 Pass 3 splice 到 T 容器类的 Build() 方法体前部。
    ///
    /// **v1.0 设计**：本方法是 GenerateToAttribute&lt;T&gt; 派生类注册展开的
    /// **唯一 API**——v0.11 的方案 A `GenerateTo(name, code)` 埋点配对已撤销。
    /// 容器类必须提供 `Build()` 方法作为唯一展开槽位。
    ///
    /// **适用场景**：容器类源码可修改且 Build() 方法存在时使用——直接前置追加代码。
    /// </summary>
    /// <param name="expansion">接收 StringBuilder 并向其追加代码的回调。</param>
    protected void Build(Action<StringBuilder> expansion) {
        // 编译器识别此方法调用，运行时为 no-op。
    }
}
