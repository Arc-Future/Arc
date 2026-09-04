// RFC 042 M5: Noise_XK handshake + ChaCha20-Poly1305 transport session.
//
// Self-contained C11 implementation of the Noise Protocol Framework
// XK handshake pattern (1-RTT authentication) and post-handshake
// transport encryption.  Session state is heap-allocated (opaque handle),
// following the same pattern as rt_socket_* / rt_task_*.
//
// Uses X25519 for DH, ChaCha20-Poly1305 for AEAD, SHA-256 for HMAC/HKDF.
// Thread-safe per-session (no global state).  All ABI functions return
// 0 on success, -1 on error.
//
// Correctness contract: this implementation is verified byte-for-byte
// against the Noise Protocol Framework official test vectors
// (cacophony Noise_XK_25519_ChaChaPoly_SHA256; verified via the retired
// noise_xk_vectors_e2e, arc-integration removed in a2627a0f).  Message layout,
// MixHash/MixKey ordering, EncryptAndHash/DecryptAndHash semantics and the
// Split key schedule follow the spec exactly:
//
//   XK pre-message:  <- s
//   msg1 (I -> R):   -> e, es
//   msg2 (R -> I):   <- e, ee
//   msg3 (I -> R):   -> s, se
//   Split:           k1 = initiator->responder, k2 = responder->initiator
//
// Public ABI entries are kept (frozen surface); their internal behaviour
// was corrected to be spec-compliant (msg1 = e.pub + empty-payload AEAD
// tag = 48 bytes; initiator-finalize verifies the msg2 tag before mixing
// `se`; responder now consumes msg3 via rt_noise_respond_finalize).
// New test/vector support symbols use new names (rt_noise_session_set_ephemeral,
// rt_noise_handshake_write/read, rt_noise_respond_finalize,
// rt_noise_session_last_msg, rt_noise_session_handshake_hash).

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ===================================================================
 *  Session state (opaque — caller sees void*)
 * =================================================================== */

typedef struct {
    uint8_t  ck[32];        /* chaining key */
    uint8_t  h[32];         /* handshake hash */
    uint8_t  k[32];         /* current cipher key (handshake phase) */
    int      k_set;         /* 1 once MixKey has derived k (EncryptAndHash k-empty branch) */
    uint8_t  nonce[12];     /* handshake nonce (little-endian 64-bit counter) */
    uint8_t  s_sk[32];      /* local static private key */
    uint8_t  s_pk[32];      /* local static public key */
    uint8_t  rs_pk[32];     /* remote static public key (initiator: responder static;
                               responder: overwritten with initiator static on msg3) */
    uint8_t  e_sk[32];      /* local ephemeral private key */
    uint8_t  e_pk[32];      /* local ephemeral public key */
    uint8_t  re_pk[32];     /* remote ephemeral public key */
    uint8_t  k_send[32];    /* transport send key (Split output) */
    uint8_t  k_recv[32];    /* transport recv key (Split output) */
    uint8_t  nonce_send[12];/* transport send nonce */
    uint8_t  nonce_recv[12];/* transport recv nonce */
    uint8_t  last_msg[128]; /* last written handshake message (e.g. msg3 for the responder) */
    int      last_len;
    int      initiator;
    int      step;          /* 0/1/2 = next handshake message index (XK has three) */
    int      done;          /* handshake complete (Split done) */
    int      pm_done;       /* pre-message mixed into h (lazy, after optional prologue) */
    int      e_set;         /* deterministic ephemeral injected via rt_noise_session_set_ephemeral */
} NoiseSession;

/* ===================================================================
 *  Raw SHA-256 + HMAC-SHA256 (FIPS 180-4, FIPS 198-1)
 *
 *  Self-contained C11 implementation operating directly on raw byte
 *  buffers.  This replaces the previous approach of hex-encoding data,
 *  calling rt_crypto_sha256 (which hashed the hex string rather than
 *  the original bytes), and hex-decoding the result — a correctness bug
 *  that produced entirely wrong hashes.
 * =================================================================== */

static inline uint32_t shr32(uint32_t x, int n) { return (x >> n) | (x << (32 - n)); }

typedef struct {
    uint32_t state[8];
    uint64_t count;
    uint8_t  buf[64];
} sha256_ctx_raw;

static const uint32_t sha256_K_raw[64] = {
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
};

