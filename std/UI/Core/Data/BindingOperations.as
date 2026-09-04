// RFC 037 D3.3 / D4: Arc.UI — 绑定操作入口。
//
// BindingOperations 提供 BindingExpression 与依赖属性的运行时连接：
//   - 绑定源（DataContext）解析
//   - 路径解析（Path = "User.Address.City"）
//   - Mode 应用（OneWay/TwoWay/OneTime/OneWayToSource）
//
// **设计决策**：Arc 放弃 C# INotifyPropertyChanged + event 体系（RFC 037 D3.2），
// 绑定源变更通知统一由 Signal<T> 驱动——BindingExpression 通过
// Signal<T>.Subscribe 订阅源变更，无需 add_/remove_ 访问器对。
//
// **命名空间归属**：本文件位于 std/UI/Data/ 子目录（绑定域：Binding /
// BindingOperations / DataContext 团聚），但归属到 `Arc.UI`
// 根命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」命名空间分层原则）。
//
// 实现状态：
//   - M1: 类型骨架声明
//   - M2: OneWay/OneTime 绑定路径解析
//   - M3: TwoWay + Signal<T> 订阅（替代 INPC 监听）
//   - M4（RFC 037）：`x:Bind` 编译期生成的订阅/退订的运行时载体
//     （`SetBinding<T>(Element, DependencyProperty<T>, Signal<T>)`、
//      `SetTwoWay(Input, Signal<string>)`、`BindText`）；集合绑定「替换配对」
//     原语 `BindCollection<T>`；`{Binding}` 运行时路径解析 / DataContext 动态
//     切换为后移项（RFC 037 §4 明确排除）。
//
// **逃逸闭包约束（2026-08-05 诚实标注）**：编译器对逃逸闭包的 ByRef 捕获存
// 外层变量槽地址，闭包跨函数逃逸后槽位悬垂 → UB（Signal.Subscribe 回调在
// 注册函数返回后触发时实测 AV，2026-08-05）。故本文件所有注册的订阅/退订
// 回调一律**只捕获 int 绑定 id**（ByValue，值拷贝进 env，跨函数安全），
// 目标元素 / 源 Signal / 属性元数据等类引用存入静态 `BindingRegistry`
// 记录，由静态分发经 id 定位（与 ItemsControl M6 静态方法组绕行同源；
// 多实例并发订阅仍依赖编译器逃逸闭包修复——见 RFC 037 M4 报告）。

namespace Arc.UI;

using Arc.Collections;
using Arc.UI.Components;

/// <summary>绑定操作入口，连接 BindingExpression 与依赖属性。</summary>
public class BindingOperations {
    /// <summary>为指定元素的依赖属性应用绑定。</summary>
    /// <typeparam name="T">属性值类型。</typeparam>
    /// <param name="target">目标元素。</param>
    /// <param name="property">目标依赖属性（强类型 DependencyProperty&lt;T&gt;）。</param>
    /// <param name="binding">绑定描述。</param>
    public static void SetBinding<T>(Element target, DependencyProperty<T> property, Binding binding) {
        // RFC 037 §4：`{Binding}` 运行时路径解析 / DataContext 动态切换为**后移项**——
        // Binding 为纯描述 struct（Path/Mode/Converter...），本签名保持占位；
        // `x:Bind` 编译期生成的订阅/退订由下方 Signal 源重载承载（零反射）。
    }

    /// <summary>
    /// RFC 037 M4：`x:Bind` 编译期生成的订阅/退订的运行时载体（零反射）。
    /// </summary>
    /// <remarks>
    /// 接收编译器已静态定址的源 <see cref="Signal{T}"/>（`ObserveProperty("Prop")`
    /// 直访——无运行期字符串解析），完成目标 DP 的初始值写入 + 源变更订阅 +
    /// G2 卸载退订登记（<see cref="Element.RegisterDetach"/>）。codegen 将
    /// `<TextBox Text="{x:Bind Prop}"/>`（OneTime/OneWay）脱糖为对本方法的单次调用。
    /// 订阅/退订回调只捕获绑定 id（int），不捕获类引用（逃逸闭包约束）。
    /// </remarks>
    public static void SetBinding<T>(Element target, DependencyProperty<T> property, Signal<T> source) {
        if (target == null || property == null || source == null) {
            return;
        }
        int id = BindingRegistry.PutValue<T>(target, property, source);
        target.SetValue<T>(property, source.Value);
        int token = source.Subscribe((v: T) => {
            BindingRegistry.ApplyValue<T>(id, v);
        });
        BindingRegistry.SetValueToken<T>(id, token);
        target.RegisterDetach(() => {
            BindingRegistry.DetachValue<T>(id);
        });
    }

