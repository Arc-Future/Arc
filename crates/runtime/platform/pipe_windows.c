/* RFC 048 §3 / §5.2 / M2: Windows named pipe backend（经 rt_pipe.c 单 TU 合并）。
 * 语义：CreateNamedPipeW BYTE 模式 duplex + FILE_FLAG_OVERLAPPED（Reactor 真异步）；
 * 同步面经 OVERLAPPED + GetOverlappedResult 阻塞等待（重叠句柄不可传 NULL ov）；
 * ReadFile ERROR_BROKEN_PIPE → 0；WriteFile ERROR_BROKEN_PIPE/ERROR_NO_DATA → 0；
 * DisconnectNamedPipe 断开复用；缓冲默认 64KB（RFC 048 §3.1-2）。
 * 名字规范化：Arc 逻辑名 → `\\.\pipe\{name}`。
 * M2 异步：rt_pipe_wait_connect_async / read_async / write_async → IOCP。
 */
#include <stdio.h>
#include <windows.h>

typedef struct RtPipePlatform {
    HANDLE handle; /* server: 管道实例句柄；client: CreateFileW 文件句柄 */
} RtPipePlatform;

/* 同步阻塞等待重叠 IO（句柄以 FILE_FLAG_OVERLAPPED 打开）。 */
static int32_t rt_pipe_ov_wait(HANDLE h, OVERLAPPED* ov, DWORD* transferred) {
    if (!GetOverlappedResult(h, ov, transferred, TRUE)) {
        DWORD err = GetLastError();
        if (err == ERROR_BROKEN_PIPE || err == ERROR_NO_DATA || err == ERROR_HANDLE_EOF) {
            *transferred = 0;
            return 0;
        }
        return -1;
    }
    return 0;
}

static HANDLE rt_pipe_physical_name_windows(const char* name, char* out, int out_cap) {
    /* 名字规范化（RFC 048 §5.1-3）：`\\.\pipe\{name}`；物理名超长按失败处理。 */
    if (name == NULL || out == NULL || out_cap < 16) {
        return NULL;
    }
    int written = snprintf(out, (size_t)out_cap, "\\\\.\\pipe\\%s", name);
    if (written <= 0 || written >= out_cap - 1) {
        return NULL;
    }
    return out;
}

static void rt_pipe_platform_free(RtPipe* p) {
    if (p->platform != NULL) {
        free(p->platform);
        p->platform = NULL;
    }
}

static int32_t rt_pipe_handle_fd(RtPipe* p) {
    if (p == NULL || p->platform == NULL) {
        return -1;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return -1;
    }
    return (int32_t)(intptr_t)h;
}

void* rt_pipe_server_create(const char* name, int32_t max_instances) {
    char physical[MAX_PATH];
    if (rt_pipe_physical_name_windows(name, physical, MAX_PATH) == NULL) {
        return NULL;
    }
    RtPipe* p = (RtPipe*)rt_pipe_state_alloc(1, max_instances, name);
    if (p == NULL) {
        return NULL;
    }
    RtPipePlatform* plat = (RtPipePlatform*)malloc(sizeof(RtPipePlatform));
    if (plat == NULL) {
        rt_pipe_state_free(p);
        return NULL;
    }
    plat->handle = INVALID_HANDLE_VALUE;
    p->platform = plat;

    int n = MultiByteToWideChar(CP_UTF8, 0, physical, -1, NULL, 0);
    if (n <= 0) {
        rt_pipe_platform_free(p);
        rt_pipe_state_free(p);
        return NULL;
    }
    WCHAR* wide = (WCHAR*)malloc((size_t)n * sizeof(WCHAR));
    if (wide == NULL) {
        rt_pipe_platform_free(p);
        rt_pipe_state_free(p);
        return NULL;
    }
    MultiByteToWideChar(CP_UTF8, 0, physical, -1, wide, n);

    /* BYTE 模式 + OVERLAPPED（M2 IOCP）；缓冲 64KB（§3.1-2）。 */
    HANDLE h = CreateNamedPipeW(
        wide,
        PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        max_instances > 0 ? max_instances : 1,
        65536,
        65536,
        0,
        NULL); /* 默认 DACL：当前用户私有（RFC 048 §5.1-5） */
    free(wide);
    if (h == INVALID_HANDLE_VALUE) {
        rt_pipe_platform_free(p);
        rt_pipe_state_free(p);
        return NULL;
    }
    plat->handle = h;
    return p;
}

