// RFC 037 数据面目标态：ItemSourceView —— 数据源视图（object 管道 + 编译期显示投影）。
//
// 与 RFC 037 的对应：
//   - 「强类型 DataContext / 集合 ObservableCollection<T>」：ItemsControl 数据管道
//     载荷从 string 物化升为 object 本体（ItemAt 返回数据项本体，SelectedItem
//     本体化取此值）；string 仅作为「默认显示投影」存在（DisplayAt）。
//   - 「值元素强类型非裸字符串 / 无运行时反射」：显示投影编译期函数化——
//     From&lt;T&gt;(items, Func&lt;T, string&gt; display) 取代 WPF DisplayMemberPath 的
//     反射路径字符串（路径写错编译期不可查，且依赖运行时反射，双重背离）。
//
// 结构（三轨一线）：
//   - string 便捷轨：From(string) / From(List&lt;string&gt;)——显示即本体，活引用语义
//     （沿用原 ItemsControl string 轨行为：源列表原地变更后经 RefreshItems 可见）。
//   - 强类型静态轨：From&lt;T&gt;(List&lt;T&gt;, display) / From&lt;T&gt;(EnumOptions&lt;T&gt;)——From 时
//     一次烘焙平行表（object 本体 + string 投影）；静态源不可变，无变更通道。
//   - 动态轨：From(ObservableCollection&lt;string&gt;)——视图内订阅源通道（静态方法组 +
//     单活跃路由槽，逃逸闭包 UB 惯例同 ItemsControl 原机制，机制自此迁入视图），
//     string 载荷转换 object 载荷后经视图变更表面（CollectionChangedEventArgs&lt;object&gt;）
//     转发；ItemsControl 只消费视图表面，不再感知 ObservableCollection。
//
// 诚实边界：
//   - ObservableCollection&lt;T&gt;（非 string）动态轨暂不支持：泛型方法内无法以静态
//     方法组订阅 T 闭型通道（泛型方法组未支持 + 逃逸闭包 UB 未修复，属编译器
//     依赖项）；能力恢复后于本文件扩展 From&lt;T&gt;(ObservableCollection&lt;T&gt;, display)。
//   - 单活跃实例约束：同进程同时至多一个视图订阅动态源生效（多实例并发订阅
//     依赖编译器逃逸闭包修复）——与原 ItemsControl 机制约束等价，非退化。
//   - 接口化（IItemSourceView）暂缓：当前单一实现，待第二实现出现时再抽
//     （契约免维护双份）；管道消费方（ItemsControl/Generator/Panel）持具体类引用。

namespace Arc.UI.Components;

using Arc.Collections;
using Arc.ComponentModel;

/// <summary>
/// 数据源视图——项宿主与数据源之间的统一载荷面：数据项本体（object）+
/// 默认显示投影（string）+ 集合级变更表面。经静态工厂 <see cref="ItemSourceView.From"/>
/// 族构建，禁止裸构造（单一惯用法）。
/// </summary>
public class ItemSourceView {
    /// <summary>string 便捷轨：源列表活引用（显示即本体，投影恒等）。</summary>
    private List<string> _stringList;
    /// <summary>强类型静态轨：数据项本体（From 时一次装箱烘焙）。</summary>
    private List<object> _items;
    /// <summary>强类型静态轨：默认显示投影（编译期函数化结果）。</summary>
    private List<string> _displays;
    /// <summary>动态轨：可观察源活引用（Count/ItemAt/DisplayAt 直读源）。</summary>
    private ObservableCollection<string> _observableSource;
    /// <summary>动态轨源订阅 token（Unsubscribe 精确退订）。</summary>
    private int _sourceToken;
    /// <summary>视图变更表面 handler 表（与 ObservableCollection M-D0 同款
    /// handler 表 + 全局递增 token 空间）。</summary>
    private List<Action<CollectionChangedEventArgs<object>>> _handlers;
    private int _nextToken;

    /// <summary>动态轨订阅路由槽：静态方法组回调经此定位当前活跃视图
    /// （单活跃实例约束，见文件头诚实边界）。</summary>
    private static ItemSourceView _activeObservableView;

    private ItemSourceView() {
        _stringList = null;
        _items = null;
        _displays = null;
        _observableSource = null;
        _sourceToken = -1;
        _handlers = null;
        _nextToken = 0;
    }

    // ── 数据面（管道消费方：ItemsControl / ItemContainerGenerator / VirtualizingStackPanel）──

    /// <summary>项总数。</summary>
    public int Count {
        get {
            if (_observableSource != null) {
                return _observableSource.Count;
            }
            if (_stringList != null) {
                return _stringList.Count;
            }
            if (_items != null) {
                return _items.Count;
            }
            return 0;
        }
    }

    /// <summary>数据项本体（object 承载；SelectedItem 本体化取此值）。</summary>
    public object ItemAt(int index) {
        if (_observableSource != null) {
            return _observableSource[index];
        }
        if (_stringList != null) {
            return _stringList[index];
        }
        if (_items != null) {
            return _items[index];
        }
        return null;
    }

    /// <summary>默认显示投影（无 ItemTemplate 时 TextBlock 文本）。</summary>
    public string DisplayAt(int index) {
        if (_observableSource != null) {
            return _observableSource[index];
        }
        if (_stringList != null) {
            return _stringList[index];
        }
        if (_displays != null) {
            return _displays[index];
        }
        return "";
    }

