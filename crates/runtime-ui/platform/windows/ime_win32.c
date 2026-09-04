/*
 * Win32 IMM32 IME dispatch (WM_IME_* → rt_ui_ime_dispatch).
 * RFC 037 · 026-ime-east-asian-input.md §5.3
 */
#include "../common/rt_ui_platform.h"
#include "../common/rt_ui_ime_internal.h"
#include "../common/rt_ui_ime_types.h"
#include "ime_win32.h"

#include <imm.h>
#include <stdlib.h>
#include <string.h>

static char* rt_win32_wide_to_utf8(const wchar_t* wtext) {
    if (!wtext) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    if (!wtext[0]) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    int ulen = WideCharToMultiByte(CP_UTF8, 0, wtext, -1, NULL, 0, NULL, NULL);
    if (ulen <= 0) return NULL;
    char* utf8 = (char*)malloc((size_t)ulen);
    if (!utf8) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, wtext, -1, utf8, ulen, NULL, NULL);
    return utf8;
}

static int32_t rt_win32_utf8_byte_index(const char* utf8, int32_t wchar_index) {
    if (!utf8 || wchar_index <= 0) return 0;
    int32_t i = 0;
    int32_t chars = 0;
    while (utf8[i] && chars < wchar_index) {
        unsigned char c = (unsigned char)utf8[i];
        if (c < 0x80) i += 1;
        else if ((c & 0xE0) == 0xC0) i += 2;
        else if ((c & 0xF0) == 0xE0) i += 3;
        else if ((c & 0xF8) == 0xF0) i += 4;
        else i += 1;
        chars++;
    }
    return i;
}

static int rt_win32_ime_read_bytes(HIMC himc, DWORD index,
                                   void** out_buf, LONG* out_bytes) {
    *out_buf = NULL;
    *out_bytes = 0;
    if (!himc) return 0;
    LONG bytes = ImmGetCompositionStringW(himc, index, NULL, 0);
    if (bytes <= 0) return 0;
    void* buf = malloc((size_t)bytes + (index == GCS_COMPSTR ? sizeof(wchar_t) : 0));
    if (!buf) return 0;
    ImmGetCompositionStringW(himc, index, buf, bytes);
    if (index == GCS_COMPSTR) {
        ((wchar_t*)buf)[bytes / (LONG)sizeof(wchar_t)] = L'\0';
    }
    *out_buf = buf;
    *out_bytes = bytes;
    return 1;
}

static void rt_win32_ime_default_candidate_rect(HWND hwnd,
                                                int32_t* out_x, int32_t* out_y,
                                                int32_t* out_w, int32_t* out_h) {
    *out_x = 16;
    *out_y = 16;
    *out_w = 200;
    *out_h = 28;
    RECT rc;
    if (GetClientRect(hwnd, &rc)) {
        int32_t cw = (int32_t)(rc.right - rc.left);
        if (*out_x + *out_w > cw && cw > 32) *out_w = cw - 32;
    }
}

static void rt_win32_ime_apply_candidate_rect(HWND hwnd) {
    HIMC himc = ImmGetContext(hwnd);
    if (!himc) return;

    int32_t x, y, w, h;
    RtUiElement* cand_input = NULL;
    if (rt_ui_ime_query_candidate_rect(&cand_input, &x, &y, &w, &h)) {
        (void)cand_input;
    } else {
        rt_win32_ime_default_candidate_rect(hwnd, &x, &y, &w, &h);
    }
    if (w < 8) w = 8;
    if (h < 8) h = 8;

    /* Arc 传入 DIP；IMM COMPOSITIONFORM/CANDIDATEFORM 要客户区物理像素。 */
    {
        extern double rt_window_dpi_scale(void);
        double scale = rt_window_dpi_scale();
        if (scale < 1.0) {
            scale = 1.0;
        }
        x = (int32_t)((double)x * scale);
        y = (int32_t)((double)y * scale);
        w = (int32_t)((double)w * scale);
        h = (int32_t)((double)h * scale);
    }

    COMPOSITIONFORM cf = {0};
    cf.dwStyle = CFS_POINT;
    cf.ptCurrentPos.x = x;
    cf.ptCurrentPos.y = y + h;
    ImmSetCompositionWindow(himc, &cf);

    CANDIDATEFORM cand = {0};
    cand.dwIndex = 0;
    cand.dwStyle = CFS_CANDIDATEPOS;
    cand.ptCurrentPos.x = x;
    cand.ptCurrentPos.y = y + h + 4;
    ImmSetCandidateWindow(himc, &cand);

    ImmReleaseContext(hwnd, himc);
}

