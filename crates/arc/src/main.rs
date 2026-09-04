use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

use arc::manifest::ArcManifest;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "arc", version = VERSION, about = "Arc AOT compiler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show compiler version
    Version,
    /// Print the resolved SDK environment (对标 `go env`; Phase 1 toolchain).
    ///
    /// 输出当前 SDK 根、std/rt/native 路径、rt_cache 与 std 解析链胜出来源
    ///（`[std].path` / SDK 捆绑 / `ARC_STD_ROOT` / workspace），供诊断与 CI 消费。
    Env {
        /// Output as JSON (对标 `go env -json`; 机器可消费)
        #[arg(long)]
        json: bool,
    },
    /// Run SDK/toolchain self-checks (对标 `rustup doctor`; Phase 1 toolchain).
    ///
    /// 检测 SDK 完整性、clang/LLVM 可用性、`ARC_STD_ROOT` 一致性、rt_cache 可写、
    /// native DLL（crypto_native/wgpu_native）、MSVC（Windows msvc 宿主）与环境变量；
    /// 任一 FAIL 时退出码非零（CI 门禁）。
    Doctor {
        /// Output as JSON (机器可消费)
        #[arg(long)]
        json: bool,
    },
    /// 按需安装/管理外部工具链（Phase 2：首个组件 = LLVM/clang）。
    ///
    /// `arc toolchain install llvm --archive <zip>|--url <url>` 安装到
    /// `<tools_root>/llvm/<ver>`（`$ARC_HOME/tools` 或 `~/.arc/tools`）并写
    /// `llvm/current` 指针；`codegen::clang_path` 自动接线该 clang（`ARC_CLANG`
    /// 之后、标准安装位之前）。`list` / `uninstall` / `status` 管理状态机。
    Toolchain {
        #[command(subcommand)]
        command: ToolchainCommand,
    },
    /// 按需组件安装/管理（Phase 3：首个可下载组件 = wgpu）。
    ///
    /// `arc component install wgpu` 下载 → SHA256 校验 → 解包 → 归一化
    /// `bin/<os>/` 布局 → 原子落位 `<tools_root>/components/wgpu/<ver>` 并写
    /// `current` 指针；codegen wgpu 二进制目录解析自动接线（组件优先、
    /// vendored 兜底）。`list`（builtin/installed/not-installed）/ `uninstall` /
    /// `status` 管理状态机。清单 `components.json` 内嵌于编译器二进制。
    Component {
        #[command(subcommand)]
        command: ComponentCommand,
    },
    /// 签名发布自更新（RFC 031 §13）。
    ///
    /// 下载签名 manifest → SHA256 + Ed25519 校验 → staging 解压 → `--version` 自检 →
    /// 原子提交 `bin/` 指针与 `versions/` 标记。默认目标 = manifest 中高于当前的
    /// 最高版本；`--check` 仅检查；`--rollback` 回退上一版本。
    SelfUpdate {
        /// 目标版本（缺省取 manifest 中高于当前的最新版本）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// 发布源：`https://…`、`file:///…` 或本地目录（覆盖 `$ARC_RELEASE_BASE`）。
        #[arg(long, value_name = "SOURCE")]
        source: Option<String>,
        /// 安装根覆盖（CI / 测试；缺省自动定位）。
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// 仅检查更新（下载并验签 manifest，不安装）。
        #[arg(long)]
        check: bool,
        /// 回滚到 `versions/current.previous` 记录的上一版本。
        #[arg(long)]
        rollback: bool,
        /// 允许重复安装 / 显式目标等于当前版本。
        #[arg(long)]
        force: bool,
    },
    /// 发布端点工具：签名发布 manifest 生成 / 校验 / 密钥生成（RFC 031 §13）。
    ///
    /// 发布协议：`manifest.json`（版本 × target 的 url/sha256/size + 通道 + clang 基线）
    /// 与分离签名 `manifest.json.sig`（Ed25519 覆盖 manifest 原始字节；信任锚 = 编译期
    /// 内置公钥或 `$ARC_RELEASE_PUBKEY`）。`arc self-update` / 安装脚本消费同一协议。
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    /// 打包项目为 `.aopkg` 源码分发包（RFC 017 源码打包 / RFC 031 §13）。
    ///
    /// zip 容器（`<name>-<version>/`）：arc.toml + 源码（.as/.arml）+ native/ 契约 +
    /// `FILES.json`（逐文件 SHA256 清单）；签名密钥存在时产出分离签名
    /// `<pkg>.aopkg.sig`。`--verify <PKG>` 为消费端校验（清单完整性 + 签名）。
    Publish {
        /// 项目路径——项目目录或 `arc.toml` 路径；缺省当前目录。
        #[arg(value_name = "PROJECT")]
        file: Option<PathBuf>,
        /// 与 `PROJECT` 位置参数等价（对标 `dotnet publish --project <PATH>`）。
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// 输出目录（缺省 `<project>/dist/`）。
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output: Option<PathBuf>,
        /// 签名 seed（64 hex；缺省 `$ARC_RELEASE_SIGNING_KEY`）。
        #[arg(long, value_name = "SEED")]
        key: Option<String>,
        /// 校验模式：校验指定 `.aopkg`（清单完整性 + 可选分离签名）而非打包。
        #[arg(long, value_name = "PKG")]
        verify: Option<PathBuf>,
        /// 显式分离签名路径（仅 `--verify` 模式；缺省取 `<pkg>.aopkg.sig`）。
        #[arg(long, value_name = "SIG", requires = "verify")]
        sig: Option<PathBuf>,
    },
    /// Parse source and print AST
    Parse {
        /// Source file (.as)
        file: PathBuf,
    },
    /// Type-check and borrow-check source
    Check {
        /// Source file (.as)
        file: PathBuf,
    },
    /// Compile source to native binary
    Build {
        /// 项目路径——`.as` 文件、项目目录或 `arc.toml` 路径。
        /// 对标 `dotnet build <PROJECT>`。
        ///
        /// 项目模式：当路径指向目录或 `arc.toml` 且 manifest 含 `[ui]` 节时，
        /// 触发 ARML codegen + 编译流程（对标 `dotnet build`）：
        ///   1. 对每个 `arml` 生成 `.g.as` 到 `obj/<config>/<ClassName>.g.as`
        ///   2. 合并 `.g.as` + `sources` + `program` 为 `obj/<config>/Program.as`
        ///   3. 编译 `Program.as` 为 `bin/<config>/<package.name>.exe`
        ///
        /// 单文件模式：当路径指向 `.as` 文件时，直接编译该文件（向后兼容）。
        ///
        /// 缺省时使用当前目录。
        #[arg(value_name = "PROJECT")]
        file: Option<PathBuf>,
        /// 与 `PROJECT` 位置参数等价（对标 `dotnet build --project <PATH>`）。
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// Output directory for binaries (default: `bin/<config>/`).
        /// 对标 `dotnet build -o <OUTPUT_DIR>`。
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output: Option<PathBuf>,
        /// Build configuration: `Debug` (default) or `Release`.
        /// 对标 `dotnet build -c Release`。
        #[arg(short = 'c', long, default_value = "Debug", value_name = "CONFIG")]
        configuration: String,
        /// Target runtime identifier (default: host triple).
        /// 对标 `dotnet build -r <RUNTIME>`。
        #[arg(short = 'r', long, visible_alias = "target", value_name = "RUNTIME")]
        runtime: Option<String>,
        /// Verbosity level: `quiet`, `minimal`, `normal` (default), `detailed`, `diagnostic`.
        /// 对标 `dotnet build --verbosity <LEVEL>`。
        #[arg(long, value_name = "LEVEL", default_value = "normal")]
        verbosity: String,
        /// Emit DWARF 5 debug info (RFC 031 §2 / RFC 015 Phase B.2)
        #[arg(short = 'g', long)]
        debug: bool,
        /// Native library search path (RFC 016 M2); repeatable, also reads [native].ani-native-lib
        #[arg(long, value_name = "DIR")]
        ani_native_lib: Vec<PathBuf>,
        /// RFC 017 D8 v1.0：编译为动态库（`kind = "library"` + `dynamic = true`）。
        ///
        /// 覆盖 manifest `[package].dynamic` 字段。产物为 `.dll`/`.so`/`.dylib`，
        /// 通过 `rt_library_load` 加载、`rt_library_sym` 查找领域约定符号。
        /// 仅 `kind = "library"` 时生效；`binary` 项目传入直接报错（意图错位
        /// 不静默忽略，防止「以为产了动态库实际产了 .exe」的隐蔽误用）。
        #[arg(long)]
        dynamic: bool,
        /// Intermediate products directory (overrides default `obj/<config>/`).
        ///
        /// 用于隔离并发构建的中间产物（`out.ll`/`out.o`/`rt_*.o`），避免
        /// 多个 `arc build` 进程踩踏共享工作目录。默认 `obj/Debug/` 或 `obj/Release/`。
        #[arg(long, value_name = "DIR")]
        obj_dir: Option<PathBuf>,
        /// RFC 017 产物域（U3，UX 迭代评审 §2.3）：保留 LLVM 文本 IR（`out.ll`）。
        ///
        /// 默认 clang 编译成功后即焚毁 `.ll`（用户目录中间产物膨胀的单项最大源，
        /// examples 实测 114 MB）；本开关显式保留供 IR 诊断与审阅。
        /// clang 失败路径恒保留现场（排障），不受本开关影响。
        #[arg(long)]
        emit_llvm: bool,
        /// Print known target triples and build support status (RFC 037 M-W1a).
        #[arg(long)]
        list_targets: bool,
        /// RFC 037 M-W3 (Draft): emit `.ll` + `.wasm` for wasm targets only.
        /// Does **not** lift M-W1a gate for normal builds; no browser/DOM/wgpu-web.
        #[arg(long)]
        experimental_wasm_emit: bool,
        /// RFC 005 里程碑④：编译期字段环 warning 策略——`warn`（默认，打印
        /// `arc-cycle-001` 不阻断编译）| `off`（静默）。覆盖 arc.toml
        /// `[compiler] field_cycle_policy`。**无 `error` 档**。
        #[arg(long, value_name = "POLICY")]
        field_cycle_policy: Option<String>,
        /// RFC 036 §2 增量构建门禁：构建后输出 `--incremental-report`——
        /// 报告哪些 `.as` 被重编/复用、消费了哪些 `.aopkg`、及耗时。
        #[arg(long)]
        incremental_report: bool,
        /// 并行构建 workspace 成员（默认串行、确定性优先）。仅当互不依赖的
        /// 成员可并发时受益；依赖序始终保留（被依赖者先完成）。
        #[arg(long)]
        parallel: bool,
        /// 并行 worker 数（仅配合 `--parallel` 生效；默认 = 逻辑核心数）。
        #[arg(short = 'j', long, value_name = "N")]
        jobs: Option<usize>,
    },
    /// Parse, check, build, and run
    Run {
        /// 项目路径——`.as` 文件或项目目录。
        /// 对标 `dotnet run <PROJECT>`。
        #[arg(value_name = "PROJECT")]
        file: PathBuf,
        /// 与 `PROJECT` 位置参数等价（对标 `dotnet run --project <PATH>`）。
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// Build configuration: `Debug` (default) or `Release`.
        /// 对标 `dotnet run -c Release`。
        #[arg(short = 'c', long, default_value = "Debug", value_name = "CONFIG")]
        configuration: String,
        /// Target runtime identifier (default: host triple).
        /// 对标 `dotnet run -r <RUNTIME>`。
        #[arg(short = 'r', long, visible_alias = "target", value_name = "RUNTIME")]
        runtime: Option<String>,
        /// Run without building first.
        /// 对标 `dotnet run --no-build`。
        #[arg(long)]
        no_build: bool,
        /// Verbosity level: `quiet`, `minimal`, `normal` (default), `detailed`, `diagnostic`.
        /// 对标 `dotnet run --verbosity <LEVEL>`。
        #[arg(long, value_name = "LEVEL", default_value = "normal")]
        verbosity: String,
        /// Panic output format: human (default) or json (RFC 031 §3)
        #[arg(long, value_name = "FMT")]
        panic_format: Option<String>,
        /// Emit DWARF 5 debug info (RFC 031 §2 / RFC 015 Phase B.2)
        #[arg(short = 'g', long)]
        debug: bool,
        /// Native library search path (RFC 016 M2); repeatable, also reads [native].ani-native-lib
        #[arg(long, value_name = "DIR")]
        ani_native_lib: Vec<PathBuf>,
        /// RFC 005 里程碑④：编译期字段环 warning 策略——`warn`（默认，打印
        /// `arc-cycle-001` 不阻断编译）| `off`（静默）。覆盖 arc.toml
        /// `[compiler] field_cycle_policy`。**无 `error` 档**。
        #[arg(long, value_name = "POLICY")]
        field_cycle_policy: Option<String>,
    },
    /// Emit semantic index (`.arcgr`) for AI tooling / LSP / debugger (RFC 034 M2).
    ///
    /// 运行 `parse → hir → typeck → collect_arcgr_file` 产出 `.arcgr` 语义索引，
    /// 或通过 `--aopkg` 直接从预编译包内嵌的 `.arcgr` 消费（RFC 017），
    /// 输出可达性分析摘要供 AI 工具链、LSP、调试器消费（RFC 034/RFC 038/RFC 039
    /// 共享数据底座）。
    Inspect {
        /// Source file (.as) — single-file MVP (RFC 034 M2 Step 4)
        file: PathBuf,
        /// Output format: `human` (default) or `json`
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
        /// Write `.arcgr` binary to the given path (RFC 034 M2 Step 5)
        ///
        /// 设置时同时输出二进制 `.arcgr` 文件，供跨工具链共享同一份语义索引。
        #[arg(long, value_name = "PATH")]
        emit: Option<PathBuf>,
    },
    /// Locate a symbol definition (RFC 034 M3).
    ///
    /// 输出定义位置 file:span + 签名。精确符号名匹配（不支持模糊匹配）。
    Locate {
        /// `.arcgr` 文件路径（由 `arc inspect --emit` 生成）
        arcgr: PathBuf,
        /// 符号名（精确匹配，如 `Main` 或 `Foo.Bar`）
        symbol: String,
        /// 输出格式：`human`（默认）或 `json`
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
    },
    /// Explain a symbol — L2 symbol card (RFC 034 M3).
    ///
    /// 输出签名 + doc_summary + 直接 callers/callees + 引用数 + 入口/可达性标记。
    /// token 预算 ~4K，供 AI 工具链消费。
    Explain {
        /// `.arcgr` 文件路径
        arcgr: PathBuf,
        /// 符号名
        symbol: String,
        /// 输出格式：`human`（默认）或 `json`
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
    },
    /// Query callers/callees/impls/references (RFC 034 M3).
    ///
    /// 意图查询——精确符号名 + 4 种关系类型，全部支持 `--format json`。
    Query {
        /// 查询意图：`callers`|`callees`|`impls`|`references`
        kind: String,
        /// `.arcgr` 文件路径
        arcgr: PathBuf,
        /// 符号名
        symbol: String,
        /// 输出格式：`human`（默认）或 `json`
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
    },
    /// AI 首触入口——输出项目骨架 L0/L1（RFC 034 M4）。
    ///
    /// 通过 `arc.toml` + `.arcgr` 输出 ContextManifest，让 AI 无需读源码即知项目结构。
    /// 默认输出 L0 项目概览（~500 tok）；`--detail` 输出 L0+L1 完整模块面（~2K tok）。
    Overview {
        /// 入口源文件（用于定位 `arc.toml` 与编译流程起点）
        file: PathBuf,
        /// 输出 L0 + L1 完整模块面（默认仅 L0）
        #[arg(long)]
        detail: bool,
        /// 输出格式：`human`（默认）或 `json`
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
    },
    /// 生成最小可编译项目骨架（RFC 043 场景 1.2 B 面：空目录 → `arc new` → D0 build 绿）。
    ///
    /// 产物：`arc.toml`（name / namespace + `Arc` 依赖）+ `Program.as`（`void Main()`
    /// 最小入口）+ 可选 `README.md`。`--agent` 追加 `Arc.Agent` + `Arc.Agent.Harness`
    /// 依赖（对齐 Coding Harness 三包消费）并落 `.arcagent/conventions.md` 初始模板
    /// （默认 Skills 面）。对标 `dotnet new console`。
    New {
        /// 生成目录（不存在则创建；已含 arc.toml 时报错）。
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// 包名（缺省取目录名；namespace 由 `-`/`_`/`.` 分段首字母大写派生）。
        #[arg(long, value_name = "PKG")]
        name: Option<String>,
        /// 追加 Agent 依赖（Arc.Agent + Arc.Agent.Harness）+ 落 conventions 模板。
        #[arg(long)]
        agent: bool,
        /// 跳过 README.md 生成。
        #[arg(long)]
        no_readme: bool,
    },
    /// 项目类型识别（RFC 043 场景 1.2 DetectProject）。
    ///
    /// 判定：无 `arc.toml` → `uninitialized`；依赖含 `Arc.Agent.Harness.Coding` →
    /// `coding_harness`；含 `Arc.Agent.Harness` 无 Coding → `domain_two`；其余 →
    /// `arc_project`。输出 human（默认）或 `--format json`（机器可消费）。
    Detect {
        /// 项目目录（缺省当前目录）。
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
        /// 输出格式：human（默认）| json。
        #[arg(long, value_name = "FMT")]
        format: Option<String>,
    },
    /// Remove build artifacts (对标 `dotnet clean`).
    ///
    /// RFC 017 产物域（U3，UX 迭代评审 §2.3）：项目级删除 `obj/` 与 `bin/`；
    /// `--cache` 追加清除全局共享缓存 `$ARC_HOME/cache`（vendored native dll
    /// 单副本落点，下次构建自动重建）。只删产物，不触碰源码与 `arc.toml`；
    /// 目录不存在时幂等成功。
    Clean {
        /// 项目路径——目录、`.as` 文件或 `arc.toml`（缺省当前目录）。
        #[arg(value_name = "PROJECT")]
        file: Option<PathBuf>,
        /// 追加清除全局共享缓存 `$ARC_HOME/cache`（RFC 017 产物域）。
        #[arg(long)]
        cache: bool,
    },
    /// Declarative UI tools (RFC 037): inspect/verify `.arml` files.
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
    /// Compile test source and run QIF tests (RFC 032 Phase 2c pure Arc path).
    ///
    /// 流程：扫描 AST 收集 [Fact]/[Theory] → 生成合成 __QifTestHost.Main()
    /// → 编译为可执行文件 → 运行并转发退出码。
    Test {
        /// 测试项目路径——`.as` 文件或项目目录。
        /// 对标 `dotnet test <PROJECT>`。
        #[arg(value_name = "PROJECT")]
        file: PathBuf,
        /// 与 `PROJECT` 位置参数等价（对标 `dotnet test --project <PATH>`）。
        #[arg(long, value_name = "PATH")]
        project: Option<PathBuf>,
        /// Output directory for test binary (default: `bin/<config>/`).
        /// 对标 `dotnet test -o <OUTPUT_DIR>`。
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output: Option<PathBuf>,
        /// Build configuration: `Debug` (default) or `Release`.
        /// 对标 `dotnet test -c Release`。
        #[arg(short = 'c', long, default_value = "Debug", value_name = "CONFIG")]
        configuration: String,
        /// Target runtime identifier (default: host triple).
        /// 对标 `dotnet test -r <RUNTIME>`。
        #[arg(short = 'r', long, visible_alias = "target", value_name = "RUNTIME")]
        runtime: Option<String>,
        /// Skip build — run tests with existing test binary.
        /// 对标 `dotnet test --no-build`。
        #[arg(long)]
        no_build: bool,
        /// Verbosity level: `quiet`, `minimal`, `normal` (default), `detailed`, `diagnostic`.
        /// 对标 `dotnet test --verbosity <LEVEL>`。
        #[arg(long, value_name = "LEVEL", default_value = "normal")]
        verbosity: String,
        /// Emit DWARF 5 debug info
        #[arg(short = 'g', long)]
        debug: bool,
        /// Native library search path (RFC 016 M2); repeatable
        #[arg(long, value_name = "DIR")]
        ani_native_lib: Vec<PathBuf>,
        /// Intermediate products directory (overrides default `obj/`).
        ///
        /// 用于隔离并发 `arc test` 的中间产物（`out.ll`/`out.o`/`rt_*.o`）。
        /// 默认 `obj/`。
        #[arg(long, value_name = "DIR")]
        obj_dir: Option<PathBuf>,
        /// 过滤测试（全限定名 contains 匹配，如 "CalculatorTests::Add"）。
        /// 对标 `dotnet test --filter <EXPRESSION>`。
        /// QIF-6：支持 XUnit 风格 `Field~Value` 表达式与 `|`/`&`/`!` 组合。
        #[arg(long, value_name = "EXPRESSION")]
        filter: Option<String>,
        /// 按命名空间前缀选择测试（如 `Arc.Collections`）。与 `--filter` 叠加为 AND。
        #[arg(long, value_name = "NAMESPACE")]
        namespace: Option<String>,
        /// 按测试 Kind 选择（`Fact`/`Theory`/`Integration`/`E2e`/`Benchmark`/`Property`/`Snapshot`/`Contract`）。
        /// 与 `--filter` 叠加为 AND。
        #[arg(long, value_name = "KIND")]
        kind: Option<String>,
        /// 列出所有测试（不执行）。
        /// 对标 `dotnet test --list-tests`。
        #[arg(long)]
        list_tests: bool,
        /// 列出测试的输出格式：text（默认）| json（便于 CI 消费）。
        #[arg(long, value_name = "FORMAT")]
        list_format: Option<String>,
        /// 输出格式：human（默认）| json | junit。
        /// 覆盖 arc.toml [qif].output_format。
        /// 对标 `dotnet test --logger <LOGGER>`。
        #[arg(long, value_name = "LOGGER")]
        logger: Option<String>,
        /// 并行执行测试（XUnit 默认行为）。
        /// 对标 `xunit.runner.json parallelizeTestCollections`。
        /// 实验性：需 Arc codegen 支持 lambda delegate → 函数指针转换。
        #[arg(long)]
        parallel: bool,
        /// 并行执行的最大并发度（1..N；0 = 不限）。覆盖 arc.toml [qif].max_parallel。
        /// 仅配合 `--parallel` 生效；`--parallel` 未给度数时取 [qif].max_parallel（缺省不限）。
        #[arg(long, value_name = "N")]
        max_parallel: Option<i32>,
        /// 默认单测试超时毫秒（0 = 不限制）。覆盖 arc.toml [qif].default_timeout。
        #[arg(long, value_name = "MS")]
        timeout: Option<i32>,
        /// RFC 005 里程碑④：编译期字段环 warning 策略——`warn`（默认，打印
        /// `arc-cycle-001` 不阻断编译）| `off`（静默）。覆盖 arc.toml
        /// `[compiler] field_cycle_policy`。**无 `error` 档**。
        #[arg(long, value_name = "POLICY")]
        field_cycle_policy: Option<String>,
    },
}

