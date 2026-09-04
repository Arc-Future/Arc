/* rt_quic_native.c — vendored QUIC 底座 `rt_quic_*` ABI 实现面（RFC 034 S4）
 *
 * S4 在 vendored ngtcp2 1.25.0 + OpenSSL 3.5（MSYS2 prebuilt，见
 * `bin/VENDOR.md`）之上实现 RFC 9000（QUIC v1）最小客户端/本地测试服务器
 * 驱动面。TLS 1.3 over QUIC（RFC 9001）适配由 OpenSSL 3.5 的 QUIC TLS
 * 回调面经 `ngtcp2_crypto_ossl_configure_{client,server}_session` 提供
 * （set_encryption_secrets / add_handshake_data / flush_flight / send_alert
 * 位于 vendored `libngtcp2_crypto_ossl-0.dll` 内），本 shim 不重复实现。
 *
 * 形态约定（与 `crates/runtime-crypto/shim/rt_crypto_native.c` 对齐）：
 *   - `byte[]` 实参/返回值 = RtArrayHeader{int32 length; int32 elem_size}
 *     之后的 payload 指针（header 位于 payload-8）；
 *   - 返回 `byte[]` 用 `arr_create`（malloc header+payload，调用方释放）；
 *   - 连接句柄 = `rt_quic_conn*`（64 位指针直传，防 int32 截断）；
 *   - `rt_quic_conn_flush` 出参 `out` = 帧化报文序列
 *     [u32 大端长度][报文 payload]...（一次 flush 至多一帧装一个 UDP
 *     报文；往返调用直至返回 0）；
 *   - 失败返回 NULL / 0 / 负错误码；成功返回新分配的 byte[] / 非零句柄 /
 *     ≥0 结果。负错误码 = ngtcp2 错误码原样透传（见
 *     `ngtcp2_err_infer_quic_transport_error_code` 族）。
 *
 * ABI 命名：延续 `rt_crypto_*` 前缀惯例 → `rt_quic_*`（RFC 025 S4 逐项
 * 独立立宪，见 033 §2.6 S4 验收完结注记的 ABI 表）。
 *
 * 诚实边界：本 shim 仅实现 QUIC v1 最小客户端 + 本地测试服务器驱动；
 * 0-RTT / 连接迁移 / QPACK 动态表 / 拥塞控制完整调参 / 实网互操作后置。
 * 服务器证书校验在 e2e 局部范围关闭（SSL_VERIFY_NONE，文档注记），
 * 不构成对生产服务器模式的宣称。
 *
 * 许可证：Apache-2.0（Arc 兼容；与 ngtcp2 MIT / OpenSSL Apache-2.0 一致）。
 */

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <ngtcp2/ngtcp2.h>
#include <ngtcp2/ngtcp2_crypto.h>
#include <ngtcp2/ngtcp2_crypto_ossl.h>
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/rand.h>
#include <openssl/x509.h>
#include <openssl/pem.h>

#if defined(_WIN32)
#  include <winsock2.h>
#  include <windows.h>
#  define QUIC_ABI_EXPORT __declspec(dllexport)
#else
#  include <sys/socket.h>
#  include <netinet/in.h>
#  define QUIC_ABI_EXPORT __attribute__((visibility("default")))
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

/* ---- 单调时钟（ngtcp2_tstamp = 纳秒） ---- */

static uint64_t rt_quic_now(void) {
#if defined(_WIN32)
    return (uint64_t)GetTickCount64() * 1000000u;
#else
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000u + (uint64_t)ts.tv_nsec;
#endif
}

/* ---- 连接对象 ---- */

#define RT_QUIC_MAX_CIDLEN 18

/* 每流接收缓冲（recv_stream_data_cb 写入，rt_quic_stream_recv 排空） */
typedef struct rt_quic_recv_buf {
    int64_t stream_id;
    uint8_t* data;
    size_t len;
    size_t cap;
    struct rt_quic_recv_buf* next;
} rt_quic_recv_buf;

/* 待发送队列（rt_quic_stream_send 入队，flush 经 writev_stream 交给 ngtcp2）。
   ngtcp2 的重传缓冲只保存对应用数据的引用（ngtcp2_rtb 存 stream.data vec，
   不拷贝），因此本缓冲必须在数据被对端 ACK 前保持存活——不能随 handoff 释放。 */
typedef struct rt_quic_send_buf {
    int64_t stream_id;
    uint8_t* data;      /* 拥有权在本结构 */
    size_t len;         /* 总字节数 */
    size_t base;        /* 本缓冲在流字节空间内的起始偏移 */
    size_t handoff;     /* 已作为"新数据"交给 ngtcp2 的字节数（pdatalen 推进） */
    size_t acked;       /* 已获对端 ACK 的字节数（acked_stream_data_offset 推进） */
    struct rt_quic_send_buf* next;
} rt_quic_send_buf;

typedef struct rt_quic_conn {
    ngtcp2_conn* conn;
    ngtcp2_crypto_ossl_ctx* ossl_ctx;
    SSL_CTX* ssl_ctx;
    SSL* ssl;
    ngtcp2_crypto_conn_ref conn_ref;
    int is_server;
    int handshake_done;
    int closed;          /* 连接进入 closing/draining 或致命错误 */
    int64_t last_err;
    int rx_any;          /* 已成功读取过对端数据报 */
    int initial_written; /* 客户端已写出首个 Initial（等待服务器响应） */

    ngtcp2_cid dcid;  /* 远端连接 ID（client: 自选随机；server: 客户端 Initial SCID） */
    ngtcp2_cid scid;  /* 本地连接 ID */

    struct sockaddr_in6 local_sa;
    struct sockaddr_in6 remote_sa;
    ngtcp2_path path;
    ngtcp2_pkt_info pi;

    uint8_t alpn_wire[16];  /* 服务器 ALPN 选择用（wire 格式） */
    size_t alpn_wirelen;

    rt_quic_recv_buf* recv_head;
    rt_quic_send_buf* send_head;
    rt_quic_send_buf* send_tail;
} rt_quic_conn;

