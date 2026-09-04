// Arc.Data 独立库：PoolOverflowPolicy — 连接池溢出策略枚举（对标 ADO.NET 连接池溢出策略）。
namespace Arc.Data;

/// <summary>
/// 连接池溢出策略——池满时的行为。
/// </summary>
internal enum PoolOverflowPolicy {
    /// <summary>等待空闲连接（默认，阻塞直到有连接归还）。</summary>
    Wait,
    /// <summary>新建临时连接（超出 MaxSize，归还后立即销毁）。</summary>
    Grow,
    /// <summary>抛出异常（拒绝请求，要求调用方重试）。</summary>
    Fail,
}