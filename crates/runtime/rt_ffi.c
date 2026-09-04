// RFC 016 M2: native callback TLS 回调表。
//
// 有捕获 lambda 传给 `.ani` 契约声明为 `native callback` 的 C 函数形参时，
// codegen 在调用前把 arc_closure 指针存入当前线程的 TLS slot；trampoline
// 从 slot 取 closure 后间接调用 `fn_ptr(env, ...)`。C 端回调完成后调用点
// 清理 slot。
//
// 静态 TLS（__declspec(thread) / _Thread_local）直接内嵌，线程启动零初始化；
// 本运行时 .o 直接链入可执行文件，无动态库加载场景的 TLS 初始化问题。

#include "rt_abi.h"
#include <stdint.h>

#if defined(_MSC_VER)
#define RT_FFI_TLS __declspec(thread)
#else
#define RT_FFI_TLS _Thread_local
#endif

static RT_FFI_TLS void* g_rt_ffi_slots[RT_FFI_MAX_CALLBACK_SLOTS];

void* rt_ffi_get_callback(int32_t slot) {
    if (slot < 0 || slot >= RT_FFI_MAX_CALLBACK_SLOTS) return NULL;
    return g_rt_ffi_slots[slot];
}

void rt_ffi_set_callback(int32_t slot, void* closure) {
    if (slot < 0 || slot >= RT_FFI_MAX_CALLBACK_SLOTS) return;
    g_rt_ffi_slots[slot] = closure;
}

void rt_ffi_clear_callback(int32_t slot) {
    if (slot < 0 || slot >= RT_FFI_MAX_CALLBACK_SLOTS) return;
    g_rt_ffi_slots[slot] = NULL;
}
