// HashSet<T> runtime ABI (RFC Phase 5).
//
// Contiguous entry table + index chaining (same shape as .NET HashSet):
// buckets hold entry indices; collisions link via entry.next. New inserts
// grow the entry array 2× instead of malloc'ing one node per key.
//
// G8 续刀（feat/hashset-soa-oa · RFC 005 §3.5 / §3.7）：
//   - 试 SoA 开放寻址：成对 A/B **变慢**（链 ~7.2 → OA ~9.1 ns/op）→ 撤回
//   - int_keys 内联 + identity 与 rt_hash_int 对齐；初桶/表 64（H2 续刀）
//   - entry 仍 16B；公开 ABI 不变
// H2 续刀：entry 存 hashCode；int_keys 桶 4× grow；初桶/表 256 + entry 4×（raw≤0.85）
//
// P0 续刀（free-list 负值编码 + int_keys 12B 紧凑 entry）：
//   - 仿 .NET StartOfFreeList = -3：Remove 写 `next = -3 - free_head`，
//     alloc 弹空位 `free_head = -3 - next`；活项 `next >= -1`，free 项 `next < -1`。
//   - free-list 头不再占独立 struct 字段，存在 `buckets[bucket_count]`（保留槽；
//     桶数为 2 的幂，该索引永不命中位掩码）。
//   - 枚举/集合运算改原地扫 entries（`next >= -1` 判活），免 malloc 快照。
//   - int_keys（≤32 位标量键）用紧凑 entry {hashCode, next, int32 key}（12B）；
//     long/double/引用类型仍 void* 槽。codegen box 为零扩展，低 32 位即原值。
//
// Bucket count stays a power of two so lookup uses bitmask, not modulo.
// Grows by 2× when load factor exceeds 0.75（int_keys 4×）。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

#define RT_SET_INITIAL_BUCKETS 256
#define RT_SET_INITIAL_ENTRIES 256
#define RT_SET_LOAD_FACTOR_NUM 3
#define RT_SET_LOAD_FACTOR_DEN 4
#define RT_SET_EMPTY (-1)
#define RT_SET_START_OF_FREE_LIST (-3)

typedef uint32_t (*rt_hash_fn)(void* key);
typedef int32_t  (*rt_eq_fn)(void* a, void* b);

/* int_keys 紧凑 entry：{hashCode, next, int32 key} = 12B。 */
typedef struct RtSetEntryInt {
    int32_t hashCode;
    int32_t next;     /* next collision index, or RT_SET_EMPTY / free-list link */
    int32_t key;      /* (int32_t)(intptr_t)key — ≤32-bit scalar key bits */
} RtSetEntryInt;

/* 通用 entry：{hashCode, next, void* key} = 16B。 */
typedef struct RtSetEntryGen {
    int32_t hashCode;
    int32_t next;
    void*   key;
} RtSetEntryGen;

typedef struct RtSet {
    int32_t*     buckets;     /* (bucket_count+1) indices into entries, or EMPTY;
                                 [bucket_count] = free-list head (EMPTY when none) */
    void*        entries;     /* RtSetEntryInt* (int_keys) or RtSetEntryGen* (gen) */
    int32_t      bucket_count;
    int32_t      size;        /* live key count */
    int32_t      entry_cap;
    int32_t      next_free;   /* next never-used slot in entries[0..entry_cap) */
    rt_hash_fn   hash;
    rt_eq_fn     eq;
    int32_t      int_keys;    /* 1 when hash/eq are rt_hash_int/rt_eq_int */
} RtSet;

/* ---- typed entry accessors ---- */

static RtSetEntryInt* rt_set_ent_int(RtSet* s) { return (RtSetEntryInt*)s->entries; }
static RtSetEntryGen* rt_set_ent_gen(RtSet* s) { return (RtSetEntryGen*)s->entries; }

static int32_t rt_set_ent_next(RtSet* s, int32_t idx) {
    return s->int_keys ? rt_set_ent_int(s)[idx].next : rt_set_ent_gen(s)[idx].next;
}

static void rt_set_ent_set_next(RtSet* s, int32_t idx, int32_t v) {
    if (s->int_keys) {
        rt_set_ent_int(s)[idx].next = v;
    } else {
        rt_set_ent_gen(s)[idx].next = v;
    }
}

