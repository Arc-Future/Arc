/*
 * Win32 keyboard — RFC 037 §8 IN-R2：平台只做机械转换。
 * WM_KEYDOWN → rt_ui_dispatch_key(vk, mods)
 * WM_CHAR    → rt_ui_dispatch_text(utf8)
 * 编辑语义 / IsReadOnly / Shift 扩选分支均在 Arc KeyboardRouter → TextBoxModel。
 * WM_IME_* 组字是唯一必须留在平台层的逻辑（组字中吞编辑键交给 IME）。
 *
 * 事件泵契约：keydown 返回 1 会跳过 TranslateMessage（无 WM_CHAR）。
 * 因此仅对导航/编辑/快捷键 return 1；可打印字符 return 0，由 CHAR 通道插入。
 */
#include "../common/rt_ui_ime_internal.h"
#include "../common/rt_ui_ime_types.h"
#include "../common/rt_ui_platform.h"
#include "keyboard_win32.h"

#include <imm.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* mods：bit0 = Shift，bit1 = Ctrl（与 KeyboardRouter / RFC 037 §8 对齐）。 */
#define RT_UI_KEY_MOD_SHIFT 1
#define RT_UI_KEY_MOD_CTRL  2

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

static int32_t rt_win32_key_mods(void) {
    int32_t mods = 0;
    if (GetKeyState(VK_SHIFT) & 0x8000) mods |= RT_UI_KEY_MOD_SHIFT;
    if (GetKeyState(VK_CONTROL) & 0x8000) mods |= RT_UI_KEY_MOD_CTRL;
    return mods;
}

int rt_win32_keyboard_handle_char(HWND hwnd, WPARAM wch) {
    /* 组字期可打印字符归 IME；控制字符（含旧 Ctrl+A=0x01）不进 text 通道。 */
    if (rt_win32_ime_is_composing(hwnd)) return 0;
    wchar_t wc = (wchar_t)wch;
    if (wc < 32u) return 0;

    char* utf8 = rt_win32_wide_to_utf8(wc);
    if (!utf8 || !utf8[0]) {
        free(utf8);
        return 0;
    }
    rt_ui_dispatch_text(utf8);
    free(utf8);
    InvalidateRect(hwnd, NULL, FALSE);
    return 1;
}

int rt_win32_keyboard_handle_keydown(HWND hwnd, WPARAM wParam) {
    /* 组字中：编辑/导航键交 IME，不进 KeyboardRouter。 */
    if (rt_win32_ime_is_composing(hwnd)) {
        if (wParam == VK_LEFT || wParam == VK_RIGHT ||
            wParam == VK_UP || wParam == VK_DOWN ||
            wParam == VK_HOME || wParam == VK_END ||
            wParam == VK_BACK || wParam == VK_DELETE ||
            wParam == VK_TAB || wParam == VK_RETURN || wParam == VK_SPACE) {
            return 0;
        }
    }

    int32_t mods = rt_win32_key_mods();
    int ctrl = (mods & RT_UI_KEY_MOD_CTRL) != 0;
    int is_nav_edit =
        wParam == VK_TAB || wParam == VK_RETURN || wParam == VK_SPACE ||
        wParam == VK_ESCAPE ||
        wParam == VK_LEFT || wParam == VK_RIGHT ||
        wParam == VK_UP || wParam == VK_DOWN ||
        wParam == VK_HOME || wParam == VK_END ||
        wParam == VK_BACK || wParam == VK_DELETE ||
        (ctrl && wParam >= 'A' && wParam <= 'Z');

    if (!is_nav_edit) {
        /* 可打印字符：放行 TranslateMessage → WM_CHAR → dispatch_text。 */
        return 0;
    }

    rt_ui_dispatch_key((int32_t)wParam, mods);
    InvalidateRect(hwnd, NULL, FALSE);
    return 1;
}
