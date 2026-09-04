/* rt_image.c — RFC 029 M1 图像编解码 ABI（vendored stb 底座包装器）
 *
 * 本 TU 把 stb_image (decode) + stb_image_write (encode) 合并进单一编译单元：
 *   #define STB_IMAGE_IMPLEMENTATION / STB_IMAGE_WRITE_IMPLEMENTATION
 * 在 #include 之前定义，单文件头实现只在本 TU 实例化一次。
 *
 * ABI 清单（对齐 RFC 029 §1.5；返回值约定：成功 0 / 失败非零）：
 *   rt_image_decode       解码内存字节 → RGBA8 像素缓冲（stbi 分配，rt_image_free 释放）
 *   rt_image_decode_file  解码文件路径 → RGBA8 像素缓冲（同上）
 *   rt_image_encode_png   像素 → PNG 内存缓冲（本包装 malloc 分配，rt_image_free 释放）
 *   rt_image_encode_jpg   像素 → JPEG 内存缓冲（同上）
 *   rt_image_free         释放 decode/encode 输出的像素/编码缓冲
 *
 * M1 Bitmap 像素面补充 ABI（std/Drawing/Bitmap 使用；不属 RFC §1.5 主清单）：
 *   rt_image_alloc        分配 w*h*4 字节零初始化 RGBA8 缓冲（空 Bitmap 画布）
 *   rt_image_get_pixel    取像素 → 打包 ARGB（int64 0x00000000AARRGGBB；(x,y) 越界 -1）
 *   rt_image_set_pixel    写像素（打包 ARGB；成功 1 / 越界 0）
 *   rt_image_fill_rect    实心矩形填充（打包 ARGB；越界裁剪；std P2 批渲染专用）
 *   rt_image_write_buffer 把编码缓冲原样写入文件路径（Save 落盘辅助）
 *
 * 释放语义（rt_image_free）：stb 默认分配器即 C 标准库 malloc/free
 * （STBI_MALLOC/STBI_FREE 未重定义），因此 stb 解码返回的缓冲（stbi_image_free）
 * 与本包装 malloc 的编码缓冲（free）都可用同一 `free()` 安全释放；
 * 实现统一走 `stbi_image_free`（= STBI_FREE = free）。
 *
 * 防御：非法输入（NULL / len=0 / 越界 / stb 失败）一律返回失败码，不崩溃。
 *
 * 编译裁剪（RFC 029 §1.2）：不硬编码 STBI_NO_* 能力宏——本 TU 内所有解码器
 * （png/jpg/bmp/gif/tga/hdr）与编码器（png/jpg/bmp/tga）都编译进 .o，靠链接期
 * section GC（-ffunction-sections -fdata-sections + --gc-sections//OPT:REF）裁掉
 * 未引用函数；裁剪断言见 imaging_prune_e2e。
 */

#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#define STB_IMAGE_IMPLEMENTATION
#define STB_IMAGE_WRITE_IMPLEMENTATION
#include "stb_image.h"
#include "stb_image_write.h"

/* ---- GIF/SVG 解码器（RFC 029 M2 · Image 多格式扩展）----
 *
 * GIF：stb_image 的 stbi_load_gif_from_memory 一次性解码全部帧到连续
 * RGBA8 缓冲（frames*w*h*4 字节），并返回每帧延时（毫秒）数组。两者均由
 * stb 分配，rt_image_free（= STBI_FREE = free）释放。
 *
 * SVG：vendored nanosvg（zlib）解析 + 光栅化。单文件实现宏仅在本 TU
 * 实例化一次；输出预乘 alpha 的 RGBA8，本包装解预乘为直通 RGBA8（与
 * stb/纹理采样语义一致）。输出由本 TU malloc 分配，rt_image_free 释放。
 *
 * 链接裁剪：decode_svg/gif_frame 未被引用时经 section GC 裁掉。
 */
#define NANOSVG_IMPLEMENTATION
#define NANOSVGRAST_IMPLEMENTATION
#include "nanosvg.h"
#include "nanosvgrast.h"

/* ---- 内存编码缓冲（stbi_write_*_to_func 回调写入） ---- */

typedef struct {
    uint8_t* data;
    size_t len;
    size_t cap;
    int ok;
} RtImageBuf;

