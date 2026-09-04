// Cryptographic ABI (RFC 026 M3: arc-security runtime).
//
// Split out of the runtime so each concern lives in its own translation unit:
// this file implements the hash / MAC / CSPRNG primitives backing the
// arc-security std facade. All public entry points return a freshly malloc'd
// NUL-terminated lowercase-hex string (caller-managed via ARC). Input NULL is
// treated as the empty string. All functions are thread-safe (no mutable
// global state). The algorithms are self-contained C11 with no external
// dependency beyond system libraries for random generation.

#include "rt_abi.h"

#include <stdlib.h>
#include <string.h>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#else
#  include <fcntl.h>
#  include <unistd.h>
#endif

/* ---- Bit / byte helpers ------------------------------------------------ */

static inline uint32_t rotl32(uint32_t x, int n) {
    return (x << n) | (x >> (32 - n));
}
static inline uint32_t rotr32(uint32_t x, int n) {
    return (x >> n) | (x << (32 - n));
}
static inline uint64_t rotr64(uint64_t x, int n) {
    return (x >> n) | (x << (64 - n));
}

static inline uint32_t read_le32(const uint8_t* p) {
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
           ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static inline uint32_t read_be32(const uint8_t* p) {
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) |
           ((uint32_t)p[2] << 8) | (uint32_t)p[3];
}
static inline uint64_t read_be64(const uint8_t* p) {
    return ((uint64_t)p[0] << 56) | ((uint64_t)p[1] << 48) |
           ((uint64_t)p[2] << 40) | ((uint64_t)p[3] << 32) |
           ((uint64_t)p[4] << 24) | ((uint64_t)p[5] << 16) |
           ((uint64_t)p[6] << 8) | (uint64_t)p[7];
}

static inline void write_le32(uint8_t* p, uint32_t v) {
    p[0] = (uint8_t)(v); p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16); p[3] = (uint8_t)(v >> 24);
}
static inline void write_be32(uint8_t* p, uint32_t v) {
    p[0] = (uint8_t)(v >> 24); p[1] = (uint8_t)(v >> 16);
    p[2] = (uint8_t)(v >> 8); p[3] = (uint8_t)(v);
}
static inline void write_be64(uint8_t* p, uint64_t v) {
    for (int i = 0; i < 8; i++) p[i] = (uint8_t)(v >> (56 - 8 * i));
}

/* ---- Hex encoding helper ----------------------------------------------- */

/* Allocate a lowercase-hex string for `len` bytes. Always NUL-terminates. */
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

/* ---- Message padding --------------------------------------------------- */

/* Build the padded message buffer for a Merkle-Damgard hash.
 *   block_size: 64 (MD5/SHA1/SHA256) or 128 (SHA512)
 *   len_size:   8 (64-bit length) or 16 (128-bit length)
 *   big_endian: 1 for SHA family length field, 0 for MD5 (little-endian)
 * Returns a malloc'd buffer of size *out_len, or NULL on allocation failure. */
static uint8_t* make_padded(const uint8_t* data, size_t len,
                            int block_size, int len_size,
                            int big_endian, size_t* out_len) {
    size_t len_field = (size_t)(block_size - len_size);
    size_t pos = len + 1;
    size_t rem = pos % (size_t)block_size;
    size_t P = (rem <= len_field)
                   ? pos + (len_field - rem)
                   : pos + ((size_t)block_size - rem + len_field);
    size_t total = P + (size_t)len_size;

    uint8_t* buf = (uint8_t*)malloc(total);
    if (!buf) return NULL;
    memcpy(buf, data, len);
    buf[len] = 0x80;
    if (P > len + 1) memset(buf + len + 1, 0, P - len - 1);

    uint64_t bit_len = (uint64_t)len * 8;
    if (big_endian) {
        for (int i = 0; i < len_size; i++) {
            int shift = (len_size - 1 - i) * 8;
            buf[P + i] = (shift >= 64) ? 0 : (uint8_t)((bit_len >> shift) & 0xff);
        }
    } else {
        for (int i = 0; i < len_size; i++) {
            buf[P + i] = (uint8_t)((bit_len >> (i * 8)) & 0xff);
        }
    }
    *out_len = total;
    return buf;
}

