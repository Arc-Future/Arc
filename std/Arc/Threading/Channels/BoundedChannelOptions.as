// BoundedChannelOptions — 有界通道选项（RFC 046）。
namespace Arc.Threading.Channels;

/// <summary>
/// 有界通道选项。容量经构造设定；背压模式默认 Wait。
/// 不设 SingleReader/SingleWriter——Monitor 串行化实现下为假旋钮
///（RFC 046 诚实差异；未来 native 快路径随 RFC 扩展选项面）。
/// </summary>
public class BoundedChannelOptions {
    /// <summary>缓冲容量上限（&gt; 0）。</summary>
    public int Capacity { get; set; }

    /// <summary>缓冲满时的写入策略；默认 Wait。</summary>
    public BoundedChannelFullMode FullMode { get; set; }

    /// <summary>以容量上限构造选项。</summary>
    /// <param name="capacity">缓冲容量上限（&gt; 0）。</param>
    public BoundedChannelOptions(int capacity) {
        this.Capacity = capacity;
        this.FullMode = BoundedChannelFullMode.Wait;
    }
}
