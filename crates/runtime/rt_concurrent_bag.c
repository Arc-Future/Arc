// ConcurrentBag<T> — per-worker thread-local bag with work-stealing (RFC 024 M3).
//
// Per-slot mutex 保护 owner Add/Take 与 steal（禁止 owner 无锁 CAS 与 steal
// 无 CAS 改 head/tail 竞态——高压下会丢/重/读到已 retire 节点）。
// 并发 Take 后节点 retire（不 free/复用）；clear 独占路径 free。
//
// Slots: 256 per-worker slots, mapped by rt_threadpool_worker_id() % 256.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

#define BAG_MAX_WORKERS 256

typedef struct rt_cb_node {
    void*               value;
    struct rt_cb_node*  next;
} rt_cb_node_t;

typedef struct {
    rt_cb_node_t* head;      // owner writes (CAS-free), readers use atomic load
    rt_cb_node_t* tail;      // for enumeration / steal
    void*         lock;       // rt_mutex_t* for steal path
    int32_t       count;      // per-slot count (approximate)
} rt_cb_worker_t;

typedef struct rt_concurrent_bag_t {
    rt_cb_worker_t workers[BAG_MAX_WORKERS];
    rt_cb_node_t*  free_list;   // internal recycled node pool
    int32_t        free_count;
    int32_t        total_count; // approximate, atomic via __sync_*
    int32_t        worker_count;
} rt_concurrent_bag_t;

/* ---- Node pool ----
 * 并发 Take 后不 free/复用节点：free_list 与堆复用均可致读到 value=NULL/脏指针。
 * clear 路径独占时再 free。 */

static rt_cb_node_t* bag_alloc_node(rt_concurrent_bag_t* b) {
    (void)b;
    return (rt_cb_node_t*)calloc(1, sizeof(rt_cb_node_t));
}

static void bag_retire_node(rt_concurrent_bag_t* b, rt_cb_node_t* node) {
    (void)b;
    (void)node;
}

/* ---- Create ---- */

void* rt_concurrent_bag_create(void) {
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)calloc(1, sizeof(*b));
    for (int32_t i = 0; i < BAG_MAX_WORKERS; i++) {
        b->workers[i].lock = rt_mutex_create();
        b->workers[i].head = NULL;
        b->workers[i].tail = NULL;
        b->workers[i].count = 0;
    }
    b->free_list = NULL;
    b->free_count = 0;
    b->total_count = 0;
    return b;
}

/* ---- Add (per-slot mutex；与 steal 同锁) ---- */

void rt_concurrent_bag_add(void* bag, void* value) {
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)bag;
    int32_t wid = rt_threadpool_worker_id();
    if (wid < 0) wid = 0;
    int32_t slot = wid % BAG_MAX_WORKERS;
    rt_cb_worker_t* w = &b->workers[slot];

    rt_cb_node_t* node = bag_alloc_node(b);
    node->value = value;
    node->next = NULL;

    rt_mutex_lock(w->lock);
    node->next = w->head;
    w->head = node;
    if (w->tail == NULL) {
        w->tail = node;
    }
    w->count++;
    rt_mutex_unlock(w->lock);
    __sync_fetch_and_add(&b->total_count, 1);
}

/* ---- TryTake (own slot 持锁，再 steal 持他槽锁) ---- */

