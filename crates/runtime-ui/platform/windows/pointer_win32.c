/*
 * Win32 pointer input (WM_MOUSE*) — RFC 037 Button visual states Draft.
 *
 * 坐标契约：LayoutX/Y 与命中测试均为客户区 DIP；WM_* 的 lParam 为物理像素。
 * 必须先 / dpi_scale，否则高 DPI 下命中框相对绘制面上移偏左（感知「鼠标偏移」）。
 */
#include "../common/rt_ui_platform.h"
#include <string.h>

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdio.h>

/* window.cpp 导出；与 WgpuRender._dpiScale / CreateWindow 客户区缩放同源。 */
extern double rt_window_dpi_scale(void);

static void rt_ui_win32_client_to_dip(HWND hwnd, int32_t phys_x, int32_t phys_y,
                                      int32_t* out_x, int32_t* out_y,
                                      int32_t* out_w, int32_t* out_h) {
    double scale = rt_window_dpi_scale();
    if (scale < 1.0) {
        scale = 1.0;
    }
    RECT rc;
    int32_t cw = 0;
    int32_t ch = 0;
    if (GetClientRect(hwnd, &rc)) {
        cw = (int32_t)(rc.right - rc.left);
        ch = (int32_t)(rc.bottom - rc.top);
    }
    if (out_w) {
        *out_w = (int32_t)((double)cw / scale);
        if (*out_w < 1) {
            *out_w = 1;
        }
    }
    if (out_h) {
        *out_h = (int32_t)((double)ch / scale);
        if (*out_h < 1) {
            *out_h = 1;
        }
    }
    if (out_x) {
        *out_x = (int32_t)((double)phys_x / scale);
    }
    if (out_y) {
        *out_y = (int32_t)((double)phys_y / scale);
    }
}

static int rt_ui_win32_is_button(RtUiElement* elem) {
    return elem && elem->type_name && strcmp(elem->type_name, "Button") == 0;
}

/* 指针目标泛化：Button（专用通道）或注册了泛化交互回调的控件（RFC 037 D10.6）。 */
static int rt_ui_win32_is_pointer_target(RtUiElement* elem) {
    if (!elem) return 0;
    if (rt_ui_win32_is_button(elem)) return 1;
    return rt_ui_has_control_handler(elem);
}

static void rt_ui_win32_dispatch_visual(RtUiElement* elem) {
    if (!elem) return;
    if (rt_ui_win32_is_button(elem)) {
        rt_ui_dispatch_button_visual_state(elem);
    } else {
        rt_ui_dispatch_control_visual_state(elem);
    }
}

static void rt_ui_win32_set_hover(RtUiElement** pointer_over, RtUiElement* next,
                                  int* dirty) {
    RtUiElement* prev = pointer_over ? *pointer_over : NULL;
    if (prev == next) return;
    if (prev && rt_ui_win32_is_pointer_target(prev) && prev->is_mouse_over) {
        prev->is_mouse_over = 0;
        rt_ui_win32_dispatch_visual(prev);
        *dirty = 1;
    }
    if (next && rt_ui_win32_is_pointer_target(next) && !next->is_mouse_over) {
        next->is_mouse_over = 1;
        rt_ui_win32_dispatch_visual(next);
        *dirty = 1;
    }
    if (pointer_over) {
        *pointer_over = next;
    }
}

static void rt_ui_win32_clear_pressed(RtUiElement* elem, int* dirty) {
    if (elem && rt_ui_win32_is_pointer_target(elem) && elem->is_pressed) {
        elem->is_pressed = 0;
        rt_ui_win32_dispatch_visual(elem);
        *dirty = 1;
    } else if (elem && rt_ui_win32_is_pointer_target(elem)) {
        elem->is_pressed = 0;
    }
}

static void rt_ui_win32_pointer_move(RtUiElement** root, RtUiElement** pointer_down,
                                     RtUiElement** pointer_over,
                                     HWND hwnd, int32_t px, int32_t py) {
    if (!root || !*root) return;
    int32_t cw = 0;
    int32_t ch = 0;
    int32_t dip_x = 0;
    int32_t dip_y = 0;
    rt_ui_win32_client_to_dip(hwnd, px, py, &dip_x, &dip_y, &cw, &ch);
    RtUiElement* hit = rt_ui_hit_test(*root, cw, ch, dip_x, dip_y);
    RtUiElement* hover = rt_ui_win32_is_pointer_target(hit) ? hit : NULL;
    int dirty = 0;
    rt_ui_win32_set_hover(pointer_over, hover, &dirty);
    /* RFC 037 D10.6：按下期间连续拖拽（Slider）——pointer_down 目标驱动，与
     * window.cpp vscroll 拖拽分流次序互斥（vscroll 先消费，未命中才落本层）。 */
    if (pointer_down && *pointer_down && rt_ui_win32_is_pointer_target(*pointer_down)) {
        if (rt_ui_dispatch_control_drag(*pointer_down, dip_x, dip_y)) {
            dirty = 1;
        }
    }
    if (dirty) {
        InvalidateRect(hwnd, NULL, FALSE);
    }
    TRACKMOUSEEVENT tme;
    memset(&tme, 0, sizeof(tme));
    tme.cbSize = sizeof(tme);
    tme.dwFlags = TME_LEAVE;
    tme.hwndTrack = hwnd;
    TrackMouseEvent(&tme);
}

