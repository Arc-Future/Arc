#include "scroll_win32.h"
#include "../common/rt_ui_props.h"
#include "../common/rt_ui_scroll_dispatch.h"
#include "rt_ui_scrollbar.h"
#include <string.h>
#include <stdio.h>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

/* 与 pointer_win32：WM 物理像素 → 布局 DIP（LayoutX/Y / 滚动条几何同源）。 */
extern double rt_window_dpi_scale(void);

static void rt_ui_win32_phys_to_dip_xy(int32_t phys_x, int32_t phys_y,
                                       int32_t* out_x, int32_t* out_y) {
    double scale = rt_window_dpi_scale();
    if (scale < 1.0) {
        scale = 1.0;
    }
    if (out_x) {
        *out_x = (int32_t)((double)phys_x / scale);
    }
    if (out_y) {
        *out_y = (int32_t)((double)phys_y / scale);
    }
}

static RtUiElement* rt_ui_find_scrollview_at_impl(RtUiElement* elem, int32_t px, int32_t py, int depth) {
    if (!elem || !elem->layout_valid || depth <= 0) return NULL;
    for (size_t i = elem->child_count; i > 0; i--) {
        RtUiElement* hit = rt_ui_find_scrollview_at_impl(elem->children[i - 1], px, py, depth - 1);
        if (hit) return hit;
    }
    const char* type = elem->type_name ? elem->type_name : "Element";
    if (strcmp(type, "ScrollView") != 0) return NULL;
    if ((double)px >= elem->layout_x && (double)px < elem->layout_x + elem->layout_w &&
        (double)py >= elem->layout_y && (double)py < elem->layout_y + elem->layout_h) {
        return elem;
    }
    return NULL;
}

static RtUiElement* rt_ui_find_scrollview_at(RtUiElement* elem, int32_t px, int32_t py) {
    /* 深度上限 64：防御循环引用/异常深树导致的栈溢出（滚轮崩溃根因之一）。 */
    return rt_ui_find_scrollview_at_impl(elem, px, py, 64);
}

static void rt_ui_vscroll_input_from_elem(RtUiElement* elem, RtUiVScrollInput* out) {
    if (!out) return;
    memset(out, 0, sizeof(*out));
    if (!elem) return;
    out->visibility = rt_ui_get_string(elem, "VerticalScrollBarVisibility", "Auto");
    out->extent_h = rt_ui_get_number(elem, "ExtentHeight", 0.0);
    out->viewport_h = rt_ui_get_number(elem, "ViewportHeight", 0.0);
    if (out->viewport_h <= 0.0) out->viewport_h = elem->layout_h;
    if (out->extent_h <= 0.0) out->extent_h = out->viewport_h;
    out->offset = rt_ui_get_number(elem, "VerticalOffset", 0.0);
    out->x = (int32_t)elem->layout_x;
    out->y = (int32_t)elem->layout_y;
    out->w = (int32_t)elem->layout_w;
    out->h = (int32_t)elem->layout_h;
}

static struct {
    int32_t dragging;
    RtUiElement* scroll_elem;
    int32_t drag_offset_y;
} g_rt_ui_vscroll_drag = {0, NULL, 0};

LRESULT rt_ui_win32_handle_scroll_wheel(HWND hwnd, RtUiElement* root, WPARAM wp, LPARAM lp) {
    if (!root) return 0;
    /* [SCROLL-DIAG] 临时诊断（幽灵滚轮排查）。注意：注入标记（LLMHF_INJECTED）
     * 仅在低级鼠标钩子（WH_MOUSE_LL）的 MSLLHOOKSTRUCT 中可见；WM_MOUSEWHEEL
     * 的 lParam 是屏幕坐标打包值，不可解引用为结构指针。 */
    fprintf(stderr, "[SCROLL-DIAG] WM_MOUSEWHEEL raw delta=%d screen=(%d,%d)\n",
            (int)(int16_t)HIWORD(wp), (int)(int16_t)LOWORD(lp), (int)(int16_t)HIWORD(lp));
    POINT pt;
    pt.x = (LONG)(int16_t)LOWORD(lp);
    pt.y = (LONG)(int16_t)HIWORD(lp);
    ScreenToClient(hwnd, &pt);
    RECT rc;
    if (!GetClientRect(hwnd, &rc)) return 0;
    int32_t dip_x = 0;
    int32_t dip_y = 0;
    rt_ui_win32_phys_to_dip_xy((int32_t)pt.x, (int32_t)pt.y, &dip_x, &dip_y);
    RtUiElement* hit = rt_ui_find_scrollview_at(root, dip_x, dip_y);
    if (hit) {
        int16_t wheel_delta = (int16_t)HIWORD(wp);
        rt_ui_dispatch_scroll_wheel(hit, 0, (int32_t)wheel_delta);
        InvalidateRect(hwnd, NULL, FALSE);
    }
    return 0;
}

