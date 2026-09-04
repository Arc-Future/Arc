/* rt_crypto_native.c — vendored 密码学底座 `rt_crypto_*` ABI 实现面（RFC 026 M1）
 *
 * M0 交付的 `crypto_native.dll` 仅含三枚 BoringSSL 族探针符号（openssl_compat.c）。
 * 本文件为 M1 语义落地：在 vendored mbedTLS 4.1.1 底座上用真实 API 实现 RFC 026
 * §1.3 的 AEAD / RSA / X25519 ABI 行（以 byte[] 原始字节形态，R1 已验证编译器
 * `byte[]` Builtin 参数支持，§1.3 形态注记允许升级；免 hex 往返、零拷贝）。
 *
 * 形态约定（与 `crates/runtime/rt_array.c` 对齐）：
 *   - `byte[]` 实参/返回值 = RtArrayHeader{int32 length; int32 elem_size} 之后的
 *     payload 指针（header 位于 payload-8）；
 *   - 返回 `byte[]` 用 `arr_create`（malloc header+payload，调用方经 ARC 释放）；
 *   - RSA opaque 句柄 = `mbedtls_pk_context*`（64 位指针直传，防 int32 截断）；
 *   - X25519 opaque 句柄 = PSA key id（int32 存储；M1 不做销毁 ABI，诚实泄漏）；
 *   - 失败返回 NULL / 0；成功返回新分配的 byte[] / 非零句柄。
 *
 * ABI 命名：RFC 026 §1.3 表中 `rt_crypto_aead_encrypt/decrypt` 与 RFC 042 P2P
 * 在途 ABI 同名（crates/runtime/rt_chacha20_poly1305.c），M1 采用区分名
 * `rt_crypto_aesgcm_*`；X25519 行同理避免与 `rt_crypto_x25519_dh`（P2P）冲突。
 *
 * 许可证：Apache-2.0（Arc 兼容；与 mbedTLS/BoringSSL/AWS-LC 一致）。
 */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <psa/crypto.h>
#include <mbedtls/md.h>
#include <mbedtls/pk.h>
#include <mbedtls/ssl.h>
#include <mbedtls/ssl_ticket.h>
#include <mbedtls/x509.h>
#include <mbedtls/x509_crt.h>
#include <mbedtls/x509_crl.h>

#if defined(_WIN32)
#  include <windows.h>
#  include <wincrypt.h>
#  define CRYPTO_ABI_EXPORT __declspec(dllexport)
#else
#  define CRYPTO_ABI_EXPORT __attribute__((visibility("default")))
#endif

/* ---- Arc `byte[]` 载体（对齐 rt_array.c 布局） ---- */

static int32_t arr_len(const uint8_t* p) {
    if (!p) return 0;
    return ((const int32_t*)p)[-2];
}

static uint8_t* arr_create(int32_t cap) {
    /* {int32 length; int32 elem_size} + payload */
    size_t bytes = 8u + (size_t)cap;
    uint8_t* h = (uint8_t*)malloc(bytes);
    if (!h) return NULL;
    ((int32_t*)h)[0] = cap;
    ((int32_t*)h)[1] = 1; /* elem_size = byte */
    return h + 8;
}

static uint8_t* arr_from_bytes(const void* data, int32_t len) {
    if (len < 0) return NULL;
    uint8_t* out = arr_create(len);
    if (!out) return NULL;
    if (len > 0 && data) {
        memcpy(out, data, (size_t)len);
    }
    return out;
}

/* ---- PSA 全局初始化（幂等） ---- */

static int g_psa_ready = 0;

static void ensure_psa(void) {
    if (!g_psa_ready) {
        if (psa_crypto_init() == PSA_SUCCESS) {
            g_psa_ready = 1;
        }
    }
}

/* ---- AES-256-GCM（rt_crypto_aesgcm_*；RFC 026 §1.2 ①） ---- */

/* 生成 32 字节随机 AES-256 密钥（Create() 无参面）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_aesgcm_new_key(void) {
    ensure_psa();
    uint8_t key[32];
    size_t len = 0;
    if (psa_generate_random(key, sizeof(key)) != PSA_SUCCESS) {
        return NULL;
    }
    return arr_from_bytes(key, 32);
}

/* Encrypt → 返回 byte[] = ciphertext || tag（16 字节 tag 附尾的封装形态，
 * RFC 026 §1.2 ①「含附加 tag 的封装形态」）；失败返回 NULL。
 * key 支持 16/24/32 字节（PSA AES 均可）；facade 公开面限定 32 字节。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_aesgcm_encrypt(
    const uint8_t* key, const uint8_t* nonce, const uint8_t* plain) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t nonce_len = arr_len(nonce);
    int32_t plain_len = arr_len(plain);
    if (key_len < 16 || nonce_len != 12 || plain_len < 0) return NULL;
    if (plain_len == 0 && !plain) plain = (const uint8_t*)"";

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_ENCRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) != PSA_SUCCESS) {
        return NULL;
    }

    size_t out_cap = (size_t)plain_len + 16u; /* ct || tag */
    uint8_t* out = (uint8_t*)malloc(out_cap);
    size_t out_len = 0;
    psa_status_t st = psa_aead_encrypt(
        key_id, PSA_ALG_GCM, nonce, (size_t)nonce_len, NULL, 0,
        plain, (size_t)plain_len, out, out_cap, &out_len);
    psa_destroy_key(key_id);
    if (st != PSA_SUCCESS) {
        free(out);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(out, (int32_t)out_len);
    free(out);
    return arr;
}

/* Decrypt → 返回 byte[] 明文；认证失败（篡改 tag/密文）返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_aesgcm_decrypt(
    const uint8_t* key, const uint8_t* nonce, const uint8_t* cipher,
    const uint8_t* tag) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t nonce_len = arr_len(nonce);
    int32_t ct_len = arr_len(cipher);
    int32_t tag_len = arr_len(tag);
    if (key_len < 16 || nonce_len != 12 || ct_len < 0 || tag_len != 16) return NULL;
    if (ct_len == 0 && !cipher) cipher = (const uint8_t*)"";

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_DECRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) != PSA_SUCCESS) {
        return NULL;
    }

    size_t in_cap = (size_t)ct_len + 16u;
    uint8_t* in = (uint8_t*)malloc(in_cap);
    if (!in) {
        psa_destroy_key(key_id);
        return NULL;
    }
    if (ct_len > 0) memcpy(in, cipher, (size_t)ct_len);
    if (tag) memcpy(in + ct_len, tag, 16u);

    uint8_t* plain = (uint8_t*)malloc((size_t)ct_len);
    size_t plain_len = 0;
    psa_status_t st = psa_aead_decrypt(
        key_id, PSA_ALG_GCM, nonce, (size_t)nonce_len, NULL, 0,
        in, in_cap, plain, (size_t)ct_len, &plain_len);
    psa_destroy_key(key_id);
    free(in);
    if (st != PSA_SUCCESS) {
        free(plain);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(plain, (int32_t)plain_len);
    free(plain);
    return arr;
}

/* ---- 记录层 AEAD（rt_crypto_aesgcm_*_aad；RFC 026 §1.2 ① AAD 后置） ----
 *
 * §1.2 ①：AAD（associated data）用户面不暴露，TLS 记录层 AAD 在
 * `rt_crypto_tls_*` 内部。本对 ABI 为记录层原语（RFC 8448 §3 应用数据
 * 向量直测 + M3 记录层复用），独立立宪、不改既有 `rt_crypto_aesgcm_*`
 * 签名/语义。 */

/* Encrypt with AAD → 返回 byte[] = ciphertext || tag；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_aesgcm_encrypt_aad(
    const uint8_t* key, const uint8_t* nonce, const uint8_t* plain,
    const uint8_t* aad) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t nonce_len = arr_len(nonce);
    int32_t plain_len = arr_len(plain);
    int32_t aad_len = arr_len(aad);
    if (key_len < 16 || nonce_len != 12 || plain_len < 0 || aad_len < 0) return NULL;
    if (plain_len == 0 && !plain) plain = (const uint8_t*)"";
    if (aad_len == 0 && !aad) aad = (const uint8_t*)"";

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_ENCRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) != PSA_SUCCESS) {
        return NULL;
    }

    size_t out_cap = (size_t)plain_len + 16u; /* ct || tag */
    uint8_t* out = (uint8_t*)malloc(out_cap);
    size_t out_len = 0;
    psa_status_t st = psa_aead_encrypt(
        key_id, PSA_ALG_GCM, nonce, (size_t)nonce_len, aad, (size_t)aad_len,
        plain, (size_t)plain_len, out, out_cap, &out_len);
    psa_destroy_key(key_id);
    if (st != PSA_SUCCESS) {
        free(out);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(out, (int32_t)out_len);
    free(out);
    return arr;
}

