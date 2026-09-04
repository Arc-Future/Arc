#ifndef ARC_RT_UI_PLATFORM_H
#define ARC_RT_UI_PLATFORM_H

#include "../../../runtime/rt_abi.h"
#include "../../rt_ui_abi.h"
#include "rt_ui_element_internal.h"

#ifdef __cplusplus
extern "C" {
#endif

void rt_window_set_text(void* window, const char* text);
void rt_window_set_root_element(void* window, RtUiElement* root);

void rt_ui_element_set_arc_ptr(RtUiElement* elem, int64_t arc_ptr);
RtUiElement* rt_ui_hit_test(RtUiElement* root, int32_t width, int32_t height,
                            int32_t x, int32_t y);
void rt_ui_dispatch_button_click(RtUiElement* elem);
void rt_ui_set_scroll_wheel_handler(void* fn, void* env);
void rt_ui_set_scroll_bar_handler(void* fn, void* env);
void rt_ui_invalidate_active_window(void);
void rt_ui_dispatch_button_visual_state(RtUiElement* elem);
void rt_ui_set_button_visual_state_handler(void* fn, void* env);

void rt_ui_set_input_focus_handler(void* fn, void* env);
void rt_ui_dispatch_input_focus(RtUiElement* elem);
/* M-caret2：Input 点击定位 caret（local_dip_x 为命中元素局部 DIP 坐标）。 */
void rt_ui_set_input_click_handler(void* fn, void* env);
void rt_ui_dispatch_input_click_at(RtUiElement* elem, int32_t local_dip_x);
void rt_ui_set_keyboard_handler(void* fn, void* env);
void rt_ui_dispatch_keyboard(int32_t virtual_key, int32_t shift_down);
void rt_window_invalidate(void* window);

/* RFC 037 D10.6 — 泛化指针路由（按控件类型注册，additive ABI）。
 *
 * Button 保留专用 rt_ui_set_button_*_handler 通道（基础面冻结，语义不变）；
 * ToggleButton/CheckBox/Slider 等非 Button 交互控件经本通道注册：
 *   - click：   (env, handle)             —— 指针在控件内按下+释放
 *   - visual：  (env, handle, over, pressed) —— Hover/Pressed 视觉态同步
 *   - drag：    (env, handle, value)      —— 值型拖拽（Slider），C 层按渲染几何
 *                                            换算像素→值（Step 取整 + clamp）
 * 类型以元素 type_name 精确匹配；注册表每次 Show 前须 rt_ui_clear_control_handlers。
 * 回调 fn 支持 (env, ...) 与裸 (...) 两种形态（env 为 NULL 时用裸形态）。 */
void rt_ui_set_control_click_handler(const char* type_name, void* fn, void* env);
void rt_ui_set_control_visual_state_handler(const char* type_name, void* fn, void* env);
void rt_ui_set_control_drag_handler(const char* type_name, void* fn, void* env);
void rt_ui_clear_control_handlers(void);
int rt_ui_has_control_handler(RtUiElement* elem);
int rt_ui_dispatch_control_click(RtUiElement* elem);
int rt_ui_dispatch_control_click_at(RtUiElement* elem, int32_t px, int32_t py);
int rt_ui_dispatch_control_visual_state(RtUiElement* elem);
int rt_ui_dispatch_control_drag(RtUiElement* elem, int32_t px, int32_t py);

#ifdef __cplusplus
}
#endif

#endif /* ARC_RT_UI_PLATFORM_H */
