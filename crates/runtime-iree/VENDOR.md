# IREE Runtime 外部依赖（Arc.AI.Iree · crates/runtime-iree）

本目录收敛 **shim 桥接源码**（`iree_shim.{h,cpp}`），把 IREE Runtime 的高层 C API
（`iree/runtime/api.h` — instance/session/call）包成 `extern "C"` C ABI，供 Arc 的
验证式 FFI（[iree.ani](../arc/native/iree.ani) · RFC 027 / RFC 103）驱动推理。

IREE Runtime 是**重量级外部依赖**：**不 vendored 进仓库**（工作区卫生 G″，对齐
onnx/zxing 先例）。下载/编译/分发由脚本负责，产物落 `target/iree-native/`：

| 脚本 | 职责 | 产物 |
|------|------|------|
| `scripts/fetch-iree-native.ps1` | 下载 IREE runtime release + SHA256 记录 | `target/iree-native/{iree.dll, iree.lib, include/, SHA256.txt}` |
| `scripts/build-iree-shim.ps1` | clang++/MSVC 编译 `iree_shim.cpp` 并链 IREE runtime | `target/iree-native/iree_shim.dll` |

## 里程碑状态（RFC 053 S3）

- **M-I0（本里程碑）**：立 `iree.ani` 契约 + `iree_shim.{h,cpp}` **骨架** + vendoring
  脚本 + `IreeNative` 门面 + 降级链（`iree_unavailable_e2e` 离线硬绿）。降级链**不
  加载真库**——只验证 `.ani` `load="auto"` 门闩语义（`Native.IsAvailable("iree") == false`）。
- **M-I1**：`iree_create_runtime` / `iree_load_module` / `iree_invoke` / `iree_invoke_arg_count`
  真体（CPU 最小闭环）。**Arc std 表面已就位**（`IreeSession` 实现 `IAIModel` +
  `IreeBufferView` + `IreeModelFactory.Create`，经 `iree.ani` 契约调用 shim；`iree_infer_e2e`
  定义）。shim 真体 + 真库 vendored（`fetch/build-iree-native.ps1`）后解除 e2e 软跳过。
- **M-I2**：`iree_create_buffer_*` / `iree_buffer_view_read_*` 类型化读写（张量完备）。
- **M-I3**：`iree_device_driver_available` 多后端探测 + 确定性回退 `local-task`。
- **M-I4**：接共享抽象（`Tensor` / `IAIModel`）。

## 运行时布局与加载

`iree.ani` 为 `load="auto"` + `library = Environment.GetEnvironmentVariable("ARC_IREE_LIB")`：
编译期库可定位 → static；否则确定性降级 runtime。运行时 `ARC_IREE_LIB` 指向**目录**，
再追加平台库名（`iree_shim.dll` / `libiree_shim.so` / `libiree_shim.dylib`）懒加载。

`iree_shim.dll` 依赖 IREE runtime DLL（如 `iree.dll` 及 `iree_compiler` 组件），故相关
DLL 须与 shim 同目录：

```
$env:ARC_IREE_LIB = "e:\GitCode\RF\dlang\target\iree-native"
```

未设置 / 目录缺库 / 符号缺失 → 模块 `unavailable`（`Native.IsAvailable("iree") == false`），
调用点优雅降级（[IreeNative.as](../../std/AI/Iree/IreeNative.as) 门闩 + `IreeNotAvailableException`）。

## 版本锁定

| 项 | 值 |
|----|-----|
| 分发 | IREE runtime release（`iree.org` / GitHub `iree-org/iree` releases 或 NuGet `IREE` 包） |
| 默认版本 | 待定（`-Version` 改；对齐 fetch-onnx-native 参数） |
| 哈希证据 | 首次 `fetch-iree-native.ps1` 计算并写入 `target/iree-native/SHA256.txt` |

> **宣称纪律（RFC 025 §1.1）**：脚本自动记录 SHA256 到 `SHA256.txt`，但**正式版本锁定
> 前须与上游 release 公告人工核验**，核验通过后回填上表/下文，未核验不得宣称"已固定"。
> 版本升级 = 独立 PR（单目标）；升级后重跑两脚本并更新本表。

## 依赖形态对照

| 库 | 形态 | vendored? | 桥接 | 加载 |
|----|------|-----------|------|------|
| zxing-cpp | 外部共享库 | 否（`target/zxing-native/`） | `shim/zxing_shim.cpp` | `ARC_ZXING_LIB` |
| ONNX Runtime | 外部 DLL + import lib | 否（`target/onnx-native/`） | `onnx_shim.cpp` | `ARC_ONNX_LIB` |
| **IREE Runtime** | **外部 DLL + import lib** | **否（`target/iree-native/`）** | **`iree_shim.cpp`** | **`ARC_IREE_LIB`** |
| mbedTLS | 源码直编 | 是（`crates/runtime-crypto/bin/`） | `rt_crypto_native.c` | 静态 |

## 许可

IREE 为 **Apache-2.0 WITH LLVM-exception**。`fetch` 脚本在可定位时拷贝包内
`LICENSE*` 到 `target/iree-native/LICENSE`（运行时工件不随仓库分发，不额外登记
NOTICE；如改为 vendored 则须登记）。