static ngtcp2_conn* rt_quic_conn_ref_get_conn(ngtcp2_crypto_conn_ref* ref) {
    return ((rt_quic_conn*)ref->user_data)->conn;
}

/* 校验并换算句柄 */
static rt_quic_conn* rt_quic_from_handle(int64_t handle) {
    if (handle == 0) return NULL;
    return (rt_quic_conn*)(intptr_t)handle;
}

QUIC_ABI_EXPORT void rt_quic_conn_free(int64_t handle); /* 前向声明 */

/* ---- 调试日志（RT_QUIC_DEBUG=1 时输出到 stderr；默认静默） ---- */

static int rt_quic_debug_enabled(void) {
    static int cached = -1;
    if (cached < 0) {
        cached = getenv("RT_QUIC_DEBUG") ? 1 : 0;
    }
    return cached;
}

static void rt_quic_log(const char* fmt, ...) {
    if (!rt_quic_debug_enabled()) return;
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
}

/* 包装 ngtcp2 crypto 的 recv_crypto_data：失败时打印 TLS 错误栈，便于 e2e 排查 */
static int cb_debug_recv_crypto_data(ngtcp2_conn* conn,
                                     ngtcp2_encryption_level level,
                                     uint64_t offset, const uint8_t* data,
                                     size_t datalen, void* user_data) {
    int rv = ngtcp2_crypto_recv_crypto_data_cb(conn, level, offset, data,
                                               datalen, user_data);
    if (rv != 0 && rt_quic_debug_enabled()) {
        fprintf(stderr, "[quic] recv_crypto_data rv=%d\n", rv);
        unsigned long e;
        while ((e = ERR_get_error()) != 0) {
            char buf[256];
            ERR_error_string_n(e, buf, sizeof(buf));
            fprintf(stderr, "[quic] openssl: %s\n", buf);
        }
    }
    return rv;
}

static void cb_log_printf(void* user_data, const char* fmt, ...) {
    (void)user_data;
    if (!rt_quic_debug_enabled()) return;
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}

static int cb_debug_client_initial(ngtcp2_conn* conn, void* user_data) {
    rt_quic_log("[quic] client_initial_cb fired\n");
    int rv = ngtcp2_crypto_client_initial_cb(conn, user_data);
    if (rv != 0 && rt_quic_debug_enabled()) {
        fprintf(stderr, "[quic] client_initial_cb rv=%d\n", rv);
        unsigned long e;
        while ((e = ERR_get_error()) != 0) {
            char buf[256];
            ERR_error_string_n(e, buf, sizeof(buf));
            fprintf(stderr, "[quic] openssl: %s\n", buf);
        }
    }
    return rv;
}

static int cb_debug_recv_retry(ngtcp2_conn* conn, const ngtcp2_pkt_hd* hd,
                               void* user_data) {
    rt_quic_log("[quic] recv_retry_cb fired (retry received!)\n");
    return ngtcp2_crypto_recv_retry_cb(conn, hd, user_data);
}

static int cb_debug_recv_client_initial(ngtcp2_conn* conn,
                                        const ngtcp2_cid* dcid,
                                        void* user_data) {
    rt_quic_log("[quic] recv_client_initial_cb fired\n");
    return ngtcp2_crypto_recv_client_initial_cb(conn, dcid, user_data);
}

/* ---- 随机数 / 连接 ID ---- */

static void rt_quic_rand_bytes(uint8_t* dest, size_t len) {
    RAND_bytes(dest, (int)len);
}

static void rt_quic_gen_cid(ngtcp2_cid* cid, size_t len) {
    uint8_t data[RT_QUIC_MAX_CIDLEN];
    rt_quic_rand_bytes(data, len);
    ngtcp2_cid_init(cid, data, len);
}

/* ngtcp2 需要随机数据时的回调。 */
static void cb_rand(uint8_t* dest, size_t destlen,
                    const ngtcp2_rand_ctx* rand_ctx) {
    (void)rand_ctx;
    rt_quic_rand_bytes(dest, destlen);
}

/* ngtcp2 需要新连接 ID 时的回调（RFC 9000 NEW_CONNECTION_ID）。 */
static int cb_get_new_connection_id(ngtcp2_conn* conn, ngtcp2_cid* cid,
                                    uint8_t* token, size_t cidlen,
                                    void* user_data) {
    (void)conn; (void)token; (void)user_data;
    if (cidlen == 0 || cidlen > RT_QUIC_MAX_CIDLEN) return NGTCP2_ERR_INVALID_ARGUMENT;
    rt_quic_gen_cid(cid, cidlen);
    return 0;
}

/* 握手完成的回调：置标记（ngtcp2_conn_tls_handshake_completed 由
   ngtcp2_crypto_read_write_crypto_data 在 SSL_do_handshake 成功时调用，
   本回调只负责应用侧状态）。 */
static int cb_handshake_completed(ngtcp2_conn* conn, void* user_data) {
    (void)conn;
    rt_quic_conn* rc = (rt_quic_conn*)user_data;
    rc->handshake_done = 1;
    rt_quic_log("[quic] handshake_completed\n");
    return 0;
}

