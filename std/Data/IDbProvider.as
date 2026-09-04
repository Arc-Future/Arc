// Arc.Data 独立库：IDbProvider — 数据库提供者抽象接口。
//
// Arc 异步一体原则 + C# 命名规范：
//   - I/O 方法 Async 后缀 + CancellationToken
//   - 无参重载收敛：同名重载会导致 itable 符号碰撞（仅保留 CT 重载）
//
// 设计目标：
//   - 关系型 / 文档型共用核心接口；DbContext 与具体后端解耦
//   - 数据库基础设施独立于 ORM 框架层（Arc.Orm），供任何数据访问层复用
namespace Arc.Data;

using Arc;
using Arc.Linq.Expressions;

/// <summary>数据库连接抽象（对标 C# DbConnection 常用子集）。</summary>
public interface IDbConnection : IDisposable {
    /// <summary>打开连接。</summary>
    Task OpenAsync(CancellationToken cancellationToken);

    /// <summary>关闭连接。</summary>
    Task CloseAsync();

    /// <summary>连接是否已打开（等价 <see cref="State"/> == <see cref="ConnectionState.Open"/>）。</summary>
    bool IsOpen { get; }

    /// <summary>当前连接状态。</summary>
    ConnectionState State { get; }

    /// <summary>连接字符串。</summary>
    string ConnectionString { get; }

    /// <summary>连接超时秒数（0 = 未设置/无限）。</summary>
    int ConnectionTimeout { get; }

    /// <summary>当前数据库名（未打开可为空字符串）。</summary>
    string Database { get; }
}

/// <summary>数据库事务抽象（对标 C# DbTransaction 常用子集）。</summary>
public interface IDbTransaction : IDisposable {
    /// <summary>所属连接。</summary>
    IDbConnection Connection { get; }

    /// <summary>事务隔离级别。</summary>
    IsolationLevel IsolationLevel { get; }

    /// <summary>同步提交事务。</summary>
    void Commit();

    /// <summary>同步回滚事务。</summary>
    void Rollback();

    /// <summary>提交事务。</summary>
    Task CommitAsync();
    Task CommitAsync(CancellationToken cancellationToken);

    /// <summary>回滚事务。</summary>
    Task RollbackAsync();

    /// <summary>事务 ID（用于日志/调试）。</summary>
    Guid TransactionId { get; }
}

/// <summary>连接状态枚举（对齐 ADO.NET <c>System.Data.ConnectionState</c> 常用子集）。</summary>
public enum ConnectionState {
    /// <summary>已关闭。</summary>
    Closed = 0,

    /// <summary>已打开。</summary>
    Open = 1,

    /// <summary>正在打开。</summary>
    Connecting = 2,

    /// <summary>正在执行命令。</summary>
    Executing = 4,

    /// <summary>正在读取数据。</summary>
    Fetching = 8,

    /// <summary>连接已损坏。</summary>
    Broken = 16,
}

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

/// <summary>数据库提供者抽象——ORM 框架通过此接口与具体数据库交互。</summary>
public interface IDbProvider : IQueryProvider, IDisposable {
    /// <summary>数据库类别（关系型/文档型等粗粒度分类，封闭枚举；见 <see cref="DatabaseKind"/>）。</summary>
    DatabaseKind Kind { get; }

    /// <summary>数据库提供程序名称（开放字符串，如 "SQLite" / "MySQL" / "PostgreSQL" / "MongoDB"）。
    /// 新提供程序只需返回其自身名称，无需改动 <see cref="DatabaseKind"/> 枚举——具体数据库不断涌现，
    /// 由开放字符串承载，封闭枚举仅作粗粒度分类。</summary>
    string ProviderName { get; }

    /// <summary>创建新连接。</summary>
    IDbConnection CreateConnection();

    /// <summary>开始事务。</summary>
    Task<IDbTransaction> BeginTransactionAsync(CancellationToken cancellationToken);
}

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