#include "rt_ui_scroll_dispatch.h"
#include <stdint.h>
#include <stdio.h>

/* Arc 委托/lambda 调用 ABI（与 rt_ui_pointer.c 同一契约）：所有非 env 参数按
 * 「指向槽位的指针」传递（lambda 侧 `load {ty}, ptr %arg` 取值）。native → Arc
 * 回调分派必须把实参值的地址传给 fn——按值传会让 lambda 把标量当槽地址解引用
 * → 0xC0000005（滚轮命中 ScrollView 实测即崩）。env 为 null（静态方法委托）时
 * 走 bare 路径：不带 env、全指针参数。 */
typedef void (*RtUiScrollWheelFnCap)(void* env, int64_t* platform_handle,
                                     int32_t* delta_x, int32_t* delta_y);
typedef void (*RtUiScrollWheelFnBare)(int64_t* platform_handle,
                                      int32_t* delta_x, int32_t* delta_y);
typedef void (*RtUiScrollBarFnCap)(void* env, int64_t* platform_handle,
                                   int32_t* action, double* value);
typedef void (*RtUiScrollBarFnBare)(int64_t* platform_handle,
                                    int32_t* action, double* value);

static RtUiScrollWheelFnCap g_rt_ui_scroll_wheel_fn = NULL;
static void* g_rt_ui_scroll_wheel_env = NULL;
static RtUiScrollBarFnCap g_rt_ui_scroll_bar_fn = NULL;
static void* g_rt_ui_scroll_bar_env = NULL;

void rt_ui_set_scroll_wheel_handler(void* fn, void* env) {
    g_rt_ui_scroll_wheel_fn = (RtUiScrollWheelFnCap)fn;
    g_rt_ui_scroll_wheel_env = env;
}

void rt_ui_set_scroll_bar_handler(void* fn, void* env) {
    g_rt_ui_scroll_bar_fn = (RtUiScrollBarFnCap)fn;
    g_rt_ui_scroll_bar_env = env;
}

void rt_ui_dispatch_scroll_wheel(RtUiElement* elem, int32_t delta_x, int32_t delta_y) {
    /* [SCROLL-DIAG] 临时诊断：确认滚轮派发真实来源（键入引发自动滚动排查）。 */
    fprintf(stderr, "[SCROLL-DIAG] wheel dispatch elem=%p type=%s dx=%d dy=%d\n",
            (void*)elem, elem && elem->type_name ? elem->type_name : "?", delta_x, delta_y);
    if (!elem || !g_rt_ui_scroll_wheel_fn) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (g_rt_ui_scroll_wheel_env) {
        g_rt_ui_scroll_wheel_fn(g_rt_ui_scroll_wheel_env, &handle, &delta_x, &delta_y);
    } else {
        ((RtUiScrollWheelFnBare)g_rt_ui_scroll_wheel_fn)(&handle, &delta_x, &delta_y);
    }
}

void rt_ui_dispatch_scroll_bar(RtUiElement* elem, int32_t action, double value) {
    if (!elem || !g_rt_ui_scroll_bar_fn) return;
    int64_t handle = (int64_t)(uintptr_t)elem;
    if (g_rt_ui_scroll_bar_env) {
        g_rt_ui_scroll_bar_fn(g_rt_ui_scroll_bar_env, &handle, &action, &value);
    } else {
        ((RtUiScrollBarFnBare)g_rt_ui_scroll_bar_fn)(&handle, &action, &value);
    }
}
