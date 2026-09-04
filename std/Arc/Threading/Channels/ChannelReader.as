// ChannelReader<T> — 通道读端契约（RFC 046）。
namespace Arc.Threading.Channels;

using Arc;
using Arc.Collections;

/// <summary>
/// 通道读端契约——TryRead 同步快路径 + ReadAsync 挂起读 + ReadAllAsync
/// 流式消费 + Completion 完成信号。经 Channel&lt;T&gt;.Reader 获取。
///
/// 契约成员为方法形态（CanCount/Count/Completion）：泛型基类上的抽象属性
/// override 触发编译器挂死缺陷（非泛型 Stream 无此问题），方法 override
/// 为已验证路径；随编译器修复可回升属性形态（RFC 046 诚实差异）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public abstract class ChannelReader<T> {
    /// <summary>是否支持 Count（当前实现恒 true）。</summary>
    public abstract bool CanCount();

    /// <summary>缓冲中当前元素数（直付中的元素不计入）。</summary>
    public abstract int Count();

    /// <summary>
    /// 完成信号 Task：终结且排尽后正常完成（true）；以 error 终结则携带 error 失败。
    /// 对标 .NET 的 Task 形状完成信号——TCS 无法承载 void 完成信号，
    /// 以 Task&lt;bool&gt; 承载（RFC 046 诚实差异）。
    /// </summary>
    public abstract Task<bool> Completion();

    /// <summary>同步读：缓冲有值出队；空（含终结排尽）返回 false。</summary>
    /// <param name="item">出队元素（无值时为 default(T)）。</param>
    /// <returns>true 表示取得元素。</returns>
    public abstract bool TryRead(out T item);

    /// <summary>
    /// 异步读：缓冲有值同步完成；空载挂起直至元素直付或终结；
    /// 终结排尽抛 ChannelClosedException（或终结 error）；取消经 ct 协作中断。
    /// </summary>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>读取的元素。</returns>
    public abstract Task<T> ReadAsync(CancellationToken cancellationToken = default);

    /// <summary>
    /// 流式消费全部元素（IAsyncEnumerable）：终结排尽即序列结束；
    /// 以 error 终结或取消时异常原样传播。序列持读取委托（捕获本读端与
    /// 取消令牌）——泛型类交叉引用会令单态化注册中断。
    /// </summary>
    /// <param name="cancellationToken">取消令牌（挂起读的协作取消信号）。</param>
    /// <returns>异步元素序列。</returns>
    public IAsyncEnumerable<T> ReadAllAsync(CancellationToken cancellationToken = default) {
        return new ChannelAllEnumerable<T>(() => this.ReadAsync(cancellationToken));
    }
}
