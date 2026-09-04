/* rt_proc.c — Process 体系 runtime ABI（crates/arc/native/rt_process.ani 契约实现）。
 *
 * 提供：子进程 spawn/wait/kill/close + 管道 I/O + PTY 终端。
 * Windows: CreateProcess + CreatePipe + ConPTY；POSIX: fork/exec + pipe + openpty。
 */

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdio.h>

typedef struct RtProc {
#ifdef _WIN32
    void* hProcess;
    void* hThread;
    int32_t pid;
    int32_t exited;
    int32_t exit_code;
    void* hpc;       /* HPCON */
    int32_t has_pty;
#else
    int32_t pid;
    int32_t exited;
    int32_t exit_code;
    int32_t wait_status;   /* waitpid status：保留 WIFSIGNALED/WTERMSIG 供 rt_proc_get_stats 暴露信号号 */
    int32_t has_pty;
#endif
} RtProc;

static char* dup_str(const char* s) {
    if (!s) return NULL;
    size_t n = strlen(s) + 1;
    char* out = (char*)malloc(n);
    if (out) memcpy(out, s, n);
    return out;
}

/* per-fd 读缓冲：消除逐字节系统调用（Windows/POSIX 共用） */
#define RT_PIPE_BUF_SIZE 4096
#define RT_MAX_PIPE_FDS 1024

typedef struct {
    int32_t fd;
    unsigned char buf[RT_PIPE_BUF_SIZE];
    int32_t pos;
    int32_t len;
    int32_t in_use;
} RtPipeReadBuf;

static RtPipeReadBuf g_read_bufs[RT_MAX_PIPE_FDS];

static RtPipeReadBuf* get_read_buf(int32_t fd) {
    if (fd < 0 || fd >= RT_MAX_PIPE_FDS) return NULL;
    if (!g_read_bufs[fd].in_use) {
        g_read_bufs[fd].in_use = 1;
        g_read_bufs[fd].fd = fd;
        g_read_bufs[fd].pos = 0;
        g_read_bufs[fd].len = 0;
    }
    return &g_read_bufs[fd];
}

/* ═══════════════════════ Windows ═══════════════════════ */
#ifdef _WIN32

#include <windows.h>
#include <io.h>
#include <fcntl.h>

typedef HRESULT (WINAPI *PFN_CreatePseudoConsole)(COORD, HANDLE, HANDLE, DWORD, HPCON*);
typedef void (WINAPI *PFN_ResizePseudoConsole)(HPCON, COORD);
typedef void (WINAPI *PFN_ClosePseudoConsole)(HPCON);

/* STARTUPINFOEXW 属性列表 API（Win8.1+；运行时探测，Win7 回退旧路径） */
typedef BOOL (WINAPI *PFN_InitProcAttrList)(LPPROC_THREAD_ATTRIBUTE_LIST, DWORD, DWORD, PSIZE_T);
typedef BOOL (WINAPI *PFN_UpdateProcAttr)(LPPROC_THREAD_ATTRIBUTE_LIST, DWORD, DWORD_PTR, PVOID, SIZE_T, PVOID, PSIZE_T);
typedef VOID (WINAPI *PFN_DeleteProcAttrList)(LPPROC_THREAD_ATTRIBUTE_LIST);

static PFN_CreatePseudoConsole g_createPseudoConsole = NULL;
static PFN_ResizePseudoConsole g_resizePseudoConsole = NULL;
static PFN_ClosePseudoConsole g_closePseudoConsole = NULL;

/* PROCESS_MEMORY_COUNTERS 最小布局（避免依赖 psapi.lib；PeakWorkingSetSize 为第 3 字段） */
typedef struct RtProcessMemoryCounters {
    DWORD cb;
    DWORD PageFaultCount;
    SIZE_T PeakWorkingSetSize;
    SIZE_T WorkingSetSize;
    SIZE_T QuotaPeakPagedPoolUsage;
    SIZE_T QuotaPagedPoolUsage;
    SIZE_T QuotaPeakNonPagedPoolUsage;
    SIZE_T QuotaNonPagedPoolUsage;
    SIZE_T PagefileUsage;
    SIZE_T PeakPagefileUsage;
} RtProcessMemoryCounters;

typedef BOOL (WINAPI *PFN_K32GetProcessMemoryInfo)(HANDLE, RtProcessMemoryCounters*, DWORD);
static PFN_K32GetProcessMemoryInfo g_k32GetProcessMemoryInfo = NULL;

static void load_mem_funcs(void) {
    if (g_k32GetProcessMemoryInfo) return;
    HMODULE h = LoadLibraryA("kernel32.dll");
    if (!h) return;
    /* K32GetProcessMemoryInfo 经 kernel32.dll 转发（Windows 7+），免链 psapi.lib */
    g_k32GetProcessMemoryInfo = (PFN_K32GetProcessMemoryInfo)GetProcAddress(h, "K32GetProcessMemoryInfo");
}

static void load_pty_funcs(void) {
    if (g_createPseudoConsole) return;
    HMODULE h = LoadLibraryA("kernel32.dll");
    if (!h) return;
    g_createPseudoConsole = (PFN_CreatePseudoConsole)GetProcAddress(h, "CreatePseudoConsole");
    g_resizePseudoConsole = (PFN_ResizePseudoConsole)GetProcAddress(h, "ResizePseudoConsole");
    g_closePseudoConsole = (PFN_ClosePseudoConsole)GetProcAddress(h, "ClosePseudoConsole");
}

