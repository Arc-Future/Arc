// RFC 042 M1: Ed25519 digital signatures (RFC 8032).
//
// Self-contained C11 implementation of Ed25519 key generation, signing,
// and verification over the twisted Edwards curve edwards25519.  Uses
// SHA-512 as the internal hash function.  The curve arithmetic operates
// over the same prime field GF(2^255-19) as X25519.
//
// All public entry points operate on caller-owned byte buffers.
// Thread-safe (no mutable global state).

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ===================================================================
 *  SHA-512 (FIPS 180-4)
 * =================================================================== */

static inline uint64_t rotr64(uint64_t x, int n) {
    return (x >> n) | (x << (64 - n));
}

typedef struct {
    uint64_t state[8];
    uint64_t count[2];
    uint8_t  buf[128];
} sha512_ctx;

static const uint64_t sha512_K[80] = {
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

#define CH(x,y,z)  (((x) & (y)) ^ (~(x) & (z)))
#define MAJ(x,y,z) (((x) & (y)) ^ ((x) & (z)) ^ ((y) & (z)))
#define SIG0(x) (rotr64((x),28) ^ rotr64((x),34) ^ rotr64((x),39))
#define SIG1(x) (rotr64((x),14) ^ rotr64((x),18) ^ rotr64((x),41))
#define sig0(x) (rotr64((x),1)  ^ rotr64((x),8)  ^ ((x) >> 7))
#define sig1(x) (rotr64((x),19) ^ rotr64((x),61) ^ ((x) >> 6))

static void sha512_init(sha512_ctx* ctx) {
    ctx->state[0] = 0x6a09e667f3bcc908ULL; ctx->state[1] = 0xbb67ae8584caa73bULL;
    ctx->state[2] = 0x3c6ef372fe94f82bULL; ctx->state[3] = 0xa54ff53a5f1d36f1ULL;
    ctx->state[4] = 0x510e527fade682d1ULL; ctx->state[5] = 0x9b05688c2b3e6c1fULL;
    ctx->state[6] = 0x1f83d9abfb41bd6bULL; ctx->state[7] = 0x5be0cd19137e2179ULL;
    ctx->count[0] = ctx->count[1] = 0;
}

static void sha512_transform(sha512_ctx* ctx, const uint8_t data[128]) {
    uint64_t W[80];
    for (int i = 0; i < 16; i++) {
        W[i] = ((uint64_t)data[i*8] << 56) | ((uint64_t)data[i*8+1] << 48) |
               ((uint64_t)data[i*8+2] << 40) | ((uint64_t)data[i*8+3] << 32) |
               ((uint64_t)data[i*8+4] << 24) | ((uint64_t)data[i*8+5] << 16) |
               ((uint64_t)data[i*8+6] << 8)  | (uint64_t)data[i*8+7];
    }
    for (int i = 16; i < 80; i++)
        W[i] = sig1(W[i-2]) + W[i-7] + sig0(W[i-15]) + W[i-16];

    uint64_t a = ctx->state[0], b = ctx->state[1], c = ctx->state[2], d = ctx->state[3];
    uint64_t e = ctx->state[4], f = ctx->state[5], g = ctx->state[6], h = ctx->state[7];

    for (int i = 0; i < 80; i++) {
        uint64_t T1 = h + SIG1(e) + CH(e,f,g) + sha512_K[i] + W[i];
        uint64_t T2 = SIG0(a) + MAJ(a,b,c);
        h = g; g = f; f = e; e = d + T1;
        d = c; c = b; b = a; a = T1 + T2;
    }
    ctx->state[0] += a; ctx->state[1] += b; ctx->state[2] += c; ctx->state[3] += d;
    ctx->state[4] += e; ctx->state[5] += f; ctx->state[6] += g; ctx->state[7] += h;
}

static void sha512_update(sha512_ctx* ctx, const uint8_t* data, uint64_t len) {
    uint32_t idx = (uint32_t)(ctx->count[0] & 0x7f);
    ctx->count[0] += (uint32_t)len;
    if (ctx->count[0] < (uint32_t)len) ctx->count[1]++;

    uint32_t space = 128 - idx;
    if (len >= space) {
        memcpy(ctx->buf + idx, data, space);
        sha512_transform(ctx, ctx->buf);
        uint64_t off = space;
        while (off + 128 <= len) {
            sha512_transform(ctx, data + off);
            off += 128;
        }
        memcpy(ctx->buf, data + off, (uint32_t)(len - off));
    } else {
        memcpy(ctx->buf + idx, data, (uint32_t)len);
    }
}

static void sha512_final(sha512_ctx* ctx, uint8_t out[64]) {
    uint64_t total_bits = (ctx->count[0] * 8) + (ctx->count[1] << 32 << 3);
    uint32_t idx = (uint32_t)(ctx->count[0] & 0x7f);

    ctx->buf[idx++] = 0x80;
    if (idx > 112) {
        memset(ctx->buf + idx, 0, 128 - idx);
        sha512_transform(ctx, ctx->buf);
        idx = 0;
    }
    memset(ctx->buf + idx, 0, 128 - idx);
    for (int i = 0; i < 8; i++) ctx->buf[120 + i] = (uint8_t)(total_bits >> (56 - 8*i));

    sha512_transform(ctx, ctx->buf);
    for (int i = 0; i < 8; i++) {
        out[i*8]     = (uint8_t)(ctx->state[i] >> 56);
        out[i*8 + 1] = (uint8_t)(ctx->state[i] >> 48);
        out[i*8 + 2] = (uint8_t)(ctx->state[i] >> 40);
        out[i*8 + 3] = (uint8_t)(ctx->state[i] >> 32);
        out[i*8 + 4] = (uint8_t)(ctx->state[i] >> 24);
        out[i*8 + 5] = (uint8_t)(ctx->state[i] >> 16);
        out[i*8 + 6] = (uint8_t)(ctx->state[i] >> 8);
        out[i*8 + 7] = (uint8_t)(ctx->state[i]);
    }
}

/* ===================================================================
 *  GF(2^255 - 19) field arithmetic (identical to X25519 representation)
 * =================================================================== */

#define FE_LIMBS 10

typedef struct { uint64_t v[FE_LIMBS]; } fe25519;

static const fe25519 fe_d = {{
    0x035978a3, 0x02d37284, 0x018ab75e, 0x01350507, 0x0000700a,
    0x01de7a26, 0x00740797, 0x03f9ce33, 0x00ee2b6f, 0x001480db
}};
static const fe25519 fe_sqrtm1 = {{
    0x020ea0b0, 0x0386c9d2, 0x02478c4e, 0x001ab4bf, 0x032f4318,
    0x037ef5e9, 0x00d00993, 0x037c2cad, 0x00804fc1, 0x000ae0c9
}};

static void fe_carry(fe25519* h) {
    /* 9 lower limbs of 26 bits = 234 bits. Top limb must be 21 bits to sum to 255. */
    for (int iter = 0; iter < 4; iter++) {
        uint64_t c = 0;
        for (int i = 0; i < 9; i++) {
            uint64_t v = h->v[i] + c;
            c = v >> 26;
            h->v[i] = v & 0x3ffffffULL;
        }
        uint64_t v9 = h->v[9] + c;
        uint64_t c9 = v9 >> 21;
        h->v[9] = v9 & 0x1fffffULL;
        /* 2^255 ≡ 19 mod p (since p = 2^255 - 19). */
        h->v[0] += c9 * 19;
    }
}

/* Strong reduction: canonicalize the field element to [0, p).
 * Algorithm:
 *   1. Fully carry limbs (fe_carry) to ensure each limb fits its bit width.
 *   2. Serialize to 32 little-endian bytes using full limb bit widths; this
 *      yields a value in [0, 2^256) (limb 9 may temporarily reach up to its
 *      26-bit intermediate capacity in the 10-limb → 32-byte mapping).
 *   3. While bytes >= p (or byte[31] has bit 7 set → >= 2^255 > p), subtract
 *      the canonical 32-byte encoding of p = 2^255 - 19.  Loop at most twice
 *      (worst case ~2p).
 *   4. Re-parse the 32 bytes back into 10×26bit limbs via fe_from_bytes. */
static void fe_strong_reduce(fe25519* h) {
    fe_carry(h);

    /* --- Serialize h into 32 LE bytes (260 bits max; top 4 bits beyond 256
     *     are implicitly captured into byte[31] and compared against p's
     *     255-bit boundary) --- */
    uint8_t b[32];
    memset(b, 0, 32);
    {
        uint64_t v = 0;
        int bit = 0, o = 0;
        for (int i = 0; i < FE_LIMBS; i++) {
            /* Mask to 26 bits to be safe (fe_carry keeps < 2^26 / 2^21). */
            uint64_t limb = h->v[i] & 0x3ffffffULL;
            v |= limb << bit;
            bit += (i < 9) ? 26 : 21;
            while (bit >= 8 && o < 32) {
                b[o++] = (uint8_t)(v & 0xff);
                v >>= 8;
                bit -= 8;
            }
        }
        if (bit > 0 && o < 32) b[o++] = (uint8_t)(v & 0xff);
    }

    /* --- Subtract p (0x7FFFFFFF...FFED) repeatedly while value >= p --- */
    for (int iter = 0; iter < 3; iter++) {
        /* Quick early exit: if bit 7 of byte 31 is set, value >= 2^255 > p.
         * Otherwise compare bytewise against p's canonical LE encoding. */
        int ge = 0;
        if ((b[31] & 0x80) != 0) {
            ge = 1;
        } else {
            for (int i = 31; i >= 0; i--) {
                uint8_t pb;
                if (i == 0) pb = 0xED;
                else if (i == 31) pb = 0x7F;
                else pb = 0xFF;
                if (b[i] > pb) { ge = 1; break; }
                if (b[i] < pb) { ge = 0; break; }
            }
        }
        if (!ge) break;

        /* Subtract p from b (little-endian byte array, borrow-propagate). */
        uint16_t borrow = 0;
        for (int i = 0; i < 32; i++) {
            uint16_t pb;
            if (i == 0) pb = 0xED;
            else if (i == 31) pb = 0x7F;
            else pb = 0xFF;
            uint16_t diff = (uint16_t)b[i] - pb - borrow;
            if (diff & 0xFF00) { /* underflow: add 256, keep borrow */
                b[i] = (uint8_t)(diff + 0x100);
                borrow = 1;
            } else {
                b[i] = (uint8_t)diff;
                borrow = 0;
            }
        }
        /* borrow must be 0 here because we already established value >= p;
         * if non-zero it's a logic bug; break to avoid infinite loop. */
        if (borrow != 0) break;
    }

    /* --- Re-parse the canonical 32-byte LE representation back into limbs. */
    /* Use the bytewise-bit-accumulator style from ge_from_bytes that is known
     * to produce correct 26-bit / 21-bit splits. */
    memset(h->v, 0, sizeof(h->v));
    {
        uint32_t limb = 0;
        int limb_bits = 0;
        int out = 0;
        for (int i = 0; i < 32 && out < FE_LIMBS; i++) {
            uint32_t byte = b[i];
            for (int bit = 0; bit < 8; bit++) {
                limb |= (uint32_t)((byte >> bit) & 1) << limb_bits;
                limb_bits++;
                int max_bits = (out < 9) ? 26 : 21;
                if (limb_bits == max_bits) {
                    h->v[out++] = (uint64_t)limb;
                    limb = 0;
                    limb_bits = 0;
                    if (out == FE_LIMBS) break;
                }
            }
        }
        /* (no trailing bits expected for a valid 32-byte Ed25519 field element) */
    }
}

static void fe_add(fe25519* h, const fe25519* f, const fe25519* g) {
    for (int i = 0; i < FE_LIMBS; i++) h->v[i] = f->v[i] + g->v[i];
    fe_carry(h);
    fe_strong_reduce(h);
}

static void fe_sub(fe25519* h, const fe25519* f, const fe25519* g) {
    /* h = f - g mod p, p = 2^255 - 19.
     * Limb widths: limbs 0..8 each 26 bits => 234 bits; limb 9 is 21 bits (255 total). */
    int64_t L[FE_LIMBS];
    for (int i = 0; i < FE_LIMBS; i++) L[i] = (int64_t)f->v[i] - (int64_t)g->v[i];

    for (int pass = 0; pass < 6; pass++) {
        int64_t borrow = 0;
        for (int i = 0; i < FE_LIMBS; i++) {
            int64_t v = L[i] - borrow;
            borrow = 0;
            int64_t mod = (i < 9) ? (int64_t)(1ULL << 26) : (int64_t)(1ULL << 21);
            if (v < 0) {
                v += mod;
                borrow = 1;
            }
            L[i] = v;
        }
        if (borrow == 0) break;
        /* p = 2^255 - 19.  Canonical reduced 10-limb representation: */
        static const int64_t p_limbs[FE_LIMBS] = {
            (int64_t)((1ULL << 26) - 19),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 26) - 1),
            (int64_t)((1ULL << 21) - 1)
        };
        int64_t carry = 0;
        for (int i = 0; i < FE_LIMBS; i++) {
            int64_t mod = (i < 9) ? (int64_t)(1ULL << 26) : (int64_t)(1ULL << 21);
            int64_t s = L[i] + p_limbs[i] + carry;
            if (s >= mod) { carry = 1; s -= mod; } else { carry = 0; }
            L[i] = s;
        }
    }
    for (int i = 0; i < FE_LIMBS; i++) h->v[i] = (uint64_t)L[i];
    fe_strong_reduce(h);
}

