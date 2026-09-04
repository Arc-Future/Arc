// ConcurrentQueue<T> — Michael-Scott lock-free queue (RFC 024 M2).
//
// 节点分配：calloc。并发 dequeue 路径禁止 free/复用哨兵指针——否则 MS 队列 ABA
//（free 后 calloc 撞址 → CAS(head) 成功但链语义损坏 → 丢/重/脏值）。
// 旧 head 仅在 clear（无并发）时归还堆；hazard/epoch + slab 属后续优化。
//
// Count is maintained via __sync_fetch_and_add/__sync_fetch_and_sub for
// approximate thread-safe reads.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct rt_cq_node {
    void*               value;
    struct rt_cq_node*  next;
} rt_cq_node_t;

typedef struct rt_concurrent_queue_t {
    rt_cq_node_t* head;
    rt_cq_node_t* tail;
    rt_cq_node_t* free_list;   /* 保留字段兼容布局；并发路径不复用 */
    int32_t       free_count;
    int32_t       count;        // approximate, atomic via __sync_*
} rt_concurrent_queue_t;

/* ---- Node alloc / retire ---- */

static rt_cq_node_t* cq_alloc_node(rt_concurrent_queue_t* q) {
    (void)q;
    return (rt_cq_node_t*)calloc(1, sizeof(rt_cq_node_t));
}

static void cq_retire_node(rt_concurrent_queue_t* q, rt_cq_node_t* node) {
    /* 并发寿命内泄漏，避免 ABA。 */
    (void)q;
    (void)node;
}

/* ---- Create / Clear ---- */

void* rt_concurrent_queue_create(void) {
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)calloc(1, sizeof(*q));
    // sentinel node
    rt_cq_node_t* sentinel = cq_alloc_node(q);
    sentinel->value = NULL;
    sentinel->next = NULL;
    q->head = sentinel;
    q->tail = sentinel;
    q->free_list = NULL;
    q->free_count = 0;
    q->count = 0;
    return q;
}

void rt_concurrent_queue_clear(void* queue) {
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)queue;
    /* clear：调用方保证无并发；可安全 free 数据节点 */
    rt_cq_node_t* n = q->head->next;
    while (n) {
        rt_cq_node_t* next = n->next;
        free(n);
        n = next;
    }
    q->head->next = NULL;
    q->tail = q->head;
    q->count = 0;
}

/* ---- Core operations ---- */

void rt_concurrent_queue_enqueue(void* queue, void* value) {
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)queue;
    rt_cq_node_t* node = cq_alloc_node(q);
    node->value = value;
    node->next = NULL;

    /* Michael-Scott：若 tail 落后于真实链尾，必须 help 推进，否则
       CAS(tail->next, NULL, node) 会在 next!=NULL 时永久失败。 */
    for (;;) {
        rt_cq_node_t* tail = q->tail;
        rt_cq_node_t* next = tail->next;
        if (tail != q->tail) {
            continue;
        }
        if (next != NULL) {
            __sync_bool_compare_and_swap(&q->tail, tail, next);
            continue;
        }
        if (__sync_bool_compare_and_swap(&tail->next, NULL, node)) {
            __sync_bool_compare_and_swap(&q->tail, tail, node);
            break;
        }
    }
    __sync_fetch_and_add(&q->count, 1);
}

int32_t rt_concurrent_queue_try_dequeue(void* queue, void** out_value) {
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)queue;
    rt_cq_node_t* head;
    rt_cq_node_t* next;
    do {
        head = q->head;
        next = head->next;
        if (next == NULL) return 0;
    } while (!__sync_bool_compare_and_swap(&q->head, head, next));
    *out_value = next->value;
    __sync_fetch_and_sub(&q->count, 1);
    /* 旧 head（哨兵角色移交）retire 而不 free，避免 ABA */
    cq_retire_node(q, head);
    return 1;
}

int32_t rt_concurrent_queue_try_peek(void* queue, void** out_value) {
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)queue;
    rt_cq_node_t* next = q->head->next;
    if (next == NULL) return 0;
    *out_value = next->value;
    return 1;
}

int32_t rt_concurrent_queue_count(void* queue) {
    return ((rt_concurrent_queue_t*)queue)->count;
}

int32_t rt_concurrent_queue_is_empty(void* queue) {
    return ((rt_concurrent_queue_t*)queue)->head->next == NULL;
}

void* rt_concurrent_queue_to_array(void* queue) {
    if (!queue) return NULL;
    rt_concurrent_queue_t* q = (rt_concurrent_queue_t*)queue;
    int32_t cnt = q->count;
    if (cnt <= 0) cnt = 0;
    void* arr = rt_array_create(cnt, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    rt_cq_node_t* cur = q->head->next;  // skip sentinel
    int32_t idx = 0;
    while (cur && idx < cnt) {
        items[idx++] = cur->value;
        cur = cur->next;
    }
    return arr;
}

int32_t rt_concurrent_queue_try_add(void* queue, void* value) {
    rt_concurrent_queue_enqueue(queue, value);
    return 1;
}

int32_t rt_concurrent_queue_try_take(void* queue, void** out_value) {
    return rt_concurrent_queue_try_dequeue(queue, out_value);
}

void rt_concurrent_queue_copy_to(void* queue, void* dst, int32_t start_idx) {
    void* arr = rt_concurrent_queue_to_array(queue);
    if (!arr) return;
    int32_t len = rt_array_length(arr);
    void** src = (void**)arr;
    void** dst_items = (void**)dst;
    for (int32_t i = 0; i < len; i++) {
        dst_items[start_idx + i] = src[i];
    }
    rt_array_destroy(arr);
}
