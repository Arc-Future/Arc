/* rt_font.c — RFC 029 M6 图形绘制与字体光栅化 ABI（vendored stb_truetype 底座包装器）
 *
 * 本 TU 把 stb_truetype（TTF/OTF 字形光栅化 + 度量）实例化为独立编译单元：
 *   #define STB_TRUETYPE_IMPLEMENTATION
 * 在 #include 之前定义，单文件头实现只在本 TU 实例化一次。
 *
 * 独立 TU 取舍（不并入 rt_image.c，注释说明）：
 *   1. 编译期裁剪（RFC 029 §1.2 机制③）：TU 级「不引用不链接」——仅编解码的
 *      程序不链接本 TU（无 rt_image_font_* 引用），仅绘制的程序不链接 rt_image.o
 *      的 stb 编解码实现；若并入 rt_image.c，stb_truetype 实现会随 M1 图像程序
 *      一起编译进 TU（虽可被 section GC 裁掉，TU 边界更清晰、裁剪断言更稳）。
 *   2. 安全面隔离：stb_truetype.h 头部声明「NO SECURITY GUARANTEE — DO NOT USE
 *      THIS ON UNTRUSTED FONT FILES」（不做字体偏移范围检查）；与图像解码安全面
 *      相互独立、独立演进（RFC 029 §4 R3 / VENDOR.md M6 注记）。
 *   3. 上游更新独立 PR：stb_truetype.h 升级不触碰 rt_image.c（VENDOR.md 更新纪律）。
 *
 * ABI 清单（对齐 RFC 029 §1.5；返回值约定：成功 0 / 失败非零）：
 *   rt_image_font_load      加载 TTF/OTF 内存 → 不透明 font handle
 *                           （含 stbtt_fontinfo + scale + ascent/descent/line_gap 预计算）
 *   rt_image_font_metrics   取字体度量（ascent/descent/line_gap）
 *   rt_image_font_measure   步进度量（逐字符 advance + kerning 累加，UTF-8 输入）
 *   rt_image_font_glyph     取字形单通道灰度位图（alpha 覆盖；bitmap_out=0 查询
 *                           尺寸 / 1 填充；DrawText 合成时按颜色着色）
 *   rt_image_font_free      释放 font handle
 *
 * 释放语义：RtFont 与 stbtt 位图缓冲均由本 TU malloc 分配，free 释放。
 *
 * 防御：非法输入（NULL / len=0 / 字体表损坏 / 越界偏移）一律返回失败码，不崩溃。
 *
 * 注册（rfc036-int 收口完成）：本 TU（stb_truetype 单实例）已在
 * prepare_runtime_objects（crates/codegen/src/llvm_ir/mod.rs）注册并注入
 * `-I crates/runtime-drawing`；分派经 runtime_decls.rs declare + builtin_dispatch.rs/
 * emit_call.rs `Font::_*` → `rt_image_font_*`（long 句柄 inttoptr ↔ ptr 往返）。
 */

#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"

/* ---- 不透明 font handle ---- */

typedef struct {
    stbtt_fontinfo info;   /* stbtt 字体表（InitFont 后） */
    float scale;           /* ScaleForPixelHeight(size) 预计算 */
    float ascent;          /* 像素域 ascent（已乘 scale） */
    float descent;         /* 像素域 descent（已乘 scale，通常为负） */
    float line_gap;        /* 像素域 line_gap（已乘 scale） */
} RtFont;

