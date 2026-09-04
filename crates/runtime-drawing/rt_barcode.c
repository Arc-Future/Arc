/* rt_barcode.c — RFC 029 M4 二维码解码 ABI（vendored quirc 底座包装器）
 *
 * 本 TU 把 quirc 上游多文件库合并进单一编译单元：quirc 上游 lib/ 为
 *   quirc.c / decode.c / identify.c / version_db.c（+ quirc.h / quirc_internal.h），
 * 经 #include 合并进本 TU（对齐 rt_image.c 合并 stb 单文件头的形态）；
 * 上游文件保持原样、不改动，便于版本 diff 与更新（RFC 029 §1.3「两种写法
 * 均给出建议，实现 Sprint 以可维护性定稿」）。函数级 section GC
 * （-ffunction-sections -fdata-sections + --gc-sections//OPT:REF）对本 TU
 * 仍生效：未引用函数不进最终产物。
 *
 * ABI 清单（对齐 RFC 029 §1.5；返回值约定：成功 0 / 失败非零）：
 *   rt_barcode_quirc_decode   RGBA8 像素 → quirc 灰度 → 解码首个 QR →
 *                             text_out（NUL 终止）
 *
 * M4 仅实现 quirc 静态内置路径（rt_barcode_quirc_*）；zxing-cpp 通用解码
 * 增强（rt_barcode_zxing_*）属 M5，勿在本里程碑实现。
 *
 * 像素输入形态：RGBA8（8-bit 每通道、直通 alpha；对齐 std/Drawing Bitmap
 * 的 Rgba32 语义）。RGBA → 灰度采用整数加权 luma：Y = (77R + 150G + 29B) >> 8
 * （0.299R + 0.587G + 0.114B，系数乘 256 取整，和恰为 256）。
 *
 * 防御：非法输入（NULL / w≤0 / h≤0 / text_cap=0 / quirc 分配失败 / 无码 /
 * 解码失败）一律返回非零，不崩溃（对齐 M1 rt_image.c 防御纪律）。
 *
 * 注册（rfc036-int 收口完成）：本 TU（quirc 单 TU 合并）已在
 * prepare_runtime_objects（crates/codegen/src/llvm_ir/mod.rs）注册并注入
 * `-I crates/runtime-drawing`；分派经 runtime_decls.rs declare + emit_call.rs
 * `BarcodeNative.QuircDecode` → `rt_barcode_quirc_decode`（text_cap i32→size_t
 * zext i64）。
 */

#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <stdlib.h>

#include "quirc.h"
/* quirc 上游实现合并进本 TU（保持上游文件原样） */
#include "quirc.c"
#include "version_db.c"
#include "decode.c"
#include "identify.c"

int32_t rt_barcode_quirc_decode(const uint8_t* rgba, int32_t w, int32_t h,
                                char* text_out, size_t text_cap) {
    if (!rgba || w <= 0 || h <= 0 || !text_out || text_cap == 0) return 1;

    struct quirc* q = quirc_new();
    if (!q) return 1;
    if (quirc_resize(q, w, h) < 0) {
        quirc_destroy(q);
        return 1;
    }

    uint8_t* image = quirc_begin(q, NULL, NULL);
    if (!image) {
        quirc_destroy(q);
        return 1;
    }

    size_t n = (size_t)w * (size_t)h;
    for (size_t i = 0; i < n; i++) {
        const uint8_t* px = rgba + i * 4;
        uint32_t y = (uint32_t)px[0] * 77u + (uint32_t)px[1] * 150u +
                     (uint32_t)px[2] * 29u;
        image[i] = (uint8_t)(y >> 8);
    }

    quirc_end(q);

    int32_t ret = 1;
    if (quirc_count(q) > 0) {
        struct quirc_code code;
        struct quirc_data data;
        quirc_extract(q, 0, &code);
        if (quirc_decode(&code, &data) == QUIRC_SUCCESS && data.payload_len > 0) {
            size_t len = (size_t)data.payload_len;
            size_t cap = text_cap - 1;
            if (len > cap) len = cap;
            memcpy(text_out, data.payload, len);
            text_out[len] = '\0';
            ret = 0;
        }
    }

    quirc_destroy(q);
    return ret;
}

