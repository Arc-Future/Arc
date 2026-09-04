/* RFC 048 §5.3: Named pipe runtime (本机 IPC · RFC 048-named-pipes).
 *
 * 公共核：状态结构与平台分发；平台实现按 reactor 约定经单 TU 合并
 * （rt_reactor.c 同式）——Windows `platform/pipe_windows.c`，POSIX
 * `platform/pipe_posix.c`。语义契约见 rt_abi.h 的 rt_pipe_* 块：
 * 字节流、无消息边界、Read 0 = 对端有序关闭、Write 短写补尽且对端
 * 读端关闭时返回 0、缓冲默认 64KB、单写者/单读者配对（RFC 048 §3.1-3）。
 */
#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* 平台中立的管道状态（平台实现持有并维护；字段为两后端公共最小集）。
 *
 * 析构契约（RFC 048 M1 落定）：模式 A 裸句柄无 ARC 头、无析构钩子——显式
 * `rt_pipe_close`（门面 Terminate/Dispose）是**唯一**收口路径；close 后
 * `closed` 置位、幂等早退，后续任何方法调用经入口守卫安全返回（0/false）。
 * 状态块**不**随 close 释放（泄漏至进程退出，与 Thread/Socket 同策 H1）——
 * close 即 free 会让「close 后仍被引用的门面对象」落入 UAF。 */
typedef struct RtPipe {
    int32_t is_server;       /* 1 = server 侧（server_create 创建） */
    int32_t is_connected;    /* 当前是否处于已连接状态 */
    int32_t closed;          /* 已关闭标志：幂等 close 早退 + 方法入口守卫 */
    int32_t max_instances;   /* server: 最大实例数（POSIX 映射为串行排队） */
    char*   name;            /* Arc 逻辑名（rt_str 语义的 NUL 终止拷贝，调用方已保证） */
    void*   platform;        /* 平台私有状态（pipe_windows/pipe_posix 各自定义） */
} RtPipe;

void* rt_pipe_state_alloc(int32_t is_server, int32_t max_instances, const char* name);
void  rt_pipe_state_free(void* state);

void* rt_pipe_state_alloc(int32_t is_server, int32_t max_instances, const char* name) {
    /* RFC 050 M-a：opaque 统一头试点——对象自描述身份，ARC 误计数物理无害。 */
    RtPipe* p = (RtPipe*)rt_obj_alloc_opaque(sizeof(RtPipe));
    if (p == NULL) {
        return NULL;
    }
    p->is_server = is_server;
    p->is_connected = 0;
    p->closed = 0;
    p->max_instances = max_instances;
    p->platform = NULL;
    if (name != NULL) {
        size_t n = strlen(name) + 1;
        p->name = (char*)malloc(n);
        if (p->name == NULL) {
            rt_obj_free(p);
            return NULL;
        }
        memcpy(p->name, name, n);
    } else {
        p->name = NULL;
    }
    return p;
}

void rt_pipe_state_free(void* state) {
    RtPipe* p = (RtPipe*)state;
    if (p == NULL) {
        return;
    }
    if (p->name != NULL) {
        free(p->name);
    }
    rt_obj_free(p);
}

#ifdef _WIN32
#include "platform/pipe_windows.c"
#else
#include "platform/pipe_posix.c"
#endif
