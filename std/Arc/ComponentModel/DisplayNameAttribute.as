// RFC 012 M3: 标准库属性 — 显示名称 [DisplayName]。
//
// 对标 C# System.ComponentModel.DisplayNameAttribute，扩展 typeof+nameof 本地化支持。
// 提供两种构造方式：
//   1. `[DisplayName("literal")]`                                         — 字面量，不本地化
//   2. `[DisplayName(typeof(ResourceClass), nameof(ResourceClass.Key))]`  — typeof+nameof 强类型本地化引用
//
// RFC 018 M5：ResourceType 为 Type?（typeof(T) → RuntimeType），不再使用已删除的 TypeId struct。
//
// 本地化解析（RFC 027）：方式 2 只携带 (ResourceType, ResourceKey) 数据对——
// 元数据不含行为（RFC 018 物理边界，MethodInfo 无 Invoke），解析由消费框架
// （UI 属性网格等）按自身策略完成；ResX CodeGen 生成的访问器属性即文化
// 前缀分支，是推荐的解析目标。

namespace Arc.ComponentModel;

using Arc.Reflection;

/// <summary>
/// 指定属性或事件的显示名称，支持本地化。
///
/// 字面量用法（不本地化）：
///   `[DisplayName("Full Name")]`
///
/// 强类型本地化引用：
///   `[DisplayName(typeof(MyApp.Resources.Strings), nameof(MyApp.Resources.Strings.FullName))]`
///
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class DisplayNameAttribute : Attribute {
    /// 显示名称（字面量构造的最终值；本地化引用构造下为空串，由消费方按资源对解析）。
    public string DisplayName { get; }
    /// 资源类型（非 null 时表示此值为本地化引用；由 typeof(T) 得到）。
    public Type? ResourceType { get; }
    /// 资源键（与 ResourceType 组成二元组；指向访问器类的属性名）。
    public string ResourceKey { get; }

    /// <summary>字面量构造（非本地化）。</summary>
    public DisplayNameAttribute(string displayName) {
        DisplayName = displayName;
        ResourceType = null;
        ResourceKey = "";
    }

    /// <summary>
    /// typeof + nameof 强类型本地化引用构造。
    ///
    /// 用法：`[DisplayName(typeof(MyApp.Strings), nameof(MyApp.Strings.Welcome))]`
    /// </summary>
    public DisplayNameAttribute(Type resourceType, string resourceKey) {
        DisplayName = "";
        ResourceType = resourceType;
        ResourceKey = resourceKey;
    }
}
