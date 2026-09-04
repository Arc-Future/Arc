// SortedSet<T> runtime ABI (Phase 3 · L2 Stable 最小面).
//
// 红黑树实现的有序集合，对齐 C# System.Collections.Generic.SortedSet<T>。
// 按元素排序，O(log n) 插入/查找/删除，O(log n) Min/Max，O(n) 中序遍历产出有序数组。
//
// 设计契约（与 rt_sorted_dict.c 一致）：
//   - 比较函数 cmp_fn 返回 int32_t：<0 表示 a<b，0 表示相等，>0 表示 a>b。
//     标量键以指针位装箱（inttoptr；rt_cmp_int 比较 intptr），string 用 rt_cmp_str。
//   - 节点内存由 runtime 拥有；元素通过 void* 传递，runtime 不维护 ARC。
//   - 红黑树不变量同 CLRS 第 13 章（详见 rt_sorted_dict.c 注释）。
//
// Stable 公开面（SortedSet.as + sorted_set_e2e）：
//   create / destroy / add / contains / remove / min / max / count / clear
//
// C 侧另有 to_array / reverse / view_between / union / intersect / except，
// 但 Arc 公开面已移除对应 API（禁止 IEnumerable/比较器静默 stub）；扩展时另刀。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef int32_t (*rt_cmp_fn)(void* a, void* b);

typedef struct RtSsNode {
    void*             key;
    int32_t           red;
    struct RtSsNode*  parent;
    struct RtSsNode*  left;
    struct RtSsNode*  right;
} RtSsNode;

typedef struct RtSortedSet {
    RtSsNode*  root;
    RtSsNode*  nil;
    int32_t    size;
    int32_t    elem_size;   /* 仅用于 to_array 时元素拷贝；0 表示元素为 void* 句柄 */
    rt_cmp_fn  cmp;
} RtSortedSet;

/* ---- 内部辅助 ---- */

static RtSsNode* rt_ss_new_node(RtSortedSet* s, void* key) {
    RtSsNode* n = (RtSsNode*)calloc(1, sizeof(RtSsNode));
    if (!n) rt_panic("oom");
    n->key    = key;
    n->red    = 1;
    n->parent = s->nil;
    n->left   = s->nil;
    n->right  = s->nil;
    return n;
}

static void rt_ss_free_subtree(RtSortedSet* s, RtSsNode* n) {
    if (n == s->nil) return;
    rt_ss_free_subtree(s, n->left);
    rt_ss_free_subtree(s, n->right);
    free(n);
}

static void rt_ss_inorder(RtSortedSet* s, RtSsNode* n, void** out, int32_t* idx) {
    if (n == s->nil) return;
    rt_ss_inorder(s, n->left, out, idx);
    out[(*idx)++] = n->key;
    rt_ss_inorder(s, n->right, out, idx);
}

/// Collect all keys into a malloc'd array via inorder traversal. Caller must free().
static void** rt_set_collect_keys_sorted(RtSortedSet* s, int32_t* out_count) {
    *out_count = s->size;
    if (*out_count == 0) return NULL;
    void** keys = (void**)malloc((size_t)(*out_count) * sizeof(void*));
    if (!keys) return NULL;
    int32_t idx = 0;
    rt_ss_inorder(s, s->root, keys, &idx);
    return keys;
}

static void rt_ss_left_rotate(RtSortedSet* s, RtSsNode* x) {
    RtSsNode* y = x->right;
    x->right = y->left;
    if (y->left != s->nil) y->left->parent = x;
    y->parent = x->parent;
    if (x->parent == s->nil)       s->root = y;
    else if (x == x->parent->left) x->parent->left  = y;
    else                           x->parent->right = y;
    y->left   = x;
    x->parent = y;
}

static void rt_ss_right_rotate(RtSortedSet* s, RtSsNode* x) {
    RtSsNode* y = x->left;
    x->left  = y->right;
    if (y->right != s->nil) y->right->parent = x;
    y->parent = x->parent;
    if (x->parent == s->nil)        s->root = y;
    else if (x == x->parent->right) x->parent->right = y;
    else                            x->parent->left  = y;
    y->right  = x;
    x->parent = y;
}

static void rt_ss_insert_fixup(RtSortedSet* s, RtSsNode* z) {
    while (z->parent->red) {
        if (z->parent == z->parent->parent->left) {
            RtSsNode* y = z->parent->parent->right;
            if (y->red) {
                z->parent->red = 0;
                y->red         = 0;
                z->parent->parent->red = 1;
                z = z->parent->parent;
            } else {
                if (z == z->parent->right) {
                    z = z->parent;
                    rt_ss_left_rotate(s, z);
                }
                z->parent->red = 0;
                z->parent->parent->red = 1;
                rt_ss_right_rotate(s, z->parent->parent);
            }
        } else {
            RtSsNode* y = z->parent->parent->left;
            if (y->red) {
                z->parent->red = 0;
                y->red         = 0;
                z->parent->parent->red = 1;
                z = z->parent->parent;
            } else {
                if (z == z->parent->left) {
                    z = z->parent;
                    rt_ss_right_rotate(s, z);
                }
                z->parent->red = 0;
                z->parent->parent->red = 1;
                rt_ss_left_rotate(s, z->parent->parent);
            }
        }
    }
    s->root->red = 0;
}