#define CH(x,y,z)  (((x) & (y)) ^ (~(x) & (z)))
#define MAJ(x,y,z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))
#define BSIG0(x)   (shr32((x),2) ^ shr32((x),13) ^ shr32((x),22))
#define BSIG1(x)   (shr32((x),6) ^ shr32((x),11) ^ shr32((x),25))
#define SSIG0(x)   (shr32((x),7) ^ shr32((x),18) ^ ((x) >> 3))
#define SSIG1(x)   (shr32((x),17) ^ shr32((x),19) ^ ((x) >> 10))

static void sha256_raw_init(sha256_ctx_raw* ctx) {
    ctx->state[0] = 0x6a09e667; ctx->state[1] = 0xbb67ae85;
    ctx->state[2] = 0x3c6ef372; ctx->state[3] = 0xa54ff53a;
    ctx->state[4] = 0x510e527f; ctx->state[5] = 0x9b05688c;
    ctx->state[6] = 0x1f83d9ab; ctx->state[7] = 0x5be0cd19;
    ctx->count = 0;
}

static void sha256_raw_transform(sha256_ctx_raw* ctx, const uint8_t data[64]) {
    uint32_t W[64], a, b, c, d, e, f, g, h;
    for (int i = 0; i < 16; i++)
        W[i] = ((uint32_t)data[i*4] << 24) | ((uint32_t)data[i*4+1] << 16) |
               ((uint32_t)data[i*4+2] << 8) | (uint32_t)data[i*4+3];
    for (int i = 16; i < 64; i++)
        W[i] = SSIG1(W[i-2]) + W[i-7] + SSIG0(W[i-15]) + W[i-16];
    a = ctx->state[0]; b = ctx->state[1]; c = ctx->state[2]; d = ctx->state[3];
    e = ctx->state[4]; f = ctx->state[5]; g = ctx->state[6]; h = ctx->state[7];
    for (int i = 0; i < 64; i++) {
        uint32_t T1 = h + BSIG1(e) + CH(e,f,g) + sha256_K_raw[i] + W[i];
        uint32_t T2 = BSIG0(a) + MAJ(a,b,c);
        h = g; g = f; f = e; e = d + T1; d = c; c = b; b = a; a = T1 + T2;
    }
    ctx->state[0] += a; ctx->state[1] += b; ctx->state[2] += c; ctx->state[3] += d;
    ctx->state[4] += e; ctx->state[5] += f; ctx->state[6] += g; ctx->state[7] += h;
}

static void sha256_raw_update(sha256_ctx_raw* ctx, const uint8_t* data, size_t len) {
    uint32_t idx = (uint32_t)(ctx->count & 0x3f);
    ctx->count += len;
    uint32_t space = 64 - idx;
    if (len >= space) {
        memcpy(ctx->buf + idx, data, space);
        sha256_raw_transform(ctx, ctx->buf);
        for (size_t off = space; off + 64 <= len; off += 64)
            sha256_raw_transform(ctx, data + off);
        memcpy(ctx->buf, data + len - ((len - space) & 63), (len - space) & 63);
    } else {
        memcpy(ctx->buf + idx, data, len);
    }
}

static void sha256_raw_final(sha256_ctx_raw* ctx, uint8_t out[32]) {
    uint64_t bits = ctx->count * 8;
    uint32_t idx = (uint32_t)(ctx->count & 0x3f);
    ctx->buf[idx++] = 0x80;
    if (idx > 56) {
        memset(ctx->buf + idx, 0, 64 - idx);
        sha256_raw_transform(ctx, ctx->buf);
        idx = 0;
    }
    memset(ctx->buf + idx, 0, 56 - idx);
    for (int i = 0; i < 8; i++) ctx->buf[56 + i] = (uint8_t)(bits >> (56 - 8*i));
    sha256_raw_transform(ctx, ctx->buf);
    for (int i = 0; i < 8; i++) {
        out[i*4]     = (uint8_t)(ctx->state[i] >> 24);
        out[i*4 + 1] = (uint8_t)(ctx->state[i] >> 16);
        out[i*4 + 2] = (uint8_t)(ctx->state[i] >> 8);
        out[i*4 + 3] = (uint8_t)(ctx->state[i]);
    }
}