static int32_t rt_set_ent_hash(RtSet* s, int32_t idx) {
    return s->int_keys ? rt_set_ent_int(s)[idx].hashCode : rt_set_ent_gen(s)[idx].hashCode;
}

/* 读回键：int_keys 零扩展（匹配 codegen inttoptr 装箱），gen 原样。 */
static void* rt_set_ent_key(RtSet* s, int32_t idx) {
    if (s->int_keys) {
        int32_t k = rt_set_ent_int(s)[idx].key;
        return (void*)(uintptr_t)(uint32_t)k;
    }
    return rt_set_ent_gen(s)[idx].key;
}

/* ---- internal helpers ---- */

static uint32_t rt_set_hash_key(RtSet* s, void* key) {
    if (s->int_keys) {
        /* Match rt_hash_int identity (H2 · .NET Int32.GetHashCode). */
        return (uint32_t)(uintptr_t)key;
    }
    return s->hash ? s->hash(key) : 0;
}

static int32_t rt_set_should_resize(RtSet* s) {
    return (s->size + 1) * RT_SET_LOAD_FACTOR_DEN
           > s->bucket_count * RT_SET_LOAD_FACTOR_NUM;
}

static void rt_set_ensure_entry_cap(RtSet* s, int32_t needed) {
    if (needed <= s->entry_cap) return;
    int32_t grow = s->int_keys ? 4 : 2;
    int32_t new_cap = s->entry_cap > 0 ? s->entry_cap * grow : RT_SET_INITIAL_ENTRIES;
    if (new_cap < needed) new_cap = needed;
    size_t esz = s->int_keys ? sizeof(RtSetEntryInt) : sizeof(RtSetEntryGen);
    void* neu = realloc(s->entries, (size_t)new_cap * esz);
    if (!neu) rt_panic("oom");
    s->entries = neu;
    s->entry_cap = new_cap;
}

static int32_t rt_set_alloc_entry(RtSet* s) {
    int32_t head = s->buckets[s->bucket_count];
    if (head != RT_SET_EMPTY) {
        int32_t idx = head;
        s->buckets[s->bucket_count] = RT_SET_START_OF_FREE_LIST - rt_set_ent_next(s, idx);
        return idx;
    }
    rt_set_ensure_entry_cap(s, s->next_free + 1);
    return s->next_free++;
}

static void rt_set_resize(RtSet* s, int32_t new_count) {
    int32_t* new_buckets = (int32_t*)malloc((size_t)(new_count + 1) * sizeof(int32_t));
    if (!new_buckets) rt_panic("oom");
    for (int32_t i = 0; i <= new_count; i++) {
        new_buckets[i] = RT_SET_EMPTY;
    }
    uint32_t mask = (uint32_t)(new_count - 1);
    for (int32_t b = 0; b < s->bucket_count; b++) {
        int32_t idx = s->buckets[b];
        while (idx != RT_SET_EMPTY) {
            int32_t next = rt_set_ent_next(s, idx);
            uint32_t nb = (uint32_t)rt_set_ent_hash(s, idx) & mask;
            rt_set_ent_set_next(s, idx, new_buckets[nb]);
            new_buckets[nb] = idx;
            idx = next;
        }
    }
    new_buckets[new_count] = s->buckets[s->bucket_count]; /* carry free-list head */
    free(s->buckets);
    s->buckets = new_buckets;
    s->bucket_count = new_count;
}

static int32_t rt_set_find(RtSet* s, void* key, uint32_t* out_bucket) {
    uint32_t h = rt_set_hash_key(s, key);
    uint32_t b = h & (uint32_t)(s->bucket_count - 1);
    if (out_bucket) *out_bucket = b;
    int32_t ih = (int32_t)h;
    if (s->int_keys) {
        RtSetEntryInt* ent = rt_set_ent_int(s);
        int32_t ikey = (int32_t)(intptr_t)key;
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; idx = ent[idx].next) {
            if (ent[idx].hashCode == ih && ent[idx].key == ikey) return idx;
        }
    } else {
        RtSetEntryGen* ent = rt_set_ent_gen(s);
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; idx = ent[idx].next) {
            if (ent[idx].hashCode == ih && s->eq(ent[idx].key, key)) return idx;
        }
    }
    return RT_SET_EMPTY;
}

