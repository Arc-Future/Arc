// RFC 024 M7: Arc.Collections.Concurrent — 线程安全集合接口。
// 对齐 C# System.Collections.Concurrent.IProducerConsumerCollection<T> 最小面。
// 批量操作留给具体类型（RFC 024 §4.4）。
namespace Arc.Collections.Concurrent;

/// <summary>并发集合接口——线程安全集合的公共抽象。</summary>
public interface IConcurrentCollection<T> {
    /// <summary>尝试添加元素。成功返回 true。</summary>
    bool TryAdd(T item);
    /// <summary>尝试取出元素。成功返回 true 并输出值。</summary>
    bool TryTake(out T item);
    /// <summary>复制到数组快照。</summary>
    T[] ToArray();
    /// <summary>复制到目标数组（自 index 起）。</summary>
    void CopyTo(T[] array, int index);
    /// <summary>元素数（近似值）。</summary>
    int Count { get; }
    /// <summary>是否为空。</summary>
    bool IsEmpty { get; }
}