/* Decrypt with AAD → 返回 byte[] 明文；认证失败（篡改 tag/密文/AAD）返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_aesgcm_decrypt_aad(
    const uint8_t* key, const uint8_t* nonce, const uint8_t* cipher,
    const uint8_t* tag, const uint8_t* aad) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t nonce_len = arr_len(nonce);
    int32_t ct_len = arr_len(cipher);
    int32_t tag_len = arr_len(tag);
    int32_t aad_len = arr_len(aad);
    if (key_len < 16 || nonce_len != 12 || ct_len < 0 || tag_len != 16 || aad_len < 0) {
        return NULL;
    }
    if (ct_len == 0 && !cipher) cipher = (const uint8_t*)"";
    if (aad_len == 0 && !aad) aad = (const uint8_t*)"";

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_DECRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) != PSA_SUCCESS) {
        return NULL;
    }

    size_t in_cap = (size_t)ct_len + 16u;
    uint8_t* in = (uint8_t*)malloc(in_cap);
    if (!in) {
        psa_destroy_key(key_id);
        return NULL;
    }
    if (ct_len > 0) memcpy(in, cipher, (size_t)ct_len);
    if (tag) memcpy(in + ct_len, tag, 16u);

    uint8_t* plain = (uint8_t*)malloc((size_t)ct_len);
    size_t plain_len = 0;
    psa_status_t st = psa_aead_decrypt(
        key_id, PSA_ALG_GCM, nonce, (size_t)nonce_len, aad, (size_t)aad_len,
        in, in_cap, plain, (size_t)ct_len, &plain_len);
    psa_destroy_key(key_id);
    free(in);
    if (st != PSA_SUCCESS) {
        free(plain);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(plain, (int32_t)plain_len);
    free(plain);
    return arr;
}

/* ---- RSA（rt_crypto_rsa_*；RFC 026 §1.2 ② · RSASSA-PSS-SHA256） ---- */

/* Keygen → opaque `mbedtls_pk_context*` 句柄（64 位指针直传，避免 int32 截断）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_rsa_keygen(int32_t bits) {
    ensure_psa();
    if (bits < 512 || bits > 8192) return NULL;

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_RSA_KEY_PAIR);
    psa_set_key_bits(&attrs, (size_t)bits);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_SIGN_HASH | PSA_KEY_USAGE_EXPORT);
    psa_set_key_algorithm(&attrs, PSA_ALG_RSA_PSS(PSA_ALG_SHA_256));
    psa_key_id_t key_id = 0;
    if (psa_generate_key(&attrs, &key_id) != PSA_SUCCESS) return NULL;

    /* PSA 导出 PKCS#1 RSAPrivateKey DER → 经 PK 层载入（PKCS#8/SPKI 导出、
     * PSS 签名统一走 mbedtls_pk_*，避免 PSA RSA 导出格式仅 PKCS#1 的限制）。 */
    uint8_t der[8192];
    size_t der_len = 0;
    psa_status_t st = psa_export_key(key_id, der, sizeof(der), &der_len);
    psa_destroy_key(key_id);
    if (st != PSA_SUCCESS) return NULL;

    mbedtls_pk_context* pk = (mbedtls_pk_context*)malloc(sizeof(mbedtls_pk_context));
    if (!pk) return NULL;
    mbedtls_pk_init(pk);
    if (mbedtls_pk_parse_key(pk, der, der_len, NULL, 0) != 0) {
        mbedtls_pk_free(pk);
        free(pk);
        return NULL;
    }
    return (uint8_t*)pk;
}

static mbedtls_pk_context* rsa_from_handle(const uint8_t* handle) {
    if (!handle) return NULL;
    return (mbedtls_pk_context*)(void*)handle;
}

/* SPKI DER 导出（SubjectPublicKeyInfo）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_rsa_spki_export(const uint8_t* handle) {
    mbedtls_pk_context* pk = rsa_from_handle(handle);
    if (!pk) return NULL;
    uint8_t buf[4096];
    int len = mbedtls_pk_write_pubkey_der(pk, buf, sizeof(buf));
    if (len <= 0) return NULL;
    return arr_from_bytes(buf + sizeof(buf) - (size_t)len, len);
}

/* PKCS#8 DER 导出（PrivateKeyInfo）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_rsa_pkcs8_export(const uint8_t* handle) {
    mbedtls_pk_context* pk = rsa_from_handle(handle);
    if (!pk) return NULL;
    uint8_t buf[8192];
    int len = mbedtls_pk_write_key_der(pk, buf, sizeof(buf));
    if (len <= 0) return NULL;
    return arr_from_bytes(buf + sizeof(buf) - (size_t)len, len);
}

/* SPKI DER 导入 → opaque 句柄（64 位指针）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_rsa_spki_import(const uint8_t* der) {
    int32_t len = arr_len(der);
    if (len <= 0 || !der) return NULL;
    mbedtls_pk_context* pk = (mbedtls_pk_context*)malloc(sizeof(mbedtls_pk_context));
    if (!pk) return NULL;
    mbedtls_pk_init(pk);
    if (mbedtls_pk_parse_public_key(pk, der, (size_t)len) != 0) {
        mbedtls_pk_free(pk);
        free(pk);
        return NULL;
    }
    return (uint8_t*)pk;
}

/* RSASSA-PSS-SHA256 签名 → byte[] 签名（2048-bit → 256 字节）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_rsa_sign_pss(const uint8_t* handle, const uint8_t* data) {
    mbedtls_pk_context* pk = rsa_from_handle(handle);
    if (!pk) return NULL;
    int32_t data_len = arr_len(data);
    if (data_len < 0) return NULL;
    if (data_len == 0 && !data) data = (const uint8_t*)"";

    uint8_t hash[32];
    size_t hash_len = 0;
    if (psa_hash_compute(PSA_ALG_SHA_256, data, (size_t)data_len,
                         hash, sizeof(hash), &hash_len) != PSA_SUCCESS) {
        return NULL;
    }
    uint8_t sig[512];
    size_t sig_len = 0;
    if (mbedtls_pk_sign_ext(MBEDTLS_PK_SIGALG_RSA_PSS, pk, MBEDTLS_MD_SHA256,
                            hash, hash_len, sig, sizeof(sig), &sig_len) != 0) {
        return NULL;
    }
    return arr_from_bytes(sig, (int32_t)sig_len);
}

/* RSASSA-PSS-SHA256 验签 → 0 成功 / 非零失败。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_rsa_verify_pss(
    const uint8_t* handle, const uint8_t* data, const uint8_t* sig) {
    mbedtls_pk_context* pk = rsa_from_handle(handle);
    if (!pk) return -1;
    int32_t data_len = arr_len(data);
    int32_t sig_len = arr_len(sig);
    if (data_len < 0 || sig_len <= 0 || !sig) return -1;
    if (data_len == 0 && !data) data = (const uint8_t*)"";

    uint8_t hash[32];
    size_t hash_len = 0;
    if (psa_hash_compute(PSA_ALG_SHA_256, data, (size_t)data_len,
                         hash, sizeof(hash), &hash_len) != PSA_SUCCESS) {
        return -1;
    }
    return mbedtls_pk_verify_ext(MBEDTLS_PK_SIGALG_RSA_PSS, pk, MBEDTLS_MD_SHA256,
                                 hash, hash_len, sig, (size_t)sig_len);
}

/* ---- X25519（rt_crypto_x25519_*；RFC 026 §1.2 ③） ---- */

/* Keygen → PSA key id 句柄（int32）；失败返回 0。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_x25519_keygen(void) {
    ensure_psa();
    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_ECC_KEY_PAIR(PSA_ECC_FAMILY_MONTGOMERY));
    psa_set_key_bits(&attrs, 255);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_EXPORT | PSA_KEY_USAGE_DERIVE);
    psa_set_key_algorithm(&attrs, PSA_ALG_ECDH);
    psa_key_id_t key_id = 0;
    if (psa_generate_key(&attrs, &key_id) != PSA_SUCCESS) return 0;
    return (int32_t)(uint32_t)key_id;
}

/* 32 字节公钥导出；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_x25519_pubkey(int32_t handle) {
    if (handle == 0) return NULL;
    psa_key_id_t key_id = (psa_key_id_t)(uint32_t)handle;
    uint8_t pub[32];
    size_t len = 0;
    if (psa_export_public_key(key_id, pub, sizeof(pub), &len) != PSA_SUCCESS) {
        return NULL;
    }
    return arr_from_bytes(pub, (int32_t)len);
}

/* 导入原始 32 字节 X25519 私钥（RFC 7748 §6.1 向量面）→ 句柄；失败返回 0。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_x25519_import_private(const uint8_t* priv) {
    ensure_psa();
    int32_t len = arr_len(priv);
    if (len != 32 || !priv) return 0;
    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_ECC_KEY_PAIR(PSA_ECC_FAMILY_MONTGOMERY));
    psa_set_key_bits(&attrs, 255);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_EXPORT | PSA_KEY_USAGE_DERIVE);
    psa_set_key_algorithm(&attrs, PSA_ALG_ECDH);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, priv, 32u, &key_id) != PSA_SUCCESS) return 0;
    return (int32_t)(uint32_t)key_id;
}

/* ECDH 共享秘密（32 字节原始共享秘密）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_x25519_derive(int32_t handle, const uint8_t* other_public) {
    if (handle == 0) return NULL;
    int32_t other_len = arr_len(other_public);
    if (other_len != 32 || !other_public) return NULL;
    psa_key_id_t key_id = (psa_key_id_t)(uint32_t)handle;
    uint8_t secret[32];
    size_t len = 0;
    psa_status_t st = psa_raw_key_agreement(PSA_ALG_ECDH, key_id,
                                            other_public, 32u,
                                            secret, sizeof(secret), &len);
    if (st != PSA_SUCCESS) return NULL;
    return arr_from_bytes(secret, (int32_t)len);
}

/* ---- TLS 1.3 记录层 + 握手密钥调度（rt_crypto_tls_*；RFC 026 M2） ----
 *
 * RFC 8446 §7.1 密钥调度 + §5.2 记录保护原语。本组 ABI 复用 M1 的
 * `rt_crypto_aesgcm_*_aad` 记录层原语（PSA AEAD + AAD = 记录头），新增
 * HKDF-Extract / HKDF-Expand-Label / Derive-Secret 与记录封装/解封。
 * 逐项独立立宪（RFC 036 §3.5），不改既有 `rt_crypto_*` 签名/语义。
 *
 * 形态约定（同 M1）：
 *   - `byte[]` 实参/返回值 = RtArrayHeader{int32 length; int32 elem_size} + payload；
 *   - label 以 byte[] 传入（ASCII），**不含** "tls13 " 前缀（实现内部拼接）；
 *   - `rt_crypto_tls_record_seal` 的 plaintext = 明文 fragment || inner content type
 *     （调用方附加内层类型字节，RFC 8446 §5.2）；返回 = 5 字节记录头 || ct || tag
 *     （外层类型恒 0x17 application_data，封装后记录）；
 *   - `rt_crypto_tls_record_open` 的 record = 完整记录（头 || ct || tag），返回
 *     plaintext（含 inner content type，由调用方校验）；认证失败返回 NULL；
 *   - 序列号：调用方管理递增（每密钥 epoch 独立），ABI 仅负责 nonce 派生
 *     （nonce = iv XOR seq，seq 8 字节大端零左填充，RFC 8446 §5.3）。
 */