/* 收流数据：立即扩充流控窗口（最小流控 = 接受即补额），并缓存到每流缓冲。 */
static int cb_recv_stream_data(ngtcp2_conn* conn, uint32_t flags,
                               int64_t stream_id, uint64_t offset,
                               const uint8_t* data, size_t datalen,
                               void* user_data, void* stream_user_data) {
    (void)flags; (void)offset; (void)stream_user_data;
    rt_quic_conn* rc = (rt_quic_conn*)user_data;
    if (datalen == 0) return 0;

    ngtcp2_conn_extend_max_stream_offset(conn, stream_id, (uint64_t)datalen);
    ngtcp2_conn_extend_max_offset(conn, (uint64_t)datalen);

    rt_quic_recv_buf* buf = rc->recv_head;
    while (buf && buf->stream_id != stream_id) buf = buf->next;
    if (!buf) {
        buf = (rt_quic_recv_buf*)calloc(1, sizeof(*buf));
        if (!buf) return NGTCP2_ERR_NOMEM;
        buf->stream_id = stream_id;
        buf->next = rc->recv_head;
        rc->recv_head = buf;
    }
    if (buf->len + datalen > buf->cap) {
        size_t ncap = buf->cap ? buf->cap : 1024;
        while (ncap < buf->len + datalen) ncap *= 2;
        uint8_t* ndata = (uint8_t*)realloc(buf->data, ncap);
        if (!ndata) return NGTCP2_ERR_NOMEM;
        buf->data = ndata;
        buf->cap = ncap;
    }
    memcpy(buf->data + buf->len, data, datalen);
    buf->len += datalen;
    return 0;
}

/* 远端 RST_STREAM：缓冲流标记为已关（剩余数据可继续排空）。 */
static int cb_stream_reset(ngtcp2_conn* conn, int64_t stream_id,
                           uint64_t final_size, uint64_t app_error_code,
                           void* user_data, void* stream_user_data) {
    (void)conn; (void)final_size; (void)app_error_code;
    (void)user_data; (void)stream_user_data; (void)stream_id;
    return 0;
}

/* ---- ALPN ---- */

static const uint8_t* rt_quic_find_alpn(const uint8_t* list, size_t listlen,
                                        const uint8_t* want, size_t wantlen) {
    size_t off = 0;
    while (off < listlen) {
        size_t l = list[off];
        if (off + 1 + l > listlen) return NULL;
        if (l == wantlen && memcmp(list + off + 1, want, wantlen) == 0) {
            return list + off;
        }
        off += 1 + l;
    }
    return NULL;
}

static int cb_alpn_select(SSL* ssl, const unsigned char** out,
                          unsigned char* outlen, const unsigned char* in,
                          unsigned int inlen, void* arg) {
    (void)ssl;
    rt_quic_conn* rc = (rt_quic_conn*)arg;
    const uint8_t* hit = rt_quic_find_alpn(in, inlen, rc->alpn_wire + 1,
                                           rc->alpn_wire[0]);
    if (!hit) return SSL_TLSEXT_ERR_ALERT_FATAL;
    *out = hit + 1;
    *outlen = hit[0];
    return SSL_TLSEXT_ERR_OK;
}

/* 从内存 PEM 载入证书/私钥到 SSL_CTX。 */
static int rt_quic_ssl_ctx_use_pem(SSL_CTX* ctx, const uint8_t* cert_pem,
                                   size_t cert_len, const uint8_t* key_pem,
                                   size_t key_len) {
    BIO* bio = BIO_new_mem_buf(cert_pem, (int)cert_len);
    if (!bio) return -1;
    X509* cert = PEM_read_bio_X509(bio, NULL, NULL, NULL);
    BIO_free(bio);
    if (!cert) return -1;
    int ok = SSL_CTX_use_certificate(ctx, cert);
    X509_free(cert);
    if (ok != 1) return -1;

    bio = BIO_new_mem_buf(key_pem, (int)key_len);
    if (!bio) return -1;
    EVP_PKEY* pkey = PEM_read_bio_PrivateKey(bio, NULL, NULL, NULL);
    BIO_free(bio);
    if (!pkey) return -1;
    ok = SSL_CTX_use_PrivateKey(ctx, pkey);
    EVP_PKEY_free(pkey);
    if (ok != 1) return -1;
    return 0;
}

/* ---- 路径（::1 回环，仅作端对端标识；本底座驱动不依赖真实 UDP 套接字） ---- */

static void rt_quic_setup_addr(struct sockaddr_in6* sa, uint16_t port) {
    memset(sa, 0, sizeof(*sa));
    sa->sin6_family = AF_INET6;
    sa->sin6_port = htons(port);
    sa->sin6_addr.s6_addr[15] = 1; /* ::1 */
}

/* ---- 连接建立 ---- */

static int rt_quic_alloc(rt_quic_conn* rc, uint16_t local_port,
                         uint16_t remote_port) {
    rc->conn_ref.get_conn = rt_quic_conn_ref_get_conn;
    rc->conn_ref.user_data = rc;
    rt_quic_setup_addr(&rc->local_sa, local_port);
    rt_quic_setup_addr(&rc->remote_sa, remote_port);
    rc->path.local.addr = (ngtcp2_sockaddr*)&rc->local_sa;
    rc->path.local.addrlen = (ngtcp2_socklen)sizeof(rc->local_sa);
    rc->path.remote.addr = (ngtcp2_sockaddr*)&rc->remote_sa;
    rc->path.remote.addrlen = (ngtcp2_socklen)sizeof(rc->remote_sa);
    rc->path.user_data = rc;
    return 0;
}

