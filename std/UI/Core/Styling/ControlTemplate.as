// RFC 037 D3.6 / D3.7 / M3: Arc.UI.Styling — ControlTemplate 控件模板。
//
// ControlTemplate 定义控件的视觉树结构（一组 Element），
// 由 ContentPresenter 占位呈现 ContentControl.Content。
//
// M3 模板套用：ControlTemplate 改为 class（结构可装箱进 Control.Template
// object DP）——`ApplyTo(Control)` 把视觉树挂为控件子节点，并把树内
// ContentPresenter 的 Content 同步为宿主 ContentControl.Content（手算一致，
// 验收见 style_selector_cascade_e2e）。DataTemplate 保持 struct（数据呈现
// 边界，ItemTemplate 消费方未到 M3）。
//
// 模板套用双路径（WPF 对齐）：
//   - 单树直挂：VisualTree 挂为控件子节点（代码构建模板的一次套用场景）；
//   - 多实例工厂：Instantiate 委托非 null 时逐宿主实例化独立视觉树——
//     同一 ControlTemplate 套用到多个宿主时互不共享节点（与
//     DataTemplate.Instantiate 委托模式同款，验收见 template_binding_e2e）。
//
// TemplateBinding 属性同步（WPF TemplateBinding 语义）：模板树内元素经
// SetAttachedString(TemplateBindingPropertyKey, 宿主DP名) 挂标，
// ApplyTo 后递归扫描标记元素，把宿主属性值同步写入元素属性——如模板内
// TextBlock ← 宿主 Button.Content 呈现文本。
//
// 动态重同步（WPF TemplateBinding 动态语义对齐）：ApplyTo 建立绑定的同时
// 订阅绑定涉及的宿主 DP 变更通知（Element.Observe<T> → Signal<T>.Subscribe，
// SetValue 复用同一 Signal 并触发其通知链），宿主属性后续变更时自动对模板
// 树重跑同步。
//
// 订阅机制（诚实标注，与 ItemsControl M6 静态方法组绕行同源）：回调用
// **静态方法组**注册（bare fn ptr，无捕获闭包）——编译器对逃逸闭包的
// ByRef 捕获存外层栈槽地址，闭包跨函数逃逸后槽位悬垂 → UB（多宿主并发
// 订阅实测 AV，与 `items.OnChanged((args) => ...)` 2026-08-05 实测同源）。
// 静态回调经 `_activeHost` 路由，**单活跃宿主**约束：最后 ApplyTo 的宿主
// 自动重同步；多宿主并发的全自动订阅依赖编译器逃逸闭包修复（依赖项），
// 其余宿主经公共 RefreshBindings(host) 手动重同步（全场景正确）。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Components;
using Arc.UI.Media;

/// <summary>控件模板，定义控件的视觉树结构。</summary>
public class ControlTemplate {
    /// <summary>
    /// TemplateBinding 附加属性标记 Key：标记值 = 宿主 DP 名（如 "Content"）。
    /// 模板树内元素经 <c>SetAttachedString(TemplateBindingPropertyKey, 名)</c>
    /// 挂标，ApplyTo 后由模板把宿主属性值同步到该元素属性。
    /// </summary>
    public const string TemplateBindingPropertyKey = "Arc.UI.TemplateBinding.Property";

    /// <summary>目标类型名（如 "Button"）。</summary>
    public string TargetType;

    /// <summary>模板视觉树根元素（单树直挂路径；Instantiate 非 null 时本字段被取代）。</summary>
    public Element VisualTree;

    /// <summary>触发器集合（M3+）。</summary>
    public object Triggers;

    /// <summary>
    /// 视觉树工厂：为宿主独立实例化视觉树。多宿主共享同一 ControlTemplate 时
    /// 每宿主调用一次、产出互不共享的树（WPF 多实例模板语义）；null 时走
    /// VisualTree 单树直挂。两者同时设置时本工厂优先。
    /// </summary>
    public Func<Control, Element> Instantiate;

    public ControlTemplate() {
        this.TargetType = "";
        this.VisualTree = null;
        this.Triggers = null;
        this.Instantiate = null;
    }