/* HMAC-SHA256 (FIPS 198-1) operating on raw byte buffers. */
static void raw_hmac_sha256(const uint8_t* key, const uint8_t* data, int dlen,
                             uint8_t out32[32]) {
    uint8_t k_ipad[64], k_opad[64];
    memset(k_ipad, 0x36, 64);
    memset(k_opad, 0x5c, 64);
    for (int i = 0; i < 32; i++) {
        k_ipad[i] ^= key[i];
        k_opad[i] ^= key[i];
    }
    sha256_ctx_raw ctx;
    sha256_raw_init(&ctx);
    sha256_raw_update(&ctx, k_ipad, 64);
    sha256_raw_update(&ctx, data, (size_t)dlen);
    uint8_t inner[32];
    sha256_raw_final(&ctx, inner);
    sha256_raw_init(&ctx);
    sha256_raw_update(&ctx, k_opad, 64);
    sha256_raw_update(&ctx, inner, 32);
    sha256_raw_final(&ctx, out32);
}

/* SHA-256 operating on raw byte buffers. */
static void raw_sha256(const uint8_t* data, int len, uint8_t out32[32]) {
    sha256_ctx_raw ctx;
    sha256_raw_init(&ctx);
    sha256_raw_update(&ctx, data, (size_t)len);
    sha256_raw_final(&ctx, out32);
}

/* HKDF-Extract(salt32, ikm, ikm_len) → prk32 */
static void hkdf_extract(const uint8_t salt[32], const uint8_t* ikm, int ikm_len,
                          uint8_t prk[32]) {
    raw_hmac_sha256(salt, ikm, ikm_len, prk);
}

/* HKDF-Expand(prk32, info, info_len) → okm32 (single 32-byte output, T(1)) */
static void hkdf_expand(const uint8_t prk[32], const uint8_t* info, int info_len,
                         uint8_t okm[32]) {
    uint8_t buf[1056];
    memcpy(buf, info, info_len);
    buf[info_len] = 0x01;
    raw_hmac_sha256(prk, buf, info_len + 1, okm);
}

/* HKDF(ck, ikm, 2): two 32-byte outputs (Noise MixKey / Split schedule).
 * out1 = T(1), out2 = T(2) per the Noise spec §5.2. */
static void hkdf2(const uint8_t ck[32], const uint8_t* ikm, int ikm_len,
                  uint8_t out1[32], uint8_t out2[32]) {
    uint8_t prk[32];
    hkdf_extract(ck, ikm, ikm_len, prk);
    hkdf_expand(prk, (const uint8_t*)"", 0, out1);
    uint8_t t1[33];
    memcpy(t1, out1, 32);
    t1[32] = 0x02;
    raw_hmac_sha256(prk, t1, 33, out2);
}

/* ===================================================================
 *  Noise internal helpers (spec §5.2 SymmetricState / §7 message processing)
 * =================================================================== */

/* Incremental MixHash: h = SHA-256(h || data). */
static void noise_mix_hash(NoiseSession* s, const uint8_t* data, int len) {
    sha256_ctx_raw ctx;
    sha256_raw_init(&ctx);
    sha256_raw_update(&ctx, s->h, 32);
    sha256_raw_update(&ctx, data, (size_t)len);
    sha256_raw_final(&ctx, s->h);
}

/* MixKey(ikm): (ck, k) = HKDF(ck, ikm, 2); nonce := 0. */
static void noise_mix_key(NoiseSession* s, const uint8_t ikm[32]) {
    uint8_t out1[32], out2[32];
    hkdf2(s->ck, ikm, 32, out1, out2);
    memcpy(s->ck, out1, 32);
    memcpy(s->k, out2, 32);
    s->k_set = 1;
    memset(s->nonce, 0, 12);
}

static void noise_nonce_inc(uint8_t nonce[12]) {
    /* Noise 64-bit counter lives in bytes 4..11 of the 12-byte nonce
     * (bytes 0..3 are the fixed zero prefix per the spec's SetNonce). */
    uint64_t n;
    memcpy(&n, nonce + 4, 8);
    n++;
    memcpy(nonce + 4, &n, 8);
}

/* EncryptAndHash(plaintext): if k set → AEAD(k, n++, h, pt); MixHash(ciphertext).
 * Returns bytes written to out. */