static void rt_image_write_fn(void* context, void* data, int size) {
    RtImageBuf* b = (RtImageBuf*)context;
    if (!b->ok || size < 0) return;
    size_t need = b->len + (size_t)size;
    if (need > b->cap) {
        size_t ncap = b->cap ? b->cap * 2 : 4096;
        while (ncap < need) ncap *= 2;
        uint8_t* nd = (uint8_t*)realloc(b->data, ncap);
        if (!nd) { b->ok = 0; return; }
        b->data = nd;
        b->cap = ncap;
    }
    memcpy(b->data + b->len, data, (size_t)size);
    b->len += (size_t)size;
}

/* ---- M1 主 ABI（RFC 029 §1.5） ---- */

int32_t rt_image_decode(const uint8_t* data, size_t len, uint8_t** out_rgba, int32_t* w, int32_t* h) {
    if (!data || len == 0 || !out_rgba || !w || !h) return 1;
    *out_rgba = NULL;
    int32_t iw = 0, ih = 0, comp = 0;
    stbi_uc* px = stbi_load_from_memory(data, (int)len, &iw, &ih, &comp, 4);
    if (!px || iw <= 0 || ih <= 0) return 1;
    *out_rgba = (uint8_t*)px;
    *w = iw;
    *h = ih;
    return 0;
}

int32_t rt_image_decode_file(const char* path, uint8_t** out_rgba, int32_t* w, int32_t* h) {
    if (!path || !out_rgba || !w || !h) return 1;
    *out_rgba = NULL;
    int32_t iw = 0, ih = 0, comp = 0;
    stbi_uc* px = stbi_load(path, &iw, &ih, &comp, 4);
    if (!px || iw <= 0 || ih <= 0) return 1;
    *out_rgba = (uint8_t*)px;
    *w = iw;
    *h = ih;
    return 0;
}

int32_t rt_image_encode_png(const uint8_t* rgba, int32_t w, int32_t h, uint8_t** out, size_t* out_len) {
    if (!rgba || w <= 0 || h <= 0 || !out || !out_len) return 1;
    if (w > INT32_MAX / 4) return 1; /* stride w*4 防溢出 */
    RtImageBuf b = {0};
    b.ok = 1; /* 初始 ok=0 会令 rt_image_write_fn 首写即丢弃 */
    if (!stbi_write_png_to_func(rt_image_write_fn, &b, w, h, 4, rgba, w * 4)) {
        free(b.data);
        return 1;
    }
    if (!b.ok) { free(b.data); return 1; }
    *out = b.data;
    *out_len = b.len;
    return 0;
}

int32_t rt_image_encode_jpg(const uint8_t* rgba, int32_t w, int32_t h, int32_t quality, uint8_t** out, size_t* out_len) {
    if (!rgba || w <= 0 || h <= 0 || quality < 1 || quality > 100 || !out || !out_len) return 1;
    if (w > INT32_MAX / 4) return 1;
    RtImageBuf b = {0};
    b.ok = 1; /* 初始 ok=0 会令 rt_image_write_fn 首写即丢弃 */
    if (!stbi_write_jpg_to_func(rt_image_write_fn, &b, w, h, 4, rgba, quality)) {
        free(b.data);
        return 1;
    }
    if (!b.ok) { free(b.data); return 1; }
    *out = b.data;
    *out_len = b.len;
    return 0;
}

void rt_image_free(uint8_t* p) {
    /* stb 默认分配器 = C malloc（STBI_MALLOC 未重定义），故 decode 的 stbi
     * 缓冲与 encode 的 malloc 缓冲统一以 stbi_image_free（= STBI_FREE = free）
     * 释放。若未来重定义 STBI_MALLOC，此处需同步区分。 */
    if (p) stbi_image_free(p);
}

/* ---- M1 Bitmap 像素面补充 ABI ---- */

uint8_t* rt_image_alloc(int32_t w, int32_t h) {
    if (w <= 0 || h <= 0) return NULL;
    if ((uint64_t)w * (uint64_t)h > (uint64_t)SIZE_MAX / 4) return NULL;
    size_t n = (size_t)w * (size_t)h * 4;
    uint8_t* p = (uint8_t*)malloc(n);
    if (!p) return NULL;
    memset(p, 0, n);
    return p;
}

