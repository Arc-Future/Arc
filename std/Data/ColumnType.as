// Arc.Data 独立库：ColumnType — 数据列类型（对标 C# System.Data.DataColumn.DataType 常用子集）。
namespace Arc.Data;

/// <summary>
/// 数据列类型。Arc 无 boxed object，列值以类型化槽存储，故以枚举标注列类型
/// 供 DataRow 类型化取值分派。涵盖数据库常用标量类型（对标 C# System.Data
/// 的 DataType 常用子集：Int32/Int64/Double/Boolean/String/DateTime/Guid）。
/// </summary>
public enum ColumnType {
    /// <summary>整型列（32 位，对标 C# int/Int32）。</summary>
    Int,

    /// <summary>长整型列（64 位，对标 C# long/Int64）。</summary>
    Long,

    /// <summary>浮点列（64 位，对标 C# double/Double）。</summary>
    Double,

    /// <summary>布尔列（对标 C# bool/Boolean）。</summary>
    Bool,

    /// <summary>字符串列（对标 C# string/String）。</summary>
    String,

    /// <summary>日期时间列（对标 C# System.DateTime）。</summary>
    DateTime,

    /// <summary>全局唯一标识列（对标 C# System.Guid）。</summary>
    Guid,
}