static int noise_encrypt_and_hash(NoiseSession* s,
                                  const uint8_t* plaintext, int pt_len,
                                  uint8_t* out) {
    if (s->k_set) {
        /* The AEAD requires a non-NULL plaintext pointer even for zero-length
         * payloads; route empty payloads through a static dummy buffer. */
        static const uint8_t noise_empty[16] = {0};
        const uint8_t* pt = plaintext ? plaintext : noise_empty;
        rt_crypto_aead_encrypt(pt, (uint32_t)pt_len, s->k, s->nonce,
                               s->h, 32, out, out + pt_len);
        noise_nonce_inc(s->nonce);
        noise_mix_hash(s, out, pt_len + 16);
        return pt_len + 16;
    }
    memcpy(out, plaintext, (size_t)pt_len);
    noise_mix_hash(s, out, pt_len);
    return pt_len;
}

/* DecryptAndHash(ciphertext): if k set → AEAD-verify(k, n++, h, ct||tag);
 * MixHash(ciphertext).  Returns plaintext length or -1 on AEAD failure. */
static int noise_decrypt_and_hash(NoiseSession* s,
                                  const uint8_t* in, int in_len,
                                  uint8_t* out) {
    if (s->k_set) {
        int r = rt_crypto_aead_decrypt(in, (uint32_t)(in_len - 16), s->k, s->nonce,
                                       s->h, 32, in + in_len - 16, out);
        if (r != 0) return -1;
        noise_nonce_inc(s->nonce);
        noise_mix_hash(s, in, in_len);
        return in_len - 16;
    }
    memcpy(out, in, (size_t)in_len);
    noise_mix_hash(s, in, in_len);
    return in_len;
}

/* Split: two transport CipherStates (initiator sends c1/recv c2; responder vice versa).
 * Per Noise spec §5.2: (temp_k1, temp_k2) = HKDF(ck, zerolen, 2) with zerolen being
 * a ZERO-LENGTH byte sequence — the extract step is HMAC(ck, ""), NOT HMAC(ck, 32
 * zero bytes). This matches the cacophony/noise-c reference vectors (rev 30+). */
static int noise_split(NoiseSession* s) {
    static const uint8_t dummy_zerolen = 0;
    uint8_t out1[32], out2[32];
    hkdf2(s->ck, &dummy_zerolen, 0, out1, out2);   /* temp_k1 = out1, temp_k2 = out2 */
    if (s->initiator) {
        memcpy(s->k_send, out1, 32);
        memcpy(s->k_recv, out2, 32);
    } else {
        memcpy(s->k_send, out2, 32);
        memcpy(s->k_recv, out1, 32);
    }
    memset(s->nonce_send, 0, 12);
    memset(s->nonce_recv, 0, 12);
    s->done = 1;
    return 0;
}

static void gen_x25519_keypair(uint8_t pk[32], uint8_t sk[32]) {
    /* 共享 CSPRNG（Windows: BCryptGenRandom/RtlGenRandom；POSIX: /dev/urandom）。
     * 失败时清零输出——绝不降级到 rand()（密码学场景下 rand() 是安全漏洞）。 */
    if (rt_crypto_csprng_bytes(sk, 32) != 0) {
        memset(sk, 0, 32);
        memset(pk, 0, 32);
        return;
    }
    uint8_t base[32] = {9};
    rt_crypto_x25519_dh(sk, base, pk);
}

#define PROTO_NAME "Noise_XK_25519_ChaChaPoly_SHA256"

/* ===================================================================
 *  XK message engine (pattern tokens: [E,ES] / [E,EE] / [S,SE])
 * =================================================================== */

/* Validate that this party may write the handshake message at s->step. */
static int noise_can_write(const NoiseSession* s) {
    return (s->initiator && (s->step == 0 || s->step == 2)) ||
           (!s->initiator && s->step == 1);
}

/* Validate that this party may read the handshake message at s->step. */
static int noise_can_read(const NoiseSession* s) {
    return (!s->initiator && (s->step == 0 || s->step == 2)) ||
           (s->initiator && s->step == 1);
}

