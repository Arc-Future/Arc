// Environment ABI
// Phase 1 (2026-07-20): 命令行参数访问（argc/argv）。
// Phase 2 (2026-07-21): 环境变量、进程控制、系统信息、当前目录、机器/用户名。
//
// 设计：
// - 进程级全局状态（argc/argv/exit_code）仅在主线程初始化，运行时只读访问
// - 字符串返回值为 malloc 出的 NUL 终止串，调用方拥有所有权；
//   失败一律返回空串（malloc(1) 的 '\0'），杜绝 NULL 解引用
// - 跨平台：Windows 走 GetEnvironmentVariable/SetEnvironmentVariable/GetCurrentDirectory/
//   SetCurrentDirectory/GetComputerName/GetUserName；POSIX 走 getenv/setenv/getcwd/
//   chdir/gethostname/getlogin_r
// - rt_env_newline 返回静态常量（无需释放）

#include "rt_abi.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#include <lmcons.h> /* UNLEN for rt_env_user_name */
#include <process.h> /* _exit */
#else
#include <unistd.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <time.h>
#ifdef __APPLE__
#include <TargetConditionals.h>
#include <mach-o/dyld.h> /* _NSGetExecutablePath（rt_env_self_exe） */
#endif
#endif

/* ---- 全局状态（进程级） ---- */

static int    g_argc = 0;
static char** g_argv = NULL;
static int32_t g_exit_code = 0;

/* ---- 初始化 —— 程序启动时调用一次 ---- */

#ifdef _WIN32
static LONG WINAPI rt_env_crash_probe_veh(EXCEPTION_POINTERS* ep);
#endif

void rt_env_init(int argc, char** argv) {
    g_argc = argc;
    g_argv = argv;
    rt_type_init();  /* 启动期初始化基元 typeinfo，供反射元数据直接读取 */
#ifdef _WIN32
    /* 取证基建（RFC 046 唤醒链崩溃专项）：`ARC_CRASH_PROBE=1` 时安装 hard-crash
     * VEH——打印异常码/地址/模块偏移/访问目标/寄存器/栈上返回地址候选，随后放行
     * 默认处理。默认关闭零开销；harness 侧以 PDB publics 符号化重建调用链。
     * ARC 异常（MSVC C++ EH 0xE06D7363）与其余软异常不在此拦截。 */
    if (getenv("ARC_CRASH_PROBE") != NULL) {
        AddVectoredExceptionHandler(0, rt_env_crash_probe_veh);
    }
#endif
}

#ifdef _WIN32
static LONG WINAPI rt_env_crash_probe_veh(EXCEPTION_POINTERS* ep) {
    static const DWORD hard[] = { 0xC0000005, 0xC000001D, 0xC0000094, 0xC00000FD, 0xC0000409 };
    DWORD code = ep->ExceptionRecord->ExceptionCode;
    int hard_hit = 0;
    for (int i = 0; i < (int)(sizeof(hard) / sizeof(hard[0])); i++) {
        if (code == hard[i]) { hard_hit = 1; break; }
    }
    if (!hard_hit) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    /* stdout 为 CRT 全缓冲（重定向场景）——崩溃即丢缓冲，stdout 语句夹逼
     * 全部失效。崩溃瞬间先冲刷，恢复「最后一条打印=最后执行的语句」证据力。 */
    fflush(stdout);
    void* addr = ep->ExceptionRecord->ExceptionAddress;
    HMODULE mod = NULL;
    char mod_name[256] = "?";
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                       GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                       (LPCWSTR)addr, &mod);
    uintptr_t base = (uintptr_t)mod;
    uintptr_t off = base != 0 ? (uintptr_t)addr - base : 0;
    if (base != 0) {
        GetModuleFileNameA(mod, mod_name, (DWORD)sizeof(mod_name));
    }
    ULONG_PTR rw = ep->ExceptionRecord->NumberParameters > 0
                       ? ep->ExceptionRecord->ExceptionInformation[0] : 0;
    ULONG_PTR target = ep->ExceptionRecord->NumberParameters > 1
                       ? ep->ExceptionRecord->ExceptionInformation[1] : 0;
    CONTEXT* c = ep->ContextRecord;
    fprintf(stderr,
            "[crash] code=0x%08lX rip=%p mod=%s off=+0x%llX %s target=0x%llX "
            "rcx=0x%llX rdx=0x%llX r8=0x%llX rsp=0x%llX\n",
            (unsigned long)code, addr, mod_name,
            (unsigned long long)off,
            code == 0xC0000005 ? (rw == 0 ? "READ" : (rw == 1 ? "WRITE" : "XEXEC")) : "-",
            (unsigned long long)target,
            (unsigned long long)c->Rcx, (unsigned long long)c->Rdx,
            (unsigned long long)c->R8, (unsigned long long)c->Rsp);
    /* 栈上返回地址候选：扫描 [rsp, rsp+0x200) 落在本模块映像内的 QWORD，
     * 打印其模块偏移——harness 侧用 PDB publics 符号化重建调用链。 */
    if (base != 0) {
        uintptr_t lo = base;
        uintptr_t hi = base + 0x4000000; /* 64MB 映像上界（粗略） */
        uintptr_t* sp = (uintptr_t*)c->Rsp;
        int shown = 0;
        for (int i = 0; i < 128 && shown < 8; i++) {
            uintptr_t v = sp[i];
            if (v > lo + 0x1000 && v < hi) {
                fprintf(stderr, "[crash] ret%d @rsp+%d = %p off=+0x%llX\n",
                        shown, i * 8, (void*)v, (unsigned long long)(v - base));
                shown++;
            }
        }
    }
    fflush(stderr);
    return EXCEPTION_CONTINUE_SEARCH;
}
#endif

