/* RFC 048 §3 / §5.2: POSIX FIFO backend（经 rt_pipe.c 单 TU 合并）。
 * 双工组装：`{物理名}.in`（client→server）+ `{物理名}.out`（server→client）双 FIFO；
 * 物理名 = `$XDG_RUNTIME_DIR`（回退 /tmp）`/arc-ipc-{sanitized}`（RFC 048 §5.1-3）。
 * 接入序列（无死锁）：server wait_connect = open(.in, RDONLY 阻塞) → open(.out, WRONLY 阻塞)；
 * client connect = open(.in, WRONLY|NONBLOCK 轮询) → open(.out, RDONLY)。
 * EOF/EPIPE：读 0 = 有序关闭；写 EPIPE → 0——SIGPIPE 全局 SIG_IGN（RFC 048 §3.1-1，
 * 进程级一次安装；runtime 全域自此对 SIGPIPE 免疫，socket 侧 MSG_NOSIGNAL 语义不变）。
 * 卫生：mkfifo 的 FIFO 由创建者（server 侧）在 close 时 unlink（§5.1-4）。
 */
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <time.h>

typedef struct RtPipePlatform {
    int read_fd;
    int write_fd;
    int owns_fifo_in;  /* unlink 责任：server 创建 .in */
    int owns_fifo_out; /* unlink 责任：server 创建 .out */
    char path_in[512];
    char path_out[512];
} RtPipePlatform;

/* SIGPIPE 进程级防护（RFC 048 §3.1-1）：首次创建管道时一次安装。 */
static void rt_pipe_sigpipe_guard(void) {
    static int installed = 0;
    if (!installed) {
        signal(SIGPIPE, SIG_IGN);
        installed = 1;
    }
}

static int rt_pipe_sanitize_name(const char* name, char* out, int out_cap) {
    if (name == NULL || out == NULL || out_cap <= 0) {
        return -1;
    }
    int w = 0;
    for (const char* s = name; *s != '\0'; s++) {
        char c = *s;
        int ok = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
                 (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-';
        if (w >= out_cap - 1) {
            return -1;
        }
        out[w++] = ok ? c : '-';
    }
    out[w] = '\0';
    if (w == 0) {
        return -1;
    }
    return w;
}

static int rt_pipe_physical_paths(const char* name, char* in_path, int cap_in, char* out_path, int cap_out) {
    char sanitized[256];
    if (rt_pipe_sanitize_name(name, sanitized, (int)sizeof(sanitized)) < 0) {
        return -1;
    }
    const char* base = getenv("XDG_RUNTIME_DIR");
    if (base == NULL || base[0] == '\0') {
        base = "/tmp";
    }
    int n1 = snprintf(in_path, (size_t)cap_in, "%s/arc-ipc-%s.in", base, sanitized);
    int n2 = snprintf(out_path, (size_t)cap_out, "%s/arc-ipc-%s.out", base, sanitized);
    if (n1 <= 0 || n1 >= cap_in || n2 <= 0 || n2 >= cap_out) {
        return -1;
    }
    return 0;
}

static void rt_pipe_platform_free(RtPipe* p) {
    if (p->platform != NULL) {
        RtPipePlatform* plat = (RtPipePlatform*)p->platform;
        if (plat->read_fd >= 0) {
            close(plat->read_fd);
        }
        if (plat->write_fd >= 0) {
            close(plat->write_fd);
        }
        free(p->platform);
        p->platform = NULL;
    }
}

void* rt_pipe_server_create(const char* name, int32_t max_instances) {
    (void)max_instances; /* POSIX：多实例为串行排队语义（RFC 048 §5.2），核心不分支 */
    rt_pipe_sigpipe_guard();
    RtPipe* p = (RtPipe*)rt_pipe_state_alloc(1, max_instances, name);
    if (p == NULL) {
        return NULL;
    }
    RtPipePlatform* plat = (RtPipePlatform*)malloc(sizeof(RtPipePlatform));
    if (plat == NULL) {
        rt_pipe_state_free(p);
        return NULL;
    }
    plat->read_fd = -1;
    plat->write_fd = -1;
    plat->owns_fifo_in = 0;
    plat->owns_fifo_out = 0;
    p->platform = plat;

    if (rt_pipe_physical_paths(p->name, plat->path_in, (int)sizeof(plat->path_in),
                               plat->path_out, (int)sizeof(plat->path_out)) != 0) {
        free(plat);
        p->platform = NULL;
        rt_pipe_state_free(p);
        return NULL;
    }
    /* 命名冲突 = 残骸接管自愈（§5.1-3 M1 修订）：POSIX FIFO 无内核生命周期，
     * 崩溃残留无法判定活跃性——EEXIST 时 unlink 旧文件重建（先到者成为孤儿，
     * 已有 fd 不受影响）。同名双 server 属未定义用法（不做清单），测试批锁定
     * 「Terminate 后同名重建」自愈路径。 */
    if (mkfifo(plat->path_in, 0600) != 0) {
        if (errno != EEXIST || unlink(plat->path_in) != 0 ||
            mkfifo(plat->path_in, 0600) != 0) {
            free(plat);
            p->platform = NULL;
            rt_pipe_state_free(p);
            return NULL;
        }
    }
    plat->owns_fifo_in = 1;
    if (mkfifo(plat->path_out, 0600) != 0) {
        if (errno != EEXIST || unlink(plat->path_out) != 0 ||
            mkfifo(plat->path_out, 0600) != 0) {
            unlink(plat->path_in);
            free(plat);
            p->platform = NULL;
            rt_pipe_state_free(p);
            return NULL;
        }
    }
    plat->owns_fifo_out = 1;
    return p;
}

int32_t rt_pipe_server_wait_connect(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || !p->is_server || p->platform == NULL || p->closed) {
        return 0;
    }
    RtPipePlatform* plat = (RtPipePlatform*)p->platform;
    if (plat->read_fd >= 0) {
        close(plat->read_fd);
        plat->read_fd = -1;
    }
    if (plat->write_fd >= 0) {
        close(plat->write_fd);
        plat->write_fd = -1;
    }
    /* 序列一：阻塞至 client 打开 .in 写端。 */
    int rfd = open(plat->path_in, O_RDONLY);
    if (rfd < 0) {
        return 0;
    }
    /* 序列二：阻塞至 client 打开 .out 读端。 */
    int wfd = open(plat->path_out, O_WRONLY);
    if (wfd < 0) {
        close(rfd);
        return 0;
    }
    plat->read_fd = rfd;
    plat->write_fd = wfd;
    p->is_connected = 1;
    return 1;
}

