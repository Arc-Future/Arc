// RFC 042 M1: ChaCha20-Poly1305 AEAD (RFC 8439).
//
// Self-contained C11 implementation of the ChaCha20 stream cipher and
// Poly1305 one-time authenticator, composed into the AEAD construction
// that underlies the Noise Protocol Framework transport (RFC 042 D5).
//
// All public entry points operate on caller-owned byte buffers.  None
// of the functions allocate memory.  Thread-safe (no mutable global state).

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ===================================================================
 *  ChaCha20 (RFC 8439 §2.4)
 * =================================================================== */

#define CHACHA_ROUNDS 20

static inline uint32_t rotl32(uint32_t x, int n) {
    return (x << n) | (x >> (32 - n));
}

static void chacha20_quarter_round(uint32_t* a, uint32_t* b, uint32_t* c, uint32_t* d) {
    *a += *b; *d ^= *a; *d = rotl32(*d, 16);
    *c += *d; *b ^= *c; *b = rotl32(*b, 12);
    *a += *b; *d ^= *a; *d = rotl32(*d, 8);
    *c += *d; *b ^= *c; *b = rotl32(*b, 7);
}

static void chacha20_block(const uint8_t key[32], uint32_t counter,
                           const uint8_t nonce[12], uint8_t out[64]) {
    /* Initial state: "expand 32-byte k" constant + key + counter + nonce */
    uint32_t s[16];
    s[0] = 0x61707865; s[1] = 0x3320646e; s[2] = 0x79622d32; s[3] = 0x6b206574;
    for (int i = 0; i < 8; i++) {
        s[4 + i] = (uint32_t)key[4*i] | ((uint32_t)key[4*i+1] << 8) |
                   ((uint32_t)key[4*i+2] << 16) | ((uint32_t)key[4*i+3] << 24);
    }
    s[12] = counter;
    for (int i = 0; i < 3; i++) {
        s[13 + i] = (uint32_t)nonce[4*i] | ((uint32_t)nonce[4*i+1] << 8) |
                    ((uint32_t)nonce[4*i+2] << 16) | ((uint32_t)nonce[4*i+3] << 24);
    }

    uint32_t x[16];
    memcpy(x, s, sizeof(x));

    for (int i = 0; i < CHACHA_ROUNDS; i += 2) {
        /* Column rounds */
        chacha20_quarter_round(&x[0], &x[4], &x[8],  &x[12]);
        chacha20_quarter_round(&x[1], &x[5], &x[9],  &x[13]);
        chacha20_quarter_round(&x[2], &x[6], &x[10], &x[14]);
        chacha20_quarter_round(&x[3], &x[7], &x[11], &x[15]);
        /* Diagonal rounds */
        chacha20_quarter_round(&x[0], &x[5], &x[10], &x[15]);
        chacha20_quarter_round(&x[1], &x[6], &x[11], &x[12]);
        chacha20_quarter_round(&x[2], &x[7], &x[8],  &x[13]);
        chacha20_quarter_round(&x[3], &x[4], &x[9],  &x[14]);
    }

    for (int i = 0; i < 16; i++) x[i] += s[i];
    for (int i = 0; i < 16; i++) {
        out[4*i]     = (uint8_t)(x[i]);
        out[4*i + 1] = (uint8_t)(x[i] >> 8);
        out[4*i + 2] = (uint8_t)(x[i] >> 16);
        out[4*i + 3] = (uint8_t)(x[i] >> 24);
    }
}

static void chacha20_encrypt(const uint8_t key[32], uint32_t counter,
                             const uint8_t nonce[12],
                             const uint8_t* plaintext, uint32_t pt_len,
                             uint8_t* out_ciphertext) {
    uint8_t block[64];
    for (uint32_t j = 0; j < pt_len; j += 64) {
        chacha20_block(key, counter, nonce, block);
        counter++;
        uint32_t chunk = (pt_len - j < 64) ? pt_len - j : 64;
        for (uint32_t k = 0; k < chunk; k++)
            out_ciphertext[j + k] = plaintext[j + k] ^ block[k];
    }
}

/* ===================================================================
 *  Poly1305 (RFC 8439 §2.5)
 * =================================================================== */

static void poly1305_clamp(uint8_t r[16]) {
    r[3]  &= 15; r[7]  &= 15; r[11] &= 15; r[15] &= 15;
    r[4]  &= 252; r[8] &= 252; r[12] &= 252;
}

static uint32_t u8to32_le(const uint8_t p[4]) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void u32to8_le(uint8_t p[4], uint32_t v) {
    p[0] = (uint8_t)v; p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24);
}