static void rt_ss_delete_fixup(RtSortedSet* s, RtSsNode* x) {
    while (x != s->root && !x->red) {
        if (x == x->parent->left) {
            RtSsNode* w = x->parent->right;
            if (w->red) {
                w->red = 0;
                x->parent->red = 1;
                rt_ss_left_rotate(s, x->parent);
                w = x->parent->right;
            }
            if (!w->left->red && !w->right->red) {
                w->red = 1;
                x = x->parent;
            } else {
                if (!w->right->red) {
                    w->left->red = 0;
                    w->red = 1;
                    rt_ss_right_rotate(s, w);
                    w = x->parent->right;
                }
                w->red = x->parent->red;
                x->parent->red = 0;
                w->right->red  = 0;
                rt_ss_left_rotate(s, x->parent);
                x = s->root;
            }
        } else {
            RtSsNode* w = x->parent->left;
            if (w->red) {
                w->red = 0;
                x->parent->red = 1;
                rt_ss_right_rotate(s, x->parent);
                w = x->parent->left;
            }
            if (!w->right->red && !w->left->red) {
                w->red = 1;
                x = x->parent;
            } else {
                if (!w->left->red) {
                    w->right->red = 0;
                    w->red = 1;
                    rt_ss_left_rotate(s, w);
                    w = x->parent->left;
                }
                w->red = x->parent->red;
                x->parent->red = 0;
                w->left->red   = 0;
                rt_ss_right_rotate(s, x->parent);
                x = s->root;
            }
        }
    }
    x->red = 0;
}

static RtSsNode* rt_ss_min(RtSortedSet* s, RtSsNode* x) {
    while (x->left != s->nil) x = x->left;
    return x;
}

static RtSsNode* rt_ss_max(RtSortedSet* s, RtSsNode* x) {
    while (x->right != s->nil) x = x->right;
    return x;
}

static RtSsNode* rt_ss_search(RtSortedSet* s, void* key) {
    RtSsNode* cur = s->root;
    while (cur != s->nil) {
        int32_t c = s->cmp(key, cur->key);
        if (c == 0) return cur;
        cur = c < 0 ? cur->left : cur->right;
    }
    return NULL;
}

static void rt_ss_transplant(RtSortedSet* s, RtSsNode* u, RtSsNode* v) {
    if (u->parent == s->nil)       s->root = v;
    else if (u == u->parent->left) u->parent->left  = v;
    else                           u->parent->right = v;
    v->parent = u->parent;
}

/* ---- 公共 ABI ---- */

void* rt_sorted_set_create(rt_cmp_fn cmp) {
    RtSortedSet* s = (RtSortedSet*)calloc(1, sizeof(RtSortedSet));
    if (!s) rt_panic("oom");
    s->nil = (RtSsNode*)calloc(1, sizeof(RtSsNode));
    if (!s->nil) {
        free(s);
        rt_panic("oom");
    }
    s->nil->red    = 0;
    s->nil->parent = s->nil;
    s->nil->left   = s->nil;
    s->nil->right  = s->nil;
    s->nil->key    = NULL;
    s->root        = s->nil;
    s->size        = 0;
    s->elem_size   = 0;
    s->cmp         = cmp;
    return s;
}

void rt_sorted_set_destroy(void* handle) {
    if (!handle) return;
    RtSortedSet* s = (RtSortedSet*)handle;
    rt_ss_free_subtree(s, s->root);
    free(s->nil);
    free(s);
}

void rt_sorted_set_clear(void* handle) {
    if (!handle) return;
    RtSortedSet* s = (RtSortedSet*)handle;
    rt_ss_free_subtree(s, s->root);
    s->root = s->nil;
    s->size = 0;
}

int32_t rt_sorted_set_count(void* handle) {
    if (!handle) return 0;
    return ((RtSortedSet*)handle)->size;
}

int32_t rt_sorted_set_add(void* handle, void* key) {
    if (!handle) return 0;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSsNode* y = s->nil;
    RtSsNode* x = s->root;
    int32_t c = 0;
    while (x != s->nil) {
        y = x;
        c = s->cmp(key, x->key);
        if (c == 0) return 0;  /* 已存在 */
        x = c < 0 ? x->left : x->right;
    }
    RtSsNode* z = rt_ss_new_node(s, key);
    z->parent = y;
    if (y == s->nil)      s->root = z;
    else if (c < 0)       y->left  = z;
    else                  y->right = z;
    rt_ss_insert_fixup(s, z);
    s->size++;
    return 1;
}

int32_t rt_sorted_set_contains(void* handle, void* key) {
    if (!handle) return 0;
    return rt_ss_search((RtSortedSet*)handle, key) != NULL ? 1 : 0;
}

