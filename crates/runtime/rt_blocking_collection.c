// BlockingCollection<T> — Semaphore-based bounded producer-consumer (RFC 024 M5/M7).
//
// Internally wraps an IConcurrentCollection backing store (Queue/Bag/Stack)
// and uses two semaphores:
//   add_sem = bounded_capacity (or MAX if unbounded) — producers wait here
//   take_sem = 0 — consumers wait here until items arrive
//
// M7: rt_blocking_collection_create_with(inner, kind, ...) selects backing by kind
//   0 = ConcurrentQueue, 1 = ConcurrentBag, 2 = ConcurrentStack.
// Default create() still owns a fresh ConcurrentQueue (M5 compat).

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

#define UNBOUNDED_CAPACITY 0x7FFFFFFF

enum {
    RT_PCC_KIND_QUEUE = 0,
    RT_PCC_KIND_BAG = 1,
    RT_PCC_KIND_STACK = 2,
};

typedef struct rt_blocking_collection {
    void*      inner;              // PCC handle (Queue/Bag/Stack)
    int32_t    kind;               // RT_PCC_KIND_*
    void*      add_sem;            // capacity-count (producers wait)
    void*      take_sem;           // count (consumers wait)
    void*      inner_lock;         // mutex for backing access
    int32_t    bounded_capacity;
    // C# semantics: IsAddingCompleted after CompleteAdding;
    // IsCompleted = adding done AND underlying collection empty.
    int32_t    is_adding_completed;
} rt_blocking_collection_t;

/* ---- PCC dispatch (RFC 024 M7) ---- */

static void pcc_try_add(void* inner, int32_t kind, void* value) {
    switch (kind) {
    case RT_PCC_KIND_BAG:
        rt_concurrent_bag_add(inner, value);
        break;
    case RT_PCC_KIND_STACK:
        rt_concurrent_stack_push(inner, value);
        break;
    case RT_PCC_KIND_QUEUE:
    default:
        rt_concurrent_queue_enqueue(inner, value);
        break;
    }
}

static int32_t pcc_try_take(void* inner, int32_t kind, void** out_value) {
    switch (kind) {
    case RT_PCC_KIND_BAG:
        return rt_concurrent_bag_try_take(inner, out_value);
    case RT_PCC_KIND_STACK:
        return rt_concurrent_stack_try_pop(inner, out_value);
    case RT_PCC_KIND_QUEUE:
    default:
        return rt_concurrent_queue_try_dequeue(inner, out_value);
    }
}

static int32_t pcc_count(void* inner, int32_t kind) {
    switch (kind) {
    case RT_PCC_KIND_BAG:
        return rt_concurrent_bag_count(inner);
    case RT_PCC_KIND_STACK:
        return rt_concurrent_stack_count(inner);
    case RT_PCC_KIND_QUEUE:
    default:
        return rt_concurrent_queue_count(inner);
    }
}

static void* pcc_to_array(void* inner, int32_t kind) {
    switch (kind) {
    case RT_PCC_KIND_BAG:
        return rt_concurrent_bag_to_array(inner);
    case RT_PCC_KIND_STACK:
        return rt_concurrent_stack_to_array(inner);
    case RT_PCC_KIND_QUEUE:
    default:
        return rt_concurrent_queue_to_array(inner);
    }
}

void* rt_blocking_collection_create_with(void* inner, int32_t kind,
                                         int32_t capacity, int32_t strategy) {
    (void)strategy;
    if (!inner) return NULL;
    if (kind < RT_PCC_KIND_QUEUE || kind > RT_PCC_KIND_STACK) return NULL;

    int32_t n = pcc_count(inner, kind);
    int32_t bound = capacity > 0 ? capacity : UNBOUNDED_CAPACITY;
    if (capacity > 0 && n > capacity) return NULL;

    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)calloc(1, sizeof(*bc));
    bc->inner = inner;
    bc->kind = kind;
    bc->inner_lock = rt_mutex_create();
    bc->bounded_capacity = bound;
    bc->add_sem = rt_semaphore_create(bound - n, bound);
    bc->take_sem = rt_semaphore_create(n, UNBOUNDED_CAPACITY);
    bc->is_adding_completed = 0;
    return bc;
}

void* rt_blocking_collection_create(int32_t capacity, int32_t strategy) {
    return rt_blocking_collection_create_with(
        rt_concurrent_queue_create(), RT_PCC_KIND_QUEUE, capacity, strategy);
}

void* rt_blocking_collection_create_with_queue(void* inner, int32_t capacity, int32_t strategy) {
    return rt_blocking_collection_create_with(inner, RT_PCC_KIND_QUEUE, capacity, strategy);
}

