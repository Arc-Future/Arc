# SDK 布局与资源自定位

> 本子项定义 Arc SDK（分发包）的**目录布局契约**与**资源运行期自定位规则**，是 [017 编译产物、包体系与类型身份(../../017-build-artifacts-packages.md) 的能力子项。环境变量清单与语义见 [031 §10 环境变量清单](../../031-compiler-cli.md)，本子项只记录布局本身与定位规则。

## 1. 背景

Arc 编译器在构建期消费三类「SDK 资源」：runtime C 源码、vendored 原生库（`crypto_native` / `wgpu_native`）、内置 `.ani` 契约与捆绑 `std/`。历史实现以编译期 `env!("CARGO_MANIFEST_DIR")` 固化绝对路径，产物（`arc.exe`）离开构建机目录即失效——**不可重定位**。

本契约以**运行期自定位**取代编译期固化：`arc.exe` 经 `current_exe()` 定位自身所在目录并向上查找 SDK 标记目录（Go 式 `GOROOT` 模式），使 SDK 分发包可整体复制到任意位置仍可 `arc build`。

## 2. 目录布局契约（双布局）

两种 SDK 布局**等价识别**，统一经 [SDK 根判定](#22-sdk-根判定) 识别：

```text
安装态（分发物）                      仓库态（开发，仓库自身即 SDK）
<root>/bin/arc(.exe)                 <root>/std/
<root>/lib/std/                      <root>/crates/runtime/
<root>/lib/rt/                       <root>/crates/runtime-ui/
  └── runtime/、runtime-ui/、          <root>/crates/runtime-drawing/
      runtime-drawing/、              <root>/crates/runtime-sqlite/
      runtime-sqlite/、              <root>/crates/runtime-crypto/
      runtime-crypto/                <root>/crates/arc/native/
<root>/lib/native/
```

### 2.1 资源落点映射

| 资源 | 安装态 | 仓库态 |
|------|--------|--------|
| 捆绑 std 根 | `<sdk>/lib/std` | `<repo>/std` |
| runtime C 源码基目录（`runtime/`、`runtime-ui/`、`runtime-drawing/`、`runtime-sqlite/`、`runtime-crypto/`） | `<sdk>/lib/rt` | `<repo>/crates` |
| 内置 native 契约（`.ani`） | `<sdk>/lib/native` | `<repo>/crates/arc/native` |
| vendored `wgpu-native` | `<sdk>/lib/rt/runtime-ui/wgpu-native` | `<repo>/crates/runtime-ui/wgpu-native` |
| vendored `runtime-crypto` | `<sdk>/lib/rt/runtime-crypto` | `<repo>/crates/runtime-crypto` |
| runtime 共享单副本（`arc_runtime`，[017 跨库符号共享策略(../../017-build-artifacts-packages.md) 产物） | `<sdk>/lib/rt/arc-runtime/` | 不预置——构建期产出落 `target/`、缓存入 rt_cache（§4） |

runtime 共享 dll（`arc_runtime.<dll|so|dylib>`，含链接期导入库）按 [017 跨库符号共享策略(../../017-build-artifacts-packages.md) 全局单副本：安装态随 SDK 预置于 `<sdk>/lib/rt/arc-runtime/`（**SDK 域**，版本随 SDK 绑定、随 §8 self-update 多版本共存），安装态缺失时以 rt_cache 构建产物现场补位（单一解析序：SDK 预置 → 现场构建）；仓库态不预置，构建产物与 §4 rt_cache 同键空间（target + config + g|nog + sanitize）。项目 `bin/` 经硬链接引用单副本（与 U3 vendored dll 同模式）。与 U3 vendored dll **用户域**单副本缓存 `$ARC_HOME/cache`（`native_cache_dir()`）的层级关系：同为单副本共享形态，SDK 域按 SDK 版本唯一，用户域按内容全局唯一。

### 2.2 SDK 根判定

| 布局 | 标记（全部命中） |
|------|------------------|
| 安装态 | `<root>/bin/arc(.exe)` 为文件（可执行名随平台：Windows `arc.exe` / Unix `arc`，`sdk_layout::installed_arc_exe_name` 单一来源），且 `<root>/lib/rt` 或 `<root>/lib/std` 为目录 |
| 仓库态 | `<root>/std` 与 `<root>/crates/runtime` 均为目录 |

两态互斥判定：优先安装态；仓库态为兜底。显式环境变量覆盖（见 §3.1）即使目录缺标记也原样接受，由下游消费方报出明确错误。

## 3. 资源定位规则

### 3.1 SDK 根定位链（高 → 低）

```text
ARC_SDK_ROOT 环境变量  →  current_exe() 逐级向上找标记目录  →  CARGO_MANIFEST_DIR 开发兜底
```

| 次序 | 来源 | 语义 |
|------|------|------|
| 1 | `ARC_SDK_ROOT` 环境变量 | 显式指定 SDK 根；**最高优先级**，即使目录缺标记也原样返回（错误留给下游） |
| 2 | `current_exe()` 自定位 | 从 `arc.exe` 所在目录逐级向上查找标记目录（§2.2），命中即止 |
| 3 | 编译期 `CARGO_MANIFEST_DIR` 开发兜底 | 仅当自定位失败时生效（源码树内开发场景），保证既有构建不回归 |

### 3.2 资源根派生

| 函数 | 安装态 | 仓库态 | 自定位失败回退 |
|------|--------|--------|----------------|
| `sdk_runtime_base()` | `<sdk>/lib/rt` | `<root>/crates` | `CARGO_MANIFEST_DIR` 上溯一级（`crates/`） |
| `sdk_std_root()` | `<sdk>/lib/std` | `<root>/std` | 编译器源码树 `<repo>/std`；均不存在返回 `None` |
| `sdk_native_dir()` | `<sdk>/lib/native` | `<root>/crates/arc/native` | `CARGO_MANIFEST_DIR` 上溯一级 + `arc/native` |

### 3.3 std 解析链

统一经 `resolve_effective_std_root` 解析（产品消费方 build/lock/publish/core_arc 全部走此函数）：

```text
[std].path（项目显式覆盖）  →  SDK 捆绑 std  →  ARC_STD_ROOT 环境变量  →  workspace/std 兜底
```

| 次序 | 来源 | 语义 |
|------|------|------|
| 1 | `[std].path`（`arc.toml`） | 项目显式覆盖，相对 `arc.toml` 所在目录解析并 canonicalize（复用 `resolve_std_root` 纯函数） |
| 2 | SDK 捆绑 std | 安装态 `<sdk>/lib/std`；仓库态 `<repo>/std`（`sdk_std_root()`） |
| 3 | `ARC_STD_ROOT` 环境变量 | 显式指定 std 库根（开发调试用），canonicalize |
| 4 | `workspace/std` 兜底 | 当前工作目录向上定位 workspace 根内的 `std/` |

纯函数 `resolve_std_root` 保留给无 SDK/环境依赖的调用（单元测试、纯 `[std].path` 覆盖路径）。

## 4. rt_cache 用户缓存定位

runtime C 源码编译所得 `.o` 按内容缓存至**用户级缓存**，与 SDK 目录解耦（对标 Go `GOCACHE`）：

```text
$ARC_HOME/rt_cache/<target>_<config>_<g|nog><sanitize>/<obj>
```

| 项 | 规则 |
|----|------|
| 根 | `ARC_HOME` 环境变量（未设则 `HOME`/`USERPROFILE` 下 `.arc/`） |
| 子目录键 | `{target}_{release\|debug}_{g\|nog}`（debug 信息 + sanitize 后缀按需追加，纳入缓存键防产物互串） |
| 定位函数 | `codegen::sdk_layout::runtime_cache_dir()` |

缓存是用户数据，**不随 SDK 目录移动**；卸载 SDK 不影响既有缓存。该缓存与依赖包缓存（`$ARC_HOME/cache`，见 [031 §6](../../031-compiler-cli.md)）同属 `$ARC_HOME` 用户域。

## 5. 向后兼容与冻结面

- **仓库内开发零配置**：仓库态布局下 `<repo>` 自身即 SDK，`current_exe()` 自定位或 `CARGO_MANIFEST_DIR` 兜底均命中，既有无环境变量工作流不回归。
- **冻结面说明**：本契约**只改变编译器对 SDK 资源的运行期定位方式**，**不触碰** `rt_*` ABI、语言语义、`std/Arc` Stable 面（[036 成熟度(../../036-maturity.md) §3 基础面冻结）；`resolve_std_root` 纯函数签名保留，产品消费方统一走 `resolve_effective_std_root` 完整链。
- 环境变量语义变更（如优先级、行为）须随 [031 §10](../../031-compiler-cli.md) 同步。

## 6. 可重定位判别

以**目录复制冷构建**判别可重定位：

1. `scripts/sdk-stage.ps1 -OutDir <dir>` 将 `arc(.exe)` + 资源排布为安装态布局（`bin/` + `lib/{std,rt,native}`）；
2. 将 `<dir>` 复制到任意目录（脱离仓库源码树）；
3. 隐藏/移走仓库 `crates/runtime` 后于异地对普通项目 `arc build`，构建成功（证明资源来自 SDK 目录而非编译期固化路径）。

该判别与既有仓库内开发（零配置）双轨并存。

## 7. 打包、安装与工具命令

可重定位目录布局直接服务打包与安装（脚本归 `scripts/packaging/`，产物落 `target/` 下）：

| 构件 | 落点 | 职责 |
|------|------|------|
| 打包 | `scripts/packaging/arc-pack.ps1` | release 构建 → 打安装态目录（`bin/` + `lib/{std,rt,native}`）→ 容器随宿主（Windows `Compress-Archive` 产出 `arc-<ver>-<triple>.zip`；Unix `tar -cJf` 产出 `.tar.xz`，归档前恢复 `bin/arc`/`lib/llvm/bin`/`install.sh` 可执行位）+ `.sha256` → 自动判别验收（解包异地冷构建 + 运行） |
| 安装（Windows） | `scripts/packaging/install.ps1` | HTTPS 下载 zip → SHA256 校验 → 解压到 `%LOCALAPPDATA%\arc\versions\<pkg>` → 用户级 PATH（`-NoModifyPath` 可跳过）→ `arc doctor` |
| 安装（Unix 骨架） | `scripts/packaging/arc-install.sh` | 同契约（tar.xz，`~/.arc/versions/<pkg>`，`--no-modify-path`） |
| 工具命令 | `arc env` / `arc doctor` | 诊断与 CI 自检（见 [031 §11](../../031-compiler-cli.md)） |

zip 内布局与 §2 安装态完全一致，另附包元数据：

```text
arc-<ver>-<triple>/
├── bin/arc(.exe)
├── lib/{std,rt,native}      ← §2 资源落点
├── version.txt              ← arc=<ver> / triple=<triple> / commit=<sha> / layout=installed
└── arc.env                  ← 环境变量说明模板（自定位无需任何变量，仅显式覆盖参考）
```

**判别**：解包容器（Windows zip / Unix tar.xz）到任意目录 → `arc env` 输出 `SDK_LAYOUT=installed` 且 `ARC_SDK_ROOT` 指向解包目录 → `arc build` 离线示例成功（std 取自包内 `lib/std`，runtime C 取自包内 `lib/rt`）→ `arc doctor` 全绿。安装脚本 URL 为占位（完整发布端点另行交付）。

## 8. 签名发布与自更新布局

在 §7 安装布局之上叠加「多版本共存 + 指针切换 + 按需工具链」。命令面与协议见 [031 §12](../../031-compiler-cli.md)；本 § 只记录目录布局。

### 8.1 self-update 布局

```text
<root>/                       ← Windows %LOCALAPPDATA%\arc / Unix ~/.arc（ARC_INSTALL_ROOT 可覆盖）
├── bin/arc(.exe)           ← 稳定 PATH 指针 = 活动版本的 bin/arc(.exe) 副本（唯一 PATH 注入点）
└── versions/
    ├── current               ← 活动版本标记（内容 = 版本号，如 `0.2.0`）
    ├── current.previous      ← 上一版本（`arc self-update --rollback` 目标）
    └── arc-<ver>-<triple>/   ← §2 安装态完整 SDK（多版本共存）
```

要点：
- 版本目录即 §2 安装态 SDK（可重定位），`current_exe()` 自定位不变；`bin/arc(.exe)` 指针以副本身份 re-exec 活动版本，切换只改指针与标记，PATH 永不变。
- 原子性：staging（`versions/.staging-<pid>/`）解压 + 校验后 rename 提交；指针/标记临时名 → rename。
- 回滚：`--rollback` 按 `current.previous` 回切指针（版本目录保留）。

### 8.2 toolchain 布局（`arc toolchain install llvm`）

```text
<tools_root>/                  ← $ARC_HOME/tools（未设则 ~/.arc/tools）
└── llvm/
    ├── current                ← 活动版本指针（内容 = 版本号，如 `22.1.8`）
    └── <ver>/bin/clang[.exe]  ← 版本目录（多版本共存）
```

- clang 解析序（`codegen::clang_path`）：`ARC_CLANG` → `<tools>/llvm/current` 指针 → 标准 LLVM 安装位 → PATH；安装后 `arc build`/`arc env`/`arc doctor` 自动使用（单一解析序，避免双轨）。
- 工具根解析函数：`codegen::sdk_layout::{toolchain_tools_root, toolchain_llvm_dir, toolchain_llvm_clang_path}`。

### 8.3 发布端点协议

发布根固定 `manifest.json`（版本 × target 的 url/sha256/size + channel + `clang_min_version`）与分离签名 `manifest.json.sig`（Ed25519 覆盖原始字节）。生成/校验/密钥工具：`arc release manifest|verify|keygen`。详见 [031 §12.1](../../031-compiler-cli.md)。

## 9. 组件、捆绑 LLVM 与平台矩阵

在 §8 布局之上叠加「按需组件」与「捆绑 LLVM」两类可选交付。命令面与协议见 [031 §13](../../031-compiler-cli.md)；本 § 只记录目录布局。

### 9.1 组件布局（`arc component`）

```text
<tools_root>/components/            ← $ARC_HOME/tools/components（未设则 ~/.arc/tools/components）
└── <name>/
    ├── current                     ← 活动版本指针（内容 = 版本号，如 `v29.0.1.1`）
    └── <ver>/                      ← 版本目录（多版本共存）
        └── bin/<os>/wgpu_native.dll|.lib   ← 归一化平台二进制（wgpu 组件）
        └── include/                ← 归档携带的头文件（wgpu 组件：webgpu.h/wgpu.h）
```

- 组件二进制子目录（`bin/<os>/`）与 vendored 布局（`<rt-base>/runtime-ui/wgpu-native`）**一致**，codegen 经 `component_active_dir("wgpu")` 优先解析组件二进制、vendored 为兜底（单一解析序）。
- 安装流程：下载 → SHA256 校验 → staging 解压 → 归一化 `bin/<os>/` → 原子 rename → 写 `current` 指针（与 toolchain/self-update 同一原子性模式）。
- 定位函数：`codegen::sdk_layout::{components_root, component_dir, component_active_dir}`。
- 组件清单 `components.json` **内嵌于编译器二进制**（`include_str!`），一组件一条目：`builtin`（随 SDK 捆绑）/ 可下载组件（`url` 模板 + `sha256` + `size` + `platforms`）。

### 9.2 捆绑 LLVM 布局（`arc-pack.ps1 -BundleLlm`）

```text
<sdk>/lib/llvm/                     ← 安装态 SDK 内（仓库态无此目录）
└── bin/clang[.exe] / lld-link[.exe] / lld / ld.lld / ld64.lld
    └── llvm-rc / llvm-ar / llvm-ranlib
```

- **瘦身版原则**：仅交付 `arc build` 链路必需工具（clang 驱动 + lld 链接器族 + 少量辅助），不包含 clangd/lldb/Flang/OpenMP 等扩展工具；官方 Windows 发行中 clang.exe 为自包含单文件（无运行时 DLL 依赖），与 lld 同目录即可完整编译+链接。
- 打包时从 `ARC_CLANG` → 标准 LLVM 安装位 → PATH 定位 clang；`arc.env` 模板标注 `ARC_CLANG=<sdk>/lib/llvm/bin/clang[.exe]`；`version.txt` 记录 `llvm=bundled`。
- 判别验收：解包后把 `ARC_CLANG` 指向捆绑 clang → `arc env` 反映该值 → `arc doctor` clang 检测 PASS。

### 9.3 平台矩阵（安装形态）

| 平台 | 主渠道 | 备渠道 |
|------|--------|--------|
| Windows x64（主） | `install.ps1`（zip + SHA256 + 用户级 PATH） | winget/scoop（远期） |
| Windows ARM64 | 同一 zip（`aarch64-pc-windows-msvc`） | — |
| Linux x64/ARM64 | `arc-install.sh`（tar.xz + SHA256 + `~/.profile`/`~/.zshrc`） | deb/rpm（远期） |
| macOS | 同一 `arc-install.sh` / `.pkg`（远期） | Homebrew（远期） |
| OHOS / wasm | 不涉及安装（编译期交叉目标） | — |

### 9.4 能力边界

| 项 | 边界 |
|----|------|
| `arc-install.sh` 在 macOS/Linux 实机执行 | Windows 主机仅 `sh -n` 语法检查 + 静态审查 |
| macOS `.pkg` 实际构建 | 无 macOS 构建机 |
| Linux deb/rpm 发行包 | 需分发端点支持 |
| ARM64 平台包 | 待真实端点 + 工具链 |
| 真实发布端点（`static.arc.dev`） | `DEFAULT_RELEASE_BASE` 为占位 |

---

[返回 017 索引](index.md) · [返回 017 主题入口(../../017-build-artifacts-packages.md) · [环境变量清单 → 031 §10](../../031-compiler-cli.md)