#[derive(Subcommand)]
enum UiCommand {
    /// Output JSON structure tree + ASCII layout preview (D11).
    Inspect {
        /// `.arml` source file
        file: PathBuf,
    },
    /// Type-check + A11y + layout + adaptive verification report (D11 / RFC 016 M-U1).
    Verify {
        /// `.arml` source file
        file: PathBuf,
        /// 将 RFC 016 §11.3 标记 `strict = error` 的自适应 warning（区间未全覆盖 /
        /// 档位阈值漂移 / 死分支 / 同权重歧义）升级为 error。
        #[arg(long)]
        strict: bool,
    },
    /// Generate Arc code from `.arml` (RFC 037 M2 ARML code-behind, WPF-style).
    ///
    /// Parses one or more `.arml` files, generates `partial class` per file
    /// (written to `obj/<config>/<ClassName>.g.as`), and merges all generated
    /// code + user `.arml.as` partial classes into a single `Program.as`
    /// compilation unit.
    ///
    /// 支持根元素：
    /// - `<Window x:Class="Ns.MainWindow">` —— 主窗口声明
    /// - `<Application x:Class="Ns.App" StartupUri="MainWindow.arml">` —— 应用入口
    Codegen {
        /// `.arml` source files (one or more; e.g. `App.arml MainWindow.arml`)
        files: Vec<PathBuf>,
        /// Output `Program.as` file path (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Generated namespace (default: `Arc.UI.Generated`)
        #[arg(long, value_name = "NS")]
        namespace: Option<String>,
        /// 用户 partial class 源文件（可重复；如 `App.arml.as`、`MainWindow.arml.as`）
        #[arg(long, value_name = "PATH")]
        user_source: Vec<PathBuf>,
        /// 程序入口文件（如 `Program.as`），含 `Main()` 函数。
        /// 所有 Arc 项目统一此标准——合并到生成 `Program.as` 末尾。
        #[arg(long, value_name = "PATH")]
        program: Option<PathBuf>,
        /// 构建配置（`Debug` 或 `Release`，默认 `Debug`），影响 `.g.as`
        /// 输出子目录与可执行文件输出子目录。
        #[arg(long, value_name = "CONFIG")]
        config: Option<String>,
    },
}

