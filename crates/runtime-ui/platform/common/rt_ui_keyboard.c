/*
 * RFC 037 M-focus Draft: keyboard handler dispatch (Tab / Enter / Space).
 * Platform key translation: crates/runtime-ui/platform/windows/keyboard_win32.c (Win32 WM_KEYDOWN).
 * Linux/macOS: Draft stub — rt_ui_dispatch_keyboard no-op until platform刀迁移。
 */
#include "rt_ui_element_internal.h"
#include <stdint.h>

/* Arc 委托/lambda 调用 ABI（与 rt_ui_pointer.c 同一契约）：所有非 env 参数按
 * 「指向槽位的指针」传递；env 为 null（静态方法委托）时走 bare 路径。 */
typedef void (*RtUiKeyboardFnCap)(void* env, int32_t* virtual_key, int32_t* shift_down);
typedef void (*RtUiKeyboardFnBare)(int32_t* virtual_key, int32_t* shift_down);

static RtUiKeyboardFnCap g_rt_ui_keyboard_fn = NULL;
static void* g_rt_ui_keyboard_env = NULL;

void rt_ui_set_keyboard_handler(void* fn, void* env) {
    g_rt_ui_keyboard_fn = (RtUiKeyboardFnCap)fn;
    g_rt_ui_keyboard_env = env;
}

void rt_ui_dispatch_keyboard(int32_t virtual_key, int32_t shift_down) {
    if (!g_rt_ui_keyboard_fn) return;
    if (g_rt_ui_keyboard_env) {
        g_rt_ui_keyboard_fn(g_rt_ui_keyboard_env, &virtual_key, &shift_down);
    } else {
        ((RtUiKeyboardFnBare)g_rt_ui_keyboard_fn)(&virtual_key, &shift_down);
    }
}
