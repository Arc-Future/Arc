// RFC 037 D2.1 / D9.2 / D3.1 + RFC 037 D6: Arc.UI —— .arml 运行时基础类型。
//
// Element 是所有 .arml 元素在运行时的统一基类，承担：
//   - 逻辑树/可视树节点身份
//   - 响应式属性存储宿主（RFC 037 D6：Dictionary<long, object> + Signal<T> 后端）
//   - 统一生命周期钩子（OnInitialized/OnLoaded/OnUnloaded）
//
// RFC 037 D6 属性存储模型（WPF 同构 + 零装箱）：
//   - 元素实例持有 `Dictionary<long, object> _propertyStorage` 字典
//   - DependencyProperty<T>.Id 作为 key，Signal<T> 引用（boxed 为 object）作 value
//   - GetValue<T> / SetValue<T> / Observe<T> 通过 Id 查找 Signal<T>
//   - Signal<T> 是 class（引用类型），存入 object 字段仅存引用——零装箱
//   - Signal<T>._value 字段类型为 T，值类型 T 直接存储在 Signal 实例内——零装箱
//
// 用户编码范式（WPF 同构）：
//   ```arc
//   public static DependencyProperty<double> WidthProperty =
//       RegisterProperty<double>(nameof(Width), typeof(Window), 0.0);
//
//   public double Width {
//       get { return this.GetValue<double>(WidthProperty); }
//       set { this.SetValue<double>(WidthProperty, value); }
//   }
//   ```
//   - 用户只写 DP 元数据 + 属性 wrapper 两件套
//   - Signal<T> 后端由 Element 基类内部维护，用户不感知
//   - 渲染层通过 this.Observe<T>(WidthProperty) 获取 Signal<T> 订阅局部刷新
//   - Signal<T> 已迁移至 `Arc` 根命名空间（响应式原语通用化）
//
// 生命周期（对标 WPF FrameworkElement）：
//   1. 构造（new）——_propertyStorage 初始化为空 Dictionary
//   2. InitializeComponent()——由 codegen 在 partial 派生类中 override，
//      设置从 .arml 解析出的属性（Title/Width/Height/Content 等），
//      属性 set 调用 SetValue<T> 触发 Signal<T>.Set 通知订阅者
//   3. OnInitialized()——元素已初始化、属性已设置，可被访问
//   4. OnLoaded()——元素已挂载到逻辑/可视树，准备渲染
//   5. OnUnloaded()——元素从树中移除，可释放资源
//
// 编码规范（Arc 标准库典范）：
//   - private/protected 下划线字段（_xxx）裸访问，不带 `this.` 前缀
//   - public 成员字段、属性、方法访问带 `this.` 前缀（提高识别度）
//   - 例：`_propertyStorage` 裸访问；`this.AddChild(...)`/`this.OnLoaded()` 带 this.
//
// **命名空间归属**：本文件位于 std/UI/Markup/ 子目录，但归属到 `Arc.UI`
// 根命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」+ 命名空间分层原则：
// 基类放根命名空间，派生实现在子命名空间）。Element 是所有 UI 元素的基类，
// 必须在 `Arc.UI` 根命名空间，使派生类（Arc.UI.Components.Window 等）
// 只需 `using Arc.UI;` 即可访问基类——避免基类在子命名空间、派生在另一
// 子命名空间导致双方都需 `using Arc.UI.Markup` 的反向引用反模式。
//
// 架构红线（RFC 026 D8.1）：
//   - 本文件属于 Arc.UI 子库（std/UI/），由 Arc 语言实现
//   - 不依赖编译器核心 7 crate
//   - 通过 rt_ui_* ABI 与渲染后端交互（D10.1）

namespace Arc.UI;

using Arc.Collections;
using Arc.UI.Internal;