static int32_t rt_set_remove_key(RtSet* s, void* key) {
    uint32_t h = rt_set_hash_key(s, key);
    uint32_t b = h & (uint32_t)(s->bucket_count - 1);
    int32_t ih = (int32_t)h;
    int32_t prev = RT_SET_EMPTY;
    if (s->int_keys) {
        RtSetEntryInt* ent = rt_set_ent_int(s);
        int32_t ikey = (int32_t)(intptr_t)key;
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; ) {
            int32_t next = ent[idx].next;
            if (ent[idx].hashCode == ih && ent[idx].key == ikey) {
                if (prev == RT_SET_EMPTY) {
                    s->buckets[b] = next;
                } else {
                    ent[prev].next = next;
                }
                ent[idx].hashCode = 0;
                ent[idx].key = 0;
                ent[idx].next = RT_SET_START_OF_FREE_LIST - s->buckets[s->bucket_count];
                s->buckets[s->bucket_count] = idx;
                s->size--;
                return 1;
            }
            prev = idx;
            idx = next;
        }
    } else {
        RtSetEntryGen* ent = rt_set_ent_gen(s);
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; ) {
            int32_t next = ent[idx].next;
            if (ent[idx].hashCode == ih && s->eq(ent[idx].key, key)) {
                if (prev == RT_SET_EMPTY) {
                    s->buckets[b] = next;
                } else {
                    ent[prev].next = next;
                }
                ent[idx].hashCode = 0;
                ent[idx].key = NULL;
                ent[idx].next = RT_SET_START_OF_FREE_LIST - s->buckets[s->bucket_count];
                s->buckets[s->bucket_count] = idx;
                s->size--;
                return 1;
            }
            prev = idx;
            idx = next;
        }
    }
    return 0;
}

static int32_t rt_set_add_key(RtSet* s, void* key) {
    uint32_t h = rt_set_hash_key(s, key);
    uint32_t b = h & (uint32_t)(s->bucket_count - 1);
    int32_t ih = (int32_t)h;
    if (s->int_keys) {
        RtSetEntryInt* ent = rt_set_ent_int(s);
        int32_t ikey = (int32_t)(intptr_t)key;
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; idx = ent[idx].next) {
            if (ent[idx].hashCode == ih && ent[idx].key == ikey) return 0;
        }
    } else {
        RtSetEntryGen* ent = rt_set_ent_gen(s);
        for (int32_t idx = s->buckets[b]; idx != RT_SET_EMPTY; idx = ent[idx].next) {
            if (ent[idx].hashCode == ih && s->eq(ent[idx].key, key)) return 0;
        }
    }
    if (rt_set_should_resize(s)) {
        int32_t grow = s->int_keys ? (s->bucket_count * 4) : (s->bucket_count * 2);
        rt_set_resize(s, grow);
        b = h & (uint32_t)(s->bucket_count - 1);
    }
    int32_t idx = rt_set_alloc_entry(s);
    if (s->int_keys) {
        RtSetEntryInt* ent = rt_set_ent_int(s);
        ent[idx].hashCode = ih;
        ent[idx].key = (int32_t)(intptr_t)key;
        ent[idx].next = s->buckets[b];
    } else {
        RtSetEntryGen* ent = rt_set_ent_gen(s);
        ent[idx].hashCode = ih;
        ent[idx].key = key;
        ent[idx].next = s->buckets[b];
    }
    s->buckets[b] = idx;
    s->size++;
    return 1;
}

static int32_t rt_set_contains_key(RtSet* s, void* key) {
    return rt_set_find(s, key, NULL) != RT_SET_EMPTY;
}

/* ---- public ABI ---- */

