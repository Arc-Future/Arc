#ifndef ARC_RT_UI_IME_WIN32_H
#define ARC_RT_UI_IME_WIN32_H

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif

/** WM_IME_SETCONTEXT … WM_IME_KEYUP range. */
int rt_win32_ime_is_ime_message(UINT msg);

/** Returns 0 when handled; -1 to fall through to DefWindowProc. */
LRESULT rt_win32_ime_handle_message(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp);

LRESULT rt_win32_ime_on_killfocus(HWND hwnd);

#ifdef __cplusplus
}
#endif

#endif /* ARC_RT_UI_IME_WIN32_H */
