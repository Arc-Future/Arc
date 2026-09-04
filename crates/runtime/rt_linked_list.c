// LinkedList<T> runtime ABI (Phase 3).
//
// 双向链表 + 哨兵节点，O(1) 头/尾/中间插入与删除。对齐 C#
// System.Collections.Generic.LinkedList<T>。
//
// 设计契约：
//   - 节点内存由 runtime 拥有；Arc 侧 LinkedListNode<T> 是不透明句柄
//     （即 RtLinkedListNode*），可安全传递与比较。
//   - 元素 ARC 维护：add 时 arc_inc，remove/clear/destroy 时 arc_dec，
//     与 rt_list 的 ARC 槽位回调语义一致。
//   - 相等判定 eq_fn 复用 rt_list_eq_fn 签名（const void* / const void*），
//     允许 codegen 在单态化时复用 List<T> 的 eq 实现。
//   - 哨兵节点 _sentinel.next = first、_sentinel.prev = last；空链表时
//     二者均指向 _sentinel。消除首/尾边界特判。
//
// 与 Arc facade (std/Arc/Collections/LinkedList.as) 的 ABI 契约：
//   rt_linked_list_create  (elem_size, eq_fn, arc_inc, arc_dec) → handle
//   rt_linked_list_destroy (handle)
//   rt_linked_list_add_last / add_first / add_after / add_before → node*
//   rt_linked_list_remove_node (handle, node*) → void
//   rt_linked_list_remove      (handle, elem_ptr) → int32_t (1=removed,0=not found)
//   rt_linked_list_clear       (handle) → void
//   rt_linked_list_first / last → node* (NULL if empty)
//   rt_linked_list_count       (handle) → int32_t
//   rt_linked_list_find / find_last (handle, elem_ptr) → node*
//   rt_linked_list_contains    (handle, elem_ptr) → int32_t
//   rt_linked_list_node_value  (node*, void* out_ptr) → void
//   rt_linked_list_node_prev / node_next (node*) → node*  (Arc 侧 Previous/Next)
//   rt_linked_list_node_list   (node*) → handle          (Arc 侧 List)

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct RtLinkedListNode {
    void*                 value;     /* malloc'd copy of elem_size bytes */
    struct RtLinkedListNode* prev;
    struct RtLinkedListNode* next;
    struct RtLinkedList*   list;     /* 所属链表，用于 Arc 侧 node.List */
} RtLinkedListNode;

typedef struct RtLinkedList {
    RtLinkedListNode   sentinel;     /* 哨兵：sentinel.next=first, sentinel.prev=last */
    int32_t            size;
    int32_t            elem_size;
    rt_list_eq_fn      eq;
    rt_list_arc_fn     arc_inc;
    rt_list_arc_fn     arc_dec;
} RtLinkedList;

/* ---- 内部辅助 ---- */

static void rt_ll_init_sentinel(RtLinkedList* list) {
    list->sentinel.value = NULL;
    list->sentinel.prev  = &list->sentinel;
    list->sentinel.next  = &list->sentinel;
    list->sentinel.list  = list;
}

static RtLinkedListNode* rt_ll_new_node(RtLinkedList* list, const void* elem_ptr) {
    RtLinkedListNode* n = (RtLinkedListNode*)calloc(1, sizeof(RtLinkedListNode));
    if (!n) rt_panic("oom");
    n->value = malloc((size_t)list->elem_size);
    if (!n->value) {
        free(n);
        rt_panic("oom");
    }
    memcpy(n->value, elem_ptr, (size_t)list->elem_size);
    n->list = list;
    /* arc_inc 由调用方在 insert 前调用，此处不再重复 */
    return n;
}

static void rt_ll_link_after(RtLinkedListNode* target, RtLinkedListNode* node) {
    node->prev = target;
    node->next = target->next;
    target->next->prev = node;
    target->next = node;
}

