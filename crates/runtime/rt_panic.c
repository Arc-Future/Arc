/* rt_panic.c — 运行时可观测性 ABI（RFC 017 M1 + M2）
 *
 * rt_panic_at：携带源位置的 panic
 * rt_backtrace：符号化栈回溯（M2 通过 .arcdbg 符号化）
 * ARC 帧折叠：跳过 rt_arc_inc/dec 等运行时内部帧（M2 启用）
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

/* 平台相关 backtrace 头 */
#if defined(_WIN32)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#else
#  include <execinfo.h>
#endif

/* ArcStackFrame：符号化栈帧（RFC 017 D4.2） */
typedef struct ArcStackFrame {
    const char* symbol;  /* 符号名（未修饰）；M2 通过 .arcdbg 符号化 */
    const char* file;    /* 源文件；M2 通过 .arcdbg 符号化 */
    int32_t line;        /* 源码行；M2 通过 .arcdbg 符号化 */
} ArcStackFrame;

/* Forward declaration: rt_backtrace is defined later in this file */
int32_t rt_backtrace(ArcStackFrame* frames, int32_t max_frames);

/* .arcdbg 符号化 API（rt_debug.c，RFC 017 M2 / D5.2） */
extern int32_t rt_debug_lookup(uint64_t addr, const char** symbol, const char** file,
                               int32_t* line, int32_t* col);
extern int32_t rt_debug_is_arc_frame(const char* symbol);

/* json_emit_string：输出 JSON 字符串字面量（含转义）到 stderr */
static void json_emit_string(const char* s) {
    fputc('"', stderr);
    if (s) {
        for (const char* p = s; *p; p++) {
            unsigned char c = (unsigned char)*p;
            switch (c) {
                case '"':  fputs("\\\"", stderr); break;
                case '\\': fputs("\\\\", stderr); break;
                case '\n': fputs("\\n", stderr); break;
                case '\r': fputs("\\r", stderr); break;
                case '\t': fputs("\\t", stderr); break;
                default:
                    if (c < 0x20) {
                        fprintf(stderr, "\\u%04x", c);
                    } else {
                        fputc((int)c, stderr);
                    }
            }
        }
    }
    fputc('"', stderr);
}

/* emit_panic_json：输出 RFC 017 D4.4 panic JSON 到 stderr
 *
 * M2：backtrace 字段填充符号化栈帧（通过 .arcdbg 解析）。
 */
static void emit_panic_json(const char* msg, const char* file, int32_t line, int32_t col) {
    /* Capture backtrace for JSON output */
    ArcStackFrame frames[32];
    int32_t bt_count = rt_backtrace(frames, 32);

    fputs("{\n", stderr);
    fputs("  \"message\": ", stderr);
    json_emit_string(msg ? msg : "unknown");
    fputs(",\n", stderr);
    fputs("  \"location\": { ", stderr);
    fputs("\"file\": ", stderr);
    json_emit_string(file ? file : "");
    fprintf(stderr, ", \"line\": %d, \"col\": %d },\n", line, col);
    fputs("  \"backtrace\": [", stderr);
    for (int32_t i = 0; i < bt_count; i++) {
        if (i > 0) fputs(",", stderr);
        fputs("\n    {", stderr);
        fputs("\"symbol\": ", stderr);
        json_emit_string(frames[i].symbol ? frames[i].symbol : "");
        fputs(", \"file\": ", stderr);
        json_emit_string(frames[i].file ? frames[i].file : "");
        fprintf(stderr, ", \"line\": %d", frames[i].line);
        fputs("}", stderr);
    }
    if (bt_count > 0) fputs("\n  ", stderr);
    fputs("]\n", stderr);
    fputs("}\n", stderr);
}

/* rt_panic_at：携带源位置的 panic（RFC 014）
 *
 * codegen 在 panic 调用处插入当前 span 对应的 file:line:col。
 * 默认输出人类可读格式；ARC_PANIC_FORMAT=json 时输出 JSON（RFC 031 §3）。
 */
void rt_panic_at(const char* msg, const char* file, int32_t line, int32_t col) {
    const char* fmt = getenv("ARC_PANIC_FORMAT");
    if (fmt && strcmp(fmt, "json") == 0) {
        emit_panic_json(msg, file, line, col);
    } else if (file && line > 0) {
        fprintf(stderr, "Arc panic at %s:%d:%d: %s\n", file, line, col, msg ? msg : "unknown");
    } else if (file) {
        fprintf(stderr, "Arc panic in %s: %s\n", file, msg ? msg : "unknown");
    } else {
        fprintf(stderr, "Arc panic: %s\n", msg ? msg : "unknown");
    }
    exit(1);
}

/* rt_panic：兼容入口（内部调用 rt_panic_at，源位置未知） */
void rt_panic(const char* msg) {
    rt_panic_at(msg, NULL, 0, 0);
}