static const mbedtls_md_info_t* tls_md(void) {
    return mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
}

/* HMAC-SHA256（RFC 2104）手工实现：mbedTLS 4.x 移除了 `mbedtls_md_hmac_*`，
 * 基于 `mbedtls_md_starts/update/finish` 搭建（HKDF 的 HMAC 基元）。 */
static int tls_hmac_sha256(const uint8_t* key, size_t key_len,
                           const uint8_t* msg, size_t msg_len,
                           uint8_t* out) {
    const mbedtls_md_info_t* md = tls_md();
    uint8_t k[64];
    size_t k_len = 0;
    uint8_t ipad[64];
    uint8_t opad[64];
    uint8_t inner[32];
    int rc;

    if (key_len > 64) {
        mbedtls_md_context_t ctx;
        mbedtls_md_init(&ctx);
        rc = mbedtls_md_setup(&ctx, md, 0);
        if (rc == 0) rc = mbedtls_md_starts(&ctx);
        if (rc == 0) rc = mbedtls_md_update(&ctx, key, key_len);
        if (rc == 0) rc = mbedtls_md_finish(&ctx, k);
        k_len = 32;
        mbedtls_md_free(&ctx);
        if (rc != 0) return -1;
    } else {
        if (key_len > 0) memcpy(k, key, key_len);
        k_len = key_len;
    }
    for (size_t i = 0; i < 64; i++) {
        uint8_t kb = (i < k_len) ? k[i] : 0;
        ipad[i] = kb ^ 0x36;
        opad[i] = kb ^ 0x5c;
    }

    mbedtls_md_context_t ctx;
    mbedtls_md_init(&ctx);
    rc = mbedtls_md_setup(&ctx, md, 0);
    if (rc == 0) rc = mbedtls_md_starts(&ctx);
    if (rc == 0) rc = mbedtls_md_update(&ctx, ipad, 64);
    if (rc == 0) rc = mbedtls_md_update(&ctx, msg, msg_len);
    if (rc == 0) rc = mbedtls_md_finish(&ctx, inner);
    mbedtls_md_free(&ctx);
    if (rc != 0) return -1;

    mbedtls_md_init(&ctx);
    rc = mbedtls_md_setup(&ctx, md, 0);
    if (rc == 0) rc = mbedtls_md_starts(&ctx);
    if (rc == 0) rc = mbedtls_md_update(&ctx, opad, 64);
    if (rc == 0) rc = mbedtls_md_update(&ctx, inner, 32);
    if (rc == 0) rc = mbedtls_md_finish(&ctx, out);
    mbedtls_md_free(&ctx);
    return rc;
}

/* HKDF-Extract（RFC 5869 §2.2）：PRK = HMAC-Hash(salt, IKM)；salt 空 → HashLen 全零。 */
static int tls_hkdf_extract(const uint8_t* salt, size_t salt_len,
                            const uint8_t* ikm, size_t ikm_len,
                            uint8_t* prk) {
    uint8_t zeros[32];
    memset(zeros, 0, sizeof(zeros));
    const uint8_t* s = (salt && salt_len > 0) ? salt : zeros;
    size_t sl = (salt && salt_len > 0) ? salt_len : sizeof(zeros);
    return tls_hmac_sha256(s, sl, ikm, ikm_len, prk);
}

/* HKDF-Expand（RFC 5869 §2.3）：T(i) = HMAC(PRK, T(i-1) || info || i)。 */
static int tls_hkdf_expand(const uint8_t* prk, size_t prk_len,
                           const uint8_t* info, size_t info_len,
                           uint8_t* okm, size_t okm_len) {
    if (prk_len != 32 || okm_len == 0 || okm_len > 255u * 32u) return -1;
    uint8_t prev[32];
    size_t prev_len = 0;
    size_t done = 0;
    uint8_t counter = 1;
    while (done < okm_len) {
        uint8_t block[32 + 255 + 1];
        size_t b = 0;
        if (prev_len > 0) { memcpy(block, prev, prev_len); b += prev_len; }
        if (info_len > 0) { memcpy(block + b, info, info_len); b += info_len; }
        block[b++] = counter;
        if (tls_hmac_sha256(prk, prk_len, block, b, prev) != 0) return -1;
        prev_len = 32;
        size_t take = okm_len - done;
        if (take > 32) take = 32;
        memcpy(okm + done, prev, take);
        done += take;
        counter++;
    }
    return 0;
}

/* HKDF-Expand-Label（RFC 8446 §7.1）：label 不带 "tls13 " 前缀（内部拼接）。 */
static int tls_hkdf_expand_label(const uint8_t* secret, size_t secret_len,
                                 const uint8_t* label, size_t label_len,
                                 const uint8_t* context, size_t context_len,
                                 uint8_t* okm, size_t okm_len) {
    static const char kPrefix[] = "tls13 ";
    if (label_len > 255 || context_len > 255 || okm_len == 0 || okm_len > 0xffffu) return -1;
    size_t total_label = sizeof(kPrefix) - 1 + label_len;
    if (total_label > 255) return -1;

    uint8_t info[2 + 1 + 255 + 1 + 255];
    size_t p = 0;
    info[p++] = (uint8_t)(okm_len >> 8);
    info[p++] = (uint8_t)(okm_len & 0xff);
    info[p++] = (uint8_t)total_label;
    memcpy(info + p, kPrefix, sizeof(kPrefix) - 1); p += sizeof(kPrefix) - 1;
    memcpy(info + p, label, label_len); p += label_len;
    info[p++] = (uint8_t)context_len;
    memcpy(info + p, context, context_len); p += context_len;
    return tls_hkdf_expand(secret, secret_len, info, p, okm, okm_len);
}

/* HKDF-Extract（SHA-256）→ byte[] PRK（32 字节）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_hkdf_extract(const uint8_t* salt, const uint8_t* ikm) {
    int32_t salt_len = arr_len(salt);
    int32_t ikm_len = arr_len(ikm);
    if (salt_len < 0 || ikm_len < 0) return NULL;
    if (ikm_len == 0 && !ikm) ikm = (const uint8_t*)"";
    uint8_t prk[32];
    if (tls_hkdf_extract(salt, (size_t)salt_len, ikm, (size_t)ikm_len, prk) != 0) return NULL;
    return arr_from_bytes(prk, 32);
}

/* HKDF-Expand-Label（SHA-256）→ byte[] OKM；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_hkdf_expand_label(
    const uint8_t* secret, const uint8_t* label, const uint8_t* context, int32_t out_len) {
    int32_t secret_len = arr_len(secret);
    int32_t label_len = arr_len(label);
    int32_t context_len = arr_len(context);
    if (secret_len < 0 || label_len <= 0 || context_len < 0 || out_len <= 0 || out_len > 8192) {
        return NULL;
    }
    if (context_len == 0 && !context) context = (const uint8_t*)"";
    uint8_t* out = (uint8_t*)malloc((size_t)out_len);
    if (!out) return NULL;
    if (tls_hkdf_expand_label(secret, (size_t)secret_len, label, (size_t)label_len,
                              context, (size_t)context_len, out, (size_t)out_len) != 0) {
        free(out);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(out, out_len);
    free(out);
    return arr;
}

/* Derive-Secret（RFC 8446 §7.1）：= HKDF-Expand-Label(Secret, Label, Hash(Messages), len)。
 * transcript_hash 为调用方预哈希的 32 字节 SHA-256 摘要 → byte[]（32 字节）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_derive_secret(
    const uint8_t* secret, const uint8_t* label, const uint8_t* transcript_hash) {
    int32_t secret_len = arr_len(secret);
    int32_t label_len = arr_len(label);
    int32_t th_len = arr_len(transcript_hash);
    if (secret_len < 0 || label_len <= 0 || th_len != 32) return NULL;
    uint8_t out[32];
    if (tls_hkdf_expand_label(secret, (size_t)secret_len, label, (size_t)label_len,
                              transcript_hash, (size_t)th_len, out, sizeof(out)) != 0) {
        return NULL;
    }
    return arr_from_bytes(out, 32);
}

/* nonce = iv XOR (0 0 0 0 seq_be_8)（RFC 8446 §5.3）。 */
static void tls_nonce_from_seq(const uint8_t* iv, int64_t seq, uint8_t* nonce) {
    uint64_t s = (uint64_t)seq;
    for (int i = 0; i < 12; i++) {
        uint8_t x = (i < 4) ? 0 : (uint8_t)(s >> (8u * (11u - (uint32_t)i)));
        nonce[i] = iv[i] ^ x;
    }
}

