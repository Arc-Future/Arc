// Channels — 通道工厂枢纽（RFC 046）。
namespace Arc.Threading.Channels;

/// <summary>
/// 通道工厂枢纽（静态类）。对标 .NET 非泛型 Channel 静态工厂；返回
/// Channel&lt;T&gt;（构造函数收 internal 核心，外部仅能经本工厂创建通道），
/// 具体实现不进公开面。
/// </summary>
public static class Channels {
    /// <summary>创建指定容量上限的有界通道（Wait 背压）。</summary>
    /// <typeparam name="T">元素类型。</typeparam>
    /// <param name="capacity">缓冲容量上限（&gt; 0）。</param>
    /// <returns>有界通道实例。</returns>
    public static Channel<T> CreateBounded<T>(int capacity) {
        ChannelCore<T> core = new ChannelCore<T>(capacity, BoundedChannelFullMode.Wait, false);
        return new Channel<T>(new CoreChannelReader<T>(core), new CoreChannelWriter<T>(core));
    }

    /// <summary>按选项创建有界通道。</summary>
    /// <typeparam name="T">元素类型。</typeparam>
    /// <param name="options">有界通道选项（容量与背压模式）。</param>
    /// <returns>有界通道实例。</returns>
    public static Channel<T> CreateBounded<T>(BoundedChannelOptions options) {
        ChannelCore<T> core = new ChannelCore<T>(options.Capacity, options.FullMode, false);
        return new Channel<T>(new CoreChannelReader<T>(core), new CoreChannelWriter<T>(core));
    }

    /// <summary>创建无界通道（写端永不等待）。</summary>
    /// <typeparam name="T">元素类型。</typeparam>
    /// <returns>无界通道实例。</returns>
    public static Channel<T> CreateUnbounded<T>() {
        ChannelCore<T> core = new ChannelCore<T>(0, BoundedChannelFullMode.Wait, true);
        return new Channel<T>(new CoreChannelReader<T>(core), new CoreChannelWriter<T>(core));
    }
}