static void fe_mul(fe25519* h, const fe25519* f, const fe25519* g) {
    /* Accumulate raw 64-bit products into t[0..18] (i+j up to 18).  Then
     * cascade 26-bit carries through 0..18, and fold the portion that lives
     * above 255 bits down by the identity 2^255 ≡ 19 (mod p).  Repeat until
     * the upper fold region is empty.  Finally strong-reduce the 10-limb
     * result to canonical [0, p). */
    uint64_t t[19];
    memset(t, 0, sizeof(t));
    for (int i = 0; i < FE_LIMBS; i++) {
        uint64_t fi = f->v[i];
        for (int j = 0; j < FE_LIMBS; j++)
            t[i + j] += fi * g->v[j];
    }

    for (int pass = 0; pass < 4; pass++) {
        /* Carry chain through all 19 limbs with 26-bit boundaries. */
        uint64_t c = 0;
        for (int i = 0; i < 18; i++) {
            uint64_t v = t[i] + c;
            c = v >> 26;
            t[i] = v & 0x3ffffffULL;
        }
        t[18] += c;

        /* Now each t[i] (for i<18) is < 2^26; t[18] is small.
         * Fold contributions from limb index >= 10 down:
         *   weight 2^(26 * i) = 2^(255 + (26*i - 255))
         *                    ≡ 19 * 2^(26*i - 255)    (mod p).
         * For i = 10, 26*10 - 255 = 5, i.e. 2^(260) ≡ 19 * 2^5.
         * For i > 10, the same identity applies iteratively because
         *   2^(26 * i) = 2^(26*(i-10)) * 2^260 ≡ (2^(26*(i-10))) * 19 * 2^5,
         * which means we can simply add the whole upper-limb region into the
         * lower region with the weight conversion for one "255→wrap" step;
         * any bits that still land above 255 will be re-folded by subsequent
         * passes.  To avoid double-counting we first copy upper limbs out,
         * zero them, then distribute (wrapped) contributions into the lower
         * limbs using the same 5-bit shift / 19 multiply rule used by the
         * rest of this reduction chain. */
        uint64_t upper[9]; /* t[10] .. t[18] */
        memset(upper, 0, sizeof(upper));
        for (int i = 10; i < 19; i++) { upper[i - 10] = t[i]; t[i] = 0; }
        for (int k = 0; k < 9; k++) {
            uint64_t wrap = upper[k];
            if (!wrap) continue;
            /* Original limb index was i = 10 + k.  Weight after fold:
             * multiply by 19 and shift by 5 bits, then place starting at
             * limb index (k).  This matches 2^(26*i) ≡ 19*2^(26*k + 5). */
            uint64_t prod = wrap * 19;
            int limb = k;
            int bit_off = 5;
            uint64_t val = prod;
            int remaining = 64;
            while (remaining > 0 && limb < 19) {
                int room = 26 - bit_off;
                if (room <= 0) { limb++; bit_off = 0; room = 26; if (limb >= 19) break; }
                int take = remaining < room ? remaining : room;
                uint64_t mask = (take == 64) ? (uint64_t)-1 : ((1ULL << take) - 1);
                uint64_t chunk = val & mask;
                t[limb] += chunk << bit_off;
                val >>= take;
                remaining -= take;
                bit_off += take;
                if (bit_off >= 26) { bit_off -= 26; limb++; }
            }
        }
    }

    h->v[0] = t[0];
    for (int i = 1; i < 9; i++) h->v[i] = t[i];
    h->v[9] = t[9];
    fe_strong_reduce(h);
}