/* ======================================================================= */
/* MD5 (RFC 1321) — 4 rounds, 512-bit blocks, little-endian.                */
/* ======================================================================= */

static const uint32_t MD5_K[64] = {
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee,
    0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
    0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa,
    0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
    0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05,
    0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039,
    0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
    0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
};
static const int MD5_S[64] = {
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
    5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20, 5,  9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
};

static void md5_hash(const uint8_t* data, size_t len, uint8_t out[16]) {
    size_t total = 0;
    uint8_t* buf = make_padded(data, len, 64, 8, 0, &total);
    if (!buf) { memset(out, 0, 16); return; }

    uint32_t a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
    for (size_t off = 0; off < total; off += 64) {
        uint32_t M[16];
        for (int i = 0; i < 16; i++) M[i] = read_le32(buf + off + 4 * i);
        uint32_t A = a0, B = b0, C = c0, D = d0;
        for (int i = 0; i < 64; i++) {
            uint32_t F; int g;
            if (i < 16)      { F = (B & C) | (~B & D);       g = i; }
            else if (i < 32) { F = (D & B) | (~D & C);       g = (5 * i + 1) & 15; }
            else if (i < 48) { F = B ^ C ^ D;                g = (3 * i + 5) & 15; }
            else             { F = C ^ (B | ~D);             g = (7 * i) & 15; }
            uint32_t temp = A + F + MD5_K[i] + M[g];
            A = D; D = C; C = B;
            B = B + rotl32(temp, MD5_S[i]);
        }
        a0 += A; b0 += B; c0 += C; d0 += D;
    }
    write_le32(out,      a0);
    write_le32(out + 4,  b0);
    write_le32(out + 8,  c0);
    write_le32(out + 12, d0);
    free(buf);
}

char* rt_crypto_md5(const char* data) {
    if (!data) data = "";
    uint8_t digest[16];
    md5_hash((const uint8_t*)data, strlen(data), digest);
    return bytes_to_hex(digest, 16);
}

/* ======================================================================= */
/* SHA-1 (FIPS 180-4) — 80 rounds, 512-bit blocks, big-endian.              */
/* ======================================================================= */

static void sha1_hash(const uint8_t* data, size_t len, uint8_t out[20]) {
    size_t total = 0;
    uint8_t* buf = make_padded(data, len, 64, 8, 1, &total);
    if (!buf) { memset(out, 0, 20); return; }

    uint32_t h0 = 0x67452301, h1 = 0xEFCDAB89, h2 = 0x98BADCFE,
             h3 = 0x10325476, h4 = 0xC3D2E1F0;
    for (size_t off = 0; off < total; off += 64) {
        uint32_t W[80];
        for (int i = 0; i < 16; i++) W[i] = read_be32(buf + off + 4 * i);
        for (int i = 16; i < 80; i++)
            W[i] = rotl32(W[i - 3] ^ W[i - 8] ^ W[i - 14] ^ W[i - 16], 1);

        uint32_t a = h0, b = h1, c = h2, d = h3, e = h4;
        for (int i = 0; i < 80; i++) {
            uint32_t f, k;
            if (i < 20)      { f = (b & c) | (~b & d);            k = 0x5A827999; }
            else if (i < 40) { f = b ^ c ^ d;                     k = 0x6ED9EBA1; }
            else if (i < 60) { f = (b & c) | (b & d) | (c & d);   k = 0x8F1BBCDC; }
            else             { f = b ^ c ^ d;                     k = 0xCA62C1D6; }
            uint32_t temp = rotl32(a, 5) + f + e + k + W[i];
            e = d; d = c; c = rotl32(b, 30); b = a; a = temp;
        }
        h0 += a; h1 += b; h2 += c; h3 += d; h4 += e;
    }
    write_be32(out,      h0);
    write_be32(out + 4,  h1);
    write_be32(out + 8,  h2);
    write_be32(out + 12, h3);
    write_be32(out + 16, h4);
    free(buf);
}

char* rt_crypto_sha1(const char* data) {
    if (!data) data = "";
    uint8_t digest[20];
    sha1_hash((const uint8_t*)data, strlen(data), digest);
    return bytes_to_hex(digest, 20);
}