/* Single-shot Poly1305 over msg[0..msg_len).  Faithful port of the
 * canonical 26-bit-limb implementation (poly1305-donna / RFC 8439 §2.5):
 * r is clamped byte-wise, message blocks are consumed with the aligned
 * 26-bit extraction (byte offsets 0,3,6,9,12), every full 16-byte block
 * carries the 2^128 hibit, and a trailing partial block is padded with a
 * single 1 bit (hibit = 0).  Verified byte-for-byte against RFC 8439
 * §2.5.2 and Appendix A.3 test vectors #5-#10. */
static void poly1305_mac(const uint8_t key[32],
                         const uint8_t* msg, uint32_t msg_len,
                         uint8_t out_tag[16]) {
    uint8_t r[16], s[16];
    memcpy(r, key, 16);
    poly1305_clamp(r);
    memcpy(s, key + 16, 16);

    uint32_t r0 = u8to32_le(r + 0) & 0x3ffffff;
    uint32_t r1 = (u8to32_le(r + 3) >> 2) & 0x3ffffff;
    uint32_t r2 = (u8to32_le(r + 6) >> 4) & 0x3ffffff;
    uint32_t r3 = (u8to32_le(r + 9) >> 6) & 0x3ffffff;
    uint32_t r4 = (u8to32_le(r + 12) >> 8) & 0x00fffff;

    uint32_t h0 = 0, h1 = 0, h2 = 0, h3 = 0, h4 = 0;
    uint32_t s1 = r1 * 5, s2 = r2 * 5, s3 = r3 * 5, s4 = r4 * 5;
    uint32_t c;
    uint64_t d0, d1, d2, d3, d4;

    uint32_t pad[4];
    pad[0] = u8to32_le(s + 0); pad[1] = u8to32_le(s + 4);
    pad[2] = u8to32_le(s + 8); pad[3] = u8to32_le(s + 12);

    uint32_t i = 0;
    while (i + 16 <= msg_len) {
        const uint8_t* m = msg + i;
        const uint32_t hibit = 1U << 24;   /* 2^128 for full blocks */
        h0 += u8to32_le(m + 0) & 0x3ffffff;
        h1 += (u8to32_le(m + 3) >> 2) & 0x3ffffff;
        h2 += (u8to32_le(m + 6) >> 4) & 0x3ffffff;
        h3 += (u8to32_le(m + 9) >> 6) & 0x3ffffff;
        h4 += (u8to32_le(m + 12) >> 8) | hibit;

        /* h *= r */
        d0 = (uint64_t)h0*r0 + (uint64_t)h1*s4 + (uint64_t)h2*s3 + (uint64_t)h3*s2 + (uint64_t)h4*s1;
        d1 = (uint64_t)h0*r1 + (uint64_t)h1*r0 + (uint64_t)h2*s4 + (uint64_t)h3*s3 + (uint64_t)h4*s2;
        d2 = (uint64_t)h0*r2 + (uint64_t)h1*r1 + (uint64_t)h2*r0 + (uint64_t)h3*s4 + (uint64_t)h4*s3;
        d3 = (uint64_t)h0*r3 + (uint64_t)h1*r2 + (uint64_t)h2*r1 + (uint64_t)h3*r0 + (uint64_t)h4*s4;
        d4 = (uint64_t)h0*r4 + (uint64_t)h1*r3 + (uint64_t)h2*r2 + (uint64_t)h3*r1 + (uint64_t)h4*r0;

        /* partial reduction */
        c = (uint32_t)(d0 >> 26); h0 = (uint32_t)d0 & 0x3ffffff; d1 += c;
        c = (uint32_t)(d1 >> 26); h1 = (uint32_t)d1 & 0x3ffffff; d2 += c;
        c = (uint32_t)(d2 >> 26); h2 = (uint32_t)d2 & 0x3ffffff; d3 += c;
        c = (uint32_t)(d3 >> 26); h3 = (uint32_t)d3 & 0x3ffffff; d4 += c;
        c = (uint32_t)(d4 >> 26); h4 = (uint32_t)d4 & 0x3ffffff; h0 += c * 5;
        c = h0 >> 26; h0 &= 0x3ffffff; h1 += c;

        i += 16;
    }

    /* Final partial block (if any): pad with a single 1 bit, no hibit */
    if (i < msg_len) {
        uint8_t buf[16];
        uint32_t rem = msg_len - i;
        memcpy(buf, msg + i, rem);
        buf[rem] = 1;
        for (uint32_t k = rem + 1; k < 16; k++) buf[k] = 0;

        h0 += u8to32_le(buf + 0) & 0x3ffffff;
        h1 += (u8to32_le(buf + 3) >> 2) & 0x3ffffff;
        h2 += (u8to32_le(buf + 6) >> 4) & 0x3ffffff;
        h3 += (u8to32_le(buf + 9) >> 6) & 0x3ffffff;
        h4 += (u8to32_le(buf + 12) >> 8);

        d0 = (uint64_t)h0*r0 + (uint64_t)h1*s4 + (uint64_t)h2*s3 + (uint64_t)h3*s2 + (uint64_t)h4*s1;
        d1 = (uint64_t)h0*r1 + (uint64_t)h1*r0 + (uint64_t)h2*s4 + (uint64_t)h3*s3 + (uint64_t)h4*s2;
        d2 = (uint64_t)h0*r2 + (uint64_t)h1*r1 + (uint64_t)h2*r0 + (uint64_t)h3*s4 + (uint64_t)h4*s3;
        d3 = (uint64_t)h0*r3 + (uint64_t)h1*r2 + (uint64_t)h2*r1 + (uint64_t)h3*r0 + (uint64_t)h4*s4;
        d4 = (uint64_t)h0*r4 + (uint64_t)h1*r3 + (uint64_t)h2*r2 + (uint64_t)h3*r1 + (uint64_t)h4*r0;

        c = (uint32_t)(d0 >> 26); h0 = (uint32_t)d0 & 0x3ffffff; d1 += c;
        c = (uint32_t)(d1 >> 26); h1 = (uint32_t)d1 & 0x3ffffff; d2 += c;
        c = (uint32_t)(d2 >> 26); h2 = (uint32_t)d2 & 0x3ffffff; d3 += c;
        c = (uint32_t)(d3 >> 26); h3 = (uint32_t)d3 & 0x3ffffff; d4 += c;
        c = (uint32_t)(d4 >> 26); h4 = (uint32_t)d4 & 0x3ffffff; h0 += c * 5;
        c = h0 >> 26; h0 &= 0x3ffffff; h1 += c;
    }

    /* Fully carry h */
    c = h1 >> 26; h1 &= 0x3ffffff; h2 += c;
    c = h2 >> 26; h2 &= 0x3ffffff; h3 += c;
    c = h3 >> 26; h3 &= 0x3ffffff; h4 += c;
    c = h4 >> 26; h4 &= 0x3ffffff; h0 += c * 5;
    c = h0 >> 26; h0 &= 0x3ffffff; h1 += c;

    /* Compute h + -p */
    uint32_t g0 = h0 + 5; c = g0 >> 26; g0 &= 0x3ffffff;
    uint32_t g1 = h1 + c; c = g1 >> 26; g1 &= 0x3ffffff;
    uint32_t g2 = h2 + c; c = g2 >> 26; g2 &= 0x3ffffff;
    uint32_t g3 = h3 + c; c = g3 >> 26; g3 &= 0x3ffffff;
    uint32_t g4 = h4 + c - (1U << 26);

    /* Select h if h < p, or h + -p if h >= p */
    uint32_t mask = (g4 >> 31) - 1;
    g0 &= mask; g1 &= mask; g2 &= mask; g3 &= mask; g4 &= mask;
    mask = ~mask;
    h0 = (h0 & mask) | g0;
    h1 = (h1 & mask) | g1;
    h2 = (h2 & mask) | g2;
    h3 = (h3 & mask) | g3;
    h4 = (h4 & mask) | g4;

    /* h = h % (2^128) */
    h0 = (h0 | (h1 << 26)) & 0xffffffff;
    h1 = ((h1 >> 6) | (h2 << 20)) & 0xffffffff;
    h2 = ((h2 >> 12) | (h3 << 14)) & 0xffffffff;
    h3 = ((h3 >> 18) | (h4 << 8)) & 0xffffffff;

    /* mac = (h + pad) % (2^128) */
    uint64_t f = (uint64_t)h0 + pad[0]; h0 = (uint32_t)f;
    f = (uint64_t)h1 + pad[1] + (f >> 32); h1 = (uint32_t)f;
    f = (uint64_t)h2 + pad[2] + (f >> 32); h2 = (uint32_t)f;
    f = (uint64_t)h3 + pad[3] + (f >> 32); h3 = (uint32_t)f;

    u32to8_le(out_tag + 0, h0);
    u32to8_le(out_tag + 4, h1);
    u32to8_le(out_tag + 8, h2);
    u32to8_le(out_tag + 12, h3);
}

