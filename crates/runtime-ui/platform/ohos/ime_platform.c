#include "../common/rt_ui_ime_internal.h"
#include <stdio.h>

static void rt_ui_ime_log_once(const char* message) {
    static int logged;
    if (logged) return;
    logged = 1;
    fprintf(stderr, "[arc-ui][ime] %s\n", message);
}

void rt_ui_ime_platform_on_focus(RtUiElement* input) {
    (void)input;
    rt_ui_ime_log_once("OHOS IME not implemented (M5-OHOS AbilityHost extension track)");
}

void rt_ui_ime_platform_on_candidate_rect(RtUiElement* input,
                                          int32_t x, int32_t y,
                                          int32_t w, int32_t h) {
    (void)input; (void)x; (void)y; (void)w; (void)h;
}
