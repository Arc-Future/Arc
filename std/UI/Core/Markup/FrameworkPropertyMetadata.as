// RFC 037 §4：DP 注册期元数据（属性值继承能力声明）。
//
// 对标 WPF FrameworkPropertyMetadata(Options.Inherits)：继承能力由注册期
// 元数据声明，Element 查找引擎按元数据驱动——属性集合零硬编码，任何 DP
// 注册时携带 Inherits 选项即进入环境属性体系。
//
// Arc enum AST 不支持显式值语法，位掩码选项以 class + public const int
// 实现（AttributeTargets 同构先例，std/Arc/Attribute.as）。

namespace Arc.UI;

/// <summary>
/// DP 注册期元数据选项位掩码（对标 WPF FrameworkPropertyMetadataOptions [Flags]）。
/// 组合用法：FrameworkPropertyMetadataOptions.Inherits。
/// </summary>
public class FrameworkPropertyMetadataOptions {
    /// <summary>无选项。</summary>
    public const int None = 0;

    /// <summary>环境属性：读取时无本地/样式值则沿 Parent 链取最近祖先有效值
    ///（对标 WPF property value inheritance，如 FontFamily/FontSize）。</summary>
    public const int Inherits = 1;

    // 预留位（需求落地时增补，勿提前设计）：AffectsMeasure / AffectsRender /
    // AffectsArrange / NotDataBindable。
}

/// <summary>
/// 依赖属性注册期元数据（对标 WPF FrameworkPropertyMetadata）。
/// 经 RegisterProperty(name, ownerType, defaultValue, metadata) 附加到 DP。
/// </summary>
public class FrameworkPropertyMetadata {
    /// <summary>选项位掩码（FrameworkPropertyMetadataOptions.* 组合）。</summary>
    public int Options { get; }

    /// <summary>是否环境属性（Options 含 Inherits 位）。</summary>
    public bool Inherits {
        get {
            return (this.Options & FrameworkPropertyMetadataOptions.Inherits) != 0;
        }
    }

    /// <param name="options">选项位掩码（FrameworkPropertyMetadataOptions.* 组合）。</param>
    public FrameworkPropertyMetadata(int options) {
        this.Options = options;
    }
}