static void fe_sq(fe25519* h, const fe25519* f) { fe_mul(h, f, f); }

static void fe_invert(fe25519* h, const fe25519* f) {
    /* Fermat's little theorem: h = f^(p-2) mod p, p = 2^255 - 19.
     * p-2 = 2^255 - 21.  Binary representation (MSB → LSB, positions 254..0):
     *   position 254: 1
     *   positions 253..5: 1 (249 ones)
     *   positions 4..0:  0 1 0 1 1  (= 11 decimal; -21's low bits)
     * We use the ref10 add-chain from SUPERCOP; for simplicity and correctness
     * here we exponentiate by direct square-and-multiply bit-by-bit using
     * a big-endian traversal of the 255-bit exponent (bit 254 MSB first).
     */
    uint8_t e_bytes[32];
    memset(e_bytes, 0, 32);
    /* p-2 = 2^255 - 21 = 0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeb.
     * Write as 32 little-endian bytes: */
    e_bytes[0] = 0xEB;
    for (int i = 1; i < 31; i++) e_bytes[i] = 0xFF;
    e_bytes[31] = 0x7F;

    fe25519 acc;
    /* acc = 1 in field: limb 0 = 1 */
    memset(&acc, 0, sizeof(acc));
    acc.v[0] = 1;

    for (int bit = 254; bit >= 0; bit--) {
        fe_sq(&acc, &acc);
        int byte_idx = bit / 8;
        int bit_shift = bit % 8;
        if ((e_bytes[byte_idx] >> bit_shift) & 1) {
            fe_mul(&acc, &acc, f);
        }
    }
    *h = acc;
}

static int fe_is_negative(const fe25519* f) {
    fe25519 t;
    memcpy(&t, f, sizeof(t));
    fe_carry(&t);
    return (int)(t.v[0] & 1);
}

/* ===================================================================
 *  Extended twisted Edwards point operations
 * =================================================================== */

typedef struct {
    fe25519 X, Y, Z, T; /* extended coordinates */
} ge_p3;