    /// <summary>
    /// RFC 037 M4：`<TextBox Text="{x:Bind Prop, Mode=TwoWay}"/>` 的运行时载体。
    /// </summary>
    /// <remarks>
    /// 源→目标方向（初始值 + 订阅 + G2 卸载退订）与目标→源写回（控件
    /// `TextChanged` → 源 <see cref="Signal{T}"/>，即 VM `[Observable]` 属性
    /// setter 路径——`ObserveProperty` 通道即属性 backing store）一次性建立；
    /// 相等性守卫在 <see cref="BindingRegistry"/> 记录内（`TextBox.Text` setter
    /// 无条件触发 `TextChanged`、string 信号无相等性短路，写回回声经守卫截断防回环）。
    /// 回调一律只捕获绑定 id（int），跨函数逃逸安全。
    /// </remarks>
    public static void SetTwoWay(TextBox target, Signal<string> source) {
        if (target == null || source == null) {
            return;
        }
        int id = BindingRegistry.PutTwoWay(target, source);
        target.Text = source.Value;
        int token = source.Subscribe((v: string) => {
            BindingRegistry.ApplyTwoWay(id, v);
        });
        BindingRegistry.SetTwoWayToken(id, token);
        target.OnTextChanged((x: string) => {
            BindingRegistry.WriteBackTwoWay(id, x);
        });
        target.RegisterDetach(() => {
            BindingRegistry.DetachTwoWay(id);
        });
    }

    /// <summary>
    /// RFC 037 M4：`<TextBox Text="{x:Bind Prop}"/>`（OneWay/TwoWay 的 VM→UI
    /// 半边）运行时载体——经 <c>TextBox.Text</c> setter 路由编辑内核
    /// （TextBoxModel 唯一真相），不裸写 DP（防内核失同步）。
    /// </summary>
    /// <remarks>
    /// 初始 `TextBox.Text` 赋值 + 源 <see cref="Signal{T}"/> 订阅（Apply 经
    /// Text setter → 内核 SetText → SyncFromModel 全链）+ G2 卸载退订登记。
    /// 回声收敛：UI→VM 写回 → VM setter → 信号 → Apply 同值 → 内核 SetText
    /// 同值早退（不进撤销栈）。回调只捕获绑定 id（int），跨函数逃逸安全。
    /// </remarks>
    public static int BindTextBoxText(TextBox target, Signal<string> source) {
        if (target == null || source == null) {
            return -1;
        }
        target.Text = source.Value;
        int id = BindingRegistry.PutTextBox(target, source);
        int token = source.Subscribe((v: string) => {
            BindingRegistry.ApplyTextBox(id, v);
        });
        BindingRegistry.SetTextBoxToken(id, token);
        target.RegisterDetach(() => {
            BindingRegistry.DetachTextBox(id);
        });
        return id;
    }

    /// <summary>
    /// RFC 037 M4：`x:Bind` codegen 脱糖辅助——同步 Text 逻辑树与平台镜像（零反射）。
    /// </summary>
    public static void SyncText(TextBlock target, long platformHandle, string value) {
        target.Text = value;
        WindowHost.ElementSetString(platformHandle, "Text", value);
    }

