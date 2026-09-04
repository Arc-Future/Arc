// 跨平台 Socket 原语（RFC 009 M2）。
//
// 提供 socket 创建/绑定/监听/连接/关闭等同步原语，作为 Reactor 异步 IO 的基础。
// Reactor 的 submit_accept / submit_connect / submit_read / submit_write 在此基础上
// 实现异步语义（IOCP/io_uring/kqueue 完成后触发 waker）。
//
// 平台差异：
//   - Windows：WinSock2（WSAStartup 初始化 + WSASocket/socket/bind/listen/connect/closesocket）
//   - POSIX：sys/socket.h / netinet/in.h / arpa/inet.h / netdb.h
//
// 设计：
//   - 返回值统一为 int32_t fd（>0 成功，<0 失败，-errno 风格）
//   - 阻塞模式默认；rt_socket_set_nonblocking 切换为非阻塞（Reactor 必需）
//   - SO_REUSEADDR 默认开启（服务器快速重启）
//   - WSAStartup 引用计数（多次调用安全）

#include "rt_abi.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- 平台抽象 ---- */

#if defined(_WIN32) || defined(_WIN64)
  #include <winsock2.h>
  #include <ws2tcpip.h>
  #include <windows.h>
  #pragma comment(lib, "ws2_32.lib")

  typedef int socklen_t_win;
  #define RT_SOCKET_INVALID INVALID_SOCKET
  #define RT_SOCKET_ERROR   SOCKET_ERROR
  #define RT_SOCKET_CLOSE   closesocket
  #define RT_SOCKET_ERRNO   WSAGetLastError()

  /* WSAStartup 引用计数 */
  static LONG g_wsa_init_count = 0;
  static CRITICAL_SECTION g_wsa_lock;
  static int g_wsa_lock_initialized = 0;

  static void rt_socket_ensure_wsa(void) {
      if (!g_wsa_lock_initialized) {
          InitializeCriticalSection(&g_wsa_lock);
          g_wsa_lock_initialized = 1;
      }
      EnterCriticalSection(&g_wsa_lock);
      if (g_wsa_init_count == 0) {
          WSADATA wsaData;
          WSAStartup(MAKEWORD(2, 2), &wsaData);
      }
      g_wsa_init_count++;
      LeaveCriticalSection(&g_wsa_lock);
  }

  static void rt_socket_cleanup_wsa(void) {
      if (!g_wsa_lock_initialized) return;
      EnterCriticalSection(&g_wsa_lock);
      if (g_wsa_init_count > 0) {
          g_wsa_init_count--;
          if (g_wsa_init_count == 0) {
              WSACleanup();
          }
      }
      LeaveCriticalSection(&g_wsa_lock);
  }

  /* Windows fd 转换：SOCKET 是 UINT_PTR，强转 int32_t 可能丢失高位。
   * 实际场景 SOCKET 句柄值通常 < 2^31，可直接强转。 */
  static inline int32_t rt_socket_to_fd(SOCKET s) {
      return (int32_t)(intptr_t)s;
  }
  static inline SOCKET rt_fd_to_socket(int32_t fd) {
      return (SOCKET)(intptr_t)fd;
  }

#else
  #include <unistd.h>
  #include <sys/socket.h>
  #include <sys/types.h>
  #include <netinet/in.h>
  #include <netinet/tcp.h>
  #include <arpa/inet.h>
  #include <netdb.h>
  #include <fcntl.h>
  #include <errno.h>

  #define RT_SOCKET_INVALID (-1)
  #define RT_SOCKET_ERROR   (-1)
  #define RT_SOCKET_CLOSE   close
  #define RT_SOCKET_ERRNO   errno

  static inline int32_t rt_socket_to_fd(int s) {
      return s;
  }
  static inline int rt_fd_to_socket(int32_t fd) {
      return fd;
  }

  static void rt_socket_ensure_wsa(void) { /* POSIX no-op */ }
  static void rt_socket_cleanup_wsa(void) { /* POSIX no-op */ }
