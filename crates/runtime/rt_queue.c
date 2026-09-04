// Queue<T> runtime ABI (RFC Phase 5).
//
// Circular buffer with 2x growth strategy. Head/tail indices track the
// logical front/back; wrapping handles the ring. No per-element ARC callbacks
// in phase 1 — reference counted types are stored as raw pointers and the
// caller is responsible for lifetime management (matching List<T> phase 1).

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

#define RT_QUEUE_INITIAL_CAPACITY 8

typedef struct RtQueue {
    void*    data;
    int32_t  size;
    int32_t  capacity;
    int32_t  elem_size;
    int32_t  head;   // index of first element
    int32_t  tail;   // index after last element
} RtQueue;

/* ---- public ABI ---- */

void* rt_queue_create(int32_t elem_size) {
    RtQueue* q = (RtQueue*)calloc(1, sizeof(RtQueue));
    if (!q) return NULL;
    q->capacity  = RT_QUEUE_INITIAL_CAPACITY;
    q->elem_size = elem_size;
    q->data      = calloc((size_t)q->capacity, (size_t)elem_size);
    if (!q->data) {
        free(q);
        return NULL;
    }
    q->size = 0;
    q->head = 0;
    q->tail = 0;
    return q;
}

void rt_queue_destroy(void* handle) {
    if (!handle) return;
    RtQueue* q = (RtQueue*)handle;
    free(q->data);
    free(q);
}

static void rt_queue_grow(RtQueue* q) {
    int32_t new_cap   = q->capacity * 2;
    void*   new_data  = calloc((size_t)new_cap, (size_t)q->elem_size);
    if (!new_data) return;
    // Copy elements from head..tail to 0..size in new buffer (unwrapped)
    char* src = (char*)q->data;
    char* dst = (char*)new_data;
    int32_t es = q->elem_size;
    for (int32_t i = 0; i < q->size; i++) {
        int32_t src_idx = (q->head + i) % q->capacity;
        memcpy(dst + i * es, src + src_idx * es, (size_t)es);
    }
    free(q->data);
    q->data     = new_data;
    q->capacity = new_cap;
    q->head     = 0;
    q->tail     = q->size;
}

void rt_queue_enqueue(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return;
    RtQueue* q = (RtQueue*)handle;
    if (q->size == q->capacity) rt_queue_grow(q);
    char* dst = (char*)q->data + q->tail * q->elem_size;
    memcpy(dst, elem_ptr, (size_t)q->elem_size);
    q->tail = (q->tail + 1) % q->capacity;
    q->size++;
}

int32_t rt_queue_dequeue(void* handle, void* out_ptr) {
    if (!handle || !out_ptr) return 0;
    RtQueue* q = (RtQueue*)handle;
    if (q->size == 0) return 0;
    char* src = (char*)q->data + q->head * q->elem_size;
    memcpy(out_ptr, src, (size_t)q->elem_size);
    q->head = (q->head + 1) % q->capacity;
    q->size--;
    return 1;
}

int32_t rt_queue_peek(void* handle, void* out_ptr) {
    if (!handle || !out_ptr) return 0;
    RtQueue* q = (RtQueue*)handle;
    if (q->size == 0) return 0;
    char* src = (char*)q->data + q->head * q->elem_size;
    memcpy(out_ptr, src, (size_t)q->elem_size);
    return 1;
}

int32_t rt_queue_count(void* handle) {
    if (!handle) return 0;
    return ((RtQueue*)handle)->size;
}

void rt_queue_clear(void* handle) {
    if (!handle) return;
    RtQueue* q = (RtQueue*)handle;
    q->size  = 0;
    q->head  = 0;
    q->tail  = 0;
}

int32_t rt_queue_contains(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtQueue* q = (RtQueue*)handle;
    int32_t es = q->elem_size;
    char* data = (char*)q->data;
    for (int32_t i = 0; i < q->size; i++) {
        int32_t idx = (q->head + i) % q->capacity;
        if (memcmp(data + idx * es, elem_ptr, (size_t)es) == 0) {
            return 1;
        }
    }
    return 0;
}

void* rt_queue_to_array(void* handle) {
    if (!handle) return NULL;
    RtQueue* q = (RtQueue*)handle;
    int32_t es = q->elem_size;
    char* arr = (char*)malloc((size_t)(q->size + 1) * (size_t)es);
    if (!arr) return NULL;
    // Store length in first element slot
    int32_t* len = (int32_t*)arr;
    *len = q->size;
    char* dst = arr + es;
    char* src = (char*)q->data;
    for (int32_t i = 0; i < q->size; i++) {
        int32_t idx = (q->head + i) % q->capacity;
        memcpy(dst + i * es, src + idx * es, (size_t)es);
    }
    return arr;
}
