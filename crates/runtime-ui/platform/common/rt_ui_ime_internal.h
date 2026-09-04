#ifndef ARC_RT_UI_IME_INTERNAL_H
#define ARC_RT_UI_IME_INTERNAL_H

#include "rt_ui_ime_types.h"

#ifdef __cplusplus
extern "C" {
#endif

void rt_ui_ime_dispatch(int32_t kind, const void* payload);
RtUiElement* rt_ui_ime_get_focus(void);
int rt_ui_ime_query_candidate_rect(RtUiElement** out_input,
                                   int32_t* x, int32_t* y,
                                   int32_t* w, int32_t* h);

#if defined(__APPLE__)
void rt_macos_ime_sync_focus(RtUiElement* input);
void rt_macos_ime_sync_candidate_rect(void);
#endif

#if defined(__linux__)
void rt_linux_ime_on_focus_changed(RtUiElement* input);
void rt_linux_ime_on_candidate_rect(void);
#endif

#ifdef __cplusplus
}
#endif

#endif /* ARC_RT_UI_IME_INTERNAL_H */