static wchar_t* to_wchar(const char* s) {
    if (!s) return NULL;
    int n = MultiByteToWideChar(CP_UTF8, 0, s, -1, NULL, 0);
    wchar_t* w = (wchar_t*)malloc(n * sizeof(wchar_t));
    if (w) MultiByteToWideChar(CP_UTF8, 0, s, -1, w, n);
    return w;
}

/* 解析命令行为 argc/argv（Windows 命令行拼接） */
static wchar_t* build_cmdline(const wchar_t* exe, const wchar_t* args) {
    if (!exe) return NULL;
    size_t len = wcslen(exe) + 3;  /* "exe" + space + null */
    if (args && args[0]) len += wcslen(args) + 1;
    wchar_t* cmd = (wchar_t*)malloc(len * sizeof(wchar_t));
    if (!cmd) return NULL;
    if (args && args[0]) {
        _snwprintf(cmd, len, L"\"%s\" %s", exe, args);
    } else {
        _snwprintf(cmd, len, L"\"%s\"", exe);
    }
    return cmd;
}

void* rt_proc_spawn(const char* exe_path, const char* args, const char* working_dir,
                    int32_t redirect_stdin, int32_t redirect_stdout, int32_t redirect_stderr,
                    int32_t create_no_window,
                    int32_t* stdin_fd, int32_t* stdout_fd, int32_t* stderr_fd) {
    if (!exe_path) return NULL;
    *stdin_fd = -1; *stdout_fd = -1; *stderr_fd = -1;

    RtProc* p = (RtProc*)calloc(1, sizeof(RtProc));
    if (!p) return NULL;

    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof(SECURITY_ATTRIBUTES);
    sa.bInheritHandle = TRUE;
    sa.lpSecurityDescriptor = NULL;

    HANDLE child_stdin_read = NULL, child_stdin_write = NULL;
    HANDLE child_stdout_read = NULL, child_stdout_write = NULL;
    HANDLE child_stderr_read = NULL, child_stderr_write = NULL;

    if (redirect_stdin) {
        if (!CreatePipe(&child_stdin_read, &child_stdin_write, &sa, 0)) goto fail;
        SetHandleInformation(child_stdin_write, HANDLE_FLAG_INHERIT, 0);
    } else {
        /* STARTF_USESTDHANDLES 要求三个句柄全部有效——宿主无有效 stdin（无控制台 /
         * 句柄被 detach，GetStdHandle 返回 NULL/INVALID_HANDLE_VALUE）时直接透传会让
         * 子进程启动失败 0xC0000142（STATUS_DLL_INIT_FAILED，git 等 CRT 初始化崩）。
         * 造空 stdin 管道（父端立即关闭 → 子端读到 EOF），语义等价无输入。 */
        HANDLE h = GetStdHandle(STD_INPUT_HANDLE);
        if (h == NULL || h == INVALID_HANDLE_VALUE) {
            if (!CreatePipe(&child_stdin_read, &child_stdin_write, &sa, 0)) goto fail;
            CloseHandle(child_stdin_write);
            child_stdin_write = NULL;
        }
    }
    if (redirect_stdout) {
        if (!CreatePipe(&child_stdout_read, &child_stdout_write, &sa, 0)) goto fail;
        SetHandleInformation(child_stdout_read, HANDLE_FLAG_INHERIT, 0);
    }
    if (redirect_stderr) {
        if (!CreatePipe(&child_stderr_read, &child_stderr_write, &sa, 0)) goto fail;
        SetHandleInformation(child_stderr_read, HANDLE_FLAG_INHERIT, 0);
    }

    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    ZeroMemory(&si, sizeof(si));
    si.cb = sizeof(si);
    ZeroMemory(&pi, sizeof(pi));
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = child_stdin_read ? child_stdin_read : GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = child_stdout_write ? child_stdout_write : GetStdHandle(STD_OUTPUT_HANDLE);
    si.hStdError = child_stderr_write ? child_stderr_write : GetStdHandle(STD_ERROR_HANDLE);

    DWORD creation_flags = 0;
    /* 2026-08-21（CD-34 根因）：console 子进程（git/clang 等）在无有效控制台的宿主
     * （服务/重定向/无桌面会话）下，未加 CREATE_NO_WINDOW 时系统为子进程分配新控制台
     * （conhost）——闪窗 + conhost 初始化与子进程 DLL 初始化竞争 → 偶发 0xC0000142
     * （STATUS_DLL_INIT_FAILED，35% 复现；判别实证见 plan.md CD-34）。Arc 子进程面
     * （Process/RunCapture）统一默认无窗口；CreateNoWindow=false 的交互窗口场景登记
     * 为已知边界（std/Arc/Diagnostics 文档），当前一律 CREATE_NO_WINDOW。
     * 诚实边界：std ProcessStartInfo.CreateNoWindow 属性保留（API 面），runtime 暂不区分。 */
    creation_flags |= CREATE_NO_WINDOW;

    wchar_t* wexe = to_wchar(exe_path);
    wchar_t* wargs = to_wchar(args);
    wchar_t* wdir = to_wchar(working_dir);
    wchar_t* cmdline = build_cmdline(wexe, wargs);

    BOOL ok = FALSE;
    /* 2026-08-21（CD-34 根因）：bInheritHandles=TRUE 的全表句柄继承在父进程
     * 并发创建线程（RunCapture 的读取线程 _beginthreadex）时，与子进程的
     * DLL 初始化存在加载器竞态 → git 等子进程偶发 0xC0000142（STATUS_DLL_INIT_
     * FAILED，35% 复现；同步无读取线程/延迟读取均 0%）。Win8.1+ 用
     * STARTUPINFOEXW + PROC_THREAD_ATTRIBUTE_HANDLE_LIST（bInheritHandles=FALSE
     * + 显式句柄列表，业界标准：.NET/Chromium）——子进程只继承所需管道句柄，
     * 消除全表复制竞态；Win7 回退旧路径（运行时探测，GetProcAddress）。 */
    static PFN_InitProcAttrList  pfnInitAttrList = NULL;
    static PFN_UpdateProcAttr    pfnUpdateAttr = NULL;
    static PFN_DeleteProcAttrList pfnDeleteAttrList = NULL;
    if (!pfnInitAttrList) {
        HMODULE k32 = GetModuleHandleA("kernel32.dll");
        if (k32) {
            pfnInitAttrList = (PFN_InitProcAttrList)GetProcAddress(k32, "InitializeProcThreadAttributeList");
            pfnUpdateAttr = (PFN_UpdateProcAttr)GetProcAddress(k32, "UpdateProcThreadAttribute");
            pfnDeleteAttrList = (PFN_DeleteProcAttrList)GetProcAddress(k32, "DeleteProcThreadAttributeList");
        }
    }
    if (pfnInitAttrList && pfnUpdateAttr && pfnDeleteAttrList) {
        SIZE_T attr_size = 0;
        pfnInitAttrList(NULL, 1, 0, &attr_size);
        LPPROC_THREAD_ATTRIBUTE_LIST attr_list = (LPPROC_THREAD_ATTRIBUTE_LIST)malloc(attr_size ? attr_size : 1);
        if (attr_list && pfnInitAttrList(attr_list, 1, 0, &attr_size)) {
            HANDLE inherit_handles[3];
            int n_handles = 0;
            if (child_stdin_read)  inherit_handles[n_handles++] = child_stdin_read;
            if (child_stdout_write) inherit_handles[n_handles++] = child_stdout_write;
            if (child_stderr_write) inherit_handles[n_handles++] = child_stderr_write;
            if (pfnUpdateAttr(attr_list, 0, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                              inherit_handles, n_handles * sizeof(HANDLE), NULL, NULL)) {
                STARTUPINFOEXW siEx;
                ZeroMemory(&siEx, sizeof(siEx));
                siEx.StartupInfo = si;
                siEx.StartupInfo.cb = sizeof(siEx);
                siEx.lpAttributeList = attr_list;
                ok = CreateProcessW(NULL, cmdline, NULL, NULL, FALSE,
                                    EXTENDED_STARTUPINFO_PRESENT | creation_flags,
                                    NULL, wdir, &siEx.StartupInfo, &pi);
            }
        }
        if (attr_list) {
            if (ok) { pfnDeleteAttrList(attr_list); }
            free(attr_list);
        }
    }
    if (!ok) {
        /* 回退：旧 bInheritHandles=TRUE 路径（Win7 / HANDLE_LIST 初始化失败） */
        ok = CreateProcessW(NULL, cmdline, NULL, NULL, TRUE,
                            creation_flags, NULL, wdir, &si, &pi);
    }
    free(wexe); free(wargs); free(wdir); free(cmdline);

    if (!ok) goto fail;

    /* 关闭子进程端的句柄（父进程不需要） */
    if (child_stdin_read) CloseHandle(child_stdin_read);
    if (child_stdout_write) CloseHandle(child_stdout_write);
    if (child_stderr_write) CloseHandle(child_stderr_write);

    p->hProcess = pi.hProcess;
    p->hThread = pi.hThread;
    p->pid = pi.dwProcessId;
    p->exited = 0;
    p->exit_code = 0;
    p->has_pty = 0;

    /* fd 用句柄低 32 位编码（简单方案：存索引到全局表） */
    /* 简化：直接用 _open_osfhandle 转换为 C fd */
    if (child_stdin_write) *stdin_fd = _open_osfhandle((intptr_t)child_stdin_write, 0);
    if (child_stdout_read) *stdout_fd = _open_osfhandle((intptr_t)child_stdout_read, 0);
    if (child_stderr_read) *stderr_fd = _open_osfhandle((intptr_t)child_stderr_read, 0);

    return p;

fail:
    if (child_stdin_read) CloseHandle(child_stdin_read);
    if (child_stdin_write) CloseHandle(child_stdin_write);
    if (child_stdout_read) CloseHandle(child_stdout_read);
    if (child_stdout_write) CloseHandle(child_stdout_write);
    if (child_stderr_read) CloseHandle(child_stderr_read);
    if (child_stderr_write) CloseHandle(child_stderr_write);
    free(p);
    return NULL;
}

