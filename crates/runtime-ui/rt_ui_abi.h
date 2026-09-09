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
/* IN-R2：编辑键/ASCII 不再经 IME kind；见 rt_ui_dispatch_key / rt_ui_dispatch_text。 */

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
/* RFC 037 §8 IN-R2：单一键盘通道（mods bit0=Shift bit1=Ctrl）。 */
void rt_ui_set_key_handler(void* fn, void* env);
void rt_ui_dispatch_key(int32_t virtual_key, int32_t mods);
void rt_ui_set_text_handler(void* fn, void* env);
void rt_ui_dispatch_text(const char* utf8);

/* RFC 037 TextBox 剪贴板：堆 UTF-8（空→""；失败可 NULL）；Set 写 CF_UNICODETEXT。 */
char* rt_ui_clipboard_get_text(void);
void rt_ui_clipboard_set_text(const char* utf8);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* DLANG_RT_UI_ABI_H */
