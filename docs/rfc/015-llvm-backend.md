# RFC 015 LLVM 原生后端

## 背景

Arc 是**原生 LLVM 语言**。LLVM IR 文本是编译器的**规范输出**，LLVM 工具链（clang/lld/lldb）负责优化、代码生成与调试。**不包含** C 后端、Backend trait 抽象层、`--backend` CLI flag 或任何备选后端——LLVM IR 是唯一代码生成路径。`codegen` 以 `MirCfgBody → .ll → clang` 单向链路产出最小可运行二进制。

## 设计决策

### 代码生成路径

| 项 | 决策 |
|----|------|
| 后端 | LLVM IR 文本（`emit_*` 模块）；clang 驱动 |
| 产物 | `MirCfgBody → .ll → clang 目标文件 → 链接` |
| 内存策略 | alloca + mem2reg；opaque pointers |
| 优化 | clang `-O2` |
| 调试 | DWARF 5 + 内嵌符号表（`__arc_dbg_table`） |
| 覆盖率 | LLVM source-based（`-fprofile-instr-generate -fcoverage-mapping` → `.profraw` → `llvm-profdata` → `llvm-cov` lcov），见 [references/coverage](015-llvm-backend/references/coverage.md) |

`codegen::compile_module` / `compile_module_to_object` 等完成 MIR → LLVM IR 文本（`.ll`）与对象文件发射；clang 将 `.ll` 编译为目标文件，与 `runtime.o` 链接为原生二进制或动态库。

### 异常模型（zero-cost EH）

`try`/`catch`/`finally` lowering 采用 **LLVM `invoke`/`landingpad`**（Windows SEH 主平台：`catchswitch`/`catchpad`/`cleanuppad`，`__CxxFrameHandler3` personality）：

- 未抛出路径**零开销**；
- finally 深层 unwind **恒执行**；
- catch 类型过滤 C# 对齐，`rt_exception` TLS；
- async 状态机协作：await 提取点 faulted Task 经 `rt_task_is_faulted`/`rt_task_get_exception` rethrow → 外层 catch（try 跨 await 语义正确）；
- cleanup funclet 内 `call` 携带 `"funclet"("token")` 操作数（LLVM WinEH 强制）；
- 已知 nounwind 外部与 facade 方法按 `RT_MAY_THROW` 镜像标注；
- async 状态机局部变量所有权由 **env 唯一 owner + resume 级 EH cleanup pad** 收敛，任何 unwind 路径由 personality 驱动释放恰一次。

### `nounwind` 不动点推断

用户函数的 `nounwind` 由**模块内 call-graph 不动点**推断：无局部 `Throw`/`TryCatch`，且每个调用均解析到已知 `nounwind` 被调方时才标注。已知 `nounwind` 被调方包括：

1. **模块内**已推断为 `nounwind` 的用户函数；
2. **`rt_*` 白名单**（closed-world 审计下，除 `RT_MAY_THROW` 表外的全部 `rt_*`，含 `rt_get_exception`/`rt_panic*`）；
3. 常用 libc leaf（`malloc`/`free`/`memcpy`/…）与 `llvm.*` intrinsic。

虚分派 / 接口 / 间接 / **未知外部**（native FFI 等）一律视为 may-throw——中间帧若误标 `nounwind`，unwind 会穿栈导致 `STATUS_BAD_STACK`（Windows `0xc00000ff`）。`RT_MAY_THROW` 表同时作为 invoke 转换判据（B.7 谓词）；已知 nounwind `declare` 统一补 `nounwind`。`rt_*` may-throw 表清单见 [014 运行时 ABI](014-runtime-abi.md)。

### 调试信息

DWARF 5 + 内嵌符号表（`__arc_dbg_table`）。`Exception.StackTrace` 捕获真实返回地址，经 `__arc_dbg_table` 还原函数名 + 可行时 file:line（与 DWARF `-g` 解耦；Windows MSVC/MinGW 与 POSIX 同路径）；POSIX `backtrace_symbols` 次级；仍无符号时 `at <0x…>`。

### 覆盖率（source-based coverage）

覆盖率复用 clang 管线的 **LLVM source-based coverage**（零自研插桩）：`.ll` 编译注入 `-fprofile-instr-generate -fcoverage-mapping`，链接注入 `-fprofile-instr-generate`，运行退出写 `.profraw`，经 `llvm-profdata merge` 与 `llvm-cov export -format=lcov` 产 lcov 报告。插桩范围**仅 Arc 源码**（runtime C 不插桩）。机制、CLI（`arc test --coverage`）、报告格式、QIF 衔接与验收标准的完整设计见 [references/coverage](015-llvm-backend/references/coverage.md)。

### 链接

典型链接单元：codegen 输出 → `runtime.c` → `crates/runtime/platform/<os>/window.*`（若需要）→ 系统库。`arc build` 在 `-o` 指定路径后调用宿主链接器（如 `clang`）。动态库（`--dynamic`）经 `EmitRole::DynamicLibrary` 发射内嵌 `__arc_dbg_table`/`__arc_dbg_count` + Entry wrapper + 资源导出符号（见 [017 编译产物、包体系与类型身份](017-build-artifacts-packages.md)）。

### 确定性

相同输入（源码、flags、target、工具链版本）产生相同 MIR 与等价二进制；禁止在 codegen 引入非确定性随机或时间依赖。

### 交叉编译边界

主机桌面三元组为主路径；`wasm32-unknown-unknown` / `wasm32-wasip*` 目标被视为**未支持目标**，编译报硬错误，禁止以原生方式静默编译。WASM 链接须 runtime 子集且无 `platform.o`。其余目标矩阵由 CLI 门禁约束（见 031）。

## 边界

- 本篇只讲**LLVM IR 文本后端、代码生成、链接与确定性**；管线阶段编排见 [013 编译管线架构](013-compiler-pipeline.md)。
- `rt_*` ABI 符号面与 `RT_MAY_THROW` 表见 [014 运行时 ABI](014-runtime-abi.md)。
- 语言级内存语义见 [005 内存模型与资源安全](005-memory-model.md)。
- **覆盖率机制与报告格式**见 [015 references/coverage](015-llvm-backend/references/coverage.md)；`arc test --coverage` 用户面见 [032](032-qif.md)。

---
上一节：[014 运行时 ABI](014-runtime-abi.md) · 下一节：[016 验证式 FFI 与 Native 加载](016-verified-ffi.md)