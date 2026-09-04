// RFC 009 M3: 标准库属性 — 描述文本 [Description]。
//
// 对标 C# System.ComponentModel.DescriptionAttribute，扩展 typeof+nameof 本地化支持。
// 提供两种构造方式：
//   1. `[Description("literal")]`                                         — 字面量，不本地化
//   2. `[Description(typeof(ResourceClass), nameof(ResourceClass.Key))]`  — typeof+nameof 强类型本地化引用
//
// RFC 018 M5：ResourceType 为 Type?（typeof(T) → RuntimeType），不再使用已删除的 TypeId struct。
//
// 本地化解析（RFC 027）：方式 2 只携带 (ResourceType, ResourceKey) 数据对——
// 元数据不含行为（RFC 018 物理边界，MethodInfo 无 Invoke），解析由消费框架
// 按自身策略完成；ResX CodeGen 生成的访问器属性即文化前缀分支，是推荐的解析目标。

namespace Arc.ComponentModel;

using Arc.Reflection;

/// <summary>
/// 为属性或事件提供描述文本，支持本地化。
///
/// 字面量用法（不本地化）：
///   `[Description("User's full name")]`
///
/// 强类型本地化引用：
///   `[Description(typeof(MyApp.Resources.Strings), nameof(MyApp.Resources.Strings.UserFullName))]`
///
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class DescriptionAttribute : Attribute {
    /// 描述文本（字面量构造的最终值；本地化引用构造下为空串，由消费方按资源对解析）。
    public string Description { get; }
    /// 资源类型（非 null 时表示此值为本地化引用；由 typeof(T) 得到）。
    public Type? ResourceType { get; }
    /// 资源键（与 ResourceType 组成二元组；指向访问器类的属性名）。
    public string ResourceKey { get; }

    /// <summary>字面量构造（非本地化）。</summary>
    public DescriptionAttribute(string description) {
        Description = description;
        ResourceType = null;
        ResourceKey = "";
    }

    /// <summary>
    /// typeof + nameof 强类型本地化引用构造。
    ///
    /// 用法：`[Description(typeof(MyApp.Strings), nameof(MyApp.Strings.HelpText))]`
    /// </summary>
    public DescriptionAttribute(Type resourceType, string resourceKey) {
        Description = "";
        ResourceType = resourceType;
        ResourceKey = resourceKey;
    }
}
