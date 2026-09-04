// SIMD 运行时检测 ABI 实现（RFC 009 M3 + M7 优化）。
//
// 跨平台 CPU 特征检测：
//   - x86-64：__cpuid（Windows/MSVC）/ __builtin_cpu_supports（GCC/Clang）
//   - ARM64：getauxval AT_HWCAP（Linux）/ sysctlbyname（macOS）
//   - 其他：保守降级到标量
//
// 检测维度：
//   - rt_simd_width_bytes：最大可用向量宽度（64=AVX-512, 32=AVX2, 16=SSE2/NEON, 0=标量）
//   - rt_simd_supports_fma：FMA3 指令集
//   - rt_simd_supports_avx512：AVX-512F 基础指令集
//   - rt_simd_supports_gather：AVX2 gather 指令集
//
// 多平台说明：
//   - Windows x86-64：__cpuid intrinsic（intrin.h），检测 AVX/AVX2/AVX-512/FMA
//   - Linux/macOS x86-64：__builtin_cpu_supports（GCC/Clang 编译器内置）
//   - Linux ARM64：getauxval(AT_HWCAP) 检测 NEON/SVE
//   - macOS ARM64：sysctlbyname("hw.optional.neon") 检测 NEON
//   - 其他架构：保守返回 NEON/SSE2 基线或标量

#include "rt_abi.h"

/* ==== 平台检测 ==== */
#if defined(_MSC_VER) && (defined(_M_X64) || defined(_M_IX86))
#define RT_SIMD_X86_MSVC 1
#elif (defined(__GNUC__) || defined(__clang__)) && (defined(__x86_64__) || defined(__i386__))
#define RT_SIMD_X86_GCC 1
#elif defined(__aarch64__) || defined(_M_ARM64)
#define RT_SIMD_ARM64 1
#endif

/* ==== x86-64 MSVC：__cpuid 检测 ==== */
#if defined(RT_SIMD_X86_MSVC)
#include <intrin.h>

static int32_t g_simd_width = -1;
static int32_t g_simd_fma = -1;
static int32_t g_simd_avx512 = -1;
static int32_t g_simd_gather = -1;

static void rt_simd_init(void) {
    if (g_simd_width >= 0) return;  /* 已初始化 */

    int32_t width = 0;
    int32_t fma = 0;
    int32_t avx512 = 0;
    int32_t gather = 0;

    int cpuinfo[4];

    /* leaf 1: ECX bit 28 = AVX, bit 12 = FMA */
    __cpuid(cpuinfo, 1);
    int has_avx = (cpuinfo[2] >> 28) & 1;
    int has_fma_cpuid = (cpuinfo[2] >> 12) & 1;

    /* 需检查 OSXSAVE + AVX 操作系统支持 */
    int osxsave = (cpuinfo[2] >> 27) & 1;
    if (osxsave && has_avx) {
        /* leaf 7: EBX bit 5 = AVX2, bit 29 = AVX-512F */
        __cpuidex(cpuinfo, 7, 0);
        int has_avx2 = (cpuinfo[1] >> 5) & 1;
        int has_avx512f = (cpuinfo[1] >> 16) & 1;

        /* xgetbv 检查 YMM/ZMM 状态 */
        unsigned long long xcr0 = _xgetbv(0);
        int ymm_enabled = (xcr0 & 0x6) == 0x6;  /* bit 1-2: YMM */
        int zmm_enabled = (xcr0 & 0xE0) == 0xE0;  /* bit 5-7: ZMM/Opmask */

        if (has_avx2 && ymm_enabled) {
            width = 32;  /* AVX2 = 32 字节 */
            gather = 1;  /* AVX2 gather */
        } else if (ymm_enabled) {
            width = 32;  /* AVX = 32 字节（无 gather） */
        }

        if (has_avx512f && zmm_enabled) {
            width = 64;  /* AVX-512 = 64 字节 */
            avx512 = 1;
            gather = 1;  /* AVX-512 也支持 gather */
        }

        if (has_fma_cpuid && ymm_enabled) {
            fma = 1;  /* FMA3 */
        }
    }

    /* SSE2 基线（所有 x86-64 均支持） */
    if (width == 0) {
        width = 16;
    }

    g_simd_width = width;
    g_simd_fma = fma;
    g_simd_avx512 = avx512;
    g_simd_gather = gather;
}