static int rt_quic_ssl_client_init(rt_quic_conn* rc) {
    /* OpenSSL 3.5 QUIC TLS（外部 QUIC 栈，如 ngtcp2）：必须使用 TLS_client_method
       并随后 SSL_set_connect_state；SSL_set_quic_tls_cbs 仅对 TLS 类型 SSL 合法
       （OSSL_QUIC_*_method 是 OpenSSL 自家 QRL 全栈，配 ngtcp2 会报
       "connection type not set"/"called a function you should not call"）。
       顺序对齐 ngtcp2 官方示例 tls_client_session_ossl.cc。 */
    rc->ssl_ctx = SSL_CTX_new(TLS_client_method());
    if (!rc->ssl_ctx) { rt_quic_log("[quic] ssl_client_init: CTX_new failed\n"); return -1; }
    SSL_CTX_set_min_proto_version(rc->ssl_ctx, TLS1_3_VERSION);
    rt_quic_log("[quic] ssl_client_init: min_proto set\n");
    /* e2e 局部范围：本地自签测试服务器，关闭对端证书校验（诚实注记见 VENDOR.md） */
    SSL_CTX_set_verify(rc->ssl_ctx, SSL_VERIFY_NONE, NULL);
    rt_quic_log("[quic] ssl_client_init: verify set\n");
    if (SSL_CTX_set_alpn_protos(rc->ssl_ctx, rc->alpn_wire,
                                (unsigned int)rc->alpn_wirelen) != 0) {
        rt_quic_log("[quic] ssl_client_init: CTX_set_alpn_protos failed\n");
        return -1;
    }
    rt_quic_log("[quic] ssl_client_init: alpn set on ctx\n");
    rc->ssl = SSL_new(rc->ssl_ctx);
    if (!rc->ssl) { rt_quic_log("[quic] ssl_client_init: SSL_new failed\n"); return -1; }
    if (SSL_set_alpn_protos(rc->ssl, rc->alpn_wire,
                            (unsigned int)rc->alpn_wirelen) != 0) {
        rt_quic_log("[quic] ssl_client_init: SSL_set_alpn_protos failed\n");
        return -1;
    }
    rt_quic_log("[quic] ssl_client_init: alpn set on ssl\n");
    SSL_set_app_data(rc->ssl, &rc->conn_ref);
    if (ngtcp2_crypto_ossl_configure_client_session(rc->ssl) != 0) {
        rt_quic_log("[quic] ssl_client_init: configure_client_session failed\n");
        return -1;
    }
    rt_quic_log("[quic] ssl_client_init: configured\n");
    /* 关键：qtls 回调面经 SSL_do_handshake 驱动，必须显式置客户端握手态
       （否则 SSL_do_handshake 报 SSL_R_CONNECTION_TYPE_NOT_SET）。 */
    SSL_set_connect_state(rc->ssl);
    rt_quic_log("[quic] ssl_client_init: connect_state set\n");
    return 0;
}

static int rt_quic_ssl_server_init(rt_quic_conn* rc, const uint8_t* cert_pem,
                                   size_t cert_len, const uint8_t* key_pem,
                                   size_t key_len) {
    rc->ssl_ctx = SSL_CTX_new(TLS_server_method());
    if (!rc->ssl_ctx) return -1;
    SSL_CTX_set_min_proto_version(rc->ssl_ctx, TLS1_3_VERSION);
    if (rt_quic_ssl_ctx_use_pem(rc->ssl_ctx, cert_pem, cert_len, key_pem,
                                key_len) != 0) {
        return -1;
    }
    SSL_CTX_set_alpn_select_cb(rc->ssl_ctx, cb_alpn_select, rc);
    rc->ssl = SSL_new(rc->ssl_ctx);
    if (!rc->ssl) return -1;
    SSL_set_app_data(rc->ssl, &rc->conn_ref);
    if (ngtcp2_crypto_ossl_configure_server_session(rc->ssl) != 0) {
        return -1;
    }
    /* 对齐 tls_server_session_ossl.cc：必须显式置服务器握手态。 */
    SSL_set_accept_state(rc->ssl);
    return 0;
}

/* 对端 ACK 流数据：释放已确认的发送缓冲（ngtcp2 重传缓冲仅引用应用数据，
   确认后方可释放；释放前缓冲必须保持存活）。 */
static int cb_acked_stream_data_offset(ngtcp2_conn* conn, int64_t stream_id,
                                       uint64_t offset, uint64_t datalen,
                                       void* user_data, void* stream_user_data) {
    (void)conn; (void)stream_user_data;
    rt_quic_conn* rc = (rt_quic_conn*)user_data;
    uint64_t ack_end = (uint64_t)offset + datalen;
    rt_quic_send_buf** pp = &rc->send_head;
    while (*pp) {
        rt_quic_send_buf* sb = *pp;
        if (sb->stream_id == stream_id &&
            (uint64_t)sb->base < ack_end) {
            uint64_t covered = ack_end - (uint64_t)sb->base;
            if (covered > sb->len) covered = sb->len;
            if (covered > sb->acked) sb->acked = (size_t)covered;
            if (sb->acked >= sb->len) {
                *pp = sb->next;
                if (rc->send_tail == sb) rc->send_tail = NULL;
                free(sb->data);
                free(sb);
                continue;
            }
        }
        pp = &sb->next;
    }
    return 0;
}

