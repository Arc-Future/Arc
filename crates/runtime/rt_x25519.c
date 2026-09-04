// RFC 042 M1: X25519 Diffie-Hellman key exchange (RFC 7748 §5).
//
// Self-contained C11 implementation of the X25519 function using the
// Montgomery ladder over the prime field GF(2^255 - 19).  The scalar
// is clamped per RFC 7748.  Used by the Noise Protocol Framework
// (Noise_XK) for ephemeral-static and static-static DH operations.
//
// Correctness contract: verified byte-for-byte against the RFC 7748 §5.2
// official test vectors (previously exercised end-to-end through the
// Noise_XK handshake by noise_xk_vectors_e2e; retired with
// arc-integration, a2627a0f).
// The previous 10×26-bit field multiplication reduced mod 2^255-19
// incorrectly (limbs 10..18 were dropped), producing wrong DH outputs; it
// was replaced with the canonical 5×51-bit representation (RFC 7748
// reference construction) using 128-bit accumulation.
//
// All arithmetic is constant-time (fixed instruction sequence; the only
// branch is the square-and-multiply exponentiation over the *public*
// exponent p-2).  Thread-safe (no mutable global state).

#include "rt_abi.h"
#include <string.h>

/* ===================================================================
 *  Field elements: 5 limbs of 51 bits (little-endian), value = Σ v[i]·2^(51i)
 * =================================================================== */

#define MASK51 0x7ffffffffffffULL   /* 2^51 - 1 */
#define A24    121665               /* (A - 2) / 4 for Curve25519 */

typedef struct { uint64_t v[5]; } fe51;

static const fe51 fe51_zero = {{0, 0, 0, 0, 0}};
static const fe51 fe51_one  = {{1, 0, 0, 0, 0}};

static void fe51_0(fe51* h) { *h = fe51_zero; }
static void fe51_1(fe51* h) { *h = fe51_one; }

/* h = f + g  (limbs < 2^51 each → result limbs < 2^52) */
static void fe51_add(fe51* h, const fe51* f, const fe51* g) {
    h->v[0] = f->v[0] + g->v[0];
    h->v[1] = f->v[1] + g->v[1];
    h->v[2] = f->v[2] + g->v[2];
    h->v[3] = f->v[3] + g->v[3];
    h->v[4] = f->v[4] + g->v[4];
}

/* h = f - g (mod p).  Adds 2p = 2^256 - 38 (as limb vector
 * [2^51-38, 2^51-1, 2^51-1, 2^51-1, 2^52-1], ≡ 0 mod p) so the
 * subtraction never underflows; then carries to canonical limbs < 2^51. */
static void fe51_sub(fe51* h, const fe51* f, const fe51* g) {
    __int128 d0 = (__int128)f->v[0] - g->v[0] + ((1ULL << 51) - 38);
    __int128 d1 = (__int128)f->v[1] - g->v[1] + ((1ULL << 51) - 1);
    __int128 d2 = (__int128)f->v[2] - g->v[2] + ((1ULL << 51) - 1);
    __int128 d3 = (__int128)f->v[3] - g->v[3] + ((1ULL << 51) - 1);
    __int128 d4 = (__int128)f->v[4] - g->v[4] + ((1ULL << 52) - 1);
    /* Carry + fold mod 2^255-19, twice (fully canonicalizes). */
    for (int pass = 0; pass < 2; pass++) {
        d1 += d0 >> 51; d0 &= MASK51;
        d2 += d1 >> 51; d1 &= MASK51;
        d3 += d2 >> 51; d2 &= MASK51;
        d4 += d3 >> 51; d3 &= MASK51;
        __int128 c = d4 >> 51; d4 &= MASK51;
        d0 += 19 * c;                 /* 2^255 ≡ 19 (mod p) */
        d1 += d0 >> 51; d0 &= MASK51;
    }
    h->v[0] = (uint64_t)d0; h->v[1] = (uint64_t)d1;
    h->v[2] = (uint64_t)d2; h->v[3] = (uint64_t)d3;
    h->v[4] = (uint64_t)d4;
}

/* h = f * g (mod p).  Schoolbook 5×5 → 9 partial products, folded with the
 * identity 2^255 ≡ 19 (mod p): products at weights 2^255/2^306/2^357/2^408
 * fold into weights 2^0/2^51/2^102/2^153 with coefficient 19.  Output limbs
 * are canonical (< 2^51).  Alias-safe (inputs are read into locals first). */