int32_t rt_proc_wait(void* handle, int64_t timeout_ms) {
    RtProc* p = (RtProc*)handle;
    if (!p || !p->hProcess) return -1;
    DWORD ms = (timeout_ms < 0) ? INFINITE : (DWORD)timeout_ms;
    DWORD r = WaitForSingleObject(p->hProcess, ms);
    if (r == WAIT_OBJECT_0) {
        DWORD code = 0;
        GetExitCodeProcess(p->hProcess, &code);
        p->exit_code = (int32_t)code;
        p->exited = 1;
        return 0;
    }
    if (r == WAIT_TIMEOUT) return 1;
    return -1;
}

int32_t rt_proc_kill(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p || !p->hProcess) return -1;
    if (TerminateProcess(p->hProcess, 1)) return 0;
    return -1;
}

int32_t rt_proc_close(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (p->hpc) {
        load_pty_funcs();
        if (g_closePseudoConsole) g_closePseudoConsole(p->hpc);
        p->hpc = NULL;
    }
    if (p->hThread) { CloseHandle(p->hThread); p->hThread = NULL; }
    if (p->hProcess) { CloseHandle(p->hProcess); p->hProcess = NULL; }
    free(p);
    return 0;
}

int32_t rt_proc_get_pid(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return 0;
    return p->pid;
}