/* ======================================================================= */
/* SHA-256 (FIPS 180-4) — 64 rounds, 8x32-bit state, big-endian.            */
/* ======================================================================= */

static const uint32_t SHA256_K[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
};

static void sha256_hash(const uint8_t* data, size_t len, uint8_t out[32]) {
    size_t total = 0;
    uint8_t* buf = make_padded(data, len, 64, 8, 1, &total);
    if (!buf) { memset(out, 0, 32); return; }

    uint32_t H[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    };
    for (size_t off = 0; off < total; off += 64) {
        uint32_t W[64];
        for (int i = 0; i < 16; i++) W[i] = read_be32(buf + off + 4 * i);
        for (int i = 16; i < 64; i++) {
            uint32_t s0 = rotr32(W[i - 15], 7) ^ rotr32(W[i - 15], 18) ^ (W[i - 15] >> 3);
            uint32_t s1 = rotr32(W[i - 2], 17) ^ rotr32(W[i - 2], 19) ^ (W[i - 2] >> 10);
            W[i] = W[i - 16] + s0 + W[i - 7] + s1;
        }
        uint32_t a = H[0], b = H[1], c = H[2], d = H[3],
                 e = H[4], f = H[5], g = H[6], h = H[7];
        for (int i = 0; i < 64; i++) {
            uint32_t S1 = rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25);
            uint32_t ch = (e & f) ^ (~e & g);
            uint32_t t1 = h + S1 + ch + SHA256_K[i] + W[i];
            uint32_t S0 = rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22);
            uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
            uint32_t t2 = S0 + maj;
            h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        H[0] += a; H[1] += b; H[2] += c; H[3] += d;
        H[4] += e; H[5] += f; H[6] += g; H[7] += h;
    }
    for (int i = 0; i < 8; i++) write_be32(out + 4 * i, H[i]);
    free(buf);
}

char* rt_crypto_sha256(const char* data) {
    if (!data) data = "";
    uint8_t digest[32];
    sha256_hash((const uint8_t*)data, strlen(data), digest);
    return bytes_to_hex(digest, 32);
}

/* ======================================================================= */
/* SHA-512 (FIPS 180-4) — 80 rounds, 8x64-bit state, big-endian.            */
/* ======================================================================= */

static const uint64_t SHA512_K[80] = {
    0x428a2f98d728ae22ULL, 0x7137449123ef65cdULL, 0xb5c0fbcfec4d3b2fULL, 0xe9b5dba58189dbbcULL,
    0x3956c25bf348b538ULL, 0x59f111f1b605d019ULL, 0x923f82a4af194f9bULL, 0xab1c5ed5da6d8118ULL,
    0xd807aa98a3030242ULL, 0x12835b0145706fbeULL, 0x243185be4ee4b28cULL, 0x550c7dc3d5ffb4e2ULL,
    0x72be5d74f27b896fULL, 0x80deb1fe3b1696b1ULL, 0x9bdc06a725c71235ULL, 0xc19bf174cf692694ULL,
    0xe49b69c19ef14ad2ULL, 0xefbe4786384f25e3ULL, 0x0fc19dc68b8cd5b5ULL, 0x240ca1cc77ac9c65ULL,
    0x2de92c6f592b0275ULL, 0x4a7484aa6ea6e483ULL, 0x5cb0a9dcbd41fbd4ULL, 0x76f988da831153b5ULL,
    0x983e5152ee66dfabULL, 0xa831c66d2db43210ULL, 0xb00327c898fb213fULL, 0xbf597fc7beef0ee4ULL,
    0xc6e00bf33da88fc2ULL, 0xd5a79147930aa725ULL, 0x06ca6351e003826fULL, 0x142929670a0e6e70ULL,
    0x27b70a8546d22ffcULL, 0x2e1b21385c26c926ULL, 0x4d2c6dfc5ac42aedULL, 0x53380d139d95b3dfULL,
    0x650a73548baf63deULL, 0x766a0abb3c77b2a8ULL, 0x81c2c92e47edaee6ULL, 0x92722c851482353bULL,
    0xa2bfe8a14cf10364ULL, 0xa81a664bbc423001ULL, 0xc24b8b70d0f89791ULL, 0xc76c51a30654be30ULL,
    0xd192e819d6ef5218ULL, 0xd69906245565a910ULL, 0xf40e35855771202aULL, 0x106aa07032bbd1b8ULL,
    0x19a4c116b8d2d0c8ULL, 0x1e376c085141ab53ULL, 0x2748774cdf8eeb99ULL, 0x34b0bcb5e19b48a8ULL,
    0x391c0cb3c5c95a63ULL, 0x4ed8aa4ae3418acbULL, 0x5b9cca4f7763e373ULL, 0x682e6ff3d6b2b8a3ULL,
    0x748f82ee5defb2fcULL, 0x78a5636f43172f60ULL, 0x84c87814a1f0ab72ULL, 0x8cc702081a6439ecULL,
    0x90befffa23631e28ULL, 0xa4506cebde82bde9ULL, 0xbef9a3f7b2c67915ULL, 0xc67178f2e372532bULL,
    0xca273eceea26619cULL, 0xd186b8c721c0c207ULL, 0xeada7dd6cde0eb1eULL, 0xf57d4f7fee6ed178ULL,
    0x06f067aa72176fbaULL, 0x0a637dc5a2c898a6ULL, 0x113f9804bef90daeULL, 0x1b710b35131c471bULL,
    0x28db77f523047d84ULL, 0x32caab7b40c72493ULL, 0x3c9ebe0a15c9bebcULL, 0x431d67c49c100d4cULL,
    0x4cc5d4becb3e42b6ULL, 0x597f299cfc657e2aULL, 0x5fcb6fab3ad6faecULL, 0x6c44198c4a475817ULL,
};

