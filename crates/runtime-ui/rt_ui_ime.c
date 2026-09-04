/*
 * RFC 037 · rt_ui_ime_* platform-agnostic core.
 * OS IME wiring: crates/runtime-ui/platform/<os>/ime_platform.c + window backend.
 */
#include "rt_ui_ime_platform.h"
#include <stddef.h>
#include <stdio.h>

typedef struct RtUiImeState {
    RtUiImeHandler handler;
    void* handler_ctx;
    RtUiElement* focus;
    RtUiElement* cand_input;
    int32_t cand_x;
    int32_t cand_y;
    int32_t cand_w;
    int32_t cand_h;
    int cand_valid;
} RtUiImeState;

static RtUiImeState g_rt_ui_ime;

void rt_ui_ime_set_handler(RtUiImeHandler handler, void* ctx) {
    g_rt_ui_ime.handler = handler;
    g_rt_ui_ime.handler_ctx = ctx;
}

void rt_ui_ime_set_focus(RtUiElement* input) {
    fprintf(stderr, "[DBG] ime_set_focus %p\n", (void*)input);
    g_rt_ui_ime.focus = input;
    if (!input) {
        g_rt_ui_ime.cand_valid = 0;
        g_rt_ui_ime.cand_input = NULL;
    }
    rt_ui_ime_platform_on_focus(input);
}

void rt_ui_ime_set_candidate_rect(RtUiElement* input,
                                  int32_t x, int32_t y,
                                  int32_t w, int32_t h) {
    g_rt_ui_ime.cand_input = input;
    g_rt_ui_ime.cand_x = x;
    g_rt_ui_ime.cand_y = y;
    g_rt_ui_ime.cand_w = w;
    g_rt_ui_ime.cand_h = h;
    g_rt_ui_ime.cand_valid = (input && w > 0 && h > 0) ? 1 : 0;
    rt_ui_ime_platform_on_candidate_rect(input, x, y, w, h);
}

void rt_ui_ime_dispatch(int32_t kind, const void* payload) {
    fprintf(stderr, "[DBG] ime_dispatch kind=%d handler=%p focus=%p\n",
            (int)kind, (void*)g_rt_ui_ime.handler, (void*)g_rt_ui_ime.focus);
    if (!g_rt_ui_ime.handler) {
        return;
    }
    g_rt_ui_ime.handler(g_rt_ui_ime.handler_ctx, g_rt_ui_ime.focus, kind, payload);
}

RtUiElement* rt_ui_ime_get_focus(void) {
    return g_rt_ui_ime.focus;
}

int rt_ui_ime_query_candidate_rect(RtUiElement** out_input,
                                   int32_t* x, int32_t* y,
                                   int32_t* w, int32_t* h) {
    if (!g_rt_ui_ime.cand_valid || !g_rt_ui_ime.cand_input ||
        g_rt_ui_ime.cand_input != g_rt_ui_ime.focus) {
        return 0;
    }
    if (out_input) *out_input = g_rt_ui_ime.cand_input;
    if (x) *x = g_rt_ui_ime.cand_x;
    if (y) *y = g_rt_ui_ime.cand_y;
    if (w) *w = g_rt_ui_ime.cand_w;
    if (h) *h = g_rt_ui_ime.cand_h;
    return 1;
}