/// <summary>
/// 所有 .arml 元素的运行时基类，承载逻辑树、响应式属性存储与生命周期钩子。
/// </summary>
public class Element {
    /// <summary>元素在逻辑树中的父节点；根元素为 null。</summary>
    ///
    /// 弱引用（RFC 005 §4.2：断 Parent↔Children 双向强引用环）。
    /// get 经 <see cref="Weak{T}.TryGet"/> 提升为强引用（父已回收时返回 null）；
    /// set 存储 <see cref="Weak{T}"/>（null 表示清除）。
    private Weak<Element> _weakParent;

    /// <summary>元素在逻辑树中的父节点（弱引用语义）。</summary>
    public Element? Parent {
        get {
            if (_weakParent == null) {
                return null;
            }
            return _weakParent.TryGet();
        }
        set {
            if (value == null) {
                _weakParent = null;
            } else {
                _weakParent = new Weak<Element>(value);
            }
        }
    }

    /// <summary>元素在 .arml 中声明的名称（x:Name 属性值）。</summary>
    public string Name;

    /// <summary>
    /// 元素类型名（如 "Button"/"Window"/"Text"）。
    /// 由派生类构造函数设置，供 StyleEvaluator 属性分派。
    /// </summary>
    public string TypeName;

    /// <summary>
    /// 元素运行时类型（CLR/Arc 类型身份）。由派生类构造函数赋 `typeof(具体类型)`，
    /// 供沿类型链按名解析依赖属性（Element.ResolveProperty → StyleEvaluator 动态分派）。
    /// 与 <see cref="TypeName"/>（声明式/逻辑身份字符串，供隐式样式匹配与 Grid 伪节点）
    /// 职责互补：TypeName 可为无真实类的伪节点（ColumnDefinitions）命名，Type 仅对
    /// 真实类有效。
    /// </summary>
    public Type Type;

    /// <summary>
    /// 子元素集合（逻辑树）。.arml 中声明的子元素由 codegen 实例化后通过
    /// <see cref="AddChild"/> 添加到本集合。
    /// </summary>
    public List<Element> Children;

    private Dictionary<string, double> _attachedNumbers;
    private Dictionary<string, string> _attachedStrings;

    /// <summary>G2 卸载退订登记表（惰性创建；null = 空表）。</summary>
    private List<Action> _detachActions;

    // ===== Element 基础 DP（Arc 简化：合并 WPF DependencyObject 角色）=====
    //
    // DataContext 是 WPF 中 FrameworkElement 才有的 DP，但 Arc 的 Element
    // 已合并 DependencyObject 角色（无独立 DependencyObject 中间层），
    // 故把 DataContext 提前到 Element——所有元素都有 DataContext，简化
    // 数据绑定路径，避免类型转换。
    //
    // 其他 WPF FrameworkElement DP（Width/Height/Margin/Alignment/Style/
    // Resources/Tag）保留在 FrameworkElement 子类，保持层级语义清晰。

    /// <summary>DataContext 属性元数据——数据上下文（绑定系统数据源）。</summary>
    public static DependencyProperty<object> DataContextProperty =
        RegisterProperty<object>(nameof(DataContext), typeof(Element), null);

    /// <summary>
    /// 数据上下文——绑定系统的数据源。子元素未显式设置时自动继承父元素的 DataContext
    /// （由 Panel 在添加子元素时传递，或由绑定引擎在路径解析时回溯）。
    /// </summary>
    public object DataContext {
        get {
            if (this.HasLocalDataContext()) {
                return this.GetLocalDataContextValue();
            }
            return this.GetInheritedDataContextObject();
        }
        set { this.SetValue<object>(DataContextProperty, value); }
    }

    /// <summary>
    /// RFC 037 D6：依赖属性存储后端。
    ///
    /// 以 DependencyProperty&lt;T&gt;.Id 为键，Signal&lt;T&gt; 引用为值。
    /// Signal&lt;T&gt; 是 class（引用类型），存入 object 字段仅存指针——
    /// 零装箱。值类型 T 直接存储在 Signal 实例内的 Value 字段——零装箱。
    /// </summary>
    private Dictionary<long, object> _propertyStorage;