/* 记录封装：plaintext = fragment || inner content type（调用方附加）→
 * byte[] = 5 字节记录头 || ct || tag（外层类型恒 0x17）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_record_seal(
    const uint8_t* key, const uint8_t* iv, int64_t seq, const uint8_t* plaintext) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t iv_len = arr_len(iv);
    int32_t plain_len = arr_len(plaintext);
    if (key_len < 16 || iv_len != 12 || seq < 0 || plain_len <= 0) return NULL;
    if (plain_len > 0xffff - 16) return NULL; /* 记录长度字段上限 */

    uint8_t nonce[12];
    tls_nonce_from_seq(iv, seq, nonce);

    size_t rec_len = (size_t)plain_len + 16u;
    uint8_t header[5];
    header[0] = 0x17; /* application_data（封装后外层类型恒 0x17） */
    header[1] = 0x03;
    header[2] = 0x03; /* legacy_record_version（TLS 1.3 恒 0x0303） */
    header[3] = (uint8_t)(rec_len >> 8);
    header[4] = (uint8_t)(rec_len & 0xff);

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_ENCRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) != PSA_SUCCESS) {
        return NULL;
    }

    size_t out_cap = (size_t)plain_len + 16u;
    uint8_t* out = (uint8_t*)malloc(out_cap);
    size_t out_len = 0;
    psa_status_t st = psa_aead_encrypt(
        key_id, PSA_ALG_GCM, nonce, sizeof(nonce), header, sizeof(header),
        plaintext, (size_t)plain_len, out, out_cap, &out_len);
    psa_destroy_key(key_id);
    if (st != PSA_SUCCESS) {
        free(out);
        return NULL;
    }

    /* 组装完整记录：头 || ct || tag。 */
    uint8_t* rec = (uint8_t*)malloc(5u + out_len);
    if (!rec) {
        free(out);
        return NULL;
    }
    memcpy(rec, header, sizeof(header));
    memcpy(rec + 5, out, out_len);
    free(out);
    uint8_t* arr = arr_from_bytes(rec, (int32_t)(5u + out_len));
    free(rec);
    return arr;
}

/* 记录解封：record = 完整记录（头 || ct || tag）→ byte[] plaintext（含 inner
 * content type）；认证失败 / 长度字段不符返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_record_open(
    const uint8_t* key, const uint8_t* iv, int64_t seq, const uint8_t* record) {
    ensure_psa();
    int32_t key_len = arr_len(key);
    int32_t iv_len = arr_len(iv);
    int32_t rec_len = arr_len(record);
    if (key_len < 16 || iv_len != 12 || seq < 0) return NULL;
    if (rec_len < 5 + 1 + 16 || !record) return NULL;

    const uint8_t* header = record;
    size_t field = ((size_t)header[3] << 8) | header[4];
    if (field != (size_t)(rec_len - 5)) return NULL; /* 长度字段必须与实际密文||tag 一致 */
    size_t ct_len = (size_t)rec_len - 5u - 16u;

    uint8_t nonce[12];
    tls_nonce_from_seq(iv, seq, nonce);

    uint8_t* in = (uint8_t*)malloc(ct_len + 16u);
    if (!in) return NULL;
    memcpy(in, record + 5, ct_len);
    memcpy(in + ct_len, record + 5 + ct_len, 16u);

    uint8_t* plain = (uint8_t*)malloc(ct_len);
    size_t plain_len = 0;

    psa_key_attributes_t attrs = psa_key_attributes_init();
    psa_set_key_type(&attrs, PSA_KEY_TYPE_AES);
    psa_set_key_usage_flags(&attrs, PSA_KEY_USAGE_DECRYPT);
    psa_set_key_algorithm(&attrs, PSA_ALG_GCM);
    psa_key_id_t key_id = 0;
    psa_status_t st = PSA_ERROR_GENERIC_ERROR;
    if (psa_import_key(&attrs, key, (size_t)key_len, &key_id) == PSA_SUCCESS) {
        st = psa_aead_decrypt(
            key_id, PSA_ALG_GCM, nonce, sizeof(nonce), header, 5u,
            in, ct_len + 16u, plain, ct_len, &plain_len);
        psa_destroy_key(key_id);
    }
    free(in);
    if (st != PSA_SUCCESS) {
        free(plain);
        return NULL;
    }
    uint8_t* arr = arr_from_bytes(plain, (int32_t)plain_len);
    free(plain);
    return arr;
}

/* ---- X.509 证书解析（rt_crypto_x509_*；RFC 026 M3）
 *
 * RFC 026 §1.2 ④：S0 仅解析 DER/PEM 格式证书，提取 Subject 名称 + 公钥
 *（验签用）；完整链校验/有效期/主机名匹配后置。
 *
 * 不透明句柄 = `mbedtls_x509_crt*`（malloc'd，由 Arc 侧最终释放）。
 */

/* 从 DER 字节解析证书 → opaque `mbedtls_x509_crt*` 句柄；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_x509_parse_der(const uint8_t* der) {
    ensure_psa();
    int32_t der_len = arr_len(der);
    if (der_len <= 0 || !der) return NULL;

    mbedtls_x509_crt* crt = (mbedtls_x509_crt*)malloc(sizeof(mbedtls_x509_crt));
    if (!crt) return NULL;
    mbedtls_x509_crt_init(crt);

    int ret = mbedtls_x509_crt_parse_der(crt, der, (size_t)der_len);
    if (ret != 0) {
        mbedtls_x509_crt_free(crt);
        free(crt);
        return NULL;
    }

    return (uint8_t*)crt;
}

/* 从 PEM 字符串解析证书 → opaque 句柄；失败返回 NULL。
 * pem 为 NUL 结尾的 C 字符串（Arc `string` 即 UTF-8 C 字符串，直接透传）。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_x509_parse_pem(const char* pem) {
    ensure_psa();
    if (!pem || !*pem) return NULL;
    size_t pem_len = strlen(pem);

    mbedtls_x509_crt* crt = (mbedtls_x509_crt*)malloc(sizeof(mbedtls_x509_crt));
    if (!crt) return NULL;
    mbedtls_x509_crt_init(crt);

    /* mbedtls_x509_crt_parse 仅当 buf 以 '\0' 结尾（buf[buflen-1]=='\0'）才判定为
     * PEM 格式；故 buflen 须含终止 NUL（strlen+1），否则按 DER 解析必然失败。 */
    int ret = mbedtls_x509_crt_parse(crt, (const uint8_t*)pem, pem_len + 1);
    if (ret != 0) {
        mbedtls_x509_crt_free(crt);
        free(crt);
        return NULL;
    }

    return (uint8_t*)crt;
}

/* 提取证书 subject 名称 → 以 '\0' 结尾的 malloc'd C 字符串；失败返回 NULL。
 * 调用方：Arc 侧将 C 字符串转为 Arc string 后释放该缓冲区。
 * mbedTLS 4.x 无 `mbedtls_x509_crt_get_subject_name`，改用 `mbedtls_x509_dn_gets`
 * 直接格式化 `crt->subject` 名称链表。 */
CRYPTO_ABI_EXPORT char* rt_crypto_x509_subject(const uint8_t* handle) {
    if (!handle) return NULL;
    mbedtls_x509_crt* crt = (mbedtls_x509_crt*)(void*)handle;

    char buf[1024];
    int ret = mbedtls_x509_dn_gets(buf, sizeof(buf), &crt->subject);
    if (ret < 0) return NULL;

    size_t len = strlen(buf);
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    memcpy(out, buf, len + 1);
    return out;
}

/* 提取证书公钥 → opaque `mbedtls_pk_context*` 句柄（RSA 用）；失败返回 NULL。
 * 公钥句柄可直接用于 `rt_crypto_rsa_verify_pss` 验签（TLS 握手证书验签）。
 * mbedTLS 4.x 无 `mbedtls_pk_get_type`，改用 `mbedtls_pk_can_do_psa` 判定 RSA-PSS。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_x509_pubkey(const uint8_t* handle) {
    if (!handle) return NULL;
    mbedtls_x509_crt* crt = (mbedtls_x509_crt*)(void*)handle;

    mbedtls_pk_context* pk = &crt->pk;
    if (!mbedtls_pk_can_do_psa(pk, PSA_ALG_RSA_PSS(PSA_ALG_SHA_256),
                               PSA_KEY_USAGE_VERIFY_HASH)) {
        /* S0 仅支持 RSA 证书验签，非 RSA 返回 NULL（诚实边界） */
        return NULL;
    }

    return (uint8_t*)pk;
}

/* 验证证书链：leaf 是否由 trust 信任锚签发（RFC 026 §1.2 ④ 最小证书校验）。
 * 返回 0 = 有效；非零 = 无效（含签名不符/链不信任/有效期/CN 等）。
 * trust 为已在 Arc 侧解析的信任锚 `mbedtls_x509_crt*`（自签 CA 或自签端实体）。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_x509_verify(const uint8_t* leaf, const uint8_t* trust) {
    if (!leaf || !trust) return -1;
    mbedtls_x509_crt* l = (mbedtls_x509_crt*)(void*)leaf;
    mbedtls_x509_crt* t = (mbedtls_x509_crt*)(void*)trust;

    uint32_t flags = 0;
    int ret = mbedtls_x509_crt_verify(l, t, NULL, NULL, &flags, NULL, NULL);
    return ret == 0 ? 0 : 1;
}

/* 释放 X.509 证书句柄（由 Arc 侧 Dispose 时调用）。 */
CRYPTO_ABI_EXPORT void rt_crypto_x509_free(uint8_t* handle) {
    if (!handle) return;
    mbedtls_x509_crt* crt = (mbedtls_x509_crt*)(void*)handle;
    mbedtls_x509_crt_free(crt);
    free(crt);
}