    /// <summary>
    /// 将模板视觉树套用到目标控件：卸载旧模板子树（换树语义，重复套用幂等），
    /// 视觉树挂为控件子节点，把树内 ContentPresenter 的 Content 同步为宿主
    /// ContentControl.Content，并按 TemplateBinding 标记把宿主属性值同步到
    /// 树内元素。视觉树来源双路径：Instantiate 工厂优先，否则 VisualTree 直挂。
    /// 套用同时订阅绑定涉及的宿主 DP 变更——宿主属性后续变更自动重同步模板树。
    /// </summary>
    public void ApplyTo(Control target) {
        if (target == null) {
            return;
        }
        Element root = this.ResolveVisualTree(target);
        if (root == null) {
            return;
        }
        while (target.Children.Count > 0) {
            target.Children.RemoveAt(0);
        }
        target.AddChild(root);
        this.SyncContentPresenters(target, root);
        this.SyncTemplateBindings(target, root);
        this.WatchHostChanges(target, root);
    }

    /// <summary>
    /// 手动重同步入口：立即把宿主当前属性值重刷到其模板视觉树（经 ApplyTo
    /// 建立的绑定记录）。自动订阅已覆盖 DP SetValue 全路径；本方法供宿主/
    /// 交互层在旁路变更（绕过 DP 的直接字段写）后显式兜底。无绑定记录
    /// （未 ApplyTo 或模板树无标记元素）时为无害空操作。
    /// </summary>
    public void RefreshBindings(Control host) {
        TemplateBindingWatcher.ResyncHost(host);
    }

    /// <summary>
    /// 解析套用树根：Instantiate 工厂非 null 时逐宿主实例化独立树，否则直挂
    /// VisualTree。委托字段先读入局部变量再调用——规避 codegen 委托字段
    /// 直调的静态化风险（与 ItemContainerGenerator.InstantiateTemplated 同款）。
    /// </summary>
    private Element ResolveVisualTree(Control target) {
        Func<Control, Element> factory = this.Instantiate;
        if (factory != null) {
            return factory(target);
        }
        return this.VisualTree;
    }

    private void SyncContentPresenters(Control target, Element node) {
        if (node == null) {
            return;
        }
        if (node is ContentPresenter) {
            ContentPresenter presenter = (ContentPresenter)node;
            if (target is ContentControl) {
                ContentControl host = (ContentControl)target;
                presenter.Content = host.Content;
            }
        }
        List<Element> children = node.Children;
        if (children != null) {
            foreach (var child in children) {
                this.SyncContentPresenters(target, child);
            }
        }
    }

    /// <summary>
    /// 递归扫描模板树，对带 TemplateBinding 标记的元素执行宿主→模板属性同步。
    /// 扫描基于已挂载的树根进行——代码构建的 VisualTree 与 Instantiate 工厂
    /// 产出的实例树（工厂内自行挂标）同样生效。
    /// </summary>
    private void SyncTemplateBindings(Control host, Element node) {
        if (node == null) {
            return;
        }
        string hostPropertyName = node.GetAttachedString(TemplateBindingPropertyKey, "");
        if (hostPropertyName.Length > 0) {
            this.ApplyTemplateBinding(host, node, hostPropertyName);
        }
        List<Element> children = node.Children;
        if (children != null) {
            foreach (var child in children) {
                this.SyncTemplateBindings(host, child);
            }
        }
    }

    /// <summary>
    /// 单条 TemplateBinding 同步：宿主属性值写入目标元素属性（字符串 DP 先行）。
    /// "Content" 特殊分派——宿主 Content 为 variant，经 ContentHelper 提取呈现
    /// 文本写入目标 Text DP（Button.Content → 模板 TextBlock.Text 主场景）；
    /// 其余属性名按宿主/目标各自 ResolveProperty 解析的同名 string DP 读写，
    /// 宿主属性非 string DP 或目标无同名 string DP 时静默跳过。
    /// </summary>
    private void ApplyTemplateBinding(Control host, Element target, string hostPropertyName) {
        if (hostPropertyName == "Content") {
            if (host is ContentControl) {
                ContentControl contentHost = (ContentControl)host;
                string text = ContentHelper.TextOrEmpty(contentHost.Content);
                this.WriteTargetString(target, "Text", text);
            }
            return;
        }
        object hostDp = host.ResolveProperty(hostPropertyName);
        if (hostDp is DependencyProperty<string>) {
            DependencyProperty<string> dp = (DependencyProperty<string>)hostDp;
            string value = host.GetValue<string>(dp);
            this.WriteTargetString(target, hostPropertyName, value);
            return;
        }
        if (hostDp is DependencyProperty<Brush>) {
            DependencyProperty<Brush> brushDp = (DependencyProperty<Brush>)hostDp;
            this.WriteTargetBrush(target, hostPropertyName, host.GetValue<Brush>(brushDp));
        }
    }

    /// <summary>把字符串值写入目标元素按名解析的 string DP；未命中或非 string DP 静默跳过。</summary>
    private void WriteTargetString(Element target, string propertyName, string value) {
        object targetDp = target.ResolveProperty(propertyName);
        if (targetDp is DependencyProperty<string>) {
            target.SetValue<string>((DependencyProperty<string>)targetDp, value);
        }
    }

