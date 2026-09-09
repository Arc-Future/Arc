# 设计决策入口

本手册以**用法**为主。若你需要「为什么这样设计」或裁决语义冲突，请查阅仓库内的 **已接受 RFC**（不随本发布包上传）。

## 何时打开 RFC

- 手册与运行行为冲突，需要权威裁决  
- 贡献语言 / 标准库 / 领域库，要改语义边界  
- 实现编译器、运行时或 ABI  

应用开发日常**不必**通读 RFC。

## 快速索引（仓库路径）

| 主题 | RFC（仓库相对 `docs/`） |
|------|-------------------------|
| 语言宪章 / 表面契约 | `rfc/001-language-charter.md`、`rfc/002-surface-contract.md` |
| 类型 / 内存 / 对象 / 异步 | `rfc/004`–`rfc/010` |
| 查询与表达式树 | `rfc/011-expression-trees-query.md` |
| 标准库架构 | `rfc/020-std-architecture.md` |
| CLI 与构建 | `rfc/031-compiler-cli.md` |
| UI（wgpu 唯一后端） | `rfc/037-ui.md` |
| ORM / Web / AI / P2P | `rfc/039`–`rfc/042` |
| 完整目录 | [`rfc/index.md`](https://github.com/Arc-Future/Arc/blob/main/docs/rfc/index.md) |

编译管线、运行时 ABI、自举与成熟度门禁等**实现者材料**同样只在 `docs/rfc/` 与相关 crate 文档中维护，**不编入本手册主叙事**。

## 相关

- 手册目录：[INDEX.md](../INDEX.md)