/* ---- TLS 1.3 会话（rt_crypto_tls_*；RFC 026 M3 + S5） ----
 *
 * RFC 026 §1.2 ⑤：TlsClientSession 会话面——内存 BIO 非阻塞握手（mbedtls_ssl_set_bio
 * 挂双 FIFO 自定义 send/recv 回调）+ ALPN 协商。句柄 = malloc'd `tls_session*`，
 * 由 `rt_crypto_tls_free` 释放。M3 交付：客户端会话 + 信任锚最小校验；S5 在其上
 * 追加（逐项独立立宪、不改既有 ABI 签名/语义，见文件尾 S5 段）：完整链校验
 * （set_verify / set_crl / verify_result）、会话恢复（session_save/load）、0-RTT
 * 早数据（enable_early_data / write_early_data / early_data_status / read_early_data）、
 * 客户端证书（set_client_cert）、公开服务端 facade（server_new_ex + drain）。
 * 客户端会话票证自 S5 起启用（会话恢复前置），握手完成后可有 NewSessionTicket
 * post-handshake 消息（由对端接收）。
 */

typedef struct {
    uint8_t* data;
    size_t len;
    size_t cap;
    size_t rpos;
} tls_fifo;

typedef struct {
    mbedtls_ssl_context ssl;
    mbedtls_ssl_config conf;
    mbedtls_x509_crt cacert;     /* 客户端信任锚 / 服务端 client CA（链） */
    mbedtls_x509_crl crl;        /* CRL（吊销 · 最小面） */
    mbedtls_x509_crt own_cert;   /* 服务端证书 / 客户端证书 */
    mbedtls_pk_context own_key;  /* 服务端私钥 / 客户端私钥 */
    tls_fifo in;
    tls_fifo out;
    tls_fifo early_rx;           /* S5：服务端已吸收的 0-RTT 早数据（握手内 drain） */
    char* alpn_buf;
    const char* alpn_ptrs[8];
} tls_session;

/* S5 会话票证：进程级共享 ticket 上下文（跨连接复用——真实服务器票证密钥跨连接共享；
 * 每连接新建随机密钥则二次连接无法解密首连票证，会话恢复 e2e 无法闭环）。
 * 单线程顺序连接使用；并发接入需加锁（当前 e2e 形态不并发）。 */
static mbedtls_ssl_ticket_context g_ticket_ctx;
static int g_ticket_ctx_ready = 0;

static int ticket_ctx_global_init(void) {
    if (g_ticket_ctx_ready) return 0;
    mbedtls_ssl_ticket_init(&g_ticket_ctx);
    if (mbedtls_ssl_ticket_setup(&g_ticket_ctx, PSA_ALG_GCM, PSA_KEY_TYPE_AES,
                                 256u, 86400u) != 0) {
        return -1;
    }
    g_ticket_ctx_ready = 1;
    return 0;
}

static int fifo_reserve(tls_fifo* f, size_t extra) {
    if (f->len + extra <= f->cap) return 0;
    size_t ncap = f->cap ? f->cap : 256;
    while (ncap < f->len + extra) ncap *= 2;
    uint8_t* nd = (uint8_t*)realloc(f->data, ncap);
    if (!nd) return -1;
    f->data = nd;
    f->cap = ncap;
    return 0;
}

static size_t fifo_available(tls_fifo* f) {
    return f->len - f->rpos;
}

static int fifo_push(tls_fifo* f, const uint8_t* p, size_t n) {
    if (fifo_reserve(f, n) != 0) return -1;
    memcpy(f->data + f->len, p, n);
    f->len += n;
    return 0;
}

static size_t fifo_pop(tls_fifo* f, uint8_t* out, size_t n) {
    size_t avail = fifo_available(f);
    if (n > avail) n = avail;
    if (n > 0) {
        memcpy(out, f->data + f->rpos, n);
        f->rpos += n;
    }
    if (f->rpos == f->len) {
        f->rpos = 0;
        f->len = 0;
    }
    return n;
}

/* mbedTLS 内存 BIO 回调（双 FIFO；growable → 永不阻塞，握手由调用方喂取/取走）。 */
static int tls_send_cb(void* ctx, const unsigned char* buf, size_t len) {
    tls_session* s = (tls_session*)ctx;
    if (fifo_push(&s->out, buf, len) != 0) return MBEDTLS_ERR_SSL_WANT_WRITE;
    return (int)len;
}

static int tls_recv_cb(void* ctx, unsigned char* buf, size_t len) {
    tls_session* s = (tls_session*)ctx;
    size_t n = fifo_pop(&s->in, buf, len);
    if (n == 0) return MBEDTLS_ERR_SSL_WANT_READ;
    return (int)n;
}

/* 解析 ALPN 列表（byte[] = NUL 分隔名称序列，尾随 NUL）→ mbedtls_ssl_conf_alpn_protocols。 */
static int tls_setup_alpn(tls_session* s, const uint8_t* alpn_list) {
    int32_t len = arr_len(alpn_list);
    if (!alpn_list || len <= 0) return 0;

    s->alpn_buf = (char*)malloc((size_t)len + 1);
    if (!s->alpn_buf) return -1;
    memcpy(s->alpn_buf, alpn_list, (size_t)len);
    s->alpn_buf[len] = '\0';

    int n = 0;
    char* p = s->alpn_buf;
    while (n < 7 && *p) {
        s->alpn_ptrs[n++] = p;
        p += strlen(p) + 1;
        if (p > s->alpn_buf + len) break;
    }
    s->alpn_ptrs[n] = NULL;
    if (n == 0) return 0;
    return mbedtls_ssl_conf_alpn_protocols(&s->conf, s->alpn_ptrs);
}

/* 取走输出 FIFO 全部字节 → Arc byte[]（可能为空数组；OOM 返回 NULL）。 */
static uint8_t* tls_drain(tls_session* s) {
    size_t n = fifo_available(&s->out);
    uint8_t* out = arr_create((int32_t)n);
    if (!out) return NULL;
    if (n > 0) fifo_pop(&s->out, out, n);
    return out;
}

static void tls_free_arr(uint8_t* out) {
    if (out) free(out - 8);
}

static void tls_session_free(tls_session* s) {
    if (!s) return;
    mbedtls_ssl_free(&s->ssl);
    mbedtls_ssl_config_free(&s->conf);
    mbedtls_x509_crt_free(&s->cacert);
    mbedtls_x509_crl_free(&s->crl);
    mbedtls_x509_crt_free(&s->own_cert);
    mbedtls_pk_free(&s->own_key);
    free(s->in.data);
    free(s->out.data);
    free(s->early_rx.data);
    free(s->alpn_buf);
    free(s);
}

/* 解析信任链 blob：DER 单证书 / PEM 链（可多张，mbedtls 经 `next` 链表链接）。
 * 检测 PEM 标记（"-----BEGIN"）→ `mbedtls_x509_crt_parse`（要求 NUL 结尾，
 * 内部按 PEM 格式解析）；否则按 DER。返回 0 成功 / 负值失败。 */
static int tls_parse_certs(mbedtls_x509_crt* crt, const uint8_t* blob, int32_t len) {
    static const char PEM_MARK[] = "-----BEGIN";
    const int32_t PEM_MARK_LEN = (int32_t)(sizeof(PEM_MARK) - 1);
    int pem = 0;
    for (int32_t i = 0; i + PEM_MARK_LEN <= len; i++) {
        if (memcmp(blob + i, PEM_MARK, PEM_MARK_LEN) == 0) {
            pem = 1;
            break;
        }
    }
    if (pem) {
        /* mbedtls_x509_crt_parse 以 buf[len-1]=='\0' 判定 PEM → 复制附 NUL。 */
        uint8_t* buf = (uint8_t*)malloc((size_t)len + 1);
        if (!buf) return -90;
        memcpy(buf, blob, (size_t)len);
        buf[len] = '\0';
        int rc = mbedtls_x509_crt_parse(crt, buf, (size_t)len + 1);
        free(buf);
        return rc;
    }
    /* DER：可能为多张证书拼接（`TlsClientSession.BuildTrustChainBlob` 形态）——
     * `mbedtls_x509_crt_parse_der` 单次仅解析一张，按已解析 `raw.len` 逐张推进。 */
    const uint8_t* p = blob;
    int32_t remaining = len;
    while (remaining > 0) {
        int rc = mbedtls_x509_crt_parse_der(crt, p, (size_t)remaining);
        if (rc != 0) return rc;
        mbedtls_x509_crt* last = crt;
        while (last->next != NULL) last = last->next;
        int32_t consumed = (int32_t)last->raw.len;
        if (consumed <= 0 || consumed > remaining) return -91;
        p += consumed;
        remaining -= consumed;
    }
    return 0;
}