/* ═══════════════════════════════════════════════════════════════════════
 * 原生 1D 解码（rt_barcode_1d_decode）
 *
 * 通用 1D 条码解码：EAN-13 / Code39 / Code128，三种自不自依赖、无外部库。
 * 算法（对齐 M3 生成器的图案表，可证伪往返）：
 *   1. RGBA → 灰度 → Otsu 全局阈值 → 二值；
 *   2. 逐行扫描：提取游程（bar/space），以最窄游程为模块宽（1 模块），
 *      归一化游程为模块数，定位首黑条→末黑条的数据区；
 *   3. 依序尝试 EAN-13（95 模块 guard 结构）→ Code128（start/值/校验/stop）
 *      → Code39（9 元素字符 + 窄空 gap）；
 *   4. 首个成功即返回载荷文本（NUL 终止），全部失败返回失败码。
 *
 * 图案表与 std/Drawing/BarcodeWriter.as 的 M3 生成表同源（标准公开规范），
 * 保证「自编自解」往返一致性。table 以字符串字面量内联（对齐 M3 表形态）。
 */

/* ── 1D 图案表（与 BarcodeWriter.as 同源） ── */
static const char RT_C39_DIGITS[] =
    "00011010010010000100110000110110000000011000110011000000111000000"
    "0100101100100100001100100";
static const char RT_C39_UPPER[] =
    "10000100100100100110100100000001100110001100000101100000000110110"
    "00011000010011000000111001000000110010000111010000100000100111000"
    "10010001010010000000111100000110001000110000010110110000001011000"
    "001111000000010010001110010000011010000";
static const char RT_EAN_L[] =
    "0001101001100100100110111101010001101100010101111011101101101110001011";
static const char RT_EAN_G[] =
    "0100111011001100110110100001001110101110010000101001000100010010010111";
static const char RT_EAN_R[] =
    "1110010110011011011001000010101110010011101010000100010010010001110100";
static const char RT_EAN_MASK[] =
    "000000001011001101001110010011011001011100010101010110011010";
static const char RT_C128[] =
    "110110011001100110110011001100110100100110001001000110010001"
    "001100100110010001001100010010001100100110010010001100100010"
    "011000100100101100111001001101110010011001110101110011001001"
    "110110010011100110110011100101100101110011001001110110111001"
    "001100111010011101101110111010011001110010110011100100110111"
    "011001001110011010011100110010110110110001101100011011000110"
    "110101000110001000101100010001000110101100010001000110100010"
    "001100010110100010001100010100011000100010101101110001011000"
    "111010001101110101110110001011100011010001110110111011101101"
    "101000111011000101110110111010001101110001011011101110111010"
    "110001110100011011100010110111011010001110110001011100011010"
    "111011110101100100001011110001010101001100001010000110010010"
    "110000100100001101000010110010000100110101100100001011000010"
    "010011010000100110000101000011010010000110010110000100101100"
    "101000011110111010110000101001000111101010100111100100101111"
    "001001001111010111100100100111101001001111001011110100100111"
    "100101001111001001011011011110110111101101111011011010101111"
    "000101000111101000101111010111101000101111000101111010100011"
    "110100010101110111101011110111011101011110111101011101101000"
    "01001101001000011010011100";
static const char RT_C128_STOP[] = "1100011101011";

/* ── 运行（bar/space）刻画 ── */
typedef struct {
    int color; /* 1=黑条 0=白空 */
    int mods;  /* 模块数 */
} rt_bd_run;

#define RT_BD_MAXRUN 600
#define RT_BD_MAXMOD 600