static void fe51_mul(fe51* h, const fe51* f, const fe51* g) {
    uint64_t a0 = f->v[0], a1 = f->v[1], a2 = f->v[2], a3 = f->v[3], a4 = f->v[4];
    uint64_t b0 = g->v[0], b1 = g->v[1], b2 = g->v[2], b3 = g->v[3], b4 = g->v[4];
    __int128 r0 = (__int128)a0 * b0;
    __int128 r1 = (__int128)a0 * b1 + (__int128)a1 * b0;
    __int128 r2 = (__int128)a0 * b2 + (__int128)a1 * b1 + (__int128)a2 * b0;
    __int128 r3 = (__int128)a0 * b3 + (__int128)a1 * b2 + (__int128)a2 * b1 + (__int128)a3 * b0;
    __int128 r4 = (__int128)a0 * b4 + (__int128)a1 * b3 + (__int128)a2 * b2 + (__int128)a3 * b1 + (__int128)a4 * b0;
    __int128 r5 = (__int128)a1 * b4 + (__int128)a2 * b3 + (__int128)a3 * b2 + (__int128)a4 * b1;
    __int128 r6 = (__int128)a2 * b4 + (__int128)a3 * b3 + (__int128)a4 * b2;
    __int128 r7 = (__int128)a3 * b4 + (__int128)a4 * b3;
    __int128 r8 = (__int128)a4 * b4;

    __int128 t0 = r0 + 19 * r5;
    __int128 t1 = r1 + 19 * r6;
    __int128 t2 = r2 + 19 * r7;
    __int128 t3 = r3 + 19 * r8;
    __int128 t4 = r4;

    for (int pass = 0; pass < 2; pass++) {
        t1 += t0 >> 51; t0 &= MASK51;
        t2 += t1 >> 51; t1 &= MASK51;
        t3 += t2 >> 51; t2 &= MASK51;
        t4 += t3 >> 51; t3 &= MASK51;
        __int128 c = t4 >> 51; t4 &= MASK51;
        t0 += 19 * c;
        t1 += t0 >> 51; t0 &= MASK51;
    }
    h->v[0] = (uint64_t)t0; h->v[1] = (uint64_t)t1;
    h->v[2] = (uint64_t)t2; h->v[3] = (uint64_t)t3;
    h->v[4] = (uint64_t)t4;
}

/* h = f² */
static void fe51_sq(fe51* h, const fe51* f) { fe51_mul(h, f, f); }

/* h = s * f  (small scalar, e.g. a24) */
static void fe51_scalar_mul(fe51* h, const fe51* f, uint64_t s) {
    __int128 t0 = (__int128)f->v[0] * s;
    __int128 t1 = (__int128)f->v[1] * s;
    __int128 t2 = (__int128)f->v[2] * s;
    __int128 t3 = (__int128)f->v[3] * s;
    __int128 t4 = (__int128)f->v[4] * s;
    for (int pass = 0; pass < 2; pass++) {
        t1 += t0 >> 51; t0 &= MASK51;
        t2 += t1 >> 51; t1 &= MASK51;
        t3 += t2 >> 51; t2 &= MASK51;
        t4 += t3 >> 51; t3 &= MASK51;
        __int128 c = t4 >> 51; t4 &= MASK51;
        t0 += 19 * c;
        t1 += t0 >> 51; t0 &= MASK51;
    }
    h->v[0] = (uint64_t)t0; h->v[1] = (uint64_t)t1;
    h->v[2] = (uint64_t)t2; h->v[3] = (uint64_t)t3;
    h->v[4] = (uint64_t)t4;
}

/* h = f^(p-2) = f^(2^255 - 21), via square-and-multiply over the fixed
 * public exponent.  Constant-time w.r.t. f (the exponent is public). */
static void fe51_invert(fe51* h, const fe51* z) {
    /* e = 2^255 - 21 = 0x7F FF … FF EB (little-endian bytes). */
    static const uint8_t e[32] = {
        0xEB, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F,
    };
    fe51 acc;
    fe51_1(&acc);
    for (int i = 254; i >= 0; i--) {
        fe51_mul(&acc, &acc, &acc);
        if ((e[i / 8] >> (i & 7)) & 1) fe51_mul(&acc, &acc, z);
    }
    *h = acc;
}

/* Constant-time conditional swap (RFC 7748 cswap). */
static void fe51_cswap(fe51* a, fe51* b, uint64_t swap) {
    uint64_t mask = 0ULL - swap;
    for (int i = 0; i < 5; i++) {
        uint64_t d = (a->v[i] ^ b->v[i]) & mask;
        a->v[i] ^= d;
        b->v[i] ^= d;
    }
}

/* Decode 32-byte little-endian u-coordinate (top bit already cleared). */
static void fe51_from_bytes(fe51* h, const uint8_t s[32]) {
    uint64_t c0, c1, c2, c3;
    memcpy(&c0, s, 8); memcpy(&c1, s + 8, 8);
    memcpy(&c2, s + 16, 8); memcpy(&c3, s + 24, 8);
    h->v[0] = c0 & MASK51;
    h->v[1] = ((c0 >> 51) | (c1 << 13)) & MASK51;
    h->v[2] = ((c1 >> 38) | (c2 << 26)) & MASK51;
    h->v[3] = ((c2 >> 25) | (c3 << 39)) & MASK51;
    h->v[4] = (c3 >> 12) & MASK51;
}

