// L3 Orm SQLite execute MVP：SqliteConnection — 连接 + prepare/step + 绑定 + 事务边界。
namespace Arc.Orm.SQLite;

using Arc;
using Arc.Collections;
using Arc.Data;
using Arc.Orm;

/// <summary>
/// SQLite 连接（IDbConnection）。绿路径：Open / Exec / QueryDataRows / ExecuteBound / BeginTransaction。
/// IDbConnection.OpenAsync 仅保留 CancellationToken 重载（避免同名重载 itable 符号碰撞）。
/// </summary>
public class SqliteConnection : IDbConnection {
    /// <summary>sqlite3_step 返回码：有行可用（对应 C API SQLITE_ROW）。</summary>
    public static readonly int SqliteRow = 100;
    /// <summary>sqlite3_step 返回码：语句执行完成（对应 C API SQLITE_DONE）。</summary>
    public static readonly int SqliteDone = 101;

    private bool _disposed;

    public SqliteConnection(string connectionString) {
        this.ConnectionString = connectionString;
        if (this.ConnectionString == null) {
            this.ConnectionString = ":memory:";
        }
        if (this.ConnectionString == "") {
            this.ConnectionString = ":memory:";
        }
        this.DbHandle = 0;
        this.IsOpen = false;
        _disposed = false;
    }

    public string ConnectionString { get; }

    public bool IsOpen { get; set; }

    /// <summary>连接状态（Open/Closed）。</summary>
    public ConnectionState State {
        get {
            if (this.IsOpen) {
                return ConnectionState.Open;
            }
            return ConnectionState.Closed;
        }
    }

    /// <summary>连接超时（当前未设置，返回 0）。</summary>
    public int ConnectionTimeout { get; }

    /// <summary>当前数据库名（内存库返回 "main"；文件库返回文件名；未打开返回空字符串）。</summary>
    public string Database {
        get {
            if (!this.IsOpen) {
                return "";
            }
            if (this.ConnectionString == ":memory:") {
                return "main";
            }
            // 反向扫描最后一个 '/' 或 '\' 后的文件名段。
            int i = this.ConnectionString.Length - 1;
            int cut = -1;
            bool go = i >= 0;
            while (go) {
                string ch = this.ConnectionString.Substring(i, 1);
                if (ch == "/" || ch == "\\") {
                    cut = i;
                    go = false;
                } else {
                    i = i - 1;
                    go = i >= 0;
                }
            }
            if (cut < 0) {
                return this.ConnectionString;
            }
            return this.ConnectionString.Substring(cut + 1, this.ConnectionString.Length - cut - 1);
        }
    }

    /// <summary>内部 db 句柄（事务 / 绑定共用；0 = 未打开）。</summary>
    public int DbHandle { get; set; }

    public Task OpenAsync(CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        this.Open();
        return Task.CompletedTask;
    }

    /// <summary>同步打开（execute-MVP 可证伪入口）。</summary>
    public void Open() {
        if (_disposed) {
            throw new ObjectDisposedException("SqliteConnection");
        }
        if (this.IsOpen) {
            return;
        }
        this.DbHandle = SqliteDb.Open(this.ConnectionString);
        if (this.DbHandle == 0) {
            throw new InvalidOperationException("sqlite open failed");
        }
        this.IsOpen = true;
    }

    public Task CloseAsync() {
        this.Close();
        return Task.CompletedTask;
    }

    public void Close() {
        if (!this.IsOpen) {
            return;
        }
        if (this.DbHandle != 0) {
            SqliteDb.Close(this.DbHandle);
            this.DbHandle = 0;
        }
        this.IsOpen = false;
    }

