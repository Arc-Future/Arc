namespace Arc.Collections;

/// <summary>
/// 异步枚举器接口——按需驱动生产者推进的拉模型序列游标
/// （RFC 008 AsyncStream；对齐 C# IAsyncEnumerator&lt;T&gt;，MoveNextAsync
/// 返回 Task&lt;bool&gt;——单一惯用法，同步完成快速路径由 rt_task inline
/// poll 直通提供）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface IAsyncEnumerator<out T> {
    /// <summary>驱动到下一个元素。</summary>
    /// <returns>前进成功返回 true；越过序列末尾返回 false。</returns>
    Task<bool> MoveNextAsync();

    /// <summary>当前指向的元素。</summary>
    T Current { get; }
}
