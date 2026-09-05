// IsolationLevel —— 拆分自 IDbProvider.as（一文件一公开类型）。
namespace Arc.Data;
using Arc;
using Arc.Linq.Expressions;

/// <summary>事务隔离级别枚举（对齐 ADO.NET <c>System.Data.IsolationLevel</c> 常用子集）。</summary>
public enum IsolationLevel {
    /// <summary>未指定。</summary>
    Unspecified = 0,

    /// <summary>脏读（允许读取未提交数据）。</summary>
    ReadUncommitted = 1,

    /// <summary>已提交读（默认；禁止脏读）。</summary>
    ReadCommitted = 2,

    /// <summary>可重复读。</summary>
    RepeatableRead = 3,

    /// <summary>可序列化（最高隔离）。</summary>
    Serializable = 4,

    /// <summary>快照隔离。</summary>
    Snapshot = 5,
}