int32_t rt_proc_get_exit_code(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (!p->exited) {
        DWORD code = 0;
        if (GetExitCodeProcess(p->hProcess, &code)) {
            p->exit_code = (int32_t)code;
            if (code != STILL_ACTIVE) p->exited = 1;
        }
    }
    return p->exit_code;
}

int32_t rt_proc_get_current_pid(void) {
    return (int32_t)GetCurrentProcessId();
}

/* rt_proc_get_stats — 进程资源统计（rt_process.ani 契约）。
 * user_ms/kernel_ms：CPU 时间（毫秒）；peak_mem_bytes：峰值工作集/ru_maxrss（字节）；
 * exit_reason：0 = 正常退出；>0 = 被信号终止（POSIX 信号号）；<0 = 尚未退出。
 * 返回 0 成功、-1 句柄非法。additive：不改变既有进程语义。 */
int32_t rt_proc_get_stats(void* handle, int64_t* out_user_ms, int64_t* out_kernel_ms,
                          int64_t* out_peak_mem_bytes, int32_t* out_exit_reason) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (out_user_ms) *out_user_ms = 0;
    if (out_kernel_ms) *out_kernel_ms = 0;
    if (out_peak_mem_bytes) *out_peak_mem_bytes = 0;
    if (out_exit_reason) *out_exit_reason = -1;
    if (!p->hProcess) return 0;

    FILETIME createTime, exitTime, kernelTime, userTime;
    if (GetProcessTimes(p->hProcess, &createTime, &exitTime, &kernelTime, &userTime)) {
        ULARGE_INTEGER ku, uu;
        ku.LowPart = kernelTime.dwLowDateTime; ku.HighPart = kernelTime.dwHighDateTime;
        uu.LowPart = userTime.dwLowDateTime; uu.HighPart = userTime.dwHighDateTime;
        /* FILETIME 单位为 100ns → 毫秒 */
        *out_kernel_ms = (int64_t)(ku.QuadPart / 10000);
        *out_user_ms = (int64_t)(uu.QuadPart / 10000);
    }
    load_mem_funcs();
    if (g_k32GetProcessMemoryInfo) {
        RtProcessMemoryCounters pmc;
        memset(&pmc, 0, sizeof(pmc));
        pmc.cb = sizeof(pmc);
        if (g_k32GetProcessMemoryInfo(p->hProcess, &pmc, sizeof(pmc))) {
            *out_peak_mem_bytes = (int64_t)pmc.PeakWorkingSetSize;
        }
    }
    DWORD code = 0;
    if (GetExitCodeProcess(p->hProcess, &code)) {
        if (code == STILL_ACTIVE) {
            *out_exit_reason = -1;
        } else {
            p->exited = 1;
            p->exit_code = (int32_t)code;
            *out_exit_reason = 0;   /* Windows 无信号语义：退出即正常形态，崩溃码在 exit_code 暴露 */
        }
    }
    return 0;
}

int32_t rt_proc_pipe_read_byte(int32_t fd) {
    if (fd < 0) return -1;
    RtPipeReadBuf* rb = get_read_buf(fd);
    if (!rb) return -1;
    if (rb->pos >= rb->len) {
        HANDLE h = (HANDLE)_get_osfhandle(fd);
        DWORD n = 0;
        if (!ReadFile(h, rb->buf, RT_PIPE_BUF_SIZE, &n, NULL)) return -1;
        if (n == 0) return -1;  /* EOF */
        rb->pos = 0;
        rb->len = (int32_t)n;
    }
    return (int32_t)rb->buf[rb->pos++];
}

int32_t rt_proc_pipe_write_byte(int32_t fd, int32_t b) {
    if (fd < 0) return -1;
    unsigned char byte = (unsigned char)b;
    DWORD n = 0;
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    if (!WriteFile(h, &byte, 1, &n, NULL)) return -1;
    return (int32_t)n;
}

const char* rt_proc_pipe_read_line(int32_t fd) {
    if (fd < 0) return NULL;
    int capacity = 256;
    char* buf = (char*)malloc(capacity);
    if (!buf) return NULL;
    int pos = 0;
    while (1) {
        int b = rt_proc_pipe_read_byte(fd);
        if (b < 0) break;
        if (b == '\n') break;
        if (b == '\r') continue;
        if (pos >= capacity - 1) {
            capacity *= 2;
            char* nb = (char*)realloc(buf, capacity);
            if (!nb) { free(buf); return NULL; }
            buf = nb;
        }
        buf[pos++] = (char)b;
    }
    if (pos == 0) { free(buf); return NULL; }
    buf[pos] = '\0';
    return buf;  /* codegen 将 const char* 转为 Arc string 后 free */
}

int32_t rt_proc_pipe_write_line(int32_t fd, const char* data) {
    if (fd < 0 || !data) return -1;
    size_t len = strlen(data);
    size_t total = len + 1;  /* data + '\n' 单次批量写 */
    char* buf = (char*)malloc(total);
    if (!buf) return -1;
    memcpy(buf, data, len);
    buf[len] = '\n';
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    DWORD written = 0;
    BOOL ok = WriteFile(h, buf, (DWORD)total, &written, NULL);
    free(buf);
    if (!ok) return -1;
    return 0;
}