static void sha512_hash(const uint8_t* data, size_t len, uint8_t out[64]) {
    size_t total = 0;
    uint8_t* buf = make_padded(data, len, 128, 16, 1, &total);
    if (!buf) { memset(out, 0, 64); return; }

    uint64_t H[8] = {
        0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL,
        0x3c6ef372fe94f82bULL, 0xa54ff53a5f1d36f1ULL,
        0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
        0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL,
    };
    for (size_t off = 0; off < total; off += 128) {
        uint64_t W[80];
        for (int i = 0; i < 16; i++) W[i] = read_be64(buf + off + 8 * i);
        for (int i = 16; i < 80; i++) {
            uint64_t s0 = rotr64(W[i - 15], 1) ^ rotr64(W[i - 15], 8) ^ (W[i - 15] >> 7);
            uint64_t s1 = rotr64(W[i - 2], 19) ^ rotr64(W[i - 2], 61) ^ (W[i - 2] >> 6);
            W[i] = W[i - 16] + s0 + W[i - 7] + s1;
        }
        uint64_t a = H[0], b = H[1], c = H[2], d = H[3],
                 e = H[4], f = H[5], g = H[6], h = H[7];
        for (int i = 0; i < 80; i++) {
            uint64_t S1 = rotr64(e, 14) ^ rotr64(e, 18) ^ rotr64(e, 41);
            uint64_t ch = (e & f) ^ (~e & g);
            uint64_t t1 = h + S1 + ch + SHA512_K[i] + W[i];
            uint64_t S0 = rotr64(a, 28) ^ rotr64(a, 34) ^ rotr64(a, 39);
            uint64_t maj = (a & b) ^ (a & c) ^ (b & c);
            uint64_t t2 = S0 + maj;
            h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        H[0] += a; H[1] += b; H[2] += c; H[3] += d;
        H[4] += e; H[5] += f; H[6] += g; H[7] += h;
    }
    for (int i = 0; i < 8; i++) write_be64(out + 8 * i, H[i]);
    free(buf);
}

char* rt_crypto_sha512(const char* data) {
    if (!data) data = "";
    uint8_t digest[64];
    sha512_hash((const uint8_t*)data, strlen(data), digest);
    return bytes_to_hex(digest, 64);
}

/* ======================================================================= */
/* SHA-384 (FIPS 180-4) — SHA-512 truncated to 384 bits (48 bytes).         */
/* ======================================================================= */

/* SHA-384 initial hash values (FIPS 180-4 §5.3.4) — first 64 bits of
 * fractional parts of square roots of the 9th through 16th primes. */
static const uint64_t SHA384_IV[8] = {
    0xcbbb9d5dc1059ed8ULL, 0x629a292a367cd507ULL,
    0x9159015a3070dd17ULL, 0x152fecd8f70e5939ULL,
    0x67332667ffc00b31ULL, 0x8eb44a8768581511ULL,
    0xdb0c2e0d64f98fa7ULL, 0x47b5481dbefa4fa4ULL,
};

static void sha384_hash(const uint8_t* data, size_t len, uint8_t out[48]) {
    size_t total = 0;
    uint8_t* buf = make_padded(data, len, 128, 16, 1, &total);
    if (!buf) { memset(out, 0, 48); return; }

    uint64_t H[8];
    memcpy(H, SHA384_IV, sizeof(SHA384_IV));
    for (size_t off = 0; off < total; off += 128) {
        uint64_t W[80];
        for (int i = 0; i < 16; i++) W[i] = read_be64(buf + off + 8 * i);
        for (int i = 16; i < 80; i++) {
            uint64_t s0 = rotr64(W[i - 15], 1) ^ rotr64(W[i - 15], 8) ^ (W[i - 15] >> 7);
            uint64_t s1 = rotr64(W[i - 2], 19) ^ rotr64(W[i - 2], 61) ^ (W[i - 2] >> 6);
            W[i] = W[i - 16] + s0 + W[i - 7] + s1;
        }
        uint64_t a = H[0], b = H[1], c = H[2], d = H[3],
                 e = H[4], f = H[5], g = H[6], h = H[7];
        for (int i = 0; i < 80; i++) {
            uint64_t S1 = rotr64(e, 14) ^ rotr64(e, 18) ^ rotr64(e, 41);
            uint64_t ch = (e & f) ^ (~e & g);
            uint64_t t1 = h + S1 + ch + SHA512_K[i] + W[i];
            uint64_t S0 = rotr64(a, 28) ^ rotr64(a, 34) ^ rotr64(a, 39);
            uint64_t maj = (a & b) ^ (a & c) ^ (b & c);
            uint64_t t2 = S0 + maj;
            h = g; g = f; f = e; e = d + t1; d = c; c = b; b = a; a = t1 + t2;
        }
        H[0] += a; H[1] += b; H[2] += c; H[3] += d;
        H[4] += e; H[5] += f; H[6] += g; H[7] += h;
    }
    /* Truncate: write 6×64 bits = 48 bytes (original SHA-512 uses 8×64). */
    for (int i = 0; i < 6; i++) write_be64(out + 8 * i, H[i]);
    free(buf);
}

char* rt_crypto_sha384(const char* data) {
    if (!data) data = "";
    uint8_t digest[48];
    sha384_hash((const uint8_t*)data, strlen(data), digest);
    return bytes_to_hex(digest, 48);
}

/* ======================================================================= */
/* HMAC-SHA256 (RFC 2104) — inner/outer SHA-256 over a 64-byte block.       */
/* ======================================================================= */

char* rt_crypto_hmac_sha256(const char* key, const char* msg) {
    if (!key) key = "";
    if (!msg) msg = "";
    size_t klen = strlen(key);
    size_t mlen = strlen(msg);

    /* K0: key zero-padded to block size; over-long keys are hashed first. */
    uint8_t k0[64];
    memset(k0, 0, 64);
    if (klen > 64) {
        uint8_t kh[32];
        sha256_hash((const uint8_t*)key, klen, kh);
        memcpy(k0, kh, 32);
    } else {
        memcpy(k0, key, klen);
    }

    uint8_t ipad[64], opad[64];
    for (int i = 0; i < 64; i++) {
        ipad[i] = (uint8_t)(k0[i] ^ 0x36);
        opad[i] = (uint8_t)(k0[i] ^ 0x5c);
    }

    /* inner = SHA256(ipad || msg) */
    uint8_t* inner_buf = (uint8_t*)malloc(64 + mlen);
    if (!inner_buf) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    memcpy(inner_buf, ipad, 64);
    memcpy(inner_buf + 64, msg, mlen);
    uint8_t inner_hash[32];
    sha256_hash(inner_buf, 64 + mlen, inner_hash);
    free(inner_buf);

    /* outer = SHA256(opad || inner_hash) */
    uint8_t outer_buf[64 + 32];
    memcpy(outer_buf, opad, 64);
    memcpy(outer_buf + 64, inner_hash, 32);
    uint8_t mac[32];
    sha256_hash(outer_buf, 64 + 32, mac);
    return bytes_to_hex(mac, 32);
}

/* ======================================================================= */
/* CSPRNG — platform-specific secure random bytes.                          */
/* ======================================================================= */

/* Fill `buf` with `len` cryptographically-secure random bytes.
 * Returns 0 on success, -1 on failure. No mutable global state, so the
 * function is reentrant/thread-safe.
 *
 * 导出为 rt_crypto_csprng_bytes 供 rt_ed25519.c / rt_noise.c 等其他
 * runtime TU 复用——避免每个 crypto 模块各自维护 rand() 桩，确保所有
 * 密钥生成路径都走真正的 CSPRNG（Windows: BCryptGenRandom/RtlGenRandom，
 * POSIX: /dev/urandom）。 */
int rt_crypto_csprng_bytes(uint8_t* buf, size_t len) {
#if defined(_WIN32)
    /* Prefer BCryptGenRandom (Vista+); fall back to RtlGenRandom (XP+).
     * Both are loaded at runtime so no link-time library dependency is
     * required — only kernel32 (LoadLibrary/GetProcAddress) is needed. */
    HMODULE hb = LoadLibraryA("bcrypt.dll");
    if (hb) {
        /* BCRYPT_USE_SYSTEM_PREFERRED_RNG allows hAlgorithm = NULL. */
        typedef LONG (WINAPI *PFN_BCryptGenRandom)(PVOID, PUCHAR, ULONG, ULONG);
        PFN_BCryptGenRandom fn =
            (PFN_BCryptGenRandom)GetProcAddress(hb, "BCryptGenRandom");
        if (fn) {
            LONG status = fn(NULL, buf, (ULONG)len, 0x00000002);
            FreeLibrary(hb);
            return status == 0 ? 0 : -1;
        }
        FreeLibrary(hb);
    }
    HMODULE ha = LoadLibraryA("advapi32.dll");
    if (ha) {
        /* RtlGenRandom is exported as SystemFunction036. */
        typedef BOOLEAN (WINAPI *PFN_RtlGenRandom)(PVOID, ULONG);
        PFN_RtlGenRandom fn =
            (PFN_RtlGenRandom)GetProcAddress(ha, "SystemFunction036");
        if (fn) {
            BOOLEAN ok = fn(buf, (ULONG)len);
            FreeLibrary(ha);
            return ok ? 0 : -1;
        }
        FreeLibrary(ha);
    }
    return -1;
#else
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0) return -1;
    size_t got = 0;
    while (got < len) {
        ssize_t n = read(fd, buf + got, len - got);
        if (n <= 0) {
            close(fd);
            return -1;
        }
        got += (size_t)n;
    }
    close(fd);
    return 0;
#endif
}

