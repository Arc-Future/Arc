// RFC 037 D3.9 / RFC 037 / RFC 037: ThemeDictionary —— 主题资源持有器。
//
// 对标 WPF ThemeDictionaries / MAUI ThemeDictionary：按主题名持有 ResourceDictionary，
// 由 <see cref="Application.Current"/> 作为单一解析根持有本实例；`Switch` 切换活动主题。
// 内置默认主题 Light/Dark 的色值见 `std/UI/Themes/*.arml`（经 BuiltInThemeColors）；
// 几何/motion 与键常量见 BuiltInTheme.as。
//
// 本类不承担字符串查找魔法，也不暴露全局单例——解析统一经 Application.Current 的
// ResolveColor/ResolveNumber（WPF DynamicResource 语义），渲染器为唯一消费方。

namespace Arc.UI.Styling;

using Arc.Collections;

/// <summary>主题资源持有器（主题名 → ResourceDictionary）。</summary>
internal class ThemeDictionary {
    /// <summary>当前主题名。</summary>
    public string CurrentTheme;

    /// <summary>主题资源表（主题名 → ResourceDictionary）。</summary>
    public Dictionary<string, ResourceDictionary> Themes;

    /// <summary>当前活动主题资源字典。</summary>
    public ResourceDictionary Active;

    public ThemeDictionary() {
        this.Themes = new Dictionary<string, ResourceDictionary>();
        this.Themes.Add("Light", BuiltInTheme.CreateLight());
        this.Themes.Add("Dark", BuiltInTheme.CreateDark());
        this.CurrentTheme = "Light";
        this.Active = this.Themes["Light"];
    }

    /// <summary>切换当前活动主题（未注册的主题不生效）。</summary>
    public void Switch(string name) {
        if (this.Themes.ContainsKey(name)) {
            this.CurrentTheme = name;
            this.Active = this.Themes[name];
        }
    }

    /// <summary>
    /// 注册/覆盖一个主题。生成的字典为**编译期已聚合的平坦结果**（codegen 扁平化
    /// `BasedOn` 链 + 内置基底），此处仅纯存储，不做运行期合并——避免第三方库多层
    /// 封装覆盖时的逐层 Merged 开销。切主题即 O(1) 换引用。
    /// </summary>
    /// <param name="name">主题名。</param>
    /// <param name="dict">编译期已聚合的完整主题资源字典。</param>
    public void RegisterTheme(string name, ResourceDictionary dict) {
        this.Themes.Remove(name);
        this.Themes.Add(name, dict);
        if (this.CurrentTheme == name) {
            this.Active = dict;
        }
    }
}