/* 客户端会话：server_name（SNI + 证书校验主机名）+ 信任锚 DER（空 = 不校验）+
 * ALPN 列表 → opaque `tls_session*` 句柄；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_client_new(
    const char* server_name, const uint8_t* trust_der, const uint8_t* alpn_list) {
    ensure_psa();
    if (!server_name || !*server_name) return NULL;

    tls_session* s = (tls_session*)calloc(1, sizeof(tls_session));
    if (!s) return NULL;
    mbedtls_ssl_init(&s->ssl);
    mbedtls_ssl_config_init(&s->conf);
    mbedtls_x509_crt_init(&s->cacert);
    mbedtls_x509_crt_init(&s->own_cert);
    mbedtls_pk_init(&s->own_key);

    if (mbedtls_ssl_config_defaults(&s->conf, MBEDTLS_SSL_IS_CLIENT,
                                    MBEDTLS_SSL_TRANSPORT_STREAM,
                                    MBEDTLS_SSL_PRESET_DEFAULT) != 0) {
        goto fail;
    }
    mbedtls_ssl_conf_min_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);
    mbedtls_ssl_conf_max_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);
    /* S5：会话恢复前置——客户端会话票证启用（解析/存储 NewSessionTicket，
     * `rt_crypto_tls_session_save/load` 复用；M3 时该行 DISABLED）。 */
    mbedtls_ssl_conf_session_tickets(&s->conf, MBEDTLS_SSL_SESSION_TICKETS_ENABLED);

    int32_t trust_len = arr_len(trust_der);
    if (trust_der && trust_len > 0) {
        if (mbedtls_x509_crt_parse_der(&s->cacert, trust_der, (size_t)trust_len) != 0) goto fail;
        mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, NULL);
        mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_REQUIRED);
    } else {
        mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_NONE);
    }

    if (tls_setup_alpn(s, alpn_list) != 0) goto fail;
    if (mbedtls_ssl_setup(&s->ssl, &s->conf) != 0) goto fail;
    if (mbedtls_ssl_set_hostname(&s->ssl, server_name) != 0) goto fail;
    mbedtls_ssl_set_bio(&s->ssl, s, tls_send_cb, tls_recv_cb, NULL);
    return (uint8_t*)s;

fail:
    tls_session_free(s);
    return NULL;
}

/* 服务端会话（本地 TLS 1.3 测试服务器专用；非公开 facade 面）：证书 DER +
 * 私钥 DER（PKCS#1）+ ALPN 列表 → opaque `tls_session*` 句柄；失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_server_new(
    const uint8_t* cert_der, const uint8_t* key_der, const uint8_t* alpn_list) {
    ensure_psa();
    int32_t cert_len = arr_len(cert_der);
    int32_t key_len = arr_len(key_der);
    if (cert_len <= 0 || key_len <= 0 || !cert_der || !key_der) return NULL;

    tls_session* s = (tls_session*)calloc(1, sizeof(tls_session));
    if (!s) return NULL;
    mbedtls_ssl_init(&s->ssl);
    mbedtls_ssl_config_init(&s->conf);
    mbedtls_x509_crt_init(&s->cacert);
    mbedtls_x509_crt_init(&s->own_cert);
    mbedtls_pk_init(&s->own_key);

    if (mbedtls_x509_crt_parse_der(&s->own_cert, cert_der, (size_t)cert_len) != 0) goto fail;
    if (mbedtls_pk_parse_key(&s->own_key, key_der, (size_t)key_len, NULL, 0) != 0) goto fail;

    if (mbedtls_ssl_config_defaults(&s->conf, MBEDTLS_SSL_IS_SERVER,
                                    MBEDTLS_SSL_TRANSPORT_STREAM,
                                    MBEDTLS_SSL_PRESET_DEFAULT) != 0) {
        goto fail;
    }
    mbedtls_ssl_conf_min_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);
    mbedtls_ssl_conf_max_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);
    mbedtls_ssl_conf_session_tickets(&s->conf, MBEDTLS_SSL_SESSION_TICKETS_DISABLED);

    if (tls_setup_alpn(s, alpn_list) != 0) goto fail;
    if (mbedtls_ssl_conf_own_cert(&s->conf, &s->own_cert, &s->own_key) != 0) goto fail;
    if (mbedtls_ssl_setup(&s->ssl, &s->conf) != 0) goto fail;
    mbedtls_ssl_set_bio(&s->ssl, s, tls_send_cb, tls_recv_cb, NULL);
    return (uint8_t*)s;

fail:
    tls_session_free(s);
    return NULL;
}

/* 非阻塞握手：喂入 recv（可为空 byte[]）→ 产出 send_out（byte[]，可能为空）。
 * state：1 = 握手完成；0 = 需更多输入（调用方读下一块再喂）；-1 = 出错（返回 NULL）。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_handshake(
    uint8_t* handle, const uint8_t* recv, int32_t* state) {
    if (state) *state = -1;
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;

    int32_t recv_len = arr_len(recv);
    if (recv && recv_len > 0) {
        if (fifo_push(&s->in, recv, (size_t)recv_len) != 0) return NULL;
    }

    int rc = mbedtls_ssl_handshake(&s->ssl);

#if defined(MBEDTLS_SSL_EARLY_DATA)
    /* S5（服务端 · 0-RTT）：握手遇 MBEDTLS_ERR_SSL_RECEIVED_EARLY_DATA 时，
     * 内部经 mbedtls_ssl_read_early_data 吸收早数据入 early_rx FIFO 后继续握手。
     * 既有 state 语义（0/1/-1）不变；早数据由 rt_crypto_tls_read_early_data 读出。 */
    {
        int guard = 0;
        while (rc == MBEDTLS_ERR_SSL_RECEIVED_EARLY_DATA && guard++ < 32) {
            uint8_t tmp[16384];
            int n = mbedtls_ssl_read_early_data(&s->ssl, tmp, (size_t)sizeof(tmp));
            if (n > 0) {
                if (fifo_push(&s->early_rx, tmp, (size_t)n) != 0) {
                    uint8_t* o2 = tls_drain(s);
                    if (o2) tls_free_arr(o2);
                    return NULL;
                }
            }
            rc = mbedtls_ssl_handshake(&s->ssl);
        }
    }
#endif /* MBEDTLS_SSL_EARLY_DATA */

    uint8_t* out = tls_drain(s);

    if (rc == 0) {
        if (state) *state = 1;
        return out;
    }
    if (rc == MBEDTLS_ERR_SSL_WANT_READ || rc == MBEDTLS_ERR_SSL_WANT_WRITE) {
        if (state) *state = 0;
        return out;
    }
    if (out) tls_free_arr(out);
    return NULL;
}

/* 明文写 → 加密字节（byte[]，内部 0x00 不截断）；失败返回 NULL。
 * WANT_READ 时返回空数组（密文尚未产出，需先处理入站数据），调用方读后重试同一块。
 * S5：客户端写路径遇 NewSessionTicket post-handshake 消息时 mbedTLS 返回
 * MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET（票证已内部吸收、会话可导出），
 * 语义 = 重试同参——此处内部循环吸收后继续写出（守护 32 次防失控）。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_write(uint8_t* handle, const uint8_t* plain) {
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;

    int32_t plain_len = arr_len(plain);
    if (plain_len <= 0) return arr_create(0);
    if (!plain) plain = (const uint8_t*)"";

    int rc;
    int guard = 0;
    for (;;) {
        rc = mbedtls_ssl_write(&s->ssl, plain, (size_t)plain_len);
        if (rc == MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET && guard++ < 32) continue;
        break;
    }
    uint8_t* out = tls_drain(s);

    if (rc < 0) {
        if (out) tls_free_arr(out);
        if (rc == MBEDTLS_ERR_SSL_WANT_READ || rc == MBEDTLS_ERR_SSL_WANT_WRITE) {
            return arr_create(0);
        }
        return NULL;
    }
    if (rc < plain_len) {
        /* TLS 1.3 单记录明文上限 2^14-1；facade 分块 ≤16KB，超出即防御性失败。 */
        if (out) tls_free_arr(out);
        return NULL;
    }
    return out;
}

/* 解密明文读：喂入 enc（密文字节）→ 明文写入 buffer[offset..]。
 * 返回实际字节数；0 = EOF（对端 close_notify）；-2 = 需更多输入；-1 = 出错。
 * S5：客户端读路径遇 NewSessionTicket post-handshake 消息时 mbedTLS 返回
 * MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET（票证已吸收、会话可导出，
 * `rt_crypto_tls_session_save` 此后可用）——语义 = 重试同参（票证记录本身不入
 * 应用数据流）；内部循环吸收后继续解密，直至产出真实数据或 WANT/出错。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_read(
    uint8_t* handle, const uint8_t* enc, uint8_t* buffer, int32_t offset, int32_t count) {
    if (!handle || !buffer || offset < 0 || count <= 0) return -1;
    tls_session* s = (tls_session*)(void*)handle;

    int32_t enc_len = arr_len(enc);
    if (enc && enc_len > 0) {
        if (fifo_push(&s->in, enc, (size_t)enc_len) != 0) return -1;
    }

    int rc;
    int guard = 0;
    for (;;) {
        rc = mbedtls_ssl_read(&s->ssl, buffer + offset, (size_t)count);
        if (rc == MBEDTLS_ERR_SSL_RECEIVED_NEW_SESSION_TICKET && guard++ < 32) continue;
        break;
    }
    if (rc < 0) {
        if (rc == MBEDTLS_ERR_SSL_WANT_READ || rc == MBEDTLS_ERR_SSL_WANT_WRITE) return -2;
        return -1;
    }
    return rc;
}

/* 协商出的 ALPN 协议 → malloc'd C 字符串（无协商返回空串 ""）；失败返回 NULL。 */
CRYPTO_ABI_EXPORT char* rt_crypto_tls_alpn(uint8_t* handle) {
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;
    const char* proto = mbedtls_ssl_get_alpn_protocol(&s->ssl);
    if (!proto) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    size_t len = strlen(proto);
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    memcpy(out, proto, len + 1);
    return out;
}