    /// <summary>执行非查询 SQL（CREATE/INSERT/UPDATE/DELETE）。失败抛 InvalidOperationException。</summary>
    public int Execute(string sql) {
        this.EnsureOpen();
        int n = SqliteDb.Exec(this.DbHandle, sql);
        if (n < 0) {
            throw new InvalidOperationException("sqlite exec failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        return n;
    }

    /// <summary>异步执行非查询 SQL（带取消令牌）。</summary>
    /// <param name="sql">待执行的 SQL 语句。</param>
    /// <param name="cancellationToken">取消令牌。</param>
    public async Task ExecuteAsync(string sql, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        this.Execute(sql);
    }

    /// <summary>异步执行非查询 SQL。</summary>
    /// <param name="sql">待执行的 SQL 语句。</param>
    public async Task ExecuteAsync(string sql) {
        await this.ExecuteAsync(sql, new CancellationToken());
    }

    /// <summary>
    /// prepare → bind(text,int,int) → step。用于 <c>INSERT ... VALUES(?,?,?)</c>。
    /// 失败抛 InvalidOperationException（禁假成功）。
    /// </summary>
    public int ExecuteBound(string sql, string text0, int int1, int int2) {
        this.EnsureOpen();
        int stmt = SqliteDb.Prepare(this.DbHandle, sql);
        if (stmt == 0) {
            throw new InvalidOperationException("sqlite prepare failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        if (SqliteDb.BindText(stmt, 1, text0) != 0) {
            SqliteDb.Finalize(stmt);
            throw new InvalidOperationException("sqlite bind_text failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        if (SqliteDb.BindInt(stmt, 2, int1) != 0) {
            SqliteDb.Finalize(stmt);
            throw new InvalidOperationException("sqlite bind_int failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        if (SqliteDb.BindInt(stmt, 3, int2) != 0) {
            SqliteDb.Finalize(stmt);
            throw new InvalidOperationException("sqlite bind_int failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        int rc = SqliteDb.Step(stmt);
        SqliteDb.Finalize(stmt);
        if (rc != SqliteDone) {
            throw new InvalidOperationException("sqlite step failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        return SqliteDb.Changes(this.DbHandle);
    }

    /// <summary>
    /// prepare → step → 物化为 <see cref="DataTable"/>（列由 stmt 列名 + 列类型动态构建）。
    /// 可证伪最小查询面；完整实体物化器属后置。
    /// </summary>
    public DataTable QueryDataRows(string sql) {
        this.EnsureOpen();
        int stmt = SqliteDb.Prepare(this.DbHandle, sql);
        if (stmt == 0) {
            throw new InvalidOperationException("sqlite prepare failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        return this.StepMaterialize(stmt);
    }

    /// <summary>带一个 TEXT 绑定的查询（例如 <c>WHERE Name=?</c>）。</summary>
    public DataTable QueryDataRowsBound(string sql, string text0) {
        this.EnsureOpen();
        int stmt = SqliteDb.Prepare(this.DbHandle, sql);
        if (stmt == 0) {
            throw new InvalidOperationException("sqlite prepare failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        if (SqliteDb.BindText(stmt, 1, text0) != 0) {
            SqliteDb.Finalize(stmt);
            throw new InvalidOperationException("sqlite bind_text failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        return this.StepMaterialize(stmt);
    }

    /// <summary>同步事务：Exec BEGIN，返回 <see cref="SqliteTransaction"/>。</summary>
    public SqliteTransaction BeginTransaction() {
        this.EnsureOpen();
        this.Execute("BEGIN");
        return new SqliteTransaction(this);
    }

    private DataTable StepMaterialize(int stmt) {
        DataTable table = new DataTable();
        int n = SqliteDb.ColumnCount(stmt);
        // 先 step 到首行再建列：sqlite3_column_type 返回**当前行值**的类型，
        // 未 step 时全为 0（未知）→ 会误判全 String。StepMaterialize 在首行
        // 上建列，后续行沿用同一列元数据。
        bool hasRow = false;
        int rc = SqliteDb.Step(stmt);
        if (rc == SqliteRow) {
            hasRow = true;
            this.BuildColumns(table, stmt, n);
            while (rc == SqliteRow) {
                DataRow row = table.NewRow();
                this.MaterializeRow(row, stmt, n);
                table.AddRow(row);
                rc = SqliteDb.Step(stmt);
            }
        }
        // 空结果集无行值可判类型：以列名建 String 列（类型未知保持诚实）。
        if (!hasRow) {
            this.BuildColumns(table, stmt, n);
        }
        SqliteDb.Finalize(stmt);
        if (rc != SqliteDone) {
            throw new InvalidOperationException("sqlite step failed: " + SqliteDb.Errmsg(this.DbHandle));
        }
        return table;
    }

    /// <summary>按 stmt 列名 + 列类型（SQLITE_*）动态构建列元数据。</summary>
    private void BuildColumns(DataTable table, int stmt, int n) {
        int i = 0;
        while (i < n) {
            string col = SqliteDb.ColumnName(stmt, i);
            ColumnType t = this.MapColumnType(SqliteDb.ColumnType(stmt, i));
            table.AddColumn(col, t);
            i = i + 1;
        }
    }

    /// <summary>
    /// SQLite 列类型码 → ColumnType。INTEGER→Int；FLOAT→Double；TEXT/BLOB→String。
    /// NULL→String（无行值可判类型，诚实回退）。列类型按首行值判定，后续行复用。
    /// </summary>
    private ColumnType MapColumnType(int sqliteType) {
        if (sqliteType == 1) {
            return ColumnType.Int;
        }
        if (sqliteType == 2) {
            return ColumnType.Double;
        }
        return ColumnType.String;
    }

    private void MaterializeRow(DataRow row, int stmt, int n) {
        int i = 0;
        while (i < n) {
            // 按**当前行值**类型识别 NULL：sqlite3_column_type 返回当前行值类型，
            // 5 = SQLITE_NULL。列类型由首行判定，但后续行该列可为 NULL，故逐行检测。
            if (SqliteDb.ColumnType(stmt, i) == 5) {
                row.SetNull(i);
                i = i + 1;
                continue;
            }
            ColumnType t = row.Table.GetColumnType(i);
            if (t == ColumnType.Int) {
                row.SetIntValue(i, SqliteDb.ColumnInt(stmt, i));
            } else if (t == ColumnType.Double) {
                row.SetDoubleValue(i, SqliteDb.ColumnDouble(stmt, i));
            } else {
                row.SetStringValue(i, SqliteDb.ColumnText(stmt, i));
            }
            i = i + 1;
        }
    }

    private void EnsureOpen() {
        if (_disposed) {
            throw new ObjectDisposedException("SqliteConnection");
        }
        if (!this.IsOpen) {
            throw new InvalidOperationException("SqliteConnection is not open");
        }
        if (this.DbHandle == 0) {
            throw new InvalidOperationException("SqliteConnection is not open");
        }
    }

    public void Dispose() {
        if (_disposed) {
            return;
        }
        this.Close();
        _disposed = true;
    }
}