int32_t rt_sorted_set_remove(void* handle, void* key) {
    if (!handle) return 0;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSsNode* z = rt_ss_search(s, key);
    if (!z) return 0;

    RtSsNode* y = z;
    RtSsNode* x;
    int32_t y_orig_red = y->red;
    if (z->left == s->nil) {
        x = z->right;
        rt_ss_transplant(s, z, z->right);
    } else if (z->right == s->nil) {
        x = z->left;
        rt_ss_transplant(s, z, z->left);
    } else {
        y = rt_ss_min(s, z->right);
        y_orig_red = y->red;
        x = y->right;
        if (y->parent == z) {
            x->parent = y;
        } else {
            rt_ss_transplant(s, y, y->right);
            y->right = z->right;
            y->right->parent = y;
        }
        rt_ss_transplant(s, z, y);
        y->left = z->left;
        y->left->parent = y;
        y->red = z->red;
    }
    free(z);
    if (!y_orig_red) rt_ss_delete_fixup(s, x);
    s->nil->parent = s->nil;
    s->size--;
    return 1;
}

int32_t rt_sorted_set_min(void* handle, void* out_ptr) {
    if (!handle || !out_ptr) return 0;
    RtSortedSet* s = (RtSortedSet*)handle;
    if (s->size == 0) return 0;
    RtSsNode* n = rt_ss_min(s, s->root);
    *(void**)out_ptr = n->key;
    return 1;
}

int32_t rt_sorted_set_max(void* handle, void* out_ptr) {
    if (!handle || !out_ptr) return 0;
    RtSortedSet* s = (RtSortedSet*)handle;
    if (s->size == 0) return 0;
    RtSsNode* n = rt_ss_max(s, s->root);
    *(void**)out_ptr = n->key;
    return 1;
}

void* rt_sorted_set_to_array(void* handle) {
    if (!handle) return NULL;
    RtSortedSet* s = (RtSortedSet*)handle;
    void* arr = rt_array_create(s->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    rt_ss_inorder(s, s->root, items, &idx);
    return arr;
}

/* ---- reverse enumerator ---- */

typedef struct RtSortedSetEnumerator {
    void** keys;
    int32_t count;
    int32_t index;
    int32_t reverse;  /* 1 = reverse order */
} RtSortedSetEnumerator;

static void* rt_ss_make_enumerator(RtSortedSet* s, int32_t reverse) {
    RtSortedSetEnumerator* e = (RtSortedSetEnumerator*)malloc(sizeof(RtSortedSetEnumerator));
    if (!e) return NULL;
    e->keys = (void**)malloc((size_t)s->size * sizeof(void*));
    if (!e->keys) {
        free(e);
        return NULL;
    }
    int32_t idx = 0;
    rt_ss_inorder(s, s->root, e->keys, &idx);
    e->count = s->size;
    e->index = reverse ? e->count : -1;
    e->reverse = reverse;
    return e;
}

void* rt_sorted_set_reverse(void* handle) {
    if (!handle) return NULL;
    return rt_ss_make_enumerator((RtSortedSet*)handle, 1);
}

/* ---- range view ---- */

/// Collect keys in range [lower, upper] into a new sorted set.
void* rt_sorted_set_view_between(void* handle, void* lower, void* upper) {
    if (!handle) return NULL;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSortedSet* view = (RtSortedSet*)rt_sorted_set_create(s->cmp);
    if (!view) return NULL;
    int32_t count;
    void** all_keys = rt_set_collect_keys_sorted(s, &count);
    if (!all_keys) return view;
    for (int32_t i = 0; i < count; i++) {
        if (s->cmp(all_keys[i], lower) >= 0 && s->cmp(all_keys[i], upper) <= 0) {
            rt_sorted_set_add(view, all_keys[i]);
        }
    }
    free(all_keys);
    return view;
}

/* ---- set operations ---- */

void rt_sorted_set_union(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSortedSet* other = (RtSortedSet*)other_handle;
    int32_t count;
    void** keys = rt_set_collect_keys_sorted(other, &count);
    if (!keys) return;
    for (int32_t i = 0; i < count; i++) {
        rt_sorted_set_add(s, keys[i]);
    }
    free(keys);
}

void rt_sorted_set_intersect(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSortedSet* other = (RtSortedSet*)other_handle;
    int32_t count;
    void** keys = rt_set_collect_keys_sorted(s, &count);
    if (!keys) return;
    for (int32_t i = 0; i < count; i++) {
        if (!rt_sorted_set_contains(other, keys[i])) {
            rt_sorted_set_remove(s, keys[i]);
        }
    }
    free(keys);
}

void rt_sorted_set_except(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSortedSet* s = (RtSortedSet*)handle;
    RtSortedSet* other = (RtSortedSet*)other_handle;
    int32_t count;
    void** keys = rt_set_collect_keys_sorted(other, &count);
    if (!keys) return;
    for (int32_t i = 0; i < count; i++) {
        rt_sorted_set_remove(s, keys[i]);
    }
    free(keys);
}
