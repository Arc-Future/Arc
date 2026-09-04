#ifndef ARC_RT_UI_KEYBOARD_WIN32_H
#define ARC_RT_UI_KEYBOARD_WIN32_H

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif

int rt_win32_keyboard_handle_char(HWND hwnd, WPARAM wch);
int rt_win32_keyboard_handle_keydown(HWND hwnd, WPARAM wParam);

#ifdef __cplusplus
}
#endif

#endif /* ARC_RT_UI_KEYBOARD_WIN32_H */