    // ── 变更表面（ItemsControl 订阅面；静态源不触发）──

    /// <summary>订阅视图变更（动态轨源变更经此转发）；返回退订 token。</summary>
    public int OnChanged(Action<CollectionChangedEventArgs<object>> handler) {
        if (handler == null) {
            return -1;
        }
        if (_handlers == null) {
            _handlers = new List<Action<CollectionChangedEventArgs<object>>>();
        }
        int token = _nextToken;
        _nextToken = _nextToken + 1;
        _handlers.Add(handler);
        return token;
    }

    /// <summary>按 token 退订（置空跳过）；无效 token 静默忽略。</summary>
    public void Unsubscribe(int token) {
        if (token >= 0 && _handlers != null && token < _handlers.Count) {
            _handlers[token] = null;
        }
    }

    /// <summary>解除与源的绑定（ItemsSource 换绑/清空时调用）：退订动态源通道。
    /// 静态轨视图 no-op。</summary>
    public void Detach() {
        if (_observableSource != null) {
            _observableSource.Unsubscribe(_sourceToken);
            _observableSource = null;
            _sourceToken = -1;
            if (_activeObservableView == this) {
                _activeObservableView = null;
            }
        }
    }

    /// <summary>动态轨源变更路由：string 载荷 → object 载荷转换后经视图表面转发。</summary>
    private void OnSourceChanged(CollectionChangedEventArgs<string> args) {
        if (_handlers == null) {
            return;
        }
        CollectionChangedEventArgs<object> forwarded = new CollectionChangedEventArgs<object>();
        forwarded.Action = args.Action;
        forwarded.Index = args.Index;
        forwarded.OldIndex = args.OldIndex;
        forwarded.NewItem = args.NewItem;
        forwarded.OldItem = args.OldItem;
        this.NotifyChanged(forwarded);
    }

    /// <summary>向全部订阅者广播变更（跳过已退订的 null 槽）。</summary>
    private void NotifyChanged(CollectionChangedEventArgs<object> args) {
        if (_handlers == null) {
            return;
        }
        int count = _handlers.Count;
        int i = 0;
        while (i < count) {
            Action<CollectionChangedEventArgs<object>> handler = _handlers[i];
            if (handler != null) {
                handler(args);
            }
            i = i + 1;
        }
    }

    // ── 动态轨源订阅（静态方法组 + 单活跃路由槽；逃逸闭包 UB 惯例见文件头）──

    private static void OnSourceChangedStatic(CollectionChangedEventArgs<string> args) {
        ItemSourceView view = _activeObservableView;
        if (view != null) {
            view.OnSourceChanged(args);
        }
    }

    // ── 静态工厂（ItemsSource 判别物化的唯一构建入口）──

    /// <summary>单项便捷轨（string 显示即本体）。</summary>
    public static ItemSourceView From(string item) {
        ItemSourceView view = new ItemSourceView();
        view._items = new List<object>();
        view._displays = new List<string>();
        if (item != null) {
            view._items.Add(item);
            view._displays.Add(item);
        }
        return view;
    }

    /// <summary>string 列表便捷轨：活引用（沿用原 ItemsControl string 轨语义，
    /// 源列表原地变更后经 RefreshItems 可见）。</summary>
    public static ItemSourceView From(List<string> items) {
        ItemSourceView view = new ItemSourceView();
        if (items != null) {
            view._stringList = items;
        }
        return view;
    }

    /// <summary>强类型静态轨：编译期显示投影（DisplayMemberPath 反射路径的取代面，
    /// RFC 037 无反射目标）。From 时一次烘焙平行表，静态源不可变。
    /// display 为 null 时投影空串（无 ToString 反射回退）。</summary>
    public static ItemSourceView From<T>(List<T> items, Func<T, string> display) {
        ItemSourceView view = new ItemSourceView();
        view._items = new List<object>();
        view._displays = new List<string>();
        if (items != null) {
            int count = items.Count;
            int i = 0;
            while (i < count) {
                T item = items[i];
                view._items.Add(item);
                if (display != null) {
                    view._displays.Add(display(item));
                } else {
                    view._displays.Add("");
                }
                i = i + 1;
            }
        }
        return view;
    }

    /// <summary>动态轨：订阅可观察源（CollectionChanged → 视图变更表面转发，
    /// string 载荷 object 化）；null 源退化为空视图。</summary>
    public static ItemSourceView From(ObservableCollection<string> items) {
        ItemSourceView view = new ItemSourceView();
        if (items != null) {
            view._observableSource = items;
            Action<CollectionChangedEventArgs<string>> handler =
                ItemSourceView.OnSourceChangedStatic;
            _activeObservableView = view;
            view._sourceToken = items.OnChanged(handler);
        }
        return view;
    }

    /// <summary>强类型枚举选项轨：本体 = 枚举值 T（SelectedItem 本体化直取），
    /// 投影 = DisplayName；From 时一次烘焙（EnumOptions 构建期定形，视作静态源）。</summary>
    public static ItemSourceView From<T>(EnumOptions<T> options) {
        ItemSourceView view = new ItemSourceView();
        view._items = new List<object>();
        view._displays = new List<string>();
        if (options != null) {
            int count = options.Count;
            int i = 0;
            while (i < count) {
                EnumOption<T> option = options.Get(i);
                view._items.Add(option.Value);
                view._displays.Add(option.DisplayName);
                i = i + 1;
            }
        }
        return view;
    }
}
