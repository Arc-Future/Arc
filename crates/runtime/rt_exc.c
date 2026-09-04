// Exception unwinding ABI (zero-cost EH: invoke/landingpad → native raise).
//
// Milestone ② (zero-cost EH, Windows SEH 主平台): `rt_throw` on Windows raises
// the exception natively via `_CxxThrowException`, carrying the Arc exception
// object in the payload. Codegen catches it via invoke → catchswitch/catchpad.
// `rt_exception` is per-thread TLS so concurrent throws on different threads
// never race. Milestone ⑥ removed the legacy try-stack registry; POSIX targets
// have no handler registry until milestone ⑨ (Itanium) — an unhandled throw
// converges to `rt_panic`.
//
// StackTrace (L2 符号完备)：`rt_format_stacktrace` 在 throw 路径由 codegen 写入
// `Exception.StackTrace`。捕获真实返回地址；主路径嵌入 `__arc_dbg_table`
//（与 DWARF `-g` 解耦，默认有函数名 + 可行时 file:line）；POSIX 上
// backtrace_symbols 为次级；仍无符号时诚实 `at <0x…>`；极端无帧时 `at <throw>`。

#include "rt_abi.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#else
#  include <execinfo.h>
#endif

#define RT_ST_MAX_FRAMES 32
#define RT_ST_BUF_SIZE 4096

/* Per-thread exception object slot. TLS guarantees a throw on one thread
 * cannot overwrite the exception being caught on another. */
#if defined(_WIN32)
__declspec(thread) static void* rt_exception = NULL;
#else
_Thread_local static void* rt_exception = NULL;
#endif

#if defined(_WIN32)
/* Minimal MSVC C++ ThrowInfo for a catch-all (typeDescriptor RVA is never
 * dereferenced by a `catch (…)`/`catch ptr null` filter; kept 0 as the probe
 * validated). Layout matches <vcruntime.h> so no header dependency is needed
 * and rt_exc.c stays C99-clean. */
typedef struct RtCatchableType {
    unsigned long properties;
    intptr_t typeDescriptor;
    unsigned long sizeOrOffset;
    intptr_t copyFunction;
    void* pNull;
} RtCatchableType;
typedef struct RtCatchableTypeArray {
    int nCatchableTypes;
    RtCatchableType* arrayOfCatchableTypes[1];
} RtCatchableTypeArray;
typedef struct RtThrowInfo {
    unsigned int attributes;
    void* pmfnUnwind;
    void* pForwardCompat;
    RtCatchableTypeArray* pCatchableTypeArray;
} RtThrowInfo;

static RtCatchableType rt_catchable_type = { 0, 0, sizeof(void*), 0, NULL };
static RtCatchableTypeArray rt_catchable_type_array = { 1, { &rt_catchable_type } };
static RtThrowInfo rt_throw_info = { 0, NULL, NULL, &rt_catchable_type_array };

extern void __cdecl _CxxThrowException(void* pExceptionObject, void* pThrowInfo);
#endif

/* rt_debug.c — symbolization for captured frames */
extern int32_t rt_debug_lookup(uint64_t addr, const char** symbol, const char** file,
                               int32_t* line, int32_t* col);
extern int32_t rt_debug_is_arc_frame(const char* symbol);

void rt_throw(void* exception_obj) {
    rt_exception = exception_obj;
#if defined(_WIN32)
    /* Zero-cost EH milestone ②: native raise. The payload is an 8-byte struct
     * holding the Arc exception object pointer; the MSVC unwinder delivers it
     * to the matching catchpad and codegen reads it back via rt_get_exception. */
    {
        struct { void* obj; } payload;
        payload.obj = exception_obj;
        _CxxThrowException(&payload, &rt_throw_info);
        __builtin_unreachable();  /* _CxxThrowException never returns */
    }
#else
    /* POSIX: zero-cost EH (Itanium) is milestone ⑨ (1.1+, non-1.0 gate).
     * Milestone ⑥ removed the legacy try-stack registry, so there is no
     * registered handler on this path — an unhandled throw converges to
     * `rt_panic("unhandled exception")` (std::terminate 等价). */
    rt_panic("unhandled exception");
#endif
}

