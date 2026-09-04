/* RFC 037 M3.5 Image blit (Draft). 合 hub 迁 crates/runtime-ui/platform/windows/ (8e8fd488). */
#include "rt_ui_image.h"
#include <stdlib.h>
#include <string.h>

void rt_ui_image_release(RtUiBitmap* bmp) {
    if (!bmp) return;
    free(bmp->pixels);
    free(bmp);
}

void rt_ui_blit_bitmap(uint32_t* dest, int32_t dest_w, int32_t dest_h,
                       const RtUiBitmap* src, int32_t dx, int32_t dy,
                       int32_t dw, int32_t dh, const char* stretch) {
    if (!dest || !src || !src->pixels || dest_w <= 0 || dest_h <= 0) return;
    if (dw <= 0 || dh <= 0) return;
    int32_t sw = src->width, sh = src->height;
    if (sw <= 0 || sh <= 0) return;
    const char* mode = stretch ? stretch : "None";
    int32_t blit_w = dw, blit_h = dh, off_x = dx, off_y = dy;
    double scale = 1.0; int32_t crop_x = 0, crop_y = 0;
    if (strcmp(mode, "None") == 0) {
        blit_w = sw < dw ? sw : dw; blit_h = sh < dh ? sh : dh;
    } else if (strcmp(mode, "Uniform") == 0) {
        double sx = (double)dw / sw, sy = (double)dh / sh;
        scale = sx < sy ? sx : sy;
        blit_w = (int32_t)(sw * scale); blit_h = (int32_t)(sh * scale);
        off_x = dx + (dw - blit_w) / 2; off_y = dy + (dh - blit_h) / 2;
    } else if (strcmp(mode, "UniformToFill") == 0) {
        double sx = (double)dw / sw, sy = (double)dh / sh;
        scale = sx > sy ? sx : sy;
        crop_x = ((int32_t)(sw * scale) - dw) / 2;
        crop_y = ((int32_t)(sh * scale) - dh) / 2;
        if (crop_x < 0) crop_x = 0; if (crop_y < 0) crop_y = 0;
    }
    for (int32_t row = 0; row < blit_h; row++) {
        int32_t dy_row = off_y + row;
        if (dy_row < 0 || dy_row >= dest_h) continue;
        uint32_t* drow = dest + (size_t)dy_row * dest_w;
        for (int32_t col = 0; col < blit_w; col++) {
            int32_t dx_col = off_x + col;
            if (dx_col < 0 || dx_col >= dest_w) continue;
            int32_t sx, sy;
            if (strcmp(mode, "None") == 0) { sx = col; sy = row; }
            else if (strcmp(mode, "Fill") == 0) {
                sx = (int32_t)((double)col * sw / blit_w);
                sy = (int32_t)((double)row * sh / blit_h);
            } else if (strcmp(mode, "Uniform") == 0) {
                sx = (int32_t)(col / scale); sy = (int32_t)(row / scale);
            } else {
                sx = (int32_t)((col + crop_x) / scale);
                sy = (int32_t)((row + crop_y) / scale);
            }
            if (sx < 0) sx = 0; if (sy < 0) sy = 0;
            if (sx >= sw) sx = sw - 1; if (sy >= sh) sy = sh - 1;
            uint32_t c = src->pixels[(size_t)sy * sw + sx];
            if ((c >> 24) != 0) drow[dx_col] = c;
        }
    }
}

RtUiBitmap* rt_ui_image_make_failed(const char* path) {
    (void)path;
    RtUiBitmap* bmp = (RtUiBitmap*)calloc(1, sizeof(RtUiBitmap));
    if (!bmp) return NULL;
    bmp->width = bmp->height = 64; bmp->loaded = 0;
    bmp->pixels = (uint32_t*)malloc(64 * 64 * 4);
    if (!bmp->pixels) { free(bmp); return NULL; }
    for (int32_t y = 0; y < 64; y++) {
        for (int32_t x = 0; x < 64; x++) {
            uint32_t c = 0xFFCCCCCC;
            if (x < 8 && y < 8) c = 0xFF0000CC;
            bmp->pixels[y * 64 + x] = c;
        }
    }
    return bmp;
}
