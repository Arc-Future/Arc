// SortedDictionary<K, V> runtime ABI (Phase 3 · L2 Stable 最小面).
//
// 红黑树实现的有序映射，对齐 C# System.Collections.Generic.SortedDictionary<K, V>。
// 按键排序，O(log n) 插入/查找/删除；Keys/Values 中序遍历产出有序数组（C 侧预留）。
//
// 设计契约：
//   - 比较函数 cmp_fn 返回 int32_t： <0 表示 a<b，0 表示相等，>0 表示 a>b。
//     标量键以指针位装箱（inttoptr；rt_cmp_int 比较 intptr），string 用 rt_cmp_str。
//   - 节点内存由 runtime 拥有；key/value 通过 void* 传递，runtime 不维护 ARC。
//   - 红黑树不变量同 CLRS 第 13 章；sentinel NIL 简化边界处理。
//
// Stable 公开面（SortedDictionary.as + sorted_dictionary_e2e）：
//   create / destroy / get / set / add / try_get / remove / contains / count / clear
//
// C 侧另有 keys / values（中序数组）；Arc 公开面已移除对应 API（禁静默 stub）；扩展时另刀。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef int32_t (*rt_cmp_fn)(void* a, void* b);

typedef struct RtSdNode {
    void*             key;
    void*             value;
    int32_t           red;        /* 1=red, 0=black */
    struct RtSdNode*  parent;
    struct RtSdNode*  left;
    struct RtSdNode*  right;
} RtSdNode;

typedef struct RtSortedDict {
    RtSdNode*  root;
    RtSdNode*  nil;            /* 哨兵 NIL，所有叶子指向它 */
    int32_t    size;
    rt_cmp_fn  cmp;
} RtSortedDict;

/* ---- 内部辅助 ---- */

static RtSdNode* rt_sd_new_node(RtSortedDict* d, void* key, void* value) {
    RtSdNode* n = (RtSdNode*)calloc(1, sizeof(RtSdNode));
    if (!n) rt_panic("oom");
    n->key    = key;
    n->value  = value;
    n->red    = 1;  /* 新插入节点为红 */
    n->parent = d->nil;
    n->left   = d->nil;
    n->right  = d->nil;
    return n;
}

static void rt_sd_free_subtree(RtSortedDict* d, RtSdNode* n) {
    if (n == d->nil) return;
    rt_sd_free_subtree(d, n->left);
    rt_sd_free_subtree(d, n->right);
    free(n);
}

/* 中序遍历收集 key/value 到数组 */
static void rt_sd_inorder_keys(RtSortedDict* d, RtSdNode* n, void** out, int32_t* idx) {
    if (n == d->nil) return;
    rt_sd_inorder_keys(d, n->left, out, idx);
    out[(*idx)++] = n->key;
    rt_sd_inorder_keys(d, n->right, out, idx);
}

static void rt_sd_inorder_values(RtSortedDict* d, RtSdNode* n, void** out, int32_t* idx) {
    if (n == d->nil) return;
    rt_sd_inorder_values(d, n->left, out, idx);
    out[(*idx)++] = n->value;
    rt_sd_inorder_values(d, n->right, out, idx);
}

/* 标准红黑树旋转（CLRS 13.2） */
static void rt_sd_left_rotate(RtSortedDict* d, RtSdNode* x) {
    RtSdNode* y = x->right;
    x->right = y->left;
    if (y->left != d->nil) y->left->parent = x;
    y->parent = x->parent;
    if (x->parent == d->nil)      d->root = y;
    else if (x == x->parent->left) x->parent->left  = y;
    else                           x->parent->right = y;
    y->left   = x;
    x->parent = y;
}

static void rt_sd_right_rotate(RtSortedDict* d, RtSdNode* x) {
    RtSdNode* y = x->left;
    x->left  = y->right;
    if (y->right != d->nil) y->right->parent = x;
    y->parent = x->parent;
    if (x->parent == d->nil)       d->root = y;
    else if (x == x->parent->right) x->parent->right = y;
    else                            x->parent->left  = y;
    y->right  = x;
    x->parent = y;
}

