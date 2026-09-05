// ConnectionState —— 拆分自 IDbProvider.as（一文件一公开类型）。
namespace Arc.Data;
using Arc;
using Arc.Linq.Expressions;

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
