# Arc — AOT 系统级编程语言

[English](README.md) | [简体中文](README.zh.md)

**Arc** 是一门**纯 AOT 编译的系统级编程语言**，面向人机协作时代。它以 **C# 惯用表面语法**为基准，融合 Rust 风格的内存安全语义，经 AOT 编译为原生机器码，**拒绝全局停顿式垃圾回收**；核心抽象（类型、LINQ、`Expression<T>`、编译期元编程）在**编译期与链接期**展开，而非运行时解释。

> **同名消歧**：本仓库是 **Arc 语言**（`Arc-Future/Arc`：编译器 / 标准库 / 运行时），**不是** [.NET 应用框架 Cratis/Arc](https://github.com/Cratis/Arc)，也与其他同名 “Arc” 项目无关。

## 为何选择 Arc

| | Arc | 典型 C# | 典型 Rust |
|---|-----|---------|-----------|
| 表面语法 | C# 惯用（前导类型、LINQ、`async`） | C# | Rust |
| 执行模型 | 纯 AOT → 原生 | JIT / 可选 NativeAOT | AOT（LLVM） |
| 内存 | 所有权 + 借用；无 STW GC | GC（STW） | 所有权 + 借用 |
| 查询 / 表达式树 | 编译期 LINQ 与 `Expression<T>` | 常偏运行时 / JIT 形态 | 宏 / 手写 |
| 智能体协作 | 结构化诊断、显式能力 | 工具链不一 | 工具链不一 |

四元设计方程（[RFC 001 语言宪章](docs/rfc/001-language-charter.md)）：

```
Arc = 可读性 × 编译期安全 × AOT 确定性 × 人机协作性
```

四个因子**相乘**而非相加——任何一项为零，语言价值归零。详见[语言宪章](docs/rfc/001-language-charter.md)与[语言宣言](docs/arc/01-introduction/manifesto.md)。

## 当前状态

**Arc 0.1**（预正式公开线）：单一 `arc` 可执行文件 + 源码分发的标准库 + 随包 runtime C 源码（首次构建经内容寻址缓存按需编译），AOT 至原生机器码，无 JIT。不宣称 C# 完备对等。

- 编译器当前为 **Rust bootstrap**（`crates/*`）；默认 CLI 保持 Rust，直至 Arc 自举等价。
- 基础面（语言核心 / `rt_*` ABI / `std/Arc` Stable）默认冻结；破坏性变更须先 RFC。
- 版本历史见 [CHANGELOG](CHANGELOG.md)；成熟度治理见 [RFC 036](docs/rfc/036-maturity.md)。

## 快速开始

### 环境要求

| 项 | 要求 |
| ---- | --- |
| Rust | Rust 工具链（`cargo`，stable） |
| LLVM | LLVM 22+（`clang` ≥ 22.0.0；`arc doctor` 强制该基线） |

### 构建编译器

```bash
cargo build --release
cargo test --workspace
```

### 使用

```bash
cargo run -p arc -- --version
cargo run -p arc -- doctor
cargo run -p arc -- check examples/CompilerSmoke/Program.as
cargo run -p arc -- build examples/CompilerSmoke/Program.as -o hello.exe
cargo run -p arc -- run examples/CompilerSmoke/Program.as
```

Hello World（`Program.as`）：

```as
using Arc;

void Main() {
    Console.WriteLine("Hello, Arc!");
}
```

详见[安装与快速开始](docs/arc/02-quickstart/getting-started.md)与[构建与运行](docs/arc/02-quickstart/build-run.md)。

## 安装 / SDK（0.1）

二进制包见 [GitHub Releases](https://github.com/Arc-Future/Arc/releases)。公网 `static.arc.dev` 仍为占位；自更新可将 `ARC_RELEASE_BASE` 设为 Release 下载根。

常用命令：`arc env`、`arc doctor`、`arc toolchain`、`arc component`、`arc release`、`arc self-update`、`arc publish`、`arc new`、`arc detect`。

## 文档

| 文档 | 用途 |
|------|------|
| [llms.txt](llms.txt) | 面向 AI 智能体 / 爬虫的权威知识地图 |
| [Arc 开发者手册](docs/arc/INDEX.md) | **官方可发布文档**（Docbit slug `arc`） |
| [仓库文档导航](docs/SUMMARY.md) | 手册 + RFC 索引 |
| [语言宣言](docs/arc/01-introduction/manifesto.md) | 四元方程与信条 |
| [RFC](docs/rfc/index.md) | 设计决策权威（默认不发布） |
| [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) | 贡献与漏洞报告 |

## 仓库布局

```
crates/     # 编译器（Rust）+ 运行时 ABI（C）+ vendored 原生依赖
std/        # 标准库（.as）
docs/       # arc/ 开发者手册（可发布）+ rfc/ 设计权威
examples/   # 示例（CompilerSmoke、ArmlDemo、ArcAgent 等）
```

## 作者

- **作者**：LUSIDA（Start）—— Arc 语言创建者  
- **邮箱**：<474309146@qq.com>  
- **网站**：[www.lusida.net](https://www.lusida.net)  
- **版权**：Copyright (c) 2026 LUSIDA (Start)。保留所有权利。

## License

本项目采用 [MIT 许可证](LICENSE)。`crates/runtime-*/` 下 vendored 第三方代码保留各自许可证（见 `NOTICE` / `VENDOR.md`）。
