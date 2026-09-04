// RFC 037 M3: Arc.UI.Styling — StyleManager 样式管理器。
//
// 统一应用入口 ApplyAllStyles（优先级递增两趟，后应用者覆盖同名属性）：
//   1. 隐式——无 Key 样式按 TargetType 命中元素类型名；
//   2. 显式——arml `Style={StaticResource K1, K2}` 多资源绑定脱糖产物，
//      依携带形态直达：窗口内键 codegen 定型为 Style 对象（多键为定型
//      对象列表，零字符串查找）；App 域键持请求键字符串（逗号分隔组合
//      表达式），应用期经解析链逐键查找后套用。x:Key 统一资源键，
//      无独立样式类体系。
//
// 解析域链（BuildLookupChain）：临时包装字典按 [fallback, primary] 合并序
// 构建，逆序查找使 primary（窗口/宿主域）优先、fallback（App 全局域）兜底
// ——WPF「元素→窗口→App」资源查找链同构。BasedOn 父样式解析、显式键引用、
// Setter {StaticResource} 值解析统一走此链，跨主题/合并字典可达。
//
// 属性触发器生命周期（WPF Trigger 进入/退出语义）：EvaluateTriggers 在本实例
// 侧维护 (元素引用, 触发器引用) → 激活记录；条件未命中→命中时先快照触发
// Setters 将写 DP 的旧值再应用，命中→未命中时回写快照——条件失效即恢复
// 原值，不残留触发态样式。
//
// 与 VisualStateManager 的分工边界（RFC 037 §1 双轨禁令）：交互态视觉
// （hover/pressed/focus/disabled/checked/selected）不经此通道，唯一正道是
// VisualStateManager 的 internal 强类型配方（态反馈唯一来源）；本通道仅表达
// 通用属性条件样式。详见 Trigger.as 头注释。

namespace Arc.UI.Styling;

using Arc.Collections;
using Arc.UI;
using Arc.UI.Media;

/// <summary>样式管理器。</summary>
public class StyleManager {
    private StyleEvaluator _evaluator;

    /// <summary>
    /// 触发器激活记录表（进入/退出语义的状态载体）：键为元素引用 + 触发器
    /// 引用。引用类型键的 Dictionary 哈希不可靠（codegen 载荷边界），采用线性
    /// 表 + 引用相等（==）遍历——记录量级为「每元素每触发器一条」，可接受。
    /// </summary>
    private List<TriggerActivation> _activations;

    public StyleManager() {
        _evaluator = new StyleEvaluator();
        _activations = new List<TriggerActivation>();
    }

    /// <summary>
    /// 统一样式应用入口：遍历视觉树，对每个元素按优先级递增两趟应用样式
    /// （隐式 → 显式）。primary 为元素就近域（窗口/宿主局部字典），
    /// fallback 为应用全局域（primary 未命中兜底）；两者均可为 null。
    /// </summary>
    public void ApplyAllStyles(Element root, ResourceDictionary primary, ResourceDictionary fallback) {
        if (root == null) {
            return;
        }
        List<Style> styles = new List<Style>();
        if (fallback != null) {
            fallback.CollectStyles(styles);
        }
        if (primary != null) {
            primary.CollectStyles(styles);
        }
        ResourceDictionary chain = this.BuildLookupChain(primary, fallback);
        this.WalkApply(root, styles, chain);
    }

    /// <summary>
    /// 键解析域链：临时包装字典按 [fallback, primary] 合并序构建——逆序查找
    /// 先查 primary 再查 fallback，primary 优先、fallback 兜底。不污染调用方
    /// 字典（MergedDictionaries 只读引用）。
    /// </summary>
    private ResourceDictionary BuildLookupChain(ResourceDictionary primary, ResourceDictionary fallback) {
        ResourceDictionary chain = new ResourceDictionary();
        if (fallback != null) {
            chain.MergedDictionaries.Add(fallback);
        }
        if (primary != null) {
            chain.MergedDictionaries.Add(primary);
        }
        return chain;
    }

