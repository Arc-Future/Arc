// RFC 037 D2.1 / M3 模板套用 + 数据模板分派: ContentPresenter — 在
// ControlTemplate 中呈现 ContentControl.Content。
//
// OnLoaded 同步（M3）：沿 Parent 链向上查找最近 ContentControl，把其 Content
// 拷入自身 Content（ContentProperty 约定，RFC 037 D3.6）。无父 ContentControl
// 时保持 None（平台宿主场景由 ControlTemplate.ApplyTo 显式注入）。
//
// 数据模板分派（WPF ContentPresenter + DataTemplate 对齐）：宿主处于数据
// 路径（DataContent 非 null）时，显式 ContentTemplate 优先 → 资源字典按
// DataTypeName 隐式匹配 → 兜底保持 Content variant 文本呈现。

namespace Arc.UI.Components;

using Arc.UI;
using Arc.UI.Styling;

/// <summary>在 ControlTemplate 中呈现 ContentControl.Content 的占位元素。</summary>
public class ContentPresenter : Element {
    /// <summary>构造元素并绑定运行时类型身份（供动态依赖属性解析）。</summary>
    public ContentPresenter() {
        this.Type = typeof(ContentPresenter);
        this.Content = Content.None;
    }

    /// <summary>
    /// 呈现的内容（来自父 ContentControl.Content，Content variant 直存——
    /// 消费方经 ContentHelper.TextOrEmpty / ElementOrNull 定向提取）。
    /// </summary>
    public Content Content;

    /// <summary>
    /// 沿 Parent 链查找最近 ContentControl 宿主；无宿主返回 null。
    /// </summary>
    private ContentControl? FindContentHost() {
        Element? node = this.Parent;
        while (true) {
            if (node == null) {
                return null;
            }
            if (node is ContentControl) {
                return (ContentControl)node;
            }
            node = node.Parent;
        }
    }

    /// <summary>
    /// 挂载时沿 Parent 链同步父 ContentControl.Content（M3 模板套用）。
    /// 被 ControlTemplate.ApplyTo 挂载时父链含宿主 ContentControl → 自动同步；
    /// 无 ContentControl 祖先保持 None。
    /// </summary>
    public override void OnLoaded() {
        ContentControl? host = this.FindContentHost();
        if (host != null) {
            this.Content = host.Content;
        }
    }

    /// <summary>
    /// 数据模板分派（WPF ContentPresenter 对齐）：宿主处于数据路径
    /// （DataContent 非 null）时，宿主 ContentTemplate（显式 DataTemplate）
    /// 优先，其次按宿主 DataTypeName 从资源字典隐式匹配；命中即经模板
    /// Instantiate 工厂物化视觉树挂为自身子节点（重复分派换树幂等，旧
    /// 子树卸载）。均未命中时保持既有 Content variant 呈现（兜底文本路径
    /// 由消费方经 ContentHelper 提取），数据路径不落文本。
    /// </summary>
    public void ApplyDataTemplate(ResourceDictionary resources) {
        ContentControl? host = this.FindContentHost();
        if (host == null) {
            return;
        }
        object payload = host.DataContent;
        if (payload == null) {
            return;
        }
        DataTemplate template = null;
        object templateSlot = host.ContentTemplate;
        if (templateSlot is DataTemplate) {
            template = (DataTemplate)templateSlot;
        }
        if (template == null && resources != null) {
            string typeName = host.DataTypeName;
            if (typeName != "") {
                template = resources.LookupTemplate(typeName);
            }
        }
        if (template == null) {
            return;
        }
        // 委托先读局部变量再调用（codegen 约束，InstantiateTemplated 同模式）
        Func<object, Element> instantiate = template.Instantiate;
        if (instantiate == null) {
            return;
        }
        Element visual = instantiate(payload);
        if (visual == null) {
            return;
        }
        while (this.Children.Count > 0) {
            this.Children.RemoveAt(0);
        }
        this.AddChild(visual);
    }
}