void* rt_pipe_client_create(const char* name) {
    rt_pipe_sigpipe_guard();
    RtPipe* p = (RtPipe*)rt_pipe_state_alloc(0, 0, name);
    if (p == NULL) {
        return NULL;
    }
    RtPipePlatform* plat = (RtPipePlatform*)malloc(sizeof(RtPipePlatform));
    if (plat == NULL) {
        rt_pipe_state_free(p);
        return NULL;
    }
    plat->read_fd = -1;
    plat->write_fd = -1;
    plat->owns_fifo_in = 0;
    plat->owns_fifo_out = 0;
    plat->path_in[0] = '\0';
    plat->path_out[0] = '\0';
    p->platform = plat;
    return p;
}

int32_t rt_pipe_client_connect(void* handle, int32_t timeout_ms) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->is_server || p->platform == NULL || p->name == NULL || p->closed) {
        return 0;
    }
    RtPipePlatform* plat = (RtPipePlatform*)p->platform;
    char path_in[512];
    char path_out[512];
    if (rt_pipe_physical_paths(p->name, path_in, (int)sizeof(path_in),
                               path_out, (int)sizeof(path_out)) != 0) {
        return 0;
    }
    struct timespec ts = {0, 5 * 1000 * 1000}; /* 5ms 轮询步 */
    long deadline_ms = timeout_ms < 0 ? -1 : (long)timeout_ms;
    long elapsed_ms = 0;

    int wfd = -1;
    for (;;) {
        /* 非阻塞探测：ENXIO = 读端未就绪（服务端未 wait_connect）。 */
        wfd = open(path_in, O_WRONLY | O_NONBLOCK);
        if (wfd >= 0) {
            break;
        }
        if (errno != ENXIO && errno != ENOENT) {
            return 0;
        }
        if (deadline_ms >= 0 && elapsed_ms >= deadline_ms) {
            return 0;
        }
        nanosleep(&ts, NULL);
        elapsed_ms += 5;
    }
    /* 恢复阻塞语义（后续 write 走正常流控背压）。 */
    int fl = fcntl(wfd, F_GETFL, 0);
    if (fl >= 0) {
        fcntl(wfd, F_SETFL, fl & ~O_NONBLOCK);
    }
    /* .out 读端：RDONLY|NONBLOCK 立即成功（无需服务端写端先行）。 */
    int rfd = open(path_out, O_RDONLY | O_NONBLOCK);
    if (rfd < 0) {
        close(wfd);
        return 0;
    }
    fl = fcntl(rfd, F_GETFL, 0);
    if (fl >= 0) {
        fcntl(rfd, F_SETFL, fl & ~O_NONBLOCK);
    }
    plat->write_fd = wfd; /* client 写 → server 读（.in） */
    plat->read_fd = rfd;  /* client 读 ← server 写（.out） */
    p->is_connected = 1;
    return 1;
}