/// `arc release` 子命令：发布端点协议工具（RFC 031 §13）。
#[derive(Subcommand)]
enum ReleaseCommand {
    /// 生成签名发布清单 `manifest.json` + `manifest.json.sig`。
    ///
    /// 从本地分发包计算 SHA256/size，构建版本清单并 Ed25519 签名（`--key <seed>`
    /// 或 `$ARC_RELEASE_SIGNING_KEY`）。URL 缺省按 `--url-prefix` + 包文件名派生
    /// 或逐条 `--url`。
    Manifest {
        /// 版本号（缺省当前 `CARGO_PKG_VERSION`）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// target triple（可重复；缺省逐条取宿主 triple）。
        #[arg(long, value_name = "TRIPLE")]
        triple: Vec<String>,
        /// 分发包本地路径（可重复；与 `--triple` 配对，计算 sha256/size）。
        #[arg(long, value_name = "ARCHIVE")]
        archive: Vec<PathBuf>,
        /// 分发包 URL（可重复；缺省按 `--url-prefix` + 包文件名派生）。
        #[arg(long, value_name = "URL")]
        url: Vec<String>,
        /// 发布根 URL 前缀（派生 artifact URL）。
        #[arg(long, value_name = "PREFIX")]
        url_prefix: Option<String>,
        /// 输出目录（写 `manifest.json` 与 `manifest.json.sig`；缺省当前目录）。
        #[arg(short, long, value_name = "OUTDIR")]
        output: Option<PathBuf>,
        /// 签名 seed（64 hex；缺省 `$ARC_RELEASE_SIGNING_KEY`；两者皆无则报错）。
        #[arg(long, value_name = "SEED")]
        key: Option<String>,
    },
    /// 校验发布清单：解析 → 验签（Ed25519，信任锚内置/`$ARC_RELEASE_PUBKEY`）→
    /// 可选比对分发包 SHA256（`--version` + `--triple`，远端下载或 `--archive` 本地）。
    Verify {
        /// 发布源：`https://…`、`file:///…` 或本地目录。
        #[arg(value_name = "SOURCE")]
        source: String,
        /// 校验指定版本的分发包（缺省仅验证 manifest 本体）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// 分发包 target triple（缺省宿主 triple）。
        #[arg(long, value_name = "TRIPLE")]
        triple: Option<String>,
        /// 本地分发包路径（比对 sha256/size；缺省按 manifest 从发布源下载）。
        #[arg(long, value_name = "ARCHIVE")]
        archive: Option<PathBuf>,
    },
    /// 生成发布签名密钥对（输出 64-hex seed 与公钥）。
    ///
    /// seed 即 Ed25519 私钥（离线托管）；公钥内嵌 `release.rs::RELEASE_PUBLIC_KEY_HEX`
    /// 作为信任锚。`--seed <hex>` 可从给定 seed 派生公钥（dev/CI 复现用）。
    Keygen {
        /// 从给定 seed 派生公钥（缺省随机生成新密钥对）。
        #[arg(long, value_name = "SEED")]
        seed: Option<String>,
    },
}

/// `arc toolchain` 子命令。
#[derive(Subcommand)]
enum ToolchainCommand {
    /// 安装工具链组件（首个：`llvm`）。
    ///
    /// 来源：`--url <url>`（真实端点，Phase 2 占位）或 `--archive <zip>`（本地/离线）。
    /// 安装到 `<tools_root>/llvm/<ver>` 并写 `llvm/current` 指针；`codegen::clang_path`
    /// 自动接线（`ARC_CLANG` 之后、标准安装位之前）。`--set-env`（默认）写用户环境
    /// `ARC_CLANG`。`ARC_CLANG`/PATH 已有 clang 时提示「已就绪」并跳过（`--force` 覆盖）。
    Install {
        /// 组件名（当前支持 `llvm`）。
        #[arg(value_name = "COMPONENT")]
        component: String,
        /// 目标版本（缺省 `22.1.8`，与 `.aopkg` metadata 对齐）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// 下载 URL（覆盖占位模板；真实端点见 RFC 031 §12 外部依赖）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// 本地分发包（zip；离线/测试）。
        #[arg(long, value_name = "ARCHIVE")]
        archive: Option<PathBuf>,
        /// 可选分发包 SHA256（64 hex；提供则校验，不符拒绝安装）。
        #[arg(long, value_name = "HEX")]
        sha256: Option<String>,
        /// 跳过「clang 已可用 / 已安装」幂等捷径（强制重装）。
        #[arg(long)]
        force: bool,
        /// 不写用户环境 `ARC_CLANG`（仅打印指引）。
        #[arg(long)]
        no_set_env: bool,
    },
    /// 列出已装工具链组件与状态（active / 版本 / clang 基线）。
    List,
    /// 卸载工具链组件（`--version` 指定版本；缺省卸载当前活动版本）。
    Uninstall {
        /// 组件名（当前支持 `llvm`）。
        #[arg(value_name = "COMPONENT")]
        component: String,
        /// 指定版本（缺省当前活动版本）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
    },
    /// 工具根 / 活动版本 / clang 解析结果（与 doctor 同一解析序）。
    Status,
}

/// `arc component` 子命令（Phase 3：按需组件）。
#[derive(Subcommand)]
enum ComponentCommand {
    /// 安装可下载组件（首个：`wgpu`）。
    ///
    /// 来源：组件清单 URL 模板（默认）> `--url <url>` > `--archive <zip>`（离线/测试）。
    /// 下载 → SHA256 校验（`--sha256` > 清单固定值）→ 解包 → 归一化 `bin/<os>/`
    /// 布局 → 原子落位 `<tools_root>/components/<name>/<ver>` + `current` 指针。
    /// 装后 `arc build` / `arc doctor` 自动使用（codegen 单一解析序）。
    Install {
        /// 组件名（当前支持 `wgpu`）。
        #[arg(value_name = "NAME")]
        name: String,
        /// 目标版本（缺省组件清单固定版本）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
        /// 下载 URL（覆盖组件清单模板）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
        /// 本地分发包（zip；离线/测试）。
        #[arg(long, value_name = "ARCHIVE")]
        archive: Option<PathBuf>,
        /// 可选分发包 SHA256（64 hex；提供则校验，不符拒绝安装）。
        #[arg(long, value_name = "HEX")]
        sha256: Option<String>,
        /// 跳过「已安装」幂等捷径（强制重装）。
        #[arg(long)]
        force: bool,
    },
    /// 列出全部组件与状态（`builtin` / `installed (active)` / `not-installed`）。
    List,
    /// 卸载组件（`--version` 指定版本；缺省卸载当前活动版本）。
    Uninstall {
        /// 组件名（当前支持 `wgpu`）。
        #[arg(value_name = "NAME")]
        name: String,
        /// 指定版本（缺省当前活动版本）。
        #[arg(long, value_name = "VER")]
        version: Option<String>,
    },
    /// 组件根 / 各组件状态与活动路径。
    Status,
}

