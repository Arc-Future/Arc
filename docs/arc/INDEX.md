# Arc 开发者手册 · 目录

> 面向用 Arc 写程序的开发者 · 渐进式披露 · Docbit slug: `arc`

---

## 开篇

| 文档 | 说明 |
|------|------|
| [前言](FOREWORD.md) | 本书定位、读者画像、阅读路径 |
| [上传说明](UPLOAD.md) | 拷贝到 `<app_base>/docs/arc/` |

---

## 第一部分 · 入门

### [第一章 认识 Arc](01-introduction/INDEX.md)

- [什么是 Arc](01-introduction/what-is-arc.md)
- [适用场景与边界](01-introduction/who-should-use.md)
- [语言宣言](01-introduction/manifesto.md)
- [仓库与示例全景](01-introduction/ecosystem-overview.md)

### [第二章 快速上手](02-quickstart/INDEX.md)

- [安装与快速开始](02-quickstart/getting-started.md)
- [构建与运行](02-quickstart/build-run.md)

---

## 第二部分 · 语言基础

### [第三章 编码与语法](03-syntax/INDEX.md)

- [编码与语法标准](03-syntax/encoding-standard.md)
- [词法与语法](03-syntax/lexicon-syntax.md)

### [第四章 类型系统](04-types/INDEX.md)

- [类型系统](04-types/type-system.md)

### [第五章 内存与资源](05-memory/INDEX.md)

- [内存与资源](05-memory/memory-resources.md)

### [第六章 对象模型](06-objects/INDEX.md)

- [对象模型](06-objects/object-model.md)

### [第七章 异步与任务](07-async/INDEX.md)

- [异步与任务](07-async/async-tasks.md)

### [第八章 查询与表达式树](08-query/INDEX.md)

- [查询语言](08-query/query-language.md)
- [表达式树](08-query/expression-trees.md)

---

## 第三部分 · 标准库与工具链

### [第九章 标准库导览](09-stdlib/INDEX.md)

- [标准库导览](09-stdlib/standard-library.md)

### [第十章 工具链](10-toolchain/INDEX.md)

- [编译器 CLI](10-toolchain/compiler-cli.md)
- [arc.toml 项目清单](10-toolchain/arc-toml-reference.md)
- [Native 组件集成](10-toolchain/native-integration.md)
- [热重载编排](10-toolchain/hot-reload.md)

---

## 第四部分 · 领域库

### [第十一章 领域库使用](11-domain/INDEX.md)

- [Arc.UI](11-domain/ui.md)
- [Arc.Agent](11-domain/ai-host.md)
- [Arc.AI](11-domain/ai-inference.md)
- [Arc.Orm](11-domain/orm.md)
- [Arc.Web](11-domain/web.md)
- [Arc.Net](11-domain/networking-p2p.md)
- [Arc.DI](11-domain/di.md)
- [Arc.Chord](11-domain/chord.md)
- [拟真引擎（规划）](11-domain/realism-engine.md)

---

## 第五部分 · 进阶与设计入口

### [第十二章 进阶与设计](12-advanced/INDEX.md)

- [结构化诊断](12-advanced/diagnostics.md)
- [能力系统](12-advanced/capabilities.md)
- [设计决策入口](12-advanced/design-decisions.md)（链到仓库 `docs/rfc/`，非 RFC 全文）
- [术语表](12-advanced/glossary.md)
- [符号约定](12-advanced/notation.md)

---

**不收录**：RFC 全文目录、编译器管线实现专章、`rt_*` ABI 手册、实现规划 / reviews / proposals。
