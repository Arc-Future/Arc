# Arc

[English](README.en.md) | 简体中文

Arc 是一门**纯 AOT 编译的系统级编程语言**，面向人机协作时代。它以 **C# 惯用表面语法**为基准，融合 Rust 风格的内存安全语义，经 AOT 编译为原生机器码，拒绝全局停顿式垃圾回收；核心抽象（类型、LINQ、`Expression<T>`、编译期元编程）在**编译期与链接期**展开，而非运行时解释。

## 定位

四元设计方程（[RFC 001 语言宪章](docs/rfc/001-language-charter.md)）：

```
Arc = 可读性 × 编译期安全 × AOT 确定性 × 人机协作性
```

四个因子**相乘**而非相加——任何一项为零，语言价值归零。

## 五条设计信条

| 信条         | 含义                  | 核心机制                       |
| ---------- | ------------------- | -------------------------- |
| ① 可读即可协作   | 程序首先是写给人与智能体共同阅读的文本 | 前导类型、单一惯用法、确定性格式、声明式查询     |
| ② 安全在编译期完成 | 运行时崩溃是设计的失败，不是程序的常态 | 资源所有权、借用约束、穷尽匹配、显式错误链      |
| ③ 行为在编译期确定 | 可预测性是系统软件的底线        | 纯 AOT、无 STW GC、零成本抽象、确定性输出 |
| ④ 代码即数据    | 程序结构是可分析、可变换、可传递的数据 | 表达式树、Provider 模式、双路径查询     |
| ⑤ 为人机共写而生  | 智能体是一等协作伙伴          | 结构化诊断、局部推理、能力显式、契约可声明      |

详见[语言宪章](docs/rfc/001-language-charter.md)与[语言宣言](docs/manifesto.md)。

## 当前状态

**Arc 1.0**（2026-09-04）——语言、编译器、标准库与运行时的首个稳定版：单一 `arc` 可执行文件 + 源码分发的标准库 + 随包 runtime C 源码（首次构建经内容寻址缓存按需编译），AOT 编译至原生机器码，无 JIT 运行时。当前仍处于活跃演进期，不宣称 C# 完备对等；版本历史见 [CHANGELOG](CHANGELOG.md)，成熟度治理见[成熟度宪章](docs/rfc/036-maturity.md)。

- **里程碑**：F0–M3 ✅（资产不回滚）；M4 可排期未开工；M5–Mn 逐层自举（HIR / typeck / codegen）推进中。

- **自举**：编译器当前为 **Rust bootstrap 实现**（`crates/*`）；默认 CLI 保持 Rust 编译器，直至 Arc 自举编译器等价（Mn）。

- **宣称纪律**：基础面（语言核心 / `rt_*` ABI / `std/Arc` Stable）默认冻结，破坏性变更须先 RFC；未经验收协议不得宣称。

## Quick Start

### 环境要求

| 项    | 要求                                            |
| ---- | --------------------------------------------- |
| Rust | Rust 工具链（`cargo`，stable）                      |
| LLVM | LLVM 22+（`clang` ≥ 22.0.0；`arc doctor` 强制该基线） |

### 构建编译器

```bash
cargo build --release
cargo test --workspace
```

### 使用

```bash
# 查看版本
cargo run -p arc -- --version

# 环境自检（对标 rustup doctor）
cargo run -p arc -- doctor

# 类型检查（含借用检查，不生成代码）
cargo run -p arc -- check examples/CompilerSmoke/Program.as

# 编译为原生二进制（宿主目标）
cargo run -p arc -- build examples/CompilerSmoke/Program.as -o hello.exe

# 编译并运行
cargo run -p arc -- run examples/CompilerSmoke/Program.as
```

Hello World（`Program.as`）：

```as
using Arc;

void Main() {
    Console.WriteLine("Hello, Arc!");
}
```

详见[安装与快速开始](docs/user-guide/01-getting-started.md)与[构建与运行](docs/user-guide/02-build-run.md)。

## Project Layout

```
arc/
├── crates/
│   ├── arc/             # CLI 驱动（env / doctor / toolchain / component / release / self-update / publish / new / detect / inspect）
│   ├── parse/           # 语法解析
│   ├── ast/             # 抽象语法树
│   ├── hir/             # 高级 IR
│   ├── typeck/          # 类型检查
│   ├── mir/             # 中级 IR
│   ├── codegen/         # LLVM 22 原生代码生成
│   ├── reachability/    # L2 入口可达性分析
│   ├── arc-server/      # LSP 服务化
│   ├── arcgr/           # .arcgr 语义索引
│   ├── arc-ui/          # 声明式 UI（.arml 解析 / typeck / inspect）
│   ├── arc-ssr/         # SSR 模板编译
│   ├── arc-tests/       # 分层测试（L1 快测 / L2 full-rt 门控）
│   ├── runtime/         # 运行时 ABI（纯 C 资源）
│   ├── runtime-crypto/  # vendored mbedTLS
│   ├── runtime-drawing/ # vendored qrcodegen / quirc / stb
│   ├── runtime-iree/    # vendored IREE
│   ├── runtime-onnx/    # vendored ONNX Runtime
│   ├── runtime-quic/    # vendored ngtcp2 + OpenSSL（QUIC/TLS）
│   ├── runtime-sqlite/  # vendored SQLite
│   └── runtime-ui/      # UI 运行时 ABI + wgpu-native 渲染后端
├── std/                 # 标准库（.as）
├── docs/                # Arc 语言之书（white-paper / user-guide / domain / rfc）
└── examples/            # 示例解决方案（CompilerSmoke / ArmlDemo / ArcAgent / ReviewAgent 等）
```

## Documentation

- [Arc 语言之书 · 目录](docs/SUMMARY.md)

- [前言](docs/preface.md)

- [语言宣言](docs/manifesto.md)

- [白皮书](docs/white-paper/index.md)

- [用户手册](docs/user-guide/index.md)

- [领域库](docs/domain/index.md)

- [RFC 设计决策](docs/rfc/index.md)

- [CHANGELOG](CHANGELOG.md)

## 安装 / SDK（可发布候选）

Arc 提供 .NET CLI 观感的命令行工具与 SDK 能力，当前为**可发布候选**（非正式发布）：

- `arc env` — 环境变量与 SDK 布局

- `arc doctor` — 环境自检（clang/LLVM 22 基线）

- `arc toolchain` — 工具链安装 / 管理

- `arc component` — 组件安装 / 管理

- `arc release` — 签名发布清单（生成 / 校验 / 密钥生成）

- `arc self-update` — 自更新分发

- `arc publish` — 打包发布（`.aopkg` 源码分发包）

- `arc new` / `arc detect` — 脚手架与项目识别

## 作者

- **作者**：LUSIDA（Start）—— Arc 语言创建者

- **邮箱**：<474309146@qq.com>

- **网站**：[www.lusida.net](https://www.lusida.net)

- **版权**：Copyright (c) 2026 LUSIDA (Start)。保留所有权利。

## License

本项目采用 [MIT 许可证](LICENSE)。

> 仓库内 `crates/runtime-*/` 下 vendored 的第三方代码（SQLite、mbedTLS、qrcodegen / quirc / stb、IREE、ONNX Runtime、ngtcp2 / OpenSSL、wgpu-native）保留各自独立许可证，以其 `NOTICE` / `VENDOR.md` 归因文件为准，不适用本仓库的 MIT 主许可证。