    /// <summary>已通过 SetValue（本地值/CLR 属性 setter）写入的属性 id 集合。
    /// 样式 Setter 调用 SetStyleValue 时若 key 已在此集合中则跳过——实现 WPF DP 优先级：
    /// 本地值 > 样式 Setter（高特异性样式仍可覆盖低特异性样式，因两者均走 SetStyleValue）。</summary>
    private HashSet<long> _localPropertyKeys;

    /// <summary>
    /// 环境属性继承槽（RFC 037 §4 推送式继承）：pid → 祖先 Signal 引用。
    /// 存储 Signal **引用**而非值拷贝——祖先后续 SetValue 改写同一 Signal，
    /// 全子树读值即时一致，无需重推；仅树结构变化（AddChild）时重算本槽。
    /// 读路径：_propertyStorage（本地/样式）→ 本槽（继承）→ DP 默认值。
    /// </summary>
    private Dictionary<long, object> _inheritedPropertyStorage;

    /// <summary>构造 Element，初始化 Children 与 _propertyStorage。</summary>
    public Element() {
        this.Type = typeof(Element);
        this.Children = new List<Element>();
        _propertyStorage = new Dictionary<long, object>();
        _localPropertyKeys = new HashSet<long>();
        _inheritedPropertyStorage = new Dictionary<long, object>();
        _attachedNumbers = new Dictionary<string, double>();
        _attachedStrings = new Dictionary<string, string>();
    }

    // ===== RFC 037 D6：依赖属性访问 API =====
    //
    // 三件套 GetValue<T>/SetValue<T>/Observe<T> 通过 DependencyProperty<T>.Id
    // 查询 _propertyStorage 中的 Signal<T> 引用。首次访问时按 prop.DefaultValue
    // 惰性创建 Signal<T>。
    //
    // 类型 T 由 DependencyProperty<T> 静态绑定，无运行时类型检查开销。
    // Signal<T> 是引用类型，存入 Dictionary<long, object> 不发生装箱。

    /// <summary>
    /// 读取依赖属性当前值。
    /// </summary>
    /// <typeparam name="T">属性值类型。</typeparam>
    /// <param name="prop">依赖属性元数据。</param>
    /// <returns>属性当前值；环境属性无本地/样式值时读继承槽（祖先 Signal 引用），其余返回 prop.DefaultValue。</returns>
    public T GetValue<T>(DependencyProperty<T> prop) {
        long pid = prop.Id;
        if (!_propertyStorage.ContainsKey(pid)) {
            // 环境属性（RFC 037 §4 推送式继承）：无本地/样式值时读继承槽
            //（祖先 Signal 引用，写时已向下传播），未命中回 DP 默认值。读 O(1)。
            if (_inheritedPropertyStorage.ContainsKey(pid)) {
                return ((Signal<T>)_inheritedPropertyStorage[pid]).Value;
            }
            return prop.DefaultValue;
        }
        object box = _propertyStorage[pid];
        Signal<T> signal = (Signal<T>)box;
        return signal.Value;
    }

    // ===== RFC 037 §4：环境属性推送式继承（WPF property inheritance 同构） =====
    //
    // 写时向下传播（SetValue/SetStyleValue）、挂接时自祖先重算（AddChild）：
    // 静态 ARML 树在构建期一次固化（等效编译期确定），动态变更只付受影响
    // 子树成本；传播的是 Signal 引用，祖先后续改值全子树即时一致。

    /// <summary>是否已有本地/样式值（_propertyStorage 命中）。继承传播时作为
    /// 子孙屏蔽判定：有则本元素成为子树新继承源。</summary>
    internal bool HasOwnValue(long pid) {
        return _propertyStorage.ContainsKey(pid);
    }

    /// <summary>返回 pid 的本地/样式 Signal 装载对象；未写返回 null。</summary>
    internal object OwnSignalIfSet(long pid) {
        if (_propertyStorage.ContainsKey(pid)) {
            return _propertyStorage[pid];
        }
        return null;
    }

