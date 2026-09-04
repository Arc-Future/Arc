/* RFC 048 §3 / §5.2: Windows named pipe backend（经 rt_pipe.c 单 TU 合并）。
 * 语义：CreateNamedPipeW BYTE 模式 duplex；ConnectNamedPipe 同步等待；
 * ReadFile ERROR_BROKEN_PIPE → 0；WriteFile ERROR_BROKEN_PIPE/ERROR_NO_DATA → 0；
 * DisconnectNamedPipe 断开复用；缓冲默认 64KB（RFC 048 §3.1-2）。
 * 名字规范化：Arc 逻辑名 → `\\.\pipe\{name}`。
 */
#include <stdio.h>
#include <windows.h>

typedef struct RtPipePlatform {
    HANDLE handle; /* server: 管道实例句柄；client: CreateFileW 文件句柄 */
} RtPipePlatform;

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

    /* BYTE 模式（无消息边界，§3 约定）；duplex；缓冲 64KB（§3.1-2）。 */
    HANDLE h = CreateNamedPipeW(
        wide,
        PIPE_ACCESS_DUPLEX,
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
    /* 已连接（ERROR_PIPE_CONNECTED）视作成功。 */
    if (ConnectNamedPipe(h, NULL)) {
        p->is_connected = 1;
        return 1;
    }
    if (GetLastError() == ERROR_PIPE_CONNECTED) {
        p->is_connected = 1;
        return 1;
    }
    return 0;
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
                        OPEN_EXISTING, 0, NULL);
        if (h != INVALID_HANDLE_VALUE) {
            break;
        }
        DWORD err = GetLastError();
        if (err != ERROR_PIPE_BUSY && err != ERROR_FILE_NOT_FOUND && err != ERROR_PATH_NOT_FOUND) {
            break;
        }
        if (err == ERROR_FILE_NOT_FOUND || err == ERROR_PATH_NOT_FOUND) {
            /* 服务端未建：轮询重试（WaitNamedPipeW 对不存在的名字立即失败）。 */
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
    ((RtPipePlatform*)p->platform)->handle = h;
    p->is_connected = 1;
    return 1;
}

int32_t rt_pipe_read(void* handle, void* buffer, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || buffer == NULL || length < 0 || p->closed) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    /* 字节流语义：单次 ReadFile，短读合法（返回已得字节）；0 = 对端有序关闭
     *（ERROR_BROKEN_PIPE）——不得循环读满（Stream.Read 为至多 count 语义，
     * 读满循环在无新数据时永久阻塞）。 */
    DWORD got = 0;
    if (!ReadFile(h, buffer, (DWORD)length, &got, NULL)) {
        return 0;
    }
    return (int32_t)got;
}

int32_t rt_pipe_write(void* handle, const void* data, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || data == NULL || length <= 0 || p->closed) {
        return 0;
    }
    HANDLE h = ((RtPipePlatform*)p->platform)->handle;
    if (h == INVALID_HANDLE_VALUE) {
        return 0;
    }
    DWORD total = 0;
    while (total < (DWORD)length) {
        DWORD sent = 0;
        if (!WriteFile(h, (const char*)data + total, (DWORD)length - total, &sent, NULL)) {
            DWORD err = GetLastError();
            /* 对端读端关闭 → 统一返回 0（RFC 048 §3 / §5.2）。 */
            if (err == ERROR_BROKEN_PIPE || err == ERROR_NO_DATA) {
                return 0;
            }
            return (int32_t)total;
        }
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
    /* 析构契约（RFC 048 M1 落定，见 rt_pipe.c 状态注释）：closed 置位使 close
     * 幂等、方法入口守卫安全返回；状态块不释放（泄漏至进程退出，与 Thread/
     * Socket 同策 H1）——close 即 free 会令 close 后仍被引用的门面对象 UAF。 */
    p->closed = 1;
    p->is_connected = 0;
}
