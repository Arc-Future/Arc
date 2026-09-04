// L3 Orm SQLite execute MVP：SqliteProvider — 连接 + prepare/step + DataRow 物化绿路径。
//
// 可证伪面：
//   - CreateConnection → SqliteConnection（非 null）
//   - ExecuteDataRows：Translate → Open → prepare/step → List<DataRow>
//   - 连接级 BeginTransaction / ExecuteBound（见 SqliteConnection）
//   - ExecuteAsync<T> / BeginTransactionAsync：显式 NotImplementedException（禁空假绿）
//
// 仍禁：新方言、完整实体 codegen 物化、完整 SQL Provider。
namespace Arc.Orm.SQLite;

using Arc;
using Arc.Collections;
using Arc.Data;
using Arc.Linq;
using Arc.Linq.Expressions;
using Arc.Orm;

public class SqliteProvider : IDbProvider {
    private string _connectionString;
    private bool _disposed;

    public SqliteProvider() {
        _connectionString = ":memory:";
        _disposed = false;
    }

    public SqliteProvider(string connectionString) {
        _connectionString = connectionString;
        if (_connectionString == null) {
            _connectionString = ":memory:";
        }
        if (_connectionString == "") {
            _connectionString = ":memory:";
        }
        _disposed = false;
    }

    public DatabaseKind Kind { get; } = DatabaseKind.Relational;

    public string ProviderName { get; } = "SQLite";

    public IDbConnection CreateConnection() {
        return new SqliteConnection(_connectionString);
    }

    /// <summary>Provider 级事务后置；连接级见 <see cref="SqliteConnection.BeginTransaction"/>。</summary>
    public Task<IDbTransaction> BeginTransactionAsync(CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        throw new NotImplementedException(
            "SqliteProvider.BeginTransactionAsync: use SqliteConnection.BeginTransaction (sync MVP)"
        );
    }

    /// <summary>Translate + prepare/step + DataTable 物化（可证伪查询绿路径）。</summary>
    public DataTable ExecuteDataRows(Expression expression) {
        SqlTranslator translator = new SqlTranslator();
        string sql = translator.Translate(expression);
        SqliteConnection conn = new SqliteConnection(_connectionString);
        conn.Open();
        DataTable table = conn.QueryDataRows(sql);
        conn.Close();
        conn.Dispose();
        return table;
    }

    public Task<List<T>> ExecuteAsync<T>(Expression expression, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        throw new NotImplementedException(
            "SqliteProvider.ExecuteAsync<T>: entity materializer not registered; use ExecuteDataRows / SqliteConnection.QueryDataRows"
        );
    }

    public Task<R> ExecuteScalarAsync<R>(Expression expression, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        throw new NotImplementedException(
            "SqliteProvider.ExecuteScalarAsync: scalar materializer not in execute-MVP scope"
        );
    }

    public void Dispose() {
        if (!_disposed) {
            _disposed = true;
        }
    }
}
