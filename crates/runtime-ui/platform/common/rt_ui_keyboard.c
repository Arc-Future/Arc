/*
 * RFC 037 §8 IN-R2: 单一键盘通道。
 * WM_KEYDOWN → rt_ui_dispatch_key(vk, mods)；WM_CHAR → rt_ui_dispatch_text(utf8)。
 * Linux/macOS: Draft stub — 平台刀迁移前 key/text handler 可装、dispatch 无平台源。
 */
#include "rt_ui_element_internal.h"
#include <stdint.h>

/* Arc 委托 ABI：非 env 参数按「指向槽位的指针」传递。 */
typedef void (*RtUiKeyFnCap)(void* env, int32_t* virtual_key, int32_t* mods);
typedef void (*RtUiKeyFnBare)(int32_t* virtual_key, int32_t* mods);
typedef void (*RtUiTextFnCap)(void* env, int64_t* utf8_ptr);
typedef void (*RtUiTextFnBare)(int64_t* utf8_ptr);

static RtUiKeyFnCap g_rt_ui_key_fn = NULL;
static void* g_rt_ui_key_env = NULL;
static RtUiTextFnCap g_rt_ui_text_fn = NULL;
static void* g_rt_ui_text_env = NULL;

void rt_ui_set_key_handler(void* fn, void* env) {
    g_rt_ui_key_fn = (RtUiKeyFnCap)fn;
    g_rt_ui_key_env = env;
}

void rt_ui_dispatch_key(int32_t virtual_key, int32_t mods) {
    if (!g_rt_ui_key_fn) return;
    if (g_rt_ui_key_env) {
        g_rt_ui_key_fn(g_rt_ui_key_env, &virtual_key, &mods);
    } else {
        ((RtUiKeyFnBare)g_rt_ui_key_fn)(&virtual_key, &mods);
    }
}

void rt_ui_set_text_handler(void* fn, void* env) {
    g_rt_ui_text_fn = (RtUiTextFnCap)fn;
    g_rt_ui_text_env = env;
}

void rt_ui_dispatch_text(const char* utf8) {
    if (!g_rt_ui_text_fn || !utf8) return;
    int64_t ptr = (int64_t)(intptr_t)utf8;
    if (g_rt_ui_text_env) {
        g_rt_ui_text_fn(g_rt_ui_text_env, &ptr);
    } else {
        ((RtUiTextFnBare)g_rt_ui_text_fn)(&ptr);
    }
}