static int noise_write_message(NoiseSession* s, const uint8_t* payload, int plen,
                               uint8_t* out, int out_max) {
    if (!s || s->done || !noise_can_write(s) || plen < 0) return -1;
    if (!out || out_max < 0) return -1;

    /* Pre-message `<- s` is mixed lazily on the first handshake message so an
     * optional prologue (spec Initialize) can be mixed first. */
    if (!s->pm_done) {
        /* pre-message `<- s`: responder's static — rs_pk on the initiator,
         * our own s_pk on the responder (mirror of noise_write_message). */
        noise_mix_hash(s, s->initiator ? s->rs_pk : s->s_pk, 32);
        s->pm_done = 1;
    }

    /* Wire size: step0/1 = e.pub(32) + payload + tag(16) (k set after DH token);
     * step2 = s ciphertext(32+16) + payload + tag(16). */
    int need = (s->step == 2) ? 48 + plen + 16 : 32 + plen + 16;
    if (out_max < need) return -1;

    if (s->step == 0 || s->step == 1) {
        /* token e: use a deterministic injected ephemeral if present
         * (official-vector tests), otherwise generate a fresh keypair */
        if (s->e_set) {
            s->e_set = 0;   /* consume the injected ephemeral */
        } else {
            gen_x25519_keypair(s->e_pk, s->e_sk);
        }
        memcpy(out, s->e_pk, 32);
        noise_mix_hash(s, s->e_pk, 32);
        out += 32;
    } else {
        /* token s (initiator step2): EncryptAndHash(local static public) */
        int n = noise_encrypt_and_hash(s, s->s_pk, 32, out);
        if (n <= 0) return -1;
        out += n;
    }

    /* DH token: es (step0) / ee (step1) / se (step2, initiator) */
    uint8_t dh[32];
    if (s->step == 0) {
        rt_crypto_x25519_dh(s->e_sk, s->rs_pk, dh);   /* es: DH(e, rs) */
        noise_mix_key(s, dh);
    } else if (s->step == 1) {
        rt_crypto_x25519_dh(s->e_sk, s->re_pk, dh);   /* ee: DH(e, re) */
        noise_mix_key(s, dh);
    } else {
        rt_crypto_x25519_dh(s->s_sk, s->re_pk, dh);   /* se: DH(s, re) */
        noise_mix_key(s, dh);
    }

    /* payload: EncryptAndHash(payload) */
    int n = noise_encrypt_and_hash(s, payload, plen, out);
    out += n;

    if ((size_t)need <= sizeof(s->last_msg)) {
        uint8_t* msg = out - need;
        memcpy(s->last_msg, msg, (size_t)need);
        s->last_len = need;
    } else {
        s->last_len = -1;
    }

    if (s->step == 2) {
        /* msg3 written: handshake complete → Split */
        return noise_split(s) == 0 ? need : -1;
    }
    s->step++;
    return need;
}

static int noise_read_message(NoiseSession* s, const uint8_t* in, int in_len,
                              uint8_t* out_payload, int out_payload_max) {
    if (!s || s->done || !noise_can_read(s) || !in || in_len < 0) return -1;

    if (!s->pm_done) {
        /* `<- s` on the read side mirrors the write side: the responder mixes
         * its own static (its rs_pk still holds the remote initiator static
         * until msg3 overwrites it). */
        noise_mix_hash(s, s->initiator ? s->rs_pk : s->s_pk, 32);
        s->pm_done = 1;
    }

    int need = (s->step == 2) ? 48 : 32;
    if (in_len < need) return -1;

    const uint8_t* p = in;
    if (s->step == 0 || s->step == 1) {
        /* token e: re = in[:32]; MixHash(re) */
        memcpy(s->re_pk, p, 32);
        noise_mix_hash(s, s->re_pk, 32);
        p += 32;
    } else {
        /* token s (responder step2): DecryptAndHash(48B) → initiator static pub;
         * verify AEAD tag → store as rs (used by the se DH below). */
        int r = noise_decrypt_and_hash(s, p, 48, s->rs_pk);
        if (r != 32) return -1;
        p += 48;
    }

    /* DH token: es (step0, responder) / ee (step1, initiator) / se (step2, responder) */
    uint8_t dh[32];
    if (s->step == 0) {
        rt_crypto_x25519_dh(s->s_sk, s->re_pk, dh);   /* es: DH(s, re) */
        noise_mix_key(s, dh);
    } else if (s->step == 1) {
        rt_crypto_x25519_dh(s->e_sk, s->re_pk, dh);   /* ee: DH(e, re) */
        noise_mix_key(s, dh);
    } else {
        rt_crypto_x25519_dh(s->e_sk, s->rs_pk, dh);   /* se: DH(e, rs) — rs now = initiator static */
        noise_mix_key(s, dh);
    }

    /* payload: DecryptAndHash(remaining) */
    int rem = in_len - (int)(p - in);
    uint8_t scratch[512];
    uint8_t* dst = out_payload;
    if (!dst || out_payload_max < (rem > 0 ? rem - 16 : 0)) {
        if (rem - 16 > (int)sizeof(scratch)) return -1;
        dst = scratch;
    }
    int plen = noise_decrypt_and_hash(s, p, rem, dst);
    if (plen < 0) return -1;

    if (s->step == 2) {
        /* msg3 read: handshake complete → Split */
        return noise_split(s) == 0 ? plen : -1;
    }
    s->step++;
    return plen;
}