/* 最小 UTF-8 解码（供 measure 遍历；NUL 结尾，非法序列退回单字节处理） */
static int rt_font_utf8_decode(const uint8_t* s, uint32_t* out_cp) {
    uint8_t b0 = s[0];
    if (b0 < 0x80) { *out_cp = b0; return 1; }
    if ((b0 & 0xE0) == 0xC0) {
        if ((s[1] & 0xC0) != 0x80) { *out_cp = b0; return 1; }
        *out_cp = ((uint32_t)(b0 & 0x1F) << 6) | (uint32_t)(s[1] & 0x3F);
        return 2;
    }
    if ((b0 & 0xF0) == 0xE0) {
        if ((s[1] & 0xC0) != 0x80 || (s[2] & 0xC0) != 0x80) { *out_cp = b0; return 1; }
        *out_cp = ((uint32_t)(b0 & 0x0F) << 12) |
                  ((uint32_t)(s[1] & 0x3F) << 6) |
                  (uint32_t)(s[2] & 0x3F);
        return 3;
    }
    if ((b0 & 0xF8) == 0xF0) {
        if ((s[1] & 0xC0) != 0x80 || (s[2] & 0xC0) != 0x80 || (s[3] & 0xC0) != 0x80) {
            *out_cp = b0;
            return 1;
        }
        *out_cp = ((uint32_t)(b0 & 0x07) << 18) |
                  ((uint32_t)(s[1] & 0x3F) << 12) |
                  ((uint32_t)(s[2] & 0x3F) << 6) |
                  (uint32_t)(s[3] & 0x3F);
        return 4;
    }
    *out_cp = b0;
    return 1;
}

/* ---- M6 主 ABI（RFC 029 §1.5） ---- */

void* rt_image_font_load(const uint8_t* ttf, size_t len, float size) {
    if (!ttf || len == 0 || size <= 0.0f) return NULL;
    int offset = stbtt_GetFontOffsetForIndex(ttf, 0);
    if (offset < 0 || (size_t)offset >= len) return NULL;
    RtFont* f = (RtFont*)calloc(1, sizeof(RtFont));
    if (!f) return NULL;
    if (!stbtt_InitFont(&f->info, ttf, offset)) {
        free(f);
        return NULL;
    }
    f->scale = stbtt_ScaleForPixelHeight(&f->info, size);
    if (f->scale <= 0.0f) {
        free(f);
        return NULL;
    }
    int ascent = 0, descent = 0, line_gap = 0;
    stbtt_GetFontVMetrics(&f->info, &ascent, &descent, &line_gap);
    f->ascent = (float)ascent * f->scale;
    f->descent = (float)descent * f->scale;
    f->line_gap = (float)line_gap * f->scale;
    return f;
}

int32_t rt_image_font_metrics(void* font, float* ascent, float* descent, float* line_gap) {
    if (!font || !ascent || !descent || !line_gap) return 1;
    RtFont* f = (RtFont*)font;
    *ascent = f->ascent;
    *descent = f->descent;
    *line_gap = f->line_gap;
    return 0;
}

float rt_image_font_measure(void* font, const char* text) {
    if (!font || !text) return 0.0f;
    RtFont* f = (RtFont*)font;
    const uint8_t* s = (const uint8_t*)text;
    float x = 0.0f;
    int prev = 0; /* 上一字形 codepoint，用于 kerning（0 = 无上一字形） */
    while (*s) {
        uint32_t cp = 0;
        int n = rt_font_utf8_decode(s, &cp);
        int adv = 0, lsb = 0;
        stbtt_GetCodepointHMetrics(&f->info, (int)cp, &adv, &lsb);
        x += (float)adv * f->scale;
        if (prev != 0) {
            x += (float)stbtt_GetCodepointKernAdvance(&f->info, prev, (int)cp) * f->scale;
        }
        prev = (int)cp;
        s += (size_t)n;
    }
    return x;
}

/* RtFont 内部访问——供 wgpu atlas 直接读 scale/info（同链接单元，不破坏封装）。
 * 返回 stbtt_fontinfo*（opaque）和像素 scale；wgpu 侧可直接调用 stbtt_* 族函数，
 * 避免为每个 metrics 单独增加 ABI（RFC 037 M3 atlas 热路径需要 HMetrics/Kerning）。 */
const void* rt_image_font_get_stbtt_info(void* font) {
    if (!font) return NULL;
    return &((RtFont*)font)->info;
}

float rt_image_font_get_scale(void* font) {
    if (!font) return 0.0f;
    return ((RtFont*)font)->scale;
}

/* 内部核心：按给定 scale 光栅化字形（单通道 alpha 覆盖位图）。
 * bitmap_out=NULL 仅查询尺寸/metrics。 */