    /// <summary>返回 pid 的继承槽 Signal 装载对象；无则 null。</summary>
    internal object InheritedSignalIfSet(long pid) {
        if (_inheritedPropertyStorage.ContainsKey(pid)) {
            return _inheritedPropertyStorage[pid];
        }
        return null;
    }

    /// <summary>写入继承槽（pid → 祖先 Signal 引用）。仅继承引擎调用。</summary>
    internal void SetInheritedSignal(long pid, object box) {
        _inheritedPropertyStorage[pid] = box;
    }

    /// <summary>
    /// 将有效值 Signal 引用向下传播：子孙无本地/样式值 → 写入其继承槽；
    /// 有 → 屏蔽并以其自身值为子树新源继续下传（最近祖先语义）。
    /// </summary>
    /// <param name="pid">属性 id。</param>
    /// <param name="box">本元素对该 pid 的有效值 Signal。</param>
    private void PushInheritedSignal(long pid, object box) {
        List<Element> children = this.Children;
        int count = children.Count;
        for (int i = 0; i < count; i++) {
            Element child = children[i];
            object childBox = box;
            if (child.HasOwnValue(pid)) {
                childBox = child.OwnSignalIfSet(pid);
            } else {
                child.SetInheritedSignal(pid, box);
            }
            child.PushInheritedSignal(pid, childBox);
        }
    }

    /// <summary>
    /// 挂接后自祖先链重算本子树继承槽：对每个已注册环境属性（注册期元数据
    /// 声明，DependencyPropertyRegistry 登记，零硬编码），取最近祖先有效值
    ///（本地/样式值优先，其次其继承槽），写入本元素继承槽并向下传播。
    /// 资源隔离边界（VisualHost）不截断环境属性——环境值流动 ≠ 资源查找。
    /// </summary>
    internal void RefreshInheritanceFromAncestors() {
        List<long> inheritedIds = DependencyPropertyRegistry.InheritedIds();
        int count = inheritedIds.Count;
        for (int i = 0; i < count; i++) {
            long pid = inheritedIds[i];
            if (this.HasOwnValue(pid)) {
                this.PushInheritedSignal(pid, this.OwnSignalIfSet(pid));
                continue;
            }
            object box = Element.FindNearestAncestorSignal(this, pid);
            if (box != null) {
                this.SetInheritedSignal(pid, box);
                this.PushInheritedSignal(pid, box);
            }
        }
    }

    /// <summary>沿 Parent 链取 pid 的最近祖先有效值 Signal（本地/样式值优先于其继承槽）；无则 null。</summary>
    private static object FindNearestAncestorSignal(Element start, long pid) {
        Element? node = start.Parent;
        while (true) {
            if (node == null) {
                return null;
            }
            object own = node.OwnSignalIfSet(pid);
            if (own != null) {
                return own;
            }
            object inherited = node.InheritedSignalIfSet(pid);
            if (inherited != null) {
                return inherited;
            }
            node = node.Parent;
        }
    }

    /// <summary>本元素是否显式 SetValue 过 DataContext（不含继承）。</summary>
    private bool HasLocalDataContext() {
        return _localPropertyKeys.Contains(DataContextProperty.Id);
    }

    /// <summary>读取本地 DataContext 槽；调用前须 HasLocalDataContext()。</summary>
    private object GetLocalDataContextValue() {
        object box = _propertyStorage[DataContextProperty.Id];
        Signal<object> signal = (Signal<object>)box;
        return signal.Value;
    }

    /// <summary>
    /// DataContext inherit DP：沿 Parent 链查找最近显式 SetValue 的祖先值。
    /// </summary>
    private object GetInheritedDataContextObject() {
        Element? node = this.Parent;
        while (true) {
            if (node == null) {
                return null;
            }
            if (node.IsDataContextBoundary()) {
                // RFC 037 §3.4：隔离边界（如 VisualHost）——宿主 DataContext 不流入内层。
                // 边界节点自身显式设置的 DataContext 仍供内层根/子节点继承；未设置则截断为 null。
                if (node.HasLocalDataContext()) {
                    return node.GetLocalDataContextValue();
                }
                return null;
            }
            if (node.HasLocalDataContext()) {
                return node.GetLocalDataContextValue();
            }
            node = node.Parent;
        }
    }