    /// <summary>
    /// RFC 037 M4：`<TextBlock Text="{x:Bind Prop}"/>`（OneWay/TwoWay）的运行时载体。
    /// </summary>
    /// <remarks>
    /// 初始 `SyncText` + 源 <see cref="Signal{T}"/> 订阅（<see cref="SyncText"/>
    /// 回写逻辑树与平台镜像）+ G2 卸载退订登记。TextBlock 无输入通道，TwoWay 与
    /// OneWay 等价（仅源→目标）。回调只捕获绑定 id（int），跨函数逃逸安全。
    /// </remarks>
    public static int BindText(TextBlock target, long platformHandle, Signal<string> source) {
        if (target == null || source == null) {
            return -1;
        }
        SyncText(target, platformHandle, source.Value);
        int id = BindingRegistry.PutText(target, platformHandle, source);
        int token = source.Subscribe((v: string) => {
            BindingRegistry.ApplyText(id, v);
        });
        BindingRegistry.SetTextToken(id, token);
        target.RegisterDetach(() => {
            BindingRegistry.DetachText(id);
        });
        return token;
    }

    /// <summary>
    /// RFC 037 §5.3 / RFC 037 M4：可写集合属性「替换配对」绑定原语。
    /// </summary>
    /// <remarks>
    /// 绑定方持有属性通道 <c>Signal&lt;ObservableCollection&lt;T&gt;&gt;</c>
    /// （`ObserveProperty("Items")` 静态定址）；本方法建立两级订阅并维护
    /// 「解绑旧集合 + 订阅新集合」配对：
    /// <list type="number">
    /// <item>订阅属性通道：集合被整体替换时重配对；</item>
    /// <item>订阅当前集合 <see cref="ObservableCollection{T}.OnChanged"/>：
    /// 项级变更 → <paramref name="onCollectionChanged"/>；</item>
    /// <item>替换通知到达 → 退订旧集合 + 订阅新集合。</item>
    /// </list>
    /// 编译器**不**把订阅/退订合成进 VM setter（G1：发布方不强持订阅方；
    /// `[Observable]` 仅回答「属性被替换」，集合内部变化归集合自己）。
    /// G2：属性通道订阅登记到 <paramref name="target"/> 卸载生命周期。
    /// 回调只捕获绑定 id（int），跨函数逃逸安全。
    /// ItemsControl/ListView 对容器级复用的接线属 M6 轨。
    /// </remarks>
    public static int BindCollection<T>(
        Element target,
        Signal<ObservableCollection<T>> source,
        Action<CollectionChangedEventArgs<T>> onCollectionChanged
    ) {
        if (target == null || source == null || onCollectionChanged == null) {
            return -1;
        }
        int id = BindingRegistry.PutCollection<T>(source, onCollectionChanged);
        BindingRegistry.PairCollection<T>(id);
        int propToken = source.Subscribe((c: ObservableCollection<T>) => {
            BindingRegistry.OnCollectionReplaced<T>(id, c);
        });
        BindingRegistry.SetCollectionPropToken<T>(id, propToken);
        target.RegisterDetach(() => {
            BindingRegistry.DetachCollection<T>(id);
        });
        return id;
    }
}

/// RFC 037 M4：绑定运行时注册表——订阅/退订回调只持有 int 绑定 id，
/// 目标元素 / 源 Signal / 属性元数据等类引用存于此（静态，跨函数稳定）。
/// 分发静态方法按 id 定位记录；记录持有类引用直到 Detach 清除（G2 落点）。
/// 注：Arc `static class` 不支持字段（静态成员也可在普通类声明），故用普通类。
internal class BindingRegistry {
    private static Dictionary<int, object> _records;
    private static int _nextId;

    public static int Store(object entry) {
        if (_records == null) {
            _records = new Dictionary<int, object>();
        }
        int id = _nextId;
        _nextId = _nextId + 1;
        _records[id] = entry;
        return id;
    }

    // ── 值绑定（SetBinding<T>）──

    public static int PutValue<T>(Element target, DependencyProperty<T> property, Signal<T> source) {
        return Store(new ValueRecord<T>(target, property, source));
    }

    public static void ApplyValue<T>(int id, T value) {
        ValueRecord<T> r = (ValueRecord<T>)_records[id];
        if (r != null) {
            r.Apply(value);
        }
    }

    public static void SetValueToken<T>(int id, int token) {
        ValueRecord<T> r = (ValueRecord<T>)_records[id];
        if (r != null) {
            r.Token = token;
        }
    }

    public static void DetachValue<T>(int id) {
        ValueRecord<T> r = (ValueRecord<T>)_records[id];
        if (r != null) {
            r.Detach();
            _records[id] = null;
        }
    }