static int32_t rt_font_glyph_raster(RtFont* f, uint32_t codepoint, float scale,
                                     uint8_t* bitmap_out,
                                     int32_t* w, int32_t* h,
                                     float* xoff, float* yoff, float* advance) {
    int adv = 0, lsb = 0;
    stbtt_GetCodepointHMetrics(&f->info, (int)codepoint, &adv, &lsb);
    *advance = (float)adv * scale;
    if (bitmap_out == NULL) {
        int ix0 = 0, iy0 = 0, ix1 = 0, iy1 = 0;
        stbtt_GetCodepointBitmapBox(&f->info, (int)codepoint, scale, scale,
                                    &ix0, &iy0, &ix1, &iy1);
        *w = ix1 - ix0;
        *h = iy1 - iy0;
        *xoff = (float)ix0;
        *yoff = (float)iy0;
        return 0;
    }
    int gw = 0, gh = 0, gox = 0, goy = 0;
    unsigned char* bmp = stbtt_GetCodepointBitmap(&f->info, scale, scale,
                                                  (int)codepoint, &gw, &gh, &gox, &goy);
    if (!bmp) return 1;
    if (gw > 0 && gh > 0) {
        size_t n = (size_t)gw * (size_t)gh;
        memcpy(bitmap_out, bmp, n);
    }
    stbtt_FreeBitmap(bmp, NULL);
    *w = gw;
    *h = gh;
    *xoff = (float)gox;
    *yoff = (float)goy;
    return 0;
}

/* 完整字形光栅化：返回单通道 alpha 位图 + gw/gh + xoff/yoff + advance（像素）。
 * bitmap_out=NULL 仅查询尺寸（同 rt_image_font_glyph），但同时输出 advance。 */
int32_t rt_image_font_glyph_full(void* font, uint32_t codepoint, uint8_t* bitmap_out,
                                  int32_t* w, int32_t* h, float* xoff, float* yoff,
                                  float* advance) {
    if (!font || !w || !h || !xoff || !yoff || !advance) return 1;
    return rt_font_glyph_raster((RtFont*)font, codepoint, ((RtFont*)font)->scale,
                                bitmap_out, w, h, xoff, yoff, advance);
}

/* 按物理像素高度光栅化（per-size bucket 文本管线）：scale 由 pixel_height 现算，
 * 使 atlas 字形与目标屏幕像素 1:1，采样零缩放（正文锐度的决定性前提）。 */
int32_t rt_image_font_glyph_full_px(void* font, uint32_t codepoint, double pixel_height,
                                     uint8_t* bitmap_out,
                                     int32_t* w, int32_t* h,
                                     float* xoff, float* yoff, float* advance) {
    if (!font || !w || !h || !xoff || !yoff || !advance) return 1;
    if (pixel_height < 4.0 || pixel_height > 512.0) return 1;
    RtFont* f = (RtFont*)font;
    float scale = stbtt_ScaleForPixelHeight(&f->info, (float)pixel_height);
    return rt_font_glyph_raster(f, codepoint, scale,
                                bitmap_out, w, h, xoff, yoff, advance);
}

int32_t rt_image_font_glyph(void* font, uint32_t codepoint, uint8_t* bitmap_out,
                            int32_t* w, int32_t* h, float* xoff, float* yoff) {
    float adv_dummy;
    return rt_image_font_glyph_full(font, codepoint, bitmap_out, w, h, xoff, yoff, &adv_dummy);
}

/// 检查字体是否包含指定 codepoint 的字形（非 .notdef）。
/// 返回 1 = 有字形，0 = 无字形，<0 = 错误。用于多字体回退链逐字符查找。
int32_t rt_image_font_has_glyph(void* font, uint32_t codepoint) {
    if (!font) return -1;
    RtFont* f = (RtFont*)font;
    int glyph_index = stbtt_FindGlyphIndex(&f->info, (int)codepoint);
    return (glyph_index > 0) ? 1 : 0;
}

void rt_image_font_free(void* font) {
    if (font) free(font);
}