/* ===================================================================
 *  Public ABI (frozen surface kept; behaviour now spec-compliant)
 * =================================================================== */

void* rt_noise_session_create(const uint8_t local_sk[32], const uint8_t remote_pk[32],
                               int initiator) {
    NoiseSession* s = (NoiseSession*)malloc(sizeof(NoiseSession));
    if (!s) return NULL;
    memset(s, 0, sizeof(NoiseSession));
    s->last_len = -1;

    /* Initialize handshake: name is 32 bytes (≤ HASHLEN), so per the Noise
     * spec InitializeSymmetric: h = protocol_name (raw, zero-padded to
     * HASHLEN) and ck = h.  Not HASH(name) — hashing here was a correctness
     * bug (wrong chaining key → wrong HKDF output → wrong AEAD keys). */
    memcpy(s->ck, PROTO_NAME, 32);
    memcpy(s->h, PROTO_NAME, 32);

    memcpy(s->s_sk, local_sk, 32);
    uint8_t base[32] = {9};
    rt_crypto_x25519_dh(s->s_sk, base, s->s_pk);
    memcpy(s->rs_pk, remote_pk, 32);
    s->initiator = initiator;
    /* Pre-message `<- s` is mixed lazily on the first handshake message (see
     * noise_write_message/noise_read_message) so an optional prologue can be
     * mixed first, per the Noise spec Initialize ordering. */

    return s;
}

void rt_noise_session_destroy(void* session) {
    if (session) free(session);
}

/* Initiator msg1 (→ e, es + empty-payload tag). Returns 48 or -1. */
int rt_noise_initiate_handshake(void* session, uint8_t* out_msg, int out_max) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || s->initiator != 1 || s->done || s->step != 0) return -1;
    return noise_write_message(s, NULL, 0, out_msg, out_max);
}

/* Responder: read msg1, write msg2 (← e, ee + tag). Returns 48 or -1. */
int rt_noise_respond_handshake(void* session,
                                const uint8_t* in_msg, int in_len,
                                uint8_t* out_msg, int out_max) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || s->initiator != 0 || s->done || s->step != 0) return -1;
    uint8_t payload[512];
    int plen = noise_read_message(s, in_msg, in_len, payload, sizeof(payload));
    if (plen < 0) return -1;
    return noise_write_message(s, NULL, 0, out_msg, out_max);
}

/* Initiator: read msg2 (verify tag), write msg3 internally (retrievable via
 * rt_noise_session_last_msg), complete handshake (Split). Returns 0 or -1. */
int rt_noise_initiate_finalize(void* session, const uint8_t* in_msg, int in_len) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || s->initiator != 1 || s->done || s->step != 1) return -1;
    uint8_t payload[512];
    int plen = noise_read_message(s, in_msg, in_len, payload, sizeof(payload));
    if (plen < 0) return -1;
    if (noise_write_message(s, NULL, 0, s->last_msg, (int)sizeof(s->last_msg)) < 0) {
        return -1;
    }
    return 0;
}

/* Responder: read msg3 (verify initiator static + se), complete handshake (Split). */
int rt_noise_respond_finalize(void* session, const uint8_t* in_msg, int in_len) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || s->initiator != 0 || s->done || s->step != 2) return -1;
    uint8_t payload[512];
    int plen = noise_read_message(s, in_msg, in_len, payload, sizeof(payload));
    if (plen < 0) return -1;
    return 0;
}