static int rt_quic_fill_callbacks(ngtcp2_callbacks* cb, int is_server) {
    memset(cb, 0, sizeof(*cb));
    cb->client_initial = cb_debug_client_initial;
    cb->recv_client_initial = cb_debug_recv_client_initial;
    cb->recv_crypto_data = cb_debug_recv_crypto_data;
    cb->handshake_completed = cb_handshake_completed;
    cb->encrypt = ngtcp2_crypto_encrypt_cb;
    cb->decrypt = ngtcp2_crypto_decrypt_cb;
    cb->hp_mask = ngtcp2_crypto_hp_mask_cb;
    cb->recv_stream_data = cb_recv_stream_data;
    cb->acked_stream_data_offset = cb_acked_stream_data_offset;
    cb->recv_retry = cb_debug_recv_retry;
    cb->rand = cb_rand;
    cb->get_new_connection_id = cb_get_new_connection_id;
    cb->update_key = ngtcp2_crypto_update_key_cb;
    cb->stream_reset = cb_stream_reset;
    cb->delete_crypto_aead_ctx = ngtcp2_crypto_delete_crypto_aead_ctx_cb;
    cb->delete_crypto_cipher_ctx = ngtcp2_crypto_delete_crypto_cipher_ctx_cb;
    cb->get_path_challenge_data = ngtcp2_crypto_get_path_challenge_data_cb;
    cb->version_negotiation = ngtcp2_crypto_version_negotiation_cb;
    (void)is_server;
    return 0;
}

static void rt_quic_fill_params(ngtcp2_transport_params* params,
                                int32_t idle_timeout_ms) {
    ngtcp2_transport_params_default(params);
    params->initial_max_stream_data_bidi_local = 262144;
    params->initial_max_stream_data_bidi_remote = 262144;
    params->initial_max_stream_data_uni = 262144;
    params->initial_max_data = 1048576;
    params->initial_max_streams_bidi = 100;
    params->initial_max_streams_uni = 100;
    /* ngtcp2 时间单位 = 纳秒（NGTCP2_MILLISECONDS 换算）；直接填 ms 会把
       30s 解释为 30µs，导致空闲定时器立即到期、丢包重传判定错乱。 */
    params->max_idle_timeout =
        (uint64_t)(idle_timeout_ms > 0 ? idle_timeout_ms : 30000) *
        NGTCP2_MILLISECONDS;
}

/* 懒建服务器连接：首个 feed 解析客户端 Initial，取 SCID 为服务器 DCID。 */
static int rt_quic_server_conn_create(rt_quic_conn* rc, const uint8_t* pkt,
                                      size_t pktlen) {
    ngtcp2_pkt_hd hd;
    if (ngtcp2_accept(&hd, pkt, pktlen) != 0) {
        rt_quic_log("[quic] server_conn_create: ngtcp2_accept failed pktlen=%u\n",
                    (unsigned)pktlen);
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }
    /* 对端寻址 ID（出向报文 DCID）= 客户端 SCID（RFC 9000：服务器以客户端
       SCID 寻址客户端；ngtcp2 示例 server.c 以 hd.scid 传 ngtcp2_conn_server_new
       的 dcid 参数）。original_dcid（transport params）则是客户端 Initial 的
       原始 DCID。 */
    ngtcp2_cid_init(&rc->dcid, hd.scid.data, hd.scid.datalen);
    rt_quic_gen_cid(&rc->scid, RT_QUIC_MAX_CIDLEN);

    ngtcp2_callbacks cb;
    rt_quic_fill_callbacks(&cb, 1);
    ngtcp2_settings settings;
    ngtcp2_settings_default(&settings);
    settings.initial_ts = rt_quic_now();
    settings.handshake_timeout = 10 * NGTCP2_SECONDS;
    settings.no_pmtud = 1;
    settings.log_printf = cb_log_printf;
    ngtcp2_transport_params params;
    rt_quic_fill_params(&params, 30000);
    /* 服务器必须声明 original_dcid（客户端首个 Initial 的 DCID），
       否则 ngtcp2_conn_server_new 断言失败。 */
    params.original_dcid = hd.dcid;
    params.original_dcid_present = 1;

    int rv = ngtcp2_conn_server_new(&rc->conn, &rc->dcid, &rc->scid,
                                    &rc->path, hd.version, &cb, &settings,
                                    &params, NULL, rc);
    if (rv != 0) {
        rt_quic_log("[quic] server_conn_create: conn_server_new rv=%d (%s)\n",
                    rv, ngtcp2_strerror(rv));
        return rv;
    }

    /* set_local_transport_params 仅对服务器合法（客户端在 client_new 时已定）。 */
    rv = ngtcp2_conn_set_local_transport_params(rc->conn, &params);
    if (rv != 0) {
        rt_quic_log("[quic] server_conn_create: set_local_transport_params rv=%d\n", rv);
        return rv;
    }
    ngtcp2_conn_set_tls_native_handle(rc->conn, rc->ossl_ctx);
    rt_quic_log("[quic] server_conn_create: ok dcidlen=%u\n",
                (unsigned)rc->dcid.datalen);
    return 0;
}

/* ---- ABI：生命周期 ---- */