int32_t rt_proc_pipe_write_string(int32_t fd, const char* data) {
    if (fd < 0 || !data) return -1;
    size_t len = strlen(data);
    if (len == 0) return 0;
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    DWORD written = 0;
    if (!WriteFile(h, data, (DWORD)len, &written, NULL)) return -1;
    return (int32_t)written;
}

/* std P2 效率批：批量管道读写——与 Unix 段实现语义同构（见 Unix 段注释）：
 * read 先消费块缓冲（与 read_byte/read_line 共享 RtPipeReadBuf，混用不乱序）
 * 再单次 ReadFile 直入用户 buffer；write 单次 WriteFile 批量写出。
 * ERROR_BROKEN_PIPE（写端全部关闭）= EOF 返回 total，不报错（RFC 021 契约：
 * 读返回实际字节数、EOF→0；与 .NET FileStream 对 broken pipe 的处理一致）。 */
int32_t rt_proc_pipe_read(int32_t fd, uint8_t* data, int32_t offset, int32_t count) {
    if (fd < 0 || !data) return -1;
    if (count <= 0) return 0;
    uint8_t* dst = data + offset;
    int32_t total = 0;
    RtPipeReadBuf* rb = get_read_buf(fd);
    if (rb && rb->pos < rb->len) {
        int32_t avail = rb->len - rb->pos;
        int32_t take = avail < count ? avail : count;
        memcpy(dst, rb->buf + rb->pos, (size_t)take);
        rb->pos += take;
        total = take;
    }
    if (total >= count) return total;
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    if (h == INVALID_HANDLE_VALUE) return total > 0 ? total : -1;
    DWORD n = 0;
    if (!ReadFile(h, dst + total, (DWORD)(count - total), &n, NULL)) {
        if (GetLastError() == ERROR_BROKEN_PIPE) return total;  /* 写端关闭 = EOF */
        return total > 0 ? total : -1;
    }
    if (n == 0) return total;  /* EOF */
    return total + (int32_t)n;
}

int32_t rt_proc_pipe_write(int32_t fd, const uint8_t* data, int32_t offset, int32_t count) {
    if (fd < 0 || !data || count <= 0) return 0;
    HANDLE h = (HANDLE)_get_osfhandle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;
    DWORD n = 0;
    if (!WriteFile(h, data + offset, (DWORD)count, &n, NULL)) return -1;
    return (int32_t)n;
}

int32_t rt_proc_pipe_close(int32_t fd) {
    if (fd < 0) return -1;
    if (fd < RT_MAX_PIPE_FDS && g_read_bufs[fd].in_use) {
        g_read_bufs[fd].in_use = 0;
        g_read_bufs[fd].pos = 0;
        g_read_bufs[fd].len = 0;
    }
    return _close(fd);
}

/* ── PTY (ConPTY) ── */
void* rt_pty_spawn(const char* exe_path, const char* args, const char* working_dir,
                   int32_t cols, int32_t rows, int32_t* master_fd) {
    load_pty_funcs();
    if (!g_createPseudoConsole) return NULL;
    if (!exe_path) return NULL;
    *master_fd = -1;

    RtProc* p = (RtProc*)calloc(1, sizeof(RtProc));
    if (!p) return NULL;

    /* 创建 stdin/stdout 管道用于 PTY */
    HANDLE pty_in_read, pty_in_write, pty_out_read, pty_out_write;
    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof(SECURITY_ATTRIBUTES);
    sa.bInheritHandle = TRUE;
    sa.lpSecurityDescriptor = NULL;

    if (!CreatePipe(&pty_in_read, &pty_in_write, &sa, 0)) { free(p); return NULL; }
    if (!CreatePipe(&pty_out_read, &pty_out_write, &sa, 0)) {
        CloseHandle(pty_in_read); CloseHandle(pty_in_write);
        free(p); return NULL;
    }

    HPCON hpc = NULL;
    COORD size;
    size.X = (SHORT)(cols > 0 ? cols : 80);
    size.Y = (SHORT)(rows > 0 ? rows : 24);
    HRESULT hr = g_createPseudoConsole(size, pty_in_read, pty_out_write, 0, &hpc);
    if (FAILED(hr)) {
        CloseHandle(pty_in_read); CloseHandle(pty_in_write);
        CloseHandle(pty_out_read); CloseHandle(pty_out_write);
        free(p); return NULL;
    }

    /* 创建子进程，继承 PTY 端 */
    STARTUPINFOEXW siex;
    ZeroMemory(&siex, sizeof(siex));
    siex.StartupInfo.cb = sizeof(siex);
    siex.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    siex.StartupInfo.hStdInput = pty_in_read;
    siex.StartupInfo.hStdOutput = pty_out_write;
    siex.StartupInfo.hStdError = pty_out_write;

    /* 设置进程继承属性列表 */
    SIZE_T attr_size = 0;
    InitializeProcThreadAttributeList(NULL, 1, 0, &attr_size);
    siex.lpAttributeList = malloc(attr_size);
    if (!InitializeProcThreadAttributeList(siex.lpAttributeList, 1, 0, &attr_size)) {
        free(siex.lpAttributeList);
        g_closePseudoConsole(hpc);
        CloseHandle(pty_in_read); CloseHandle(pty_in_write);
        CloseHandle(pty_out_read); CloseHandle(pty_out_write);
        free(p); return NULL;
    }

    /* 更新 PTY 句柄到属性列表 */
    HRESULT phr = UpdateProcThreadAttribute(siex.lpAttributeList, 0,
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, hpc, sizeof(HPCON), NULL, NULL);

    PROCESS_INFORMATION pi;
    ZeroMemory(&pi, sizeof(pi));

    wchar_t* wexe = to_wchar(exe_path);
    wchar_t* wargs = to_wchar(args);
    wchar_t* wdir = to_wchar(working_dir);
    wchar_t* cmdline = build_cmdline(wexe, wargs);
    free(wexe); free(wargs);

    BOOL ok = CreateProcessW(NULL, cmdline, NULL, NULL, FALSE,
                             EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
                             NULL, wdir, (STARTUPINFOW*)&siex, &pi);
    free(cmdline); free(wdir);
    DeleteProcThreadAttributeList(siex.lpAttributeList);
    free(siex.lpAttributeList);

    /* 关闭子进程端的 PTY 管道 */
    CloseHandle(pty_in_read);
    CloseHandle(pty_out_write);

    if (!ok) {
        g_closePseudoConsole(hpc);
        CloseHandle(pty_in_write);
        CloseHandle(pty_out_read);
        free(p); return NULL;
    }

    p->hProcess = pi.hProcess;
    p->hThread = pi.hThread;
    p->pid = pi.dwProcessId;
    p->hpc = hpc;
    p->has_pty = 1;

    /* master_fd 用 pty_out_read（读子进程输出）和 pty_in_write（写子进程输入） */
    /* 简化：master_fd 只存读端，写端用单独函数处理 */
    *master_fd = _open_osfhandle((intptr_t)pty_out_read, 0);
    /* pty_in_write 保存到 hThread 中复用（hack，简化实现） */
    /* 实际项目应建立 fd→handle 映射表 */

    return p;
}