int64_t rt_image_get_pixel(const uint8_t* rgba, int32_t w, int32_t h, int32_t x, int32_t y) {
    if (!rgba || w <= 0 || h <= 0 || x < 0 || y < 0 || x >= w || y >= h) return -1;
    const uint8_t* p = rgba + ((int64_t)y * (int64_t)w + (int64_t)x) * 4;
    return ((int64_t)p[3] << 24) | ((int64_t)p[0] << 16) | ((int64_t)p[1] << 8) | (int64_t)p[2];
}

int32_t rt_image_set_pixel(uint8_t* rgba, int32_t w, int32_t h, int32_t x, int32_t y, int64_t argb) {
    if (!rgba || w <= 0 || h <= 0 || x < 0 || y < 0 || x >= w || y >= h) return 0;
    uint8_t* p = rgba + ((int64_t)y * (int64_t)w + (int64_t)x) * 4;
    p[0] = (uint8_t)((argb >> 16) & 0xFF);
    p[1] = (uint8_t)((argb >> 8) & 0xFF);
    p[2] = (uint8_t)(argb & 0xFF);
    p[3] = (uint8_t)((argb >> 24) & 0xFF);
    return 1;
}

/* std P2 效率批：实心矩形填充（BarcodeWriter._Render 白底/黑条批渲染，
 * 替代逐像素 SetPixel 循环——O(像素) 次 FFI 收敛为 O(矩形) 次）。
 * 打包 ARGB 与 set_pixel 同构；矩形与画布求交裁剪（负坐标/越界安全）。
 * 返回 1 = 有写入 / 0 = 无效输入或完全越界。 */
int32_t rt_image_fill_rect(uint8_t* rgba, int32_t w, int32_t h, int32_t x, int32_t y, int32_t rw, int32_t rh, int64_t argb) {
    if (!rgba || w <= 0 || h <= 0 || rw <= 0 || rh <= 0) return 0;
    int32_t x0 = x < 0 ? 0 : x;
    int32_t y0 = y < 0 ? 0 : y;
    int32_t x1 = (int64_t)x + (int64_t)rw > (int64_t)w ? w : x + rw;
    int32_t y1 = (int64_t)y + (int64_t)rh > (int64_t)h ? h : y + rh;
    if (x0 >= x1 || y0 >= y1) return 0;
    uint8_t r = (uint8_t)((argb >> 16) & 0xFF);
    uint8_t g = (uint8_t)((argb >> 8) & 0xFF);
    uint8_t b = (uint8_t)(argb & 0xFF);
    uint8_t a = (uint8_t)((argb >> 24) & 0xFF);
    for (int32_t yy = y0; yy < y1; yy++) {
        uint8_t* row = rgba + ((int64_t)yy * (int64_t)w + (int64_t)x0) * 4;
        for (int32_t xx = x0; xx < x1; xx++) {
            row[0] = r; row[1] = g; row[2] = b; row[3] = a;
            row += 4;
        }
    }
    return 1;
}

int32_t rt_image_write_buffer(const char* path, const uint8_t* buf, size_t len) {
    if (!path || !buf || len == 0) return 0;
    FILE* f = fopen(path, "wb");
    if (!f) return 0;
    int ok = fwrite(buf, 1, len, f) == len ? 1 : 0;
    fclose(f);
    return ok;
}

/* ---- RFC 029 M2：GIF 多帧解码 + SVG 光栅化 ABI ---- */

/* 解码 GIF → 全部帧连续 RGBA8 缓冲（frames*w*h*4）+ 每帧延时（毫秒）数组。
 * out_rgba / out_delays 由 stb 分配，rt_image_free 统一释放。成功 0 / 失败非零。 */
int32_t rt_image_decode_gif(const uint8_t* data, size_t len, uint8_t** out_rgba,
                            int32_t* w, int32_t* h, int32_t* out_frame_count,
                            int32_t** out_delays) {
    if (!data || len == 0 || !out_rgba || !w || !h || !out_frame_count || !out_delays) return 1;
    *out_rgba = NULL;
    *out_frame_count = 0;
    *out_delays = NULL;
    int* delays = NULL;
    int32_t iw = 0, ih = 0, z = 0, comp = 0;
    stbi_uc* px = stbi_load_gif_from_memory(data, (int)len, &delays, &iw, &ih, &z, &comp, 4);
    if (!px || iw <= 0 || ih <= 0 || z <= 0) {
        if (delays) STBI_FREE(delays);
        return 1;
    }
    if (!delays) {
        stbi_image_free(px);
        return 1;
    }
    *out_rgba = (uint8_t*)px;
    *w = iw;
    *h = ih;
    *out_frame_count = z;
    *out_delays = (int32_t*)delays;
    return 0;
}