    private void WalkApply(Element element, List<Style> styles, ResourceDictionary chain) {
        if (element == null || element.TypeName == null) {
            return;
        }

        this.ApplyStylesToElement(element, styles, chain);

        // RFC 037: VisualHost 内层子树不接收宿主 ResourceDictionary 样式
        if (element.TypeName == "VisualHost") {
            return;
        }

        List<Element> children = element.Children;
        if (children != null) {
            foreach (var child in children) {
                this.WalkApply(child, styles, chain);
            }
        }
    }

    /// <summary>
    /// 对单个元素按优先级递增两趟应用样式（后应用者覆盖同名属性）：
    /// 隐式（无 Key 且 TargetType 命中，同 TargetType 后加者胜）→
    /// 显式（arml `Style={StaticResource K1, K2}` 多资源绑定脱糖产物，依
    /// 携带形态直达：定型对象 / 定型对象列表 / 请求键字符串——显式意图
    /// 优先级最高）。x:Key 统一资源键，无独立样式类体系。
    /// </summary>
    private void ApplyStylesToElement(Element element, List<Style> styles, ResourceDictionary chain) {
        foreach (var s in styles) {
            bool isEmptyKey = (s.Key == null || s.Key == "");
            if (isEmptyKey && s.Matches(element)) {
                this.ApplyStyleChain(element, s, chain);
            }
        }
        object styleRef = this.ReadStyleReference(element);
        if (styleRef is Style) {
            this.ApplyStyleChain(element, (Style)styleRef, chain);
        } else if (styleRef is List<Style>) {
            List<Style> resolved = (List<Style>)styleRef;
            for (int i = 0; i < resolved.Count; i++) {
                this.ApplyStyleChain(element, resolved[i], chain);
            }
        } else if (styleRef is string) {
            string[] keys = ((string)styleRef).Split(",");
            for (int i = 0; i < keys.Length; i++) {
                string key = keys[i].Trim();
                if (key == "") {
                    continue;
                }
                Style explicitStyle = chain.LookupStyle(key);
                if (explicitStyle != null) {
                    this.ApplyStyleChain(element, explicitStyle, chain);
                }
            }
        }
    }

    /// <summary>读取元素显式样式引用（定型对象 / 列表 / 请求键字符串）；非 FrameworkElement 返回 null。</summary>
    private object ReadStyleReference(Element element) {
        if (element is FrameworkElement) {
            return ((FrameworkElement)element).Style;
        }
        return null;
    }

    /// <summary>
    /// 应用样式（含 BasedOn 继承链：先父后子；父样式经解析链键查找，可跨
    /// MergedDictionaries/主题命中）。visited 按引用防环——编译期 verify 已检
    /// 声明环，此处为程序化构造样式的运行时兜底；生命周期限于单条链（同一
    /// 父样式被多个子样式分别继承时须重复应用基座）。
    /// </summary>
    private void ApplyStyleChain(Element element, Style style, ResourceDictionary resources) {
        this.ApplyStyleChainVisited(element, style, resources, new List<Style>());
    }

    private void ApplyStyleChainVisited(Element element, Style style, ResourceDictionary resources, List<Style> visited) {
        if (element == null || style == null) {
            return;
        }
        foreach (var v in visited) {
            if (v == style) {
                return;
            }
        }
        visited.Add(style);
        if (style.BasedOn != null && style.BasedOn != "") {
            Style parentStyle = resources.LookupStyle(style.BasedOn);
            if (parentStyle != null) {
                this.ApplyStyleChainVisited(element, parentStyle, resources, visited);
            }
        }
        _evaluator.ApplySetters(element, style, resources);
        this.EvaluateTriggers(element, style, resources);
    }

