// Stack<T> — sequential LIFO stack backed by rt_list_* ABI.
//
// Implements the standard Stack<T> interface on top of the existing
// rt_list_* runtime functions. O(1) push/pop, operates on the tail of
// the list for efficient LIFO semantics.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct RtStack {
    void*   list;          // underlying rt_list handle
    int32_t elem_size;
} RtStack;

void* rt_stack_create(int32_t elem_size, void* eq_fn, void* arc_inc, void* arc_dec) {
    RtStack* s = (RtStack*)calloc(1, sizeof(RtStack));
    s->list = rt_list_create(elem_size,
        (rt_list_eq_fn)eq_fn,
        (rt_list_arc_fn)arc_inc,
        (rt_list_arc_fn)arc_dec);
    s->elem_size = elem_size;
    return s;
}

void rt_stack_push(void* stack, const void* elem_ptr) {
    if (!stack) return;
    RtStack* s = (RtStack*)stack;
    rt_list_push(s->list, elem_ptr);
}

void rt_stack_pop(void* stack, void* out_ptr) {
    if (!stack || !out_ptr) return;
    RtStack* s = (RtStack*)stack;
    int32_t sz = rt_list_size(s->list);
    if (sz == 0) {
        memset(out_ptr, 0, s->elem_size);
        return;
    }
    rt_list_get(s->list, sz - 1, out_ptr);
    rt_list_remove_at(s->list, sz - 1);
}

int32_t rt_stack_try_pop(void* stack, void* out_ptr) {
    if (!stack || !out_ptr) return 0;
    RtStack* s = (RtStack*)stack;
    int32_t sz = rt_list_size(s->list);
    if (sz == 0) return 0;
    rt_list_get(s->list, sz - 1, out_ptr);
    rt_list_remove_at(s->list, sz - 1);
    return 1;
}

void rt_stack_peek(void* stack, void* out_ptr) {
    if (!stack || !out_ptr) return;
    RtStack* s = (RtStack*)stack;
    int32_t sz = rt_list_size(s->list);
    if (sz == 0) {
        memset(out_ptr, 0, s->elem_size);
        return;
    }
    rt_list_get(s->list, sz - 1, out_ptr);
}

int32_t rt_stack_try_peek(void* stack, void* out_ptr) {
    if (!stack || !out_ptr) return 0;
    RtStack* s = (RtStack*)stack;
    int32_t sz = rt_list_size(s->list);
    if (sz == 0) return 0;
    rt_list_get(s->list, sz - 1, out_ptr);
    return 1;
}

int32_t rt_stack_count(void* stack) {
    if (!stack) return 0;
    return rt_list_size(((RtStack*)stack)->list);
}

void rt_stack_clear(void* stack) {
    if (!stack) return;
    rt_list_clear(((RtStack*)stack)->list);
}

int32_t rt_stack_contains(void* stack, const void* elem_ptr) {
    if (!stack) return 0;
    return rt_list_contains(((RtStack*)stack)->list, elem_ptr);
}

void* rt_stack_to_array(void* stack) {
    if (!stack) return NULL;
    RtStack* s = (RtStack*)stack;
    int32_t sz = rt_list_size(s->list);
    if (sz == 0) return NULL;
    // Allocate: sz elements in reverse (stack top → array[0])
    char* arr = (char*)malloc((size_t)(sz + 1) * s->elem_size);
    if (!arr) return NULL;
    int32_t* len = (int32_t*)arr;
    *len = sz;
    char* dst = arr + s->elem_size;
    // Temp buffer for element copy
    char* tmp = (char*)malloc((size_t)s->elem_size);
    if (!tmp) { free(arr); return NULL; }
    for (int32_t i = 0; i < sz; i++) {
        int32_t src_idx = sz - 1 - i;
        rt_list_get(s->list, src_idx, tmp);
        memcpy(dst + i * s->elem_size, tmp, (size_t)s->elem_size);
    }
    free(tmp);
    return arr;
}

void rt_stack_destroy(void* stack) {
    if (!stack) return;
    RtStack* s = (RtStack*)stack;
    rt_list_destroy(s->list);
    free(s);
}