QUIC_ABI_EXPORT int64_t rt_quic_client_new(int32_t local_port,
                                           int32_t remote_port,
                                           const uint8_t* alpn,
                                           int32_t max_udp_payload,
                                           int32_t idle_timeout_ms) {
    if (ngtcp2_crypto_ossl_init() != 0) return 0;

    rt_quic_conn* rc = (rt_quic_conn*)calloc(1, sizeof(*rc));
    if (!rc) return 0;
    rc->is_server = 0;

    int32_t alpn_len = arr_len(alpn);
    if (alpn_len <= 0 || (size_t)alpn_len > sizeof(rc->alpn_wire)) {
        free(rc);
        return 0;
    }
    memcpy(rc->alpn_wire, alpn, (size_t)alpn_len);
    rc->alpn_wirelen = (size_t)alpn_len;

    rt_quic_alloc(rc, (uint16_t)local_port, (uint16_t)remote_port);

    rt_quic_gen_cid(&rc->dcid, RT_QUIC_MAX_CIDLEN);
    rt_quic_gen_cid(&rc->scid, RT_QUIC_MAX_CIDLEN);

    if (rt_quic_ssl_client_init(rc) != 0) {
        rt_quic_log("[quic] client_new: ssl_client_init failed\n");
        unsigned long e;
        while ((e = ERR_get_error()) != 0) {
            char buf[256];
            ERR_error_string_n(e, buf, sizeof(buf));
            rt_quic_log("[quic] openssl: %s\n", buf);
        }
        rt_quic_conn_free((int64_t)(intptr_t)rc);
        return 0;
    }
    if (ngtcp2_crypto_ossl_ctx_new(&rc->ossl_ctx, rc->ssl) != 0) {
        rt_quic_log("[quic] client_new: ossl_ctx_new failed\n");
        rt_quic_conn_free((int64_t)(intptr_t)rc);
        return 0;
    }

    ngtcp2_callbacks cb;
    rt_quic_fill_callbacks(&cb, 0);
    ngtcp2_settings settings;
    ngtcp2_settings_default(&settings);
    settings.initial_ts = rt_quic_now();
    settings.handshake_timeout = 10 * NGTCP2_SECONDS;
    if (max_udp_payload >= (int32_t)NGTCP2_MAX_UDP_PAYLOAD_SIZE &&
        max_udp_payload <= (int32_t)NGTCP2_MAX_TX_UDP_PAYLOAD_SIZE) {
        settings.max_tx_udp_payload_size = (size_t)max_udp_payload;
    }
    settings.no_pmtud = 1;
    settings.log_printf = cb_log_printf;
    ngtcp2_transport_params params;
    rt_quic_fill_params(&params, idle_timeout_ms);

    int rv = ngtcp2_conn_client_new(&rc->conn, &rc->dcid, &rc->scid,
                                    &rc->path, NGTCP2_PROTO_VER_V1, &cb,
                                    &settings, &params, NULL, rc);
    if (rv != 0) {
        rt_quic_log("[quic] client_new: ngtcp2_conn_client_new rv=%d (%s)\n",
                    rv, ngtcp2_strerror(rv));
        rt_quic_conn_free((int64_t)(intptr_t)rc);
        return 0;
    }
    ngtcp2_conn_set_tls_native_handle(rc->conn, rc->ossl_ctx);
    return (int64_t)(intptr_t)rc;
}

QUIC_ABI_EXPORT int64_t rt_quic_server_new(int32_t local_port,
                                           const uint8_t* alpn,
                                           const uint8_t* cert_pem,
                                           const uint8_t* key_pem,
                                           int32_t max_udp_payload,
                                           int32_t idle_timeout_ms) {
    (void)max_udp_payload; (void)idle_timeout_ms;
    if (ngtcp2_crypto_ossl_init() != 0) return 0;

    rt_quic_conn* rc = (rt_quic_conn*)calloc(1, sizeof(*rc));
    if (!rc) return 0;
    rc->is_server = 1;

    int32_t alpn_len = arr_len(alpn);
    if (alpn_len <= 0 || (size_t)alpn_len > sizeof(rc->alpn_wire)) {
        free(rc);
        return 0;
    }
    memcpy(rc->alpn_wire, alpn, (size_t)alpn_len);
    rc->alpn_wirelen = (size_t)alpn_len;

    int32_t cert_len = arr_len(cert_pem);
    int32_t key_len = arr_len(key_pem);
    if (cert_len <= 0 || key_len <= 0) {
        free(rc);
        return 0;
    }

    rt_quic_alloc(rc, (uint16_t)local_port, (uint16_t)local_port);

    if (rt_quic_ssl_server_init(rc, cert_pem, (size_t)cert_len, key_pem,
                                (size_t)key_len) != 0) {
        rt_quic_conn_free((int64_t)(intptr_t)rc);
        return 0;
    }
    if (ngtcp2_crypto_ossl_ctx_new(&rc->ossl_ctx, rc->ssl) != 0) {
        rt_quic_conn_free((int64_t)(intptr_t)rc);
        return 0;
    }
    /* 服务器 ngtcp2_conn 在首个客户端 Initial 到达时懒建 */
    return (int64_t)(intptr_t)rc;
}

QUIC_ABI_EXPORT void rt_quic_conn_free(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return;
    rt_quic_recv_buf* rb = rc->recv_head;
    while (rb) {
        rt_quic_recv_buf* next = rb->next;
        free(rb->data);
        free(rb);
        rb = next;
    }
    rt_quic_send_buf* sb = rc->send_head;
    while (sb) {
        rt_quic_send_buf* next = sb->next;
        free(sb->data);
        free(sb);
        sb = next;
    }
    if (rc->conn) {
        ngtcp2_conn_del(rc->conn);
        rc->conn = NULL;
    }
    if (rc->ossl_ctx) {
        ngtcp2_crypto_ossl_ctx_del(rc->ossl_ctx);
        rc->ossl_ctx = NULL;
    }
    if (rc->ssl) {
        /* ngtcp2_crypto_ossl_ctx_del 可能已释放 conn_ref 引用；清 app_data
         * 防 SSL_free 期间 backreference（ngtcp2_crypto_ossl.h 注记）。 */
        SSL_set_app_data(rc->ssl, NULL);
        SSL_free(rc->ssl);
        rc->ssl = NULL;
    }
    if (rc->ssl_ctx) {
        SSL_CTX_free(rc->ssl_ctx);
        rc->ssl_ctx = NULL;
    }
    free(rc);
}

