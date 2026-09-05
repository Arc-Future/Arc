// IDbTransaction —— 拆分自 IDbProvider.as（一文件一公开类型）。
namespace Arc.Data;
using Arc;
using Arc.Linq.Expressions;

/// <summary>数据库事务抽象（对标 C# DbTransaction 常用子集）。</summary>
public interface IDbTransaction : IDisposable {
    /// <summary>所属连接。</summary>
    IDbConnection Connection { get; }

    /// <summary>事务隔离级别。</summary>
    IsolationLevel IsolationLevel { get; }

    /// <summary>同步提交事务。</summary>
    void Commit();

    /// <summary>同步回滚事务。</summary>
    void Rollback();

    /// <summary>提交事务。</summary>
    Task CommitAsync();
    Task CommitAsync(CancellationToken cancellationToken);

    /// <summary>回滚事务。</summary>
    Task RollbackAsync();

    /// <summary>事务 ID（用于日志/调试）。</summary>
    Guid TransactionId { get; }
}
