# WasmHello（RFC 031 M-W3 · Draft）

**实验性** wasm32 垂直切片：在 **显式** `--experimental-wasm-emit` 下验证 `arc build` 能产出 LLVM IR（`obj/.../out.ll`）与 `.wasm` 工件。

## 诚实边界（Draft）

| 已有 | 未有 |
|------|------|
| `--experimental-wasm-emit` + target-gated 最小 runtime（`rt_wasm_min.c`） | 浏览器可运行 |
| 无 `platform.o` / 全量 `rt_ui_*` / wgpu-web 链接 | DOM / canvas / WebGPU 胶水 |
| M-W1a 门禁保留：无 flag 时 `--target wasm32-*` / `web` **硬错误** | Playwright / headless 截图 e2e |

**禁止**将此示例宣传为「Web Stable」或「Arc.UI 已支持 WASM」。

## 构建

```bash
# M-W1a：无 flag → 硬错误（预期）
cargo run -p arc -- build examples/WasmHello/Program.as --target wasm32-unknown-unknown

# M-W3 Draft：显式实验 flag
cargo run -p arc -- build examples/WasmHello/Program.as \
  --target wasm32-unknown-unknown --experimental-wasm-emit

# 或
cargo run -p arc -- build examples/WasmHello --target web --experimental-wasm-emit
```

产物默认：`examples/WasmHello/bin/Debug/Program.wasm`（中间 `out.ll` 在 `obj/Debug/Program/out.ll`）。

## 验证

```bash
# 原 wasm32_gate / wasm32_hello e2e 已随 arc-integration 退场（a2627a0f），
# 未迁入 arc-tests；wasm 产物面以 `arc build examples/WasmHello` 为准。
```