char* rt_crypto_random_bytes(int32_t count) {
    if (count < 0) count = 0;
    if (count == 0) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    uint8_t* buf = (uint8_t*)malloc((size_t)count);
    if (!buf) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    char* hex;
    if (rt_crypto_csprng_bytes(buf, (size_t)count) == 0) {
        hex = bytes_to_hex(buf, (size_t)count);
    } else {
        /* Failure: return empty string, never crash. */
        hex = (char*)malloc(1);
        if (hex) hex[0] = '\0';
    }
    free(buf);
    return hex;
}

/* ======================================================================= */
/* HMAC-SHA384 (RFC 2104) — inner/outer SHA-384 over a 128-byte block.      */
/* ======================================================================= */

char* rt_crypto_hmac_sha384(const char* key, const char* msg) {
    if (!key) key = "";
    if (!msg) msg = "";
    size_t klen = strlen(key);
    size_t mlen = strlen(msg);

    uint8_t k0[128];
    memset(k0, 0, 128);
    if (klen > 128) {
        uint8_t kh[48];
        sha384_hash((const uint8_t*)key, klen, kh);
        memcpy(k0, kh, 48);
    } else {
        memcpy(k0, key, klen);
    }

    uint8_t ipad[128], opad[128];
    for (int i = 0; i < 128; i++) {
        ipad[i] = (uint8_t)(k0[i] ^ 0x36);
        opad[i] = (uint8_t)(k0[i] ^ 0x5c);
    }

    uint8_t* inner_buf = (uint8_t*)malloc(128 + mlen);
    if (!inner_buf) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    memcpy(inner_buf, ipad, 128);
    memcpy(inner_buf + 128, msg, mlen);
    uint8_t inner_hash[48];
    sha384_hash(inner_buf, 128 + mlen, inner_hash);
    free(inner_buf);

    uint8_t outer_buf[128 + 48];
    memcpy(outer_buf, opad, 128);
    memcpy(outer_buf + 128, inner_hash, 48);
    uint8_t mac[48];
    sha384_hash(outer_buf, 128 + 48, mac);
    return bytes_to_hex(mac, 48);
}

