/*
 * Arc ↔ rt_ui_ime_set_handler shim.
 * ABI 声明：crates/runtime-ui/rt_ui_abi.h
 * 平台无关状态：crates/runtime-ui/rt_ui_ime.c
 */
#include "../../rt_ui_abi.h"

void ImeBridge_OnNativeEvent(void* ctx, RtUiElement* target,
                             int32_t kind, const void* payload);

#if defined(_MSC_VER)
void ImeBridge_OnNativeEvent_default(void* ctx, RtUiElement* target,
                                     int32_t kind, const void* payload) {
    (void)ctx;
    (void)target;
    (void)kind;
    (void)payload;
}
#pragma comment(linker, "/alternatename:ImeBridge_OnNativeEvent=ImeBridge_OnNativeEvent_default")
#else
__attribute__((weak)) void ImeBridge_OnNativeEvent(void* ctx, RtUiElement* target,
                                                   int32_t kind, const void* payload) {
    (void)ctx;
    (void)target;
    (void)kind;
    (void)payload;
}
#endif

void rt_ui_ime_install_arc_handler(void) {
    rt_ui_ime_set_handler((RtUiImeHandler)ImeBridge_OnNativeEvent, NULL);
}
