// App.arml.as: 应用生命周期 code-behind（对标 WPF App.xaml.cs）。
//
// 与 codegen 自动生成的 `App.g.as` 合并构成完整 `App` 类型。
// P1：全局隐式 Button=colorError 已撤；App.arml 仅注册显式
// `DangerButtonStyle`（x:Key），Style 页演示 Rest 静态底 vs VSM Hover 覆盖。

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