fn main() {
    // 编译器 typeck 的 `check_expr_inner_impl` 拥有大型 match 表达式，debug
    // 模式下栈帧较大（不做栈槽复用）。Windows 主线程默认 1MB 栈在深度嵌套
    // 表达式（如 8 层 `&&`）下会栈溢出（0xC00000FD）。使用 8MB 工作线程执行
    // 编译，与 rustc 自身的做法一致；release 模式优化后不需要但无副作用。
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let cli = Cli::parse();
            run(cli)
        })
        .expect("failed to spawn compiler thread");
    match handle.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("error: compiler thread panicked");
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Version => {
            println!("arc {VERSION}");
            Ok(())
        }
        Commands::Env { json } => {
            let snapshot = arc::env::snapshot();
            if json {
                println!("{}", arc::env::format_json(&snapshot)?);
            } else {
                print!("{}", arc::env::format_human(&snapshot));
            }
            Ok(())
        }
        Commands::Doctor { json } => {
            let code = arc::doctor::run_doctor(json)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Commands::Toolchain { command } => run_toolchain(command),
        Commands::Component { command } => run_component(command),
        Commands::SelfUpdate {
            version,
            source,
            root,
            check,
            rollback,
            force,
        } => arc::self_update::run(&arc::self_update::SelfUpdateOptions {
            version,
            source,
            root,
            check,
            rollback,
            force,
        }),
        Commands::Release { command } => run_release(command),
        Commands::Publish {
            file,
            project,
            output,
            key,
            verify,
            sig,
        } => {
            if let Some(package) = verify {
                return arc::publish::run_verify(&arc::publish::VerifyOptions { package, sig });
            }
            arc::publish::run_publish(&arc::publish::PublishOptions {
                project: project.or(file),
                output,
                key,
            })
        }
        Commands::Parse { file } => {
            let unit = arc::load(&file).map_err(|e| format!("load error: {e}"))?;
            println!("{:#?}", unit.program);
            Ok(())
        }
        Commands::Check { file } => {
            let triple = resolve_target(None, false)?;
            // 解决方案 = workspace 聚合：入口为 workspace 根时，按依赖拓扑序逐一 check
            // 全部成员（对标 `dotnet sln` 一键全量校验）；入口命中成员项目时只
            // check 该成员及其 ProjectReference 闭包（对标 `dotnet build <csproj>`）。
            let equipments = arc::Equipments::default();
            if let Some(ws) = equipments.project.discover(&file)? {
                let target = equipments.project.locate_member(&ws, &file);
                let order = equipments.resolve.order(&ws, target)?;
                for &i in &order {
                    let member = &ws.members[i];
                    println!(
                        "checking workspace member '{}' ({})",
                        member.manifest.package.name,
                        member.root.display()
                    );
                    check_single_project(&member.root, &member.manifest, &triple)?;
                }
                return Ok(());
            }
            let manifest = arc::manifest::require_arc_manifest(&file)?;
            check_single_project(&file, &manifest.1, &triple)?;
            println!("check passed: {}", file.display());
            Ok(())
        }
        Commands::Build {
            file,
            project,
            output,
            configuration: ref config_str,
            runtime,
            debug,
            ani_native_lib,
            dynamic,
            obj_dir,
            emit_llvm,
            list_targets,
            experimental_wasm_emit,
            field_cycle_policy,
            incremental_report,
            parallel,
            jobs,
            ..
        } => {
            if list_targets {
                print!("{}", arc::target::format_target_list());
                return Ok(());
            }
            let triple = resolve_target(runtime.as_deref(), experimental_wasm_emit)?;
            let file = project.or(file).unwrap_or_else(|| PathBuf::from("."));
            let release = config_str == "Release";

            // 解决方案 = workspace 聚合：入口为 workspace 根（`arc.toml` 含
            // `[workspace] members`）时，按依赖拓扑顺序一键全量构建全部成员
            //（对标 `dotnet sln build`）。入口命中某个成员项目时只构建该成员
            // 及其 ProjectReference 闭包（对标 `dotnet build <csproj>`）。
            // 每个成员输出到各自 obj/bin（不共享 --output）。
            // 并行度策略：默认串行（确定性优先）；`--parallel` 用依赖感知并行
            //（`--jobs N` 限定并发，缺省 = 逻辑核心数）。并行不得破坏依赖序契约。
            let schedule: Box<dyn arc::CompileScheduler> = if parallel {
                let n = jobs.unwrap_or_else(|| {
                    std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1)
                });
                Box::new(arc::ParallelScheduler::with_jobs(n))
            } else {
                Box::new(arc::SerialScheduler)
            };
            let equipments = arc::Equipments {
                schedule,
                ..arc::Equipments::default()
            };
            if let Some(ws) = equipments.project.discover(&file)? {
                let target = equipments.project.locate_member(&ws, &file);
                let order = equipments.resolve.order(&ws, target)?;
                let dep_on = ws.direct_dependencies();
                equipments.schedule.run(&order, &dep_on, &|i| {
                    let member = &ws.members[i];
                    println!(
                        "building workspace member '{}' ({})",
                        member.manifest.package.name,
                        member.root.display()
                    );
                    build_single_project(
                        &member.root,
                        &(member.root.clone(), member.manifest.clone()),
                        None,
                        release,
                        debug,
                        &triple,
                        &ani_native_lib,
                        dynamic,
                        None,
                        field_cycle_policy.as_deref(),
                        incremental_report,
                        emit_llvm,
                    )
                })?;
                return Ok(());
            }

            let manifest = arc::manifest::require_arc_manifest(&file)?;
            build_single_project(
                &file,
                &manifest,
                output.as_deref(),
                release,
                debug,
                &triple,
                &ani_native_lib,
                dynamic,
                obj_dir.as_ref(),
                field_cycle_policy.as_deref(),
                incremental_report,
                emit_llvm,
            )
        }
        Commands::Run {
            file,
            project,
            configuration: ref config_str,
            runtime,
            no_build,
            verbosity: _verbosity,
            panic_format,
            debug,
            ani_native_lib,
            field_cycle_policy,
            ..
        } => {
            let file = project.unwrap_or(file);
            let manifest = arc::manifest::require_arc_manifest(&file)?;
            let config = config_str.as_str();
            let triple = resolve_target(runtime.as_deref(), false)?;
            let lib_paths = merge_native_lib_paths(&ani_native_lib, &manifest);
            let project_kind = project_kind_from_manifest(&manifest);
            let field_cycle_policy =
                resolve_field_cycle_policy(field_cycle_policy.as_deref(), &manifest.1)?;
            let compile_options = arc::CompileOptions {
                field_cycle_policy,
                ..Default::default()
            };

            // ARML UI 项目（[ui] 节）：产物为 bin/<config>/<package.name>.exe，
            // 走 ARML codegen + 编译（对标 `arc build` 的 UI 路径）。
            let is_ui = manifest.1.ui.is_some();
            let (_obj_dir, bin_dir) = resolve_project_dirs(&file, &manifest, None, config);
            let out = if is_ui {
                bin_dir.join(format!("{}{}", manifest.1.package.name, exe_suffix()))
            } else {
                bin_dir.join(default_binary_name(&file, &triple))
            };

            if !no_build {
                let release = config_str == "Release";
                if let Some(ui) = &manifest.1.ui {
                    build_ui_project(
                        &manifest.0,
                        &manifest.1,
                        ui,
                        Some(&out),
                        release,
                        debug,
                        &triple,
                        &lib_paths,
                        // RFC 017 产物域：`arc run` 无 `--emit-llvm` 旋钮，恒焚毁。
                        false,
                    )?;
                } else {
                    arc::compile_file_with_native(
                        &file,
                        release,
                        debug,
                        Some(&out),
                        None,
                        Some(&triple),
                        &lib_paths,
                        project_kind,
                        &compile_options,
                    )?;
                }
            } else if !out.exists() {
                return Err(format!(
                    "--no-build specified but binary not found: {}",
                    out.display()
                ));
            }
            let mut cmd = Command::new(&out);
            if let Some(fmt) = panic_format.as_deref() {
                cmd.env("ARC_PANIC_FORMAT", fmt);
            }
            let status = cmd.status().map_err(|e| format!("run failed: {e}"))?;
            if !status.success() {
                return Err(format!("program exited with {status}"));
            }
            Ok(())
        }
        Commands::Inspect { file, format, emit } => run_inspect(file, format, emit),
        Commands::Locate {
            arcgr,
            symbol,
            format,
        } => run_locate(arcgr, symbol, format),
        Commands::Explain {
            arcgr,
            symbol,
            format,
        } => run_explain(arcgr, symbol, format),
        Commands::Query {
            kind,
            arcgr,
            symbol,
            format,
        } => run_query(kind, arcgr, symbol, format),
        Commands::Overview {
            file,
            detail,
            format,
        } => run_overview(file, detail, format),
        Commands::New {
            dir,
            name,
            agent,
            no_readme,
        } => {
            let opts = arc::ScaffoldOptions {
                name,
                agent,
                readme: !no_readme,
            };
            let report = arc::scaffold_project(&dir, &opts)?;
            print!("{}", report.human_summary());
            Ok(())
        }
        Commands::Detect { dir, format } => {
            let root = dir.unwrap_or_else(|| PathBuf::from("."));
            let info = arc::detect_project(&root);
            match format.as_deref().unwrap_or("human") {
                "human" => println!("{}", info.human_summary()),
                "json" => println!(
                    "{}",
                    serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?
                ),
                other => return Err(format!("unknown --format `{other}` (expected: human|json)")),
            }
            Ok(())
        }
        Commands::Clean { file, cache } => {
            // RFC 017 产物域（U3，UX 迭代评审 §2.3）：项目级清 obj/ + bin/；
            // `--cache` 追加清全局共享缓存。宽容入参：指向 .as / arc.toml 时
            // 取其父目录为项目根（对齐 build 的路径解析习惯）。
            let root = file.unwrap_or_else(|| PathBuf::from("."));
            let project_root = if root.is_dir() {
                root
            } else if root.is_file() {
                root.parent().map(Path::to_path_buf).ok_or_else(|| {
                    format!("clean: cannot resolve project root of {}", root.display())
                })?
            } else {
                return Err(format!("clean: path not found: {}", root.display()));
            };
            clean_dir(&project_root.join("obj"))?;
            clean_dir(&project_root.join("bin"))?;
            if cache {
                clean_dir(&codegen::sdk_layout::native_cache_dir())?;
            }
            Ok(())
        }
        Commands::Ui { command } => run_ui(command),
        Commands::Test {
            file,
            project,
            output,
            configuration: ref config_str,
            runtime,
            no_build,
            debug,
            ani_native_lib,
            obj_dir,
            filter,
            namespace,
            kind,
            list_tests,
            list_format,
            logger,
            parallel,
            max_parallel,
            timeout,
            field_cycle_policy,
            ..
        } => {
            let file = project.unwrap_or(file);
            let release = config_str == "Release";
            // 解决方案 = workspace 聚合：入口为 workspace 根（`arc.toml` 含
            // `[workspace] members`）时，按依赖拓扑序对每个含测试的成员跑测试
            // 并汇总结果（对标 `dotnet sln test` 一键全量测试）。
            let equipments = arc::Equipments::default();
            if let Some(ws) = equipments.project.discover(&file)? {
                let target_member = equipments.project.locate_member(&ws, &file);
                return run_test_workspace(
                    &equipments,
                    ws,
                    target_member,
                    release,
                    runtime,
                    no_build,
                    debug,
                    ani_native_lib,
                    filter,
                    namespace,
                    kind,
                    list_tests,
                    list_format,
                    logger,
                    parallel,
                    max_parallel,
                    timeout,
                    field_cycle_policy,
                );
            }
            let manifest = arc::manifest::require_arc_manifest(&file)?;
            // 目标三元组校验（单项目测试经 compile_test_{file,project} 内部解析）；
            // 绑定 `_` 仅作 runtime 合法性校验，不参与本路径。
            let _triple = resolve_target(runtime.as_deref(), false)?;
            let field_cycle_policy =
                resolve_field_cycle_policy(field_cycle_policy.as_deref(), &manifest.1)?;
            let compile_options = arc::CompileOptions {
                field_cycle_policy,
                ..Default::default()
            };
            run_test(
                file,
                output,
                obj_dir,
                release,
                runtime,
                no_build,
                debug,
                ani_native_lib,
                filter,
                namespace,
                kind,
                list_tests,
                list_format,
                logger,
                parallel,
                max_parallel,
                timeout,
                &compile_options,
            )
        }
    }
}

/// `arc release`：发布端点工具（签名 manifest 生成 / 校验 / 密钥生成）。
fn run_release(command: ReleaseCommand) -> Result<(), String> {
    match command {
        ReleaseCommand::Keygen { seed } => arc::release::run_keygen(seed.as_deref()),
        ReleaseCommand::Manifest {
            version,
            triple,
            archive,
            url,
            url_prefix,
            output,
            key,
        } => arc::release::run_manifest(&arc::release::ManifestArgs {
            version,
            triples: triple,
            archives: archive,
            urls: url,
            url_prefix,
            output,
            key,
        }),
        ReleaseCommand::Verify {
            source,
            version,
            triple,
            archive,
        } => arc::release::run_verify(&arc::release::VerifyArgs {
            source,
            version,
            triple,
            archive,
        }),
    }
}

/// `arc toolchain`：外部工具链按需安装/管理（Phase 2）。
fn run_toolchain(command: ToolchainCommand) -> Result<(), String> {
    use arc::toolchain::ToolchainInstallOptions;
    match command {
        ToolchainCommand::Install {
            component,
            version,
            url,
            archive,
            sha256,
            force,
            no_set_env,
        } => {
            let opts = ToolchainInstallOptions {
                tool: component,
                version,
                url,
                archive,
                sha256,
                set_env: !no_set_env,
                force,
            };
            arc::toolchain::run_install(&opts)
        }
        ToolchainCommand::List => arc::toolchain::run_list(),
        ToolchainCommand::Uninstall { component, version } => {
            arc::toolchain::run_uninstall(&component, version.as_deref())
        }
        ToolchainCommand::Status => arc::toolchain::run_status(),
    }
}

/// `arc component`：按需组件安装/管理（Phase 3）。
fn run_component(command: ComponentCommand) -> Result<(), String> {
    use arc::components::ComponentInstallOptions;
    match command {
        ComponentCommand::Install {
            name,
            version,
            url,
            archive,
            sha256,
            force,
        } => {
            let opts = ComponentInstallOptions {
                name,
                version,
                url,
                archive,
                sha256,
                force,
            };
            arc::components::run_install(&opts)
        }
        ComponentCommand::List => arc::components::run_list(),
        ComponentCommand::Uninstall { name, version } => {
            arc::components::run_uninstall(&name, version.as_deref())
        }
        ComponentCommand::Status => arc::components::run_status(),
    }
}

