// ChannelWriterWaiter<T> — 写等待者（RFC 046；internal 状态袋）。
namespace Arc.Threading.Channels;

/// <summary>
/// WriteAsync 满载挂起等待者——携带待写元素与 TCS 门。消费者腾出空位后
/// 在锁内将 Item 收纳入缓冲并置 Served 唤醒；取消时以哨兵
///（SetResult(false)）唤醒。全部状态迁移在 ChannelCore 的 Monitor
/// 临界区内完成。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class ChannelWriterWaiter<T> {
    /// <summary>待写入元素（消费者收纳时入缓冲）。</summary>
    public T Item { get; }

    /// <summary>挂起门：空位收纳 SetResult(true)，取消哨兵 SetResult(false)。</summary>
    public TaskCompletionSource<bool> Gate { get; }

    /// <summary>是否已终结（settle 过一次）。</summary>
    public bool Done { get; set; }

    /// <summary>是否已收纳写入（true 时元素已进缓冲）。</summary>
    public bool Served { get; set; }

    /// <summary>以待写元素创建等待者。</summary>
    /// <param name="item">待写入元素。</param>
    public ChannelWriterWaiter(T item) {
        this.Item = item;
        this.Gate = new TaskCompletionSource<bool>();
    }
}