static void rt_ui_win32_pointer_leave(RtUiElement** pointer_over, HWND hwnd) {
    int dirty = 0;
    rt_ui_win32_set_hover(pointer_over, NULL, &dirty);
    if (dirty) {
        InvalidateRect(hwnd, NULL, FALSE);
    }
}

static void rt_ui_win32_pointer_down(RtUiElement** root, RtUiElement** pointer_down,
                                     RtUiElement** pointer_over,
                                     HWND hwnd, int32_t px, int32_t py) {
    if (!root || !*root || !pointer_down) return;
    int32_t cw = 0;
    int32_t ch = 0;
    int32_t dip_x = 0;
    int32_t dip_y = 0;
    rt_ui_win32_client_to_dip(hwnd, px, py, &dip_x, &dip_y, &cw, &ch);
    RtUiElement* hit = rt_ui_hit_test(*root, cw, ch, dip_x, dip_y);
    int dirty = 0;
    if (*pointer_down && *pointer_down != hit) {
        rt_ui_win32_clear_pressed(*pointer_down, &dirty);
    }
    *pointer_down = hit;
    if (hit && rt_ui_win32_is_pointer_target(hit)) {
        if (!hit->is_pressed) {
            hit->is_pressed = 1;
            rt_ui_win32_dispatch_visual(hit);
            dirty = 1;
        }
        rt_ui_win32_set_hover(pointer_over, hit, &dirty);
        /* track 点击跳转（Slider）：按下即按像素位置设置值。 */
        if (rt_ui_dispatch_control_drag(hit, dip_x, dip_y)) {
            dirty = 1;
        }
    }
    if (hit && hit->type_name && strcmp(hit->type_name, "TextBox") == 0) {
            fprintf(stderr, "[DBG] ptr hit TextBox elem=%p\n", (void*)hit);
            rt_ui_dispatch_input_focus(hit);
        /* M-caret2：点击定位 caret——局部 DIP 坐标 = 命中坐标 - 元素左缘。 */
        int32_t local_x = (int32_t)((double)dip_x - hit->layout_x);
        rt_ui_dispatch_input_click_at(hit, local_x);
        dirty = 1;
    }
    if (dirty) {
        InvalidateRect(hwnd, NULL, FALSE);
    }
}

static void rt_ui_win32_pointer_up(RtUiElement** root, RtUiElement** pointer_down,
                                   RtUiElement** pointer_over,
                                   HWND hwnd, int32_t px, int32_t py) {
    if (!root || !*root || !pointer_down) return;
    int32_t cw = 0;
    int32_t ch = 0;
    int32_t dip_x = 0;
    int32_t dip_y = 0;
    rt_ui_win32_client_to_dip(hwnd, px, py, &dip_x, &dip_y, &cw, &ch);
    RtUiElement* hit = rt_ui_hit_test(*root, cw, ch, dip_x, dip_y);
    RtUiElement* down = *pointer_down;
    int dirty = 0;
    if (down) {
        rt_ui_win32_clear_pressed(down, &dirty);
    }
    *pointer_down = NULL;
    if (hit && hit == down && rt_ui_win32_is_button(hit)) {
        rt_ui_dispatch_button_click(hit);
        dirty = 1;
    } else if (hit && hit == down) {
        /* 泛化控件（ToggleButton/CheckBox/Slider/ListView…）：释放命中原按下元素即触发
         * click；ListView 由 C 侧按像素计算命中行 index（写入镜像 "HitItemIndex"）。 */
        if (rt_ui_dispatch_control_click_at(hit, dip_x, dip_y)) {
            dirty = 1;
        }
    }
    rt_ui_win32_set_hover(pointer_over, rt_ui_win32_is_pointer_target(hit) ? hit : NULL, &dirty);
    if (dirty) {
        InvalidateRect(hwnd, NULL, FALSE);
    }
}

void rt_ui_win32_handle_pointer_message(HWND hwnd, RtUiElement** root,
                                        RtUiElement** pointer_down,
                                        RtUiElement** pointer_over,
                                        UINT msg, LPARAM lp) {
    if (!root || !*root) return;
    int32_t px = (int32_t)(int16_t)LOWORD(lp);
    int32_t py = (int32_t)(int16_t)HIWORD(lp);
    if (msg == WM_LBUTTONDOWN) {
        rt_ui_win32_pointer_down(root, pointer_down, pointer_over, hwnd, px, py);
    } else if (msg == WM_LBUTTONUP) {
        rt_ui_win32_pointer_up(root, pointer_down, pointer_over, hwnd, px, py);
    } else if (msg == WM_MOUSEMOVE) {
        rt_ui_win32_pointer_move(root, pointer_down, pointer_over, hwnd, px, py);
    } else if (msg == WM_MOUSELEAVE) {
        rt_ui_win32_pointer_leave(pointer_over, hwnd);
    }
}
