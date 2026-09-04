#ifndef ARC_RT_UI_POINTER_WIN32_H
#define ARC_RT_UI_POINTER_WIN32_H

#include "../common/rt_ui_element_internal.h"

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#ifdef __cplusplus
extern "C" {
#endif

void rt_ui_win32_handle_pointer_message(HWND hwnd, RtUiElement** root,
                                        RtUiElement** pointer_down,
                                        RtUiElement** pointer_over,
                                        UINT msg, LPARAM lp);

#ifdef __cplusplus
}
#endif

#endif /* ARC_RT_UI_POINTER_WIN32_H */
