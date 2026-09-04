// L3 Orm SQLite execute MVP：SqliteDb — rt_sqlite_* Builtin 门面。
//
// 1-based int 句柄（0 = 无效）。不扩 IDbProvider；供 SqliteConnection 调用。
namespace Arc.Orm.SQLite;

/// <summary>SQLite C ABI 门面（codegen → rt_sqlite_*）。</summary>
public class SqliteDb {
    /// <summary>打开数据库。path 空或 ":memory:" → 内存库。失败返回 0。</summary>
    [Builtin(ABI = "rt_sqlite_open")]
    public static int Open(string path) { return 0; }

    [Builtin(ABI = "rt_sqlite_close")]
    public static void Close(int db) { }

    /// <summary>执行非查询 SQL。成功返回受影响行数；失败 -1。</summary>
    [Builtin(ABI = "rt_sqlite_exec")]
    public static int Exec(int db, string sql) { return -1; }

    /// <summary>prepare。失败返回 0。</summary>
    [Builtin(ABI = "rt_sqlite_prepare")]
    public static int Prepare(int db, string sql) { return 0; }

    /// <summary>step。100 = ROW，101 = DONE。</summary>
    [Builtin(ABI = "rt_sqlite_step")]
    public static int Step(int stmt) { return -1; }

    [Builtin(ABI = "rt_sqlite_column_count")]
    public static int ColumnCount(int stmt) { return 0; }

    /// <summary>列数据类型。1=INTEGER 2=FLOAT 3=TEXT 4=BLOB 5=NULL。</summary>
    [Builtin(ABI = "rt_sqlite_column_type")]
    public static int ColumnType(int stmt, int col) { return 0; }

    [Builtin(ABI = "rt_sqlite_column_int")]
    public static int ColumnInt(int stmt, int col) { return 0; }

    [Builtin(ABI = "rt_sqlite_column_double")]
    public static double ColumnDouble(int stmt, int col) { return 0.0; }

    [Builtin(ABI = "rt_sqlite_column_text")]
    public static string ColumnText(int stmt, int col) { return ""; }

    [Builtin(ABI = "rt_sqlite_column_name")]
    public static string ColumnName(int stmt, int col) { return ""; }

    [Builtin(ABI = "rt_sqlite_finalize")]
    public static void Finalize(int stmt) { }

    [Builtin(ABI = "rt_sqlite_errmsg")]
    public static string Errmsg(int db) { return ""; }

    /// <summary>绑定 TEXT 参数（1-based）。0 = ok；-1 = fail。</summary>
    [Builtin(ABI = "rt_sqlite_bind_text")]
    public static int BindText(int stmt, int index, string text) { return -1; }

    /// <summary>绑定 INT 参数（1-based）。0 = ok；-1 = fail。</summary>
    [Builtin(ABI = "rt_sqlite_bind_int")]
    public static int BindInt(int stmt, int index, int value) { return -1; }

    /// <summary>最近语句受影响行数。无效句柄 -1。</summary>
    [Builtin(ABI = "rt_sqlite_changes")]
    public static int Changes(int db) { return -1; }
}
