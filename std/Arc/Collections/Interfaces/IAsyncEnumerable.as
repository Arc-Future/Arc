namespace Arc.Collections;

/// <summary>
/// 异步序列接口——拉模型异步元素供给方（RFC 008 AsyncStream；对齐 C#
/// IAsyncEnumerable&lt;T&gt;）。取消经 GetAsyncEnumerator 的显式 ct 参数
/// 传递（生产者侧参数，消灭 WithCancellation 双轨）。
/// </summary>
/// <typeparam name="T">元素类型。</typeparam>
public interface IAsyncEnumerable<out T> {
    /// <summary>获取本序列的异步枚举器。</summary>
    /// <param name="cancellationToken">取消令牌（生产者生成流程的取消信号）。</param>
    /// <returns>异步枚举器实例。</returns>
    IAsyncEnumerator<T> GetAsyncEnumerator(CancellationToken cancellationToken);
}
