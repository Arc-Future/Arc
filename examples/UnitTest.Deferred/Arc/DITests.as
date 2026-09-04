// RFC 023 M1：DI 挂账说明（UnitTest.Deferred）。
//
// L3 已解禁 · DI 编译器缺陷（ServiceProvider `List.Count` panic）已修复——
// 运行时注入可证伪面原见 crates/arc-integration/tests/di_runtime_injection_e2e.rs
//   （AddTransient + 构造器注入 + GetService(typeof(T)) 真实解析）；
// 类型可见性面原见 crates/arc-integration/tests/di_abstractions_e2e.rs
//   （两 e2e 均已随 arc-integration 退场，a2627a0f）。
//
// 本文件**故意无 [Fact]**——禁止 [Fact(Skip)] + Assert.True(true) 假绿。
// AddSingleton/AddScoped 完整生命周期语义 + keyed 服务 = 另排 DI 有边界 Sprint；
// **禁止**回迁 examples/UnitTest 顶绿。

namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

public class DITests
{
}