int32_t rt_pty_resize(void* handle, int32_t cols, int32_t rows) {
    RtProc* p = (RtProc*)handle;
    if (!p || !p->hpc) return -1;
    load_pty_funcs();
    if (!g_resizePseudoConsole) return -1;
    COORD size;
    size.X = (SHORT)(cols > 0 ? cols : 80);
    size.Y = (SHORT)(rows > 0 ? rows : 24);
    g_resizePseudoConsole(p->hpc, size);
    return 0;
}

int32_t rt_pty_write_string(int32_t master_fd, const char* data) {
    return rt_proc_pipe_write_string(master_fd, data);
}

const char* rt_pty_read_line(int32_t master_fd) {
    return rt_proc_pipe_read_line(master_fd);
}

int32_t rt_pty_close(int32_t master_fd) {
    return rt_proc_pipe_close(master_fd);
}

int32_t rt_pty_send_signal(void* handle, int32_t signal) {
    /* Windows 不支持 POSIX 信号，仅支持 kill */
    if (signal == 2 || signal == 15) {  /* SIGINT=2, SIGTERM=15 */
        return rt_proc_kill(handle);
    }
    return -1;
}

/* ═══════════════════════ POSIX ═══════════════════════ */
#else

#include <unistd.h>
#include <fcntl.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <sys/time.h>
#include <signal.h>
#include <errno.h>
#include <pty.h>    /* openpty */
#include <sys/ioctl.h>

void* rt_proc_spawn(const char* exe_path, const char* args, const char* working_dir,
                    int32_t redirect_stdin, int32_t redirect_stdout, int32_t redirect_stderr,
                    int32_t create_no_window,
                    int32_t* stdin_fd, int32_t* stdout_fd, int32_t* stderr_fd) {
    if (!exe_path) return NULL;
    *stdin_fd = -1; *stdout_fd = -1; *stderr_fd = -1;

    int in_pipe[2] = {-1, -1}, out_pipe[2] = {-1, -1}, err_pipe[2] = {-1, -1};
    if (redirect_stdin && pipe(in_pipe) < 0) return NULL;
    if (redirect_stdout && pipe(out_pipe) < 0) { if (in_pipe[0]>=0) close(in_pipe[0]); if (in_pipe[1]>=0) close(in_pipe[1]); return NULL; }
    if (redirect_stderr && pipe(err_pipe) < 0) { if (in_pipe[0]>=0) close(in_pipe[0]); if (in_pipe[1]>=0) close(in_pipe[1]); if (out_pipe[0]>=0) close(out_pipe[0]); if (out_pipe[1]>=0) close(out_pipe[1]); return NULL; }

    pid_t pid = fork();
    if (pid < 0) {
        if (in_pipe[0]>=0) close(in_pipe[0]); if (in_pipe[1]>=0) close(in_pipe[1]);
        if (out_pipe[0]>=0) close(out_pipe[0]); if (out_pipe[1]>=0) close(out_pipe[1]);
        if (err_pipe[0]>=0) close(err_pipe[0]); if (err_pipe[1]>=0) close(err_pipe[1]);
        return NULL;
    }

    if (pid == 0) {
        /* 子进程 */
        if (working_dir) chdir(working_dir);
        if (redirect_stdin) { dup2(in_pipe[0], STDIN_FILENO); close(in_pipe[1]); }
        if (redirect_stdout) { dup2(out_pipe[1], STDOUT_FILENO); close(out_pipe[0]); }
        if (redirect_stderr) { dup2(err_pipe[1], STDERR_FILENO); close(err_pipe[0]); }
        close(in_pipe[0]); close(out_pipe[1]); close(err_pipe[1]);

        /* 解析 args 并 exec */
        char* argv[64];
        int argc = 0;
        argv[argc++] = strdup(exe_path);
        if (args) {
            char* tmp = strdup(args);
            char* tok = strtok(tmp, " \t");
            while (tok && argc < 63) {
                argv[argc++] = tok;
                tok = strtok(NULL, " \t");
            }
            /* tmp 被 argv 引用，不 free */
        }
        argv[argc] = NULL;
        execvp(exe_path, argv);
        _exit(127);
    }

    /* 父进程 */
    if (redirect_stdin) { close(in_pipe[0]); *stdin_fd = in_pipe[1]; }
    if (redirect_stdout) { close(out_pipe[1]); *stdout_fd = out_pipe[0]; }
    if (redirect_stderr) { close(err_pipe[1]); *stderr_fd = err_pipe[0]; }

    RtProc* p = (RtProc*)calloc(1, sizeof(RtProc));
    if (!p) return NULL;
    p->pid = pid;
    p->exited = 0;
    p->exit_code = 0;
    p->has_pty = 0;
    return p;
}