    /// <summary>
    /// 是否为 DataContext 继承边界（RFC 037 §3.4）。默认 false；派生类按需
    /// override 为 true（如 VisualHost）：沿 Parent 链向上查找 DataContext 时遇
    /// 边界节点即截断——宿主 DataContext 不流入边界内层子树；边界自身的显式
    /// DataContext 仍供内层根/子节点按 WPF 规则向下继承。
    /// </summary>
    public virtual bool IsDataContextBoundary() {
        return false;
    }

    /// <summary>
    /// 设置依赖属性值。首次 SetValue 时惰性创建 Signal&lt;T&gt; 并存入字典；
    /// 后续 SetValue 复用现有 Signal，触发其通知链。
    /// </summary>
    /// <typeparam name="T">属性值类型。</typeparam>
    /// <param name="prop">依赖属性元数据。</param>
    /// <param name="value">新值。</param>
    public void SetValue<T>(DependencyProperty<T> prop, T value) {
        long pid = prop.Id;
        bool hasBefore = _propertyStorage.ContainsKey(pid);
        if (!hasBefore) {
            Signal<T> created = new Signal<T>(prop.DefaultValue);
            _propertyStorage[pid] = created;
            created.Set(value);
        } else {
            Signal<T> signal = (Signal<T>)_propertyStorage[pid];
            signal.Set(value);
        }
        _localPropertyKeys.Add(pid);
        // 环境属性（RFC 037 §4）：写时向下传播——本元素自此成为子树继承源，
        // 后续改写同一 Signal，全子树读值即时一致。
        if (prop.Metadata != null && prop.Metadata.Inherits) {
            this.PushInheritedSignal(pid, _propertyStorage[pid]);
        }
        // A-1②：属性变更 → 标记帧泵需重绘（按需渲染；幂等——一帧多变更合并为一次绘制）。
        FramePump.Invalidate();
    }

    /// <summary>样式引擎专用：仅当属性无本地值（未被 CLR setter/codegen/user code 赋值）时
    /// 才写入，实现 WPF DP 优先级「本地值 > 样式 Setter」。同特异性层级的样式可互相覆盖
    /// （因样式写入不进入 _localPropertyKeys，高特异性后写入会覆盖低特异性）。</summary>
    public void SetStyleValue<T>(DependencyProperty<T> prop, T value) {
        long pid = prop.Id;
        if (_localPropertyKeys.Contains(pid)) {
            return;
        }
        bool hasBefore = _propertyStorage.ContainsKey(pid);
        if (!hasBefore) {
            Signal<T> created = new Signal<T>(prop.DefaultValue);
            _propertyStorage[pid] = created;
            created.Set(value);
        } else {
            Signal<T> signal = (Signal<T>)_propertyStorage[pid];
            signal.Set(value);
        }
        // 环境属性：祖先的样式值亦为有效继承源（RFC 037 §4），写时向下传播。
        if (prop.Metadata != null && prop.Metadata.Inherits) {
            this.PushInheritedSignal(pid, _propertyStorage[pid]);
        }
    }

