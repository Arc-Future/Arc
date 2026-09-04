// L3 骨架：PostgreSqlProvider — 关系型后端入口（执行链未落地）。
//
// 诚实边界（禁假绿）：
//   - CreateConnection / BeginTransactionAsync / Execute* → NotImplementedException
//   - 禁止返回空 List / null 冒充可查询 PostgreSQL
// 接口仅 CT 重载（无参重载会 itable 符号碰撞）。
namespace Arc.Orm.PostgreSQL;

using Arc;
using Arc.Collections;
using Arc.Data;
using Arc.Linq;
using Arc.Linq.Expressions;

internal class PostgreSqlProvider : IDbProvider {
    private string _connectionString;
    private bool _disposed;

    public PostgreSqlProvider() {
        _connectionString = "";
        _disposed = false;
    }

    public PostgreSqlProvider(string connectionString) {
        _connectionString = connectionString;
        _disposed = false;
    }

    public DatabaseKind Kind { get; } = DatabaseKind.Relational;

    public string ProviderName { get; } = "PostgreSQL";

    public IDbConnection CreateConnection() {
        throw new NotImplementedException(
            "PostgreSqlProvider.CreateConnection: PostgreSQL connection not implemented (L3 deferred)."
        );
    }

    public Task<IDbTransaction> BeginTransactionAsync(CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string msg = "PostgreSqlProvider.BeginTransactionAsync: PostgreSQL transaction not implemented (L3 deferred).";
        if (msg.Length > 0) {
            throw new NotImplementedException(msg);
        }
        return null;
    }

    public Task<List<T>> ExecuteAsync<T>(Expression expression, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string msg = "PostgreSqlProvider.ExecuteAsync: SQL translate + wire protocol not implemented (L3 deferred).";
        if (msg.Length > 0) {
            throw new NotImplementedException(msg);
        }
        return null;
    }

    public Task<R> ExecuteScalarAsync<R>(Expression expression, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string msg = "PostgreSqlProvider.ExecuteScalarAsync: scalar path not implemented (L3 deferred).";
        if (msg.Length > 0) {
            throw new NotImplementedException(msg);
        }
        return null;
    }

    public void Dispose() {
        if (!_disposed) {
            _disposed = true;
        }
    }
}
