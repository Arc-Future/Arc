// Arc.Data 独立库：DataTable — 数据表（对标 C# System.Data.DataTable 常用子集）。
namespace Arc.Data;

using Arc;
using Arc.Collections;

/// <summary>
/// 数据表：列元数据集合 + 数据行集合。C#-aligned 动态结果集载体，
/// 供物化器（如 SQLite 查询）将原始行转为按列类型化访问的 DataRow。
///
/// 行集合语义对齐 C# DataTable.Rows：<see cref="Clear"/> 仅清空行保留列；
/// <see cref="RemoveRowAt"/> / <see cref="RemoveRow"/> 删除行。列集合经
/// <see cref="RemoveColumnAt"/> / <see cref="ClearColumns"/> 维护（列删除仅在
/// 无行时允许——行按列序号建槽，删列会使既有行槽错位，报错 > 静默）。
/// </summary>
public class DataTable {
    private List<DataColumn> _columns;
    private List<DataRow> _rows;

    /// <summary>构造空数据表。</summary>
    public DataTable() {
        _columns = new List<DataColumn>();
        _rows = new List<DataRow>();
    }

    // ── 列集合 ──

    /// <summary>新增一列并返回其序号。</summary>
    /// <param name="columnName">列名。</param>
    /// <param name="columnType">列类型。</param>
    public int AddColumn(string columnName, ColumnType columnType) {
        DataColumn col = new DataColumn(columnName, columnType);
        col.Ordinal = _columns.Count;
        _columns.Add(col);
        return col.Ordinal;
    }

    /// <summary>列数。</summary>
    public int ColumnCount() {
        return _columns.Count;
    }

    /// <summary>按序号取列。</summary>
    public DataColumn GetColumn(int ordinal) {
        DataColumn c = _columns[ordinal];
        return c;
    }

    /// <summary>按序号取列类型。</summary>
    public ColumnType GetColumnType(int ordinal) {
        DataColumn c = _columns[ordinal];
        return c.ColumnType;
    }

    /// <summary>按列名查序号；不存在返回 -1。</summary>
    public int GetOrdinal(string columnName) {
        int n = _columns.Count;
        int i = 0;
        while (i < n) {
            if (_columns[i].ColumnName == columnName) {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }

    /// <summary>删除指定序号的列；存在数据行时抛 InvalidOperationException（禁行槽错位）。</summary>
    public void RemoveColumnAt(int ordinal) {
        if (_rows.Count != 0) {
            throw new InvalidOperationException("DataTable.RemoveColumnAt: cannot remove column while rows exist");
        }
        int n = _columns.Count;
        int i = ordinal;
        while (i < n) {
            DataColumn c = _columns[i];
            c.Ordinal = i - 1;
            i = i + 1;
        }
        _columns.RemoveAt(ordinal);
    }

    /// <summary>删除指定列；存在数据行时抛 InvalidOperationException。</summary>
    public bool RemoveColumn(DataColumn column) {
        int ord = _columns.IndexOf(column);
        if (ord < 0) {
            return false;
        }
        this.RemoveColumnAt(ord);
        return true;
    }

    /// <summary>清空全部列；存在数据行时抛 InvalidOperationException。</summary>
    public void ClearColumns() {
        if (_rows.Count != 0) {
            throw new InvalidOperationException("DataTable.ClearColumns: cannot clear columns while rows exist");
        }
        _columns.Clear();
    }

    /// <summary>列集合只读视图（可与 foreach 配合枚举列）。</summary>
    public List<DataColumn> Columns() {
        return _columns;
    }

    // ── 行集合 ──

    /// <summary>新建一行（槽数与当前列数一致）。</summary>
    public DataRow NewRow() {
        return new DataRow(this);
    }

    /// <summary>新增一数据行。</summary>
    /// <param name="row">数据行（通常经 <see cref="NewRow"/> 创建）。</param>
    public void AddRow(DataRow row) {
        _rows.Add(row);
    }

    /// <summary>行数。</summary>
    public int RowCount() {
        return _rows.Count;
    }

    /// <summary>按序号取数据行。</summary>
    public DataRow GetRow(int index) {
        return _rows[index];
    }

    /// <summary>删除指定数据行；不存在返回 false。</summary>
    public bool RemoveRow(DataRow row) {
        return _rows.Remove(row);
    }

    /// <summary>删除指定序号的数据行；下标越界抛异常。</summary>
    public void RemoveRowAt(int index) {
        _rows.RemoveAt(index);
    }

    /// <summary>清空全部数据行（保留列元数据，对齐 C# DataTable.Clear）。</summary>
    public void Clear() {
        _rows.Clear();
    }

    /// <summary>行集合只读视图（可与 foreach 配合枚举行）。</summary>
    public List<DataRow> Rows() {
        return _rows;
    }
}