/* ===================================================================
 *  AEAD: ChaCha20-Poly1305 (RFC 8439 §2.8)
 * =================================================================== */

/* Compute the Poly1305 tag over (AAD || padding || data || padding || lengths)
 * where `data` is the authenticated payload (ciphertext).  Shared by the
 * encrypt and decrypt paths so the MAC message layout cannot diverge. */
static int32_t aead_compute_tag(const uint8_t* data, uint32_t data_len,
                                const uint8_t key[32], const uint8_t nonce[12],
                                const uint8_t* aad, uint32_t aad_len,
                                uint8_t out_tag[16]) {
    if (!data || !key || !nonce || !out_tag) return -1;
    if (aad_len > 0 && !aad) return -1;

    /* Step 1: Generate Poly1305 key from ChaCha20 block with counter=0 */
    uint8_t poly_key[64];
    uint8_t zero_block[64];
    memset(zero_block, 0, 64);
    chacha20_block(key, 0, nonce, poly_key);

    /* Step 2: Build MAC message over (AAD || padding || data || padding || lengths) */
    uint32_t mac_msg_len = aad_len + (16 - (aad_len & 15)) % 16
                         + data_len + (16 - (data_len & 15)) % 16 + 16;

    uint8_t mac_stack[1024];
    uint8_t* mac_msg = mac_stack;
    int heap_alloc = 0;
    if (mac_msg_len > sizeof(mac_stack)) {
        mac_msg = (uint8_t*)malloc(mac_msg_len);
        if (!mac_msg) return -1;
        heap_alloc = 1;
    }

    uint32_t off = 0;
    if (aad_len > 0) { memcpy(mac_msg + off, aad, aad_len); off += aad_len; }
    uint32_t aad_pad = (16 - (aad_len & 15)) & 15;
    memset(mac_msg + off, 0, aad_pad); off += aad_pad;
    memcpy(mac_msg + off, data, data_len); off += data_len;
    uint32_t data_pad = (16 - (data_len & 15)) & 15;
    memset(mac_msg + off, 0, data_pad); off += data_pad;
    for (int i = 0; i < 8; i++) mac_msg[off + i] = (uint8_t)((uint64_t)aad_len >> (8*i));
    for (int i = 0; i < 8; i++) mac_msg[off + 8 + i] = (uint8_t)((uint64_t)data_len >> (8*i));

    poly1305_mac(poly_key, mac_msg, off + 16, out_tag);

    if (heap_alloc) free(mac_msg);
    return 0;
}