/* 插入修复（CLRS 13.3） */
static void rt_sd_insert_fixup(RtSortedDict* d, RtSdNode* z) {
    while (z->parent->red) {
        if (z->parent == z->parent->parent->left) {
            RtSdNode* y = z->parent->parent->right;
            if (y->red) {
                z->parent->red = 0;
                y->red         = 0;
                z->parent->parent->red = 1;
                z = z->parent->parent;
            } else {
                if (z == z->parent->right) {
                    z = z->parent;
                    rt_sd_left_rotate(d, z);
                }
                z->parent->red = 0;
                z->parent->parent->red = 1;
                rt_sd_right_rotate(d, z->parent->parent);
            }
        } else {
            RtSdNode* y = z->parent->parent->left;
            if (y->red) {
                z->parent->red = 0;
                y->red         = 0;
                z->parent->parent->red = 1;
                z = z->parent->parent;
            } else {
                if (z == z->parent->left) {
                    z = z->parent;
                    rt_sd_right_rotate(d, z);
                }
                z->parent->red = 0;
                z->parent->parent->red = 1;
                rt_sd_left_rotate(d, z->parent->parent);
            }
        }
    }
    d->root->red = 0;
}

/* 删除修复（CLRS 13.4） */
static void rt_sd_delete_fixup(RtSortedDict* d, RtSdNode* x) {
    while (x != d->root && !x->red) {
        if (x == x->parent->left) {
            RtSdNode* w = x->parent->right;
            if (w->red) {
                w->red = 0;
                x->parent->red = 1;
                rt_sd_left_rotate(d, x->parent);
                w = x->parent->right;
            }
            if (!w->left->red && !w->right->red) {
                w->red = 1;
                x = x->parent;
            } else {
                if (!w->right->red) {
                    w->left->red = 0;
                    w->red = 1;
                    rt_sd_right_rotate(d, w);
                    w = x->parent->right;
                }
                w->red = x->parent->red;
                x->parent->red = 0;
                w->right->red  = 0;
                rt_sd_left_rotate(d, x->parent);
                x = d->root;
            }
        } else {
            RtSdNode* w = x->parent->left;
            if (w->red) {
                w->red = 0;
                x->parent->red = 1;
                rt_sd_right_rotate(d, x->parent);
                w = x->parent->left;
            }
            if (!w->right->red && !w->left->red) {
                w->red = 1;
                x = x->parent;
            } else {
                if (!w->left->red) {
                    w->right->red = 0;
                    w->red = 1;
                    rt_sd_left_rotate(d, w);
                    w = x->parent->left;
                }
                w->red = x->parent->red;
                x->parent->red = 0;
                w->left->red   = 0;
                rt_sd_right_rotate(d, x->parent);
                x = d->root;
            }
        }
    }
    x->red = 0;
}

static RtSdNode* rt_sd_min(RtSortedDict* d, RtSdNode* x) {
    while (x->left != d->nil) x = x->left;
    return x;
}

static RtSdNode* rt_sd_search(RtSortedDict* d, void* key) {
    RtSdNode* cur = d->root;
    while (cur != d->nil) {
        int32_t c = d->cmp(key, cur->key);
        if (c == 0) return cur;
        cur = c < 0 ? cur->left : cur->right;
    }
    return NULL;
}

/* 移植子树 u → v（CLRS RB-DELETE） */
static void rt_sd_transplant(RtSortedDict* d, RtSdNode* u, RtSdNode* v) {
    if (u->parent == d->nil)      d->root = v;
    else if (u == u->parent->left) u->parent->left  = v;
    else                           u->parent->right = v;
    v->parent = u->parent;
}

/* ---- 公共 ABI ---- */

void* rt_sorted_dict_create(rt_cmp_fn cmp) {
    RtSortedDict* d = (RtSortedDict*)calloc(1, sizeof(RtSortedDict));
    if (!d) rt_panic("oom");
    d->nil = (RtSdNode*)calloc(1, sizeof(RtSdNode));
    if (!d->nil) {
        free(d);
        rt_panic("oom");
    }
    d->nil->red    = 0;
    d->nil->parent = d->nil;
    d->nil->left   = d->nil;
    d->nil->right  = d->nil;
    d->nil->key    = NULL;
    d->nil->value  = NULL;
    d->root        = d->nil;
    d->size        = 0;
    d->cmp         = cmp;
    return d;
}

void rt_sorted_dict_destroy(void* handle) {
    if (!handle) return;
    RtSortedDict* d = (RtSortedDict*)handle;
    rt_sd_free_subtree(d, d->root);
    free(d->nil);
    free(d);
}

