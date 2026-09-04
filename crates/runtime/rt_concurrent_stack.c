// ConcurrentStack<T> — Treiber lock-free stack (RFC 024 M4).
//
// 节点分配：calloc。并发 pop 路径禁止 free/复用同一指针——否则 Treiber ABA
//（pop+free 后 calloc 撞上同一地址 → CAS 成功但 next 陈旧 → 丢/重）。
// 节点仅在 clear（调用方保证无并发推弹）时归还堆；进程退出回收其余泄漏。
// hazard/epoch + slab 安全回收仍属后续优化（不堵塞 M6 压力正确性）。
#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct rt_cs_node {
    void*               value;
    struct rt_cs_node*  next;
} rt_cs_node_t;

typedef struct rt_concurrent_stack_t {
    rt_cs_node_t* top;
    rt_cs_node_t* free_list;    /* 保留布局；并发路径不复用 */
    int32_t       free_count;
    int32_t       count;         // approximate, updated via __sync_*
} rt_concurrent_stack_t;

void* rt_concurrent_stack_create(void) {
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)calloc(1, sizeof(*s));
    s->top = NULL;
    s->free_list = NULL;
    s->free_count = 0;
    s->count = 0;
    return s;
}

static rt_cs_node_t* stack_alloc_node(rt_concurrent_stack_t* s) {
    (void)s;
    return (rt_cs_node_t*)calloc(1, sizeof(rt_cs_node_t));
}

static void stack_retire_node(rt_concurrent_stack_t* s, rt_cs_node_t* node) {
    /* 并发寿命内泄漏，避免 ABA；勿挂 free_list 复用。 */
    (void)s;
    (void)node;
}

void rt_concurrent_stack_push(void* stack, void* value) {
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)stack;
    rt_cs_node_t* node = stack_alloc_node(s);
    node->value = value;

    rt_cs_node_t* top;
    do {
        top = s->top;
        node->next = top;
    } while (!__sync_bool_compare_and_swap(&s->top, top, node));
    __sync_fetch_and_add(&s->count, 1);
}

int32_t rt_concurrent_stack_try_pop(void* stack, void** out_value) {
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)stack;
    rt_cs_node_t* top;
    rt_cs_node_t* next;
    do {
        top = s->top;
        if (top == NULL) return 0;
        next = top->next;
    } while (!__sync_bool_compare_and_swap(&s->top, top, next));
    *out_value = top->value;
    __sync_fetch_and_sub(&s->count, 1);
    stack_retire_node(s, top);
    return 1;
}

int32_t rt_concurrent_stack_try_peek(void* stack, void** out_value) {
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)stack;
    rt_cs_node_t* top = s->top;
    if (top == NULL) return 0;
    *out_value = top->value;
    return 1;
}

int32_t rt_concurrent_stack_count(void* stack) {
    return ((rt_concurrent_stack_t*)stack)->count;
}

int32_t rt_concurrent_stack_is_empty(void* stack) {
    return ((rt_concurrent_stack_t*)stack)->top == NULL;
}

void rt_concurrent_stack_clear(void* stack) {
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)stack;
    // Atomically detach top
    rt_cs_node_t* top;
    do {
        top = s->top;
    } while (!__sync_bool_compare_and_swap(&s->top, top, NULL));
    /* clear：调用方保证无并发 Push/Pop，可安全 free */
    while (top) {
        rt_cs_node_t* next = top->next;
        free(top);
        top = next;
    }
    s->count = 0;
}

void rt_concurrent_stack_push_range(void* stack, void* items, int32_t n) {
    if (!stack || !items || n <= 0) return;
    void** ptrs = (void**)items;
    for (int32_t i = 0; i < n; i++) {
        rt_concurrent_stack_push(stack, ptrs[i]);
    }
}

int32_t rt_concurrent_stack_try_pop_range(void* stack, void* out_array, int32_t max_n) {
    if (!stack || !out_array || max_n <= 0) return 0;
    void** out = (void**)out_array;
    int32_t popped = 0;
    for (int32_t i = 0; i < max_n; i++) {
        void* val = NULL;
        if (rt_concurrent_stack_try_pop(stack, &val)) {
            out[i] = val;
            popped++;
        } else {
            break;
        }
    }
    return popped;
}

void* rt_concurrent_stack_to_array(void* stack) {
    if (!stack) return NULL;
    rt_concurrent_stack_t* s = (rt_concurrent_stack_t*)stack;
    // Snapshot: read top atomically, then traverse
    rt_cs_node_t* top;
    do {
        top = s->top;
    } while (!__sync_bool_compare_and_swap(&s->top, top, top));  // atomic read
    // Count nodes
    int32_t cnt = 0;
    for (rt_cs_node_t* cur = top; cur; cur = cur->next) cnt++;
    if (cnt == 0) return rt_array_create(0, (int32_t)sizeof(void*));
    void* arr = rt_array_create(cnt, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    for (rt_cs_node_t* cur = top; cur; cur = cur->next) {
        items[idx++] = cur->value;
    }
    return arr;
}

int32_t rt_concurrent_stack_try_add(void* stack, void* value) {
    rt_concurrent_stack_push(stack, value);
    return 1;
}

int32_t rt_concurrent_stack_try_take(void* stack, void** out_value) {
    return rt_concurrent_stack_try_pop(stack, out_value);
}

void rt_concurrent_stack_copy_to(void* stack, void* dst, int32_t start_idx) {
    void* arr = rt_concurrent_stack_to_array(stack);
    if (!arr) return;
    int32_t len = rt_array_length(arr);
    void** src = (void**)arr;
    void** dst_items = (void**)dst;
    for (int32_t i = 0; i < len; i++) {
        dst_items[start_idx + i] = src[i];
    }
    rt_array_destroy(arr);
}
