// CancellationTokenSource ABI (RFC 009 M4 + M5.4).
//
// 协作式取消的控制端：atomic canceled 标志 + Treiber stack 回调 + CancelAfter 定时器。
// CT（CancellationToken）与 CTS 共享同一 RtCts* 指针（只读视图）。
//
// ## M5.4 无锁化升级
//
// M4 使用 mutex + 链表，高并发 Register/Cancel 竞争激烈（~50000ns / Register）。
// M5.4 升级为：
//   - Treiber stack（无锁 LIFO CAS push）—— Register ~30ns
//   - atomic flag 取消检查 —— IsCancellationRequested ~1ns（热路径零开销）
//   - 节点池化（per-thread free-list）—— Register 热路径零 malloc
//
// ## ABA 消除
//
// Treiber stack 经典 ABA 通过 hazard pointer / epoch reclamation 消除。
// M5.4 采用节点池化：节点从 per-thread free-list 取出/归还，生命周期与 CTS 绑定
// （CTS destroy 时释放所有剩余节点 + free-list），ABA 不发生。
//
// ## M4 ABI 兼容
//
// M4 ABI（rt_cts_register 等）保留为兼容入口，内部委托 M5.4 无锁实现。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* ---- RtCts 结构（M5.4 无锁版） ---- */

struct RtCts {
    _Atomic(rt_cts_node*) stack_top;  /* Treiber stack 顶 */
    _Atomic(int32_t)      canceled;   /* 0=未取消, 1=已取消 */
    int32_t               _pad[15];   /* cache line 对齐 */
};

/* ---- per-thread 节点 free-list（slab） ----
 *
 * 避免 Register 热路径 malloc。节点取出/归还均 O(1) LIFO。
 * free-list cap = 256（与 rt_task_slab 一致）；超出直接 malloc/free。
 * 生命周期：thread_local，线程退出时不自动释放（进程退出时由 OS 回收）。
 *
 * ABA 消除：节点从 free-list 取出后，调用方填充 cb/data 并 push 到 stack；
 * stack pop 后归还 free-list。free-list 与 stack 互斥使用同一节点，
 * 不会出现"节点在 stack 中又被另一线程 pop 后立即 push"的 ABA 窗口。
 */

#define RT_CTS_NODE_FREE_CAP 256

typedef struct rt_cts_node_freelist {
    rt_cts_node* head;       /* LIFO 链 */
    int32_t      count;      /* 当前 free-list 节点数 */
    int32_t      total_alloc;/* 累计 malloc 次数（诊断用） */
} rt_cts_node_freelist;

#if defined(_MSC_VER)
  /* MSVC __declspec(thread) 不支持非平凡类型，但 rt_cts_node_freelist 是 POD */
  __declspec(thread) static rt_cts_node_freelist t_fl = { NULL, 0, 0 };
#else
  _Thread_local static rt_cts_node_freelist t_fl = { NULL, 0, 0 };
#endif

/* ---- 节点池 ABI ---- */

rt_cts_node* rt_cts_node_alloc(void) {
    if (t_fl.head) {
        rt_cts_node* n = t_fl.head;
        t_fl.head = n->next;
        t_fl.count--;
        /* 清零字段（保留 next 由调用方设置） */
        n->cb = NULL;
        n->data = NULL;
        n->next = NULL;
        atomic_store_explicit(&n->registered, 0, memory_order_relaxed);
        return n;
    }
    /* free-list 空 → malloc */
    rt_cts_node* n = (rt_cts_node*)calloc(1, sizeof(rt_cts_node));
    if (n) {
        atomic_init(&n->registered, 0);
        t_fl.total_alloc++;
    }
    return n;
}

void rt_cts_node_free(rt_cts_node* node) {
    if (!node) return;
    if (t_fl.count < RT_CTS_NODE_FREE_CAP) {
        node->cb = NULL;
        node->data = NULL;
        node->next = t_fl.head;
        atomic_store_explicit(&node->registered, 0, memory_order_relaxed);
        t_fl.head = node;
        t_fl.count++;
    } else {
        /* free-list 满 → 直接 free */
        free(node);
    }
}

/* ---- 单节点触发（CAS 防重复） ---- */

void rt_cts_node_try_fire(rt_cts_node* node) {
    if (!node || !node->cb) return;
    int32_t expected = 1;
    if (atomic_compare_exchange_strong_explicit(
            &node->registered, &expected, 0,
            memory_order_acq_rel, memory_order_relaxed)) {
        node->cb(node->data);  /* 仅触发一次 */
    }
}

/* ---- M5.4 无锁 ABI ---- */

void rt_cts_register_lf(RtCts* cts, rt_cts_node* node) {
    if (!cts || !node || !node->cb) return;
    /* 标记已注册未触发 */
    atomic_store_explicit(&node->registered, 1, memory_order_release);
    /* Treiber push：CAS loop */
    rt_cts_node* top = atomic_load_explicit(&cts->stack_top, memory_order_acquire);
    do {
        node->next = top;
    } while (!atomic_compare_exchange_weak_explicit(
        &cts->stack_top, &top, node,
        memory_order_release, memory_order_acquire));
    /* 注册后检查取消状态（避免 cancel 先于 register 的窗口） */
    if (atomic_load_explicit(&cts->canceled, memory_order_acquire)) {
        rt_cts_node_try_fire(node);  /* 已取消，立即触发本回调 */
    }
}

