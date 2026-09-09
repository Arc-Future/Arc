# 什么是 Arc

Arc 是**纯 AOT 编译**的系统级编程语言：

- **表面语法**对标 C# 惯用法（前导类型、`namespace` / `using`、Query、`async`/`Task` 等），强调**单一惯用法**——同一意图一条正道。
- **核心抽象**（类型检查、LINQ / `Queryable`、`Expression<T>`、编译期元编程）在**编译期与链接期**完成，不是运行时 AST 解释。
- **运行时**执行已物化的原生机器码与静态数据；无 STW GC 作为默认内存模型叙事。
- **工具链**入口是单一 CLI：`arc`（体验对齐 .NET CLI：`build` / `run` / `test` / `check` 等）。

源码扩展名 **`.as`**。公开仓：[Arc-Future/Arc](https://github.com/Arc-Future/Arc)（**不是** Cratis/Arc 等同名项目）。

## 当前版本口径（0.1）

Arc **0.1** 是 pre-1.0 公开切割：单一 `arc` CLI、源码分发标准库、runtime C 源码（首次构建内容寻址缓存）、AOT 到原生机器码、无 JIT。**不宣称**与 C# 完备对等。

## 一句话定位

> 给人与智能体共同阅读、由编译器严格验证、最终变成可预测原生程序的系统语言。

## 下一步

- 判断是否适合你的场景：[适用场景与边界](who-should-use.md)
- 直接上手：[安装与快速开始](../02-quickstart/getting-started.md)