/* rt_backtrace：捕获当前调用栈并符号化（RFC 017 D4.2 + M2）
 *
 * M2 实现：用平台 API 捕获指令地址，通过 .arcdbg 符号化填充字段。
 * ARC 帧折叠：跳过 rt_arc_inc/rt_arc_dec 等运行时内部帧。
 *
 * 返回：实际填充的帧数（可能小于 max_frames，因为 ARC 帧被折叠）。
 */
int32_t rt_backtrace(ArcStackFrame* frames, int32_t max_frames) {
    if (!frames || max_frames <= 0) return 0;

#if defined(_WIN32)
    /* Windows: RtlCaptureStackBackTrace 捕获返回地址 */
    void* addrs[64];
    int32_t raw_count = (int32_t)RtlCaptureStackBackTrace(0, 64, addrs, NULL);
#else
    /* Unix: backtrace() 捕获返回地址 */
    void* addrs[64];
    int32_t raw_count = (int32_t)backtrace(addrs, 64);
#endif

    /* 符号化 + ARC 帧折叠 */
    int32_t out_count = 0;
    for (int32_t i = 0; i < raw_count && out_count < max_frames; i++) {
        uint64_t addr = (uint64_t)(uintptr_t)addrs[i];
        const char* symbol = NULL;
        const char* file = NULL;
        int32_t line = 0;
        int32_t col = 0;

        /* M2: 通过 .arcdbg 符号化。仅包含成功符号化的帧；
         * 无 .arcdbg 时返回 0（保持与 M1 空回溯兼容）。 */
        if (!rt_debug_lookup(addr, &symbol, &file, &line, &col)) {
            continue;
        }
        /* ARC 帧折叠：跳过运行时内部帧 */
        if (rt_debug_is_arc_frame(symbol)) {
            continue;
        }
        frames[out_count].symbol = symbol;
        frames[out_count].file = file;
        frames[out_count].line = line;
        out_count++;
    }
    return out_count;
}

/* ===== VEH crash reporter（诊断；落点受控，禁止写仓库根） ===== */
#if defined(_WIN32)
/* 崩溃转储路径：`ARC_CRASH_DUMP` 显式覆盖；缺省 %TEMP%/arc_crash.txt
 * （历史教训：硬编码仓库根路径曾污染工作区——工作区卫生规则产物落 target/）。 */
static const char* arc_dbg_crash_path(void) {
    static char buf[1024];
    const char* p = getenv("ARC_CRASH_DUMP");
    if (p && p[0]) {
        return p;
    }
    const char* t = getenv("TEMP");
    if (t && t[0]) {
        snprintf(buf, sizeof buf, "%s\\arc_crash.txt", t);
        return buf;
    }
    return "arc_crash.txt";
}

/* 解析地址归属：优先给出「模块名+偏移」，再附 Arc 符号化作为补充。
 * rt_debug_lookup 用「最近函数」线性匹配，对非本模块地址会误标，故以模块为准。 */
static void arc_dbg_resolve(uint64_t addr, char* out, size_t outsz) {
    if (!out || outsz == 0) return;
    char loose[128];
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((const void*)(uintptr_t)addr, &mbi, sizeof mbi) &&
        mbi.State == MEM_COMMIT && mbi.AllocationBase != NULL) {
        HMODULE mod = (HMODULE)mbi.AllocationBase;
        char file[MAX_PATH];
        DWORD len = GetModuleFileNameA(mod, file, sizeof file);
        if (len > 0) {
            const char* name = strrchr(file, '\\');
            if (name) name++; else name = file;
            uint64_t base = (uint64_t)(uintptr_t)mod;
            snprintf(loose, sizeof loose, "%s+0x%llx", name, (unsigned long long)(addr - base));
        } else {
            snprintf(loose, sizeof loose, "mod+0x%llx", (unsigned long long)(uintptr_t)mod);
        }
    } else {
        snprintf(loose, sizeof loose, "0x%llx", (unsigned long long)addr);
    }
    const char* sym = NULL; const char* file = NULL;
    int32_t line = 0, col = 0;
    if (rt_debug_lookup(addr, &sym, &file, &line, &col) && sym) {
        snprintf(out, outsz, "%s  %s(%s:%d)", loose, sym, file ? file : "?", line);
    } else {
        snprintf(out, outsz, "%s", loose);
    }
}

