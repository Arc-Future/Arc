/*
 * Win32 keyboard: caret/ASCII (M-caret1) + focus navigation (M-focus Draft).
 */
#include "../common/rt_ui_ime_internal.h"
#include "../common/rt_ui_ime_types.h"
#include "../common/rt_ui_platform.h"
#include "keyboard_win32.h"

#include <imm.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

static char* rt_win32_wide_to_utf8(wchar_t wch) {
    wchar_t buf[2] = {wch, L'\0'};
    int ulen = WideCharToMultiByte(CP_UTF8, 0, buf, -1, NULL, 0, NULL, NULL);
    if (ulen <= 0) return NULL;
    char* utf8 = (char*)malloc((size_t)ulen);
    if (!utf8) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, buf, -1, utf8, ulen, NULL, NULL);
    return utf8;
}

static int rt_win32_ime_is_composing(HWND hwnd) {
    HIMC himc = ImmGetContext(hwnd);
    if (!himc) return 0;
    LONG comp_bytes = ImmGetCompositionStringW(himc, GCS_COMPSTR, NULL, 0);
    ImmReleaseContext(hwnd, himc);
    return comp_bytes > 0;
}

static int rt_win32_input_is_read_only(RtUiElement* focus) {
    if (!focus) return 1;
    for (size_t i = 0; i < focus->bool_count; i++) {
        if (strcmp(focus->bool_names[i], "IsReadOnly") == 0) {
            return focus->bool_values[i] ? 1 : 0;
        }
    }
    return 0;
}

int rt_win32_keyboard_handle_char(HWND hwnd, WPARAM wch) {
    if (!rt_ui_ime_get_focus()) return 0;
    if (rt_win32_input_is_read_only(rt_ui_ime_get_focus())) return 0;
    if (rt_win32_ime_is_composing(hwnd)) return 0;

    wchar_t wc = (wchar_t)wch;
    /* Ctrl+A 产生 WM_CHAR(0x01)：全选而非控制字符（须先于 wc<32 过滤拦截）。 */
    if (wc == 0x0001) {
        rt_ui_ime_dispatch(RT_UI_IME_SELECT_ALL, NULL);
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    if (wc < 32u && wc != L'\t') return 0;

    char* utf8 = rt_win32_wide_to_utf8(wc);
    if (!utf8 || !utf8[0]) {
        free(utf8);
        return 0;
    }
    rt_ui_ime_dispatch(RT_UI_IME_ASCII_CHAR, utf8);
    free(utf8);
    InvalidateRect(hwnd, NULL, FALSE);
    return 1;
}

int rt_win32_keyboard_handle_keydown(HWND hwnd, WPARAM wParam) {
    if (wParam == VK_TAB || wParam == VK_RETURN || wParam == VK_SPACE) {
        int shift = (GetKeyState(VK_SHIFT) & 0x8000) ? 1 : 0;
        rt_ui_dispatch_keyboard((int32_t)wParam, shift);
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    /* M5 方向导航（RFC 006 M5）：方向键在 Input 焦点时走 caret 编辑，
     * 无 Input 焦点时走焦点导航（rt_ui_dispatch_keyboard → FocusManager.RouteKey）。
     * 此前 UP/DOWN 完全未处理、LEFT/RIGHT 无条件走 caret——现补齐方向导航。 */
    if (wParam == VK_LEFT || wParam == VK_RIGHT ||
        wParam == VK_UP || wParam == VK_DOWN) {
        if (rt_win32_ime_is_composing(hwnd)) return 0;
        if (rt_ui_ime_get_focus()) {
            int sel_shift = (GetKeyState(VK_SHIFT) & 0x8000) ? 1 : 0;
            if (wParam == VK_LEFT) {
                rt_ui_ime_dispatch(
                    sel_shift ? RT_UI_IME_CARET_LEFT_EXT : RT_UI_IME_CARET_LEFT, NULL);
            } else if (wParam == VK_RIGHT) {
                rt_ui_ime_dispatch(
                    sel_shift ? RT_UI_IME_CARET_RIGHT_EXT : RT_UI_IME_CARET_RIGHT, NULL);
            }
            /* UP/DOWN 在 Input 内无 caret 语义，忽略（不导航） */
            InvalidateRect(hwnd, NULL, FALSE);
            return 1;
        }
        int shift = (GetKeyState(VK_SHIFT) & 0x8000) ? 1 : 0;
        rt_ui_dispatch_keyboard((int32_t)wParam, shift);
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    /* M-caret3 桌面编辑键：Home/End 行首行尾（Shift 扩选）、Delete 前删。
     * 仅 Input 焦点时消费；Home/End 无 IME 焦点时保留默认（不导航）。 */
    if (wParam == VK_HOME || wParam == VK_END) {
        if (!rt_ui_ime_get_focus()) return 0;
        if (rt_win32_ime_is_composing(hwnd)) return 0;
        int sel_shift = (GetKeyState(VK_SHIFT) & 0x8000) ? 1 : 0;
        if (wParam == VK_HOME) {
            rt_ui_ime_dispatch(
                sel_shift ? RT_UI_IME_CARET_HOME_EXT : RT_UI_IME_CARET_HOME, NULL);
        } else {
            rt_ui_ime_dispatch(
                sel_shift ? RT_UI_IME_CARET_END_EXT : RT_UI_IME_CARET_END, NULL);
        }
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    if (!rt_ui_ime_get_focus()) return 0;

    if (wParam == VK_BACK) {
        if (rt_win32_ime_is_composing(hwnd)) return 0;
        rt_ui_ime_dispatch(RT_UI_IME_BACKSPACE, NULL);
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    if (wParam == VK_DELETE) {
        if (rt_win32_ime_is_composing(hwnd)) return 0;
        rt_ui_ime_dispatch(RT_UI_IME_DELETE_FORWARD, NULL);
        InvalidateRect(hwnd, NULL, FALSE);
        return 1;
    }
    return 0;
}
