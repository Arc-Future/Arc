// RFC 042 M11: P2P 网络库挂账说明（UnitTest.Deferred）。
//
// L3 已解禁 · P2P 仍 Deferred（禁止本切片开 P2P 新里程碑）。
// 可证伪纯逻辑面原见 crates/arc-integration/tests/l3_honesty_sweep_e2e.rs
//   （Multiaddr.GetValue / InMemoryPeerStore / FullMeshTopology / CircuitRelay NI；
//   该 e2e 已随 arc-integration 退场，a2627a0f）。
//
// 本文件**故意无 [Fact]**——禁止 [Fact(Skip)] + Assert.True(true) 假绿。
// 完整密码学 round-trip / Noise / 实网 = 另排 P2P Sprint；**禁止**回迁 UnitTest 顶绿。

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

public class P2PTests {
}