/* 定位 GIF 帧 i 在连续缓冲内的起始指针（帧缓冲 = 全部帧拼接，无拷贝）。
 * 越界返回 NULL。 */
uint8_t* rt_image_gif_frame(const uint8_t* rgba, int32_t w, int32_t h, int32_t frame_index) {
    if (!rgba || w <= 0 || h <= 0 || frame_index < 0) return NULL;
    uint64_t frame_bytes = (uint64_t)w * (uint64_t)h * 4;
    if ((uint64_t)frame_index * frame_bytes > (uint64_t)SIZE_MAX) return NULL;
    return (uint8_t*)rgba + (size_t)((uint64_t)frame_index * frame_bytes);
}

/* 读取 GIF 帧 i 的延时（毫秒）。delays 为 rt_image_decode_gif 输出的数组；
 * 越界返回 -1。 */
int32_t rt_image_gif_delay(const int32_t* delays, int32_t frame_index) {
    if (!delays || frame_index < 0) return -1;
    return delays[frame_index];
}

/* 解码 SVG → 直通 RGBA8（解预乘）。scale 为光栅化缩放（<=0 时按 1.0）。
 * 输出由本 TU malloc 分配，rt_image_free 释放。成功 0 / 失败非零。 */
int32_t rt_image_decode_svg(const uint8_t* data, size_t len, uint8_t** out_rgba,
                            int32_t* w, int32_t* h, float scale) {
    if (!data || len == 0 || !out_rgba || !w || !h) return 1;
    *out_rgba = NULL;
    /* nsvgParse 会改写输入串（写入 NUL 终止）——复制一份可写缓冲。 */
    char* buf = (char*)malloc(len + 1);
    if (!buf) return 1;
    memcpy(buf, data, len);
    buf[len] = 0;
    NSVGimage* image = nsvgParse(buf, "px", 96.0f);
    free(buf);
    if (!image) return 1;
    float iw = image->width;
    float ih = image->height;
    if (iw <= 0.0f) iw = 1.0f;
    if (ih <= 0.0f) ih = 1.0f;
    if (scale <= 0.0f) scale = 1.0f;
    int32_t rw = (int32_t)(iw * scale);
    int32_t rh = (int32_t)(ih * scale);
    if (rw <= 0) rw = 1;
    if (rh <= 0) rh = 1;
    if ((uint64_t)rw * (uint64_t)rh > (uint64_t)SIZE_MAX / 4) {
        nsvgDelete(image);
        return 1;
    }
    NSVGrasterizer* rst = nsvgCreateRasterizer();
    if (!rst) {
        nsvgDelete(image);
        return 1;
    }
    size_t n = (size_t)rw * (size_t)rh * 4;
    uint8_t* px = (uint8_t*)malloc(n);
    if (!px) {
        nsvgDeleteRasterizer(rst);
        nsvgDelete(image);
        return 1;
    }
    memset(px, 0, n);
    nsvgRasterize(rst, image, 0.0f, 0.0f, scale, px, rw, rh, rw * 4);
    nsvgDeleteRasterizer(rst);
    nsvgDelete(image);
    /* nanosvg 输出预乘 alpha → 解预乘为直通 RGBA8。 */
    for (uint64_t i = 0; i < (uint64_t)rw * (uint64_t)rh; i++) {
        uint8_t a = px[i * 4 + 3];
        if (a == 0) {
            px[i * 4] = 0;
            px[i * 4 + 1] = 0;
            px[i * 4 + 2] = 0;
        } else if (a != 255) {
            px[i * 4] = (uint8_t)(((uint32_t)px[i * 4] * 255u) / a);
            px[i * 4 + 1] = (uint8_t)(((uint32_t)px[i * 4 + 1] * 255u) / a);
            px[i * 4 + 2] = (uint8_t)(((uint32_t)px[i * 4 + 2] * 255u) / a);
        }
    }
    *out_rgba = px;
    *w = rw;
    *h = rh;
    return 0;
}
