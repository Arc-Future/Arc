// App.arml.as: 应用生命周期 code-behind（对标 WPF App.xaml.cs）。
//
// 与 codegen 自动生成的 `App.g.as` 合并构成完整 `App` 类型。
// 宿主隐式 Button 红样式（#FFCC2222）现以声明式呈现——见 App.arml 的
// `<Application.Resources><Style TargetType="Button">`，由 `arc ui codegen`
// 在 App.g.as 的 `InitializeComponent()` 中等价生成 `Resources.AddStyle`。
// 该样式作用于宿主层全部 Button（分区 2 的 Controls 按钮会被染红，属预期
// 演示效果）；VisualHost 内层合并 RFC 037 Light Theme，呈现 Primary 蓝
// （见 MainWindow.arml 分区 8「Style & Isolation」）。

namespace ArmlDemo;

using Arc;
using Arc.UI.Components;

public partial class App : Application
{
    public override void OnStartup()
    {
        // RFC 037 §9：正道仅 Fonts.RegisterFamily。
        // 相对路径相对应用基目录（exe 所在 bin/<Config>/）；build 复制 Assets/ → bin。
        // 三参重载产 chain "normalAbs|boldAbs"（单 '|' = Bold 面，供 FontWeight 选用）。
        bool ok = this.Fonts.RegisterFamily(
            "AppSans",
            "Assets/Fonts/AppSans.ttf",
            "Assets/Fonts/AppSans-Bold.ttf");
        if (!ok)
        {
            ok = this.Fonts.RegisterFamily("AppSans", "Assets/Fonts/AppSans.ttf");
        }
        if (!ok)
        {
            Console.ErrorWriteLine("[ArmlDemo] AppSans register failed (missing under bin/<Config>/Assets/Fonts/); FontFamily falls back to default");
        }
    }
}