/* 提取一行二值数据的模块级游程序列；返回数据区游程数，失败 -1。 */
static int rt_bd_row_runs(const uint8_t* row, int w, rt_bd_run* runs, int cap) {
    int raww[512];
    uint8_t rawc[512];
    int nraw = 0;
    int i = 0;
    while (i < w) {
        uint8_t c = row[i];
        int s = i;
        while (i < w && row[i] == c) i++;
        if (nraw < 512) {
            raww[nraw] = i - s;
            rawc[nraw] = c;
            nraw++;
        }
    }
    if (nraw < 3) return -1;
    int module = raww[0];
    for (i = 1; i < nraw; i++)
        if (raww[i] < module) module = raww[i];
    if (module < 1 || module > 64) return -1;
    int first = -1, last = -1;
    for (i = 0; i < nraw; i++)
        if (rawc[i]) {
            if (first < 0) first = i;
            last = i;
        }
    if (first < 0 || last < 0) return -1;
    int n = 0;
    for (i = first; i <= last && n < cap; i++) {
        int m = (raww[i] + module / 2) / module;
        if (m < 1) m = 1;
        runs[n].color = rawc[i];
        runs[n].mods = m;
        n++;
    }
    return n;
}

/* 游程序列 → 模块位序列（1=黑 0=白）。返回模块数。 */
static int rt_bd_build_modules(const rt_bd_run* runs, int n, uint8_t* mods, int cap) {
    int m = 0;
    for (int i = 0; i < n && m < cap; i++)
        for (int k = 0; k < runs[i].mods && m < cap; k++)
            mods[m++] = (uint8_t)runs[i].color;
    return m;
}

/* 7 位 EAN 图案匹配：mods[off..off+7] 是否等于 table[d]*7。 */
static int rt_bd_match7(const uint8_t* mods, int off, const char* table, int d) {
    for (int k = 0; k < 7; k++)
        if (mods[off + k] != (uint8_t)(table[d * 7 + k] == '1' ? 1 : 0)) return 0;
    return 1;
}

/* 11 位 Code128 图案匹配。 */
static int rt_bd_match11(const uint8_t* mods, int off, const char* table, int v) {
    for (int k = 0; k < 11; k++)
        if (mods[off + k] != (uint8_t)(table[v * 11 + k] == '1' ? 1 : 0)) return 0;
    return 1;
}

/* 13 位 Code128 Stop 匹配。 */
static int rt_bd_match_stop(const uint8_t* mods, int off) {
    for (int k = 0; k < 13; k++)
        if (mods[off + k] != (uint8_t)(RT_C128_STOP[k] == '1' ? 1 : 0)) return 0;
    return 1;
}

/* EAN-13 解码；成功 writes 13 位数字到 out，返回 1。 */
static int rt_bd_try_ean(const uint8_t* mods, int nmod, char* out, int cap) {
    if (nmod != 95) return 0;
    if (mods[0] != 1 || mods[1] != 0 || mods[2] != 1) return 0;
    int left[6], gbits = 0;
    for (int i = 0; i < 6; i++) {
        int off = 3 + i * 7, d = -1, g = 0;
        for (int dd = 0; dd < 10; dd++)
            if (rt_bd_match7(mods, off, RT_EAN_L, dd)) { d = dd; break; }
        if (d < 0)
            for (int dd = 0; dd < 10; dd++)
                if (rt_bd_match7(mods, off, RT_EAN_G, dd)) { d = dd; g = 1; break; }
        if (d < 0) return 0;
        left[i] = d;
        if (g) gbits |= (1 << i);
    }
    if (mods[45] != 0 || mods[46] != 1 || mods[47] != 0 || mods[48] != 1 ||
        mods[49] != 0)
        return 0;
    int right[6];
    for (int i = 0; i < 6; i++) {
        int off = 50 + i * 7, d = -1;
        for (int dd = 0; dd < 10; dd++)
            if (rt_bd_match7(mods, off, RT_EAN_R, dd)) { d = dd; break; }
        if (d < 0) return 0;
        right[i] = d;
    }
    if (mods[92] != 1 || mods[93] != 0 || mods[94] != 1) return 0;
    int first = -1;
    for (int f = 0; f < 10; f++) {
        int m = 0;
        for (int k = 0; k < 6; k++)
            if (RT_EAN_MASK[f * 6 + k] == '1') m |= (1 << k);
        if (m == gbits) { first = f; break; }
    }
    if (first < 0) return 0;
    int idx = 0;
    if (cap < 14) return 0;
    out[idx++] = (char)('0' + first);
    for (int i = 0; i < 6; i++) out[idx++] = (char)('0' + left[i]);
    for (int i = 0; i < 6; i++) out[idx++] = (char)('0' + right[i]);
    out[idx] = 0;
    return 1;
}