int32_t rt_pipe_server_wait_connect(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || !p->is_server || p->platform == NULL || p->closed) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    OVERLAPPED ov;
    memset(&ov, 0, sizeof(ov));
    ov.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (ov.hEvent == NULL) {
        return 0;
    }
    BOOL ok = ConnectNamedPipe(h, &ov);
    if (!ok) {
        DWORD err = GetLastError();
        if (err == ERROR_PIPE_CONNECTED) {
            CloseHandle(ov.hEvent);
            p->is_connected = 1;
            return 1;
        }
        if (err == ERROR_IO_PENDING) {
            DWORD got = 0;
            if (rt_pipe_ov_wait(h, &ov, &got) < 0) {
                CloseHandle(ov.hEvent);
                return 0;
            }
            CloseHandle(ov.hEvent);
            p->is_connected = 1;
            return 1;
        }
        CloseHandle(ov.hEvent);
        return 0;
    }
    CloseHandle(ov.hEvent);
    p->is_connected = 1;
    return 1;
}

void* rt_pipe_client_create(const char* name) {
    RtPipe* p = (RtPipe*)rt_pipe_state_alloc(0, 0, name);
    if (p == NULL) {
        return NULL;
    }
    RtPipePlatform* plat = (RtPipePlatform*)malloc(sizeof(RtPipePlatform));
    if (plat == NULL) {
        rt_pipe_state_free(p);
        return NULL;
    }
    plat->handle = INVALID_HANDLE_VALUE;
    p->platform = plat;
    return p;
}

int32_t rt_pipe_client_connect(void* handle, int32_t timeout_ms) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->is_server || p->platform == NULL || p->name == NULL || p->closed) {
        return 0;
    }
    char physical[MAX_PATH];
    if (rt_pipe_physical_name_windows(p->name, physical, MAX_PATH) == NULL) {
        return 0;
    }
    int n = MultiByteToWideChar(CP_UTF8, 0, physical, -1, NULL, 0);
    if (n <= 0) {
        return 0;
    }
    WCHAR* wide = (WCHAR*)malloc((size_t)n * sizeof(WCHAR));
    if (wide == NULL) {
        return 0;
    }
    MultiByteToWideChar(CP_UTF8, 0, physical, -1, wide, n);

    /* 接入循环：ERROR_PIPE_BUSY → WaitNamedPipeW 等待重试，直至 timeout（<0 = 无限）。 */
    DWORD start = GetTickCount();
    HANDLE h = INVALID_HANDLE_VALUE;
    for (;;) {
        h = CreateFileW(wide, GENERIC_READ | GENERIC_WRITE, 0, NULL,
                        OPEN_EXISTING, FILE_FLAG_OVERLAPPED, NULL);
        if (h != INVALID_HANDLE_VALUE) {
            break;
        }
        DWORD err = GetLastError();
        if (err != ERROR_PIPE_BUSY && err != ERROR_FILE_NOT_FOUND && err != ERROR_PATH_NOT_FOUND) {
            break;
        }
        if (err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND) {
            /* 服务端未建：轮询重试。 */
        } else {
            if (!WaitNamedPipeW(wide, 50)) {
                if (GetLastError() != ERROR_SEM_TIMEOUT) {
                    free(wide);
                    return 0;
                }
            }
        }
        if (timeout_ms >= 0) {
            DWORD elapsed = GetTickCount() - start;
            if (elapsed >= (DWORD)timeout_ms) {
                free(wide);
                return 0;
            }
        }
        Sleep(5);
    }
    free(wide);
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    ((RtPipePlatform*)p->platform)->handle = h;
    p->is_connected = 1;
    return 1;
}

int32_t rt_pipe_read(void* handle, void* buffer, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || buffer == NULL || length < 0 || p->closed) {
        return 0;
    }
    if (p->is_server && !p->is_connected) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    OVERLAPPED ov;
    memset(&ov, 0, sizeof(ov));
    ov.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
    if (ov.hEvent == NULL) {
        return 0;
    }
    DWORD got = 0;
    BOOL ok = ReadFile(h, buffer, (DWORD)length, NULL, &ov);
    if (!ok) {
        DWORD err = GetLastError();
        if (err == ERROR_IO_PENDING) {
            if (rt_pipe_ov_wait(h, &ov, &got) < 0) {
                CloseHandle(ov.hEvent);
                return 0;
            }
        } else if (err == ERROR_BROKEN_PIPE || err == ERROR_HANDLE_EOF) {
            CloseHandle(ov.hEvent);
            return 0;
        } else {
            CloseHandle(ov.hEvent);
            return 0;
        }
    } else {
        /* 立即完成：仍须 GetOverlappedResult 取字节数（部分驱动路径）。 */
        if (!GetOverlappedResult(h, &ov, &got, FALSE)) {
            got = 0;
        }
    }
    CloseHandle(ov.hEvent);
    return (int32_t)got;
}

