// SHA3-256 and SHA3-512 (Keccak sponge, FIPS 202).
//
// Split out of the runtime so each concern lives in its own translation unit.
// All public entry points return a freshly malloc'd NUL-terminated lowercase-hex
// string (caller-managed via ARC). Input NULL is treated as the empty string.
// All functions are thread-safe (no mutable global state). The algorithms are
// self-contained C11 with no external dependency beyond stdlib headers.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* Keccak round constants (iota step) */
static const uint64_t KECCAK_RC[24] = {
    0x0000000000000001ULL, 0x0000000000008082ULL, 0x800000000000808aULL,
    0x8000000080008000ULL, 0x000000000000808bULL, 0x0000000080000001ULL,
    0x8000000080008081ULL, 0x8000000000008009ULL, 0x000000000000008aULL,
    0x0000000000000088ULL, 0x0000000080008009ULL, 0x000000008000000aULL,
    0x000000008000808bULL, 0x800000000000008bULL, 0x8000000000008089ULL,
    0x8000000000008003ULL, 0x8000000000008002ULL, 0x8000000000000080ULL,
    0x000000000000800aULL, 0x800000008000000aULL, 0x8000000080008081ULL,
    0x8000000000008080ULL, 0x0000000080000001ULL, 0x8000000080008008ULL,
};

/* Rotation offsets for rho step, indexed as [x + 5*y] (column-major) */
static const int KECCAK_ROT[25] = {
     0,  1, 62, 28, 27, 36, 44,  6, 55, 20,
     3, 10, 43, 25, 39, 41, 45, 15, 21,  8,
    18,  2, 61, 56, 14,
};

static inline uint64_t rotl64(uint64_t x, int n) {
    return (x << n) | (x >> (64 - n));
}