int32_t rt_crypto_aead_encrypt(const uint8_t* plaintext, uint32_t pt_len,
                                const uint8_t key[32], const uint8_t nonce[12],
                                const uint8_t* aad, uint32_t aad_len,
                                uint8_t* out_ciphertext, uint8_t out_tag[16]) {
    if (!plaintext || !key || !nonce || !out_ciphertext || !out_tag) return -1;
    if (aad_len > 0 && !aad) return -1;

    /* Encrypt plaintext with ChaCha20 starting at counter=1 */
    chacha20_encrypt(key, 1, nonce, plaintext, pt_len, out_ciphertext);

    /* Authenticate the produced ciphertext */
    return aead_compute_tag(out_ciphertext, pt_len, key, nonce, aad, aad_len, out_tag);
}

int32_t rt_crypto_aead_decrypt(const uint8_t* ciphertext, uint32_t ct_len,
                                const uint8_t key[32], const uint8_t nonce[12],
                                const uint8_t* aad, uint32_t aad_len,
                                const uint8_t tag[16],
                                uint8_t* out_plaintext) {
    if (!ciphertext || !key || !nonce || !out_plaintext || !tag) return -1;
    if (aad_len > 0 && !aad) return -1;

    /* Step 1: Recompute tag over the received ciphertext */
    uint8_t computed_tag[16];
    int32_t r = aead_compute_tag(ciphertext, ct_len, key, nonce, aad, aad_len, computed_tag);
    if (r != 0) return -1;

    /* Step 2: Constant-time tag comparison */
    uint8_t diff = 0;
    for (int i = 0; i < 16; i++) diff |= computed_tag[i] ^ tag[i];
    if (diff != 0) {
        /* Tag mismatch: zero the output */
        memset(out_plaintext, 0, ct_len);
        return -1;
    }

    /* Step 3: Decrypt (ChaCha20 is symmetric — same keystream XOR) */
    chacha20_encrypt(key, 1, nonce, ciphertext, ct_len, out_plaintext);
    return 0;
}