/* ======================================================================= */
/* HMAC-SHA512 (RFC 2104) — inner/outer SHA-512 over a 128-byte block.      */
/* ======================================================================= */

char* rt_crypto_hmac_sha512(const char* key, const char* msg) {
    if (!key) key = "";
    if (!msg) msg = "";
    size_t klen = strlen(key);
    size_t mlen = strlen(msg);

    uint8_t k0[128];
    memset(k0, 0, 128);
    if (klen > 128) {
        uint8_t kh[64];
        sha512_hash((const uint8_t*)key, klen, kh);
        memcpy(k0, kh, 64);
    } else {
        memcpy(k0, key, klen);
    }

    uint8_t ipad[128], opad[128];
    for (int i = 0; i < 128; i++) {
        ipad[i] = (uint8_t)(k0[i] ^ 0x36);
        opad[i] = (uint8_t)(k0[i] ^ 0x5c);
    }

    uint8_t* inner_buf = (uint8_t*)malloc(128 + mlen);
    if (!inner_buf) {
        char* e = (char*)malloc(1);
        if (e) e[0] = '\0';
        return e;
    }
    memcpy(inner_buf, ipad, 128);
    memcpy(inner_buf + 128, msg, mlen);
    uint8_t inner_hash[64];
    sha512_hash(inner_buf, 128 + mlen, inner_hash);
    free(inner_buf);

    uint8_t outer_buf[128 + 64];
    memcpy(outer_buf, opad, 128);
    memcpy(outer_buf + 128, inner_hash, 64);
    uint8_t mac[64];
    sha512_hash(outer_buf, 128 + 64, mac);
    return bytes_to_hex(mac, 64);
}