void* rt_set_create(rt_hash_fn hash, rt_eq_fn eq) {
    RtSet* s = (RtSet*)calloc(1, sizeof(RtSet));
    if (!s) return NULL;
    s->int_keys = (hash == rt_hash_int && eq == rt_eq_int) ? 1 : 0;
    s->buckets = (int32_t*)malloc((RT_SET_INITIAL_BUCKETS + 1) * sizeof(int32_t));
    if (!s->buckets) {
        free(s);
        return NULL;
    }
    for (int32_t i = 0; i <= RT_SET_INITIAL_BUCKETS; i++) {
        s->buckets[i] = RT_SET_EMPTY;
    }
    size_t esz = s->int_keys ? sizeof(RtSetEntryInt) : sizeof(RtSetEntryGen);
    s->entries = malloc(RT_SET_INITIAL_ENTRIES * esz);
    if (!s->entries) {
        free(s->buckets);
        free(s);
        return NULL;
    }
    s->bucket_count = RT_SET_INITIAL_BUCKETS;
    s->size = 0;
    s->entry_cap = RT_SET_INITIAL_ENTRIES;
    s->next_free = 0;
    s->hash = hash;
    s->eq = eq;
    return s;
}

void rt_set_destroy(void* handle) {
    if (!handle) return;
    RtSet* s = (RtSet*)handle;
    free(s->buckets);
    free(s->entries);
    free(s);
}

void rt_set_ensure_capacity(void* handle, int32_t capacity) {
    if (!handle || capacity <= 0) return;
    RtSet* s = (RtSet*)handle;
    if (capacity > s->entry_cap) {
        rt_set_ensure_entry_cap(s, capacity);
    }
    /* 保持负载因子 ≤ 0.75 的最小 2 次幂桶数（对齐 rt_set_add_key 的 grow 语义）。 */
    int64_t need = ((int64_t)capacity * RT_SET_LOAD_FACTOR_DEN)
                   / RT_SET_LOAD_FACTOR_NUM + 1;
    int32_t buckets = 1;
    while (buckets < need) buckets <<= 1;
    if (buckets > s->bucket_count) {
        rt_set_resize(s, buckets);
    }
}

int32_t rt_set_add(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtSet* s = (RtSet*)handle;
    void* key = *(void* const*)elem_ptr;
    return rt_set_add_key(s, key);
}

int32_t rt_set_contains(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtSet* s = (RtSet*)handle;
    void* key = *(void* const*)elem_ptr;
    return rt_set_contains_key(s, key);
}

int32_t rt_set_remove(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtSet* s = (RtSet*)handle;
    void* key = *(void* const*)elem_ptr;
    return rt_set_remove_key(s, key);
}

int32_t rt_set_count(void* handle) {
    if (!handle) return 0;
    return ((RtSet*)handle)->size;
}

void rt_set_clear(void* handle) {
    if (!handle) return;
    RtSet* s = (RtSet*)handle;
    for (int32_t i = 0; i <= s->bucket_count; i++) {
        s->buckets[i] = RT_SET_EMPTY;
    }
    s->size = 0;
    s->next_free = 0;
}

/* ---- set operations（原地扫 entries，`next >= -1` 判活，免快照）---- */

void rt_set_union_with(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    for (int32_t i = 0; i < other->next_free; i++) {
        if (rt_set_ent_next(other, i) >= RT_SET_EMPTY) {
            rt_set_add_key(s, rt_set_ent_key(other, i));
        }
    }
}

void rt_set_intersect_with(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    for (int32_t i = 0; i < s->next_free; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            void* key = rt_set_ent_key(s, i);
            if (!rt_set_contains_key(other, key)) {
                rt_set_remove_key(s, key);
            }
        }
    }
}

void rt_set_except_with(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    for (int32_t i = 0; i < other->next_free; i++) {
        if (rt_set_ent_next(other, i) >= RT_SET_EMPTY) {
            rt_set_remove_key(s, rt_set_ent_key(other, i));
        }
    }
}

void rt_set_symmetric_except_with(void* handle, void* other_handle) {
    if (!handle || !other_handle) return;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    /* pass 1：s 中属于 other 的元素移除（s = s \ other） */
    for (int32_t i = 0; i < s->next_free; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            void* key = rt_set_ent_key(s, i);
            if (rt_set_contains_key(other, key)) {
                rt_set_remove_key(s, key);
            }
        }
    }
    /* pass 2：other 中不属于 s 的元素并入（other \ s） */
    for (int32_t i = 0; i < other->next_free; i++) {
        if (rt_set_ent_next(other, i) >= RT_SET_EMPTY) {
            void* key = rt_set_ent_key(other, i);
            if (!rt_set_contains_key(s, key)) {
                rt_set_add_key(s, key);
            }
        }
    }
}