    /// <summary>把 Brush 值写入目标元素按名解析的 Brush DP；未命中或非 Brush DP 静默跳过。</summary>
    private void WriteTargetBrush(Element target, string propertyName, Brush value) {
        object targetDp = target.ResolveProperty(propertyName);
        if (targetDp is DependencyProperty<Brush>) {
            target.SetValue<Brush>((DependencyProperty<Brush>)targetDp, value);
        }
    }

    /// <summary>
    /// 建立宿主侧变更监听：清理该宿主旧记录（重复套用/换模板——旧树已卸载，
    /// 旧订阅随之作废），收集模板树内全部绑定标记涉及的宿主 DP 名并逐个订阅。
    /// 树内无任何标记时不登记记录（省订阅，重同步无对象）。
    /// </summary>
    private void WatchHostChanges(Control host, Element root) {
        TemplateBindingWatcher.DetachHost(host);
        List<string> names = new List<string>();
        this.CollectTemplateBindingNames(root, names);
        if (names.Count == 0) {
            return;
        }
        TemplateBindingRecord binding = new TemplateBindingRecord(this, host, root);
        TemplateBindingWatcher.Put(binding);
        TemplateBindingWatcher.SetActiveHost(host);
        foreach (var name in names) {
            this.WatchHostProperty(host, name);
        }
    }

    /// <summary>
    /// 订阅单个宿主 DP 的变更通知，通道与 ApplyTemplateBinding 的读取分派
    /// 一致："Content" 走 ContentControl.ContentProperty（variant Signal），
    /// 其余按宿主 ResolveProperty 命中的 string/Brush DP。回调一律静态方法组
    /// **直接传参** OnChanged（Action&lt;T,T&gt;）——**禁止经 Subscribe**：其内部
    /// 以 `(old, new) => handler(new)` 包装 lambda 捕获栈上 handler 槽，闭包
    /// 跨函数逃逸后悬垂 → AV（多宿主实测）；OnChanged 直挂 bare fn ptr
    /// 零闭包，经 Watcher._activeHost 路由重同步。
    ///
    /// **不用块内方法组委托变量**（`Action<T,T> h = Watcher.OnX;`）：编译器
    /// 对块级 Let 的方法组委托脱糖有缺陷（CD-28，运行时委托槽垃圾 → 回调
    /// 触发崩溃）；直接传方法组表达式为既有正道（ItemsControl 同源约束）。
    /// </summary>
    private void WatchHostProperty(Control host, string name) {
        if (name == "Content") {
            if (host is ContentControl) {
                ContentControl contentHost = (ContentControl)host;
                Signal<Content> signal = contentHost.Observe<Content>(ContentControl.ContentProperty);
                signal.OnChanged(TemplateBindingWatcher.OnHostContentChanged);
            }
            return;
        }
        object hostDp = host.ResolveProperty(name);
        if (hostDp is DependencyProperty<string>) {
            DependencyProperty<string> dp = (DependencyProperty<string>)hostDp;
            Signal<string> signal = host.Observe<string>(dp);
            signal.OnChanged(TemplateBindingWatcher.OnHostStringChanged);
            return;
        }
        if (hostDp is DependencyProperty<Brush>) {
            DependencyProperty<Brush> brushDp = (DependencyProperty<Brush>)hostDp;
            Signal<Brush> signal = host.Observe<Brush>(brushDp);
            signal.OnChanged(TemplateBindingWatcher.OnHostBrushChanged);
        }
    }

    /// <summary>递归收集模板树内绑定标记的宿主 DP 名（去重——同名多元素只订阅一次）。</summary>
    private void CollectTemplateBindingNames(Element node, List<string> names) {
        if (node == null) {
            return;
        }
        string hostPropertyName = node.GetAttachedString(TemplateBindingPropertyKey, "");
        if (hostPropertyName.Length > 0 && !names.Contains(hostPropertyName)) {
            names.Add(hostPropertyName);
        }
        List<Element> children = node.Children;
        if (children != null) {
            foreach (var child in children) {
                this.CollectTemplateBindingNames(child, names);
            }
        }
    }

    /// <summary>对记录的（宿主，模板树）重跑全量同步：ContentPresenter + TemplateBinding。</summary>
    internal void ResyncTree(Control host, Element root) {
        this.SyncContentPresenters(host, root);
        this.SyncTemplateBindings(host, root);
    }
}

