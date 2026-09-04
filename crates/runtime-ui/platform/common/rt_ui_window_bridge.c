#include "../../../runtime/rt_abi.h"
#include "rt_ui_element_internal.h"

void rt_window_set_text(void* window, const char* text);
void rt_window_set_root_element(void* window, RtUiElement* root);


/* ===========================================================================
 * Arc builtin 桥接函数——由 codegen emit_call.rs 拦截 `Window.Run` /
 * `Window.RunWithText` 静态调用后生成 `call void @__arc_window_run[_with_text]`。
 * =========================================================================*/

void __arc_window_run(const char* title, int32_t width, int32_t height) {
    void* win = rt_window_create(title, width, height);
    if (!win) return;
    while (!rt_window_should_close(win)) {
        rt_event_wait(win, -1);
        rt_event_poll(win);
    }
    rt_window_destroy(win);
}

void __arc_window_run_with_text(
    const char* title, int32_t width, int32_t height, const char* text) {
    void* win = rt_window_create(title, width, height);
    if (!win) return;
    rt_window_set_text(win, text);
    while (!rt_window_should_close(win)) {
        rt_event_wait(win, -1);
        rt_event_poll(win);
    }
    rt_window_destroy(win);
}

/* RFC 037 M3：带元素树根的窗口运行入口。
 *
 * codegen 拦截 `WindowHost.RunWithRoot(title, w, h, text, root_handle)` 后
 * 发射 `call void @__arc_window_run_with_root(ptr title, i32 w, i32 h,
 * ptr root)` LLVM IR。root 为 i64 句柄经 `inttoptr` 转换为 RtUiElement*。
 *
 * 流程：
 *   1. rt_window_create 创建平台窗口
 *   2. rt_window_set_root_element 设置元素树根（渲染由 wgpu 后端驱动）
 *   3. 进入消息循环直到窗口关闭
 *   4. rt_window_destroy 递归释放元素树 + 平台窗口
 *
 * 与 __arc_window_run_with_text 关系：M3 路径取代 M2 文本路径——
 * root 非空时渲染走 wgpu 后端（WgpuRender），完全跳过 GDI / 软件光栅。 */
void __arc_window_run_with_root(
    const char* title, int32_t width, int32_t height,
    /* int64_t root_handle */ int64_t root_handle) {
    void* win = rt_window_create(title, width, height);
    if (!win) return;
    if (root_handle != 0) {
        RtUiElement* root = (RtUiElement*)(uintptr_t)root_handle;
        rt_window_set_root_element(win, root);
    }
    while (!rt_window_should_close(win)) {
        rt_event_wait(win, -1);
        rt_event_poll(win);
    }
    rt_window_destroy(win);
}