    /// <summary>
    /// 评估样式属性触发器（WPF Style.Triggers 进入/退出语义）：按元素当前
    /// 属性值判定各 Trigger 条件——未命中→命中：先快照触发 Setters 将写 DP
    /// 的旧值再应用（覆盖基础同名属性）；命中→未命中：回写快照恢复原值；
    /// 条件状态未变则不动（触发生效期间程序化写入的值不被重复评估冲掉）。
    /// 在基础 Setters 之后评估——触发 Setter 覆盖基础同名属性，且条件读取的
    /// 是基础 Setter 写入后的值。条件载荷按 DP 运行时类型分派：string 相等
    /// 比较 / bool（"True"/"False"）/ int（十进制字面量），解析失败不命中。
    /// 交互态视觉不经此通道——归 VisualStateManager（RFC 037 §1，双轨禁令）。
    /// </summary>
    public void EvaluateTriggers(Element element, Style style, ResourceDictionary resources) {
        if (element == null || style == null || style.Triggers == null) {
            return;
        }
        foreach (var trigger in style.Triggers) {
            if (trigger == null) {
                continue;
            }
            TriggerActivation active = this.FindActivation(element, trigger);
            if (this.TriggerMatches(element, trigger)) {
                if (active == null) {
                    this.EnterTrigger(element, trigger, resources);
                }
            } else {
                if (active != null) {
                    this.ExitTrigger(active);
                }
            }
        }
    }

    /// <summary>定位 (元素, 触发器) 的激活记录；未激活返回 null。引用相等即同键。</summary>
    private TriggerActivation FindActivation(Element element, Trigger trigger) {
        foreach (var activation in _activations) {
            if (activation.Element == element && activation.Trigger == trigger) {
                return activation;
            }
        }
        return null;
    }

    /// <summary>进入触发：快照被写 DP 旧值 → 应用触发 Setters → 记录激活。</summary>
    private void EnterTrigger(Element element, Trigger trigger, ResourceDictionary resources) {
        TriggerActivation activation = new TriggerActivation(element, trigger);
        if (trigger.Setters != null) {
            foreach (var s in trigger.Setters) {
                if (s != null && s.Property != null) {
                    this.SnapshotSetter(element, s, activation);
                }
            }
        }
        // 触发 Setters 装载为匿名 Style 复用 StyleEvaluator.ApplySetters
        // 公共入口——值应用（DP 动态解析 + 类型分派）与基础样式完全
        // 同轨，本类零值应用逻辑。
        Style triggerStyle = new Style();
        triggerStyle.Setters = trigger.Setters;
        _evaluator.ApplySetters(element, triggerStyle, resources);
        _activations.Add(activation);
    }

    /// <summary>退出触发：回写快照旧值（撤销触发 Setters），移除激活记录。</summary>
    private void ExitTrigger(TriggerActivation activation) {
        activation.Rollback();
        _activations.Remove(activation);
    }