int32_t rt_concurrent_bag_try_take(void* bag, void** out_value) {
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)bag;
    int32_t wid = rt_threadpool_worker_id();
    if (wid < 0) wid = 0;
    int32_t slot = wid % BAG_MAX_WORKERS;

    // 1. Own slot（与 steal 同锁，避免与无 CAS 改 head/tail 竞态）
    rt_cb_worker_t* own = &b->workers[slot];
    rt_mutex_lock(own->lock);
    if (own->head) {
        rt_cb_node_t* head = own->head;
        own->head = head->next;
        if (own->head == NULL) own->tail = NULL;
        own->count--;
        *out_value = head->value;
        rt_mutex_unlock(own->lock);
        __sync_fetch_and_sub(&b->total_count, 1);
        bag_retire_node(b, head);
        return 1;
    }
    rt_mutex_unlock(own->lock);

    // 2. Steal from other workers
    for (int32_t i = 0; i < BAG_MAX_WORKERS; i++) {
        int32_t sid = (slot + 1 + i) % BAG_MAX_WORKERS;
        if (sid == slot) continue;
        rt_cb_worker_t* w = &b->workers[sid];

        rt_mutex_lock(w->lock);
        // Take from tail (oldest) like a deque stealer
        rt_cb_node_t* tail = w->tail;
        if (tail) {
            // Find prev of tail；链不一致则跳过（勿误摘）
            rt_cb_node_t* prev = NULL;
            rt_cb_node_t* n = w->head;
            while (n && n != tail) {
                prev = n;
                n = n->next;
            }
            if (n != tail) {
                rt_mutex_unlock(w->lock);
                continue;
            }
            if (prev) {
                prev->next = NULL;
                w->tail = prev;
            } else {
                w->head = NULL;
                w->tail = NULL;
            }
            w->count--;
            *out_value = tail->value;
            rt_mutex_unlock(w->lock);
            __sync_fetch_and_sub(&b->total_count, 1);
            bag_retire_node(b, tail);
            return 1;
        }
        rt_mutex_unlock(w->lock);
    }
    return 0;
}

int32_t rt_concurrent_bag_try_peek(void* bag, void** out_value) {
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)bag;
    for (int32_t i = 0; i < BAG_MAX_WORKERS; i++) {
        rt_cb_worker_t* w = &b->workers[i];
        if (w->head) {
            *out_value = w->head->value;
            return 1;
        }
    }
    return 0;
}

int32_t rt_concurrent_bag_count(void* bag) {
    return ((rt_concurrent_bag_t*)bag)->total_count;
}

int32_t rt_concurrent_bag_is_empty(void* bag) {
    return ((rt_concurrent_bag_t*)bag)->total_count == 0;
}

void rt_concurrent_bag_clear(void* bag) {
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)bag;
    for (int32_t i = 0; i < BAG_MAX_WORKERS; i++) {
        rt_cb_worker_t* w = &b->workers[i];
        rt_mutex_lock(w->lock);
        rt_cb_node_t* n = w->head;
        while (n) {
            rt_cb_node_t* next = n->next;
            free(n);
            n = next;
        }
        w->head = NULL;
        w->tail = NULL;
        w->count = 0;
        rt_mutex_unlock(w->lock);
    }
    b->total_count = 0;
}

void* rt_concurrent_bag_to_array(void* bag) {
    if (!bag) return NULL;
    rt_concurrent_bag_t* b = (rt_concurrent_bag_t*)bag;
    int32_t cnt = b->total_count;
    if (cnt <= 0) return rt_array_create(0, (int32_t)sizeof(void*));
    void* arr = rt_array_create(cnt, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    for (int32_t i = 0; i < BAG_MAX_WORKERS && idx < cnt; i++) {
        rt_cb_worker_t* w = &b->workers[i];
        if (!w->head) continue;
        rt_mutex_lock(w->lock);
        for (rt_cb_node_t* n = w->head; n && idx < cnt; n = n->next) {
            items[idx++] = n->value;
        }
        rt_mutex_unlock(w->lock);
    }
    return arr;
}

int32_t rt_concurrent_bag_try_add(void* bag, void* value) {
    rt_concurrent_bag_add(bag, value);
    return 1;
}

void rt_concurrent_bag_copy_to(void* bag, void* dst, int32_t start_idx) {
    void* arr = rt_concurrent_bag_to_array(bag);
    if (!arr) return;
    int32_t len = rt_array_length(arr);
    void** src = (void**)arr;
    void** dst_items = (void**)dst;
    for (int32_t i = 0; i < len; i++) {
        dst_items[start_idx + i] = src[i];
    }
    rt_array_destroy(arr);
}