/* Code128 解码；成功 writes 载荷到 out，返回 1。 */
static int rt_bd_try_code128(const uint8_t* mods, int nmod, char* out, int cap) {
    int start = -1;
    for (int v = 103; v <= 105; v++)
        if (rt_bd_match11(mods, 0, RT_C128, v)) { start = v; break; }
    if (start < 0) return 0;
    int values[300], nv = 0;
    values[nv++] = start;
    int off = 11;
    while (off + 11 <= nmod) {
        if (off + 13 <= nmod && rt_bd_match_stop(mods, off)) break;
        int v = -1;
        for (int vv = 0; vv <= 105; vv++)
            if (rt_bd_match11(mods, off, RT_C128, vv)) { v = vv; break; }
        if (v < 0) return 0;
        values[nv++] = v;
        off += 11;
        if (nv >= 298) break;
    }
    if (nv < 3) return 0;
    /* 校验符 = (start*1 + data[i]*i) % 103；校验符值本身不参与求和（末位）。 */
    int sum = values[0];
    for (int i = 1; i < nv - 1; i++) sum += values[i] * i;
    if (values[nv - 1] != sum % 103) return 0;
    int di = 0, subset = start - 103;
    for (int i = 1; i < nv - 1; i++) {
        int v = values[i];
        if (subset == 2) {
            if (v < 100 && di + 2 < cap) {
                out[di++] = (char)('0' + v / 10);
                out[di++] = (char)('0' + v % 10);
            }
        } else if (subset == 1) {
            if (di < cap) out[di++] = (char)(v + 32);
        } else {
            if (di < cap) out[di++] = (char)v;
        }
    }
    out[di] = 0;
    return di > 0;
}

/* Code39 9 元素图案匹配 runs[i..i+9]。 */
static int rt_bd_c39_match(const rt_bd_run* runs, int i, const char* pat) {
    for (int k = 0; k < 9; k++) {
        int want_wide = pat[k] == '1' ? 1 : 0;
        int want_color = (k % 2 == 0) ? 1 : 0;
        if (runs[i + k].color != want_color) return 0;
        if (((runs[i + k].mods >= 2) ? 1 : 0) != want_wide) return 0;
    }
    return 1;
}

/* Code39 字符查找：返回匹配字符，未匹配返回 0。 */
static char rt_bd_c39_lookup(const rt_bd_run* runs, int i) {
    for (int d = 0; d < 10; d++)
        if (rt_bd_c39_match(runs, i, RT_C39_DIGITS + d * 9)) return (char)('0' + d);
    for (int d = 0; d < 26; d++)
        if (rt_bd_c39_match(runs, i, RT_C39_UPPER + d * 9)) return (char)('A' + d);
    static const char* SYMS = "-. $/+%*";
    static const char* SYMP[] = {
        "010000101", "110000100", "011000100", "010101000",
        "010100010", "010001010", "000101010", "010010100",
    };
    for (int d = 0; d < 8; d++)
        if (rt_bd_c39_match(runs, i, SYMP[d])) return SYMS[d];
    return 0;
}

