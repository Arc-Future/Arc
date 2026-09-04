# RFC 026 加密与安全

## 背景

密码学能力以 `std/Security`（`Arc.Security`）显式依赖子库交付。设计目标：强类型 facade 对齐 C# `System.Security.Cryptography`、单一惯用法、X.509 最小校验（TLS 会话层归 `Arc.Net.Security`，见 [025](025-networking.md)）。密码学底座经 vendored C 库接入（原语/TLS/X.509），禁止纯 Arc 手写密码学握手；用户面无 `unsafe`/指针。

## 设计决策

### Hash / HMAC / CSPRNG（`Arc.Security`）

| 面 | 类型 | 说明 |
|----|------|------|
| 摘要 | `SHA256`/`SHA512`/`SHA1`/`MD5`/`SHA3_256`/`SHA3_512` | `[Builtin]` 真实实现；NIST 向量验证 |
| 键控摘要 | `HMACSHA256`/`HMACSHA384`/`HMACSHA512` | RFC 4231 向量 |
| 随机 | `CSPRNG` | 密码学安全随机（对标 C# `RandomNumberGenerator` 角色）；非 CSPRNG 伪随机用 `Arc.Types.Random`（LCG） |

hex/base64 输出面走 `Arc.Text` 单一惯用法（`Arc.Text.Base64`/`Arc.Text.Hex`），不在安全域或 `Convert` 双轨。

### Arc.Security.Cryptography 强类型 facade

参照 C# `System.Security.Cryptography` 现代精华，取精华去糟粕、单一惯用法；经 `rt_crypto_*` ABI 直射 vendored 底座。

| 类型 | 成员 | 说明 |
|------|------|------|
| `AesGcm` | `Create()`/`Create(byte[] key)`/`Key`/`TagSize`/`Encrypt(nonce, plaintext)`/`Decrypt(nonce, ciphertext, tag)` | 仅 AES-256-GCM；nonce 固定 12 字节；**非就地**返回新 `byte[]`；tag 与密文分离返回（避免隐式内嵌布局） |
| `Rsa` | `Create()`/`ExportSubjectPublicKeyInfo`/`ExportPkcs8PrivateKey`/`ImportSubjectPublicKeyInfo`/`Sign`/`Verify` | RSASSA-PSS-SHA256；2048-bit；不引入 PKCS#1 v1.5 糟粕 |
| `ECDiffieHellman` | `Create()`/`PublicKey`/`DeriveKeyMaterial(other)`/`ImportPrivateKey` | X25519 固定曲线；返回原始共享秘密；HKDF 用户面不暴露 |
| `X509Certificate2` | `CreateFromPem`/`CreateFromDer`/`Subject`/`RawData`/`PublicKey` | 解析 + 信任锚等值 + 公钥验签 |

```as
using Arc.Security.Cryptography;

var alice = ECDiffieHellman.Create();
var bob   = ECDiffieHellman.Create();
var s1 = alice.DeriveKeyMaterial(bob.PublicKey);
var s2 = bob.DeriveKeyMaterial(alice.PublicKey);   // s1 == s2
```

**设计决策**：

- **证书校验最小**：信任锚 + 自签验签；完整证书链构建/吊销/OCSP 不在本设计面内。
- 0-RTT、会话恢复（PSK）不在本设计面内。
- 仅 AES-GCM 单 AEAD；不引入 CBC 等不推荐模式（去糟粕）。
- TLS 会话面唯一归位 `Arc.Net.Security`（RFC 025 P0 归属裁决，对标 `System.Net.Security` 归 Net）；`std/Security` 只含密码学原语与 X.509，同一会话面不得两处暴露（不双轨）。

### ABI 与底座

- 新 ABI 延续 `rt_crypto_*` 前缀（`rt_crypto_aesgcm_*`/`rt_crypto_rsa_*`/`rt_crypto_x25519_*`/`rt_crypto_x509_*`/`rt_crypto_tls_*`），不引入新前缀；逐项独立立宪 + 非 Skip e2e。
- vendored 底座收敛于 `crates/runtime-crypto/`（含 `VENDOR.md`/`NOTICE` 署名、版本锁定、获取脚本）；许可证须 Apache-2.0 兼容（wolfSSL/GPL 否决）。
- TLS 记录层/密钥调度在 `rt_crypto_tls_*` 内部完成；`byte[]` 工作载体经 `RtArrayHeader` 前置头，含内部 0x00 完整往返。
- 非 Skip e2e 以 RFC 8448/7748 测试向量 + 本地闭环验证（禁实网依赖、禁 stub 冒充）。

## 边界

- 本文档讲密码学/哈希/X.509；TLS 会话层（`Arc.Net.Security`）与网络传输、HTTP over TLS、WebSocket wss 见 [025 网络协议层](025-networking.md)。
- 伪随机（非安全）见 [021 集合、IO 与文本](021-collections-io-text.md)（`Arc.Types.Random`）。
- 二进制/文本编码见 [021](021-collections-io-text.md)。

---

上一节：[025 网络协议层](025-networking.md) · 下一节：[027 本地化与资源](027-localization-resources.md)