/* ---- ABI：驱动 ---- */

QUIC_ABI_EXPORT int32_t rt_quic_conn_feed(int64_t handle, const uint8_t* pkt) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    int32_t pktlen = arr_len(pkt);
    if (!rc || pktlen <= 0 || !pkt) return NGTCP2_ERR_INVALID_ARGUMENT;

    if (rc->is_server && !rc->conn) {
        int rv = rt_quic_server_conn_create(rc, pkt, (size_t)pktlen);
        if (rv != 0) {
            rc->last_err = rv;
            rc->closed = 1;
            return rv;
        }
    }
    if (!rc->conn) return NGTCP2_ERR_INVALID_ARGUMENT;

    int rv = ngtcp2_conn_read_pkt(rc->conn, &rc->path, &rc->pi, pkt,
                                  (size_t)pktlen, rt_quic_now());
    if (rv != 0) {
        rc->last_err = rv;
        rt_quic_log("[quic] feed: read_pkt rv=%d (%s)\n", rv,
                    ngtcp2_strerror(rv));
        if (ngtcp2_err_is_fatal(rv) || rv == NGTCP2_ERR_DRAINING) {
            rc->closed = 1;
        }
        return rv;
    }
    rc->rx_any = 1;
    return 0;
}

QUIC_ABI_EXPORT int32_t rt_quic_conn_flush(int64_t handle, uint8_t* out) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !out || !rc->conn) return NGTCP2_ERR_INVALID_ARGUMENT;
    int32_t cap = arr_len(out);
    if (cap <= 0) return NGTCP2_ERR_INVALID_ARGUMENT;

    uint64_t now = rt_quic_now();
    size_t written = 0;

    /* 每份报文 = [u32 大端长度][payload]，直至 writev_stream 无更多输出 */
    for (;;) {
        /* 客户端事件门：已发出 Initial 且尚未收到服务器任何数据报时，停止继续写
           （ngtcp2 在 CS_CLIENT_INITIAL 状态下每次 writev_stream 都会重入
           client_initial_cb 并重装 Initial 密钥 + Retry AEAD，二次调用触发断言）。
           模型与 ngtcp2 示例一致：发完 Initial 后等待对端响应；PTO 到期由 poll
           处理，此时 gate 放行以支持重传。 */
        if (!rc->is_server && rc->initial_written && !rc->rx_any &&
            !rc->handshake_done && !(now >= ngtcp2_conn_get_expiry(rc->conn))) {
            break;
        }
        if ((size_t)cap - written < 4u + 8u) break; /* 至少留一帧空位 */

        ngtcp2_vec vec;
        ngtcp2_vec* datav = NULL;
        size_t datavcnt = 0;
        uint32_t flags = NGTCP2_WRITE_STREAM_FLAG_NONE;
        int64_t stream_id = -1;
        rt_quic_send_buf* sb = rc->send_head;
        while (sb && sb->handoff >= sb->len) sb = sb->next; /* 找仍有新数据的缓冲 */
        if (sb) {
            vec.base = sb->data + sb->handoff;
            vec.len = sb->len - sb->handoff;
            datav = &vec;
            datavcnt = 1;
            stream_id = sb->stream_id;
        }

        ngtcp2_pkt_info pi;
        memset(&pi, 0, sizeof(pi));
        ngtcp2_ssize pdatalen = 0;
        ngtcp2_ssize n = ngtcp2_conn_writev_stream(
            rc->conn, &rc->path, &pi, out + 4u + written, (size_t)cap - 4u - written,
            &pdatalen, flags, stream_id, datav, datavcnt, now);
        rt_quic_log("[quic] flush: writev_stream n=%lld pdatalen=%lld\n",
                    (long long)n, (long long)pdatalen);
        if (n < 0) {
            if (n == NGTCP2_ERR_WRITE_MORE) continue;
            if (n == NGTCP2_ERR_STREAM_DATA_BLOCKED) break; /* 流控窗口占满，等窗口更新 */
            rc->last_err = (int64_t)n;
            return (int32_t)n;
        }
        if (n == 0) break;

        if (pdatalen > 0 && sb) {
            /* 只推进"新数据"游标；缓冲本身保留到 ACK（acked_stream_data_offset
               释放），因为 ngtcp2 重传缓冲只引用应用数据不拷贝。 */
            sb->handoff += (size_t)pdatalen;
        }

        uint8_t* hdr = out + written;
        hdr[0] = (uint8_t)((uint32_t)n >> 24);
        hdr[1] = (uint8_t)((uint32_t)n >> 16);
        hdr[2] = (uint8_t)((uint32_t)n >> 8);
        hdr[3] = (uint8_t)(uint32_t)n;
        written += 4u + (size_t)n;

        if (!rc->is_server && !rc->handshake_done) {
            rc->initial_written = 1;
        }
        if (rc->closed && !rc->conn) break;
    }
    rt_quic_log("[quic] flush: wrote %u bytes\n", (unsigned)written);
    return (int32_t)written;
}

