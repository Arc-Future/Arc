#ifndef DLANG_RT_UI_IME_PLATFORM_H
#define DLANG_RT_UI_IME_PLATFORM_H

/*
 * RFC 037 · crates/runtime-ui/platform/<os>/ime_platform.c implements these hooks.
 * rt_ui_ime.c calls them on set_focus / set_candidate_rect.
 */
#include "rt_ui_abi.h"

#ifdef __cplusplus
extern "C" {
#endif

void rt_ui_ime_platform_on_focus(RtUiElement* input);
void rt_ui_ime_platform_on_candidate_rect(RtUiElement* input,
                                          int32_t x, int32_t y,
                                          int32_t w, int32_t h);

#ifdef __cplusplus
}
#endif

#endif /* DLANG_RT_UI_IME_PLATFORM_H */
