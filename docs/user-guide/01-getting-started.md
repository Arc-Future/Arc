# 01 安装与快速开始

本章介绍如何安装 Arc 编译器、验证环境，并运行你的第一个 Arc 程序。

## 环境要求

| 项 | 要求 |
|----|------|
| 操作系统 | Windows、Linux、macOS |
| 源码构建 | Rust 工具链（`cargo`） |
| 二进制分发 | 单一 `arc` 可执行文件（命名随平台） |
| 构建依赖 | clang / LLVM——Windows 安装包捆绑；外部工具链支持基线 ≥ 22（`arc doctor` 门禁；低于基线可构建多数程序但不受支持） |

Arc 是纯 AOT 编译器：编译期直接生成原生机器码，无需 JIT 运行时。

## 安装 Arc 编译器

### 方式一：二进制安装包（Windows，推荐）

从发布渠道下载 `arc-<版本>-x86_64-pc-windows-msvc.zip`（附 `.sha256` 校验文件）。安装包自带：`arc.exe`、标准库源码（`lib/std`）、runtime C 源码（`lib/rt`）、捆绑的瘦身版 LLVM（`lib/llvm`——clang + lld 子集，**完全离线构建**）。

**脚本安装**（推荐）：安装包内已嵌入就地安装器 `install.ps1`。解压 zip 后，在解压出的 SDK 根目录直接运行：

```powershell
.\install.ps1
```

脚本自动把当前 SDK 落位至 `%LOCALAPPDATA%\arc\versions\`、写入版本指针与 `bin` 启动器、注入用户级 PATH，并运行 `arc doctor` 自检。也可用仓库版脚本从 zip 安装（自动校验 SHA256）：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -Archive arc-1.0.0-x86_64-pc-windows-msvc.zip -Sha256 <64-hex>
```

**手动安装**：解压 zip 到任意目录（下称 `<sdk-root>`），把 `<sdk-root>\bin` 加入 PATH 即可——SDK 经 `arc.exe` 自身位置自动定位（可用 `ARC_SDK_ROOT` 显式覆盖）。捆绑的 LLVM 由编译器**自动发现**（clang 解析序：`ARC_CLANG` → `arc toolchain` 安装位 → SDK 捆绑 `lib/llvm` → 系统安装位 → PATH），解压即得完全离线构建能力，无需设置任何环境变量；仅当改用外部 clang 时才需设 `ARC_CLANG`（参考包内 `arc.env` 模板）。

安装后运行 `arc doctor` 检查环境完整性（SDK 结构、clang 基线、MSVC 链接探测、缓存可写性等九项）。

### 方式二：二进制安装包（Linux/macOS，脚本安装）

> **状态（1.0）**：本方式为**消费端先行**——安装脚本与就地安装布局已就绪并经实机验收（`scripts/packaging/verify-arc-install.sh`，WSL2 Ubuntu 端到端 10/10），但 **tar.xz 打包产线与正式发布端点（`static.arc.dev` 占位）尚未交付**（[CHANGELOG](../../CHANGELOG.md) 1.0 已知限制）。产线就绪前可对任意已解压 SDK 目录使用 `--from-dir` 就地安装，或按[方式三：源码构建](#方式三源码构建)自行构建。

下载 `arc-<版本>-<triple>.tar.xz`（附 `.sha256` 校验文件），脚本安装（下载 → SHA256 校验 → 解压至 `~/.arc/versions/` → 版本指针与 PATH 注入）：

```bash
sh arc-install.sh --url https://…/arc-1.0.0-x86_64-unknown-linux-gnu.tar.xz
```

安装包内嵌的同名脚本也支持**就地安装**：解压 tar.xz 后，在解压出的 SDK 根目录直接 `sh install.sh`（或 `sh arc-install.sh --from-dir <sdk-dir>` 安装任意已解压 SDK 目录——目录可改名，版本取 `version.txt`）。

| 选项 | 含义 |
|------|------|
| `--url <url>` | 远程分发包地址（HTTPS，tar.xz） |
| `--from-dir <dir>` | 已解压 SDK 目录（就地安装；与 `--url` 互斥） |
| `--sha256 <hex>` | 显式校验值（缺省取 `<url>.sha256` 清单） |
| `--ca <cert>` | 自定义 CA 证书（自托管镜像 / 企业内网） |
| `--to <dir>` | 安装根（缺省 `$ARC_HOME` 或 `~/.arc`） |
| `--no-modify-path` | 不修改 shell 启动文件 |
| `--force` | 已安装时强制重装 |

安装布局与 Windows 一致（`versions/current` 指针 + `bin/arc` 启动器，与 `arc self-update` 对齐）；脚本实机验收 harness 见仓库 `scripts/packaging/verify-arc-install.sh`（以 stub `bin/arc` 验证安装协议，真实二进制链路随发布产线整体验收）。clang 解析序与 Windows 相同（`ARC_CLANG` → `arc toolchain` 安装位 → SDK 捆绑 `lib/llvm` → 系统安装位 → PATH）；Unix 侧捆绑 LLVM 随安装包产线交付，产线就绪前请使用外部 clang（≥ 22 支持基线）或 `arc toolchain install llvm`。

### 方式三：源码构建

使用 [.NET CLI](https://learn.microsoft.com/en-us/dotnet/core/tools/) 观感的命令行工具 `arc`，其命令与参数全面对齐 .NET CLI。

```bash
# 进入仓库根目录
cd /path/to/arc

# 构建 release 版本
cargo build --release
```

开发态产物为单一 `arc` 可执行文件，位于 `target/release/` 下；标准库与 runtime 直接取自仓库树（`SDK_LAYOUT=repo`）。

## 验证安装

```bash
arc --version
```

输出对应编译器版本号。若使用 Cargo 运行：

```bash
cargo run -p arc -- --version
```

## 快速开始

### 1. 创建项目目录

```bash
mkdir hello
cd hello
```

### 2. 编写 Hello World

创建 `Program.as`，内容如下：

```as
using Arc;

void Main() {
    Console.WriteLine("Hello, Arc!");
}
```

- 入口为 `void Main()`（或 `async Task<void> main()`）
- 源码采用 UTF-8 编码
- 标准库通过 `using Arc;` 引入，`Console.WriteLine` 输出到标准输出

### 3. 构建

```bash
arc build Program.as
```

产物默认输出到 `bin/Debug/` 目录。

### 4. 运行

```bash
arc run Program.as
```

终端输出：

```
Hello, Arc!
```

## 一步到位：check → build

推荐遵循「先检查、后构建」的协作工作流：

```bash
arc check Program.as   # 类型检查 + 借用检查，不生成代码
arc build Program.as   # 完整编译并链接
```

`arc check` 提供带源码位置的结构化诊断，便于快速修补；确认无误后再 `arc build` 集成。

## 下一步

- [02 构建与运行](02-build-run.md) 了解 `arc build` / `arc run` 的完整用法与项目目录约定
- [14 结构化诊断](14-structured-diagnostics.md) 了解如何阅读与消费编译诊断
- [16 编译器 CLI](16-compiler-cli.md) 查看 `arc` 全部子命令与参数

---

上一节：[返回目录](index.md) · 下一节：[02 构建与运行](02-build-run.md)