/// <summary>
/// 模板绑定监听记录：一次 ApplyTo 建立的（模板，宿主，模板树根）三元组。
/// 宿主 DP 变更回调经静态 TemplateBindingWatcher 按 id 定位到本记录后执行
/// 全树重同步。作废采用记录级置 null 哨兵（不逐 token 退订——Signal&lt;T&gt;
/// 泛型无统一非泛型 Unsubscribe 通道；死回调触发后查表得 null 即空转返回，
/// 为已权衡成本），List 下标即 id 恒稳定。
/// </summary>
internal class TemplateBindingRecord {
    /// <summary> owning 模板实例（重同步执行体）。</summary>
    public ControlTemplate Template;

    /// <summary>绑定宿主控件。</summary>
    public Control Host;

    /// <summary>模板视觉树根（重同步扫描起点）。</summary>
    public Element Root;

    public TemplateBindingRecord(ControlTemplate template, Control host, Element root) {
        this.Template = template;
        this.Host = host;
        this.Root = root;
    }

    /// <summary>对（宿主，模板树）重跑全量同步（经模板实例 internal 入口）。</summary>
    public void Resync() {
        if (this.Template != null && this.Host != null && this.Root != null) {
            this.Template.ResyncTree(this.Host, this.Root);
        }
    }
}

/// <summary>
/// 模板绑定监视注册表：绑定记录的静态登记、单活跃宿主路由与手动重同步
/// 入口。Signal 订阅回调为静态方法组（无捕获闭包，跨函数逃逸安全），
/// 经 `_activeHost` 路由——**单活跃宿主**约束（最后 ApplyTo 的宿主自动
/// 重同步；多宿主全自动并发订阅依赖编译器逃逸闭包修复，非活跃宿主经
/// ResyncHost 手动重同步，见文件头诚实标注）。记录不物理删除，作废置
/// null 哨兵（Signal.Unsubscribe 同款）。
/// </summary>
internal class TemplateBindingWatcher {
    private static List<object> _records;

    private static Control _activeHost;

    /// <summary>登记绑定记录（List 只增不删，下标恒稳定）。</summary>
    public static void Put(TemplateBindingRecord binding) {
        if (_records == null) {
            _records = new List<object>();
        }
        _records.Add(binding);
    }

    /// <summary>设置自动重同步的活跃宿主（每次 ApplyTo 建立绑定后调用）。</summary>
    public static void SetActiveHost(Control host) {
        _activeHost = host;
    }

    /// <summary>宿主 Content（variant）变更回调——重同步活跃宿主的模板树。
    /// 双参（old, new）签名对齐 Signal.OnChanged 直挂（不经 Subscribe 包装，
    /// 见 WatchHostProperty 注释）。</summary>
    public static void OnHostContentChanged(Content oldValue, Content newValue) {
        Control host = _activeHost;
        if (host != null) {
            TemplateBindingWatcher.ResyncHost(host);
        }
    }

    /// <summary>宿主 string DP 变更回调——重同步活跃宿主的模板树（双参签名
    /// 对齐 Signal.OnChanged 直挂）。</summary>
    public static void OnHostStringChanged(string oldValue, string newValue) {
        Control host = _activeHost;
        if (host != null) {
            TemplateBindingWatcher.ResyncHost(host);
        }
    }

    /// <summary>宿主 Brush DP 变更回调——重同步活跃宿主的模板树（双参签名
    /// 对齐 Signal.OnChanged 直挂）。</summary>
    public static void OnHostBrushChanged(Brush oldValue, Brush newValue) {
        Control host = _activeHost;
        if (host != null) {
            TemplateBindingWatcher.ResyncHost(host);
        }
    }

    /// <summary>对该宿主的全部活记录重跑同步（自动回调路由 + RefreshBindings 手动入口）。</summary>
    public static void ResyncHost(Control host) {
        if (_records == null || host == null) {
            return;
        }
        foreach (var entry in _records) {
            TemplateBindingRecord binding = (TemplateBindingRecord)entry;
            if (binding != null && binding.Host == host) {
                binding.Resync();
            }
        }
    }

    /// <summary>
    /// 作废该宿主的全部记录（重复 ApplyTo / 换模板时——旧树已卸载，旧订阅
    /// 不应再触发重同步）。置 null 哨兵而非物理删除，保留下标稳定性。
    /// </summary>
    public static void DetachHost(Control host) {
        if (_records == null || host == null) {
            return;
        }
        for (int i = 0; i < _records.Count; i++) {
            TemplateBindingRecord binding = (TemplateBindingRecord)_records[i];
            if (binding != null && binding.Host == host) {
                _records[i] = null;
            }
        }
    }
}