void rt_sorted_dict_clear(void* handle) {
    if (!handle) return;
    RtSortedDict* d = (RtSortedDict*)handle;
    rt_sd_free_subtree(d, d->root);
    d->root = d->nil;
    d->size = 0;
}

int32_t rt_sorted_dict_count(void* handle) {
    if (!handle) return 0;
    return ((RtSortedDict*)handle)->size;
}

int32_t rt_sorted_dict_contains(void* handle, void* key) {
    if (!handle) return 0;
    return rt_sd_search((RtSortedDict*)handle, key) != NULL ? 1 : 0;
}

void* rt_sorted_dict_get(void* handle, void* key) {
    if (!handle) return NULL;
    RtSdNode* n = rt_sd_search((RtSortedDict*)handle, key);
    return n ? n->value : NULL;
}

int32_t rt_sorted_dict_try_get(void* handle, void* key, void** out_value) {
    if (!handle || !out_value) return 0;
    RtSdNode* n = rt_sd_search((RtSortedDict*)handle, key);
    if (!n) {
        *out_value = NULL;
        return 0;
    }
    *out_value = n->value;
    return 1;
}

/* 内部插入：返回新节点指针与是否新增。
 * is_overwrite=1 时覆盖已有 value，不增加 size。 */
static RtSdNode* rt_sd_insert(RtSortedDict* d, void* key, void* value, int32_t overwrite) {
    RtSdNode* y = d->nil;
    RtSdNode* x = d->root;
    int32_t c = 0;
    while (x != d->nil) {
        y = x;
        c = d->cmp(key, x->key);
        if (c == 0) {
            if (overwrite) x->value = value;
            return x;
        }
        x = c < 0 ? x->left : x->right;
    }
    RtSdNode* z = rt_sd_new_node(d, key, value);
    z->parent = y;
    if (y == d->nil)       d->root = z;
    else if (c < 0)        y->left  = z;
    else                   y->right = z;
    rt_sd_insert_fixup(d, z);
    d->size++;
    return z;
}

void rt_sorted_dict_set(void* handle, void* key, void* value) {
    if (!handle) return;
    rt_sd_insert((RtSortedDict*)handle, key, value, 1);
}

int32_t rt_sorted_dict_add(void* handle, void* key, void* value) {
    if (!handle) return 0;
    RtSortedDict* d = (RtSortedDict*)handle;
    if (rt_sd_search(d, key)) return 0;
    rt_sd_insert(d, key, value, 0);
    return 1;
}

int32_t rt_sorted_dict_remove(void* handle, void* key) {
    if (!handle) return 0;
    RtSortedDict* d = (RtSortedDict*)handle;
    RtSdNode* z = rt_sd_search(d, key);
    if (!z) return 0;

    RtSdNode* y = z;
    RtSdNode* x;
    int32_t y_orig_red = y->red;
    if (z->left == d->nil) {
        x = z->right;
        rt_sd_transplant(d, z, z->right);
    } else if (z->right == d->nil) {
        x = z->left;
        rt_sd_transplant(d, z, z->left);
    } else {
        y = rt_sd_min(d, z->right);
        y_orig_red = y->red;
        x = y->right;
        if (y->parent == z) {
            x->parent = y;
        } else {
            rt_sd_transplant(d, y, y->right);
            y->right = z->right;
            y->right->parent = y;
        }
        rt_sd_transplant(d, z, y);
        y->left = z->left;
        y->left->parent = y;
        y->red = z->red;
    }
    free(z);
    if (!y_orig_red) rt_sd_delete_fixup(d, x);
    /* 修复 nil->parent 防止悬挂指针 */
    d->nil->parent = d->nil;
    d->size--;
    return 1;
}

void* rt_sorted_dict_keys(void* handle) {
    if (!handle) return NULL;
    RtSortedDict* d = (RtSortedDict*)handle;
    void* arr = rt_array_create(d->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    rt_sd_inorder_keys(d, d->root, items, &idx);
    return arr;
}

void* rt_sorted_dict_values(void* handle) {
    if (!handle) return NULL;
    RtSortedDict* d = (RtSortedDict*)handle;
    void* arr = rt_array_create(d->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    rt_sd_inorder_values(d, d->root, items, &idx);
    return arr;
}
