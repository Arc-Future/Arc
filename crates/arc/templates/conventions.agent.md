# Coding 约定（.arcagent/conventions.md）

本文件经 ProjectConventionsProvider 注入模型上下文（Rules 层）。`arc new --agent` 生成。

## 项目契约

- 文档为唯一权威：实现与 RFC / 规范冲突时先对齐文档。
- 语言表面对标 C# 惯用法：前导类型、`namespace` / `using`、Query / `async` 等。
- 命名：类型 / 方法 / 属性 PascalCase；参数 / 局部变量 camelCase；私有字段 `_camelCase`；接口 `I` 前缀。
- 控制流一律 `{}` 括起（Allman 风格）。
- 异步方法必须 `Async` 后缀并接受 `CancellationToken`；禁止同步 I/O 副本。
- 可空类型显式 `?` 标注并妥善空判。
- 改动最小化、外科手术式；不顺手改相邻代码。

## 验证纪律

- 编译：`arc build`（D0）；测试：`arc test`（D3）；语义：`arc inspect`（D1）。
- 未接线的门保持 Pending，不假绿。