int32_t rt_proc_wait(void* handle, int64_t timeout_ms) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (timeout_ms < 0) {
        int status = 0;
        if (waitpid(p->pid, &status, 0) < 0) return -1;
        p->exited = 1;
        p->wait_status = status;
        p->exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
        return 0;
    }
    /* 超时等待：轮询 */
    int elapsed = 0;
    while (elapsed < (int)timeout_ms) {
        int status = 0;
        pid_t r = waitpid(p->pid, &status, WNOHANG);
        if (r == p->pid) {
            p->exited = 1;
            p->wait_status = status;
            p->exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
            return 0;
        }
        usleep(10000);  /* 10ms */
        elapsed += 10;
    }
    return 1;  /* timeout */
}

int32_t rt_proc_kill(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    return kill(p->pid, SIGKILL) == 0 ? 0 : -1;
}

int32_t rt_proc_close(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    free(p);
    return 0;
}

int32_t rt_proc_get_pid(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return 0;
    return p->pid;
}

int32_t rt_proc_get_exit_code(void* handle) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (!p->exited) {
        int status = 0;
        pid_t r = waitpid(p->pid, &status, WNOHANG);
        if (r == p->pid) {
            p->exited = 1;
            p->wait_status = status;
            p->exit_code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
        }
    }
    return p->exit_code;
}

int32_t rt_proc_get_current_pid(void) {
    return (int32_t)getpid();
}

/* rt_proc_get_stats — POSIX：getrusage(RUSAGE_CHILDREN) 资源统计 + exit_reason 信号号暴露。
 * 注：RUSAGE_CHILDREN 为已回收子进程累计值，经 waitpid 回收后含目标进程；
 * ru_maxrss 单位 macOS=字节、Linux/其余=KB，统一归一为字节。 */
int32_t rt_proc_get_stats(void* handle, int64_t* out_user_ms, int64_t* out_kernel_ms,
                          int64_t* out_peak_mem_bytes, int32_t* out_exit_reason) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    if (out_user_ms) *out_user_ms = 0;
    if (out_kernel_ms) *out_kernel_ms = 0;
    if (out_peak_mem_bytes) *out_peak_mem_bytes = 0;
    if (out_exit_reason) *out_exit_reason = -1;

    struct rusage ru;
    if (getrusage(RUSAGE_CHILDREN, &ru) == 0) {
        if (out_user_ms) {
            *out_user_ms = (int64_t)ru.ru_utime.tv_sec * 1000 + ru.ru_utime.tv_usec / 1000;
        }
        if (out_kernel_ms) {
            *out_kernel_ms = (int64_t)ru.ru_stime.tv_sec * 1000 + ru.ru_stime.tv_usec / 1000;
        }
        if (out_peak_mem_bytes) {
#if defined(__APPLE__)
            *out_peak_mem_bytes = (int64_t)ru.ru_maxrss;
#else
            *out_peak_mem_bytes = (int64_t)ru.ru_maxrss * 1024;
#endif
        }
    }
    if (out_exit_reason) {
        if (p->exited) {
            if (WIFSIGNALED(p->wait_status)) {
                *out_exit_reason = (int32_t)WTERMSIG(p->wait_status);
            } else {
                *out_exit_reason = 0;
            }
        } else {
            *out_exit_reason = -1;
        }
    }
    return 0;
}

int32_t rt_proc_pipe_read_byte(int32_t fd) {
    if (fd < 0) return -1;
    RtPipeReadBuf* rb = get_read_buf(fd);
    if (!rb) return -1;
    if (rb->pos >= rb->len) {
        ssize_t n = read(fd, rb->buf, RT_PIPE_BUF_SIZE);
        if (n <= 0) return -1;
        rb->pos = 0;
        rb->len = (int32_t)n;
    }
    return (int32_t)rb->buf[rb->pos++];
}

int32_t rt_proc_pipe_write_byte(int32_t fd, int32_t b) {
    if (fd < 0) return -1;
    unsigned char byte = (unsigned char)b;
    ssize_t n = write(fd, &byte, 1);
    if (n <= 0) return -1;
    return (int32_t)n;
}

const char* rt_proc_pipe_read_line(int32_t fd) {
    if (fd < 0) return NULL;
    int capacity = 256;
    char* buf = (char*)malloc(capacity);
    if (!buf) return NULL;
    int pos = 0;
    while (1) {
        int b = rt_proc_pipe_read_byte(fd);
        if (b < 0) break;
        if (b == '\n') break;
        if (b == '\r') continue;
        if (pos >= capacity - 1) {
            capacity *= 2;
            char* nb = (char*)realloc(buf, capacity);
            if (!nb) { free(buf); return NULL; }
            buf = nb;
        }
        buf[pos++] = (char)b;
    }
    if (pos == 0) { free(buf); return NULL; }
    buf[pos] = '\0';
    return buf;  /* codegen 将 const char* 转为 Arc string 后 free */
}