/* Code39 解码；成功 writes 载荷到 out，返回 1。 */
static int rt_bd_try_c39(const rt_bd_run* runs, int n, char* out, int cap) {
    if (n < 19) return 0;
    int i = 0;
    if (rt_bd_c39_lookup(runs, i) != '*') return 0;
    i += 9;
    int di = 0;
    for (;;) {
        if (i >= n) return 0;
        i += 1; /* gap（白空） */
        if (i + 9 > n) return 0;
        char c = rt_bd_c39_lookup(runs, i);
        if (c == 0) return 0;
        if (c == '*') break;
        if (di < cap - 1) out[di++] = c;
        i += 9;
    }
    out[di] = 0;
    return di > 0;
}

/* Otsu 全局阈值。 */
static uint8_t rt_bd_otsu(const uint8_t* gray, size_t n) {
    int hist[256];
    memset(hist, 0, sizeof(hist));
    for (size_t i = 0; i < n; i++) hist[gray[i]]++;
    size_t total = n, wB = 0;
    double sum = 0, sumB = 0, best = -1;
    int th = 128;
    for (int i = 0; i < 256; i++) sum += (double)i * hist[i];
    for (int t = 0; t < 256; t++) {
        wB += (size_t)hist[t];
        if (wB == 0) continue;
        size_t wF = total - wB;
        if (wF == 0) break;
        sumB += (double)t * hist[t];
        double mB = sumB / wB, mF = (sum - sumB) / wF;
        double var = (double)wB * wF * (mB - mF) * (mB - mF);
        if (var > best) { best = var; th = t; }
    }
    return (uint8_t)th;
}

int32_t rt_barcode_1d_decode(const uint8_t* rgba, int32_t w, int32_t h,
                             char* text_out, size_t text_cap) {
    if (!rgba || w <= 0 || h <= 0 || !text_out || text_cap == 0) return 1;

    size_t n = (size_t)w * (size_t)h;
    uint8_t* gray = (uint8_t*)malloc(n);
    if (!gray) return 1;
    for (size_t i = 0; i < n; i++) {
        const uint8_t* px = rgba + i * 4;
        gray[i] = (uint8_t)(((uint32_t)px[0] * 77u + (uint32_t)px[1] * 150u +
                             (uint32_t)px[2] * 29u) >>
                            8);
    }
    uint8_t th = rt_bd_otsu(gray, n);
    uint8_t* bin = (uint8_t*)malloc(n);
    if (!bin) {
        free(gray);
        return 1;
    }
    for (size_t i = 0; i < n; i++) bin[i] = gray[i] <= th ? 1 : 0;
    free(gray);

    int32_t ret = 1;
    for (int32_t y = 0; y < h; y++) {
        const uint8_t* row = bin + (size_t)y * w;
        rt_bd_run runs[RT_BD_MAXRUN];
        int nr = rt_bd_row_runs(row, w, runs, RT_BD_MAXRUN);
        if (nr <= 0) continue;
        char tmp[512];
        /* Code39 直接按游程解析 */
        if (rt_bd_try_c39(runs, nr, tmp, (int)sizeof(tmp))) {
            size_t len = strlen(tmp);
            if (len < text_cap) {
                memcpy(text_out, tmp, len + 1);
                ret = 0;
                break;
            }
        }
        /* EAN-13 / Code128 按模块位序列解析 */
        uint8_t mods[RT_BD_MAXMOD];
        int nmod = rt_bd_build_modules(runs, nr, mods, RT_BD_MAXMOD);
        if (nmod == 95 && rt_bd_try_ean(mods, nmod, tmp, (int)sizeof(tmp))) {
            size_t len = strlen(tmp);
            if (len < text_cap) {
                memcpy(text_out, tmp, len + 1);
                ret = 0;
                break;
            }
        }
        if (rt_bd_try_code128(mods, nmod, tmp, (int)sizeof(tmp))) {
            size_t len = strlen(tmp);
            if (len < text_cap) {
                memcpy(text_out, tmp, len + 1);
                ret = 0;
                break;
            }
        }
    }
    free(bin);
    return ret;
}