static void rt_win32_ime_dispatch_composition(HIMC himc) {
    wchar_t* comp_w = NULL;
    LONG comp_bytes = 0;
    if (!rt_win32_ime_read_bytes(himc, GCS_COMPSTR, (void**)&comp_w, &comp_bytes)) {
        return;
    }

    char* comp_utf8 = rt_win32_wide_to_utf8(comp_w);
    free(comp_w);
    if (!comp_utf8) return;

    int32_t cursor = 0;
    LONG cursor_pos = ImmGetCompositionStringW(himc, GCS_CURSORPOS, NULL, 0);
    if (cursor_pos >= 0) {
        cursor = rt_win32_utf8_byte_index(comp_utf8, (int32_t)cursor_pos);
    }

    uint8_t* attrs = NULL;
    int32_t attr_len = 0;
    void* attr_buf = NULL;
    LONG attr_bytes = 0;
    if (rt_win32_ime_read_bytes(himc, GCS_COMPATTR, &attr_buf, &attr_bytes)) {
        attr_len = (int32_t)attr_bytes;
        attrs = (uint8_t*)attr_buf;
    }

    RtUiImeComposition payload = {0};
    payload.text = comp_utf8;
    payload.cursor = cursor;
    payload.attr_length = attr_len;
    payload.attributes = attrs;
    rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_UPDATE, &payload);

    free(attrs);
    free(comp_utf8);
}

static void rt_win32_ime_dispatch_commit(HIMC himc, const wchar_t* fallback_w) {
    wchar_t* result_w = NULL;
    LONG result_bytes = 0;
    const wchar_t* src = fallback_w;
    if (himc && rt_win32_ime_read_bytes(himc, GCS_RESULTSTR, (void**)&result_w, &result_bytes)) {
        src = result_w;
    }
    if (!src || !src[0]) {
        free(result_w);
        return;
    }
    char* utf8 = rt_win32_wide_to_utf8(src);
    free(result_w);
    if (!utf8 || !utf8[0]) {
        free(utf8);
        return;
    }
    rt_ui_ime_dispatch(RT_UI_IME_COMMIT, utf8);
    free(utf8);
}

int rt_win32_ime_is_ime_message(UINT msg) {
    return msg >= WM_IME_SETCONTEXT && msg <= WM_IME_KEYUP;
}

LRESULT rt_win32_ime_handle_message(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    (void)wp;
    switch (msg) {
    case WM_IME_SETCONTEXT:
        return DefWindowProcW(hwnd, msg, wp, lp);

    case WM_IME_NOTIFY:
        return DefWindowProcW(hwnd, msg, wp, lp);

    case WM_IME_STARTCOMPOSITION:
        rt_win32_ime_apply_candidate_rect(hwnd);
        return 0;

    case WM_IME_COMPOSITION: {
        HIMC himc = ImmGetContext(hwnd);
        if (!himc) return 0;
        if (lp & GCS_RESULTSTR) {
            rt_win32_ime_dispatch_commit(himc, NULL);
        }
        if (lp & (GCS_COMPSTR | GCS_CURSORPOS | GCS_COMPATTR)) {
            rt_win32_ime_dispatch_composition(himc);
        }
        ImmReleaseContext(hwnd, himc);
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;
    }

    case WM_IME_ENDCOMPOSITION:
        rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_END, NULL);
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;

    case WM_IME_CHAR: {
        wchar_t wch[2] = {(wchar_t)wp, L'\0'};
        rt_win32_ime_dispatch_commit(NULL, wch);
        InvalidateRect(hwnd, NULL, FALSE);
        return 0;
    }

    default:
        return (LRESULT)-1;
    }
}

LRESULT rt_win32_ime_on_killfocus(HWND hwnd) {
    (void)hwnd;
    if (rt_ui_ime_get_focus()) {
        rt_ui_ime_dispatch(RT_UI_IME_FOCUS_LOST, NULL);
    }
    return 0;
}
