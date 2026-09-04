#ifndef ARC_RT_UI_SCROLL_WIN32_H
#define ARC_RT_UI_SCROLL_WIN32_H

#include "../common/rt_ui_element_internal.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif

LRESULT rt_ui_win32_handle_scroll_wheel(HWND hwnd, RtUiElement* root, WPARAM wp, LPARAM lp);
LRESULT rt_ui_win32_handle_vscroll_message(HWND hwnd, RtUiElement* root,
                                           UINT msg, WPARAM wp, LPARAM lp);

#ifdef __cplusplus
}
#endif

#endif