int32_t rt_simd_width_bytes(void) {
    rt_simd_init();
    return g_simd_width;
}

int32_t rt_simd_supports_fma(void) {
    rt_simd_init();
    return g_simd_fma;
}

int32_t rt_simd_supports_avx512(void) {
    rt_simd_init();
    return g_simd_avx512;
}

int32_t rt_simd_supports_gather(void) {
    rt_simd_init();
    return g_simd_gather;
}

/* ==== x86-64 GCC/Clang：__builtin_cpu_supports 检测 ==== */
#elif defined(RT_SIMD_X86_GCC)

/* GCC/Clang 提供编译器内置 CPU 特征检测，首次调用自动初始化 */
int32_t rt_simd_width_bytes(void) {
#if defined(__AVX512F__)
    return 64;  /* 编译时硬编码 AVX-512 */
#elif defined(__AVX2__)
    return 32;  /* 编译时硬编码 AVX2 */
#else
    /* 运行时检测：__builtin_cpu_supports 在 main 前自动初始化 */
    if (__builtin_cpu_supports("avx512f")) return 64;
    if (__builtin_cpu_supports("avx2")) return 32;
    if (__builtin_cpu_supports("avx")) return 32;
    return 16;  /* SSE2 基线 */
#endif
}

int32_t rt_simd_supports_fma(void) {
#if defined(__FMA__)
    return 1;
#else
    return __builtin_cpu_supports("fma") ? 1 : 0;
#endif
}

int32_t rt_simd_supports_avx512(void) {
#if defined(__AVX512F__)
    return 1;
#else
    return __builtin_cpu_supports("avx512f") ? 1 : 0;
#endif
}

int32_t rt_simd_supports_gather(void) {
#if defined(__AVX2__)
    return 1;
#else
    return __builtin_cpu_supports("avx2") ? 1 : 0;
#endif
}

/* ==== ARM64：NEON 基线 + SVE 可选 ==== */
#elif defined(RT_SIMD_ARM64)

#if defined(__linux__)
#include <sys/auxv.h>
#include <asm/hwcap.h>
#endif

int32_t rt_simd_width_bytes(void) {
#if defined(__linux__)
    /* Linux ARM64: getauxval(AT_HWCAP) 检测 SVE */
    unsigned long hwcap = getauxval(AT_HWCAP);
    if (hwcap & HWCAP_SVE) return 64;  /* SVE: 可变长向量，最小 128-bit，典型 256/512-bit */
#endif
    /* ARM64 NEON 是基线，128-bit = 16 字节 */
    return 16;
}

int32_t rt_simd_supports_fma(void) {
    /* ARM64 NEON 内置 FMA 支持 */
    return 1;
}

int32_t rt_simd_supports_avx512(void) {
    /* ARM 无 AVX-512 */
    return 0;
}

int32_t rt_simd_supports_gather(void) {
#if defined(__linux__)
    unsigned long hwcap = getauxval(AT_HWCAP);
    if (hwcap & HWCAP_SVE) return 1;  /* SVE 支持 gather */
#endif
    return 0;  /* NEON 无硬件 gather */
}

/* ==== 其他架构：保守降级 ==== */
#else

int32_t rt_simd_width_bytes(void) {
    /* 未知架构：保守返回 0（标量），避免 codegen 发射非法向量指令 */
    return 0;
}

int32_t rt_simd_supports_fma(void) {
    return 0;
}

int32_t rt_simd_supports_avx512(void) {
    return 0;
}

int32_t rt_simd_supports_gather(void) {
    return 0;
}

#endif