/// `arc test`：RFC 032 Phase 2c 纯 Arc 测试链路。
///
/// 流程：
/// 1. 读取 arc.toml [qif] 节配置
/// 2. 合并 CLI 参数（--filter / --format 覆盖 manifest）
/// 3. 扫描源码 AST 收集 [Fact]/[Theory] 方法
/// 4. 生成合成 `__QifTestHost.Main()` 入口函数
/// 5. 编译为可执行文件
/// 6. 运行可执行文件，转发退出码
fn run_test(
    file: PathBuf,
    output: Option<PathBuf>,
    obj_dir: Option<PathBuf>,
    release: bool,
    runtime: Option<String>,
    no_build: bool,
    debug: bool,
    native_lib_paths: Vec<PathBuf>,
    filter: Option<String>,
    namespace: Option<String>,
    kind: Option<String>,
    list_tests: bool,
    list_format: Option<String>,
    logger: Option<String>,
    parallel: bool,
    max_parallel: Option<i32>,
    timeout: Option<i32>,
    compile_options: &arc::CompileOptions,
) -> Result<(), String> {
    let manifest = arc::manifest::require_arc_manifest(&file)?;
    let triple = resolve_target(runtime.as_deref(), false)?;

    // RFC 032 Phase 4: 项目级测试——对标 `dotnet test`。
    // 当 `file` 为目录（项目根）或非 `.as` 文件时，走项目级扫描。
    let is_project = file.is_dir() || file.extension().map(|e| e != "as").unwrap_or(true);

    // 1. 合并 QIF 配置：CLI 优先级 > manifest [qif] > 默认值
    let qif_section = manifest.1.qif.clone();
    let output_format = logger.unwrap_or(qif_section.output_format);
    let filter_pattern = filter.unwrap_or(qif_section.filter);
    let namespace_pattern = namespace.unwrap_or_default();
    let kind_pattern = kind.unwrap_or_default();
    let list_format_override = list_format.unwrap_or_default();

    // 并行度：`--parallel` 开启并行；有效度数 = CLI `--max-parallel` > manifest
    // `max_parallel`(>1) > 不限(-1)。未并行时恒为 1（串行）。
    let effective_max_parallel: i32 = if !parallel {
        1
    } else if let Some(n) = max_parallel {
        if n >= 1 {
            n
        } else {
            -1
        }
    } else if qif_section.max_parallel > 1 {
        qif_section.max_parallel
    } else {
        -1
    };
    let default_timeout_ms = timeout.unwrap_or(qif_section.default_timeout);

    // RFC 032 §7：QIF 产物根目录（`.arcqif` + `report.json`）。相对值以项目根为基准；
    // 持久化开启时先建目录（Arc host 的 `File.WriteAllText` 不负责建立父目录）。
    let artifact_dir = resolve_qif_artifact_dir(&manifest.0, &qif_section.output);
    if qif_section.emit_json_report || qif_section.persist_results {
        if let Err(e) = std::fs::create_dir_all(&artifact_dir) {
            return Err(format!(
                "failed to create QIF artifact dir {}: {e}",
                artifact_dir.display()
            ));
        }
    }

    let qif_opts = arc::QifCompileOptions {
        output_format,
        filter: filter_pattern,
        namespace: namespace_pattern,
        kind: kind_pattern,
        list_only: list_tests,
        list_format: list_format_override,
        parallel,
        max_parallel: effective_max_parallel,
        default_timeout_ms,
        output_dir: artifact_dir.display().to_string(),
        emit_json_report: qif_section.emit_json_report,
        persist_results: qif_section.persist_results,
    };

    // 2. 确定 obj_dir 和 output 路径（统一项目模型，对标 MSBuild）
    let config = if release { "Release" } else { "Debug" };
    let (obj_dir_resolved, bin_dir) =
        resolve_project_dirs(&file, &manifest, obj_dir.as_ref(), config);

    let project_root = manifest.0.clone();
    let stem = if is_project {
        &manifest.1.package.name
    } else {
        file.file_stem().and_then(|s| s.to_str()).unwrap_or("test")
    };
    let output_path = output
        .unwrap_or_else(|| bin_dir.join(format!("{}{}", test_binary_name(stem), exe_suffix())));

    // 3. 合并 native_lib_paths
    let lib_paths = merge_native_lib_paths(&native_lib_paths, &manifest);

    // 4. 编译测试（--no-build 跳过编译，直接使用现有二进制）
    if !no_build {
        // 增量判定（P0）：保守指纹比对，命中跳过编译（对齐 `arc build`）。
        // 指纹在 build 基础上额外覆盖：std 源码树（保守超集，即便 test 消费 `.aopkg`
        // 也纳入，宁多勿少绝不陈旧）+ QIF 选项（决定合成 `__QifTestHost.Main()` 产物）。
        // `--list` 不产生产物，恒走收集路径。
        if list_tests {
            compile_test_dispatch(
                &file,
                &project_root,
                is_project,
                release,
                debug,
                &output_path,
                &obj_dir_resolved,
                &triple,
                &lib_paths,
                &qif_opts,
                compile_options,
            )?;
        } else {
            let out_name = output_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("{}.test", stem));
            // 对齐 loader 的 workspace 推导：find_workspace_root 从 `start.parent()` 上溯、
            // 不检查 `start` 本身，故传项目根下必存在的 arc.toml，确保项目根目录被纳入。
            let workspace = arc::find_workspace_root(&project_root.join("arc.toml"));
            let std_root = arc::resolve_effective_std_root(
                &workspace,
                Some(&project_root),
                manifest.1.std.as_ref(),
            );
            let extra_inputs = vec![
                (
                    "qif.output_format".to_string(),
                    qif_opts.output_format.clone(),
                ),
                ("qif.filter".to_string(), qif_opts.filter.clone()),
                ("qif.namespace".to_string(), qif_opts.namespace.clone()),
                ("qif.kind".to_string(), qif_opts.kind.clone()),
                (
                    "qif.list_only".to_string(),
                    if qif_opts.list_only {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    },
                ),
                ("qif.list_format".to_string(), qif_opts.list_format.clone()),
                (
                    "qif.max_parallel".to_string(),
                    qif_opts.max_parallel.to_string(),
                ),
                (
                    "qif.default_timeout_ms".to_string(),
                    qif_opts.default_timeout_ms.to_string(),
                ),
                ("qif.parallel".to_string(), qif_opts.parallel.to_string()),
            ];
            let inputs = arc::FingerprintInputs {
                entry: &project_root,
                manifest: Some(&manifest.1),
                config,
                triple: triple.as_str(),
                out_name: &out_name,
                debug,
                extra_source_dirs: vec![std_root],
                extra_inputs,
            };
            let fp = arc::compute_fingerprint_inputs(&inputs);
            if arc::is_up_to_date_tagged(&obj_dir_resolved, Some("test"), &output_path, &fp) {
                println!(
                    "up-to-date: {} (target: {})",
                    output_path.display(),
                    triple.as_str()
                );
            } else {
                compile_test_dispatch(
                    &file,
                    &project_root,
                    is_project,
                    release,
                    debug,
                    &output_path,
                    &obj_dir_resolved,
                    &triple,
                    &lib_paths,
                    &qif_opts,
                    compile_options,
                )?;
                arc::record_build_tagged(&obj_dir_resolved, Some("test"), &fp);
            }
        }
    } else if !output_path.exists() {
        return Err(format!(
            "--no-build specified but test binary not found: {}",
            output_path.display()
        ));
    }

    if list_tests {
        return Ok(());
    }

    println!(
        "built test binary: {} (target: {})",
        output_path.display(),
        triple.as_str(),
    );

    // 5. 运行测试可执行文件
    let run_path = if output_path.is_absolute() {
        output_path.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("failed to get current dir: {e}"))?
            .join(&output_path)
    };
    let status = Command::new(&run_path)
        .status()
        .map_err(|e| format!("failed to run test binary: {e}"))?;
    if !status.success() {
        return Err(format!("test binary exited with {status}"));
    }
    println!("test run completed: {}", output_path.display());
    Ok(())
}

/// `arc test` workspace 聚合：按依赖拓扑序对每个含测试的成员跑测试并汇总结果。
///
/// 对标 `dotnet sln test`。非测试成员（无 `[Fact]`/`[Theory]`）经
/// [`arc::project_has_tests`] 判别后跳过；任一成员失败即整体非零退出。
fn run_test_workspace(
    equipments: &arc::Equipments,
    ws: arc::Workspace,
    target_member: Option<usize>,
    release: bool,
    runtime: Option<String>,
    no_build: bool,
    debug: bool,
    native_lib_paths: Vec<PathBuf>,
    filter: Option<String>,
    namespace: Option<String>,
    kind: Option<String>,
    list_tests: bool,
    list_format: Option<String>,
    logger: Option<String>,
    parallel: bool,
    max_parallel: Option<i32>,
    timeout: Option<i32>,
    field_cycle_policy: Option<String>,
) -> Result<(), String> {
    // 入口命中成员项目 → 只测该成员及其 ProjectReference 闭包中的测试成员
    //（对标 `dotnet test <csproj>`）；workspace 根 → 全量（`dotnet sln test`）。
    // 构建序经依赖解析装备（P2）产出——段序不依赖具体实现。
    let order = equipments.resolve.order(&ws, target_member)?;
    let mut ran = 0usize;
    let mut skipped = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for &i in &order {
        let member = &ws.members[i];
        if !arc::project_has_tests(&member.root)? {
            skipped += 1;
            continue;
        }
        println!(
            "testing workspace member '{}' ({})",
            member.manifest.package.name,
            member.root.display()
        );
        let field_cycle_policy =
            resolve_field_cycle_policy(field_cycle_policy.as_deref(), &member.manifest)?;
        let compile_options = arc::CompileOptions {
            field_cycle_policy,
            ..Default::default()
        };
        match run_test(
            member.root.clone(),
            None,
            None,
            release,
            runtime.clone(),
            no_build,
            debug,
            native_lib_paths.clone(),
            filter.clone(),
            namespace.clone(),
            kind.clone(),
            list_tests,
            list_format.clone(),
            logger.clone(),
            parallel,
            max_parallel,
            timeout,
            &compile_options,
        ) {
            Ok(()) => ran += 1,
            Err(e) => failures.push(format!("'{}': {e}", member.manifest.package.name)),
        }
    }

    if !failures.is_empty() {
        return Err(format!(
            "workspace test failed: {} of {} project(s) failed{}",
            failures.len(),
            ran + failures.len(),
            failures
                .iter()
                .map(|f| format!("\n  - {f}"))
                .collect::<String>(),
        ));
    }
    if ran == 0 {
        println!("no test projects found in workspace ({skipped} member(s) skipped)");
    } else {
        println!("workspace test passed: {ran} project(s), {skipped} skipped");
    }
    Ok(())
}