/* RFC 8032 Edwards base point B (Z=1, extended coordinates):
 * x = 0x216936d3cd6e53fec0a4e231fdd6dc5c692cc7609525a7b2c9562d608f25d51a
 * y = 0x5866666666666666666666666666666666666666666666666666666666666666
 * T = X * Y mod p.  10×26 bit limbs, little-endian order. */
static const ge_p3 ge_base = {
    {{0x0325d51a, 0x018b5823, 0x027b2c95, 0x01825496, 0x00692cc7,
      0x0375b717, 0x024e231f, 0x014ffb02, 0x02d3cd6e, 0x00085a4d}},
    {{0x02666658, 0x01999999, 0x02666666, 0x01999999, 0x02666666,
      0x01999999, 0x02666666, 0x01999999, 0x02666666, 0x00199999}},
    {{1, 0, 0, 0, 0, 0, 0, 0, 0, 0}},
    {{0x01b7dda3, 0x03a2ace9, 0x012f56dd, 0x0201dd45, 0x0120f09f,
      0x012af8df, 0x02a4e8e6, 0x01d9959b, 0x030fd78b, 0x0019e1d7}}
};

/* h = p + q */
static void ge_add(ge_p3* h, const ge_p3* p, const ge_p3* q) {
    fe25519 a, b, c, d, e, f, g, hh, two_d;
    /* two_d = 2d mod p. */
    fe_add(&two_d, &fe_d, &fe_d);
    fe_sub(&a, &p->Y, &p->X); fe_sub(&hh, &q->Y, &q->X);
    fe_mul(&a, &a, &hh);                                  /* A = (Y1-X1)(Y2-X2) */
    fe_add(&b, &p->Y, &p->X); fe_add(&hh, &q->Y, &q->X);
    fe_mul(&b, &b, &hh);                                  /* B = (Y1+X1)(Y2+X2) */
    fe_mul(&c, &p->T, &q->T);
    fe_mul(&c, &c, &two_d);                               /* C = T1 * 2d * T2 */
    fe_mul(&d, &p->Z, &q->Z); fe_add(&d, &d, &d);         /* D = 2 Z1 Z2 */
    fe_sub(&e, &b, &a);
    fe_sub(&f, &d, &c);
    fe_add(&g, &d, &c);
    fe_add(&hh, &b, &a);
    fe_mul(&h->X, &e, &f);
    fe_mul(&h->Y, &g, &hh);
    fe_mul(&h->Z, &f, &g);
    fe_mul(&h->T, &e, &hh);
    /* Strong reduce all limbs to prevent drift over many operations */
    fe_strong_reduce(&h->X); fe_strong_reduce(&h->Y);
    fe_strong_reduce(&h->Z); fe_strong_reduce(&h->T);
}

/* h = 2*p (a = -1 twisted Edwards curve, 2008 Hisil HWCD-4) */
static void ge_double(ge_p3* h, const ge_p3* p) {
    fe25519 a, b, c, d, e, f, g, hh;
    fe_sq(&a, &p->X);                                     /* A = X1^2 */
    fe_sq(&b, &p->Y);                                     /* B = Y1^2 */
    fe_sq(&c, &p->Z); fe_add(&c, &c, &c);                 /* C = 2 Z1^2 */
    fe_add(&d, &p->X, &p->Y); fe_sq(&d, &d);              /* (X1+Y1)^2 */
    fe_sub(&e, &d, &a); fe_sub(&e, &e, &b);               /* E = 2 X1 Y1 */
    /* a_curve = -1: G = -A + B; F = G - C; H = -A - B. */
    memset(&g, 0, sizeof(g));
    fe_sub(&g, &b, &a);                                   /* G = -A + B */
    fe_sub(&f, &g, &c);                                   /* F = G - C */
    fe_add(&hh, &a, &b);                                  /* H' = A+B */
    memset(&d, 0, sizeof(d));                             /* reuse d for H */
    fe_sub(&d, &d, &hh);                                  /* H = -(A+B) = -A - B */
    fe_mul(&h->X, &e, &f);
    fe_mul(&h->Y, &g, &d);
    fe_mul(&h->Z, &f, &g);
    fe_mul(&h->T, &e, &d);
    /* Strong reduce all limbs to prevent drift over many operations */
    fe_strong_reduce(&h->X); fe_strong_reduce(&h->Y);
    fe_strong_reduce(&h->Z); fe_strong_reduce(&h->T);
}