int32_t rt_pipe_read(void* handle, void* buffer, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || buffer == NULL || length < 0 || p->closed) {
        return 0;
    }
    int fd = ((RtPipePlatform*)p->platform)->read_fd;
    if (fd < 0) {
        return 0;
    }
    /* 字节流语义：单次 read(2)，短读合法（返回已得字节）；0 = 对端写端全部
     * 关闭（EOF）——不得循环读满（Stream.Read 为至多 count 语义）。 */
    for (;;) {
        ssize_t got = read(fd, buffer, (size_t)length);
        if (got >= 0) {
            return (int32_t)got;
        }
        if (errno != EINTR) {
            return 0;
        }
    }
}

int32_t rt_pipe_write(void* handle, const void* data, int32_t length) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || data == NULL || length <= 0 || p->closed) {
        return 0;
    }
    int fd = ((RtPipePlatform*)p->platform)->write_fd;
    if (fd < 0) {
        return 0;
    }
    int32_t total = 0;
    while (total < length) {
        ssize_t sent = write(fd, (const char*)data + total, (size_t)(length - total));
        if (sent > 0) {
            total += (int32_t)sent;
            continue;
        }
        if (sent < 0 && errno == EINTR) {
            continue;
        }
        /* EPIPE（对端读端关闭，SIGPIPE 已被 §3.1-1 抑制）→ 统一返回 0。 */
        return total == 0 ? 0 : total;
    }
    return total;
}

int32_t rt_pipe_server_disconnect(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || !p->is_server || p->platform == NULL || p->closed) {
        return 0;
    }
    RtPipePlatform* plat = (RtPipePlatform*)p->platform;
    /* 断开复用（对齐 Windows DisconnectNamedPipe）：仅关闭两端（client 侧感知
     * EOF），FIFO 文件**保留**——下次 wait_connect 直接 open 即可建立新连接对。
     * M1 修正：旧实现 disconnect 时 unlink+mkfifo 换 inode，会把已排队/重连的
     * client 顶到新 inode 上造成连接撕裂。 */
    if (plat->read_fd >= 0) {
        close(plat->read_fd);
        plat->read_fd = -1;
    }
    if (plat->write_fd >= 0) {
        close(plat->write_fd);
        plat->write_fd = -1;
    }
    p->is_connected = 0;
    return 1;
}

int32_t rt_pipe_is_connected(void* handle) {
    RtPipe* p = (RtPipe*)handle;
    if (p == NULL || p->platform == NULL || p->closed) {
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
        RtPipePlatform* plat = (RtPipePlatform*)p->platform;
        if (plat->read_fd >= 0) {
            close(plat->read_fd);
            plat->read_fd = -1;
        }
        if (plat->write_fd >= 0) {
            close(plat->write_fd);
            plat->write_fd = -1;
        }
        /* 创建者卫生（§5.1-4）：server 退出/close 时移除 FIFO 文件。 */
        if (p->is_server) {
            if (plat->owns_fifo_in && plat->path_in[0] != '\0') {
                unlink(plat->path_in);
            }
            if (plat->owns_fifo_out && plat->path_out[0] != '\0') {
                unlink(plat->path_out);
            }
        }
    }
    /* 析构契约（RFC 048 M1 落定，见 rt_pipe.c 状态注释）：closed 置位使 close
     * 幂等、方法入口守卫安全返回；状态块不释放（泄漏至进程退出，与 Thread/
     * Socket 同策 H1）。 */
    p->closed = 1;
    p->is_connected = 0;
}