/* ======================================================================= */
/* byte[] 变体（RFC 026 M3 修订）——字节进字节出，绕过 hex-string 中转。      */
/* 入参为 RtArray byte[] payload 指针，出参为新建 RtArray byte[]；           */
/* 语义与失败协议见 rt_abi.h 对应声明段。                                    */
/* ======================================================================= */

/* 把 `len` 字节摘要包成新 RtArray byte[]（失败路径仅 oom → rt_panic）。 */
static void* digest_to_array(const uint8_t* digest, size_t len) {
    void* out = rt_array_create((int32_t)len, 1);
    if (len > 0) memcpy(out, digest, len);
    return out;
}

/* RtArray byte[] → (数据指针, 长度)。NULL 按空数组处理；返回静态占位而非
 * NULL，避免下游 memcpy(dst, NULL, 0) 的未定义行为。 */
static size_t array_bytes(const void* arr, const uint8_t** out_data) {
    static const uint8_t k_empty[1] = { 0 };
    if (!arr) {
        *out_data = k_empty;
        return 0;
    }
    *out_data = (const uint8_t*)arr;
    return (size_t)rt_array_length((void*)arr);
}

void* rt_crypto_md5_arr(void* data) {
    const uint8_t* p;
    size_t len = array_bytes(data, &p);
    uint8_t digest[16];
    md5_hash(p, len, digest);
    return digest_to_array(digest, 16);
}