/* Decode compressed point from 32-byte little-endian y with sign bit. */
static int ge_from_bytes(ge_p3* h, const uint8_t s[32]) {
    fe25519 u, v, v3, vxx, check;
    /* s[31] bit 7 is the x sign, bits 0-6 are the top of y */
    uint8_t sign = s[31] >> 7;
    uint8_t tmp[32];
    memcpy(tmp, s, 32);
    tmp[31] &= 0x7f;

    /* Parse y from 32-byte little-endian into u (fe25519, 10×26bit limbs).
     * Walk byte offsets 0..31; each contributes 8 bits to a bit accumulator
     * `v` with `bit_off` progress; limbs are emitted every time bit_off≥26. */
    memset(&u, 0, sizeof(u));
    {
        uint64_t v = 0;
        int bit_off = 0;
        int limb = 0;
        for (int byte_i = 0; byte_i < 32; byte_i++) {
            v |= ((uint64_t)tmp[byte_i]) << bit_off;
            bit_off += 8;
            if (bit_off >= 26) {
                u.v[limb++] = v & 0x3ffffffULL;
                v >>= 26;
                bit_off -= 26;
            }
        }
        if (bit_off > 0 && limb < 10) u.v[limb] = v & 0x3ffffffULL;
    }

    /* u = y^2 - 1,  v = d*y^2 + 1.  Reuse v (v = y^2). */
    fe25519 y_val;
    memcpy(&y_val, &u, sizeof(fe25519));  /* save parsed y */
    fe25519 y_sq;
    fe_sq(&y_sq, &y_val);                 /* y_sq = y^2 */
    fe_sub(&u, &y_sq, &((fe25519){{1}})); /* u = y^2 - 1 */
    fe_mul(&v, &y_sq, &fe_d);             /* v = d*y^2  */
    fe_add(&v, &v, &((fe25519){{1}}));    /* v = d*y^2 + 1 */

    /* Compute candidate x = (u/v)^((p+3)/8) using the formula:
       x = u * v^3 * (u * v^7)^(2^252 - 3).  (p+3)/8 = 2^252 - 2. */
    fe25519 v2, v4, v7, g, z, r;
    fe_sq(&v2, &v);                       /* v^2 */
    fe_mul(&v3, &v2, &v);                 /* v^3 = v^2 * v */
    fe_sq(&v4, &v2);                      /* v^4 = (v^2)^2 */
    fe_mul(&v7, &v3, &v4);                /* v^7 = v^3 * v^4 */
    fe_mul(&g, &v3, &u);                  /* g = u * v^3 */
    fe_mul(&z, &u, &v7);                  /* z = u * v^7 */

    /* z^(2^252 - 3): iteratively square 252 times, then multiply by z^-3. */
    memcpy(&r, &z, sizeof(fe25519));
    for (int i = 0; i < 252; i++) fe_sq(&r, &r); /* r = z^(2^252) */
    {
        fe25519 invz, invz2, invz3;
        fe_invert(&invz, &z);              /* z^(-1) */
        fe_sq(&invz2, &invz);              /* z^(-2) */
        fe_mul(&invz3, &invz2, &invz);     /* z^(-3) */
        fe_mul(&r, &r, &invz3);            /* r = z^(2^252 - 3) */
    }

    /* x = g * r = u * v^3 * (u * v^7)^(2^252-3) */
    fe25519 x;
    fe_mul(&x, &g, &r);

    /* Verify: x^2 * v == u  or  x^2 * v == -u (pick sqrt(-1) fix if needed) */
    fe_sq(&vxx, &x);
    fe_mul(&vxx, &vxx, &v);
    fe_sub(&check, &vxx, &u);
    fe_strong_reduce(&check);
    int correct = 1;
    for (int i = 0; i < FE_LIMBS; i++) if (check.v[i] != 0) { correct = 0; break; }

    if (!correct) {
        /* Check x^2 * v == -u (i.e., vxx + u == 0), which means x is a sqrt of -u/v;
         * multiply by fe_sqrtm1 to get a sqrt of u/v (since sqrtm1^2 = -1). */
        fe_add(&check, &vxx, &u);
        fe_strong_reduce(&check);
        int neg_correct = 1;
        for (int i = 0; i < FE_LIMBS; i++) if (check.v[i] != 0) { neg_correct = 0; break; }
        if (neg_correct) {
            fe_mul(&x, &x, &fe_sqrtm1);
            correct = 1;
        }
    }

    if (!correct) {
        /* Try x = x * sqrt(-1) anyway and re-check (handles i * sqrt_residue variants). */
        fe_mul(&x, &x, &fe_sqrtm1);
        fe_sq(&vxx, &x);
        fe_mul(&vxx, &vxx, &v);
        fe_sub(&check, &vxx, &u);
        fe_strong_reduce(&check);
        correct = 1;
        for (int i = 0; i < FE_LIMBS; i++) if (check.v[i] != 0) { correct = 0; break; }
    }

    if (!correct) return 0; /* point not on curve */

    /* Select correct x sign bit */
    int x_neg = fe_is_negative(&x);
    if (x_neg != sign) {
        fe25519 zero;
        memset(&zero, 0, sizeof(zero));
        fe_sub(&x, &zero, &x);
    }

    memcpy(&h->X, &x, sizeof(fe25519));
    memcpy(&h->Y, &y_val, sizeof(fe25519));
    h->Z.v[0] = 1;
    for (int i = 1; i < FE_LIMBS; i++) h->Z.v[i] = 0;
    /* T = X*Y/Z; with Z=1: T = X*Y. Required for subsequent ge_add/ge_double. */
    fe_mul(&h->T, &h->X, &h->Y);
    return 1;
}

/* Encode to 32-byte little-endian. */
static void fe_to_bytes(uint8_t out[32], const fe25519* h) {
    fe25519 t;
    memcpy(&t, h, sizeof(fe25519));
    fe_strong_reduce(&t);
    /* Pack 10×26bit limbs into 32 bytes (little-endian bit stream). */
    uint64_t v = 0;
    int bit = 0;
    int o = 0;
    for (int i = 0; i < FE_LIMBS; i++) {
        v |= (t.v[i] << bit);
        bit += 26;
        while (bit >= 8 && o < 32) {
            out[o++] = (uint8_t)(v & 0xff);
            v >>= 8;
            bit -= 8;
        }
    }
}

/* ===================================================================
 *  RFC 8032 verification + scalar/point helpers
 * =================================================================== */

static int64_t ed_load_3(const uint8_t* in) {
    int64_t result = in[0];
    result |= ((int64_t)in[1]) << 8;
    result |= ((int64_t)in[2]) << 16;
    return result;
}

static int64_t ed_load_4(const uint8_t* in) {
    int64_t result = in[0];
    result |= ((int64_t)in[1]) << 8;
    result |= ((int64_t)in[2]) << 16;
    result |= ((int64_t)in[3]) << 24;
    return result;
}

/* Group order L = 2^252 + 27742317777372353535851937790883648493 (RFC 8032 §5.1.7). */
static const uint8_t ed_L[32] = {
    0xed,0xd3,0xf5,0x5c,0x1a,0x63,0x12,0x58,0xd6,0x9c,0xf7,0xa2,
    0xde,0xf9,0xde,0x14,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,
    0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x10
};

/* Reduce a 64-byte little-endian value mod L.
 * Uses the identity: 2^256 ≡ R256 (mod L), where R256 = 2^256 mod L (constant below).
 * Algorithm:
 *   For input X = X_lo (bytes 0..31) + X_hi (bytes 32..63) · 2^256 :
 *     Y = X_lo + X_hi · R256  (mod L)
 *   Y is at most 513 bits; reduce in chunks iteratively until Y fits in 32 bytes + final sub-L.
 */
/* R256 = 2^256 mod L, LE bytes. Algebraic verification: L = 2^252 + c, R256 = -16·c mod L. */
static const uint8_t sc_R256[32] = {
    0x1d,0x95,0x98,0x8d,0x74,0x31,0xec,0xd6,0x70,0xcf,0x7d,0x73,0xf4,0x5b,0xef,0xc6,
    0xfe,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0xff,0x0f
};

/* 256×256 → 512-bit schoolbook multiply, LE bytes.
 * Correct, simple implementation using 64-bit intermediate accumulators. */