static void rt_ll_link_before(RtLinkedListNode* target, RtLinkedListNode* node) {
    node->next = target;
    node->prev = target->prev;
    target->prev->next = node;
    target->prev = node;
}

static void rt_ll_unlink(RtLinkedListNode* node) {
    node->prev->next = node->next;
    node->next->prev = node->prev;
    node->prev = node->next = NULL;
}

/* 释放节点持有的元素资源（arc_dec + free value）。
 * 节点结构本身由调用方决定是否 free。 */
static void rt_ll_release_value(RtLinkedList* list, RtLinkedListNode* node) {
    if (list->arc_dec && node->value) {
        list->arc_dec(node->value);
    }
    free(node->value);
    node->value = NULL;
}

/* ---- 公共 ABI ---- */

void* rt_linked_list_create(int32_t elem_size, rt_list_eq_fn eq,
                            rt_list_arc_fn arc_inc, rt_list_arc_fn arc_dec) {
    RtLinkedList* list = (RtLinkedList*)calloc(1, sizeof(RtLinkedList));
    if (!list) rt_panic("oom");
    list->size      = 0;
    list->elem_size = elem_size;
    list->eq        = eq;
    list->arc_inc   = arc_inc;
    list->arc_dec   = arc_dec;
    rt_ll_init_sentinel(list);
    return list;
}

void rt_linked_list_destroy(void* handle) {
    if (!handle) return;
    RtLinkedList* list = (RtLinkedList*)handle;
    rt_linked_list_clear(handle);
    free(list);
}

void rt_linked_list_clear(void* handle) {
    if (!handle) return;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* cur = list->sentinel.next;
    while (cur != &list->sentinel) {
        RtLinkedListNode* next = cur->next;
        rt_ll_release_value(list, cur);
        free(cur);
        cur = next;
    }
    rt_ll_init_sentinel(list);
    list->size = 0;
}

int32_t rt_linked_list_count(void* handle) {
    if (!handle) return 0;
    return ((RtLinkedList*)handle)->size;
}

void* rt_linked_list_first(void* handle) {
    if (!handle) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    if (list->size == 0) return NULL;
    return list->sentinel.next;
}

void* rt_linked_list_last(void* handle) {
    if (!handle) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    if (list->size == 0) return NULL;
    return list->sentinel.prev;
}

void* rt_linked_list_add_last(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    if (list->arc_inc) list->arc_inc((void*)elem_ptr);
    RtLinkedListNode* n = rt_ll_new_node(list, elem_ptr);
    rt_ll_link_after(list->sentinel.prev, n);
    list->size++;
    return n;
}

void* rt_linked_list_add_first(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    if (list->arc_inc) list->arc_inc((void*)elem_ptr);
    RtLinkedListNode* n = rt_ll_new_node(list, elem_ptr);
    rt_ll_link_after(&list->sentinel, n);
    list->size++;
    return n;
}