fn test_binary_name(stem: &str) -> String {
    format!("{}.test", stem)
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

/// 解析 QIF 产物根目录（RFC 032 §7 `[qif].output`）。相对值以项目根为基准，绝对话直接使用。
fn resolve_qif_artifact_dir(project_root: &Path, qif_output: &str) -> PathBuf {
    let p = PathBuf::from(qif_output);
    if p.is_absolute() {
        p
    } else {
        project_root.join(p)
    }
}

/// `arc test` 编译派发：项目模式（扫描全部 `.as` + QIF 收集）或单文件模式。
///
/// 供 `run_test` 的增量未命中 / `--list` 两条路径共用，避免复制 is_project 分支。
fn compile_test_dispatch(
    file: &Path,
    project_root: &Path,
    is_project: bool,
    release: bool,
    debug: bool,
    output_path: &PathBuf,
    obj_dir: &Path,
    triple: &arc::target::TargetTriple,
    lib_paths: &[PathBuf],
    qif_opts: &arc::QifCompileOptions,
    compile_options: &arc::CompileOptions,
) -> Result<(), String> {
    if is_project {
        arc::compile_test_project(
            project_root,
            release,
            debug,
            Some(output_path),
            Some(obj_dir),
            Some(triple),
            lib_paths,
            qif_opts,
            compile_options,
        )
    } else {
        arc::compile_test_file(
            file,
            release,
            debug,
            Some(output_path),
            Some(obj_dir),
            Some(triple),
            lib_paths,
            qif_opts,
            compile_options,
        )
    }
}

/// `arc ui`：声明式 UI 工具入口（RFC 037 M1 D11 / M2 ARML code-behind）。
fn run_ui(command: UiCommand) -> Result<(), String> {
    use arc_ui::{
        ascii_preview, check_codebehind_report, generate_project, inspect_json,
        verify_report_with_strict, CodegenOptions, Parser, TypeChecker,
    };
    match command {
        UiCommand::Inspect { file } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
            let doc = Parser::parse(&src).map_err(|e| format!("parse error: {e}"))?;
            let json = inspect_json(&doc);
            let ascii = ascii_preview(&doc);
            println!("=== JSON ===");
            println!("{json}");
            println!("=== ASCII Preview ===");
            println!("{ascii}");
            Ok(())
        }
        UiCommand::Verify { file, strict } => {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
            let doc = Parser::parse(&src).map_err(|e| format!("parse error: {e}"))?;
            let checker = TypeChecker::new();
            let mut report = verify_report_with_strict(&doc, &checker, strict);
            // RFC 016 §4.3 P1 红线：`.arml.as` 污染检查（双文件配对扫描）
            check_codebehind_report(&file, &mut report);
            if report.is_ok() {
                let strict_note = if strict { " (strict)" } else { "" };
                println!(
                    "ok: {} components, {} bindings (0 errors, {} warnings{strict_note})",
                    report.type_check.component_count,
                    report.type_check.binding_count,
                    report.warning_count(),
                );
                Ok(())
            } else {
                println!(
                    "FAILED: {} error(s), {} warning(s)",
                    report.error_count(),
                    report.warning_count(),
                );
                for e in &report.type_check.errors {
                    println!("  error: {e}");
                }
                for e in &report.type_check.warnings {
                    println!("  warning: {e}");
                }
                for e in &report.adaptive_issues {
                    println!("  adaptive: {e}");
                }
                for e in &report.adaptive_warnings {
                    println!("  adaptive-warning: {e}");
                }
                for e in &report.codebehind_issues {
                    println!("  codebehind: {e}");
                }
                for e in &report.a11y_issues {
                    println!("  a11y: {e}");
                }
                for e in &report.layout_issues {
                    println!("  layout: {e}");
                }
                Err(format!(
                    "verification failed: {} error(s)",
                    report.error_count()
                ))
            }
        }
        UiCommand::Codegen {
            files,
            output,
            namespace,
            user_source,
            program,
            config,
        } => {
            if files.is_empty() {
                return Err("codegen requires at least one .arml file".into());
            }
            // obj/ 是编译器固定目录（ARML codegen .g.as 输出位置）。
            // 以源文件所在目录为基准，避免 CWD 污染。
            let obj_dir = files
                .first()
                .and_then(|f| f.parent())
                .map(|p| p.join("obj"))
                .unwrap_or_else(|| PathBuf::from("obj"));
            let project_root = std::env::current_dir().ok();
            let opts = CodegenOptions {
                namespace: namespace.unwrap_or_else(|| "Arc.UI.Generated".into()),
                user_sources: user_source,
                program,
                obj_dir: Some(obj_dir),
                project_root,
                config: config.unwrap_or_else(|| "Debug".into()),
                framework_sources: Vec::new(),
            };
            let result = generate_project(&files, &opts)?;

            // 报告独立 .g.as 文件路径
            for gen in &result.generated_files {
                println!("generated: {}", gen.path.display());
            }

            // 输出 Program.as
            match output {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("create output dir failed: {e}"))?;
                    }
                    std::fs::write(&path, &result.program)
                        .map_err(|e| format!("write {}: {e}", path.display()))?;
                    println!("generated: {}", path.display());
                }
                None => {
                    print!("{}", result.program);
                }
            }
            // 显式标记已使用（避免未使用绑定告警）
            let _ = CodegenOptions::default;
            Ok(())
        }
    }
}

/// 校验单个 Arc 项目（`arc check` 核心；供单项目与 workspace 成员复用）。
///
/// 对 `[ui]` 项目先 ARML codegen 合并，再 typeck + borrowck 校验。
fn check_single_project(
    file: &Path,
    manifest: &ArcManifest,
    triple: &arc::target::TargetTriple,
) -> Result<(), String> {
    // 项目根 = 源文件所在目录（`file` 为单个 `.as` 源文件）或目录本身
    // （`file` 为 workspace 成员根）。不得把源文件路径当项目根，否则
    // obj_dir 解析为 `<file>/obj/<config>`——把 `.as` 当目录 create_dir_all
    // 会因文件已存在而失败（os error 183）。
    let project_root = if file.is_dir() {
        file.to_path_buf()
    } else {
        file.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    let (obj_dir, _bin_dir) =
        resolve_project_dirs(file, &(project_root, manifest.clone()), None, "Debug");
    let project_kind = project_kind_from_manifest(&(file.to_path_buf(), manifest.clone()));
    // ARML UI 项目（[ui] 节）：先 codegen 生成 obj/Debug/code/Program.as（合并
    // 全部 .g.as + .arml.as + 框架源码），再 check 该合并文件——使 `arc check`
    // 对 UI 项目（单文件或目录入口）也能通过 partial class 跨文件合并校验。
    let check_target = if let Some(ui) = &manifest.ui {
        codegen_ui_project(file, manifest, ui, "Debug")?
    } else {
        file.to_path_buf()
    };
    arc::compile_file(
        &check_target,
        false,
        false,
        None,
        Some(&obj_dir),
        Some(triple),
        project_kind,
        &arc::CompileOptions::default(),
    )?;
    Ok(())
}

/// 编译单个 Arc 项目（`arc build` 核心；供单项目与 workspace 成员复用）。
#[allow(clippy::too_many_arguments)]
fn build_single_project(
    file: &Path,
    manifest: &(PathBuf, ArcManifest),
    output: Option<&Path>,
    release: bool,
    debug: bool,
    triple: &arc::target::TargetTriple,
    ani_native_lib: &[PathBuf],
    dynamic: bool,
    obj_dir: Option<&PathBuf>,
    field_cycle_policy: Option<&str>,
    incremental_report: bool,
    emit_llvm: bool,
) -> Result<(), String> {
    // RFC 017 D8 v1.0：解析动态库意图。
    // CLI --dynamic 优先；否则取 manifest [package].dynamic（仅 kind="library" 时生效）。
    eprintln!("[BUILD] build_single_project start: {}", file.display());
    // 帮助文本契约：--dynamic 仅对 kind = "library" 生效。binary 项目显式传入
    // 即意图错位，直接报错而非静默忽略——防止「以为产了动态库实际产了 .exe」
    // 的隐蔽误用（与 UI/dynamic 组合的既有拒绝语义一致收紧）。
    if dynamic && manifest.1.package.kind != "library" {
        return Err(format!(
            "--dynamic requires [package] kind = \"library\"; project '{}' declares kind = \"{}\"",
            manifest.1.package.name, manifest.1.package.kind
        ));
    }
    let is_dynamic =
        dynamic || (manifest.1.package.dynamic && manifest.1.package.kind == "library");

    // 项目模式：manifest 含 [ui] 节 → ARML codegen + 编译 Program.as
    //（UI 项目当前不支持动态库输出，--dynamic 在 UI 项目下被拒绝）
    if !is_dynamic {
        if let Some(ref ui) = manifest.1.ui {
            return build_ui_project(
                &manifest.0,
                &manifest.1,
                ui,
                output,
                release,
                debug,
                triple,
                ani_native_lib,
                emit_llvm,
            );
        }
    }

    let config = if release { "Release" } else { "Debug" };
    let (obj_dir_resolved, bin_dir) = resolve_project_dirs(file, manifest, obj_dir, config);

    let out = output.map(Path::to_path_buf).unwrap_or_else(|| {
        let name = if is_dynamic {
            default_dynamic_library_name(&manifest.1.package.name)
        } else {
            default_binary_name(file, triple)
        };
        bin_dir.join(name)
    });
    let lib_paths = merge_native_lib_paths(ani_native_lib, manifest);
    let project_kind = project_kind_from_manifest(manifest);
    let field_cycle_policy = resolve_field_cycle_policy(field_cycle_policy, &manifest.1)?;
    let compile_options = arc::CompileOptions {
        field_cycle_policy,
        // RFC 017 产物域：`--emit-llvm` 显式保留文本 IR，默认焚毁。
        keep_ir: emit_llvm,
        ..Default::default()
    };
    if is_dynamic {
        // RFC 017 D8 v1.0 + RFC 017 M2: 动态库编译路径。
        let export_symbols: Vec<String> = Vec::new();
        let pkg_meta = package_meta_from_manifest(manifest);
        arc::compile_file_to_dynamic_library(
            file,
            release,
            debug,
            Some(&out),
            Some(&obj_dir_resolved),
            Some(triple),
            &lib_paths,
            &export_symbols,
            pkg_meta,
            &compile_options,
        )?;
        println!(
            "built (dynamic library): {} (target: {})",
            out.display(),
            triple.as_str()
        );
    } else {
        // P0 增量构建：保守指纹比对，未变更则跳过整条 parse→typeck→codegen→link
        // 流水线（单 TU 下收益最大）。仅对标准可执行/库路径生效（UI/dynamic 恒重建）。
        let out_name = out
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| manifest.1.package.name.clone());
        let report_start = std::time::Instant::now();
        let fp = arc::compute_fingerprint(
            file,
            Some(&manifest.1),
            config,
            triple.as_str(),
            &out_name,
            debug,
        );
        if arc::is_up_to_date(&obj_dir_resolved, &out, &fp) {
            println!(
                "up-to-date: {} (target: {})",
                out.display(),
                triple.as_str()
            );
            if incremental_report {
                let mut report =
                    arc::compute_incremental_report(file, Some(&manifest.1), &obj_dir_resolved);
                report.elapsed_ms = report_start.elapsed().as_millis() as u64;
                print!("{}", arc::format_incremental_report(&report));
            }
            return Ok(());
        }

        arc::compile_file_with_native(
            file,
            release,
            debug,
            Some(&out),
            Some(&obj_dir_resolved),
            Some(triple),
            &lib_paths,
            project_kind,
            &compile_options,
        )?;
        arc::record_build(&obj_dir_resolved, &fp);
        println!("built: {} (target: {})", out.display(), triple.as_str());
        if incremental_report {
            let mut report =
                arc::compute_incremental_report(file, Some(&manifest.1), &obj_dir_resolved);
            report.elapsed_ms = report_start.elapsed().as_millis() as u64;
            print!("{}", arc::format_incremental_report(&report));
        }
    }
    Ok(())
}

/// `arc build` 项目模式：ARML codegen + 编译 Program.as（对标 `dotnet build`）。
///
/// 流程：
/// 1. 调用 `arc_ui::generate_project` 处理所有 `arml` 文件：
///    - 写独立 `.g.as` 到 `obj/<config>/<ClassName>.g.as`
///    - 合并 `.g.as` + 用户 `sources` + `program` 为 `Program.as` 字符串
/// 2. 写合并后的 `Program.as` 到 `obj/<config>/Program.as`
/// 3. 调用 `compile_file_with_native` 编译 `Program.as` 为
///    `bin/<config>/<package.name>.exe`
fn build_ui_project(
    root: &Path,
    manifest: &arc::manifest::ArcManifest,
    ui: &arc::manifest::UiSection,
    output: Option<&Path>,
    release: bool,
    debug: bool,
    triple: &arc::target::TargetTriple,
    native_lib_paths: &[PathBuf],
    // RFC 017 产物域：`arc run` 无 `--emit-llvm` 旋钮，调用方恒传 false（焚毁）。
    emit_llvm: bool,
) -> Result<(), String> {
    let config = if release { "Release" } else { "Debug" };

    // 1-5. ARML codegen：生成 .g.as + 合并 Program.as 到 obj/<config>/code/，
    // 返回合并后的 Program.as 路径。
    let program_as_path = codegen_ui_project(root, manifest, ui, config)?;

    // 6. 解析输出二进制路径
    // bin/<config>/<package.name>.exe（对标 WPF / MSBuild：最终产物在 bin/<config>/）
    let bin_dir = root.join("bin").join(config);
    let out = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| bin_dir.join(format!("{}{}", manifest.package.name, exe_suffix())));
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create bin dir failed: {e}"))?;
    }

    // RFC 037 §9 / custom-fonts：项目 `Assets/` 同步到 `bin/<config>/`，保持相对路径，
    // 供 FontManager 相对应用基目录（exe 目录）解析。
    copy_project_assets_to_bin(root, &bin_dir)?;

    // 7. 合并 native_lib_paths
    let manifest_tuple = (root.to_path_buf(), manifest.clone());
    let lib_paths = merge_native_lib_paths(native_lib_paths, &manifest_tuple);

    // 8. 编译 Program.as → 二进制（UI 项目始终为可执行程序）
    arc::compile_file_with_native(
        &program_as_path,
        release,
        debug,
        Some(&out),
        Some(&root.join("obj").join(config).join("code")),
        Some(triple),
        &lib_paths,
        codegen::ProjectKind::Executable,
        &arc::CompileOptions {
            // RFC 017 产物域：与 `arc build` 共享 `--emit-llvm` 语义。
            keep_ir: emit_llvm,
            ..arc::CompileOptions::default()
        },
    )?;
    println!("built: {} (target: {})", out.display(), triple.as_str());
    Ok(())
}