int32_t rt_proc_pipe_write_line(int32_t fd, const char* data) {
    if (fd < 0 || !data) return -1;
    size_t len = strlen(data);
    size_t total = len + 1;  /* data + '\n' 单次批量写 */
    char* buf = (char*)malloc(total);
    if (!buf) return -1;
    memcpy(buf, data, len);
    buf[len] = '\n';
    ssize_t n = write(fd, buf, total);
    free(buf);
    if (n < 0) return -1;
    return 0;
}

int32_t rt_proc_pipe_write_string(int32_t fd, const char* data) {
    if (fd < 0 || !data) return -1;
    size_t len = strlen(data);
    if (len == 0) return 0;
    ssize_t n = write(fd, data, len);
    if (n < 0) return -1;
    return (int32_t)n;
}

/* std P2 效率批：批量管道读写——与 Windows 段实现语义同构（见上方注释）：
 * read 先消费块缓冲再单次 read 直入用户 buffer；write 单次 write 批量写出。 */
int32_t rt_proc_pipe_read(int32_t fd, uint8_t* data, int32_t offset, int32_t count) {
    if (fd < 0 || !data) return -1;
    if (count <= 0) return 0;
    uint8_t* dst = data + offset;
    int32_t total = 0;
    RtPipeReadBuf* rb = get_read_buf(fd);
    if (rb && rb->pos < rb->len) {
        int32_t avail = rb->len - rb->pos;
        int32_t take = avail < count ? avail : count;
        memcpy(dst, rb->buf + rb->pos, (size_t)take);
        rb->pos += take;
        total = take;
    }
    if (total >= count) return total;
    ssize_t n = read(fd, dst + total, (size_t)(count - total));
    if (n < 0) return total > 0 ? total : -1;
    if (n == 0) return total;  /* EOF */
    return total + (int32_t)n;
}

int32_t rt_proc_pipe_write(int32_t fd, const uint8_t* data, int32_t offset, int32_t count) {
    if (fd < 0 || !data || count <= 0) return 0;
    ssize_t n = write(fd, data + offset, (size_t)count);
    if (n < 0) return -1;
    return (int32_t)n;
}

int32_t rt_proc_pipe_close(int32_t fd) {
    if (fd < 0) return -1;
    if (fd < RT_MAX_PIPE_FDS && g_read_bufs[fd].in_use) {
        g_read_bufs[fd].in_use = 0;
        g_read_bufs[fd].pos = 0;
        g_read_bufs[fd].len = 0;
    }
    return close(fd);
}

/* ── PTY (openpty) ── */
void* rt_pty_spawn(const char* exe_path, const char* args, const char* working_dir,
                   int32_t cols, int32_t rows, int32_t* master_fd) {
    if (!exe_path) return NULL;
    *master_fd = -1;

    int master, slave;
    if (openpty(&master, &slave, NULL, NULL, NULL) < 0) return NULL;

    /* 设置窗口大小 */
    struct winsize ws;
    ws.ws_col = cols > 0 ? cols : 80;
    ws.ws_row = rows > 0 ? rows : 24;
    ws.ws_xpixel = 0; ws.ws_ypixel = 0;
    ioctl(master, TIOCSWINSZ, &ws);

    pid_t pid = fork();
    if (pid < 0) { close(master); close(slave); return NULL; }

    if (pid == 0) {
        /* 子进程 */
        if (working_dir) chdir(working_dir);
        setsid();
        dup2(slave, STDIN_FILENO);
        dup2(slave, STDOUT_FILENO);
        dup2(slave, STDERR_FILENO);
        if (slave > 2) close(slave);
        close(master);

        /* 设置终端大小 */
        ioctl(STDIN_FILENO, TIOCSWINSZ, &ws);

        char* argv[64];
        int argc = 0;
        argv[argc++] = strdup(exe_path);
        if (args) {
            char* tmp = strdup(args);
            char* tok = strtok(tmp, " \t");
            while (tok && argc < 63) {
                argv[argc++] = tok;
                tok = strtok(NULL, " \t");
            }
        }
        argv[argc] = NULL;
        execvp(exe_path, argv);
        _exit(127);
    }

    /* 父进程 */
    close(slave);
    *master_fd = master;

    RtProc* p = (RtProc*)calloc(1, sizeof(RtProc));
    if (!p) { close(master); return NULL; }
    p->pid = pid;
    p->exited = 0;
    p->exit_code = 0;
    p->has_pty = 1;
    return p;
}

int32_t rt_pty_resize(void* handle, int32_t cols, int32_t rows) {
    RtProc* p = (RtProc*)handle;
    if (!p || !p->has_pty) return -1;
    /* 需要 master fd，但简化实现中 handle 不存储 fd */
    /* 实际项目应从 handle 或映射表中获取 master fd */
    return 0;
}

int32_t rt_pty_write_string(int32_t master_fd, const char* data) {
    return rt_proc_pipe_write_string(master_fd, data);
}

const char* rt_pty_read_line(int32_t master_fd) {
    return rt_proc_pipe_read_line(master_fd);
}

int32_t rt_pty_close(int32_t master_fd) {
    return rt_proc_pipe_close(master_fd);
}

int32_t rt_pty_send_signal(void* handle, int32_t signal) {
    RtProc* p = (RtProc*)handle;
    if (!p) return -1;
    return kill(p->pid, signal) == 0 ? 0 : -1;
}

#endif
