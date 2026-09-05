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