void* rt_blocking_collection_create_with_bag(void* inner, int32_t capacity, int32_t strategy) {
    return rt_blocking_collection_create_with(inner, RT_PCC_KIND_BAG, capacity, strategy);
}

void* rt_blocking_collection_create_with_stack(void* inner, int32_t capacity, int32_t strategy) {
    return rt_blocking_collection_create_with(inner, RT_PCC_KIND_STACK, capacity, strategy);
}

void rt_blocking_collection_add(void* bc_ptr, void* value) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    if (bc->is_adding_completed) return;
    rt_semaphore_wait(bc->add_sem);

    rt_mutex_lock(bc->inner_lock);
    pcc_try_add(bc->inner, bc->kind, value);
    rt_mutex_unlock(bc->inner_lock);

    rt_semaphore_release(bc->take_sem);
}

void* rt_blocking_collection_take(void* bc_ptr) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    rt_semaphore_wait(bc->take_sem);

    rt_mutex_lock(bc->inner_lock);
    void* out_value = NULL;
    pcc_try_take(bc->inner, bc->kind, &out_value);
    rt_mutex_unlock(bc->inner_lock);

    if (out_value != NULL) {
        rt_semaphore_release(bc->add_sem);
    }
    return out_value;
}

void rt_blocking_collection_complete(void* bc_ptr) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    bc->is_adding_completed = 1;
}

int32_t rt_blocking_collection_is_completed(void* bc_ptr) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    return bc->is_adding_completed && pcc_count(bc->inner, bc->kind) == 0;
}

int32_t rt_blocking_collection_count(void* bc_ptr) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    return pcc_count(bc->inner, bc->kind);
}

int32_t rt_blocking_collection_bounded_capacity(void* bc_ptr) {
    return ((rt_blocking_collection_t*)bc_ptr)->bounded_capacity;
}

int32_t rt_blocking_collection_try_add(void* bc_ptr, void* value) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    if (bc->is_adding_completed) return 0;
    if (!rt_semaphore_wait_timeout(bc->add_sem, 0)) return 0;
    rt_mutex_lock(bc->inner_lock);
    pcc_try_add(bc->inner, bc->kind, value);
    rt_mutex_unlock(bc->inner_lock);
    rt_semaphore_release(bc->take_sem);
    return 1;
}

int32_t rt_blocking_collection_try_take(void* bc_ptr, void** out_value) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    if (!rt_semaphore_wait_timeout(bc->take_sem, 0)) return 0;
    rt_mutex_lock(bc->inner_lock);
    int32_t ok = pcc_try_take(bc->inner, bc->kind, out_value);
    rt_mutex_unlock(bc->inner_lock);
    if (ok) rt_semaphore_release(bc->add_sem);
    return ok;
}

int32_t rt_blocking_collection_is_adding_completed(void* bc_ptr) {
    return ((rt_blocking_collection_t*)bc_ptr)->is_adding_completed;
}

void* rt_blocking_collection_to_array(void* bc_ptr) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    rt_mutex_lock(bc->inner_lock);
    void* arr = pcc_to_array(bc->inner, bc->kind);
    rt_mutex_unlock(bc->inner_lock);
    return arr;
}

int32_t rt_blocking_collection_try_add_to(void* bc_ptr, void* value, uint64_t timeout_ms) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    if (bc->is_adding_completed) return 0;
    if (!rt_semaphore_wait_timeout(bc->add_sem, timeout_ms)) return 0;
    rt_mutex_lock(bc->inner_lock);
    pcc_try_add(bc->inner, bc->kind, value);
    rt_mutex_unlock(bc->inner_lock);
    rt_semaphore_release(bc->take_sem);
    return 1;
}

int32_t rt_blocking_collection_try_take_to(void* bc_ptr, void** out_value, uint64_t timeout_ms) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    if (!rt_semaphore_wait_timeout(bc->take_sem, timeout_ms)) return 0;
    rt_mutex_lock(bc->inner_lock);
    int32_t ok = pcc_try_take(bc->inner, bc->kind, out_value);
    rt_mutex_unlock(bc->inner_lock);
    if (ok) rt_semaphore_release(bc->add_sem);
    return ok;
}

void rt_blocking_collection_copy_to(void* bc_ptr, void* dst, int32_t start_idx) {
    rt_blocking_collection_t* bc = (rt_blocking_collection_t*)bc_ptr;
    rt_mutex_lock(bc->inner_lock);
    void* arr = pcc_to_array(bc->inner, bc->kind);
    rt_mutex_unlock(bc->inner_lock);
    if (!arr) return;
    int32_t len = rt_array_length(arr);
    void** src = (void**)arr;
    void** dst_items = (void**)dst;
    for (int32_t i = 0; i < len; i++) {
        dst_items[start_idx + i] = src[i];
    }
    rt_array_destroy(arr);
}
