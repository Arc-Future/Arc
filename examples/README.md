# Arc Examples

自包含的 Arc 项目样例。**默认验证入口不是本目录的散落 demo**，而是：

1. **`examples/UnitTest`** — 语言 / 核心 std / QIF 的可证伪单元测试（零核心 `[Fact(Skip)]`）
2. **`cargo test --workspace`** — 编译器管线与必要的 build-and-run e2e（源码优先内联于测试，不依赖本目录第二套断言；原 `arc-integration` 已退场 a2627a0f，验证矩阵切换见 CHANGELOG）

立宪背景：[RFC 036](../docs/rfc/036-maturity.md) std-first；迭代纪律见仓库开发纪律文档（arc-iteration）。

## 准入与保留角色（Hard Rule）

每个 `examples/` 子目录必须有 `arc.toml`、能通过 `arc check`、有可运行入口（`Program.as` 或 `arc test`）。此外，**角色必须属于下表之一**，否则视为迭代污染，发现即清理：

| 角色 | 允许内容 | 禁止 |
|------|----------|------|
| **单元测试** | 仅 `UnitTest/`（核心）与 `UnitTest.Deferred/`（L3 / 未稳诚实隔离） | 在其它目录再堆与 UnitTest 重复的断言套件 |
| **门禁夹具** | RFC / §H 必需合成程序（如 `CompilerSmoke/`）；`crates/parse/fixtures` 在仓库其它路径，同等保留 | 用门禁夹具冒充「第二个测试套件」 |
| **文档 / 演示最小样例** | 短契约 demo（如 `ArmlDemo/`），不承担回归断言权威 | 与 UnitTest 双轨维护同一断言 |
| **工具夹具** | 仅被工具链/e2e 临时引用且无法内联的最小输入 | 空壳目录、已删源仍挂名的目录 |

**禁止**：在 `examples/` 再堆「第二个测试套件」（`Test*` / Native* dogfood / 演示型重复 stdout 断言等）。有价值用例 → **迁入 `UnitTest/Core|Arc|QIF`**（非 Skip）；管线 e2e 需要 build-and-run → **内联源码**（`support::build_and_run_source`）或改指 UnitTest，禁止双轨。

## 包命名

`[package].name` 采用 **PascalCase**，与目录名对齐。

## 构建与验证

```bash
# 默认验证（优先）
cargo run -p arc -- test examples/UnitTest
cargo test --workspace

# 单文件 / 门禁夹具
cargo run -p arc -- build examples/CompilerSmoke/Program.as
cargo run -p arc -- check examples/ArmlDemo/Program.as
```

## 当前保留项目

| 项目 | 角色 | 说明 |
|------|------|------|
| [UnitTest/](UnitTest/) | 单元测试 | 默认 `arc test` 入口；Core / Arc / QIF |
| [UnitTest.Deferred/](UnitTest.Deferred/) | L3 诚实隔离 | L3 **已解禁**但仍隔离未稳用例；有边界 Sprint 另排；**不**纳入默认发现；禁止 Skip 顶绿；≠ 假开全家桶 |
| [CompilerSmoke/](CompilerSmoke/) | 门禁夹具 | RFC 011 V3 / 自举入口合成程序 |
| [ArmlDemo/](ArmlDemo/) | 文档演示 | ARML 编码模型综合演示（合并原 ArmlHello/ArmlControls/ArmlList/ArmlImage/ArmlStyle/ArmlSlider/ArmlIme/XBindHello/ArmlVisualHost/ArmlCodeEditor）：单 Window + ScrollView 9 分区（原 opt-in `arml_demo_build` e2e 已随 arc-integration 退场，a2627a0f） |

已删除的历史 dogfood（AsyncLambda、Native*、StateMachine、Tensor、VectorSIMD、VarInference、OopFix、TestCV、Orm*Demo 等）断言已迁入 UnitTest 和/或内联 e2e（原 `arc-integration` 内联 e2e 已随该 crate 退场 a2627a0f），禁止恢复双轨目录。

## 单元测试布局

```
UnitTest/
├── Core/        # 语言核心 — 默认 `arc test` 入口
├── Arc/         # 核心 std（Collections / IO / Task / FFI 等）
├── QIF/         # 测试框架自测
└── GlobalUsings.as
```

```bash
cargo run -p arc -- test examples/UnitTest
cargo run -p arc -- test examples/UnitTest/Core/CoreLanguageTests.as
```

L2 Tasks Stable 面见 [RFC 009](../docs/rfc/009-async-concurrency.md)。

## 编码规范 (RFC 008)

- 入口：`Program.as`，无 `namespace`（测试项目除外），`using Arc;`
- 局部变量：前置类型或清晰 `var` 推断
- 类型/方法：PascalCase；参数/局部：camelCase
