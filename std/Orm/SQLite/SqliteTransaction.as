// L3 Orm SQLite execute-MVP：同步事务边界（BEGIN/COMMIT/ROLLBACK）。
//
// 故意不实现 IDbTransaction（Guid TransactionId 返回 ABI 未收口）；
// Provider.BeginTransactionAsync 仍 NotImplemented。
// Dispose 未提交则 Rollback（禁静默丢事务）。
namespace Arc.Orm.SQLite;

using Arc;

/// <summary>SQLite 连接级同步事务（BEGIN/COMMIT/ROLLBACK）。</summary>
public class SqliteTransaction : IDisposable {
    private SqliteConnection _connection;
    private bool _completed;
    private bool _disposed;

    public SqliteTransaction(SqliteConnection connection) {
        _connection = connection;
        _completed = false;
        _disposed = false;
    }

    /// <summary>同步提交。</summary>
    public void Commit() {
        this.EnsureActive();
        _connection.Execute("COMMIT");
        _completed = true;
    }

    /// <summary>同步回滚。</summary>
    public void Rollback() {
        if (_disposed) {
            return;
        }
        if (_completed) {
            return;
        }
        _connection.Execute("ROLLBACK");
        _completed = true;
    }

    private void EnsureActive() {
        if (_disposed) {
            throw new ObjectDisposedException("SqliteTransaction");
        }
        if (_completed) {
            throw new InvalidOperationException("SqliteTransaction already completed");
        }
    }

    public void Dispose() {
        if (_disposed) {
            return;
        }
        if (!_completed) {
            this.Rollback();
        }
        _disposed = true;
    }
}