static LONG WINAPI arc_dbg_veh(PEXCEPTION_POINTERS e) {
    if (e->ExceptionRecord->ExceptionCode == 0xC0000005) {
        FILE* f = fopen(arc_dbg_crash_path(), "w");
        if (f) {
            uint64_t insn = (uint64_t)e->ExceptionRecord->ExceptionAddress;
            uint64_t target = e->ExceptionRecord->NumberParameters >= 2
                              ? (uint64_t)e->ExceptionRecord->ExceptionInformation[1] : 0;
            uint32_t iswrite = e->ExceptionRecord->NumberParameters >= 1
                               ? (uint32_t)e->ExceptionRecord->ExceptionInformation[0] : 0;
            char loc[400];
            arc_dbg_resolve(insn, loc, sizeof loc);
            fprintf(f, "TYPE=%s\n", iswrite ? "WRITE" : "READ");
            fprintf(f, "FAULT_INSN=%s\n", loc);
            fprintf(f, "DATA_ADDR=0x%llx\n", (unsigned long long)target);
            if (e->ContextRecord) {
                const CONTEXT* c = e->ContextRecord;
                fprintf(f, "REGS rax=%llx rbx=%llx rcx=%llx rdx=%llx\n",
                        (unsigned long long)c->Rax, (unsigned long long)c->Rbx,
                        (unsigned long long)c->Rcx, (unsigned long long)c->Rdx);
                fprintf(f, "REGS rsi=%llx rdi=%llx rbp=%llx rsp=%llx rip=%llx\n",
                        (unsigned long long)c->Rsi, (unsigned long long)c->Rdi,
                        (unsigned long long)c->Rbp, (unsigned long long)c->Rsp,
                        (unsigned long long)c->Rip);
                fprintf(f, "REGS r8=%llx r9=%llx r10=%llx r11=%llx\n",
                        (unsigned long long)c->R8,  (unsigned long long)c->R9,
                        (unsigned long long)c->R10, (unsigned long long)c->R11);
            }
            MEMORY_BASIC_INFORMATION mbi;
            if (target && VirtualQuery((const void*)(uintptr_t)target, &mbi, sizeof mbi) && mbi.State == MEM_COMMIT) {
                fprintf(f, "TARGET_REGION=0x%llx size=0x%llx\n",
                        (unsigned long long)(uintptr_t)mbi.BaseAddress,
                        (unsigned long long)mbi.RegionSize);
            }
            void* bt[48];
            USHORT n = RtlCaptureStackBackTrace(0, 48, bt, NULL);
            fprintf(f, "BACKTRACE=%u\n", (unsigned)n);
            for (USHORT i = 0; i < n; i++) {
                char fl[400];
                arc_dbg_resolve((uint64_t)(uintptr_t)bt[i], fl, sizeof fl);
                fprintf(f, "#%u %s\n", (unsigned)i, fl);
            }
            fclose(f);
        }
        return EXCEPTION_CONTINUE_SEARCH;
    }
    return EXCEPTION_CONTINUE_SEARCH;
}
/* VEH 注册句柄：注销时凭句柄从链上摘除本模块的节点。 */
static void* arc_dbg_veh_handle = NULL;

__attribute__((constructor)) static void arc_dbg_install_veh(void) {
    if (getenv("ARC_NO_DIAG_VEH") && getenv("ARC_NO_DIAG_VEH")[0] == '1') {
        return;
    }
    arc_dbg_veh_handle = AddVectoredExceptionHandler(1, arc_dbg_veh);
}

/* 注册/注销必须配对（RFC 017 §2.4 热卸载）：Arc DLL 静态链接本文件，
 * 每次 LoadLibrary 经 constructor 注册一个 arc_dbg_veh 节点；若 FreeLibrary
 * 时节点残留在进程级 VEH 链上，映像 unmap 后链即含悬垂回调——后续任何
 * 异常（含正常 throw 的 0xE06D7363 first-chance）在分发器遍历 VEH 链时
 * 调用指向已卸载映像的 wild 指针，AV 发生在分发器内部并递归崩溃，绕过
 * UEF/WER 直接终止进程（exit=0xC0000005、全程无 handler 输出）。
 * destructor 由 CRT 在 DllMain(DETACH) 中执行，FreeLibrary 先摘节点、
 * 后 unmap 映像，链上不会残留悬垂节点。 */
__attribute__((destructor)) static void arc_dbg_remove_veh(void) {
    if (arc_dbg_veh_handle) {
        RemoveVectoredExceptionHandler(arc_dbg_veh_handle);
        arc_dbg_veh_handle = NULL;
    }
}
#endif

/* rt_print_backtrace：便捷函数，将栈回溯输出到 stderr */
void rt_print_backtrace(void) {
    ArcStackFrame frames[32];
    int32_t count = rt_backtrace(frames, 32);
    if (count > 0) {
        fprintf(stderr, "backtrace:\n");
        for (int32_t i = 0; i < count; i++) {
            if (frames[i].symbol) {
                fprintf(stderr, "  %d: %s (%s:%d)\n", i, frames[i].symbol,
                        frames[i].file ? frames[i].file : "?",
                        frames[i].line);
            } else {
                fprintf(stderr, "  %d: <unknown>\n", i);
            }
        }
    }
}
