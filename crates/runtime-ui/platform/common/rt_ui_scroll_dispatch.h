#ifndef ARC_RT_UI_SCROLL_DISPATCH_H
#define ARC_RT_UI_SCROLL_DISPATCH_H

#include "rt_ui_element_internal.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RT_UI_SCROLLBAR_SET_OFFSET 1
#define RT_UI_SCROLLBAR_PAGE_UP 2
#define RT_UI_SCROLLBAR_PAGE_DOWN 3
#define RT_UI_SCROLLBAR_DRAG_START 4
#define RT_UI_SCROLLBAR_DRAG_END 5

void rt_ui_set_scroll_wheel_handler(void* fn, void* env);
void rt_ui_set_scroll_bar_handler(void* fn, void* env);
void rt_ui_dispatch_scroll_wheel(RtUiElement* elem, int32_t delta_x, int32_t delta_y);
void rt_ui_dispatch_scroll_bar(RtUiElement* elem, int32_t action, double value);

#ifdef __cplusplus
}
#endif

#endif