/// 将项目根 `Assets/` 递归复制到 `bin/<config>/`，保持相对路径（字体等运行时资源）。
fn copy_project_assets_to_bin(project_root: &Path, bin_dir: &Path) -> Result<(), String> {
    let assets = project_root.join("Assets");
    if !assets.is_dir() {
        return Ok(());
    }
    copy_dir_recursive(&assets, &bin_dir.join("Assets"))
        .map_err(|e| format!("copy Assets → bin failed: {e}"))?;
    println!("copied: Assets → {}", bin_dir.join("Assets").display());
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// RFC 017 产物域（`arc clean`，UX 迭代评审 §2.3）：删除单个产物目录并按
/// 人类可读字节数对账。删除前统计（事后无从对账），目录不存在时幂等成功
///——clean 的语义是「产物已不在」而非「报错」，幂等让脚本与重复调用免于分支。
fn clean_dir(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let bytes = dir_size(dir);
    std::fs::remove_dir_all(dir).map_err(|e| format!("clean {} failed: {e}", dir.display()))?;
    println!("cleaned: {} ({})", dir.display(), format_size(bytes));
    Ok(())
}

/// 递归统计目录字节量（`arc clean` 对账）。
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(ty) if ty.is_dir() => total += dir_size(&entry.path()),
                Ok(ty) if ty.is_file() => {
                    total += entry.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    total
}

/// 字节数人类可读（1024 进制，对标文件管理器惯例；`arc clean` 对账输出）。
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// ARML UI 项目 codegen：生成 `.g.as` + 合并 `Program.as` 到 `obj/<config>/code/`，
/// 返回合并后的 `obj/<config>/code/Program.as` 路径。
///
/// 供 `arc build`（[ui] 项目）与 `arc check`（[ui] 项目单文件/目录入口）共用。
fn codegen_ui_project(
    root: &Path,
    manifest: &arc::manifest::ArcManifest,
    ui: &arc::manifest::UiSection,
    config: &str,
) -> Result<PathBuf, String> {
    use arc_ui::{generate_project, CodegenOptions};

    // 1. 解析 obj_dir（RFC 031：obj/ 是编译器固定目录）。注意 generate_project
    //    内部会自行追加 `config/code`，故此处传入**基础** obj 目录：
    //    - `.g.as` 落入 obj/<config>/code/<rel>/<stem>.g.as
    //    - 合并 `Program.as` 落入 obj/<config>/code/Program.as
    let obj_dir = root.join("obj");

    // 2. 解析 namespace（[ui].namespace > [package].namespace）
    let namespace = ui
        .namespace
        .clone()
        .unwrap_or_else(|| manifest.package.namespace.clone());

    // 3. 构造 codegen 选项
    let arml_files: Vec<PathBuf> = ui.arml.iter().map(|s| root.join(s)).collect();
    let user_sources: Vec<PathBuf> = ui.sources.iter().map(|s| root.join(s)).collect();
    let program = ui.program.as_ref().map(|s| root.join(s));

    // 自动发现 Arc.UI 框架源文件（WPF-aligned 模式必需）。
    //
    // 这些文件会被 strip namespace + using 后合并到 Program.as 末尾，
    // 使所有框架类型（Element/Window/Application/WindowHost 等）在项目命名空间下
    // 可见。这避免用户代码显式 `using Arc.UI.Components`。
    //
    // 当前最小集合覆盖 ArmlDemo demo 所需的全部类型：
    //   - Element.as / DependencyProperty.as (Markup 基础)
    //   - Window.as / Application.as / WindowHost.as (Components 入口)
    //   - ResourceDictionary.as (Application.Resources 引用)
    //   - Dictionary.as (ResourceDictionary 字段类型，Arc.Collections builtin)
    let framework_sources = discover_framework_sources(root);

    let opts = CodegenOptions {
        namespace,
        user_sources,
        program,
        obj_dir: Some(obj_dir.clone()),
        project_root: Some(root.to_path_buf()),
        config: config.to_string(),
        framework_sources,
    };

    // 4. 执行 codegen —— 生成 .g.as + Program.as 内容
    let result = generate_project(&arml_files, &opts)?;

    // 5. 写 Program.as 到 obj/<config>/code/Program.as
    let program_as_path = obj_dir.join(config).join("code").join("Program.as");
    if let Some(parent) = program_as_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create obj config dir failed: {e}"))?;
    }
    std::fs::write(&program_as_path, &result.program)
        .map_err(|e| format!("write {}: {e}", program_as_path.display()))?;

    // 报告生成产物
    for gen in &result.generated_files {
        println!("generated: {}", gen.path.display());
    }
    println!("generated: {}", program_as_path.display());

    Ok(program_as_path)
}

/// 发现 Arc.UI 框架源文件（WPF-aligned 项目模式专用）。
///
/// 在 workspace root 下查找 `std/UI/Core/`，返回 ArmlDemo demo 所需的最小集合：
///   - std/Arc/Signal.as                （Signal<T> 响应式基元，归 Arc 根命名空间）
///   - std/UI/Core/Data/Binding.as           （Binding 绑定描述）
///   - std/UI/Core/Data/DataContext.as       （DataContext 数据上下文）
///   - std/UI/Core/Markup/Element.as         （所有 UI 元素基类）
///   - std/UI/Core/Markup/DependencyProperty.as
///   - std/UI/Core/Data/BindingOperations.as
///   - std/UI/Core/Components/Window.as
///   - std/UI/Core/Components/Application.as
///   - std/UI/Core/Components/WindowHost.as
///   - std/UI/Core/Components/Text.as         （M3：文本元素）
///   - std/UI/Core/Components/Button.as       （M3：按钮元素）
///   - std/UI/Core/Components/Layout/StackPanel.as  （M3：栈式布局）
///   - std/UI/Core/Styling/ResourceDictionary.as
///
/// Signal<T> 已迁移至 `Arc` 根命名空间（响应式原语通用化），不在 `Arc.UI`
/// 专属。Data/ 与 Markup/ 子目录（Binding/DataContext/Element/DP/BO）仍归属
/// `Arc.UI` 根命名空间（按 RFC 003 §3.2「子命名空间与目录解耦」+ 命名空间分层原则）。
///
/// 文件不存在时跳过（不报错），允许最小化安装场景。
/// 未来扩展：M4+ 可根据 .arml 实际引用的元素动态发现更多 Components/Layout 源。
fn discover_framework_sources(project_root: &Path) -> Vec<PathBuf> {
    // Canonicalize to avoid relative-path issues in find_workspace_root.
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let workspace = arc::find_workspace_root(&canonical_root);
    // RFC 031 §8：尊重项目 `[std].path` 覆盖；完整解析链（`[std].path` → SDK 捆绑
    // std → `ARC_STD_ROOT` → workspace 兜底）。
    let std_dir = if let Some((manifest_dir, m)) = arc::find_arc_manifest(&canonical_root) {
        arc::resolve_effective_std_root(&workspace, Some(&manifest_dir), m.std.as_ref())
    } else {
        arc::resolve_effective_std_root(&workspace, None, None)
    };

    let candidates = [
        // Arc 基础接口
        "Arc/IDisposable.as",
        // RFC 004 M1：Generic Math 接口 facade（static abstract 接口成员）
        "Arc/INumber.as",
        "Arc/IEquatable.as",
        "Arc/IHashable.as",
        "Arc/IComparable.as",
        // Arc.Collections（List<T> 依赖链：List → IEnumerable → IEnumerator）
        "Arc/Collections/IEnumerator.as",
        "Arc/Collections/IEnumerable.as",
        "Arc/Collections/List.as",
        "Arc/Collections/ListEnumerator.as",
        // M4 集合绑定：ObservableCollection<T> 级联（集合级变更表面 + 动作枚举；
        // BindingOperations.BindCollection 配对逻辑依赖，merged UI build 内联）
        "Arc/Collections/ObservableCollection.as",
        "Arc/Collections/CollectionChangedEventArgs.as",
        "Arc/Collections/CollectionChangeAction.as",
        // RFC 037 M1.1：Signal<T> 已迁移至 Arc 根命名空间（响应式原语通用化）
        "Arc/Signal.as",     // Signal<T> 响应式基元（Window 属性后端）
        "Arc/Tasks/Task.as", // RFC 037 M-AS1: ShowAsync / RunAsync 骨架
        // UI 根命名空间核心原语（Data/ 与 Markup/ 子目录扁平化到 Arc.UI，
        // 按命名空间分层原则——基类放根命名空间，派生放子命名空间）
        "UI/Core/Data/Binding.as",     // Binding 绑定描述
        "UI/Core/Data/DataContext.as", // DataContext 数据上下文
        // UI 根命名空间 variant 类型（RFC 037 D2 / RFC 004）
        "UI/Core/Markup/Content.as", // Content variant——ContentControl.Content 类型
        // UI.Markup 基类链（Element → FrameworkElement → Control/Panel/Shape）
        // RFC 037 D1 WPF 同构层级：派生类继承基类 DP，需完整基类链参与 typeck
        "UI/Core/Markup/Element.as",
        "UI/Core/Markup/FrameworkElement.as", // Width/Height/Margin 等 DP 声明层
        "UI/Core/Markup/Control.as",          // Background/Foreground/Font 等 DP 声明层
        "UI/Core/Markup/Panel.as",            // 布局面板基类（StackPanel/Grid/...）
        "UI/Core/Markup/Shape.as",            // 形状基类（Rectangle/Ellipse/...）
        "UI/Core/Markup/DependencyProperty.as",
        "UI/Core/Data/BindingOperations.as",
        // UI.Components（派生自 Element/FrameworkElement/Control/ContentControl/Panel）
        "UI/Core/Components/ContentControl.as", // 内容控件基类——Window/Button 中间层
        "UI/Core/Components/Window.as",
        "UI/Core/Components/Application.as",
        "UI/Core/Components/WindowHost.as",
        "UI/Core/Layout/LayoutManager.as",
        "UI/Core/Layout/ITextMetrics.as",
        "UI/Core/Layout/TextMeasuring.as",
        "UI/Core/Layout/LayoutHelper.as",
        "UI/Core/Components/Text.as",
        "UI/Core/Components/Button.as",
        "UI/Core/Components/Layout/StackPanel.as",
        // UI.Styling（RFC 037 D3/D4：variant 承载 SetterValue/ResourceValue）
        "UI/Core/Styling/SetterValue.as", // SetterValue variant——Setter.Value 类型
        "UI/Core/Styling/Setter.as",      // Setter struct——依赖 SetterValue
        "UI/Core/Styling/ControlTemplate.as", // ControlTemplate struct——ResourceValue payload
        "UI/Core/Styling/Style.as",       // Style class——依赖 Setter
        "UI/Core/Styling/ResourceValue.as", // ResourceValue variant——ResourceDictionary value
        "UI/Core/Components/Input.as",
        "UI/Core/Internal/PlatformTreeSync.as",
        "UI/Core/Internal/PointerRouter.as",
        "UI/Core/Internal/ScrollRouter.as",
        "UI/Core/Internal/InputFocusRouter.as",
        "UI/Core/Internal/FocusManager.as",
        "UI/Core/Internal/UIDispatcher.as",
        "UI/Core/Internal/FramePump.as",
        // EditorInputRouter：仅 CodeEditor 示例链（RFC 037 §4）
        "UI/Core/Internal/ImeBridge.as",
        "UI/Core/Layout/LayoutSize.as",
        "UI/Core/Styling/ResourceDictionary.as",
        "UI/Core/Styling/BuiltInTheme.as", // 内置 Light/Dark 主题键常量 + 几何/motion + 薄工厂
        "UI/Core/Styling/BuiltInTheme.Colors.g.as", // UI-P2：Themes/*.arml 生成的色值填充
        "UI/Core/Styling/ThemeDictionary.as",
        "UI/Core/Styling/StyleManager.as",
        "UI/Core/Styling/VisualStateManager.as", // 状态→ControlVisual 视觉配方（颜色/几何/深度）
        "UI/Core/Media/Color.as",                // 结构化颜色（RGBA 0–1）
        "UI/Core/Media/Brush.as",                // 画刷体系（Solid/LinearGradient）
        "UI/Core/Media/Brushes.as",              // 命名色注册表（WPF Brushes 对齐；命名色单一来源）
        "UI/Core/Media/Elevation.as",            // 深度/软阴影规格
        "UI/Core/Layout/CornerRadius.as",        // 圆角四角结构
        "UI/Core/Components/VisualHost.as",
    ];

    candidates
        .iter()
        .map(|rel| std_dir.join(rel))
        .filter(|p| p.exists())
        .collect()
}

