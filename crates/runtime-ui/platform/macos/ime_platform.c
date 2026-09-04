#include "../common/rt_ui_ime_internal.h"

void rt_ui_ime_platform_on_focus(RtUiElement* input) {
    rt_macos_ime_sync_focus(input);
}

void rt_ui_ime_platform_on_candidate_rect(RtUiElement* input,
                                          int32_t x, int32_t y,
                                          int32_t w, int32_t h) {
    (void)input; (void)x; (void)y; (void)w; (void)h;
    rt_macos_ime_sync_candidate_rect();
}