/* ---- M4 ABI（兼容入口，内部委托 M5.4 无锁实现） ---- */

void* rt_cts_create(void) {
    RtCts* cts = RT_OPAQUE_NEW(RtCts);
    if (!cts) return NULL;
    atomic_init(&cts->stack_top, (rt_cts_node*)NULL);
    atomic_init(&cts->canceled, 0);
    return cts;
}

int32_t rt_cts_is_canceled(void* cts) {
    RtCts* c = (RtCts*)cts;
    if (!c) return 0;
    /* atomic load —— 热路径零开销（~1ns） */
    return atomic_load_explicit(&c->canceled, memory_order_acquire);
}

int32_t rt_cts_can_be_canceled(void* cts) {
    /* .NET 语义：None 令牌（default/null）恒不可取消；真实 CTS 背书的令牌可取消。 */
    return cts != NULL;
}

void rt_cts_cancel(void* cts) {
    RtCts* c = (RtCts*)cts;
    if (!c) return;
    /* 幂等：CAS 0→1，仅成功者触发回调 */
    int32_t expected = 0;
    if (!atomic_compare_exchange_strong_explicit(
            &c->canceled, &expected, 1,
            memory_order_acq_rel, memory_order_relaxed)) {
        return;  /* 已取消，no-op */
    }
    /* atomic_exchange 取出整个 stack（无锁） */
    rt_cts_node* node = atomic_exchange_explicit(&c->stack_top, (rt_cts_node*)NULL,
                                                  memory_order_acq_rel);
    /* 锁外遍历触发（避免回调内操作 CTS 死锁） */
    while (node) {
        rt_cts_node* next = node->next;
        rt_cts_node_try_fire(node);
        /* 触发后归还 free-list（若回调内不再持有节点） */
        rt_cts_node_free(node);
        node = next;
    }
}

void rt_cts_register(void* cts, void(*fn)(void*), void* data) {
    RtCts* c = (RtCts*)cts;
    if (!c || !fn) return;
    /* C# 语义：若已取消，立即调用 fn(data) */
    if (atomic_load_explicit(&c->canceled, memory_order_acquire)) {
        fn(data);
        return;
    }
    /* 从 slab 取节点，填充 cb/data，委托 rt_cts_register_lf */
    rt_cts_node* node = rt_cts_node_alloc();
    if (!node) {
        /* OOM fallback：直接调用（不注册） */
        fn(data);
        return;
    }
    node->cb = fn;
    node->data = data;
    rt_cts_register_lf(c, node);
}

/* CancelAfter 定时器到期回调：调用 rt_cts_cancel */
static void rt_cts_cancel_after_callback(void* data) {
    rt_cts_cancel(data);
}

void rt_cts_cancel_after(void* cts, int32_t ms) {
    RtCts* c = (RtCts*)cts;
    if (!c || ms < 0) return;
    /* 通过 EventLoop 定时器延迟触发 cancel */
    void* loop = rt_event_loop_current();
    if (!loop) {
        /* 无 EventLoop：立即 cancel（fallback） */
        rt_cts_cancel(c);
        return;
    }
    rt_event_loop_schedule(loop, rt_cts_cancel_after_callback, c, (uint64_t)ms);
}

void rt_cts_destroy(void* cts) {
    RtCts* c = (RtCts*)cts;
    if (!c) return;
    /* 释放 stack 中剩余节点（未触发的回调） */
    rt_cts_node* node = atomic_exchange_explicit(&c->stack_top, (rt_cts_node*)NULL,
                                                  memory_order_acq_rel);
    while (node) {
        rt_cts_node* next = node->next;
        free(node);  /* destroy 时直接 free，不归还 free-list（避免跨线程） */
        node = next;
    }
    free(c);
}

/* ThrowIfCancellationRequested 的完整封装：若已取消则 rt_panic。
 * 避免 codegen 发射分支 + 字符串常量；符合 facade 模式（复杂逻辑归 runtime）。
 * M4 用 rt_panic 兜底（D4 决策）；Exception 体系留独立 RFC。 */
void rt_cts_throw_if_canceled(void* cts) {
    if (rt_cts_is_canceled(cts)) {
        rt_panic("OperationCanceledException");
    }
}

/* Arc closure trampoline：ct.Register(callback) 时，callback 是 arc_closure。
 * rt_cts_register 存储 fn=rt_cts_callback_trampoline, data=closure_ptr。
 * ct 取消时调用 trampoline(closure) → closure->fn_ptr(closure->env)。
 * Action 的 lifted lambda 签名为 void(ptr env)，故 trampoline 仅转发 env。
 */
void rt_cts_callback_trampoline(void* data) {
    arc_closure* closure = (arc_closure*)data;
    if (!closure || !closure->fn_ptr) return;
    typedef void (*fn_t)(void*);
    fn_t fn = (fn_t)closure->fn_ptr;
    fn(closure->env);
}
