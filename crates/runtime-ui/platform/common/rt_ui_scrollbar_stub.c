/*
 * Non-Win32 stub: vertical scrollbar geometry helpers unavailable
 * (ScrollView layout-only, no scrollbar UI).  Public declarations live in
 * common/rt_ui_scrollbar.h, real implementation in windows/rt_ui_scrollbar.cpp.
 * 仅几何/命中原语；绘制由 Arc 侧 WgpuRender 完成，本文件不含任何光栅绘制。
 */
#include "rt_ui_scrollbar.h"
#include <string.h>

void rt_ui_vscroll_compute(const RtUiVScrollInput* in, RtUiVScrollGeom* out) {
    if (out) {
        memset(out, 0, sizeof(*out));
    }
    (void)in;
}

int32_t rt_ui_vscroll_hit(const RtUiVScrollGeom* geom, int32_t px, int32_t py) {
    (void)geom;
    (void)px;
    (void)py;
    return 0;
}

double rt_ui_vscroll_offset_from_y(const RtUiVScrollInput* in,
                                   const RtUiVScrollGeom* geom,
                                   int32_t mouse_y) {
    (void)geom;
    (void)mouse_y;
    return in ? in->offset : 0.0;
}

double rt_ui_vscroll_page_delta(const RtUiVScrollInput* in) {
    (void)in;
    return 0.0;
}