    // ── TwoWay 写回（SetTwoWay）──

    public static int PutTwoWay(TextBox target, Signal<string> source) {
        return Store(new TwoWayRecord(target, source));
    }

    public static void ApplyTwoWay(int id, string value) {
        TwoWayRecord r = (TwoWayRecord)_records[id];
        if (r != null) {
            r.Apply(value);
        }
    }

    public static void WriteBackTwoWay(int id, string value) {
        TwoWayRecord r = (TwoWayRecord)_records[id];
        if (r != null) {
            r.WriteBack(value);
        }
    }

    public static void SetTwoWayToken(int id, int token) {
        TwoWayRecord r = (TwoWayRecord)_records[id];
        if (r != null) {
            r.Token = token;
        }
    }

    public static void DetachTwoWay(int id) {
        TwoWayRecord r = (TwoWayRecord)_records[id];
        if (r != null) {
            r.Detach();
            _records[id] = null;
        }
    }

    // ── Text 同步（BindText）──

    public static int PutText(TextBlock target, long platformHandle, Signal<string> source) {
        return Store(new TextRecord(target, platformHandle, source));
    }

    public static void ApplyText(int id, string value) {
        TextRecord r = (TextRecord)_records[id];
        if (r != null) {
            r.Apply(value);
        }
    }

    public static void SetTextToken(int id, int token) {
        TextRecord r = (TextRecord)_records[id];
        if (r != null) {
            r.Token = token;
        }
    }

    public static void DetachText(int id) {
        TextRecord r = (TextRecord)_records[id];
        if (r != null) {
            r.Detach();
            _records[id] = null;
        }
    }

    // ── TextBox 编辑内核路由（BindTextBoxText）──

    public static int PutTextBox(TextBox target, Signal<string> source) {
        return Store(new TextBoxTextRecord(target, source));
    }

    public static void ApplyTextBox(int id, string value) {
        TextBoxTextRecord r = (TextBoxTextRecord)_records[id];
        if (r != null) {
            r.Apply(value);
        }
    }

    public static void SetTextBoxToken(int id, int token) {
        TextBoxTextRecord r = (TextBoxTextRecord)_records[id];
        if (r != null) {
            r.Token = token;
        }
    }

    public static void DetachTextBox(int id) {
        TextBoxTextRecord r = (TextBoxTextRecord)_records[id];
        if (r != null) {
            r.Detach();
            _records[id] = null;
        }
    }

    // ── 集合配对（BindCollection）──

    public static int PutCollection<T>(
        Signal<ObservableCollection<T>> source,
        Action<CollectionChangedEventArgs<T>> handler
    ) {
        return Store(new CollectionRecord<T>(source, handler));
    }

    public static void PairCollection<T>(int id) {
        CollectionRecord<T> r = (CollectionRecord<T>)_records[id];
        if (r != null) {
            r.Pair();
        }
    }

    public static void SetCollectionPropToken<T>(int id, int token) {
        CollectionRecord<T> r = (CollectionRecord<T>)_records[id];
        if (r != null) {
            r.PropToken = token;
        }
    }

    public static void OnCollectionReplaced<T>(int id, ObservableCollection<T> newCollection) {
        CollectionRecord<T> r = (CollectionRecord<T>)_records[id];
        if (r != null) {
            r.OnReplaced(newCollection);
        }
    }

    public static void DetachCollection<T>(int id) {
        CollectionRecord<T> r = (CollectionRecord<T>)_records[id];
        if (r != null) {
            r.Detach();
            _records[id] = null;
        }
    }
}

/// RFC 037 M4：值绑定记录（目标元素 + 属性元数据 + 源 Signal + token）。
/// 泛型类模板按 RFC 018 M4-1 设计方案不注册到 registry.types，构造体用
/// 裸字段赋值（`this.Field` 解析失败，与 DependencyProperty&lt;T&gt; 一致）。
internal class ValueRecord<T> {
    public Element Target;
    public DependencyProperty<T> Property;
    public Signal<T> Source;
    public int Token;

