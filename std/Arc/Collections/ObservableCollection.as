// RFC 037 §5.3 集合级通道：ObservableCollection<T> —— 数据驱动通知闭环的集合级载体。
//
// 与 RFC 037 §5.3 的对应：
//   - 「两级通道 · 集合级」：`ObservableCollection<T>`（`CollectionChanged` 表面），
//     项级（Add/Remove/Update/Insert/Move/Clear）通知 → ItemsControl 增量容器复用（M6）。
//   - 「集合级」设计面：最小对齐 Collection<T>（Add/Clear/Contains/Remove/Count/
//     索引器/IndexOf/Insert/RemoveAt）+ `Move`；全部**变更**操作发集合级通知，
//     只读查询（Contains/IndexOf/Count/索引器 get）不发通知。
//   - 「变更表面 CollectionChanged（kind/index/item）」：`OnChanged` 订阅 / `Unsubscribe`
//     退订，token 复用 Signal<T> M-D0 修复后的统一 token 空间模式（全局递增 token +
//     并行登记表精确定位）；事件参数 `CollectionChangedEventArgs<T>`（kind/index/oldIndex/
//     newItem/oldItem，见 CollectionChangeAction.as / CollectionChangedEventArgs.as）。
//   - 「发布方（collection）不存任何订阅方回调」：handler 表由集合持有、退订即置空，
//     VM 不持有 UI（G1/G2 生命周期配对退订归 codegen，属 M-D0 后续切片，不在本文件）。
//   - 「集合属性不做深比较短路」：本类不做元素深比较（Update/Remove 按下标/引用语义）。
//
// 与 Signal<T> 的差异（诚实标注）：内部直接实现 handler 表 + token 登记，而非内嵌
// `Signal<CollectionChangedEventArgs<T>>`——Signal 的 `OnChanged(Action<T,T>)` 二元
// 参数形态不适配集合级单参事件参数（历史注释曾归因于 lambda 链接受 rt_threadpool
// 阻断，经 2026-08-04 根因调查原归因陈旧，见 `data_driven_property_e2e`）。token
// 空间语义与 Signal M-D0 完全一致。
//
// 成熟度：M-D0 集合级切片（最小面；升 Stable 按 RFC 037 §17 走非 Skip e2e）。
//   验证：`data_driven_collection_e2e`（各动作发通知、事件参数取值、退订后零回调；
//   静态方法组注册，绕开 lambda 链接限制）。
//   不宣称：ItemsControl 增量容器复用（M6）；可写集合属性 setter 替换配对（M-D0 后续）。

namespace Arc.Collections;

/// <summary>
/// 可观察集合——最小面对齐 <see cref="Collection{T}"/> + 项级 <c>CollectionChanged</c> 通知。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
/// <remarks>
/// 订阅通道：<see cref="OnChanged"/> 返回 token，<see cref="Unsubscribe"/> 按 token 退订
/// （统一 token 空间，全局递增、精确定位，杜绝跨表误清空——Signal M-D0 同款机制）。
/// </remarks>
public class ObservableCollection<T> {
    private List<T> _items;
    private List<Action<CollectionChangedEventArgs<T>>> _handlers;
    private int _nextToken;

    /// <summary>以空列表初始化。</summary>
    public ObservableCollection() {
        _items = new List<T>();
        _handlers = new List<Action<CollectionChangedEventArgs<T>>>();
        _nextToken = 0;
    }

    // ── 订阅通道（CollectionChanged 表面 · 统一 token 空间）──

    /// <summary>订阅集合变更；返回退订 token（全局递增、唯一）。</summary>
    public int OnChanged(Action<CollectionChangedEventArgs<T>> handler) {
        if (handler == null) {
            return -1;
        }
        if (_handlers == null) {
            _handlers = new List<Action<CollectionChangedEventArgs<T>>>();
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

    // ── 只读查询（不发通知）──

    /// <summary>元素总数。</summary>
    public int Count {
        get { return _items.Count; }
    }

    /// <summary>判断是否包含。</summary>
    public bool Contains(T item) {
        return _items.Contains(item);
    }

    /// <summary>查找下标。</summary>
    public int IndexOf(T item) {
        return _items.IndexOf(item);
    }

    /// <summary>索引器：<c>collection[i]</c> / <c>collection[i]=v</c>（Update 走 indexer set）。</summary>
    public T this[int index] {
        get { return _items[index]; }
        set {
            T old = _items[index];
            _items[index] = value;
            this.NotifyChanged(CollectionChangeAction.Update, index, -1, value, old);
        }
    }

    // ── 变更操作（全部发集合级通知）──

    /// <summary>尾部追加元素（CollectionChanged.Add）。</summary>
    public void Add(T item) {
        _items.Add(item);
        this.NotifyChanged(CollectionChangeAction.Add, _items.Count - 1, -1, item, default(T));
    }

    /// <summary>在指定下标插入元素（CollectionChanged.Insert）。</summary>
    public void Insert(int index, T item) {
        _items.Insert(index, item);
        this.NotifyChanged(CollectionChangeAction.Insert, index, -1, item, default(T));
    }

    /// <summary>移除首个匹配元素；未找到返回 false（CollectionChanged.Remove）。</summary>
    public bool Remove(T item) {
        int index = _items.IndexOf(item);
        if (index < 0) {
            return false;
        }
        _items.RemoveAt(index);
        this.NotifyChanged(CollectionChangeAction.Remove, index, -1, default(T), item);
        return true;
    }

    /// <summary>移除指定下标元素（CollectionChanged.Remove）。</summary>
    public void RemoveAt(int index) {
        T old = _items[index];
        _items.RemoveAt(index);
        this.NotifyChanged(CollectionChangeAction.Remove, index, -1, default(T), old);
    }

    /// <summary>移动元素（CollectionChanged.Move：OldIndex 源 / Index 新位置）。</summary>
    /// <remarks>越界或原地移动直接返回，避免 RemoveAt 成功后 Insert 越界导致元素丢失。</remarks>
    public void Move(int fromIndex, int toIndex) {
        if (fromIndex < 0 || fromIndex >= _items.Count) {
            return;
        }
        if (toIndex < 0 || toIndex >= _items.Count) {
            return;
        }
        if (fromIndex == toIndex) {
            return;
        }
        T item = _items[fromIndex];
        _items.RemoveAt(fromIndex);
        int insertIndex = toIndex;
        if (fromIndex < toIndex) {
            insertIndex = toIndex - 1;
        }
        _items.Insert(insertIndex, item);
        this.NotifyChanged(CollectionChangeAction.Move, insertIndex, fromIndex, item, item);
    }

    /// <summary>清空全部（CollectionChanged.Clear）；空集合不通知。</summary>
    public void Clear() {
        if (_items.Count == 0) {
            return;
        }
        _items.Clear();
        this.NotifyChanged(CollectionChangeAction.Clear, -1, -1, default(T), default(T));
    }

    /// <summary>向全部订阅者广播变更（跳过已退订的 null 槽）。</summary>
    private void NotifyChanged(CollectionChangeAction action, int index, int oldIndex, T newItem, T oldItem) {
        if (_handlers == null) {
            return;
        }
        CollectionChangedEventArgs<T> args = new CollectionChangedEventArgs<T>();
        args.Action = action;
        args.Index = index;
        args.OldIndex = oldIndex;
        args.NewItem = newItem;
        args.OldItem = oldItem;
        int count = _handlers.Count;
        int i = 0;
        while (i < count) {
            Action<CollectionChangedEventArgs<T>> handler = _handlers[i];
            if (handler != null) {
                handler(args);
            }
            i = i + 1;
        }
    }
}
