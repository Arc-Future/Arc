// BoundedChannelFullMode — 有界通道背压模式（RFC 046）。
namespace Arc.Threading.Channels;

/// <summary>
/// 有界通道缓冲满时的写入策略。Wait 为默认（0 值）。
/// </summary>
public enum BoundedChannelFullMode {
    /// <summary>写入方异步等待空位（真背压）。</summary>
    Wait,
    /// <summary>逐出缓冲中最旧元素，写入新元素。</summary>
    DropOldest,
    /// <summary>丢弃传入元素，缓冲保持不变，调用视为成功。</summary>
    DropNewest,
    /// <summary>丢弃传入元素，调用视为成功。</summary>
    DropWrite,
}