/* Transport encrypt (post-handshake): AEAD(k_send, n++, ad=""). */
int rt_noise_session_encrypt(void* session,
                              const uint8_t* plaintext, int pt_len,
                              uint8_t* out_ciphertext, uint8_t out_tag[16]) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || !s->done || !plaintext || !out_ciphertext || !out_tag || pt_len < 0) return -1;
    int r = rt_crypto_aead_encrypt(plaintext, (uint32_t)pt_len, s->k_send,
                                   s->nonce_send, NULL, 0,
                                   out_ciphertext, out_tag);
    if (r != 0) return -1;
    noise_nonce_inc(s->nonce_send);
    return pt_len;
}

/* Transport decrypt (post-handshake): AEAD-verify(k_recv, n++, ad=""). */
int rt_noise_session_decrypt(void* session,
                              const uint8_t* ciphertext, int ct_len,
                              const uint8_t tag[16],
                              uint8_t* out_plaintext) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || !s->done || !ciphertext || !tag || !out_plaintext || ct_len < 0) return -1;
    int r = rt_crypto_aead_decrypt(ciphertext, (uint32_t)ct_len, s->k_recv,
                                   s->nonce_recv, NULL, 0, tag,
                                   out_plaintext);
    if (r != 0) return -1;
    noise_nonce_inc(s->nonce_recv);
    return ct_len;
}

/* ===================================================================
 *  Test / vector support ABI (new symbols — for Noise Protocol Framework
 *  official-vector verification; not part of the frozen surface)
 * =================================================================== */

/* Inject a deterministic ephemeral private key (clamped per RFC 7748) and
 * derive its public key.  Used by cacophony official-vector tests. */
int rt_noise_session_set_ephemeral(void* session, const uint8_t sk[32]) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || !sk) return -1;
    memcpy(s->e_sk, sk, 32);
    uint8_t base[32] = {9};
    rt_crypto_x25519_dh(s->e_sk, base, s->e_pk);
    s->e_set = 1;
    return 0;
}

/* Mix a prologue into the handshake hash (spec Initialize: h = SHA-256(h || p)).
 * Must be called before any handshake message.  Used by cacophony vectors. */
int rt_noise_session_set_prologue(void* session, const uint8_t* prologue, int len) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || s->step != 0 || s->done) return -1;
    if (prologue && len > 0) noise_mix_hash(s, prologue, len);
    return 0;
}

/* Generic spec-correct handshake write with payload (both roles). */
int rt_noise_handshake_write(void* session, const uint8_t* payload, int payload_len,
                             uint8_t* out, int out_max) {
    NoiseSession* s = (NoiseSession*)session;
    return noise_write_message(s, payload, payload_len, out, out_max);
}

/* Generic spec-correct handshake read with payload (both roles).  Returns the
 * decrypted payload length, or -1 on AEAD/format failure. */
int rt_noise_handshake_read(void* session, const uint8_t* in_msg, int in_len,
                            uint8_t* out_payload, int out_payload_max) {
    NoiseSession* s = (NoiseSession*)session;
    return noise_read_message(s, in_msg, in_len, out_payload, out_payload_max);
}

/* Retrieve the last handshake message this session wrote (e.g. the initiator's
 * msg3, needed by the responder to complete the handshake). */
int rt_noise_session_last_msg(void* session, uint8_t* out, int out_max) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || !out || s->last_len <= 0 || out_max < s->last_len) return -1;
    memcpy(out, s->last_msg, (size_t)s->last_len);
    return s->last_len;
}

/* Current handshake hash h (for official-vector handshake_hash assertions). */
int rt_noise_session_handshake_hash(void* session, uint8_t out[32]) {
    NoiseSession* s = (NoiseSession*)session;
    if (!s || !out) return -1;
    memcpy(out, s->h, 32);
    return 0;
}

/* ===================================================================
 *  RtArray byte[] wrappers (P0-2: byte[] facade over the frozen ABI)
 * =================================================================== */

/* Copy `len` bytes into a freshly created RtArray byte[] (elem_size = 1). */
static void* noise_arr_from(const uint8_t* data, int32_t len) {
    void* out = rt_array_create(len, 1);
    if (len > 0) memcpy(out, data, (size_t)len);
    return out;
}