#endif

/* ---- Socket ABI 实现 ---- */

/* 创建 socket：family(0=IPv4, 1=IPv6) / type(0=Stream, 1=Dgram) / proto(0=TCP, 1=UDP)
 * 返回 fd (>0) 或负 errno。 */
int32_t rt_net_create(int32_t family, int32_t type, int32_t proto) {
    rt_socket_ensure_wsa();

    int af = (family == 1) ? AF_INET6 : AF_INET;
    int st = (type == 1) ? SOCK_DGRAM : SOCK_STREAM;
    int pr = (proto == 1) ? IPPROTO_UDP : IPPROTO_TCP;

#if defined(_WIN32) || defined(_WIN64)
    SOCKET s = socket(af, st, pr);
    if (s == INVALID_SOCKET) {
        return -(int32_t)RT_SOCKET_ERRNO;
    }
    /* 默认开启 SO_REUSEADDR（服务器快速重启） */
    BOOL reuse = TRUE;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, (const char*)&reuse, sizeof(reuse));
    return rt_socket_to_fd(s);
#else
    int s = socket(af, st, pr);
    if (s < 0) {
        return -errno;
    }
    int reuse = 1;
    setsockopt(s, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse));
    return s;
#endif
}

/* 绑定到本地端口。family: 0=IPv4, 1=IPv6。port: 端口号。
 * 返回 0 成功，<0 失败。 */
