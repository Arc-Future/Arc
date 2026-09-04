# ONNX Runtime 外部依赖（Arc.AI.Onnx · crates/runtime-onnx）

本目录收敛 **shim 桥接源码**（`onnx_shim.{h,cpp}`），把 ONNX Runtime C++ API
（`onnxruntime_cxx_api.h`）包成 `extern "C"` C ABI，供 Arc 的验证式 FFI
（[onnx.ani](../arc/native/onnx.ani) · RFC 027 / RFC 103）驱动推理。

ONNX Runtime 是**重量级外部依赖**：**不 vendored 进仓库**（工作区卫生 G″，对齐
zxing-cpp 先例）。下载/编译/分发由脚本负责，产物落 `target/onnx-native/`：

| 脚本 | 职责 | 产物 |
|------|------|------|
| `scripts/fetch-onnx-native.ps1` | 下载 `Microsoft.ML.OnnxRuntime.DirectML` NuGet + SHA256 记录 | `target/onnx-native/{onnxruntime.dll, onnxruntime.lib, include/, SHA256.txt}` |
| `scripts/build-onnx-shim.ps1` | clang++/MSVC 编译 `onnx_shim.cpp` 并链 onnxruntime.lib | `target/onnx-native/onnx_shim.dll` |

## 为什么用 DirectML 包

`onnx_shim.cpp` 的 `onnx_options_append_dml` 调用
`OrtSessionOptionsAppendExecutionProvider_DML`——该 C 符号**仅 DirectML 构建导出**，
CPU-only 包不含，会导致 shim 链接失败。取 DirectML 构建可**一个 onnxruntime.dll
同时支撑 CPU 基线 + DirectML GPU EP**（与本库 `ExecutionProvider` 双枚举一致）。

## 运行时布局与加载

`onnx.ani` 为 `load="auto"` + `library = Environment.GetEnvironmentVariable("ARC_ONNX_LIB")`：
编译期库可定位 → static；否则确定性降级 runtime。运行时 `ARC_ONNX_LIB` 指向**目录**，
再追加平台库名（`onnx_shim.dll` / `libonnx_shim.so` / `libonnx_shim.dylib`）懒加载。

`onnx_shim.dll` 依赖 `onnxruntime.dll`，故两者必须在**同一目录**：

```
$env:ARC_ONNX_LIB = "e:\GitCode\RF\dlang\target\onnx-native"   # 含 onnxruntime.dll + onnx_shim.dll
```

未设置 / 目录缺库 / 符号缺失 → 模块 `unavailable`（`Native.IsAvailable("onnx") == false`），
调用点优雅降级（[OnnxNative.as](../../std/AI/Onnx/OnnxNative.as) 门闩 + `OnnxNotAvailableException`）。

## 版本锁定

| 项 | 值 |
|----|-----|
| 包 | `Microsoft.ML.OnnxRuntime.DirectML`（默认；可用 `-Package` 改） |
| 默认版本 | `1.20.1`（`-Version` 改） |
| 哈希证据 | 首次 `fetch-onnx-native.ps1` 计算并写入 `target/onnx-native/SHA256.txt` |

> **宣称纪律（RFC 025 §1.1）**：脚本自动记录 SHA256 到 `SHA256.txt`，但**正式版本锁定
> 前须与上游 release 公告人工核验**，核验通过后回填上表/下文，未核验不得宣称"已固定"。
> 版本升级 = 独立 PR（单目标）；升级后重跑两脚本并更新本表。

## 依赖形态对照

| 库 | 形态 | vendored? | 桥接 | 加载 |
|----|------|-----------|------|------|
| zxing-cpp | 外部共享库 | 否（`target/zxing-native/`） | `shim/zxing_shim.cpp` | `ARC_ZXING_LIB` |
| **ONNX Runtime** | **外部 DLL + import lib** | **否（`target/onnx-native/`）** | **`onnx_shim.cpp`** | **`ARC_ONNX_LIB`** |
| mbedTLS | 源码直编 | 是（`crates/runtime-crypto/bin/`） | `rt_crypto_native.c` | 静态 |

## 许可

ONNX Runtime 为 **MIT**。DirectML 构建同样由 Microsoft 发布（MIT）。`fetch` 脚本在可
定位时拷贝包内 `LICENSE*` 到 `target/onnx-native/LICENSE`（运行时工件不随仓库分发，
不额外登记 NOTICE；如改为 vendored 则须登记）。
