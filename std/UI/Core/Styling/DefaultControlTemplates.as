// RFC 037 · 内置控件默认 ControlTemplate（WPF Style+Template 对齐）。
//
// 契约（与 theme-style-interaction-architecture / COMPONENTS 同步）：
//   - 落点：隐式 Style 的 Template Setter（本类工厂 → BuiltInTheme.AttachDefaultTemplates）
//   - 部件名：PART_Chrome / PART_Glyph / PART_Content（禁另起命名空间）
//   - VSM：仍为 Style 内部态引擎；RenderTree 经 ChromeHostHandle+ChromeRole 把配方
//     画到 PART_*，宿主有模板子树则跳过内置 chrome 硬分支
//   - 多实例：一律 Instantiate 工厂（禁共享 VisualTree 单树）
//   - ARML `<ControlTemplate>` 字面发射后置；本轮代码工厂为权威默认源
//
// 已迁：Button/Toggle/Check/Radio/TextBox/PasswordBox/Slider/ProgressBar/ComboBox。
// 未迁（诚实）：TabControl（Panel + 内容子树，templated 门禁不适用）；
//   DataGrid（ApplyTo 清子树会毁行镜像；专属 RenderDataGrid 过大）。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Components.Layout;
using Arc.UI.Layout;

/// <summary>内置控件默认 ControlTemplate 工厂与部件名常量。</summary>
public class DefaultControlTemplates {
    /// <summary>填充/描边/圆角/焦点环外壳（Button/Toggle/TextBox/Slider/Progress/Combo）。</summary>
    public const string PartChrome = "PART_Chrome";

    /// <summary>勾选/单选指示盒（CheckBox/RadioButton）。</summary>
    public const string PartGlyph = "PART_Glyph";

    /// <summary>内容/标签文本（TemplateBinding Content→Text）。</summary>
    public const string PartContent = "PART_Content";

