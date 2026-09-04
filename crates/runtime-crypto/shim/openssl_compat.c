/* openssl_compat.c — vendored 密码学底座 OpenSSL/BoringSSL 兼容探针面（RFC 026 M0）
 *
 * 背景：RFC 026 §1.1 选定「vendored C 密码学底座」接入路径为 wgpu 模式
 * （预构建 + shim + .ani 契约）。本文件是 M0 阶段的 shim：把 RFC 026 §2 M0
 * 验收要求的三枚 BoringSSL 族核心符号（EVP_aead_aes_256_gcm / SSL_CTX_new /
 * X509_parse）导出到 vendored DLL，供 M0 加载探针验证
 * 「库可加载 + 核心符号可解析」（原 `crypto_vendor_loaded_e2e` 已随
 * arc-integration 退场，a2627a0f）。
 *
 * M0 诚实边界（对齐 RFC 026 §0「本 RFC 不包含任何 TLS 已实现宣称」）：
 *   - 本 shim 仅提供「符号可解析」探针表面，不实现 AEAD/TLS/X.509 语义；
 *   - 真实语义由后续里程碑在 vendored 底座上落地：
 *       M1 → rt_crypto_aead / rt_crypto_rsa / rt_crypto_x25519（星号后缀）
 *       M2 → rt_crypto_x509（星号后缀）
 *       M3 → rt_crypto_tls（星号后缀）
 *   - 底座更换为真实 BoringSSL/AWS-LC 预构建时，本 shim 可整体移除
 *     （那三枚符号由上游库原生导出），e2e 用例保持有效（供应商无关）。
 *
 * 许可证：Apache-2.0（Arc 兼容；与 mbedTLS/BoringSSL/AWS-LC 一致）。
 */

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#  define CRYPTO_PROBE_EXPORT __declspec(dllexport)
#else
#  define CRYPTO_PROBE_EXPORT __attribute__((visibility("default")))
#endif

/* BoringSSL 族不透明句柄类型（M0 仅需可解析，句柄布局不对外承诺）。 */
struct evp_aead_st { const char* name; };   /* EVP_AEAD */
struct ssl_ctx_st { const char* label; };   /* SSL_CTX */
struct x509_st { const char* label; };      /* X509 */

static const struct evp_aead_st kAeadAes256Gcm = { "aes-256-gcm" };

/* EVP_AEAD AES-256-GCM 描述符。BoringSSL 中返回 const EVP_AEAD*（静态描述符）。 */
CRYPTO_PROBE_EXPORT const struct evp_aead_st* EVP_aead_aes_256_gcm(void) {
    return &kAeadAes256Gcm;
}

/* SSL_CTX_new：M0 探针返回非 NULL 不透明句柄。
 * 真实 TLS 1.3 会话语义由 M3（rt_crypto_tls_*）在底座上落地。 */
CRYPTO_PROBE_EXPORT struct ssl_ctx_st* SSL_CTX_new(void) {
    static struct ssl_ctx_st ctx = { "ssl-ctx" };
    return &ctx;
}

/* X509_parse：M0 探针返回非 NULL 不透明句柄。
 * 真实 DER/PEM 解析 + 信任锚验签语义由 M2（rt_crypto_x509_*）落地。 */
CRYPTO_PROBE_EXPORT struct x509_st* X509_parse(const uint8_t* der, size_t len) {
    (void)der;
    (void)len;
    static struct x509_st x = { "x509" };
    return &x;
}
