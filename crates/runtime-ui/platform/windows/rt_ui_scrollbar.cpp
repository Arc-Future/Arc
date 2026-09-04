/* RFC 026 D10.5 · ScrollView 竖滚动条几何/命中（wgpu 唯一后端；绘制由 Arc 侧
 * WgpuRender.DrawVScrollBar 完成，本文件仅提供几何与命中测试原语）。 */

#include "rt_ui_scrollbar.h"

#include <string.h>

void rt_ui_vscroll_compute(const RtUiVScrollInput* in, RtUiVScrollGeom* out) {
    if (out) {
        memset(out, 0, sizeof(*out));
    }
    if (!in || !out || in->w <= 0 || in->h <= 0) {
        return;
    }

    const char* vis = in->visibility ? in->visibility : "Auto";
    double viewport = in->viewport_h > 0.0 ? in->viewport_h : (double)in->h;
    double extent = in->extent_h > 0.0 ? in->extent_h : viewport;
    double scrollable = extent - viewport;
    if (scrollable < 0.0) {
        scrollable = 0.0;
    }

    /* production-surface §4：Disabled/Hidden 不绘制；Visible 总是；Auto 仅溢出。 */
    int32_t show = 0;
    if (strcmp(vis, "Disabled") == 0 || strcmp(vis, "Hidden") == 0) {
        show = 0;
    } else if (strcmp(vis, "Visible") == 0) {
        show = 1;
    } else {
        /* Auto（及未知值按 Auto） */
        show = scrollable > 0.5 ? 1 : 0;
    }
    if (!show) {
        return;
    }

    out->show = 1;
    out->track_x = in->x + in->w - RT_UI_VSCROLL_WIDTH;
    out->track_y = in->y;
    out->track_w = RT_UI_VSCROLL_WIDTH;
    out->track_h = in->h;

    double ratio = viewport / extent;
    if (ratio > 1.0) {
        ratio = 1.0;
    }
    int32_t thumb_h = (int32_t)(ratio * (double)out->track_h);
    if (thumb_h < RT_UI_VSCROLL_MIN_THUMB) {
        thumb_h = RT_UI_VSCROLL_MIN_THUMB;
    }
    if (thumb_h > out->track_h) {
        thumb_h = out->track_h;
    }

    int32_t travel = out->track_h - thumb_h;
    double frac = 0.0;
    if (scrollable > 0.0 && travel > 0) {
        frac = in->offset / scrollable;
        if (frac < 0.0) {
            frac = 0.0;
        }
        if (frac > 1.0) {
            frac = 1.0;
        }
    }

    out->thumb_x = out->track_x + 1;
    out->thumb_y = out->track_y + (int32_t)(frac * (double)travel);
    out->thumb_w = out->track_w - 2;
    out->thumb_h = thumb_h;
}

int32_t rt_ui_vscroll_hit(const RtUiVScrollGeom* geom, int32_t px, int32_t py) {
    if (!geom || !geom->show) {
        return 0;
    }
    if ((double)px < (double)geom->track_x ||
        (double)px >= (double)geom->track_x + (double)geom->track_w ||
        (double)py < (double)geom->track_y ||
        (double)py >= (double)geom->track_y + (double)geom->track_h) {
        return 0;
    }
    if ((double)px >= (double)geom->thumb_x &&
        (double)px < (double)geom->thumb_x + (double)geom->thumb_w &&
        (double)py >= (double)geom->thumb_y &&
        (double)py < (double)geom->thumb_y + (double)geom->thumb_h) {
        return 1;
    }
    if (py < geom->thumb_y) {
        return 2;
    }
    return 3;
}

double rt_ui_vscroll_offset_from_y(const RtUiVScrollInput* in,
                                   const RtUiVScrollGeom* geom,
                                   int32_t mouse_y) {
    if (!in || !geom || !geom->show) {
        return in ? in->offset : 0.0;
    }
    double viewport = in->viewport_h > 0.0 ? in->viewport_h : (double)in->h;
    double extent = in->extent_h > 0.0 ? in->extent_h : viewport;
    double scrollable = extent - viewport;
    if (scrollable <= 0.0) {
        return 0.0;
    }
    int32_t travel = geom->track_h - geom->thumb_h;
    if (travel <= 0) {
        return 0.0;
    }
    int32_t rel = mouse_y - geom->track_y - geom->thumb_h / 2;
    if (rel < 0) {
        rel = 0;
    }
    if (rel > travel) {
        rel = travel;
    }
    return (double)rel / (double)travel * scrollable;
}

double rt_ui_vscroll_page_delta(const RtUiVScrollInput* in) {
    if (!in) {
        return 0.0;
    }
    double viewport = in->viewport_h > 0.0 ? in->viewport_h : (double)in->h;
    return viewport * 0.9;
}
