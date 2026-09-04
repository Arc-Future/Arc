// RFC 012 M3: 标准库属性 — 分类标记 [Category]。
//
// 对标 C# System.ComponentModel.CategoryAttribute，扩展 typeof+nameof 本地化支持。
// 提供两种构造方式：
//   1. `[Category("literal")]`                                         — 字面量，不本地化
//   2. `[Category(typeof(ResourceClass), nameof(ResourceClass.Key))]`  — typeof+nameof 强类型本地化引用
//
// RFC 018 M5：ResourceType 为 Type?（typeof(T) → RuntimeType），不再使用已删除的 TypeId struct。
//
// 本地化解析（RFC 027）：方式 2 只携带 (ResourceType, ResourceKey) 数据对——
// 元数据不含行为（RFC 018 物理边界，MethodInfo 无 Invoke），解析由消费框架
// 按自身策略完成；ResX CodeGen 生成的访问器属性即文化前缀分支，是推荐的解析目标。

namespace Arc.ComponentModel;

using Arc.Reflection;

/// <summary>
/// 指定属性或事件所属的分类，支持本地化。
///
/// 字面量用法（不本地化）：
///   `[Category("Appearance")]`
///
/// 强类型本地化引用：
///   `[Category(typeof(MyApp.Resources.Categories), nameof(MyApp.Resources.Categories.Appearance))]`
///
/// 合法附加目标：All。
/// </summary>
[AttributeUsage(AttributeTargets.All)]
public class CategoryAttribute : Attribute {
    /// 分类名称（字面量构造的最终值；本地化引用构造下为空串，由消费方按资源对解析）。
    public string Category { get; }
    /// 资源类型（非 null 时表示此值为本地化引用；由 typeof(T) 得到）。
    public Type? ResourceType { get; }
    /// 资源键（与 ResourceType 组成二元组；指向访问器类的属性名）。
    public string ResourceKey { get; }

    /// <summary>字面量构造（非本地化）。</summary>
    public CategoryAttribute(string category) {
        Category = category;
        ResourceType = null;
        ResourceKey = "";
    }

    /// <summary>
    /// typeof + nameof 强类型本地化引用构造。
    ///
    /// 用法：`[Category(typeof(MyApp.Categories), nameof(MyApp.Categories.Appearance))]`
    /// </summary>
    public CategoryAttribute(Type resourceType, string resourceKey) {
        Category = "";
        ResourceType = resourceType;
        ResourceKey = resourceKey;
    }
}