/* View an RtArray byte[] payload as a raw byte span (NULL → empty). */
static int32_t noise_arr_bytes(void* arr, const uint8_t** out_data) {
    static const uint8_t k_empty[1] = { 0 };
    if (!arr) { *out_data = k_empty; return 0; }
    *out_data = (const uint8_t*)arr;
    return rt_array_length(arr);
}

/* byte[]-facade ABI (P0-2): array inputs are RtArray byte[] payloads, array
 * outputs are freshly created RtArray byte[] (NULL = failure).
 * create_arr returns the opaque session handle (not an RtArray; carried
 * through Arc string handles).  respond_finalize_arr returns 0/-1. */

void* rt_noise_session_create_arr(void* sk_arr, void* pk_arr, int32_t initiator) {
    const uint8_t* sk;
    const uint8_t* pk;
    if (noise_arr_bytes(sk_arr, &sk) != 32 || noise_arr_bytes(pk_arr, &pk) != 32) {
        return NULL;
    }
    return rt_noise_session_create(sk, pk, initiator);
}

void* rt_noise_initiate_handshake_arr(void* session) {
    uint8_t buf[128];
    int n = rt_noise_initiate_handshake(session, buf, (int)sizeof(buf));
    if (n < 0) return NULL;
    return noise_arr_from(buf, n);
}

void* rt_noise_respond_handshake_arr(void* session, void* in_msg_arr) {
    const uint8_t* in;
    uint8_t buf[128];
    int in_len = noise_arr_bytes(in_msg_arr, &in);
    int n = rt_noise_respond_handshake(session, in, in_len, buf, (int)sizeof(buf));
    if (n < 0) return NULL;
    return noise_arr_from(buf, n);
}

/* Initiator: read msg2, write msg3 internally, complete the handshake
 * (Split) and hand msg3 out for delivery to the responder. */
void* rt_noise_initiate_finalize_arr(void* session, void* in_msg_arr) {
    const uint8_t* in;
    int in_len = noise_arr_bytes(in_msg_arr, &in);
    if (rt_noise_initiate_finalize(session, in, in_len) != 0) return NULL;
    return rt_noise_session_last_msg_arr(session);
}

int32_t rt_noise_respond_finalize_arr(void* session, void* in_msg_arr) {
    const uint8_t* in;
    int in_len = noise_arr_bytes(in_msg_arr, &in);
    /* Arc bool 语义：成功 1 / 失败 0（C 层 0/-1 惯例在此归一化，msg3 空
     * payload 成功返回 0 不能被判 false）。 */
    return rt_noise_respond_finalize(session, in, in_len) >= 0 ? 1 : 0;
}

/* Transport encrypt: returns ciphertext || tag (pt_len + 16 bytes). */
void* rt_noise_session_encrypt_arr(void* session, void* pt_arr) {
    const uint8_t* pt;
    int pt_len = noise_arr_bytes(pt_arr, &pt);
    void* out = rt_array_create(pt_len + 16, 1);
    int r = rt_noise_session_encrypt(session, pt, pt_len,
                                     (uint8_t*)out, (uint8_t*)out + pt_len);
    if (r < 0) {
        rt_array_destroy(out);
        return NULL;
    }
    return out;
}

/* Transport decrypt: ciphertext and 16-byte tag are separate inputs. */
void* rt_noise_session_decrypt_arr(void* session, void* ct_arr, void* tag_arr) {
    const uint8_t* ct;
    const uint8_t* tag;
    int ct_len = noise_arr_bytes(ct_arr, &ct);
    int tag_len = noise_arr_bytes(tag_arr, &tag);
    if (tag_len != 16) return NULL;
    void* out = rt_array_create(ct_len, 1);
    int r = rt_noise_session_decrypt(session, ct, ct_len, tag, (uint8_t*)out);
    if (r < 0) {
        rt_array_destroy(out);
        return NULL;
    }
    return out;
}

void* rt_noise_session_last_msg_arr(void* session) {
    uint8_t buf[128];
    int n = rt_noise_session_last_msg(session, buf, (int)sizeof(buf));
    if (n < 0) return NULL;
    return noise_arr_from(buf, n);
}

void* rt_noise_session_handshake_hash_arr(void* session) {
    uint8_t buf[32];
    if (rt_noise_session_handshake_hash(session, buf) != 0) return NULL;
    return noise_arr_from(buf, 32);
}
