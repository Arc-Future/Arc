namespace Arc;

/// <summary>异步资源释放接口。</summary>
/// <remarks>
/// <c>await using</c> 语句要求资源类型实现本接口；
/// 脱糖为 <c>let r = expr; try { ... } finally { await r.DisposeAsync(); }</c>。
/// 与 <see cref="IDisposable"/> 独立——类型可同时实现两者。
/// </remarks>
public interface IAsyncDisposable {
    /// <summary>异步释放资源。</summary>
    /// <returns>释放完成时 resolve 的 Task。</returns>
    Task DisposeAsync();
}