/* ---- Phase 1：命令行参数 ---- */

int32_t rt_env_argc(void) {
    return (int32_t)g_argc;
}

const char* rt_env_argv(int32_t index) {
    if (index < 0 || index >= g_argc || !g_argv) {
        return "";
    }
    return g_argv[index] ? g_argv[index] : "";
}

/* ---- Phase 2：环境变量 ---- */

char* rt_env_get_var(const char* name) {
    if (!name || !*name) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
#ifdef _WIN32
    DWORD needed = GetEnvironmentVariableA(name, NULL, 0);
    if (needed == 0) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* buf = (char*)malloc(needed);
    if (!buf) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    DWORD written = GetEnvironmentVariableA(name, buf, needed);
    if (written == 0) {
        free(buf);
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    return buf;
#else
    const char* val = getenv(name);
    if (!val) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* copy = (char*)malloc(strlen(val) + 1);
    if (!copy) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    strcpy(copy, val);
    return copy;
#endif
}

int32_t rt_env_set_var(const char* name, const char* value) {
    if (!name || !*name) return 0;
#ifdef _WIN32
    BOOL ok = SetEnvironmentVariableA(name, value && *value ? value : NULL);
    return ok ? 1 : 0;
#else
    /* value=NULL 或空串视为删除 */
    if (value && *value) {
        return setenv(name, value, 1) == 0 ? 1 : 0;
    }
    return unsetenv(name) == 0 ? 1 : 0;
#endif
}

/* ---- Phase 2：进程控制 ---- */

void rt_env_exit(int32_t code) {
    /* H1: 用 _exit，跳过 atexit / CRT 静态析构。UnitTest 满套件报告完成后
     * 若再走 exit()→析构/free 风暴，与已损堆交织 → Summary 后 flaky 0xC0000005。
     * Environment.Exit 文档承诺「不返回、finally 不跑」——_exit 更贴语义。 */
#if defined(_WIN32)
    _exit((int)code);
#else
    _exit((int)code);
#endif
}

int32_t rt_env_get_exit_code(void) {
    return g_exit_code;
}

void rt_env_set_exit_code(int32_t code) {
    g_exit_code = code;
}

void rt_env_fail_fast(const char* msg) {
    if (msg) {
        fputs(msg, stderr);
        fputc('\n', stderr);
    }
    fflush(stderr);
    abort();
}

/* ---- Phase 2：系统信息 ---- */

const char* rt_env_newline(void) {
#ifdef _WIN32
    return "\r\n";
#else
    return "\n";
#endif
}

int32_t rt_env_processor_count(void) {
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    return (int32_t)si.dwNumberOfProcessors;
#else
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    return n > 0 ? (int32_t)n : 1;
#endif
}

int32_t rt_env_is_64bit_process(void) {
#if defined(_WIN64) || defined(__x86_64__) || defined(__aarch64__) || defined(__ppc64__) || defined(_ARCH_PPC64)
    return 1;
#else
    return 0;
#endif
}

/* ---- Phase 2：当前目录 ---- */

char* rt_env_get_cwd(void) {
#ifdef _WIN32
    DWORD needed = GetCurrentDirectoryA(0, NULL);
    if (needed == 0) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* buf = (char*)malloc(needed);
    if (!buf) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    DWORD written = GetCurrentDirectoryA(needed, buf);
    if (written == 0 || written >= needed) {
        free(buf);
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    return buf;
#else
    char* cwd = getcwd(NULL, 0);
    if (!cwd) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    return cwd;
#endif
}

int32_t rt_env_set_cwd(const char* path) {
    if (!path || !*path) return 0;
#ifdef _WIN32
    return SetCurrentDirectoryA(path) ? 1 : 0;
#else
    return chdir(path) == 0 ? 1 : 0;
#endif
}

/* ---- Phase 2：自身可执行文件路径（RFC 048 M1：跨进程 echo / 自 spawn 基建） ---- */

char* rt_env_self_exe(void) {
    char* empty = (char*)malloc(1);
    if (empty) empty[0] = '\0';
#ifdef _WIN32
    /* Windows 路径上限 32767 WCHAR：一次性足量缓冲，避免 MAX_PATH 截断长路径。 */
    WCHAR wbuf[32768];
    DWORD n = GetModuleFileNameW(NULL, wbuf, 32768);
    if (n == 0 || n >= 32768) {
        return empty;
    }
    int utf8_len = WideCharToMultiByte(CP_UTF8, 0, wbuf, (int)n, NULL, 0, NULL, NULL);
    if (utf8_len <= 0) {
        return empty;
    }
    char* buf = (char*)malloc((size_t)utf8_len + 1);
    if (!buf) {
        return empty;
    }
    WideCharToMultiByte(CP_UTF8, 0, wbuf, (int)n, buf, utf8_len, NULL, NULL);
    buf[utf8_len] = '\0';
    free(empty);
    return buf;
#elif defined(__APPLE__)
    uint32_t size = 0;
    if (_NSGetExecutablePath(NULL, &size) != -1 || size == 0) {
        return empty;
    }
    char* buf = (char*)malloc((size_t)size);
    if (!buf) {
        return empty;
    }
    if (_NSGetExecutablePath(buf, &size) != 0) {
        free(buf);
        return empty;
    }
    free(empty);
    return buf;
#else
    char link_target[4096];
    ssize_t n = readlink("/proc/self/exe", link_target, sizeof(link_target) - 1);
    if (n <= 0) {
        return empty;
    }
    link_target[n] = '\0';
    char* buf = (char*)malloc((size_t)n + 1);
    if (!buf) {
        return empty;
    }
    memcpy(buf, link_target, (size_t)n + 1);
    free(empty);
    return buf;
#endif
}

/* ---- Phase 2：机器名 / 用户名 ---- */

char* rt_env_machine_name(void) {
#ifdef _WIN32
    char buf[MAX_COMPUTERNAME_LENGTH + 1];
    DWORD size = sizeof(buf);
    if (!GetComputerNameA(buf, &size)) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* copy = (char*)malloc(strlen(buf) + 1);
    if (!copy) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    strcpy(copy, buf);
    return copy;
#else
    char buf[256];
    if (gethostname(buf, sizeof(buf)) != 0) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    buf[sizeof(buf) - 1] = '\0';
    char* copy = (char*)malloc(strlen(buf) + 1);
    if (!copy) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    strcpy(copy, buf);
    return copy;
#endif
}

char* rt_env_user_name(void) {
#ifdef _WIN32
    char buf[UNLEN + 1];
    DWORD size = sizeof(buf);
    if (!GetUserNameA(buf, &size)) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    char* copy = (char*)malloc(strlen(buf) + 1);
    if (!copy) {
        char* empty = (char*)malloc(1);
        if (empty) empty[0] = '\0';
        return empty;
    }
    strcpy(copy, buf);
    return copy;
#else
    const char* user = getenv("USER");
    if (!user) user = getenv("LOGNAME");
    if (!user) {
        char buf[256];
        if (getlogin_r(buf, sizeof(buf)) != 0) {
            char* empty = (char*)malloc(1);
            if (empty) empty[0] = '\0';
            return empty;
        }
        user = buf;
    }
    char* copy = (char*)malloc(strlen(user) + 1);
    if (copy) strcpy(copy, user);
    return copy;
#endif
}

/* ---- Phase 2：平台标识（编译期常量，静态返回） ---- */

const char* rt_env_platform(void) {
#ifdef _WIN32
    return "Windows";
#elif defined(__ANDROID__)
    return "Android";
#elif defined(__OHOS__)
    return "OHOS";
#elif defined(__APPLE__)
#if TARGET_OS_IPHONE
    return "iOS";
#else
    return "macOS";
#endif
#else
    return "Linux";
#endif
}

int32_t rt_env_is_windows(void) {
#ifdef _WIN32
    return 1;
#else
    return 0;
#endif
}

int32_t rt_env_is_linux(void) {
#if defined(_WIN32) || defined(__APPLE__) || defined(__ANDROID__) || defined(__OHOS__)
    return 0;
#else
    return 1;
#endif
}

int32_t rt_env_is_macos(void) {
#ifdef __APPLE__
#if TARGET_OS_IPHONE
    return 0;
#else
    return 1;
#endif
#else
    return 0;
#endif
}

int32_t rt_env_is_android(void) {
#ifdef __ANDROID__
    return 1;
#else
    return 0;
#endif
}

int32_t rt_env_is_ios(void) {
#ifdef __APPLE__
#if TARGET_OS_IPHONE
    return 1;
#else
    return 0;
#endif
#else
    return 0;
#endif
}

int32_t rt_env_is_ohos(void) {
#ifdef __OHOS__
    return 1;
#else
    return 0;
#endif
}
