// Arc.Data 独立库：IDataReader — 数据读取器接口（对标 ADO.NET `System.Data.IDataReader`）。
namespace Arc.Data;

using Arc;

/// <summary>
/// 数据读取器接口——具体后端实现（SqliteDataReader、MongoDataReader）。
///
/// 提供 GetInt/GetString 等类型化访问器，避免装箱。通用数据读取契约，
/// 供任意数据访问层（含 ORM 物化器）在 Arc.Data 层面复用。
///
/// 生命周期语义对齐 ADO.NET：<see cref="Read"/> 前移游标；结束后调用
/// <see cref="Close"/> 释放资源；<see cref="NextResult"/> 推进到下一个结果集。
/// </summary>
public interface IDataReader {
    /// <summary>当前结果集列数；未定位时可用。</summary>
    int FieldCount { get; }

    /// <summary>当前行是否还有下一行（移动到下一行后返回 true，末尾返回 false）。</summary>
    bool Read();

    /// <summary>推进到下一个结果集；无更多结果集返回 false。</summary>
    bool NextResult();

    /// <summary>关闭读取器并释放底层资源。</summary>
    void Close();

    /// <summary>受影响的记录数（非查询语句）；-1 表示不适用。</summary>
    int RecordsAffected { get; }

    /// <summary>按列序号取列名。</summary>
    string GetName(int ordinal);

    /// <summary>按列名查找列序号；未找到返回 -1。</summary>
    int GetOrdinal(string columnName);

    /// <summary>按列序号读取 int 值。</summary>
    int GetInt(int ordinal);

    /// <summary>按列序号读取 long 值。</summary>
    long GetLong(int ordinal);

    /// <summary>按列序号读取 double 值。</summary>
    double GetDouble(int ordinal);

    /// <summary>按列序号读取 string 值。</summary>
    string GetString(int ordinal);

    /// <summary>按列序号读取 bool 值。</summary>
    bool GetBool(int ordinal);

    /// <summary>按列序号读取日期时间值。</summary>
    DateTime GetDateTime(int ordinal);

    /// <summary>按列序号读取 GUID 值。</summary>
    Guid GetGuid(int ordinal);

    /// <summary>是否为 NULL 值。</summary>
    bool IsNull(int ordinal);
}