/* 释放 TLS 会话句柄。 */
CRYPTO_ABI_EXPORT void rt_crypto_tls_free(uint8_t* handle) {
    tls_session_free((tls_session*)(void*)handle);
}

/* ──── RFC 026 S5：完整证书链校验 / 会话恢复 / 0-RTT / 公开服务端 facade ────
 *
 * 035 M3 诚实边界逐项完结（2026-08-05 · S5）。全部新 ABI 逐项独立立宪
 * （RFC 036 §3.5），不改既有 `rt_crypto_*` 签名/语义。mbedTLS 4.x API 差异
 * 实测：`mbedtls_ssl_ticket_setup` 以 PSA 类型（`psa_algorithm_t`/`psa_key_type_t`/
 * `psa_key_bits_t`）替代 3.x 的 `mbedtls_cipher_type_t`；早数据（`MBEDTLS_SSL_EARLY_DATA`）
 * 默认关闭，须构建期 `-DMBEDTLS_SSL_EARLY_DATA` 启用（`fetch-boringssl-native.ps1`
 * Cflags 已同步）。
 */

/* 显式校验策略（`TlsClientSession.TrustAnchor` 语义演进：null=不校验 → 显式策略）。
 * mode：0 = None（不校验 · 兼容面）；1 = Anchor（信任锚单 DER 证书 · 最小校验 ·
 * 等同 M3 行为）；2 = FullChain（PEM 链：根 + 中间证书可多张 · 完整链构建 + 有效期 +
 * 主机名）。主机名校验经 `mbedtls_ssl_set_hostname`（client_new 已设）+ VERIFY_REQUIRED
 * 自动执行（RFC 6066 SNI + SAN/CN）。返回 0 成功 / 负值失败。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_set_verify(
    uint8_t* handle, int32_t mode, const uint8_t* trust_blob) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    if (mode == 0) {
        mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_NONE);
        return 0;
    }
    if (mode < 1 || mode > 2) return -2;
    int32_t blen = arr_len(trust_blob);
    if (!trust_blob || blen <= 0) return -3;
    mbedtls_x509_crt_free(&s->cacert);
    mbedtls_x509_crt_init(&s->cacert);
    int rc = tls_parse_certs(&s->cacert, trust_blob, blen);
    if (rc != 0) return rc;
    mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, &s->crl);
    mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_REQUIRED);
    return 0;
}

/* 系统根证书加载（`TlsClientSession.UseSystemRoots` 默认 · 真实公网主机证书校验）：
 * 将 OS 信任库根证书载入会话 CA 链并置 VERIFY_REQUIRED——配合 `client_new` 已设的
 * 主机名（`mbedtls_ssl_set_hostname`）做完整链 + 主机名（SNI/SAN）校验。
 *   Windows: CAPI Crypt32 `CertOpenSystemStore("ROOT")` 枚举根证书 → DER 逐张解析；
 *   Unix:    依次尝试标准 CA bundle 路径（PEM）。
 * 与 `set_verify` 互斥（任择其一）：本函数自置 CA 链 + VERIFY_REQUIRED，后续调用
 * `set_verify` 会以信任 blob 覆盖；反之亦然。失败时 fail-closed（不静默降级）。
 * 返回 0 成功 / 负值失败（无系统根可用）。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_load_system_roots(uint8_t* handle) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
#if defined(_WIN32)
    mbedtls_x509_crt_free(&s->cacert);
    mbedtls_x509_crt_init(&s->cacert);
    HCERTSTORE store = CertOpenSystemStoreW(0, L"ROOT");
    if (!store) return -10;
    PCCERT_CONTEXT ctx = NULL;
    int count = 0;
    while ((ctx = CertEnumCertificatesInStore(store, ctx)) != NULL) {
        if (ctx->cbCertEncoded > 0 && ctx->pbCertEncoded) {
            if (mbedtls_x509_crt_parse_der(&s->cacert, ctx->pbCertEncoded,
                                           ctx->cbCertEncoded) == 0) {
                count++;
            }
        }
    }
    if (ctx) CertFreeCertificateContext(ctx);
    CertCloseStore(store, 0);
    if (count == 0) return -11;
    mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, &s->crl);
    mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_REQUIRED);
    return 0;
#else
    static const char* paths[] = {
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        NULL
    };
    for (int i = 0; paths[i]; i++) {
        FILE* f = fopen(paths[i], "rb");
        if (!f) continue;
        if (fseek(f, 0, SEEK_END) != 0) { fclose(f); continue; }
        long sz = ftell(f);
        rewind(f);
        if (sz <= 0) { fclose(f); continue; }
        uint8_t* buf = (uint8_t*)malloc((size_t)sz + 1);
        if (!buf) { fclose(f); return -12; }
        size_t rd = fread(buf, 1, (size_t)sz, f);
        fclose(f);
        if (rd != (size_t)sz) { free(buf); continue; }
        buf[rd] = '\0';
        mbedtls_x509_crt_free(&s->cacert);
        mbedtls_x509_crt_init(&s->cacert);
        int rc = mbedtls_x509_crt_parse(&s->cacert, buf, rd + 1);
        free(buf);
        if (rc == 0) {
            mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, &s->crl);
            mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_REQUIRED);
            return 0;
        }
    }
    return -13;
#endif
}

/* CRL（吊销 · 最小面）：解析 DER CRL 并挂到 CA 链校验路径
 * （`mbedtls_ssl_conf_ca_chain` 第三参）。须在 set_verify 之后调用（CA 非空）。
 * OCSP stapling 诚实后置。返回 0 成功 / 负值失败。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_set_crl(uint8_t* handle, const uint8_t* crl_der) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    int32_t clen = arr_len(crl_der);
    if (!crl_der || clen <= 0) return -2;
    mbedtls_x509_crl_free(&s->crl);
    mbedtls_x509_crl_init(&s->crl);
    if (mbedtls_x509_crl_parse_der(&s->crl, crl_der, (size_t)clen) != 0) return -3;
    /* 重挂 CA 链（含 CRL）。cacert 为空 = 配置误用 → 校验 fail-closed（安全侧）。 */
    mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, &s->crl);
    return 0;
}

/* 握手后校验结果：`mbedtls_ssl_get_verify_result` 位标志（0 = 通过）。
 * 仅在 VERIFY_REQUIRED + 握手完成后有效。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_verify_result(uint8_t* handle) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    return (int32_t)mbedtls_ssl_get_verify_result(&s->ssl);
}

/* 客户端证书（双向认证）：DER 证书 + DER 私钥（PKCS#8 或 PKCS#1）→ own_cert。
 * 须在握手前调用。返回 0 成功 / 负值失败。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_set_client_cert(
    uint8_t* handle, const uint8_t* cert_der, const uint8_t* key_der) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    int32_t clen = arr_len(cert_der);
    int32_t klen = arr_len(key_der);
    if (clen <= 0 || klen <= 0 || !cert_der || !key_der) return -2;
    if (mbedtls_x509_crt_parse_der(&s->own_cert, cert_der, (size_t)clen) != 0) return -3;
    if (mbedtls_pk_parse_key(&s->own_key, key_der, (size_t)klen, NULL, 0) != 0) return -4;
    if (mbedtls_ssl_conf_own_cert(&s->conf, &s->own_cert, &s->own_key) != 0) return -5;
    return 0;
}

/* 会话序列化保存（TLS 1.3 ticket 形态）：握手完成后 → 字节（含内部 0x00；
 * 显式长度由 byte[] 载体承载）。失败返回 NULL。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_session_save(uint8_t* handle) {
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;
    mbedtls_ssl_session sess;
    mbedtls_ssl_session_init(&sess);
    if (mbedtls_ssl_get_session(&s->ssl, &sess) != 0) {
        mbedtls_ssl_session_free(&sess);
        return NULL;
    }
    uint8_t buf[16384];
    size_t olen = 0;
    int rc = mbedtls_ssl_session_save(&sess, buf, sizeof(buf), &olen);
    mbedtls_ssl_session_free(&sess);
    if (rc != 0) return NULL;
    return arr_from_bytes(buf, (int32_t)olen);
}

/* 会话载入（恢复）：握手前（setup 后）载入序列化会话字节 → `mbedtls_ssl_set_session`。
 * 返回 0 成功 / 负值失败。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_session_load(uint8_t* handle, const uint8_t* bytes) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    int32_t blen = arr_len(bytes);
    if (!bytes || blen <= 0) return -2;
    mbedtls_ssl_session sess;
    mbedtls_ssl_session_init(&sess);
    if (mbedtls_ssl_session_load(&sess, bytes, (size_t)blen) != 0) {
        mbedtls_ssl_session_free(&sess);
        return -3;
    }
    int rc = mbedtls_ssl_set_session(&s->ssl, &sess);
    mbedtls_ssl_session_free(&sess);
    return rc;
}

/* 公开服务端 facade（`rt_crypto_tls_server_new` 由测试 harness 面提升）：
 * cert_der + key_der（PKCS#1/PKCS#8）+ ALPN 列表 + flags + client_ca_blob
 * （双向认证 CA：DER 单证书或 PEM 链）→ opaque `tls_session*`；失败返回 NULL。
 * flags 位：0x1 = 会话票证（tickets）启用；0x2 = 客户端证书 VERIFY_REQUIRED
 * （须 client_ca_blob 非空）；0x4 = 早数据（0-RTT）接收启用
 * （`MBEDTLS_SSL_EARLY_DATA` + `max_early_data_size` 16384；TLS 1.3 仅）。
 * tickets 启用时以 PSA AES-256-GCM 票证密钥（lifetime 86400s = 1 天，RFC 8446
 * 7 天上限内）+ 进程内 PSA 随机源。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_server_new_ex(
    const uint8_t* cert_der, const uint8_t* key_der, const uint8_t* alpn_list,
    int32_t flags, const uint8_t* client_ca_blob) {
    ensure_psa();
    int32_t cert_len = arr_len(cert_der);
    int32_t key_len = arr_len(key_der);
    if (cert_len <= 0 || key_len <= 0 || !cert_der || !key_der) return NULL;

    tls_session* s = (tls_session*)calloc(1, sizeof(tls_session));
    if (!s) return NULL;
    mbedtls_ssl_init(&s->ssl);
    mbedtls_ssl_config_init(&s->conf);
    mbedtls_x509_crt_init(&s->cacert);
    mbedtls_x509_crl_init(&s->crl);
    mbedtls_x509_crt_init(&s->own_cert);
    mbedtls_pk_init(&s->own_key);

    if (tls_parse_certs(&s->own_cert, cert_der, cert_len) != 0) {
        goto fail;
    }
    if (mbedtls_pk_parse_key(&s->own_key, key_der, (size_t)key_len, NULL, 0) != 0) {
        goto fail;
    }

    if (mbedtls_ssl_config_defaults(&s->conf, MBEDTLS_SSL_IS_SERVER,
                                    MBEDTLS_SSL_TRANSPORT_STREAM,
                                    MBEDTLS_SSL_PRESET_DEFAULT) != 0) {
        goto fail;
    }
    mbedtls_ssl_conf_min_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);
    mbedtls_ssl_conf_max_tls_version(&s->conf, MBEDTLS_SSL_VERSION_TLS1_3);

    if (flags & 0x1) {
        mbedtls_ssl_conf_session_tickets(&s->conf, MBEDTLS_SSL_SESSION_TICKETS_ENABLED);
        /* mbedTLS 4.x：ticket_setup 以 PSA 类型入参（3.x 为 mbedtls_cipher_type_t）。
         * 进程级共享 ticket 上下文（票证密钥跨连接复用 → 会话恢复闭环）。 */
        if (ticket_ctx_global_init() != 0) {
            goto fail;
        }
        mbedtls_ssl_conf_session_tickets_cb(&s->conf, mbedtls_ssl_ticket_write,
                                            mbedtls_ssl_ticket_parse, &g_ticket_ctx);
    } else {
        mbedtls_ssl_conf_session_tickets(&s->conf, MBEDTLS_SSL_SESSION_TICKETS_DISABLED);
    }

    if (flags & 0x2) {
        int32_t cb_len = arr_len(client_ca_blob);
        if (!client_ca_blob || cb_len <= 0) goto fail;
        if (tls_parse_certs(&s->cacert, client_ca_blob, cb_len) != 0) goto fail;
        mbedtls_ssl_conf_ca_chain(&s->conf, &s->cacert, NULL);
        mbedtls_ssl_conf_authmode(&s->conf, MBEDTLS_SSL_VERIFY_REQUIRED);
    }

