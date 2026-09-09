/*
 * Win32 clipboard — RFC 037 TextBox Ctrl+C/V/X。
 * 平台只做 UTF-8 ↔ CF_UNICODETEXT；编辑语义在 TextBoxController。
 */
#include "clipboard_win32.h"

#include <windows.h>
#include <stdlib.h>
#include <string.h>

static char* rt_clip_empty(void) {
    char* empty = (char*)malloc(1);
    if (!empty) {
        return NULL;
    }
    empty[0] = '\0';
    return empty;
}

static char* rt_clip_wide_to_utf8(const wchar_t* wtext) {
    if (!wtext || !wtext[0]) {
        return rt_clip_empty();
    }
    int ulen = WideCharToMultiByte(CP_UTF8, 0, wtext, -1, NULL, 0, NULL, NULL);
    if (ulen <= 0) {
        return rt_clip_empty();
    }
    char* utf8 = (char*)malloc((size_t)ulen);
    if (!utf8) {
        return rt_clip_empty();
    }
    WideCharToMultiByte(CP_UTF8, 0, wtext, -1, utf8, ulen, NULL, NULL);
    return utf8;
}

static wchar_t* rt_clip_utf8_to_wide(const char* utf8) {
    if (!utf8) {
        utf8 = "";
    }
    int wlen = MultiByteToWideChar(CP_UTF8, 0, utf8, -1, NULL, 0);
    if (wlen <= 0) {
        wchar_t* empty = (wchar_t*)malloc(sizeof(wchar_t));
        if (empty) {
            empty[0] = L'\0';
        }
        return empty;
    }
    wchar_t* wide = (wchar_t*)malloc((size_t)wlen * sizeof(wchar_t));
    if (!wide) {
        return NULL;
    }
    MultiByteToWideChar(CP_UTF8, 0, utf8, -1, wide, wlen);
    return wide;
}

char* rt_ui_clipboard_get_text(void) {
    if (!OpenClipboard(NULL)) {
        return rt_clip_empty();
    }
    HANDLE h = GetClipboardData(CF_UNICODETEXT);
    char* out = NULL;
    if (h) {
        wchar_t* locked = (wchar_t*)GlobalLock(h);
        if (locked) {
            out = rt_clip_wide_to_utf8(locked);
            GlobalUnlock(h);
        }
    }
    CloseClipboard();
    if (!out) {
        return rt_clip_empty();
    }
    return out;
}

void rt_ui_clipboard_set_text(const char* utf8) {
    wchar_t* wide = rt_clip_utf8_to_wide(utf8);
    if (!wide) {
        return;
    }
    size_t bytes = (wcslen(wide) + 1) * sizeof(wchar_t);
    HGLOBAL mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
    if (!mem) {
        free(wide);
        return;
    }
    void* locked = GlobalLock(mem);
    if (!locked) {
        GlobalFree(mem);
        free(wide);
        return;
    }
    memcpy(locked, wide, bytes);
    GlobalUnlock(mem);
    free(wide);

    if (!OpenClipboard(NULL)) {
        GlobalFree(mem);
        return;
    }
    EmptyClipboard();
    if (!SetClipboardData(CF_UNICODETEXT, mem)) {
        GlobalFree(mem);
    }
    CloseClipboard();
}