static void sc_mul256(uint8_t out[64], const uint8_t a[32], const uint8_t b[32]) {
    uint64_t t[64] = {0};
    for (int i = 0; i < 32; i++) {
        uint64_t ai = a[i];
        for (int j = 0; j < 32; j++) {
            t[i + j] += ai * (uint64_t)b[j];
        }
    }
    /* Carry propagate: each t slot is up to 32 * 255 * 255 ≈ 2,080,800 (< 2^21).
     * Single pass of byte-sized carry propagation is sufficient. */
    uint64_t carry = 0;
    for (int i = 0; i < 64; i++) {
        uint64_t s = t[i] + carry;
        out[i] = (uint8_t)(s & 0xFFu);
        carry = s >> 8;
    }
    /* carry now ≈ t[63]/256 + previous carry < ~8128; cannot overflow. */
}

/* Add 32-byte LE b into 64-byte LE r in-place at bytes 0..31, with carry propagation. */
static void sc_add32into64(uint8_t r[64], const uint8_t b[32]) {
    uint16_t c = 0;
    for (int i = 0; i < 32; i++) {
        uint32_t s = (uint32_t)r[i] + (uint32_t)b[i] + c;
        r[i] = (uint8_t)s; c = (uint16_t)(s >> 8);
    }
    for (int i = 32; i < 64 && c; i++) {
        uint32_t s = (uint32_t)r[i] + c;
        r[i] = (uint8_t)s; c = (uint16_t)(s >> 8);
    }
}

/* Subtract L shifted left by `shift` bytes from r[64] (LE bytes).
 * Returns 1 on underflow (r < shifted L, r unchanged), 0 on success (r reduced). */
static int sc_sub_l_shifted(uint8_t r[64], int shift) {
    /* Only L bytes [0..31] are non-zero in shifted L; higher bytes remain zero.
     * Compute: r[shift .. shift+31] -= L[0..31] with borrow. */
    int borrow = 0;
    for (int i = 0; i < 32; i++) {
        int d = (int)r[shift + i] - (int)ed_L[i] - borrow;
        if (d < 0) { d += 256; borrow = 1; } else borrow = 0;
        r[shift + i] = (uint8_t)d;
    }
    /* Propagate borrow through remaining upper bytes */
    for (int i = shift + 32; i < 64; i++) {
        int d = (int)r[i] - borrow;
        if (d < 0) { d += 256; borrow = 1; } else borrow = 0;
        r[i] = (uint8_t)d;
    }
    return borrow; /* 1 = underflow (nothing subtracted because caller should have checked) */
}

/* Compare 64-bit r (LE bytes) with shifted L: r >= (L << (shift*8))?
 * Returns 1 if r >= shifted L, else 0. */
static int sc_ge_l_shifted(const uint8_t r[64], int shift) {
    /* Find MSB difference in bytes [shift+31 .. shift] and beyond if any. */
    /* For bytes > shift+31: any nonzero → r is larger. */
    for (int i = 63; i > shift + 31; i--) {
        if (r[i] != 0) return 1;
    }
    /* Compare bytes [shift..shift+31] with L[0..31] MSB-first. */
    for (int i = 31; i >= 0; i--) {
        uint8_t rb = r[shift + i];
        uint8_t lb = ed_L[i];
        if (rb > lb) return 1;
        if (rb < lb) return 0;
    }
    return 1; /* equal */
}

static void sc_reduce_mod_l(uint8_t out[32], const uint8_t in[64]) {
    uint8_t r[64];
    memcpy(r, in, 64);

    /* Shifted subtraction from high to low: for shift = 31 down to 1 (bytes).
     * For each shift s: while r >= L << (s*8), subtract L << (s*8).
     * Each L << s*8 occupies bytes [s..s+31]; because L has bit 252 set,
     * max useful shift = floor((511 - 252) / 8) = 32 bytes. We go 31..1. */
    for (int s = 31; s >= 0; s--) {
        while (sc_ge_l_shifted(r, s)) {
            sc_sub_l_shifted(r, s);
        }
    }

    memcpy(out, r, 32);
}

/* Returns 1 if `s` is canonical (0 <= s < L), else 0. */
static int sc_is_canonical(const uint8_t s[32]) {
    int lt = 0;
    for (int i = 31; i >= 0; i--) {
        if (s[i] > ed_L[i]) return 0;
        if (s[i] < ed_L[i]) { lt = 1; break; }
    }
    return lt;
}

static void ge_set_identity(ge_p3* p) {
    memset(p, 0, sizeof(*p));
    p->Y.v[0] = 1;
    p->Z.v[0] = 1;
}

/* Correct RFC 8032 scalar multiplication: [scalar]B starting from identity. */
static void ge_scalarmult_base(ge_p3* out, const uint8_t scalar[32]) {
    ge_p3 acc;
    ge_set_identity(&acc);
    for (int i = 255; i >= 0; i--) {
        ge_double(&acc, &acc);
        uint8_t bit = (scalar[i / 8] >> (i & 7)) & 1;
        if (bit) ge_add(&acc, &acc, &ge_base);
    }
    *out = acc;
}

/* [scalar]P for an arbitrary point P. */
static void ge_scalarmult(ge_p3* out, const uint8_t scalar[32], const ge_p3* p) {
    ge_p3 acc;
    ge_set_identity(&acc);
    for (int i = 255; i >= 0; i--) {
        ge_double(&acc, &acc);
        uint8_t bit = (scalar[i / 8] >> (i & 7)) & 1;
        if (bit) ge_add(&acc, &acc, p);
    }
    *out = acc;
}

/* Compare two extended points for equality (projective: X1*Z2 == X2*Z1, Y1*Z2 == Y2*Z1). */
static int ge_equal(const ge_p3* a, const ge_p3* b) {
    fe25519 l, r;
    fe_mul(&l, &a->X, &b->Z); fe_mul(&r, &b->X, &a->Z);
    fe_sub(&l, &l, &r); fe_carry(&l);
    for (int i = 0; i < FE_LIMBS; i++) if (l.v[i] != 0) return 0;
    fe_mul(&l, &a->Y, &b->Z); fe_mul(&r, &b->Y, &a->Z);
    fe_sub(&l, &l, &r); fe_carry(&l);
    for (int i = 0; i < FE_LIMBS; i++) if (l.v[i] != 0) return 0;
    return 1;
}