QUIC_ABI_EXPORT int32_t rt_quic_conn_poll(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !rc->conn) return NGTCP2_ERR_INVALID_ARGUMENT;
    uint64_t now = rt_quic_now();
    if (now >= ngtcp2_conn_get_expiry(rc->conn)) {
        int rv = ngtcp2_conn_handle_expiry(rc->conn, now);
        if (rv != 0) {
            rc->last_err = rv;
            rc->closed = 1;
            return rv;
        }
    }
    if (ngtcp2_conn_in_closing_period(rc->conn) ||
        ngtcp2_conn_in_draining_period(rc->conn)) {
        rc->closed = 1;
    }
    return 0;
}

QUIC_ABI_EXPORT int32_t rt_quic_conn_expiry_due(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !rc->conn) return 0;
    return rt_quic_now() >= ngtcp2_conn_get_expiry(rc->conn) ? 1 : 0;
}

QUIC_ABI_EXPORT int32_t rt_quic_conn_handshake_completed(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return 0;
    return rc->handshake_done ? 1 : 0;
}

QUIC_ABI_EXPORT int32_t rt_quic_conn_is_active(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return 0;
    if (rc->closed) return 0;
    if (!rc->conn) return 1; /* 服务器等待客户端 Initial */
    return 1;
}

QUIC_ABI_EXPORT int64_t rt_quic_conn_last_error(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return 0;
    return rc->last_err;
}

/* ---- ABI：流 ---- */

QUIC_ABI_EXPORT int64_t rt_quic_stream_open_bidi(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !rc->conn) return NGTCP2_ERR_INVALID_ARGUMENT;
    int64_t stream_id = -1;
    int rv = ngtcp2_conn_open_bidi_stream(rc->conn, &stream_id, NULL);
    if (rv != 0) {
        rc->last_err = rv;
        return rv;
    }
    return stream_id;
}

/* HTTP/3 控制流等单向流（RFC 9114 §6.2：客户端控制流 id=2、服务器 id=3）。 */
QUIC_ABI_EXPORT int64_t rt_quic_stream_open_uni(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !rc->conn) return NGTCP2_ERR_INVALID_ARGUMENT;
    int64_t stream_id = -1;
    int rv = ngtcp2_conn_open_uni_stream(rc->conn, &stream_id, NULL);
    if (rv != 0) {
        rc->last_err = rv;
        return rv;
    }
    return stream_id;
}

QUIC_ABI_EXPORT int32_t rt_quic_stream_send(int64_t handle, int64_t stream_id,
                                            const uint8_t* data) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    int32_t datalen = arr_len(data);
    if (!rc || !rc->conn || stream_id < 0 || datalen < 0) {
        return NGTCP2_ERR_INVALID_ARGUMENT;
    }
    if (datalen == 0) return 0;

    rt_quic_send_buf* sb = (rt_quic_send_buf*)calloc(1, sizeof(*sb));
    if (!sb) return NGTCP2_ERR_NOMEM;
    sb->stream_id = stream_id;
    sb->len = (size_t)datalen;
    sb->data = (uint8_t*)malloc((size_t)datalen);
    if (!sb->data) {
        free(sb);
        return NGTCP2_ERR_NOMEM;
    }
    memcpy(sb->data, data, (size_t)datalen);
    /* base = 该流先前已入队缓冲的总字节数（ACK 回调按流字节空间对齐） */
    size_t base = 0;
    rt_quic_send_buf* it = rc->send_head;
    while (it) {
        if (it->stream_id == stream_id) base += it->len;
        it = it->next;
    }
    sb->base = base;

    if (rc->send_tail) {
        rc->send_tail->next = sb;
    } else {
        rc->send_head = sb;
    }
    rc->send_tail = sb;
    return 0;
}

QUIC_ABI_EXPORT int32_t rt_quic_stream_recv(int64_t handle, int64_t stream_id,
                                            uint8_t* out) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !out) return NGTCP2_ERR_INVALID_ARGUMENT;
    int32_t cap = arr_len(out);
    if (cap <= 0) return 0;

    rt_quic_recv_buf** pp = &rc->recv_head;
    while (*pp && (*pp)->stream_id != stream_id) pp = &(*pp)->next;
    rt_quic_recv_buf* buf = *pp;
    if (!buf || buf->len == 0) return 0;

    size_t n = buf->len < (size_t)cap ? buf->len : (size_t)cap;
    memcpy(out, buf->data, n);
    if (n < buf->len) {
        memmove(buf->data, buf->data + n, buf->len - n);
        buf->len -= n;
    } else {
        free(buf->data);
        *pp = buf->next;
        free(buf);
    }
    return (int32_t)n;
}

QUIC_ABI_EXPORT int32_t rt_quic_stream_has_data(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return 0;
    rt_quic_recv_buf* buf = rc->recv_head;
    while (buf) {
        if (buf->len > 0) return 1;
        buf = buf->next;
    }
    return 0;
}

QUIC_ABI_EXPORT int64_t rt_quic_stream_peek_id(int64_t handle) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc) return -1;
    rt_quic_recv_buf* buf = rc->recv_head;
    while (buf) {
        if (buf->len > 0) return buf->stream_id;
        buf = buf->next;
    }
    return -1;
}

/* 断言被丢弃的 STREAM 帧所在报文数（丢包重传 e2e 的损失证据）。 */
QUIC_ABI_EXPORT int64_t rt_quic_conn_stream_loss_count(int64_t handle,
                                                       int64_t stream_id) {
    rt_quic_conn* rc = rt_quic_from_handle(handle);
    if (!rc || !rc->conn || stream_id < 0) return 0;
    return (int64_t)ngtcp2_conn_get_stream_loss_count2(rc->conn, stream_id);
}
