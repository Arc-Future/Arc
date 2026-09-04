#ifndef DLANG_RT_UI_ABI_H
#define DLANG_RT_UI_ABI_H

/*
 * RFC 037 �7.6 + �D7 + 026-ime-east-asian-input.md �5
 * RFC 037 ?7.6 + ?D7 + 026-ime-east-asian-input.md ?5
 */

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct RtUiElement RtUiElement;

RtUiElement* rt_ui_element_create(const char* tag);
void rt_ui_element_destroy(RtUiElement* e);
void rt_ui_element_add_child(RtUiElement* parent, RtUiElement* child);
void rt_ui_element_set_rect(RtUiElement* e, double x, double y, double w, double h);
void rt_ui_element_set_color(RtUiElement* e, const char* fill);
void rt_ui_element_set_text(RtUiElement* e, const char* text);
void rt_ui_element_set_font_size(RtUiElement* e, double size);

/* IME?026-ime-east-asian-input.md 5.2? */
/* IME?026-ime-east-asian-input.md ?5.2? */

#define RT_UI_IME_COMPOSITION_UPDATE  1
#define RT_UI_IME_COMMIT                2
#define RT_UI_IME_COMPOSITION_END       3
#define RT_UI_IME_FOCUS_LOST            4
#define RT_UI_IME_BACKSPACE             5
/* M-caret1 ASCII / caret?????IME ??/commit ?? 1?5 */
#define RT_UI_IME_ASCII_CHAR            6  /* payload: const char* utf8????? */
#define RT_UI_IME_CARET_LEFT            7
#define RT_UI_IME_CARET_RIGHT           8
/* M-caret2 选区：Shift+方向键扩选 / Ctrl+A 全选 / 点击定位 caret。
 * M-caret3 补齐桌面编辑键：Delete 前删 / Home+End 行首行尾（Shift 变体扩选）。 */
#define RT_UI_IME_CARET_LEFT_EXT        9
#define RT_UI_IME_CARET_RIGHT_EXT       10
#define RT_UI_IME_SELECT_ALL            11
#define RT_UI_IME_DELETE_FORWARD        12
#define RT_UI_IME_CARET_HOME            13
#define RT_UI_IME_CARET_END             14
#define RT_UI_IME_CARET_HOME_EXT        15
#define RT_UI_IME_CARET_END_EXT         16

typedef struct RtUiImeComposition {
    const char* text;
    int32_t cursor;
    int32_t attr_length;
    const uint8_t* attributes;
} RtUiImeComposition;

typedef void (*RtUiImeHandler)(void* ctx, RtUiElement* target,
                               int32_t kind, const void* payload);

void rt_ui_ime_set_handler(RtUiImeHandler handler, void* ctx);
void rt_ui_ime_set_focus(RtUiElement* input);
void rt_ui_ime_set_candidate_rect(RtUiElement* input,
                                  int32_t x, int32_t y,
                                  int32_t w, int32_t h);
void rt_ui_ime_install_arc_handler(void);
void rt_ui_set_button_click_handler(void* fn, void* env);
void rt_ui_set_button_visual_state_handler(void* fn, void* env);
void rt_ui_set_input_focus_handler(void* fn, void* env);
void rt_ui_dispatch_input_focus(RtUiElement* elem);
/* M-caret2：Input 点击定位 caret（local_dip_x 为命中元素局部坐标）。 */
void rt_ui_set_input_click_handler(void* fn, void* env);
void rt_ui_dispatch_input_click_at(RtUiElement* elem, int32_t local_dip_x);
void rt_ui_set_keyboard_handler(void* fn, void* env);
void rt_ui_dispatch_keyboard(int32_t virtual_key, int32_t shift_down);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* DLANG_RT_UI_ABI_H */