    /// <summary>
    /// 快照单个触发 Setter 将写 DP 的当前值——载荷分派集与
    /// StyleEvaluator.ApplyDp 完全一致（回退保真要求快照覆盖触发可写的
    /// 全部载荷类型）。Template 载荷 Setter 走 Control.Template 包装通道
    /// （非 DP 标量写），不入快照——模板级进退属 EnterActions/ExitActions
    /// 范畴，本生命周期不覆盖。
    /// </summary>
    private void SnapshotSetter(Element element, Setter s, TriggerActivation activation) {
        object dp = element.ResolveProperty(s.Property);
        if (dp == null) {
            return;
        }
        if (dp is DependencyProperty<string>) {
            DependencyProperty<string> stringDp = (DependencyProperty<string>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<string>(stringDp,
                element.GetValue<string>(stringDp)));
            return;
        }
        if (dp is DependencyProperty<Brush>) {
            DependencyProperty<Brush> brushDp = (DependencyProperty<Brush>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<Brush>(brushDp,
                element.GetValue<Brush>(brushDp)));
            return;
        }
        if (dp is DependencyProperty<bool>) {
            DependencyProperty<bool> boolDp = (DependencyProperty<bool>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<bool>(boolDp,
                element.GetValue<bool>(boolDp)));
            return;
        }
        if (dp is DependencyProperty<int>) {
            DependencyProperty<int> intDp = (DependencyProperty<int>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<int>(intDp,
                element.GetValue<int>(intDp)));
            return;
        }
        if (dp is DependencyProperty<double>) {
            DependencyProperty<double> numberDp = (DependencyProperty<double>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<double>(numberDp,
                element.GetValue<double>(numberDp)));
            return;
        }
        if (dp is DependencyProperty<Orientation>) {
            DependencyProperty<Orientation> orientationDp = (DependencyProperty<Orientation>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<Orientation>(orientationDp,
                element.GetValue<Orientation>(orientationDp)));
            return;
        }
        if (dp is DependencyProperty<Stretch>) {
            DependencyProperty<Stretch> stretchDp = (DependencyProperty<Stretch>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<Stretch>(stretchDp,
                element.GetValue<Stretch>(stretchDp)));
            return;
        }
        if (dp is DependencyProperty<HorizontalAlignment>) {
            DependencyProperty<HorizontalAlignment> horizontalDp =
                (DependencyProperty<HorizontalAlignment>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<HorizontalAlignment>(horizontalDp,
                element.GetValue<HorizontalAlignment>(horizontalDp)));
            return;
        }
        if (dp is DependencyProperty<VerticalAlignment>) {
            DependencyProperty<VerticalAlignment> verticalDp =
                (DependencyProperty<VerticalAlignment>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<VerticalAlignment>(verticalDp,
                element.GetValue<VerticalAlignment>(verticalDp)));
            return;
        }
        if (dp is DependencyProperty<ScrollBarVisibility>) {
            DependencyProperty<ScrollBarVisibility> scrollBarDp =
                (DependencyProperty<ScrollBarVisibility>)dp;
            activation.Snapshots.Add(new TriggerSnapshot<ScrollBarVisibility>(scrollBarDp,
                element.GetValue<ScrollBarVisibility>(scrollBarDp)));
            return;
        }
    }

    /// <summary>
    /// 触发条件判定：按条件 DP 运行时载荷类型分派——string 直接相等比较；
    /// bool/int 经 Trigger.TryParse* 解析条件值（失败不命中，防御边界）。
    /// 其余载荷类型不支持条件判定（不命中）。
    /// </summary>
    private bool TriggerMatches(Element element, Trigger trigger) {
        if (trigger.Property == null || trigger.Property == "") {
            return false;
        }
        object dp = element.ResolveProperty(trigger.Property);
        if (dp is DependencyProperty<string>) {
            DependencyProperty<string> stringDp = (DependencyProperty<string>)dp;
            return element.GetValue<string>(stringDp) == trigger.Value;
        }
        if (dp is DependencyProperty<Brush>) {
            DependencyProperty<Brush> brushDp = (DependencyProperty<Brush>)dp;
            Brush brush = element.GetValue<Brush>(brushDp);
            if (brush == null) { return false; }
            return brush.ToHex() == trigger.Value;
        }
        if (dp is DependencyProperty<bool>) {
            DependencyProperty<bool> boolDp = (DependencyProperty<bool>)dp;
            bool condition;
            if (!Trigger.TryParseBool(trigger.Value, out condition)) {
                return false;
            }
            return element.GetValue<bool>(boolDp) == condition;
        }
        if (dp is DependencyProperty<int>) {
            DependencyProperty<int> intDp = (DependencyProperty<int>)dp;
            int condition;
            if (!Trigger.TryParseInt(trigger.Value, out condition)) {
                return false;
            }
            return element.GetValue<int>(intDp) == condition;
        }
        return false;
    }
}

/// <summary>
/// 触发器激活记录：条件命中时快照「触发 Setters 将写 DP」的旧值；条件失效
/// （再次评估未命中）时逐条回写——WPF Trigger 退出语义的恢复源。
/// </summary>
internal class TriggerActivation {
    /// <summary>激活目标元素（引用相等定位键之一）。</summary>
    public Element Element;

    /// <summary>激活的触发器（引用相等定位键之一）。</summary>
    public Trigger Trigger;

    /// <summary>DP 旧值快照集合（元素为 TriggerSnapshot&lt;T&gt;，回写时按运行时类型分派）。</summary>
    public List<object> Snapshots;

    public TriggerActivation(Element element, Trigger trigger) {
        this.Element = element;
        this.Trigger = trigger;
        this.Snapshots = new List<object>();
    }

