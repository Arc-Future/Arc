// Arc.Data 独立库：DataColumn — 数据列（对标 C# System.Data.DataColumn 常用子集）。
namespace Arc.Data;

/// <summary>
/// 数据列元数据：列名 + 列类型 + 序号。DataRow 按列序号经 <see cref="ColumnType"/>
/// 分派到类型化槽取值；DataTable 由列集合定义行结构。
/// </summary>
public class DataColumn {

    /// <summary>构造数据列。</summary>
    /// <param name="columnName">列名。</param>
    /// <param name="columnType">列类型。</param>
    public DataColumn(string columnName, ColumnType columnType) {
        this.ColumnName = columnName;
        this.ColumnType = columnType;
    }

    /// <summary>列名。</summary>
    public string ColumnName { get; }

    /// <summary>列类型（决定 DataRow 值槽分派）。</summary>
    public ColumnType ColumnType { get; }

    /// <summary>列序号（表内从 0 递增）。</summary>
    public int Ordinal { get; set; }
}