void* rt_crypto_sha1_arr(void* data) {
    const uint8_t* p;
    size_t len = array_bytes(data, &p);
    uint8_t digest[20];
    sha1_hash(p, len, digest);
    return digest_to_array(digest, 20);
}

void* rt_crypto_sha256_arr(void* data) {
    const uint8_t* p;
    size_t len = array_bytes(data, &p);
    uint8_t digest[32];
    sha256_hash(p, len, digest);
    return digest_to_array(digest, 32);
}

void* rt_crypto_sha384_arr(void* data) {
    const uint8_t* p;
    size_t len = array_bytes(data, &p);
    uint8_t digest[48];
    sha384_hash(p, len, digest);
    return digest_to_array(digest, 48);
}

void* rt_crypto_sha512_arr(void* data) {
    const uint8_t* p;
    size_t len = array_bytes(data, &p);
    uint8_t digest[64];
    sha512_hash(p, len, digest);
    return digest_to_array(digest, 64);
}

/* 通用 HMAC byte[] 内核（RFC 2104）。
 * block_size: SHA-256 系 64 / SHA-512 系 128；digest_len: 32/48/64。 */
static void* hmac_arr_impl(const void* key, const void* msg, int block_size,
                           void (*hash_fn)(const uint8_t*, size_t, uint8_t*),
                           size_t digest_len) {
    const uint8_t* k;
    const uint8_t* m;
    size_t klen = array_bytes(key, &k);
    size_t mlen = array_bytes(msg, &m);

    /* K0: key 零填充到块长；超长 key 先哈希（RFC 2104 §2）。 */
    uint8_t k0[128];
    memset(k0, 0, (size_t)block_size);
    if (klen > (size_t)block_size) {
        uint8_t kh[64];
        hash_fn(k, klen, kh);
        memcpy(k0, kh, digest_len);
    } else if (klen > 0) {
        memcpy(k0, k, klen);
    }

    uint8_t ipad[128], opad[128];
    for (int i = 0; i < block_size; i++) {
        ipad[i] = (uint8_t)(k0[i] ^ 0x36);
        opad[i] = (uint8_t)(k0[i] ^ 0x5c);
    }

    /* inner = H(ipad || msg) */
    uint8_t* inner_buf = (uint8_t*)malloc((size_t)block_size + mlen);
    if (!inner_buf) return NULL;
    memcpy(inner_buf, ipad, (size_t)block_size);
    if (mlen > 0) memcpy(inner_buf + block_size, m, mlen);
    uint8_t inner_hash[64];
    hash_fn(inner_buf, (size_t)block_size + mlen, inner_hash);
    free(inner_buf);

    /* outer = H(opad || inner) */
    uint8_t outer_buf[128 + 64];
    memcpy(outer_buf, opad, (size_t)block_size);
    memcpy(outer_buf + (size_t)block_size, inner_hash, digest_len);
    uint8_t mac[64];
    hash_fn(outer_buf, (size_t)block_size + digest_len, mac);
    return digest_to_array(mac, digest_len);
}

void* rt_crypto_hmac_sha256_arr(void* key, void* msg) {
    return hmac_arr_impl(key, msg, 64, sha256_hash, 32);
}

void* rt_crypto_hmac_sha384_arr(void* key, void* msg) {
    return hmac_arr_impl(key, msg, 128, sha384_hash, 48);
}

void* rt_crypto_hmac_sha512_arr(void* key, void* msg) {
    return hmac_arr_impl(key, msg, 128, sha512_hash, 64);
}

/* CSPRNG byte[] 版：count==0 返回空数组（对齐 .NET RandomNumber.GetBytes
 * 语义）；count<0 → NULL（Arc 侧先校验并抛异常，此分支仅兜底）；CSPRNG
 * 失败 → 销毁半成品并返回 NULL——彻底移除旧 rt_crypto_random_bytes 的
 * "失败返回空串"静默降级。 */
void* rt_crypto_random_bytes_arr(int32_t count) {
    if (count < 0) return NULL;
    void* out = rt_array_create(count, 1);
    if (count > 0 && rt_crypto_csprng_bytes((uint8_t*)out, (size_t)count) != 0) {
        rt_array_destroy(out);
        return NULL;
    }
    return out;
}