    /// <summary>
    /// 沿本元素类型链（Type → BaseType → …）按名解析依赖属性；未命中回退全局
    /// 所有者表按名解析。
    ///
    /// 由 StyleEvaluator 动态分派调用：Setter.Property（属性名）经本方法命中
    /// 目标元素自身类型作用域内的 DependencyProperty（object 擦除视图），替代
    /// 旧式「属性名 → 硬编码 DP」switch。未命中（目标类型链上无此属性）返回 null。
    ///
    /// **两阶段语义**（动态 DP 解析架构，与 b1d21c05 设计一致）：
    /// 1. 运行时类型链（Type → BaseType → …）：真实元素确定性消歧——同名 DP
    ///    （TextBlock/TextBox 均注册 Text）自动落到元素自身类型作用域（TextBlock→
    ///    TextBlock.TextProperty、TextBox→TextBox.TextProperty），Control/Panel 均注册 Background 同理。
    /// 2. 全局 owner 表（DependencyPropertyRegistry.FindGlobal）按注册序按名回退：
    ///    覆盖 mock / TypeName 标识元素——运行时 Type 链不含 DP 所有者（如
    ///    Element + TypeName="Button" 解析 "Background" 命中 Control 作用域）。
    ///    同名 DP 由注册序（类型静态字段初始化拓扑序，确定性）稳定裁决。
    ///
    /// 返回 object：Arc 泛型模板（DependencyProperty&lt;T&gt;）不参与类型注册，无法以
    /// 非泛型基类作返回类型；调用方（StyleEvaluator）经 `is`/cast 分派值种类。
    /// </summary>
    /// <param name="name">属性名（如 "Background"、"Text"）。</param>
    /// <returns>命中的依赖属性（object）；全局未登记该名返回 null。</returns>
    public object ResolveProperty(string name) {
        Type t = this.Type;
        while (t != null) {
            object dp = DependencyPropertyRegistry.Find(t.TypeId, name);
            if (dp != null) {
                return dp;
            }
            t = t.BaseType;
        }
        // 类型链未命中 → 全局 owner 表按注册序按名回退（见本方法文档语义 2）。
        return DependencyPropertyRegistry.FindGlobal(name);
    }

    /// <summary>
    /// 获取依赖属性的 Signal&lt;T&gt; 引用——渲染层订阅入口。
    /// 首次 Observe 时按 prop.DefaultValue 惰性创建 Signal&lt;T&gt;。
    /// </summary>
    /// <typeparam name="T">属性值类型。</typeparam>
    /// <param name="prop">依赖属性元数据。</param>
    /// <returns>属性对应的 Signal 引用（可 Subscribe 注册变更回调）。</returns>
    public Signal<T> Observe<T>(DependencyProperty<T> prop) {
        if (!_propertyStorage.ContainsKey(prop.Id)) {
            Signal<T> newSignal = new Signal<T>(prop.DefaultValue);
            _propertyStorage[prop.Id] = newSignal;
            return newSignal;
        }
        object box = _propertyStorage[prop.Id];
        return (Signal<T>)box;
    }

    // ===== 生命周期钩子（对标 WPF FrameworkElement）=====
    //
    // 派生类可 override 这些方法实现自定义逻辑：
    //   - Window: OnLoaded 创建原生窗口，OnClosed 释放资源
    //   - Application: OnStartup/OnExit 控制应用启动/退出流程
    //   - 用户自定义元素：初始化数据绑定等

    /// <summary>元素已初始化、属性已设置完成时触发。</summary>
    public virtual void OnInitialized() {
        // 默认空实现；派生类按需 override
    }

    /// <summary>元素已挂载到逻辑/可视树时触发。</summary>
    public virtual void OnLoaded() {
        // 默认空实现；派生类按需 override
    }

    /// <summary>元素从逻辑/可视树移除时触发。</summary>
    public virtual void OnUnloaded() {
        // G2 卸载退订登记执行：按注册顺序遍历登记表、跳过空槽、执行后清空登记表
        // （OnUnloaded 第二次调用幂等——空表直接跳过）。登记/退订入口见 RegisterDetach。
        if (_detachActions != null) {
            int count = _detachActions.Count;
            for (int i = 0; i < count; i++) {
                Action a = _detachActions[i];
                if (a != null) {
                    a();
                }
            }
            _detachActions.Clear();
        }
        // （下行为文件既有编码的历史占位注释，本切片未改动；实际行为见上方 G2 执行逻辑。）
        // 默认空实现；派生类按需 override
    }