int32_t rt_net_bind(int32_t fd, int32_t port, int32_t family) {
    if (fd < 0) return -1;

#if defined(_WIN32) || defined(_WIN64)
    SOCKET s = rt_fd_to_socket(fd);
    if (family == 1) {
        struct sockaddr_in6 addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin6_family = AF_INET6;
        addr.sin6_port = htons((u_short)port);
        addr.sin6_addr = in6addr_any;
        if (bind(s, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR) {
            return -(int32_t)WSAGetLastError();
        }
    } else {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons((u_short)port);
        addr.sin_addr.s_addr = htonl(INADDR_ANY);
        if (bind(s, (struct sockaddr*)&addr, sizeof(addr)) == SOCKET_ERROR) {
            return -(int32_t)WSAGetLastError();
        }
    }
    return 0;
#else
    int s = fd;
    if (family == 1) {
        struct sockaddr_in6 addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin6_family = AF_INET6;
        addr.sin6_port = htons((uint16_t)port);
        addr.sin6_addr = in6addr_any;
        if (bind(s, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
            return -errno;
        }
    } else {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons((uint16_t)port);
        addr.sin_addr.s_addr = htonl(INADDR_ANY);
        if (bind(s, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
            return -errno;
        }
    }
    return 0;
#endif
}

/* 开始监听。backlog: 等待队列最大长度。
 * 返回 0 成功，<0 失败。 */
int32_t rt_net_listen(int32_t fd, int32_t backlog) {
    if (fd < 0) return -1;
    if (backlog < 1) backlog = 128;

#if defined(_WIN32) || defined(_WIN64)
    SOCKET s = rt_fd_to_socket(fd);
    if (listen(s, backlog) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    if (listen(fd, backlog) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 同步连接到远程主机。host: 主机名或 IP 字符串。port: 端口号。
 * 返回 0 成功，<0 失败。
 * 注意：这是阻塞连接，Reactor 的 submit_connect 提供异步版本。 */
int32_t rt_net_connect(int32_t fd, const char* host, int32_t port) {
    if (fd < 0 || !host) return -1;

    /* getaddrinfo 解析主机名（支持 IPv4/IPv6/DNS） */
    struct addrinfo hints;
    struct addrinfo* res = NULL;
    memset(&hints, 0, sizeof(hints));
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    char port_str[16];
    /* snprintf 在 MSVC 上有 _snprintf 警告，但 C11 标准可用 */
    snprintf(port_str, sizeof(port_str), "%d", port);

    int gai_ret = getaddrinfo(host, port_str, &hints, &res);
    if (gai_ret != 0 || !res) {
#if defined(_WIN32) || defined(_WIN64)
        return -(int32_t)WSAGetLastError();
#else
        return -ECONNREFUSED;
#endif
    }

    int32_t result = 0;
#if defined(_WIN32) || defined(_WIN64)
    SOCKET s = rt_fd_to_socket(fd);
    if (connect(s, res->ai_addr, (int)res->ai_addrlen) == SOCKET_ERROR) {
        result = -(int32_t)WSAGetLastError();
    }
#else
    if (connect(fd, res->ai_addr, res->ai_addrlen) < 0) {
        result = -errno;
    }
#endif
    freeaddrinfo(res);
    return result;
}

/* 同步接受连接。返回新 fd (>0) 或负 errno。
 * 注意：阻塞接受；Reactor 的 submit_accept 提供异步版本。
 *
 * Windows：accept 返回的新 SOCKET 与 listener 共享 WSA 引用计数语义，
 * 但 rt_net_close 会对每个 fd 调用 rt_socket_cleanup_wsa（计数递减）。
 * 若 accept 不 increment，close(accepted) 会让计数提前归零触发 WSACleanup，
 * 导致后续 close(listener) 失败（WSA 已清理）。此处 ensure_wsa 平衡计数。 */
int32_t rt_net_accept(int32_t fd) {
    if (fd < 0) return -1;

#if defined(_WIN32) || defined(_WIN64)
    rt_socket_ensure_wsa();  /* 平衡后续 rt_net_close 的 cleanup_wsa */
    SOCKET s = rt_fd_to_socket(fd);
    SOCKET client = accept(s, NULL, NULL);
    if (client == INVALID_SOCKET) {
        rt_socket_cleanup_wsa();  /* 失败时回滚 increment */
        return -(int32_t)WSAGetLastError();
    }
    return rt_socket_to_fd(client);
#else
    int client = accept(fd, NULL, NULL);
    if (client < 0) {
        return -errno;
    }
    return client;
#endif
}

/* 设置非阻塞模式（Reactor 必须）。
 * 返回 0 成功，<0 失败。 */
int32_t rt_net_set_nonblocking(int32_t fd) {
    if (fd < 0) return -1;

#if defined(_WIN32) || defined(_WIN64)
    SOCKET s = rt_fd_to_socket(fd);
    u_long mode = 1;
    if (ioctlsocket(s, FIONBIO, &mode) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0) return -errno;
    if (fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 设置 SO_REUSEADDR。
 * 返回 0 成功，<0 失败。 */
int32_t rt_net_set_reuse_addr(int32_t fd) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    BOOL reuse = TRUE;
    if (setsockopt(rt_fd_to_socket(fd), SOL_SOCKET, SO_REUSEADDR,
                   (const char*)&reuse, sizeof(reuse)) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    int reuse = 1;
    if (setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 设置 TCP_NODELAY（禁用 Nagle）。
 * 返回 0 成功，<0 失败。 */
int32_t rt_net_set_no_delay(int32_t fd, int32_t enabled) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    BOOL val = enabled ? TRUE : FALSE;
    if (setsockopt(rt_fd_to_socket(fd), IPPROTO_TCP, TCP_NODELAY,
                   (const char*)&val, sizeof(val)) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    int val = enabled ? 1 : 0;
    if (setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &val, sizeof(val)) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 设置发送缓冲区大小。 */
int32_t rt_net_set_send_buf_size(int32_t fd, int32_t size) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    int val = size;
    if (setsockopt(rt_fd_to_socket(fd), SOL_SOCKET, SO_SNDBUF,
                   (const char*)&val, sizeof(val)) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    int val = size;
    if (setsockopt(fd, SOL_SOCKET, SO_SNDBUF, &val, sizeof(val)) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 设置接收缓冲区大小。 */
int32_t rt_net_set_recv_buf_size(int32_t fd, int32_t size) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    int val = size;
    if (setsockopt(rt_fd_to_socket(fd), SOL_SOCKET, SO_RCVBUF,
                   (const char*)&val, sizeof(val)) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
#else
    int val = size;
    if (setsockopt(fd, SOL_SOCKET, SO_RCVBUF, &val, sizeof(val)) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 关闭 socket。 */
int32_t rt_net_close(int32_t fd) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    if (closesocket(rt_fd_to_socket(fd)) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
    rt_socket_cleanup_wsa();
#else
    if (close(fd) < 0) {
        return -errno;
    }
#endif
    return 0;
}

/* 查询 socket 是否已连接（通过 getpeername 判断）。
 * 返回 1=已连接，0=未连接。 */
int32_t rt_net_connected(int32_t fd) {
    if (fd < 0) return 0;
#if defined(_WIN32) || defined(_WIN64)
    struct sockaddr_storage addr;
    int len = sizeof(addr);
    if (getpeername(rt_fd_to_socket(fd), (struct sockaddr*)&addr, &len) == SOCKET_ERROR) {
        return 0;
    }
    return 1;
#else
    struct sockaddr_storage addr;
    socklen_t len = sizeof(addr);
    if (getpeername(fd, (struct sockaddr*)&addr, &len) < 0) {
        return 0;
    }
    return 1;
#endif
}

/* 查询可读取字节数（FIONREAD）。
 * 返回字节数（>=0）或负 errno。 */
int32_t rt_net_available(int32_t fd) {
    if (fd < 0) return -1;
#if defined(_WIN32) || defined(_WIN64)
    u_long bytes = 0;
    if (ioctlsocket(rt_fd_to_socket(fd), FIONREAD, &bytes) == SOCKET_ERROR) {
        return -(int32_t)WSAGetLastError();
    }
    return (int32_t)bytes;
#else
    int bytes = 0;
    if (ioctl(fd, FIONREAD, &bytes) < 0) {
        return -errno;
    }
    return bytes;
#endif
}

/* RFC 009 M2: 同步 send/recv fd-level 原语（用于测试和非 Reactor 路径）。
 * 与 rt_socket_send/receive（handle-based facade）对应，但直接操作 int32_t fd。
 * send 返回实际发送字节数（>0）或 0（失败/连接关闭）。
 * recv 返回实际接收字节数（>0）或 0（EOF/失败），data 写入 buf。 */
int32_t rt_net_send(int32_t fd, const void* data, int32_t length) {
    if (fd < 0 || !data || length <= 0) return 0;
#if defined(_WIN32) || defined(_WIN64)
    int sent = send(rt_fd_to_socket(fd), (const char*)data, length, 0);
#else
    ssize_t sent = send(fd, (const char*)data, length, MSG_NOSIGNAL);
#endif
    return (sent > 0) ? (int32_t)sent : 0;
}

int32_t rt_net_recv(int32_t fd, void* buf, int32_t bufSize) {
    if (fd < 0 || !buf || bufSize <= 0) return 0;
#if defined(_WIN32) || defined(_WIN64)
    int n = recv(rt_fd_to_socket(fd), (char*)buf, bufSize, 0);
#else
    ssize_t n = recv(fd, (char*)buf, (size_t)bufSize, 0);
#endif
    /* 0 = 对端关闭（真 EOF）；-1 = 错误（含非阻塞 WSAEWOULDBLOCK/EWOULDBLOCK）。
     * 此前错误一律折叠为 0，使 TLS 同步读把「非阻塞无数据」误判为 EOF 而提前关连接。 */
    if (n > 0) return (int32_t)n;
    return (n == 0) ? 0 : -1;
}