/* Keccak-f[1600] -- 24-round permutation on 5x5x64 state */
static void keccak_f1600(uint64_t state[25]) {
    for (int round = 0; round < 24; round++) {
        uint64_t C[5], D[5];
        /* theta step */
        for (int x = 0; x < 5; x++)
            C[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        for (int x = 0; x < 5; x++) {
            D[x] = C[(x + 4) % 5] ^ rotl64(C[(x + 1) % 5], 1);
            for (int y = 0; y < 5; y++)
                state[x + 5 * y] ^= D[x];
        }
        /* rho and pi steps */
        uint64_t B[25];
        for (int x = 0; x < 5; x++)
            for (int y = 0; y < 5; y++)
                B[y + 5 * ((2 * x + 3 * y) % 5)] = rotl64(state[x + 5 * y], KECCAK_ROT[x + 5 * y]);
        /* chi step */
        for (int x = 0; x < 5; x++) {
            for (int y = 0; y < 5; y++) {
                state[x + 5 * y] = B[x + 5 * y] ^ (~B[(x + 1) % 5 + 5 * y] & B[(x + 2) % 5 + 5 * y]);
            }
        }
        /* iota step */
        state[0] ^= KECCAK_RC[round];
    }
}

/* ---- Hex encoding helper ----------------------------------------------- */

/* Allocate a lowercase-hex string for 'len' bytes. Always NUL-terminates. */
static char* bytes_to_hex(const uint8_t* data, size_t len) {
    char* out = (char*)malloc(len * 2 + 1);
    if (!out) return NULL;
    static const char hex[] = "0123456789abcdef";
    for (size_t i = 0; i < len; i++) {
        out[i * 2]     = hex[(data[i] >> 4) & 0xf];
        out[i * 2 + 1] = hex[data[i] & 0xf];
    }
    out[len * 2] = '\0';
    return out;
}

/* ---- SHA3 sponge absorb + squeeze ------------------------------------- */

/*
 * SHA3 sponge absorb + squeeze.
 *   rate     = bytes absorbed per block (200 - capacity/8; e.g. 136 for SHA3-256)
 *   out_bytes = output length
 */
static void sha3_hash(const uint8_t* data, size_t len, uint8_t* out, size_t out_bytes, int rate) {
    /* State: 5x5x64 = 1600 bits = 200 bytes, stored as 25 x uint64_t (little-endian) */
    uint64_t state[25];
    memset(state, 0, sizeof(state));

    /* Absorb */
    size_t pos = 0;
    while (pos + rate <= len) {
        /* XOR a full rate block (convert bytes to state words, little-endian) */
        for (int i = 0; i < rate; i += 8) {
            uint64_t w = 0;
            for (int j = 0; j < 8 && i + j < rate; j++)
                w |= (uint64_t)data[pos + i + j] << (8 * j);
            state[i / 8] ^= w;
        }
        keccak_f1600(state);
        pos += rate;
    }

    /* Pad & absorb last block (SHA3 10*1 padding with 0x06 domain separator) */
    uint8_t last_block[200];
    memset(last_block, 0, rate);
    size_t remaining = len - pos;
    memcpy(last_block, data + pos, remaining);
    last_block[remaining] = 0x06;   /* SHA3 domain separator bits (01 in LSB-first) */
    last_block[rate - 1] |= 0x80;   /* padding end bit (1 in MSB of last byte) */
    for (int i = 0; i < rate; i += 8) {
        uint64_t w = 0;
        for (int j = 0; j < 8 && i + j < rate; j++)
            w |= (uint64_t)last_block[i + j] << (8 * j);
        state[i / 8] ^= w;
    }
    keccak_f1600(state);

    /* Squeeze (can squeeze more than rate with additional permutations, but
     * SHA3-256/512 output fits in one rate block) */
    size_t squeezed = 0;
    int block_idx = 0;
    while (squeezed < out_bytes) {
        if (block_idx * 8 >= rate) {
            keccak_f1600(state);
            block_idx = 0;
        }
        uint64_t w = state[block_idx];
        for (int j = 0; j < 8 && squeezed < out_bytes; j++) {
            out[squeezed++] = (uint8_t)(w >> (8 * j));
        }
        block_idx++;
    }
}

/* ======================================================================= */
/* SHA3-256: rate = 1088/8 = 136 bytes, output = 32 bytes                   */
/* ======================================================================= */

char* rt_crypto_sha3_256(const char* data) {
    if (!data) data = "";
    uint8_t digest[32];
    sha3_hash((const uint8_t*)data, strlen(data), digest, 32, 136);
    return bytes_to_hex(digest, 32);
}

/* ======================================================================= */
/* SHA3-512: rate = 576/8 = 72 bytes, output = 64 bytes                     */
/* ======================================================================= */

char* rt_crypto_sha3_512(const char* data) {
    if (!data) data = "";
    uint8_t digest[64];
    sha3_hash((const uint8_t*)data, strlen(data), digest, 64, 72);
    return bytes_to_hex(digest, 64);
}

/* ======================================================================= */
/* byte[] 变体（RFC 026 M3 修订）——sha3_hash 为本 TU 私有内核，byte[] 入口  */
/* 须就地实现。入参 RtArray byte[] payload，出参新建 RtArray byte[]；        */
/* 语义与失败协议见 rt_abi.h 对应声明段。                                    */
/* ======================================================================= */

static void* sha3_digest_to_array(const uint8_t* digest, size_t len) {
    void* out = rt_array_create((int32_t)len, 1);
    if (len > 0) memcpy(out, digest, len);
    return out;
}

void* rt_crypto_sha3_256_arr(void* data) {
    static const uint8_t k_empty[1] = { 0 };
    const uint8_t* p = data ? (const uint8_t*)data : k_empty;
    size_t len = data ? (size_t)rt_array_length(data) : 0;
    uint8_t digest[32];
    sha3_hash(p, len, digest, 32, 136);
    return sha3_digest_to_array(digest, 32);
}

void* rt_crypto_sha3_512_arr(void* data) {
    static const uint8_t k_empty[1] = { 0 };
    const uint8_t* p = data ? (const uint8_t*)data : k_empty;
    size_t len = data ? (size_t)rt_array_length(data) : 0;
    uint8_t digest[64];
    sha3_hash(p, len, digest, 64, 72);
    return sha3_digest_to_array(digest, 64);
}
