// RFC 051 D1/D6：DependencyProperty 挂账说明（UnitTest.Deferred）。
//
// L3 已解禁 · 骨架内有边界：纯逻辑可证伪面原见
//   crates/arc-integration/tests/ui_skeleton_honesty_e2e.rs
//   （Thickness/LayoutSize + DP Registry / RegisterProperty；该 e2e 已随
//   arc-integration 退场 a2627a0f）
//
// 本文件**故意无 [Fact]**——禁止 [Fact(Skip)] + Assert.True 假绿。
// Element.GetValue/SetValue、Window 属性 wrapper、ARML / wgpu = 另排 UI 扩张 Sprint；
// **禁止**回迁 examples/UnitTest 顶绿。

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

public class DependencyPropertyTests
{
}
