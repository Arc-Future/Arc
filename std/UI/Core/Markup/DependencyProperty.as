// RFC 037 D1: Arc.UI —— 强类型依赖属性（WPF 同构编程模型）。
//
// 用户每属性仅写两件套：
//   1. 静态 DependencyProperty<T> 元数据字段
//   2. 属性 wrapper 调用 Element.GetValue<T>/SetValue<T>
//   Signal<T> 后端由 Element 基类内部维护，用户无感知。
//
// **推荐编码模型**（使用 nameof/typeof 替代硬编码字符串）：
//   public static DependencyProperty<string> TitleProperty =
//       RegisterProperty<string>(nameof(Title), typeof(Window), "");
//
// `nameof(Title)` 编译期解析为字符串 "Title"（parser desugar → StringLit）；
// `typeof(Window)` 返回 RuntimeType 实例（RFC 018 M2 step 4，Type 子类）。
// 两者均避免魔法字符串，IDE 重构可自动追踪符号引用。
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」命名空间分层原则：
// 基类放根命名空间，派生实现在子命名空间）。DependencyProperty 是
// Element 的核心依赖，必须与 Element 同处 `Arc.UI` 命名空间，避免
// 派生类（如 Arc.UI.Components.Window）需要同时 `using Arc.UI` 和
// `using Arc.UI.Markup` 的反向引用反模式。
//
// **不设非泛型基类**：Arc 泛型模板（`DependencyProperty<T>`）按 RFC 018 M4-1
// 不注册到 `registry.types`，故 `resolve_field` 无法沿基类链解析继承字段——
// 若把身份字段（Id/Name/OwnerType）放在非泛型基类，`Element.GetValue<T>` 等
// 经 `DependencyProperty<T>` 引用访问 `prop.Id` 会报
// `no field or property 'Id' on 'DependencyProperty_T'`。故身份字段必须全部
// **直接声明在泛型类内**（裸赋值，与 `Box<T>` 等既有泛型类一致）。
//
// 布局说明：本泛型类直接声明值类型字段 `DefaultValue`。CD-4 修复后 codegen
// 对「值类型 T 字段位于字段列表末尾」也按值类型布局（`double` 字段 → struct 尾部
// `double`，返回值 `ret double`），不再需要引用字段充当布局锚点。字段顺序
// [Id, Name, OwnerType, DefaultValue] 与构造器参数序一致（自然声明序）。

namespace Arc.UI;

using Arc.Collections;
using Arc.Reflection;

/// <summary>
/// 强类型依赖属性元数据。每个 DependencyProperty&lt;T&gt; 实例对应一个属性槽，
/// 全局唯一 Id 由 DependencyPropertyRegistry 分配。身份字段（Id/Name/OwnerType）
/// 与默认值 DefaultValue 均直接声明于本泛型类。
/// </summary>
/// <typeparam name="T">属性值类型（如 double/string/int）。</typeparam>
public class DependencyProperty<T> {
    /// <summary>全局唯一标识（由 DependencyPropertyRegistry 自动分配）。</summary>
    public long Id;

    /// <summary>属性名（如 "Width"、"Text"）。</summary>
    public string Name;

    /// <summary>所有者类型（由 typeof(T) 产生 RuntimeType 实例，避免硬编码字符串）。</summary>
    public Type OwnerType;

    /// <summary>注册期元数据（可空）。环境属性继承等能力由元数据声明
    ///（见 FrameworkPropertyMetadata），查找引擎零属性名硬编码。</summary>
    public FrameworkPropertyMetadata? Metadata;

    /// <summary>默认值（强类型 T，无装箱）。</summary>
    public T DefaultValue;

    /// <summary>
    /// 构造依赖属性元数据。外部代码应通过 RegisterProperty&lt;T&gt; 工厂创建，
    /// 由 DependencyPropertyRegistry 自动分配 Id。
    /// </summary>
    /// <param name="id">全局唯一标识。</param>
    /// <param name="name">属性名。</param>
    /// <param name="ownerType">所有者类型（由 typeof(T) 产生 RuntimeType 实例）。</param>
    /// <param name="defaultValue">默认值。</param>
    ///
    /// 注：泛型类模板 `DependencyProperty&lt;T&gt;` 按 RFC 018 M4-1 设计方案不注册到
    /// `registry.types`，`this.&lt;Field&gt;` 经由 `resolve_field` 查找失败——故全部字段
    /// 采用裸赋值（非 `this.` 前缀），与 `Box&lt;T&gt;` 等既有泛型类构造一致。
    public DependencyProperty(long id, string name, Type ownerType, T defaultValue) {
        Id = id;
        DefaultValue = defaultValue;
        Name = name;
        OwnerType = ownerType;
    }
}

/// <summary>
/// 注册强类型依赖属性元数据。
/// </summary>
/// <typeparam name="T">属性值类型。</typeparam>
/// <param name="name">属性名（推荐使用 nameof(属性) 产生，避免硬编码字符串）。</param>
/// <param name="ownerType">所有者类型（由 typeof(T) 产生 RuntimeType 实例）。</param>
/// <param name="defaultValue">默认值。</param>
/// <returns>强类型依赖属性元数据（含全局唯一 Id）。</returns>
public DependencyProperty<T> RegisterProperty<T>(string name, Type ownerType, T defaultValue) {
    long id = DependencyPropertyRegistry.NextId();
    var dp = new DependencyProperty<T>(id, name, ownerType, defaultValue);
    // 按所有者类型登记到类型作用域注册表（object 擦除视图），供目标元素
    // 沿类型链按名解析 DP（StyleEvaluator 动态分派，替代硬编码属性 switch）。
    DependencyPropertyRegistry.Register(ownerType.TypeId, name, dp);
    return dp;
}

/// <summary>
/// 注册环境（可继承）依赖属性：注册期元数据声明 Inherits（对标 WPF
/// FrameworkPropertyMetadataOptions.Inherits），值沿元素树向下继承——
/// Window/根容器设置一次全树生效。继承引擎按元数据驱动、零属性名硬编码，
/// 任何 DP 经本工厂注册即入环境属性体系。
/// </summary>
/// <typeparam name="T">属性值类型。</typeparam>
/// <param name="name">属性名（推荐 nameof(属性)）。</param>
/// <param name="ownerType">所有者类型。</param>
/// <param name="defaultValue">默认值（无祖先值时的兜底）。</param>
public DependencyProperty<T> RegisterInheritedProperty<T>(string name, Type ownerType, T defaultValue) {
    long id = DependencyPropertyRegistry.NextId();
    var dp = new DependencyProperty<T>(id, name, ownerType, defaultValue);
    dp.Metadata = new FrameworkPropertyMetadata(FrameworkPropertyMetadataOptions.Inherits);
    DependencyPropertyRegistry.Register(ownerType.TypeId, name, dp);
    DependencyPropertyRegistry.MarkInherited(id);
    return dp;
}