    /// <summary>把默认 Template Setter 挂到隐式控件 Style（仅空 Key）。</summary>
    public static void AttachToStyles(ResourceDictionary dict) {
        if (dict == null) {
            return;
        }
        List<Style> styles = dict.GetAllStyles();
        if (styles == null) {
            return;
        }
        for (int i = 0; i < styles.Count; i++) {
            Style style = styles[i];
            if (style == null) {
                continue;
            }
            if (style.Key != null && style.Key.Length > 0) {
                continue;
            }
            string target = style.TargetType;
            if (target == "Button") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.Button());
            } else if (target == "ToggleButton") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.ToggleButton());
            } else if (target == "CheckBox") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.CheckBox());
            } else if (target == "RadioButton") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.RadioButton());
            } else if (target == "TextBox") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.TextBox());
            } else if (target == "PasswordBox") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.PasswordBox());
            } else if (target == "Slider") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.Slider());
            } else if (target == "ProgressBar") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.ProgressBar());
            } else if (target == "ComboBox") {
                DefaultControlTemplates.AttachTemplate(style, DefaultControlTemplates.ComboBox());
            }
        }
    }

    static void AttachTemplate(Style style, ControlTemplate template) {
        Setter s = new Setter();
        s.Property = "Template";
        s.TemplateValue = template;
        style.Setters.Add(s);
    }

    /// <summary>Button：PART_Chrome(Border) + PART_Content(TextBlock)。</summary>
    public static ControlTemplate Button() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "Button";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildChromeWithLabel();
        return t;
    }

    /// <summary>ToggleButton：同 Button 结构。</summary>
    public static ControlTemplate ToggleButton() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "ToggleButton";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildChromeWithLabel();
        return t;
    }

    /// <summary>CheckBox：横排 PART_Glyph + PART_Content。</summary>
    public static ControlTemplate CheckBox() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "CheckBox";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildGlyphWithLabel();
        return t;
    }

    /// <summary>RadioButton：横排圆形 PART_Glyph + PART_Content。</summary>
    public static ControlTemplate RadioButton() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "RadioButton";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildGlyphWithLabel();
        return t;
    }

    /// <summary>TextBox：仅 PART_Chrome；文本/caret 仍宿主层。</summary>
    public static ControlTemplate TextBox() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "TextBox";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildInputChrome();
        return t;
    }

    /// <summary>PasswordBox：同 TextBox chrome 壳。</summary>
    public static ControlTemplate PasswordBox() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "PasswordBox";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildInputChrome();
        return t;
    }

    /// <summary>Slider：PART_Chrome 壳；轨/填充/thumb 经 PART Surface 角色画 VSM.Slider。</summary>
    public static ControlTemplate Slider() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "Slider";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildInputChrome();
        return t;
    }

    /// <summary>ProgressBar：PART_Chrome；比例填充 / IsIndeterminate 扫掠在 PART Surface。</summary>
    public static ControlTemplate ProgressBar() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "ProgressBar";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildInputChrome();
        return t;
    }

    /// <summary>ComboBox：PART_Chrome 壳；SelectedText/chevron 仍宿主层（展开 Popup 另轨）。</summary>
    public static ControlTemplate ComboBox() {
        ControlTemplate t = new ControlTemplate();
        t.TargetType = "ComboBox";
        t.Instantiate = (Control host) => DefaultControlTemplates.BuildInputChrome();
        return t;
    }

    public static Element BuildChromeWithLabel() {
        Border chrome = DefaultControlTemplates.NewChromeBorder();
        chrome.SetAttachedString(ControlTemplate.TemplateBindingPropertyKey, "Padding");
        TextBlock label = DefaultControlTemplates.NewContentLabel();
        chrome.Child = label;
        return chrome;
    }

    public static Element BuildGlyphWithLabel() {
        StackPanel row = new StackPanel();
        row.TypeName = "StackPanel";
        row.Orientation = Orientation.Horizontal;
        row.Spacing = ControlMetrics.ToggleLabelGap;
        row.HorizontalAlignment = HorizontalAlignment.Left;
        row.VerticalAlignment = VerticalAlignment.Center;

        Border glyph = new Border();
        glyph.Name = PartGlyph;
        glyph.TypeName = "Border";
        glyph.Width = ControlMetrics.ToggleBoxSize;
        glyph.Height = ControlMetrics.ToggleBoxSize;
        glyph.MinWidth = ControlMetrics.ToggleBoxSize;
        glyph.MinHeight = ControlMetrics.ToggleBoxSize;
        glyph.CornerRadius = ControlMetrics.ControlRadius;
        glyph.BorderThickness = "1";
        glyph.Focusable = false;
        glyph.IsTabStop = false;

        TextBlock label = DefaultControlTemplates.NewContentLabel();
        label.HorizontalAlignment = HorizontalAlignment.Left;
        label.VerticalAlignment = VerticalAlignment.Center;

        row.AddChild(glyph);
        row.AddChild(label);
        return row;
    }

    public static Element BuildInputChrome() {
        Border chrome = DefaultControlTemplates.NewChromeBorder();
        chrome.SetAttachedString(ControlTemplate.TemplateBindingPropertyKey, "Padding");
        return chrome;
    }

    static Border NewChromeBorder() {
        Border chrome = new Border();
        chrome.Name = PartChrome;
        chrome.TypeName = "Border";
        chrome.CornerRadius = ControlMetrics.ControlRadius;
        chrome.BorderThickness = "1";
        chrome.HorizontalAlignment = HorizontalAlignment.Stretch;
        chrome.VerticalAlignment = VerticalAlignment.Stretch;
        chrome.Focusable = false;
        chrome.IsTabStop = false;
        return chrome;
    }

    static TextBlock NewContentLabel() {
        TextBlock label = new TextBlock();
        label.Name = PartContent;
        label.TypeName = "TextBlock";
        label.HorizontalAlignment = HorizontalAlignment.Center;
        label.VerticalAlignment = VerticalAlignment.Center;
        label.Focusable = false;
        label.IsTabStop = false;
        label.SetAttachedString(ControlTemplate.TemplateBindingPropertyKey, "Content");
        return label;
    }
}
