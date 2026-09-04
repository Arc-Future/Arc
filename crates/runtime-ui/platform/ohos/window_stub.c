/* RFC 037 M5-OHOS stub — honest, no fake green. */
#include "../common/rt_ui_platform.h"

void* rt_window_create(const char* title, int32_t width, int32_t height) {
    (void)title; (void)width; (void)height; return NULL;
}
void rt_window_close(void* window) { (void)window; }
void rt_window_destroy(void* window) { (void)window; }
int32_t rt_window_should_close(void* window) { (void)window; return 1; }
void rt_window_invalidate(void* window) { (void)window; }
int32_t rt_event_poll(void* window) { (void)window; return RT_EVENT_CLOSE; }
void rt_window_set_text(void* window, const char* text) { (void)window; (void)text; }
/* RFC 037 §D7.2 fallback: 无窗口平台返回 0。 */
int64_t rt_window_native_handle(void* window) { (void)window; return 0; }
void rt_window_get_client_size(void* window, int32_t* out_w, int32_t* out_h) {
    (void)window;
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
}
/* M3 stub：无窗口后端，rt_window_set_root_element 释放 root 后丢弃。 */
void rt_window_set_root_element(void* window, RtUiElement* root) {
    (void)window;
    if (root) rt_ui_element_destroy(root);
}
