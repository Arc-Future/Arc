// S4 (RFC 033 §2.6): Arc.Net.Quic — 报文/帧/流类型常量与流标识语义（RFC 9000）。
//
// 纯 Arc 常量层。流 ID 类型划分（§2.1）与帧类型（§12.4）是 QUIC 消费层的最小
// 语义面；实际传输（握手/重传/流控）由 rt_quic_* ABI 承载（e2e 以 Rust harness
// 直连验证）。常量命名对齐 RFC 9000。

namespace Arc.Net.Quic;

/// <summary>QUIC 流类型判定与常量（RFC 9000 §2.1）。</summary>
public class QuicStreamType {
    /// <summary>客户端发起双向流（id % 4 == 0）。</summary>
    public static bool IsClientBidi(long id) { return id % 4 == 0; }
    /// <summary>服务器发起双向流（id % 4 == 1）。</summary>
    public static bool IsServerBidi(long id) { return id % 4 == 1; }
    /// <summary>客户端发起单向流（id % 4 == 2）。</summary>
    public static bool IsClientUni(long id) { return id % 4 == 2; }
    /// <summary>服务器发起单向流（id % 4 == 3）。</summary>
    public static bool IsServerUni(long id) { return id % 4 == 3; }
    /// <summary>流为单向（id % 4 ∈ {2, 3}）。</summary>
    public static bool IsUni(long id) { return id % 4 >= 2; }
    /// <summary>流为双向（id % 4 ∈ {0, 1}）。</summary>
    public static bool IsBidi(long id) { return id % 4 < 2; }
}
