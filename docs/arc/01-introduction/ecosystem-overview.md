# 仓库与示例全景

用 Arc 写程序时，日常接触的是这些路径（相对仓库根）：

| 路径 | 用途 |
|------|------|
| `examples/` | 可构建示例（如 `CompilerSmoke`、`ArmlDemo`） |
| `std/` | 标准库与领域库源码（`Arc`、`Arc.UI`、`Arc.Orm`…） |
| `docs/arc/` | **本手册**（官方可发布文档） |
| `docs/rfc/` | 设计决策权威（默认不发布） |

## 常用示例

| 示例 | 说明 |
|------|------|
| `examples/CompilerSmoke` | 最小编译冒烟 |
| `examples/ArmlDemo` | Arc.UI / ARML 桌面演示 |

```bash
cargo run -p arc -- run examples/CompilerSmoke
# 或已安装 SDK 后：
arc run examples/CompilerSmoke
```

## 文档分册（仓库内）

| 分册 | 角色 |
|------|------|
| [`docs/arc/`](../INDEX.md) | 开发者手册（上传根） |
| [`docs/rfc/`](https://github.com/Arc-Future/Arc/blob/main/docs/rfc/index.md) | RFC 设计权威 |

编译器实现位于 `crates/`——应用开发一般**不必**阅读，除非你在贡献编译器本身。
