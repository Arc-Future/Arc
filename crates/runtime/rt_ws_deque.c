// Chase-Lev work-stealing deque (RFC 009 M5.1).
//
// 经典 Chase-Lev deque 实现：
//   - push/pop 仅 owner worker 调用（无锁，atomic load/store）
//   - steal 由其他 worker 调用（CAS on top）
//   - LIFO 语义对 owner（cache 局部性最优），FIFO 语义对 stealer（窃取最旧任务）
//
// 不变量：bottom >= top 恒成立。
//
// 参考：
//   David Chase, Yossi Lev. "Dynamic Circular Work-Stealing Deque." SPAA 2005.
//   Le, Pop, Cohen, Nardelli. "Correct and Efficient Work-Stealing." CC 2009.
//
// 性能目标（RFC 009 §16.4）：
//   - push/pop: O(1), 无 CAS, ~5ns
//   - steal: O(1), 1 次 CAS, ~30ns
//
// M5.1 MVP：固定容量（cap_log2 参数，默认 16=65536）。溢出时调用 overflow_handler
// 将任务转入全局 injector queue（由 rt_threadpool.c 注册）。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* ---- 内部数据结构 ---- */

typedef struct rt_ws_deque {
    /* 原子索引（64-bit 避免对 32-bit 平台的 ABA） */
    _Atomic(uint64_t)  bottom;        /* owner 推入端 */
    _Atomic(uint64_t)  top;           /* stealer 偷取端 */
    void**             buffer;        /* 环形缓冲区（cap = 2^cap_log2） */
    uint64_t           cap_mask;      /* cap - 1 */
    int32_t            cap_log2;
    int32_t            worker_id;     /* 本 deque 所属 worker */
    void*              overflow_ctx;  /* 所属 ThreadPool*；溢出时传给 handler */

    /* cache line 对齐填充（避免 false sharing 跨 worker） */
    char _pad[64 - sizeof(_Atomic(uint64_t)) * 2 - sizeof(void*) * 2 - sizeof(uint64_t)
              - sizeof(int32_t) * 2];
} rt_ws_deque;

/* 全局 overflow handler（由 rt_threadpool.c 注册）。
 * 当 push 检测到 deque 已满时，将 item 转入**该 deque 的** injector queue。
 * ctx 为 per-deque overflow_ctx（池指针），禁止全局 current-pool（多池 UAF）。
 * handler=NULL 时走下方扩容路径（纯 deque 测试）。 */
static void (*g_overflow_handler)(void* ctx, void* item) = NULL;

void rt_ws_deque_set_overflow_handler(void (*handler)(void* ctx, void* item)) {
    g_overflow_handler = handler;
}

void rt_ws_deque_set_overflow_ctx(rt_ws_deque* q, void* ctx) {
    if (q) q->overflow_ctx = ctx;
}

/* ---- ABI 实现 ---- */

rt_ws_deque* rt_ws_deque_create(int32_t worker_id, int32_t cap_log2) {
    if (cap_log2 < 4) cap_log2 = 4;       /* 最小 16 */
    if (cap_log2 > 20) cap_log2 = 20;     /* 最大 1M */

    rt_ws_deque* q = (rt_ws_deque*)calloc(1, sizeof(rt_ws_deque));
    if (!q) return NULL;

    uint64_t cap = (uint64_t)1 << cap_log2;
    q->buffer = (void**)calloc((size_t)cap, sizeof(void*));
    if (!q->buffer) { free(q); return NULL; }

    q->cap_log2 = cap_log2;
    q->cap_mask = cap - 1;
    q->worker_id = worker_id;
    q->overflow_ctx = NULL;
    atomic_init(&q->bottom, 0);
    atomic_init(&q->top, 0);
    return q;
}

void rt_ws_deque_destroy(rt_ws_deque* q) {
    if (!q) return;
    /* 注意：deque 中剩余的 item 由 owner 负责；此处不释放 item 内存 */
    free(q->buffer);
    free(q);
}

void rt_ws_push(rt_ws_deque* q, void* item) {
    uint64_t b = atomic_load_explicit(&q->bottom, memory_order_relaxed);
    uint64_t t = atomic_load_explicit(&q->top, memory_order_acquire);
    uint64_t size = b - t;

    if (size >= q->cap_mask + 1) {
        /* deque 已满 → 转入所属池 injector（ctx=该池；禁进程全局 current-pool） */
        if (g_overflow_handler) {
            g_overflow_handler(q->overflow_ctx, item);
            return;
        }
        /* 无 handler（纯 deque 测试）：扩展缓冲区（简化处理） */
        uint64_t new_cap = (uint64_t)1 << (q->cap_log2 + 1);
        void** new_buf = (void**)calloc((size_t)new_cap, sizeof(void*));
        if (!new_buf) return;  /* OOM，丢弃 item */
        /* 复制旧内容到新缓冲区 */
        for (uint64_t i = t; i < b; i++) {
            new_buf[i & (new_cap - 1)] = q->buffer[i & q->cap_mask];
        }
        free(q->buffer);
        q->buffer = new_buf;
        q->cap_log2++;
        q->cap_mask = new_cap - 1;
    }

    q->buffer[b & q->cap_mask] = item;
    atomic_thread_fence(memory_order_release);
    atomic_store_explicit(&q->bottom, b + 1, memory_order_relaxed);
}

void* rt_ws_pop(rt_ws_deque* q) {
    uint64_t b = atomic_load_explicit(&q->bottom, memory_order_relaxed) - 1;
    atomic_store_explicit(&q->bottom, b, memory_order_relaxed);
    atomic_thread_fence(memory_order_seq_cst);

    uint64_t t = atomic_load_explicit(&q->top, memory_order_acquire);
    if (t > b) {
        /* deque 空 */
        atomic_store_explicit(&q->bottom, b + 1, memory_order_relaxed);
        return NULL;
    }

    void* item = q->buffer[b & q->cap_mask];
    if (t == b) {
        /* 最后一个元素，需 CAS 防 steal 竞争 */
        if (!atomic_compare_exchange_strong_explicit(
                &q->top, &t, t + 1,
                memory_order_seq_cst, memory_order_relaxed)) {
            /* 被 stealer 抢走 */
            atomic_store_explicit(&q->bottom, b + 1, memory_order_relaxed);
            return NULL;
        }
        atomic_store_explicit(&q->bottom, b + 1, memory_order_relaxed);
    }
    return item;
}

void* rt_ws_steal(rt_ws_deque* q) {
    uint64_t t = atomic_load_explicit(&q->top, memory_order_acquire);
    uint64_t b = atomic_load_explicit(&q->bottom, memory_order_acquire);
    if (t >= b) return NULL;  /* 空 */

    void* item = q->buffer[t & q->cap_mask];
    if (!atomic_compare_exchange_strong_explicit(
            &q->top, &t, t + 1,
            memory_order_seq_cst, memory_order_relaxed)) {
        return NULL;  /* CAS 失败，重试或放弃 */
    }
    return item;
}

int32_t rt_ws_deque_size(rt_ws_deque* q) {
    uint64_t b = atomic_load_explicit(&q->bottom, memory_order_relaxed);
    uint64_t t = atomic_load_explicit(&q->top, memory_order_relaxed);
    return (int32_t)(b - t);
}

int32_t rt_ws_deque_worker_id(rt_ws_deque* q) {
    return q->worker_id;
}