/// `arc inspect`：运行 `parse → hir → typeck → collect_arcgr_file`，输出可达性分析摘要。
///
/// 可选 `--format json|human`（默认 human）切换输出格式；可选 `--emit <path>`
/// 同时写入 `.arcgr` 二进制（RFC 034 M2 Step 5：跨工具链共享语义索引）。
fn run_inspect(file: PathBuf, format: Option<String>, emit: Option<PathBuf>) -> Result<(), String> {
    use arc::inspect::{emit_arcgr, format_human, format_json, inspect_source};

    let report = inspect_source(&file, &*arc::Equipments::new().context)?;

    if let Some(path) = &emit {
        emit_arcgr(&report, path)?;
        eprintln!("emitted: {}", path.display());
    }

    let fmt = format.as_deref().unwrap_or("human");
    match fmt {
        "human" => print!("{}", format_human(&report)),
        "json" => println!("{}", format_json(&report)?),
        other => return Err(format!("unknown --format `{other}` (expected: human|json)")),
    }
    Ok(())
}

/// `arc locate`：从 `.arcgr` 查询符号定义位置（RFC 034 M3）。
fn run_locate(arcgr: PathBuf, symbol: String, format: Option<String>) -> Result<(), String> {
    use arc::query::{format_locate_human, load_arcgr, locate};

    let file = load_arcgr(&arcgr)?;
    let result = locate(&file, &symbol)
        .ok_or_else(|| format!("symbol `{symbol}` not found in {}", arcgr.display()))?;

    let fmt = format.as_deref().unwrap_or("human");
    match fmt {
        "human" => println!("{}", format_locate_human(&result)),
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        ),
        other => return Err(format!("unknown --format `{other}` (expected: human|json)")),
    }
    Ok(())
}

/// `arc explain`：生成 L2 符号卡片（RFC 034 M3）。
fn run_explain(arcgr: PathBuf, symbol: String, format: Option<String>) -> Result<(), String> {
    use arc::query::{explain, format_explain_human, load_arcgr};

    let file = load_arcgr(&arcgr)?;
    let result = explain(&file, &symbol)
        .ok_or_else(|| format!("symbol `{symbol}` not found in {}", arcgr.display()))?;

    let fmt = format.as_deref().unwrap_or("human");
    match fmt {
        "human" => print!("{}", format_explain_human(&result)),
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        ),
        other => return Err(format!("unknown --format `{other}` (expected: human|json)")),
    }
    Ok(())
}

/// `arc query <callers|callees|impls|references>`：意图查询（RFC 034 M3）。
fn run_query(
    kind: String,
    arcgr: PathBuf,
    symbol: String,
    format: Option<String>,
) -> Result<(), String> {
    use arc::query::{format_query_human, load_arcgr, query, QueryKind};

    let qk = QueryKind::parse(&kind).ok_or_else(|| {
        format!("unknown query kind `{kind}` (expected: callers|callees|impls|references)")
    })?;
    let file = load_arcgr(&arcgr)?;
    let result = query(&file, qk, &symbol)
        .ok_or_else(|| format!("symbol `{symbol}` not found in {}", arcgr.display()))?;

    let fmt = format.as_deref().unwrap_or("human");
    match fmt {
        "human" => print!("{}", format_query_human(&result)),
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        ),
        other => return Err(format!("unknown --format `{other}` (expected: human|json)")),
    }
    Ok(())
}

/// `arc overview`：AI 首触入口——输出项目骨架 L0/L1（RFC 034 M4）。
///
/// 默认输出 L0 项目概览（~500 tok）；`--detail` 输出 L0+L1 完整模块面（~2K tok）。
/// 要求 `arc.toml` 存在——否则返回错误（M4 核心数据源）。
fn run_overview(file: PathBuf, detail: bool, format: Option<String>) -> Result<(), String> {
    use arc::overview::{
        format_l0_human, format_l0_json, format_l1_human, format_l1_json, overview_source,
    };

    let report = overview_source(&file, &*arc::Equipments::new().context)?;

    let fmt = format.as_deref().unwrap_or("human");
    match (fmt, detail) {
        ("human", false) => print!("{}", format_l0_human(&report)),
        ("human", true) => print!("{}", format_l1_human(&report)),
        ("json", false) => println!("{}", format_l0_json(&report)?),
        ("json", true) => println!("{}", format_l1_json(&report)?),
        other => {
            return Err(format!(
                "unknown --format `{}` (expected: human|json)",
                other.0
            ))
        }
    }
    Ok(())
}

fn resolve_target(
    target: Option<&str>,
    experimental_wasm_emit: bool,
) -> Result<arc::target::TargetTriple, String> {
    match target {
        Some(t) if experimental_wasm_emit => {
            arc::target::TargetTriple::parse_for_experimental_wasm_emit(t)
        }
        Some(t) => arc::target::TargetTriple::parse_for_build(t),
        None => Ok(arc::target::TargetTriple::host()),
    }
}

/// 统一解析项目输出目录——对标 MSBuild 的 `bin/<config>/` + `obj/<config>/` 固定约定。
///
/// **核心原则**（工作区卫生 G″ / RFC 031 §5）：项目根下固定
/// `obj/<config>/`（中间产物）与 `bin/<config>/`（最终产物）。
/// Cargo 的 workspace `target/` 与 e2e 隔离夹具 `target/e2e/<name>/` 互不混用；
/// 项目模型**禁止**默认写入 `target/bin` / `target/obj`。
/// CLI `--obj-dir` 仅覆盖 obj_dir（用于 e2e 并发测试隔离），bin_dir 不可覆盖。
///
/// 优先级（obj_dir）：
///   1. `obj_dir_override` — CLI --obj-dir flag
///   2. 默认 — `<project_root>/obj/<config>/`
///
/// 返回 `(obj_dir, bin_dir)`
fn resolve_project_dirs(
    _file: &Path,
    manifest: &(PathBuf, ArcManifest),
    obj_dir_override: Option<&PathBuf>,
    config: &str,
) -> (PathBuf, PathBuf) {
    let project_root = &manifest.0;

    let obj_dir = obj_dir_override
        .cloned()
        .unwrap_or_else(|| project_root.join("obj").join(config));

    let bin_dir = project_root.join("bin").join(config);

    (obj_dir, bin_dir)
}

fn default_binary_name(file: &Path, target: &arc::target::TargetTriple) -> String {
    if target.is_wasm_family() {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        return format!("{stem}.wasm");
    }
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    format!("{}{}", stem, if cfg!(windows) { ".exe" } else { "" })
}

/// 从 manifest 提取包元数据——嵌入动态库供运行时版本校验。
///
/// 仅当 [package].name 非空时返回 Some，否则返回 None。
fn package_meta_from_manifest(manifest: &(PathBuf, ArcManifest)) -> Option<arc::PackageMeta> {
    let (_, m) = manifest;
    if m.package.name.is_empty() {
        None
    } else {
        // 依赖均为本地 path 源码引用（对标 C# ProjectReference），依赖映射为
        // 运行时可自动加载的动态库。键排序保证确定性。
        let mut dependencies: Vec<String> = m.dependencies.keys().cloned().collect();
        dependencies.sort();
        Some(arc::PackageMeta {
            name: m.package.name.clone(),
            version: m.package.version.clone(),
            edition: m.package.edition.clone(),
            dependencies,
            // 布局指纹表由 codegen 在 compile_module_to_dynamic_library 内
            // 按 layouts 填充（此处的 manifest 无类型信息）。
            layout_sigs: Vec::new(),
        })
    }
}

/// 从 manifest 确定项目类型——对标 C# 项目模型，编译期固定规则。
///
/// - `kind = "library"` → 库项目（不要求 Main()，可含 Entry<T>() 泛型入口）
/// - `kind = "binary"` 或其他 → 可执行项目（必须有且仅有一个 Main()）
fn project_kind_from_manifest(manifest: &(PathBuf, ArcManifest)) -> codegen::ProjectKind {
    if manifest.1.package.kind == "library" {
        codegen::ProjectKind::Library
    } else {
        codegen::ProjectKind::Executable
    }
}

/// RFC 017 D8 v1.0: 动态库默认文件名（项目模式，按 package.name）。
///
/// 平台命名惯例：
/// - Windows：`<name>.dll`（不带 `lib` 前缀）
/// - Linux / OHos：`lib<name>.so`（带 `lib` 前缀）
/// - macOS：`lib<name>.dylib`（带 `lib` 前缀）
fn default_dynamic_library_name(package_name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{package_name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{package_name}.dylib")
    } else {
        format!("lib{package_name}.so")
    }
}

/// RFC 016 M2: 合并 CLI `--ani-native-lib` 与 manifest `[native].ani-native-lib`。
///
/// 搜索顺序（隐式第一项 + 显式配置）：
/// 1. **主程序根目录**（隐式第一项，`ani-native-lib` 的默认值——manifest 未配置
///    且未传 CLI 参数时仍保证主程序根目录被搜索）
/// 2. manifest `[native].ani-native-lib` 路径（相对路径以 manifest 根为基准解析为绝对路径）
/// 3. CLI `--ani-native-lib` 路径（按用户输入原样使用）
///
/// 主程序根目录始终在列（无法通过配置移除），保证项目根下的库文件无需任何配置即可被发现。
fn merge_native_lib_paths(
    cli_paths: &[PathBuf],
    manifest: &(PathBuf, ArcManifest),
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let (root, m) = manifest;
    // 隐式第一项：主程序根目录（ani-native-lib 的默认值）。
    out.push(root.clone());
    for p in &m.native.ani_native_lib {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            out.push(pb);
        } else {
            out.push(root.join(pb));
        }
    }
    out.extend(cli_paths.iter().cloned());
    // 去重（保留首现顺序）：主程序根目录可能被显式路径重复声明。
    let mut seen: Vec<PathBuf> = Vec::new();
    out.retain(|p| {
        if seen.contains(p) {
            false
        } else {
            seen.push(p.clone());
            true
        }
    });
    out
}

/// RFC 005 里程碑④：解析字段环 warning 策略——CLI 优先，否则 `arc.toml
/// [compiler] field_cycle_policy`，缺省 `"warn"`。**无 `error` 档**（未知值报错，
/// 拒绝把 warning 升级为 error）。
fn resolve_field_cycle_policy(
    cli: Option<&str>,
    manifest: &ArcManifest,
) -> Result<arc::FieldCyclePolicy, String> {
    let raw = match cli {
        Some(s) => s,
        None => manifest.compiler.field_cycle_policy.as_str(),
    };
    arc::FieldCyclePolicy::parse(raw).ok_or_else(|| {
        format!(
            "invalid field_cycle_policy `{raw}`: must be \"warn\" or \"off\" \
             (arc-cycle-001 is warning-by-default and never upgrades to an error)"
        )
    })
}
