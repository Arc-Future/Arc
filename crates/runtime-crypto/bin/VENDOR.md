# RFC 035 §1.4：vendored C 密码学底座（wgpu 模式）· vendoring 目录

本目录存放 **S0 TLS 1.3 最小子集**的 vendored C 密码学底座预编译产物，
供 M0 验收加载探针动态加载（原 `crypto_vendor_loaded_e2e` 已随
arc-integration 退场，a2627a0f），并为 M1–M3 的
`rt_crypto_*` ABI 落地提供底层实现。

## 底座选型（RFC 035 §1.1 定稿 · 本 S0 实例）

| 项 | 取值 | 说明 |
|----|------|------|
| 首选 | BoringSSL（/ AWS-LC 同面替代） | 无官方 Windows 预构建；源码构建需 cmake/perl/go（本环境不可用）→ **S0 走备选** |
| **实际底座** | **mbedTLS 4.1.1**（4.1 LTS 分支 · Apache-2.0） | RFC 035 §1.1 明确允许的 S0-only 备选（「clang 直编最省事」）；M1+ 若上游预构建就绪可整基换回 BoringSSL/AWS-LC |
| 接入路径 | wgpu 模式（预构建 + shim + .ani 契约） | 对齐 `crates/runtime-ui/wgpu-native` 先例；mbedTLS 以「clang 直编」产出预构建（sqlite 直编路径） |
| 加载模型 | 动态加载（M0 加载探针经 `LoadLibrary`/`dlopen` 解析符号） | vendored 加载后置对接 RFC 034 `load` 模型（本 M0 **不触碰** Track H 文件） |

## 供应链锁定（RFC 035 §1.4）

- 上游：mbedTLS 4.1.1 LTS（https://github.com/Mbed-TLS/mbedtls/releases/tag/mbedtls-4.1.1）
- 源码包：`mbedtls-4.1.1.tar.bz2`（GitHub release asset）
- SHA256：`3359a349e23db3d5536fcee032ae7b2ecbfc08972fab643089b5cbf2a375c98c`
- 获取方式：`scripts/fetch-boringssl-native.ps1`（下载 → SHA256 校验 → clang 直编 → 落 `bin/windows/`）
- 许可证：Apache-2.0（Arc 兼容）；署名见 `crates/runtime-crypto/NOTICE`

## 子目录结构（按平台）

```
crates/runtime-crypto/
├── bin/
│   ├── VENDOR.md                 # 本文件
│   └── windows/                  # Windows x86_64
│       ├── crypto_native.dll     # 运行时 DLL（M0 动态加载；M1+ 供 codegen 链接后自动复制）
│       ├── crypto_native.lib     # COFF 导入库（clang/MSVC 链接用）
│       └── libcrypto_native.dll.a# MinGW 导入库（Clang MinGW 惯例，备用）
├── NOTICE                        # 许可证署名（上游依赖列表）
└── shim/
    └── openssl_compat.c          # M0 探针 shim（导出三枚核心符号；真实语义由 M1–M3 落地）
```

Linux/macOS 平台：M1+ 阶段按同一 wgpu 模式补齐（`.so`/`.dylib` + SONAME）。

## 手动 vendoring 步骤

1. `powershell -File .\scripts\fetch-boringssl-native.ps1`（推荐；幂等）
2. 或手动：下载 `mbedtls-4.1.1.tar.bz2` → 校验 SHA256 → clang 直编（见 fetch 脚本内部步骤）→ 拷贝三件产物到 `bin/windows/`

## 运行时要求

- Windows：`crypto_native.dll` 与加载方同目录，或经 `SetDllDirectory`/绝对路径加载
  （原 `crypto_vendor_loaded_e2e` 采用 bin 目录绝对路径加载；该测试已随
  arc-integration 退场，a2627a0f）。
- Linux/macOS：M1+ 阶段按平台惯例（`LD_LIBRARY_PATH` / `@rpath`）。

## M0 探针 shim 与真实底座的替换路径

- M0 仅需「库可加载 + 三枚核心符号（`EVP_aead_aes_256_gcm` / `SSL_CTX_new` /
  `X509_parse`）可解析」；`shim/openssl_compat.c` 以非 NULL 探针句柄导出这三枚符号
  （诚实边界：不实现 AEAD/TLS/X.509 语义，对齐 RFC 035 §0「零 TLS 已实现宣称」）。
- 底座整体更换为真实 BoringSSL/AWS-LC 预构建时：三枚符号由上游库原生导出，
  删除 `shim/` 并重跑 fetch 脚本即可（替换对加载方透明、供应商无关；原
  `crypto_vendor_loaded_e2e` 已随 arc-integration 退场，a2627a0f）。
