/* RFC 037 M3.5 Image decode ABI (Draft).
 * 合 hub 时迁 crates/runtime-ui/platform/windows/（native tip 8e8fd488）。 */
#ifndef RT_UI_IMAGE_H
#define RT_UI_IMAGE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct RtUiBitmap {
    uint32_t* pixels;
    int32_t width;
    int32_t height;
    int32_t loaded;
} RtUiBitmap;
RtUiBitmap* rt_ui_image_load(const char* path);
RtUiBitmap* rt_ui_image_make_failed(const char* path);
void rt_ui_image_release(RtUiBitmap* bmp);
void rt_ui_blit_bitmap(uint32_t* dest, int32_t dest_w, int32_t dest_h,
                       const RtUiBitmap* src, int32_t dx, int32_t dy,
                       int32_t dw, int32_t dh, const char* stretch);
#ifdef __cplusplus
}
#endif
#endif
