// IDbConnection —— 拆分自 IDbProvider.as（一文件一公开类型）。
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