int32_t rt_pipe_write(void* handle, const void* data, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || data == NULL || length <= 0 || p->closed) {
        return 0;
    }
    if (p->is_server && !p->is_connected) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    DWORD total = 0;
    while (total < (DWORD)length) {
        OVERLAPPED ov;
        memset(&ov, 0, sizeof(ov));
        ov.hEvent = CreateEventW(NULL, TRUE, FALSE, NULL);
        if (ov.hEvent == NULL) {
            return (int32_t)total;
        }
        DWORD sent = 0;
        BOOL ok = WriteFile(h, (const char*)data + total, (DWORD)length - total, NULL, &ov);
        if (!ok) {
            DWORD err = GetLastError();
            if (err == ERROR_IO_PENDING) {
                if (rt_pipe_ov_wait(h, &ov, &sent) < 0) {
                    CloseHandle(ov.hEvent);
                    return (int32_t)total;
                }
            } else if (err == ERROR_BROKEN_PIPE || err == ERROR_NO_DATA) {
                CloseHandle(ov.hEvent);
                return 0;
            } else {
                CloseHandle(ov.hEvent);
                return (int32_t)total;
            }
        } else {
            if (!GetOverlappedResult(h, &ov, &sent, FALSE)) {
                sent = 0;
            }
        }
        CloseHandle(ov.hEvent);
        if (sent == 0) {
            break;
        }
        total += sent;
    }
    return (int32_t)total;
}

int32_t rt_pipe_server_disconnect(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || !p->is_server || p->platform == NULL || p->closed) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    if (!DisconnectNamedPipe(h)) {
        return 0;
    }
    p->is_connected = 0;
    return 1;
}

int32_t rt_pipe_is_connected(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL) {
        return 0;
    }
    return p->is_connected;
}

void rt_pipe_close(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->closed) {
        return;
    }
    if (p->platform != NULL) {
        HANDLE h = ((RtPipePlatform*)p->platform)->handle;
        if (h != INVALID_HANDLE_VALUE) {
            FlushFileBuffers(h);
            CloseHandle(h);
        }
        ((RtPipePlatform*)p->platform)->handle = INVALID_HANDLE_VALUE;
    }
    p->closed = 1;
    p->is_connected = 0;
}

/* ─── RFC 048 M2：Reactor 真异步 ─────────────────────────────────────────── */

void* rt_pipe_wait_connect_async(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || !p->is_server || p->platform == NULL || p->closed) {
        return NULL;
    }
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        return NULL;
    }
    int32_t fd = rt_pipe_handle_fd(p);
    if (fd < 0) {
        return NULL;
    }
    RtTask* task = rt_task_alloc();
    if (!task) {
        return NULL;
    }
    task->status = RT_TASK_PENDING;
    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_PIPE_CONNECT;
    compl->buf = p; /* 完成时置 is_connected；不 free */
    rt_reactor_register(reactor, fd, 0);
    if (rt_reactor_submit_named_pipe_connect(reactor, fd, compl) != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_pipe_read_async(void* handle, void* buffer, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || buffer == NULL || length <= 0 || p->closed) {
        return NULL;
    }
    if (p->is_server && !p->is_connected) {
        return NULL;
    }
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        return NULL;
    }
    int32_t fd = rt_pipe_handle_fd(p);
    if (fd < 0) {
        return NULL;
    }
    RtTask* task = rt_task_alloc();
    if (!task) {
        return NULL;
    }
    task->status = RT_TASK_PENDING;
    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_READ_BYTES;
    compl->buf = buffer;
    compl->buf_size = length;
    rt_reactor_register(reactor, fd, 0);
    if (rt_reactor_submit_read(reactor, fd, buffer, (uint32_t)length, 0, compl) != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_pipe_write_async(void* handle, const void* data, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || data == NULL || length <= 0 || p->closed) {
        return NULL;
    }
    if (p->is_server && !p->is_connected) {
        return NULL;
    }
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) {
        return NULL;
    }
    int32_t fd = rt_pipe_handle_fd(p);
    if (fd < 0) {
        return NULL;
    }
    RtTask* task = rt_task_alloc();
    if (!task) {
        return NULL;
    }
    task->status = RT_TASK_PENDING;
    RtIoCompletion* compl = (RtIoCompletion*)calloc(1, sizeof(RtIoCompletion));
    if (!compl) {
        rt_task_release(task);
        return NULL;
    }
    compl->task = task;
    compl->op_type = RT_IO_OP_WRITE;
    /* buffer 归调用方；勿写入 compl->buf（完成路径会 free）。 */
    rt_reactor_register(reactor, fd, 0);
    if (rt_reactor_submit_write(reactor, fd, data, (uint32_t)length, 0, compl) != 0) {
        free(compl);
        rt_task_release(task);
        return NULL;
    }
    return task;
}

void* rt_pipe_client_connect_async(void* handle, int32_t timeoutMs) {
    /* M2 最小切片：客户端 Connect 仍可走同步 Connect + Task.FromResult 惯用法；
     * 真异步 WaitNamedPipe 轮询另排。此处用同步 connect 包装为已完成 Task。 */
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL) {
        return NULL;
    }
    RtTask* task = rt_task_alloc();
    if (!task) {
        return NULL;
    }
    int32_t ok = rt_pipe_client_connect(handle, timeoutMs);
    task->int_result = ok ? 1 : 0;
    rt_task_complete(task);
    return task;
}
