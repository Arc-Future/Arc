// DatabaseKind —— 拆分自 IDbProvider.as（一文件一公开类型）。
namespace Arc.Data;
using Arc;
using Arc.Linq.Expressions;

/// <summary>数据库类型枚举。</summary>
public enum DatabaseKind {
    /// <summary>关系型数据库（SQLite、PostgreSQL、SQL Server）。</summary>
    Relational,

    /// <summary>文档型数据库（MongoDB、CosmosDB）。</summary>
    Document,

    /// <summary>键值型数据库（Redis）。</summary>
    KeyValue,

    /// <summary>内存数据库（测试/开发用）。</summary>
    InMemory,
}
