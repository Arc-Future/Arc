// RFC 018 §4.2.6: 事件信息——对齐 C# System.Reflection.EventInfo。
//
// Arc 拒绝 C# event 关键字（1.4 裁决判例库），但事件作为成员仍可有元数据描述
// （如属性 attribute 标记的方法对）。
// **永久剔除 Invoke() / AddEventHandler() / RemoveEventHandler()**——
// 元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。

namespace Arc.Reflection;

/// <summary>
/// 事件信息——对齐 C# System.Reflection.EventInfo。
///
/// Arc 拒绝 C# event 关键字（1.4 裁决判例库），但事件作为成员仍可有元数据描述
/// （如属性 attribute 标记的方法对）。
/// **永久剔除 Invoke() / AddEventHandler() / RemoveEventHandler()**——
/// 元数据 vs 反射的物理边界（RFC 018 §3.2 / §3.3）。
/// </summary>
public abstract class EventInfo : MemberInfo {
    /// <summary>事件处理器类型（通常是 Func&lt;...&gt; 或 Action&lt;...&gt; 委托类型）。</summary>
    public Type EventHandlerType { get; }

    /// <summary>事件 add 方法（订阅入口）。</summary>
    public MethodInfo? AddMethod { get; }

    /// <summary>事件 remove 方法（取消订阅入口）。</summary>
    public MethodInfo? RemoveMethod { get; }

    /// <summary>事件 raise 方法（触发入口，可能为 null）。</summary>
    public MethodInfo? RaiseMethod { get; }

    /// <summary>事件特性位掩码（C# System.Reflection.EventAttributes 对齐）。</summary>
    public EventAttributes Attributes { get; }

    /// <summary>受保护构造函数——具体 EventInfo 派生类通过 : base() 调用。</summary>
    protected EventInfo() {}
}