    /// <summary>
    /// 添加子元素到逻辑树。设置子元素的 Parent 引用并追加到
    /// <see cref="Children"/> 集合。
    /// </summary>
    /// <param name="child">待添加的子元素；不允许为 null。</param>
    public void AddChild(Element child) {
        child.Parent = this;
        this.Children.Add(child);
        // 挂接后自祖先链重算环境属性继承槽（RFC 037 §4）：静态树构建期
        // 一次固化（等效编译期确定），动态挂接即时生效。
        child.RefreshInheritanceFromAncestors();
        // A-1②：树结构变更 → 标记帧泵需重绘（按需渲染）。
        FramePump.Invalidate();
    }

    /// <summary>设置附加数值属性（Canvas.Left/Top 等；Grid.Row/Column 自 RFC 037 走 typed DependencyProperty&lt;int&gt;，不再经此路径）。</summary>
    public void SetAttachedNumber(string key, double value) {
        if (key == null || key.Length == 0) {
            return;
        }
        _attachedNumbers[key] = value;
    }

    /// <summary>读取附加数值属性；不存在时返回 defaultValue。</summary>
    public double GetAttachedNumber(string key, double defaultValue) {
        if (key == null || key.Length == 0) {
            return defaultValue;
        }
        double v = 0.0;
        if (_attachedNumbers.TryGetValue(key, out v)) {
            return v;
        }
        return defaultValue;
    }

    /// <summary>设置附加字符串属性（DockPanel.Dock 等）。</summary>
    public void SetAttachedString(string key, string value) {
        if (key == null || key.Length == 0) {
            return;
        }
        _attachedStrings[key] = value;
    }

    /// <summary>读取附加字符串属性；不存在时返回 defaultValue。</summary>
    public string GetAttachedString(string key, string defaultValue) {
        if (key == null || key.Length == 0) {
            return defaultValue;
        }
        string v = "";
        if (_attachedStrings.TryGetValue(key, out v)) {
            return v;
        }
        return defaultValue;
    }

    // ===== 卸载退订登记（G2 确定性退订原语 · RFC 037 §5.3）=====
    //
    // 问题：UI 框架引用没有及时释放——发布方（VM/集合，长生命周期）经订阅强持
    // 订阅方（UI 元素），元素永不被释放。G1（发布方不强持订阅方）+ G2（元素
    // 卸载即自动退订）的运行时落点 = 把退订动作登记到元素卸载生命周期：
    //   element.RegisterDetach(() => sig.Unsubscribe(token));
    // 元素从逻辑树移除（VisualHost.unloadInnerRoot → ch.OnUnloaded()）时统一执行。
    //
    // 边界（诚实标注）：本原语为 std 侧用户面能力——「订阅登记到卸载生命周期」
    // 成为可能；订阅点自动生成 Unsubscribe + 自动挂 OnUnloaded 的 codegen 自动
    // 配对退订属 M4/M5。派生类 override OnUnloaded 不调 base 时登记动作不执行
    // （未来 codegen 自动配对将强制调 base 或内联执行）。

    /// <summary>
    /// 注册卸载退订动作：元素从逻辑树移除（<see cref="OnUnloaded"/>）时按注册顺序执行。
    /// 典型用途：把订阅退订（<c>sig.Unsubscribe(token)</c>）登记到元素生命周期，使
    /// 「发布方不强持订阅方（G1）+ 元素卸载即自动退订（G2）」成为可能。
    /// </summary>
    /// <param name="action">待卸载时执行的退订动作；null 被忽略，同引用重复登记被去重
    /// （不同闭包实例视为不同动作——重复退订无副作用，Unsubscribe 幂等）。</param>
    public void RegisterDetach(Action action) {
        if (action == null) {
            return;
        }
        if (_detachActions == null) {
            _detachActions = new List<Action>();
        }
        int count = _detachActions.Count;
        for (int i = 0; i < count; i++) {
            if (_detachActions[i] == action) {
                return;
            }
        }
        _detachActions.Add(action);
    }
}