    /// <summary>
    /// 回写全部快照：走 SetStyleValue（样式通道）与触发写入对称——若目标 DP
    /// 在触发生效期间被程序化赋本地值，本地值优先、不被回写冲掉（WPF DP 优先级）。
    /// </summary>
    public void Rollback() {
        if (this.Element == null || this.Snapshots == null) {
            return;
        }
        foreach (var snap in this.Snapshots) {
            if (snap is TriggerSnapshot<string>) {
                TriggerSnapshot<string> stringSnap = (TriggerSnapshot<string>)snap;
                this.Element.SetStyleValue<string>(stringSnap.Prop, stringSnap.OldValue);
            } else if (snap is TriggerSnapshot<Brush>) {
                TriggerSnapshot<Brush> brushSnap = (TriggerSnapshot<Brush>)snap;
                this.Element.SetStyleValue<Brush>(brushSnap.Prop, brushSnap.OldValue);
            } else if (snap is TriggerSnapshot<bool>) {
                TriggerSnapshot<bool> boolSnap = (TriggerSnapshot<bool>)snap;
                this.Element.SetStyleValue<bool>(boolSnap.Prop, boolSnap.OldValue);
            } else if (snap is TriggerSnapshot<int>) {
                TriggerSnapshot<int> intSnap = (TriggerSnapshot<int>)snap;
                this.Element.SetStyleValue<int>(intSnap.Prop, intSnap.OldValue);
            } else if (snap is TriggerSnapshot<double>) {
                TriggerSnapshot<double> numberSnap = (TriggerSnapshot<double>)snap;
                this.Element.SetStyleValue<double>(numberSnap.Prop, numberSnap.OldValue);
            } else if (snap is TriggerSnapshot<Orientation>) {
                TriggerSnapshot<Orientation> orientationSnap = (TriggerSnapshot<Orientation>)snap;
                this.Element.SetStyleValue<Orientation>(orientationSnap.Prop, orientationSnap.OldValue);
            } else if (snap is TriggerSnapshot<Stretch>) {
                TriggerSnapshot<Stretch> stretchSnap = (TriggerSnapshot<Stretch>)snap;
                this.Element.SetStyleValue<Stretch>(stretchSnap.Prop, stretchSnap.OldValue);
            } else if (snap is TriggerSnapshot<HorizontalAlignment>) {
                TriggerSnapshot<HorizontalAlignment> horizontalSnap =
                    (TriggerSnapshot<HorizontalAlignment>)snap;
                this.Element.SetStyleValue<HorizontalAlignment>(horizontalSnap.Prop, horizontalSnap.OldValue);
            } else if (snap is TriggerSnapshot<VerticalAlignment>) {
                TriggerSnapshot<VerticalAlignment> verticalSnap =
                    (TriggerSnapshot<VerticalAlignment>)snap;
                this.Element.SetStyleValue<VerticalAlignment>(verticalSnap.Prop, verticalSnap.OldValue);
            } else if (snap is TriggerSnapshot<ScrollBarVisibility>) {
                TriggerSnapshot<ScrollBarVisibility> scrollBarSnap =
                    (TriggerSnapshot<ScrollBarVisibility>)snap;
                this.Element.SetStyleValue<ScrollBarVisibility>(scrollBarSnap.Prop, scrollBarSnap.OldValue);
            }
        }
    }
}

/// <summary>
/// 单 DP 旧值快照（强类型载荷，避免 variant 扩展与 object 装箱）。泛型模板
/// 构造体用裸字段赋值——this. 解析不支持泛型类模板（RFC 018 M4-1，与
/// BindingOperations.ValueRecord&lt;T&gt; 同因）。
/// </summary>
internal class TriggerSnapshot<T> {
    /// <summary>被快照的依赖属性。</summary>
    public DependencyProperty<T> Prop;

    /// <summary>触发应用前的旧值（回退恢复源）。</summary>
    public T OldValue;

    public TriggerSnapshot(DependencyProperty<T> prop, T oldValue) {
        Prop = prop;
        OldValue = oldValue;
    }
}