LRESULT rt_ui_win32_handle_vscroll_message(HWND hwnd, RtUiElement* root,
                                           UINT msg, WPARAM wp, LPARAM lp) {
    (void)wp;
    if (!root) return (LRESULT)-1;
    RECT rc;
    if (!GetClientRect(hwnd, &rc)) return (LRESULT)-1;
    int32_t mx_phys = (int32_t)(int16_t)LOWORD(lp);
    int32_t my_phys = (int32_t)(int16_t)HIWORD(lp);
    int32_t mx = 0;
    int32_t my = 0;
    rt_ui_win32_phys_to_dip_xy(mx_phys, my_phys, &mx, &my);

    if (msg == WM_LBUTTONDOWN) {
        RtUiElement* scroll = rt_ui_find_scrollview_at(root, mx, my);
        if (!scroll) return (LRESULT)-1;
        RtUiVScrollInput vin;
        RtUiVScrollGeom geom;
        rt_ui_vscroll_input_from_elem(scroll, &vin);
        /* Disabled：禁止拖/点条（滚轮在 Arc 层亦拒绝）。 */
        if (vin.visibility && strcmp(vin.visibility, "Disabled") == 0) {
            return (LRESULT)-1;
        }
        rt_ui_vscroll_compute(&vin, &geom);
        int32_t hit = rt_ui_vscroll_hit(&geom, mx, my);
        if (hit == 0) return (LRESULT)-1;
        if (hit == 1) {
            g_rt_ui_vscroll_drag.dragging = 1;
            g_rt_ui_vscroll_drag.scroll_elem = scroll;
            g_rt_ui_vscroll_drag.drag_offset_y = my - geom.thumb_y;
            SetCapture(hwnd);
            rt_ui_dispatch_scroll_bar(scroll, RT_UI_SCROLLBAR_DRAG_START, 0.0);
            return 0;
        }
        /* 轨道空白：按页滚动（production-surface §4；禁跳转到点击比例双轨）。 */
        if (hit == 2) {
            rt_ui_dispatch_scroll_bar(scroll, RT_UI_SCROLLBAR_PAGE_UP, 0.0);
        } else {
            rt_ui_dispatch_scroll_bar(scroll, RT_UI_SCROLLBAR_PAGE_DOWN, 0.0);
        }
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;
    }

    if (msg == WM_MOUSEMOVE && g_rt_ui_vscroll_drag.dragging && g_rt_ui_vscroll_drag.scroll_elem) {
        RtUiElement* scroll = g_rt_ui_vscroll_drag.scroll_elem;
        RtUiVScrollInput vin;
        RtUiVScrollGeom geom;
        rt_ui_vscroll_input_from_elem(scroll, &vin);
        rt_ui_vscroll_compute(&vin, &geom);
        int32_t thumb_y = my - g_rt_ui_vscroll_drag.drag_offset_y;
        double off = rt_ui_vscroll_offset_from_y(&vin, &geom, thumb_y + geom.thumb_h / 2);
        rt_ui_dispatch_scroll_bar(scroll, RT_UI_SCROLLBAR_SET_OFFSET, off);
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;
    }

    if (msg == WM_LBUTTONUP && g_rt_ui_vscroll_drag.dragging) {
        RtUiElement* scroll = g_rt_ui_vscroll_drag.scroll_elem;
        g_rt_ui_vscroll_drag.dragging = 0;
        g_rt_ui_vscroll_drag.scroll_elem = NULL;
        ReleaseCapture();
        if (scroll) {
            rt_ui_dispatch_scroll_bar(scroll, RT_UI_SCROLLBAR_DRAG_END, 0.0);
        }
        return 0;
    }

    return (LRESULT)-1;
}
