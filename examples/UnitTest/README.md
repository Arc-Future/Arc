# UnitTest — stdlib 权威单元测试套件

Arc 语言与核心 std 的 **QIF 单元测试**套件。对标 `dotnet test` 对 .NET BCL 的地位，是 stdlib 质量的唯一权威闸门。

政策：[examples/README.md](../README.md) · [RFC 032 QIF](../../docs/rfc/032-qif.md) · [RFC 036 成熟度](../../docs/rfc/036-maturity.md)

## 硬约束

- **核心零 `[Fact(Skip)]`** — 禁止用 Fact-Skip 粉饰绿
- `Assert.Skip` 仅契约自测（计 Skipped），≠ Fact-Skip
- 与 std 重复的断言 → 迁入本套件或删除，禁止双轨
- 任何进入本目录的测试必须有对应的 stdlib 生产代码与稳定面承诺
- 测试文件命名空间应与被测 std 子库对齐（`Arc.Collections` / `Arc.Tasks` / ...）

## 布局（按 std 子库边界分治）

| 目录 | 覆盖 | 命名空间 |
|------|------|----------|
| `Core/` | 语言核心：类型、泛型、OOP、异常、async lambda、状态机、表达式树等 | `Core.*` |
| `Arc/Collections` | `Arc.Collections.*`（List/Dictionary/HashSet/Sorted* 等） | `Arc.Collections` |
| `Arc/Tasks` | `Arc.Tasks.*`（Task、CancellationToken、TaskCompletionSource、Parallel） | `Arc.Tasks` |
| `Arc/Math` | `Arc.Math.*`（Math、Vector*、Matrix、Tensor、Quaternion） | `Arc.Math` |
| `Arc/Text` | `Arc.Text.*`（StringBuilder、Regex、Encoding、Json、Xml） | `Arc.Text` |
| `Arc/IO` | `Arc.IO.*`（File、Stream、MemoryStream、Path） | `Arc.IO` |
| `Arc/Net` | `Arc.Net.*`（Socket、HttpClient、TcpClient、WebSocket） | `Arc.Net` |
| `Arc/Security` | `Arc.Security.*`（SHA*/AES/HMAC/MD5 等） | `Arc.Security` |
| `Arc/Threading` | `Arc.Threading.*`（Lock/Mutex/Monitor/ThreadPool/Parallel） | `Arc.Threading` |
| `Arc/Diagnostics` | `Arc.Diagnostics.*`（Stopwatch、Process、管道） | `Arc.Diagnostics` |
| `Arc/Types` | `Arc.Types.*`（Lazy、Random、Guid、DateTime、Version） | `Arc.Types` |
| `Arc/ComponentModel` | `Arc.ComponentModel.*`（Bindable、Command） | `Arc.ComponentModel` |
| `QIF/` | QIF 框架自测：Assert 稳定面、Theory、Lifecycle、Parallel | `Arc.QIF` |
| `AI/` | AI 子库契约测试（Agent/Host/Session） | `Arc.Agent.*` |

## QIF 生产级能力

本套件使用 QIF v1 生产级测试框架，对标 XUnit：

| 能力 | CLI | 说明 |
|------|-----|------|
| **全量执行** | `arc test examples/UnitTest` | 默认所有测试 |
| **批量选择** | `arc test examples/UnitTest --namespace Arc.Collections` | 按命名空间前缀选择 |
| **Kind 过滤** | `arc test examples/UnitTest --kind Theory` | 仅跑 Theory 参数化用例 |
| **XUnit 表达式** | `arc test ... --filter "FullyQualifiedName~ListTests"` | 类名/方法名 contains |
| **AND 组合** | `arc test ... --filter "Trait~category=unit&ClassName~List"` | 多条件与 |
| **OR/NOT** | `arc test ... --filter "Fact\|Theory&!Trait~skip"` | 或/非组合 |
| **列出** | `arc test ... --list-tests` / `--list-format json` | 稳定字典序输出 |
| **并行** | `arc test ... --parallel --max-parallel 8` | 真实并行（Lock 保护） |
| **报告** | `arc test ... --logger json` / `junit` | CI 友好格式 |
| **零构建** | `arc test ... --no-build` | 跳过编译直接跑二进制 |

## 运行

```bash
# 全量
cargo run -p arc -- test examples/UnitTest

# 批量：仅跑 Collections 子库
cargo run -p arc -- test examples/UnitTest --namespace Arc.Collections

# 过滤：运行 ListTests 与 DictionaryTests
cargo run -p arc -- test examples/UnitTest --filter "FullyQualifiedName~ListTests|FullyQualifiedName~DictionaryTests"

# 列出测试（稳定序）
cargo run -p arc -- test examples/UnitTest --list-tests

# 列出测试（JSON，CI 消费）
cargo run -p arc -- test examples/UnitTest --list-tests --list-format json

# 并行 + JSON 报告
cargo run -p arc -- test examples/UnitTest --parallel --max-parallel 8 --logger json
```

## 验收判据（RFC 036 §4）

- **零 Fact-Skip**：整个套件不得以 `[Fact(Skip)]` 绿
- **QIF 表达式 100% 覆盖**：filter 所有 XUnit 语法分支经自测
- **Assert 命令面完备**：Equal/集合/谓词/异常/类型/比较/跳过 全覆盖
- **并行安全**：并行执行下结果与串行一致
- **报告持久化**：`report.json` 与 `run.arcqif` 可落盘供 CI 归档