void* rt_linked_list_add_after(void* handle, void* node_handle, const void* elem_ptr) {
    if (!handle || !node_handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* target = (RtLinkedListNode*)node_handle;
    if (target->list != list) {
        rt_panic("linked_list: node does not belong to this list");
    }
    if (list->arc_inc) list->arc_inc((void*)elem_ptr);
    RtLinkedListNode* n = rt_ll_new_node(list, elem_ptr);
    rt_ll_link_after(target, n);
    list->size++;
    return n;
}

void* rt_linked_list_add_before(void* handle, void* node_handle, const void* elem_ptr) {
    if (!handle || !node_handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* target = (RtLinkedListNode*)node_handle;
    if (target->list != list) {
        rt_panic("linked_list: node does not belong to this list");
    }
    if (list->arc_inc) list->arc_inc((void*)elem_ptr);
    RtLinkedListNode* n = rt_ll_new_node(list, elem_ptr);
    rt_ll_link_before(target, n);
    list->size++;
    return n;
}

void rt_linked_list_remove_node(void* handle, void* node_handle) {
    if (!handle || !node_handle) return;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* node = (RtLinkedListNode*)node_handle;
    if (node->list != list) {
        rt_panic("linked_list: node does not belong to this list");
    }
    if (node == &list->sentinel) {
        rt_panic("linked_list: cannot remove sentinel");
    }
    rt_ll_unlink(node);
    rt_ll_release_value(list, node);
    free(node);
    list->size--;
}

int32_t rt_linked_list_remove(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* cur = list->sentinel.next;
    while (cur != &list->sentinel) {
        int32_t matched = 0;
        if (list->eq) {
            matched = list->eq(cur->value, elem_ptr);
        } else {
            matched = memcmp(cur->value, elem_ptr, (size_t)list->elem_size) == 0 ? 1 : 0;
        }
        if (matched) {
            rt_linked_list_remove_node(handle, cur);
            return 1;
        }
        cur = cur->next;
    }
    return 0;
}

void* rt_linked_list_find(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* cur = list->sentinel.next;
    while (cur != &list->sentinel) {
        int32_t matched = 0;
        if (list->eq) {
            matched = list->eq(cur->value, elem_ptr);
        } else {
            matched = memcmp(cur->value, elem_ptr, (size_t)list->elem_size) == 0 ? 1 : 0;
        }
        if (matched) return cur;
        cur = cur->next;
    }
    return NULL;
}

void* rt_linked_list_find_last(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return NULL;
    RtLinkedList* list = (RtLinkedList*)handle;
    RtLinkedListNode* cur = list->sentinel.prev;
    while (cur != &list->sentinel) {
        int32_t matched = 0;
        if (list->eq) {
            matched = list->eq(cur->value, elem_ptr);
        } else {
            matched = memcmp(cur->value, elem_ptr, (size_t)list->elem_size) == 0 ? 1 : 0;
        }
        if (matched) return cur;
        cur = cur->prev;
    }
    return NULL;
}

int32_t rt_linked_list_contains(void* handle, const void* elem_ptr) {
    return rt_linked_list_find(handle, elem_ptr) != NULL ? 1 : 0;
}

/* ---- 节点访问器（供 Arc 侧 LinkedListNode<T> facade 调用） ---- */

void rt_linked_list_node_value(void* node_handle, void* out_ptr) {
    if (!node_handle || !out_ptr) return;
    RtLinkedListNode* n = (RtLinkedListNode*)node_handle;
    if (!n->value) return;
    /* elem_size 由 list 持有，通过 n->list 反查 */
    memcpy(out_ptr, n->value, (size_t)n->list->elem_size);
}

void rt_linked_list_node_set_value(void* node_handle, const void* value_ptr) {
    if (!node_handle || !value_ptr) return;
    RtLinkedListNode* n = (RtLinkedListNode*)node_handle;
    if (!n->value || !n->list) return;
    // ARC dec old value for reference types
    if (n->list->arc_dec) n->list->arc_dec(n->value);
    // Copy new value in
    memcpy(n->value, value_ptr, (size_t)n->list->elem_size);
    // ARC inc new value for reference types
    if (n->list->arc_inc) n->list->arc_inc(n->value);
}

void* rt_linked_list_node_prev(void* node_handle) {
    if (!node_handle) return NULL;
    RtLinkedListNode* n = (RtLinkedListNode*)node_handle;
    if (n->prev == &n->list->sentinel) return NULL;
    return n->prev;
}

void* rt_linked_list_node_next(void* node_handle) {
    if (!node_handle) return NULL;
    RtLinkedListNode* n = (RtLinkedListNode*)node_handle;
    if (n->next == &n->list->sentinel) return NULL;
    return n->next;
}

void* rt_linked_list_node_list(void* node_handle) {
    if (!node_handle) return NULL;
    RtLinkedListNode* n = (RtLinkedListNode*)node_handle;
    return n->list;
}