void* rt_get_exception(void) {
    return rt_exception;
}

/* Format one stack frame line into buf; returns bytes written (excl. NUL). */
static int rt_st_append_frame(char* buf, int cap, int used,
                              const char* symbol, const char* file, int32_t line,
                              uint64_t addr) {
    int n;
    if (symbol && symbol[0]) {
        if (file && file[0] && line > 0) {
            n = snprintf(buf + used, (size_t)(cap - used),
                         "   at %s in %s:%d\n", symbol, file, line);
        } else {
            n = snprintf(buf + used, (size_t)(cap - used),
                         "   at %s\n", symbol);
        }
    } else {
        /* Honest unresolved frame — real capture, no fake symbol names. */
        n = snprintf(buf + used, (size_t)(cap - used),
                     "   at <0x%llx>\n", (unsigned long long)addr);
    }
    if (n < 0) return used;
    if (used + n >= cap) return cap - 1;
    return used + n;
}

char* rt_format_stacktrace(void) {
    void* addrs[64];
    int32_t raw_count = 0;

#if defined(_WIN32)
    /* Skip this frame so the first entry is the throw site / caller. */
    raw_count = (int32_t)RtlCaptureStackBackTrace(1, 64, addrs, NULL);
#else
    raw_count = (int32_t)backtrace(addrs, 64);
    /* Skip rt_format_stacktrace itself when present as frame 0. */
    if (raw_count > 0) {
        memmove(addrs, addrs + 1, (size_t)(raw_count - 1) * sizeof(void*));
        raw_count -= 1;
    }
#endif

#if !defined(_WIN32)
    char** bt_syms = NULL;
    if (raw_count > 0) {
        bt_syms = backtrace_symbols(addrs, raw_count);
    }
#endif

    char buf[RT_ST_BUF_SIZE];
    int used = 0;
    int out_frames = 0;

    for (int32_t i = 0; i < raw_count && out_frames < RT_ST_MAX_FRAMES; i++) {
        uint64_t addr = (uint64_t)(uintptr_t)addrs[i];
        const char* symbol = NULL;
        const char* file = NULL;
        int32_t line = 0;
        int32_t col = 0;

        if (rt_debug_lookup(addr, &symbol, &file, &line, &col)) {
            if (rt_debug_is_arc_frame(symbol)) {
                continue;
            }
            used = rt_st_append_frame(buf, RT_ST_BUF_SIZE, used, symbol, file, line, addr);
#if !defined(_WIN32)
        } else if (bt_syms && bt_syms[i] && bt_syms[i][0]) {
            /* POSIX backtrace_symbols: often "path(sym+off) [addr]"; use raw. */
            used = rt_st_append_frame(buf, RT_ST_BUF_SIZE, used, bt_syms[i], NULL, 0, addr);
#endif
        } else {
            used = rt_st_append_frame(buf, RT_ST_BUF_SIZE, used, NULL, NULL, 0, addr);
        }
        out_frames++;
        if (used >= RT_ST_BUF_SIZE - 1) break;
    }

#if !defined(_WIN32)
    if (bt_syms) free(bt_syms);
#endif

    if (out_frames == 0) {
        /* Platform returned no frames (rare on sjlj/Windows) — still non-null. */
        const char* fallback = "   at <throw>\n";
        size_t n = strlen(fallback);
        char* out = (char*)malloc(n + 1);
        if (!out) return NULL;
        memcpy(out, fallback, n + 1);
        return out;
    }

    buf[used] = '\0';
    char* out = (char*)malloc((size_t)used + 1);
    if (!out) return NULL;
    memcpy(out, buf, (size_t)used + 1);
    return out;
}