    public ValueRecord(Element target, DependencyProperty<T> property, Signal<T> source) {
        Target = target;
        Property = property;
        Source = source;
        Token = -1;
    }

    public void Apply(T value) {
        if (Target != null && Property != null) {
            Target.SetValue<T>(Property, value);
        }
    }

    public void Detach() {
        if (Source != null && Token >= 0) {
            Source.Unsubscribe(Token);
        }
    }
}

/// RFC 037 M4：TwoWay 写回记录（目标 TextBox + 源 Signal + 守卫值）。
/// `Last` 在双向同步中都更新：源→目标写后置为所写值、目标→源写回后置为所写值，
/// 使「UI 回声 → 源 Set → 目标 Set → 回声」回路在守卫处收敛（防回环）。
internal class TwoWayRecord {
    public TextBox Target;
    public Signal<string> Source;
    public string Last;
    public int Token;

    public TwoWayRecord(TextBox target, Signal<string> source) {
        Target = target;
        Source = source;
        Last = source.Value;
        Token = -1;
    }

    public void Apply(string value) {
        Last = value;
        if (Target != null) {
            Target.Text = value;
        }
    }

    public void WriteBack(string value) {
        if (value != Last) {
            Last = value;
            if (Source != null) {
                Source.Set(value);
            }
        }
    }

    public void Detach() {
        if (Source != null && Token >= 0) {
            Source.Unsubscribe(Token);
        }
    }
}

/// RFC 037 M4：Text 平台同步记录（逻辑树目标 + 平台镜像句柄 + 源 Signal + token）。
internal class TextRecord {
    public TextBlock Target;
    public long PlatformHandle;
    public Signal<string> Source;
    public int Token;

    public TextRecord(TextBlock target, long platformHandle, Signal<string> source) {
        Target = target;
        PlatformHandle = platformHandle;
        Source = source;
        Token = -1;
    }

    public void Apply(string value) {
        if (Target != null) {
            BindingOperations.SyncText(Target, PlatformHandle, value);
        }
    }

    public void Detach() {
        if (Source != null && Token >= 0) {
            Source.Unsubscribe(Token);
        }
    }
}

/// RFC 037 M4：TextBox 编辑内核路由记录（目标 TextBox + 源 Signal + token）。
/// Apply 经 `TextBox.Text` setter（内核 SetText → SyncFromModel），不裸写 DP。
internal class TextBoxTextRecord {
    public TextBox Target;
    public Signal<string> Source;
    public int Token;

    public TextBoxTextRecord(TextBox target, Signal<string> source) {
        Target = target;
        Source = source;
        Token = -1;
    }

    public void Apply(string value) {
        if (Target != null) {
            Target.Text = value;
        }
    }

    public void Detach() {
        if (Source != null && Token >= 0) {
            Source.Unsubscribe(Token);
        }
    }
}

/// RFC 037 M4：集合绑定配对记录（绑定方侧状态，不侵入 VM / 不落 setter）。
internal class CollectionRecord<T> {
    public Signal<ObservableCollection<T>> Source;
    public Action<CollectionChangedEventArgs<T>> Handler;
    public ObservableCollection<T> Collection;
    public int CollectionToken;
    public int PropToken;

    public CollectionRecord(Signal<ObservableCollection<T>> source, Action<CollectionChangedEventArgs<T>> handler) {
        Source = source;
        Handler = handler;
        Collection = null;
        CollectionToken = -1;
        PropToken = -1;
    }

    public void Pair() {
        Collection = Source.Value;
        CollectionToken = -1;
        if (Collection != null) {
            CollectionToken = Collection.OnChanged(Handler);
        }
    }

    public void OnReplaced(ObservableCollection<T> newCollection) {
        if (Collection != null && CollectionToken >= 0) {
            Collection.Unsubscribe(CollectionToken);
        }
        Collection = newCollection;
        CollectionToken = -1;
        if (Collection != null) {
            CollectionToken = Collection.OnChanged(Handler);
        }
    }

    public void Detach() {
        if (Collection != null && CollectionToken >= 0) {
            Collection.Unsubscribe(CollectionToken);
        }
        if (Source != null && PropToken >= 0) {
            Source.Unsubscribe(PropToken);
        }
    }
}
