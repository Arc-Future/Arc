# RFC 033 S4：vendored C QUIC 底座（ngtcp2 + OpenSSL QUIC-TLS）· vendoring 目录

本目录存放 **HTTP/3（QUIC）客户端最小实现**（RFC 033 §2.6 · S4）的 vendored
C QUIC 底座预编译产物，供 S4 验收加载探针动态加载（原 `quic_tls13_e2e` /
`http3_quic_e2e` 已随 arc-integration 退场，a2627a0f），并为 `rt_quic_*`
ABI 落地提供底层实现。

## 底座选型（Phase 1 可行性调查 · 2026-08-05 定稿）

| 项 | 取值 | 说明 |
|----|------|------|
| 首选（RFC 033 §1.2.b） | ngtcp2 + BoringSSL `SSL_QUIC_METHOD` | BoringSSL/AWS-LC **无官方 Windows 预构建**；源码构建需 cmake/perl/go（本环境复核缺 cmake/perl/go，`winget`/`choco` 均不可用）→ 走第三方预构建路径 |
| **实际底座** | **ngtcp2 1.25.0（MIT）+ OpenSSL 3.5.4（Apache-2.0）QUIC-TLS 适配** | MSYS2 UCRT64 预构建（`libngtcp2-16.dll` + `libngtcp2_crypto_ossl-0.dll` + `libssl-3-x64.dll` + `libcrypto-3-x64.dll`）；`SSL_set_quic_tls_cbs` 的 `set_encryption_secrets` / `add_handshake_data` / `flush_flight` / `send_alert` 语义与 BoringSSL `SSL_QUIC_METHOD` 等价 |
| RFC 039 §1.1 冲突裁决 | **选项 a（双栈）** | S0 的 mbedTLS（无 QUIC）不动；S4 以独立 QUIC-TLS 底座（OpenSSL 仅作 ngtcp2 的 TLS backend，**OpenSSL 原生 QRL 全栈未使用**）并存。不做「b 整基换回」（会破坏已签收 S0） |
| 接入路径 | wgpu 模式（预构建 + shim + 动态加载） | 对齐 `crates/runtime-crypto/` 先例 |
| 加载模型 | 动态加载（加载探针经 `LoadLibrary`/`GetProcAddress` 解析 `rt_quic_*` 符号） | 与 runtime-crypto M0 探针同模式 |

## 供应链锁定（对齐 RFC 035 §1.4）

- ngtcp2 1.25.0-1：https://repo.msys2.org/mingw/ucrt64/mingw-w64-ucrt-x86_64-ngtcp2-1.25.0-1-any.pkg.tar.zst
  - SHA256：`CCE230D80A05CD0A1EC38FA5D76C0445C9BB725C65C2136BE3D23E6DC218A89E`
- OpenSSL 3.5.4-1：https://repo.msys2.org/mingw/ucrt64/mingw-w64-ucrt-x86_64-openssl-3.5.4-1-any.pkg.tar.zst
  - SHA256：`F124FDA279F00B6789D3FC4EEF564ECF37B3C44D31C712A7E77E3E822B498A06`
- 获取方式：`scripts/fetch-quic-native.ps1`（下载 → SHA256 校验 → 生成 MSVC 导入库 → clang 编 shim → 落 `bin/windows/`）
- 许可证：ngtcp2 = MIT；OpenSSL = Apache-2.0；署名见 `crates/runtime-quic/NOTICE`

## 子目录结构（按平台）

```
crates/runtime-quic/
├── bin/
│   ├── VENDOR.md                 # 本文件
│   └── windows/                  # Windows x86_64
│       ├── quic_native.dll       # 运行时 DLL（rt_quic_* ABI 面；e2e 动态加载）
│       ├── quic_native.lib       # COFF 导入库（clang 链接自动生成，备用）
│       ├── libngtcp2-16.dll      # ngtcp2（RFC 9000 传输层）
│       ├── libngtcp2_crypto_ossl-0.dll  # ngtcp2 OpenSSL QUIC-TLS 适配（RFC 9001）
│       ├── libssl-3-x64.dll      # OpenSSL 3.5（TLS backend）
│       └── libcrypto-3-x64.dll   # OpenSSL 3.5（密码学原语）
├── NOTICE                        # 许可证署名（上游依赖列表）
└── shim/
    └── rt_quic_native.c          # rt_quic_* ABI 面（ngtcp2 封装 + TLS-over-QUIC 适配）
```

Linux/macOS：后续按同一 wgpu 模式补齐（`.so`/`.dylib`）。

## 手动 vendoring 步骤

1. `powershell -File .\scripts\fetch-quic-native.ps1`（推荐；幂等）
2. 或手动：下载两个 MSYS2 包 → 校验 SHA256 → 生成 MSVC 导入库 → clang 编
   `shim/rt_quic_native.c` → 拷贝 DLL 到 `bin/windows/`（步骤见 fetch 脚本）。

## 运行时要求

- Windows：`quic_native.dll` 及其四个依赖 DLL 同目录，或经
  `SetDllDirectory`/绝对路径加载（e2e 采用 bin 目录绝对路径加载依赖再加载主库）。
- Linux/macOS：后续按平台惯例补齐。

## 诚实边界（S4 · 2026-08-05）

- **已交付**：QUIC v1 连接建立（Initial/Handshake/1-RTT 全加密等级）+ TLS 1.3
  握手闭环 + 双向 STREAM 数据交换（本地进程内中继，非实网）。
- **后置**：0-RTT / 连接迁移 / QPACK 动态表 / 拥塞控制完整 / 服务器端产品化 /
  实网互操作（诚实边界：**本地闭环绿 ≠ 实网兼容**，禁止宣称 HTTP/3 已实现）。
- OpenSSL 仅作 ngtcp2 的 TLS backend；本底座**不含** OpenSSL QRL（原生 QUIC 栈）。
- e2e 局部范围使用自签证书并关闭对端证书校验（`SSL_VERIFY_NONE`），仅限本地测试。
