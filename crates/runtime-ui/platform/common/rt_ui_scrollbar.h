#ifndef RT_UI_SCROLLBAR_H
#define RT_UI_SCROLLBAR_H

/* RFC 037 D10.5 · ScrollView 竖滚动条几何/命中（wgpu 唯一后端；绘制由 Arc 侧
 * WgpuRender.DrawVScrollBar 完成，本头仅声明几何与命中测试原语）。
 * 公共声明（common/）：实现 Win32 在 windows/rt_ui_scrollbar.cpp，非 Win32 在
 * common/rt_ui_scrollbar_stub.c。 */

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RT_UI_VSCROLL_WIDTH 12
#define RT_UI_VSCROLL_MIN_THUMB 20

typedef struct RtUiVScrollInput {
    const char* visibility; /* "Auto" / "Visible" / "Hidden" / "Disabled" */
    double extent_h;
    double viewport_h;
    double offset;
    int32_t x;
    int32_t y;
    int32_t w;
    int32_t h;
} RtUiVScrollInput;

typedef struct RtUiVScrollGeom {
    int32_t show;
    int32_t track_x;
    int32_t track_y;
    int32_t track_w;
    int32_t track_h;
    int32_t thumb_x;
    int32_t thumb_y;
    int32_t thumb_w;
    int32_t thumb_h;
} RtUiVScrollGeom;

void rt_ui_vscroll_compute(const RtUiVScrollInput* in, RtUiVScrollGeom* out);

/* 0=none 1=thumb 2=track above 3=track below */
int32_t rt_ui_vscroll_hit(const RtUiVScrollGeom* geom, int32_t px, int32_t py);

double rt_ui_vscroll_offset_from_y(const RtUiVScrollInput* in,
                                   const RtUiVScrollGeom* geom,
                                   int32_t mouse_y);

double rt_ui_vscroll_page_delta(const RtUiVScrollInput* in);

#ifdef __cplusplus
}
#endif

#endif /* RT_UI_SCROLLBAR_H */
