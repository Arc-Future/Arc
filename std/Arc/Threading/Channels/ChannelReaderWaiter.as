// ChannelReaderWaiter<T> — 读等待者（RFC 046；internal 状态袋）。
namespace Arc.Threading.Channels;

/// <summary>
/// ReadAsync 挂起等待者——TCS 门 + 终态守卫。Done 防重复 settle；
/// Served 表示元素已直付（唤醒值有效）；Recheck 表示终结唤醒（唤醒值
/// 为哨兵，须重查缓冲余量）；两者皆否为取消哨兵
///（SetResult(default(T))——取消通道走已验证的 SetResult/OCE 路径）。
/// 全部状态迁移在 ChannelCore 的 Monitor 临界区内完成。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
internal class ChannelReaderWaiter<T> {
    /// <summary>挂起门：元素直付 SetResult(value)，复查/取消哨兵 SetResult(default(T))。</summary>
    public TaskCompletionSource<T> Gate { get; }

    /// <summary>是否已终结（settle 过一次）。</summary>
    public bool Done { get; set; }

    /// <summary>是否已交付元素（true 时唤醒值有效）。</summary>
    public bool Served { get; set; }

    /// <summary>是否为终结复查唤醒（true 时须重查缓冲余量而非按取消处理）。</summary>
    public bool Recheck { get; set; }

    /// <summary>创建未终结等待者。</summary>
    public ChannelReaderWaiter() {
        this.Gate = new TaskCompletionSource<T>();
    }
}