#if defined(MBEDTLS_SSL_EARLY_DATA)
    if (flags & 0x4) {
        mbedtls_ssl_conf_early_data(&s->conf, MBEDTLS_SSL_EARLY_DATA_ENABLED);
        mbedtls_ssl_conf_max_early_data_size(&s->conf, 16384u);
    }
#endif

    if (tls_setup_alpn(s, alpn_list) != 0) {
        goto fail;
    }
    if (mbedtls_ssl_conf_own_cert(&s->conf, &s->own_cert, &s->own_key) != 0) {
        goto fail;
    }
    if (mbedtls_ssl_setup(&s->ssl, &s->conf) != 0) {
        goto fail;
    }
    mbedtls_ssl_set_bio(&s->ssl, s, tls_send_cb, tls_recv_cb, NULL);
    return (uint8_t*)s;

fail:
    tls_session_free(s);
    return NULL;
}

/* 取走输出 FIFO 全部字节（服务端 flush post-handshake 消息如 NewSessionTicket；
 * 客户端握手/读写路径已由各 ABI 内部 drain）。可能为空数组。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_drain(uint8_t* handle) {
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;
    return tls_drain(s);
}

#if defined(MBEDTLS_SSL_EARLY_DATA)
/* 0-RTT 早数据（客户端）启用开关（须在握手前；会话须来自允许早数据的 ticket）。
 * 返回 0 成功。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_enable_early_data(uint8_t* handle, int32_t enabled) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    mbedtls_ssl_conf_early_data(&s->conf, enabled ? MBEDTLS_SSL_EARLY_DATA_ENABLED
                                                  : MBEDTLS_SSL_EARLY_DATA_DISABLED);
    return 0;
}

/* 0-RTT 早数据写（客户端）：握手期间喂入 recv → 产出密文字节（byte[]，可能为空）。
 * state：1 = 早数据已写出（调用方继续正常握手）；0 = 需更多输入（读下一块再试）；
 * -1 = 无法写早数据（ticket 不允许 / 超量 / 未启用——非错误，退正常握手）；
 * -2 = 硬错误。返回 NULL 仅当 OOM/硬错误（state -2）。 */
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_write_early_data(
    uint8_t* handle, const uint8_t* recv, const uint8_t* plain, int32_t* state) {
    if (state) *state = -2;
    if (!handle) return NULL;
    tls_session* s = (tls_session*)(void*)handle;

    int32_t recv_len = arr_len(recv);
    if (recv && recv_len > 0) {
        if (fifo_push(&s->in, recv, (size_t)recv_len) != 0) return NULL;
    }
    int32_t plain_len = arr_len(plain);
    if (plain_len <= 0) return NULL;
    if (!plain) plain = (const uint8_t*)"";

    int rc = mbedtls_ssl_write_early_data(&s->ssl, plain, (size_t)plain_len);
    uint8_t* out = tls_drain(s);

    if (rc < 0) {
        if (rc == MBEDTLS_ERR_SSL_WANT_READ || rc == MBEDTLS_ERR_SSL_WANT_WRITE) {
            /* WANT_READ/WANT_WRITE：out 内含 ClientHello 等握手飞行数据，须交还调用方
             * 发送（否则死锁：对端永远收不到 ClientHello）。 */
            if (state) *state = 0;
            return out ? out : arr_create(0);
        }
        /* CANNOT_WRITE_EARLY_DATA 及同类：非错误，退正常握手。out 可能已含
         * ClientHello（未带早数据扩展）——须交还调用方发送。 */
        if (state) *state = -1;
        return out;
    }
    if (rc < plain_len) {
        if (out) tls_free_arr(out);
        if (state) *state = -1;
        return NULL;
    }
    if (state) *state = 1;
    return out;
}

/* 早数据状态（客户端 · 握手完成后）：0 = 未指示 / 1 = ACCEPTED / 2 = REJECTED。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_early_data_status(uint8_t* handle) {
    if (!handle) return -1;
    tls_session* s = (tls_session*)(void*)handle;
    return (int32_t)mbedtls_ssl_get_early_data_status(&s->ssl);
}

/* 早数据读（服务端）：优先读握手期内部吸收的 early_rx FIFO（见 rt_crypto_tls_handshake）。
 * 返回字节数；0 = 无更多早数据；-1 = 出错（内部吸收路径不返回 -2）。 */
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_read_early_data(
    uint8_t* handle, const uint8_t* enc, uint8_t* buffer, int32_t offset, int32_t count) {
    if (!handle || !buffer || offset < 0 || count <= 0) return -1;
    tls_session* s = (tls_session*)(void*)handle;

    if (fifo_available(&s->early_rx) > 0) {
        return (int32_t)fifo_pop(&s->early_rx, buffer + offset, (size_t)count);
    }

    /* 回退路径：握手未走内部吸收时（防御），直接读 mbedTLS 早数据缓冲。 */
    int32_t enc_len = arr_len(enc);
    if (enc && enc_len > 0) {
        if (fifo_push(&s->in, enc, (size_t)enc_len) != 0) return -1;
    }
    int rc = mbedtls_ssl_read_early_data(&s->ssl, buffer + offset, (size_t)count);
    if (rc < 0) {
        if (rc == MBEDTLS_ERR_SSL_WANT_READ || rc == MBEDTLS_ERR_SSL_WANT_WRITE) return -2;
        return -1;
    }
    return rc;
}
#else
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_enable_early_data(uint8_t* handle, int32_t enabled) {
    (void)handle; (void)enabled;
    return -90; /* 构建期未启用 MBEDTLS_SSL_EARLY_DATA */
}
CRYPTO_ABI_EXPORT uint8_t* rt_crypto_tls_write_early_data(
    uint8_t* handle, const uint8_t* recv, const uint8_t* plain, int32_t* state) {
    (void)handle; (void)recv; (void)plain;
    if (state) *state = -2;
    return NULL;
}
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_early_data_status(uint8_t* handle) {
    (void)handle;
    return -90;
}
CRYPTO_ABI_EXPORT int32_t rt_crypto_tls_read_early_data(
    uint8_t* handle, const uint8_t* enc, uint8_t* buffer, int32_t offset, int32_t count) {
    (void)handle; (void)enc; (void)buffer; (void)offset; (void)count;
    return -90;
}
#endif /* MBEDTLS_SSL_EARLY_DATA */