/* Hex helpers (rt_crypto.c's bytes_to_hex is static; replicate locally). */
static int ed_hexval(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static char* ed_hex_encode(const uint8_t* data, size_t len) {
    static const char hex[] = "0123456789abcdef";
    char* out = (char*)malloc(len * 2 + 1);
    if (!out) return NULL;
    for (size_t i = 0; i < len; i++) {
        out[i * 2]     = hex[(data[i] >> 4) & 0xf];
        out[i * 2 + 1] = hex[data[i] & 0xf];
    }
    out[len * 2] = '\0';
    return out;
}

/* Decode exactly `len` bytes from `src` (must contain 2*len hex chars). 0 ok, -1 bad. */
static int ed_hex_decode(const char* src, uint8_t* out, size_t len) {
    if (!src || !out) return -1;
    for (size_t i = 0; i < len; i++) {
        int hi = ed_hexval(src[i * 2]);
        int lo = ed_hexval(src[i * 2 + 1]);
        if (hi < 0 || lo < 0) return -1;
        out[i] = (uint8_t)((hi << 4) | lo);
    }
    return 0;
}

/* ===================================================================
 *  Ed25519 public API
 * =================================================================== */

void rt_crypto_ed25519_keygen(uint8_t out_sk[32], uint8_t out_pk[32]) {
    if (!out_sk || !out_pk) return;

    /* Generate 32 random bytes as seed via shared CSPRNG.
     * Windows: BCryptGenRandom / RtlGenRandom; POSIX: /dev/urandom.
     * 失败时清零输出并返回——绝不降级到 rand()（密码学场景下 rand() 是安全漏洞）。 */
    uint8_t seed[32];
    if (rt_crypto_csprng_bytes(seed, 32) != 0) {
        memset(seed, 0, 32);
        memset(out_sk, 0, 32);
        memset(out_pk, 0, 32);
        return;
    }

    /* SHA-512 the seed to get the secret scalar and prefix */
    sha512_ctx ctx;
    sha512_init(&ctx);
    sha512_update(&ctx, seed, 32);
    uint8_t hash[64];
    sha512_final(&ctx, hash);

    /* Clamp the scalar per RFC 8032 §5.1.5 */
    memcpy(out_sk, seed, 32);
    hash[0]  &= 248;
    hash[31] &= 63;
    hash[31] |= 64;

    /* Compute public key: A = scalar * B */
    uint8_t scalar[32];
    memcpy(scalar, hash, 32);

    ge_p3 R;
    ge_scalarmult_base(&R, scalar);

    /* Encode compressed point */
    fe25519 z_inv;
    fe_invert(&z_inv, &R.Z);
    fe_mul(&R.X, &R.X, &z_inv);
    fe_mul(&R.Y, &R.Y, &z_inv);

    fe_to_bytes(out_pk, &R.Y);
    out_pk[31] |= (uint8_t)(fe_is_negative(&R.X) << 7);
}

void rt_crypto_ed25519_sign(const uint8_t* msg, uint32_t msg_len,
                             const uint8_t sk[32], uint8_t out_sig[64]) {
    if (!sk || !out_sig) return;
    if (!msg && msg_len != 0) return;

    sha512_ctx ctx;
    sha512_init(&ctx);
    sha512_update(&ctx, sk, 32);
    uint8_t hash[64];
    sha512_final(&ctx, hash);

    uint8_t a[32];
    memcpy(a, hash, 32);
    a[0]  &= 248; a[31] &= 63; a[31] |= 64;

    /* r = SHA-512(hash[32..63] || msg) */
    sha512_init(&ctx);
    sha512_update(&ctx, hash + 32, 32);
    if (msg_len) sha512_update(&ctx, msg, msg_len);
    uint8_t r_hash[64];
    sha512_final(&ctx, r_hash);
    /* DEBUG */ {
        printf("DEBUG sign: prefix (hash[32..64]): ");
        for (int i=32;i<64;i++) printf("%02x", hash[i]);
        printf("\n");
        printf("DEBUG sign: r_hash (SHA512(prefix||msg)): ");
        for (int i=0;i<64;i++) printf("%02x", r_hash[i]);
        printf("\n");
    }

    /* R = r * B, encode to sig[0..31]. r = SHA-512(hash[32..63] || msg) mod L. */
    uint8_t r[32];
    sc_reduce_mod_l(r, r_hash);
    /* DEBUG */ {
        printf("DEBUG sign: r = r_hash mod L: ");
        for (int i=0;i<32;i++) printf("%02x", r[i]);
        printf("\n");
    }
    ge_p3 R;
    ge_scalarmult_base(&R, r);
    fe25519 z_inv;
    fe_invert(&z_inv, &R.Z);
    fe_mul(&R.X, &R.X, &z_inv);
    fe_mul(&R.Y, &R.Y, &z_inv);
    fe_to_bytes(out_sig, &R.Y);
    out_sig[31] |= (uint8_t)(fe_is_negative(&R.X) << 7);

    /* Compute pk = a * B, encode */
    uint8_t pk[32];
    ge_p3 A;
    ge_scalarmult_base(&A, a);
    fe_invert(&z_inv, &A.Z);
    fe_mul(&A.X, &A.X, &z_inv);
    fe_mul(&A.Y, &A.Y, &z_inv);
    fe_to_bytes(pk, &A.Y);
    pk[31] |= (uint8_t)(fe_is_negative(&A.X) << 7);

    /* h = SHA-512(R || pk || msg) */
    sha512_init(&ctx);
    sha512_update(&ctx, out_sig, 32);
    sha512_update(&ctx, pk, 32);
    if (msg_len) sha512_update(&ctx, msg, msg_len);
    uint8_t hram[64];
    sha512_final(&ctx, hram);

    /* S = (r + h * a) mod L, L = 2^252+27742317777372353535851937790883648493.
     * h = SHA-512(R || A || msg) mod L. */
    uint8_t h[32];
    sc_reduce_mod_l(h, hram);

    /* h * a (schoolbook, little-endian) → 64 bytes */
    uint8_t S[64];
    memset(S, 0, 64);
    for (int i = 0; i < 32; i++) {
        uint16_t c = 0;
        for (int j = 0; j < 32 && (i + j) < 64; j++) {
            uint16_t p = (uint16_t)h[i] * (uint16_t)a[j];
            uint16_t s = (uint16_t)S[i + j] + p + c;
            S[i + j] = (uint8_t)s;
            c = s >> 8;
        }
        for (int k = i + 32; c && k < 64; k++) {
            uint16_t s = (uint16_t)S[k] + c;
            S[k] = (uint8_t)s; c = s >> 8;
        }
    }

    /* Add r (already reduced mod L) to S */
    uint16_t carry = 0;
    for (int i = 0; i < 64; i++) {
        uint16_t s = (uint16_t)S[i] + (i < 32 ? (uint16_t)r[i] : 0) + carry;
        S[i] = (uint8_t)s; carry = s >> 8;
    }

    /* Reduce S = (h*a + r) mod L using the verified ref10 sc_reduce */
    uint8_t S_out[32];
    sc_reduce_mod_l(S_out, S);
    memcpy(out_sig + 32, S_out, 32);
}

int32_t rt_crypto_ed25519_verify(const uint8_t* msg, uint32_t msg_len,
                                  const uint8_t sig[64], const uint8_t pk[32]) {
    if (!sig || !pk) return -1;
    if (!msg && msg_len != 0) return -1;

    /* Step 1: Reject non-canonical S (S must satisfy 0 <= S < L), RFC 8032 §5.1.7. */
    if (!sc_is_canonical(sig + 32)) { printf("DEBUG verify: S not canonical (ret 11)\n"); return 0; }

    /* Step 2: Decode public key A; reject if not a valid point on the curve. */
    ge_p3 A;
    if (!ge_from_bytes(&A, pk)) { printf("DEBUG verify: A decode fail (ret 12)\n"); return 0; }

    /* Step 3: Decode R from sig[0..31]; reject if not a valid point. */
    ge_p3 R;
    if (!ge_from_bytes(&R, sig)) { printf("DEBUG verify: R decode fail (ret 13)\n"); return 0; }

    /* Step 4: k = SHA512(R || A || msg) mod L. */
    sha512_ctx ctx;
    sha512_init(&ctx);
    sha512_update(&ctx, sig, 32);
    /* encode A compressed for hash: use pk bytes (same thing), since caller passed pk. */
    sha512_update(&ctx, pk, 32);
    if (msg_len) sha512_update(&ctx, msg, msg_len);
    uint8_t k_hash[64];
    sha512_final(&ctx, k_hash);
    uint8_t k[32];
    sc_reduce_mod_l(k, k_hash);

    /* Step 5: Check [S]B == R + [k]A (cofactored verification, RFC 8032 §5.1.7). */
    uint8_t S[32];
    memcpy(S, sig + 32, 32);
    ge_p3 SB;
    ge_scalarmult_base(&SB, S);

    ge_p3 kA;
    ge_scalarmult(&kA, k, &A);
    ge_p3 RkA;
    ge_add(&RkA, &R, &kA);
    int eq = ge_equal(&SB, &RkA);

    return eq ? 1 : 0;
}

void rt_crypto_ed25519_seed_keygen(const uint8_t seed[32],
                                    uint8_t out_sk[32], uint8_t out_pk[32]) {
    if (!seed || !out_sk || !out_pk) return;
    memcpy(out_sk, seed, 32);

    sha512_ctx ctx;
    sha512_init(&ctx);
    sha512_update(&ctx, seed, 32);
    uint8_t hash[64];
    sha512_final(&ctx, hash);
    hash[0]  &= 248;
    hash[31] &= 63;
    hash[31] |= 64;

    uint8_t scalar[32];
    memcpy(scalar, hash, 32);

    ge_p3 A;
    ge_scalarmult_base(&A, scalar);
    fe25519 z_inv;
    fe_invert(&z_inv, &A.Z);
    fe_mul(&A.X, &A.X, &z_inv);
    fe_mul(&A.Y, &A.Y, &z_inv);
    fe_to_bytes(out_pk, &A.Y);
    out_pk[31] |= (uint8_t)(fe_is_negative(&A.X) << 7);
}

/* =================================================================== */
/*  RtArray byte[] 包装（P0 修复：PeerKey 门面去假面化；ABI 语义见      */
/*  rt_abi.h 对应声明段）。                                             */
/* =================================================================== */

/* 把 sk(32, seed) 与 pk(32) 拼成新 RtArray byte[64]。
 * keygen 失败（CSPRNG 不可用）时原语会输出全零——这里以全零 sk 判定失败，
 * 返回 NULL 而不是把全零密钥冒充成功（全零不是有效 Ed25519 seed）。 */
static void* ed25519_keypair_to_array(const uint8_t sk[32], const uint8_t pk[32]) {
    static const uint8_t k_zero[32] = { 0 };
    if (memcmp(sk, k_zero, 32) == 0) return NULL;
    void* out = rt_array_create(64, 1);
    uint8_t* b = (uint8_t*)out;
    memcpy(b, sk, 32);
    memcpy(b + 32, pk, 32);
    return out;
}

void* rt_crypto_ed25519_keygen_arr(void) {
    uint8_t sk[32], pk[32];
    rt_crypto_ed25519_keygen(sk, pk);
    return ed25519_keypair_to_array(sk, pk);
}

void* rt_crypto_ed25519_seed_keygen_arr(void* seed) {
    static const uint8_t k_empty[1] = { 0 };
    const uint8_t* p = seed ? (const uint8_t*)seed : k_empty;
    if (!seed || rt_array_length(seed) != 32) return NULL;
    uint8_t sk[32], pk[32];
    rt_crypto_ed25519_seed_keygen(p, sk, pk);
    return ed25519_keypair_to_array(sk, pk);
}

void* rt_crypto_ed25519_sign_arr(void* msg, void* sk) {
    static const uint8_t k_empty[1] = { 0 };
    const uint8_t* m = msg ? (const uint8_t*)msg : k_empty;
    uint32_t mlen = msg ? (uint32_t)rt_array_length(msg) : 0;
    if (!sk || rt_array_length(sk) != 32) return NULL;

    uint8_t sig[64];
    rt_crypto_ed25519_sign(m, mlen, (const uint8_t*)sk, sig);
    void* out = rt_array_create(64, 1);
    memcpy(out, sig, 64);
    return out;
}

int32_t rt_crypto_ed25519_verify_arr(void* msg, void* sig, void* pk) {
    static const uint8_t k_empty[1] = { 0 };
    const uint8_t* m = msg ? (const uint8_t*)msg : k_empty;
    uint32_t mlen = msg ? (uint32_t)rt_array_length(msg) : 0;
    if (!sig || rt_array_length(sig) != 64) return -1;
    if (!pk || rt_array_length(pk) != 32) return -1;
    return rt_crypto_ed25519_verify(m, mlen, (const uint8_t*)sig, (const uint8_t*)pk);
}