int32_t rt_set_is_subset_of(void* handle, void* other_handle) {
    if (!handle || !other_handle) return 0;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    for (int32_t i = 0; i < s->next_free; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            if (!rt_set_contains_key(other, rt_set_ent_key(s, i))) return 0;
        }
    }
    return 1;
}

int32_t rt_set_is_superset_of(void* handle, void* other_handle) {
    return rt_set_is_subset_of(other_handle, handle);
}

int32_t rt_set_is_proper_subset_of(void* handle, void* other_handle) {
    if (!handle || !other_handle) return 0;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    if (s->size >= other->size) return 0;
    return rt_set_is_subset_of(handle, other_handle);
}

int32_t rt_set_is_proper_superset_of(void* handle, void* other_handle) {
    if (!handle || !other_handle) return 0;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    if (s->size <= other->size) return 0;
    return rt_set_is_superset_of(handle, other_handle);
}

int32_t rt_set_overlaps(void* handle, void* other_handle) {
    if (!handle || !other_handle) return 0;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    for (int32_t i = 0; i < s->next_free; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            if (rt_set_contains_key(other, rt_set_ent_key(s, i))) return 1;
        }
    }
    return 0;
}

int32_t rt_set_set_equals(void* handle, void* other_handle) {
    if (!handle || !other_handle) return 0;
    RtSet* s = (RtSet*)handle;
    RtSet* other = (RtSet*)other_handle;
    if (s->size != other->size) return 0;
    return rt_set_is_subset_of(handle, other_handle);
}

void* rt_set_to_array(void* handle) {
    if (!handle) return NULL;
    RtSet* s = (RtSet*)handle;
    void* arr = rt_array_create(s->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    for (int32_t i = 0; i < s->next_free && idx < s->size; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            items[idx++] = rt_set_ent_key(s, i);
        }
    }
    return arr;
}

int32_t rt_set_get(void* handle, int32_t index, void* out_elem) {
    if (!handle || !out_elem || index < 0) return 0;
    RtSet* s = (RtSet*)handle;
    /* 与 rt_set_to_array 同序（内部桶序，稳定但非插入序）——HashSet 无
     * 语义序，索引脱糖（foreach）仅要求逐元素可枚举即可。 */
    int32_t seen = 0;
    for (int32_t i = 0; i < s->next_free; i++) {
        if (rt_set_ent_next(s, i) >= RT_SET_EMPTY) {
            if (seen == index) {
                *(void**)out_elem = rt_set_ent_key(s, i);
                return 1;
            }
            seen++;
        }
    }
    return 0;
}

typedef struct RtSetEnumerator {
    RtSet* set;
    int32_t idx;
} RtSetEnumerator;

void* rt_set_get_enumerator(void* handle) {
    if (!handle) return NULL;
    RtSet* s = (RtSet*)handle;
    RtSetEnumerator* e = (RtSetEnumerator*)calloc(1, sizeof(RtSetEnumerator));
    if (!e) return NULL;
    e->set = s;
    e->idx = -1;
    return e;
}

int32_t rt_set_enumerator_move_next(void* handle) {
    if (!handle) return 0;
    RtSetEnumerator* e = (RtSetEnumerator*)handle;
    RtSet* s = e->set;
    while (++e->idx < s->next_free) {
        if (rt_set_ent_next(s, e->idx) >= RT_SET_EMPTY) return 1;
    }
    return 0;
}

void* rt_set_enumerator_current(void* handle) {
    if (!handle) return NULL;
    RtSetEnumerator* e = (RtSetEnumerator*)handle;
    RtSet* s = e->set;
    if (e->idx < 0 || e->idx >= s->next_free) return NULL;
    if (rt_set_ent_next(s, e->idx) < RT_SET_EMPTY) return NULL;
    return rt_set_ent_key(s, e->idx);
}