/* Encode canonical field element to 32 bytes little-endian.  If the value is
 * ≥ p, subtracts p in constant time (the value is < 2^255 after our carries,
 * so a single conditional subtraction canonicalizes). */
static void fe51_to_bytes(uint8_t out[32], const fe51* h) {
    uint64_t a0 = h->v[0], a1 = h->v[1], a2 = h->v[2], a3 = h->v[3], a4 = h->v[4];
    /* q = a - p via a borrow chain; p limbs = [2^51-19, 2^51-1, ×3]. */
    uint64_t q0 = a0 + 19;                 /* a0 + 2^51 - (2^51 - 19) */
    uint64_t b0 = 1 - (q0 >> 51);          /* borrow from limb 1 if a0 < 2^51 - 19 */
    q0 &= MASK51;
    uint64_t q1 = a1 + 1 - b0;
    uint64_t b1 = 1 - (q1 >> 51);
    q1 &= MASK51;
    uint64_t q2 = a2 + 1 - b1;
    uint64_t b2 = 1 - (q2 >> 51);
    q2 &= MASK51;
    uint64_t q3 = a3 + 1 - b2;
    uint64_t b3 = 1 - (q3 >> 51);
    q3 &= MASK51;
    uint64_t q4 = a4 + 1 - b3;
    uint64_t b4 = 1 - (q4 >> 51);          /* 1 ⇒ a < p (subtraction underflowed) */
    q4 &= MASK51;
    uint64_t mask = 0ULL - (1 - b4);       /* all-ones iff a >= p */
    a0 = (q0 & mask) | (a0 & ~mask);
    a1 = (q1 & mask) | (a1 & ~mask);
    a2 = (q2 & mask) | (a2 & ~mask);
    a3 = (q3 & mask) | (a3 & ~mask);
    a4 = (q4 & mask) | (a4 & ~mask);
    /* Pack 255 bits into 32 bytes (little-endian). */
    uint64_t c0 = a0 | (a1 << 51);
    uint64_t c1 = (a1 >> 13) | (a2 << 38);
    uint64_t c2 = (a2 >> 26) | (a3 << 25);
    uint64_t c3 = (a3 >> 39) | (a4 << 12);
    memcpy(out, &c0, 8); memcpy(out + 8, &c1, 8);
    memcpy(out + 16, &c2, 8); memcpy(out + 24, &c3, 8);
}

/* ===================================================================
 *  Public X25519 DH (RFC 7748 §5)
 * =================================================================== */

void rt_crypto_x25519_dh(const uint8_t sk[32], const uint8_t pk[32],
                          uint8_t out_shared[32]) {
    if (!sk || !pk || !out_shared) return;

    /* Clamp the scalar per RFC 7748 §5; clear the top bit of the
     * u-coordinate (implementations must ignore it). */
    uint8_t scalar[32], u[32];
    memcpy(scalar, sk, 32);
    memcpy(u, pk, 32);
    scalar[0] &= 248;
    scalar[31] &= 127;
    scalar[31] |= 64;
    u[31] &= 127;

    fe51 x1, x2, z2, x3, z3;
    fe51_from_bytes(&x1, u);
    fe51_1(&x2);
    fe51_0(&z2);
    fe51_from_bytes(&x3, u);
    fe51_1(&z3);

    /* Montgomery ladder (RFC 7748 §5 pseudocode). */
    uint64_t swap = 0;
    for (int i = 254; i >= 0; i--) {
        uint64_t bit = (scalar[i / 8] >> (i & 7)) & 1;
        swap ^= bit;
        fe51_cswap(&x2, &x3, swap);
        fe51_cswap(&z2, &z3, swap);
        swap = bit;

        fe51 A, AA, B, BB, E, C, D, DA, CB, t1, t2;
        fe51_add(&A, &x2, &z2);
        fe51_sq(&AA, &A);
        fe51_sub(&B, &x2, &z2);
        fe51_sq(&BB, &B);
        fe51_sub(&E, &AA, &BB);
        fe51_add(&C, &x3, &z3);
        fe51_sub(&D, &x3, &z3);
        fe51_mul(&DA, &D, &A);
        fe51_mul(&CB, &C, &B);
        fe51_add(&t1, &DA, &CB);
        fe51_sq(&x3, &t1);
        fe51_sub(&t2, &DA, &CB);
        fe51_sq(&t2, &t2);
        fe51_mul(&z3, &x1, &t2);
        fe51_mul(&x2, &AA, &BB);
        fe51_scalar_mul(&t1, &E, A24);
        fe51_add(&t1, &AA, &t1);
        fe51_mul(&z2, &E, &t1);
    }
    fe51_cswap(&x2, &x3, swap);
    fe51_cswap(&z2, &z3, swap);

    /* Result = x2 · z2^(p-2) */
    fe51 inv, res;
    fe51_invert(&inv, &z2);
    fe51_mul(&res, &x2, &inv);
    fe51_to_bytes(out_shared, &res);
}
