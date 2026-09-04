// Arc.Data 独立库：IDbConnectionPool — 连接池抽象契约（对标 ADO.NET 连接池；无具体实现；≠ 可用池）。
namespace Arc.Data;

using Arc;

/// <summary>
/// 数据库连接池抽象——singleton 生命周期，线程安全。
///
/// 由具体后端实现（SqliteConnectionPool、MongoConnectionPool）。
/// DbContext 从池租约连接，SaveChangesAsync 后归还。
/// </summary>
public interface IDbConnectionPool {
    /// <summary>池中最大连接数。</summary>
    int MaxSize { get; }

    /// <summary>当前空闲连接数。</summary>
    int AvailableCount { get; }

    /// <summary>
    /// 异步租约连接（池空则等待或新建，取决于溢出策略）。
    ///
    /// 高并发安全：多个 DbContext 实例并发调用，池内部锁保护空闲列表。
    /// </summary>
    /// <param name="cancellationToken">取消令牌。</param>
    /// <returns>可用的数据库连接。</returns>
    Task<IDbConnection> LeaseAsync(CancellationToken cancellationToken);

    /// <summary>
    /// 归还连接（放回空闲列表，供后续租约复用）。
    ///
    /// 必须由 DbContext 在 SaveChangesAsync 完成后调用，避免连接泄漏。
    /// </summary>
    /// <param name="connection">待归还的连接。</param>
    void Release(IDbConnection connection);

    /// <summary>
    /// 异步租约连接（无 CancellationToken 重载，内部用默认令牌）。
    /// </summary>
    /// <returns>可用的数据库连接。</returns>
    Task<IDbConnection> LeaseAsync();
}