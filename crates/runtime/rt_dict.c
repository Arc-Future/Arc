// Dictionary<K,V> runtime ABI with dynamic resizing (RFC 015 Phase B).
//
// G8 续刀（feat/dict-hotpath-next）：开放寻址 + SoA（hashes/keys/values 分列）。
// 探测只扫密集 uint32 hash 数组（~16 槽/缓存行），命中后再读 key；
// 整键 `rt_hash_int`/`rt_eq_int` 内联；int hash = identity（对齐 .NET Int32.GetHashCode）；
// int_keys 扩容 4×、初容 64；键列 int32（值列始终 void*——引用/装箱不可截断）；公开 ABI 仍 inttoptr；无 unsafe。
//
// P0：long/ulong/double 键修复高位截断（int_keys 细化为 int32/int64 两档）：
//   - int_keys==1：hash/eq = rt_hash_int/rt_eq_int，键列 int32（identity hash）；
//   - int_keys==2：hash = rt_hash_long + eq = rt_eq_int，键列 int64（全 64 位键，
//     避免 `0x1_0000_0001` vs `0x2_0000_0001` 仅高位不同被误判相等导致数据丢失）；
//   - int_keys==0：通用 void* 键列（string / 用户类型 / 引用）。
//
// hash 编码：0=空，1=墓碑，≥2=占用（缓存 hash）。
// 负载因子超过 0.75 时 2× 扩容（int_keys 4×）。容量 2 的幂（位掩码）。
//
// RFC 051 S3b：值所有权两档——
//   rt_dict_create（legacy）：runtime 不维护值 ARC（标量 inttoptr / string char*）；
//   rt_dict_create_owned：值必须是 ArcHeader 对象（class / 接口 fat 盒）。插入时
//     存储侧 rt_arc_inc；set 覆盖旧值、remove、clear、destroy 时对被移除条目值
//     rt_arc_dec（空值安全；先清槽再 dec，防 finalizer 重入读到半死槽）。
//     旧版 create 的 blob 释放语义不变（destroy 只 free 表）。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

#define RT_DICT_INITIAL_CAP 16
#define RT_DICT_INITIAL_CAP_INT 64
#define RT_DICT_LOAD_FACTOR_NUM 3
#define RT_DICT_LOAD_FACTOR_DEN 4
#define RT_DICT_HASH_EMPTY 0u
#define RT_DICT_HASH_TOMB 1u

typedef uint32_t (*rt_hash_fn)(void* key);
typedef int32_t (*rt_eq_fn)(void* a, void* b);

typedef struct RtDict {
    uint32_t* hashes;       /* dense probe metadata */
    void* keys;             /* void** (gen) / int32_t* (int32) / int64_t* (int64) */
    void** values;          /* always pointer-width (ref / boxed / inttoptr V) */
    int32_t capacity;       /* power of two */
    int32_t size;           /* live key count */
    int32_t tombstones;
    rt_hash_fn hash;
    rt_eq_fn eq;
    int32_t int_keys;       /* 0=gen, 1=int32, 2=int64 key column */
    int32_t owned;          /* RFC 051 S3b: 1=值所有权字典（rt_dict_create_owned）——
                               set 覆盖/remove/clear/destroy 释放条目值（rt_arc_dec）；
                               0=legacy（标量/string 值，不维护 ARC） */
    int32_t str_keys;       /* 1=string 键（C 字面量 / char*）；创建期解析，勿靠函数指针== */
} RtDict;

/* EXE→DLL IAT 蹦床：`FF 25 disp32` = jmp qword ptr [rip+disp]。
 * codegen 把 @rt_hash_str 等以蹦床地址传入 create；裸指针 == 永远失败，
 * 导致 int_keys 误判为 0、owns_class_keys 把 inttoptr(TypeId) 当 class 键
 * rt_arc_inc → 0xC0000005（ArmlDemo 启动 DependencyPropertyRegistry 实证）。 */
static void* rt_dict_resolve_fn(void* p) {
    if (!p) {
        return NULL;
    }
#if defined(_WIN32) || defined(_WIN64)
    {
        const unsigned char* c = (const unsigned char*)p;
        if (c[0] == 0xFFu && c[1] == 0x25u) {
            int32_t disp;
            memcpy(&disp, c + 2, sizeof(disp));
            void* const* slot = (void* const*)(c + 6 + disp);
            return *slot;
        }
    }
#endif
    return p;
}

static int32_t rt_dict_fn_is(void* passed, void* canonical) {
    return rt_dict_resolve_fn(passed) == canonical;
}

/* RFC 052 S4：owned gen 键且非 string → class 键所有权（与值同律）。 */
static int32_t rt_dict_owns_class_keys(const RtDict* d) {
    return d && d->owned && d->int_keys == 0 && !d->str_keys;
}

uint32_t rt_hash_str(void* key) {
    const char* s = (const char*)key;
    uint32_t h = 5381;
    if (!s) return 0;
    for (unsigned char c = (unsigned char)*s; c; c = (unsigned char)*++s) {
        h = ((h << 5) + h) + c;
    }
    return h;
}

int32_t rt_eq_str(void* a, void* b) {
    if (!a && !b) return 1;
    if (!a || !b) return 0;
    return strcmp((const char*)a, (const char*)b) == 0;
}

uint32_t rt_hash_int(void* key) {
    /* Identity hash — matches .NET Int32.GetHashCode / Dictionary<int,*>.
     * Fibonacci mix helped weak `v^(v>>16)` + OA; with int32 key facade the
     * G8 sequential fill maps cleanly under identity + power-of-two mask.
     * ABI unchanged. */
    return (uint32_t)(uintptr_t)key;
}

int32_t rt_eq_int(void* a, void* b) {
    return (uintptr_t)a == (uintptr_t)b;
}

uint32_t rt_hash_long(void* key) {
    /* 对齐 .NET Int64.GetHashCode：`(uint32)v ^ (uint32)(v >> 32)`（64 位全量混）。 */
    uint64_t v = (uint64_t)(uintptr_t)key;
    return (uint32_t)v ^ (uint32_t)(v >> 32);
}

int32_t rt_cmp_int(void* a, void* b) {
    intptr_t ia = (intptr_t)a;
    intptr_t ib = (intptr_t)b;
    if (ia < ib) return -1;
    if (ia > ib) return 1;
    return 0;
}

int32_t rt_cmp_str(void* a, void* b) {
    if (!a && !b) return 0;
    if (!a) return -1;
    if (!b) return 1;
    return strcmp((const char*)a, (const char*)b);
}

static uint32_t rt_dict_raw_hash(RtDict* d, void* key) {
    if (d->int_keys == 1) {
        return (uint32_t)(uintptr_t)key;
    }
    return d->hash ? d->hash(key) : 0;
}

static uint32_t rt_dict_tag_hash(uint32_t h) {
    return h < 2u ? h + 2u : h;
}

static int32_t rt_dict_should_resize(RtDict* d) {
    return (d->size + d->tombstones + 1) * RT_DICT_LOAD_FACTOR_DEN
        > d->capacity * RT_DICT_LOAD_FACTOR_NUM;
}

static size_t rt_dict_key_bytes(RtDict* d, int32_t cap) {
    if (d->int_keys == 1) return (size_t)cap * sizeof(int32_t);
    if (d->int_keys == 2) return (size_t)cap * sizeof(int64_t);
    return (size_t)cap * sizeof(void*);
}

static size_t rt_dict_val_bytes(int32_t cap) {
    return (size_t)cap * sizeof(void*);
}

static int32_t rt_dict_alloc_tables(RtDict* d, int32_t cap) {
    /* Single blob: hashes | keys | values — one malloc, better locality on create/grow. */
    size_t hbytes = (size_t)cap * sizeof(uint32_t);
    size_t kbytes = rt_dict_key_bytes(d, cap);
    size_t vbytes = rt_dict_val_bytes(cap);
    size_t hpad = (sizeof(void*) - (hbytes % sizeof(void*))) % sizeof(void*);
    char* blob = (char*)calloc(1, hbytes + hpad + kbytes + vbytes);
    if (!blob) {
        d->hashes = NULL;
        d->keys = NULL;
        d->values = NULL;
        return 0;
    }
    d->hashes = (uint32_t*)blob;
    d->keys = blob + hbytes + hpad;
    d->values = (void**)((char*)d->keys + kbytes);
    d->capacity = cap;
    return 1;
}

static void rt_dict_free_tables(uint32_t* hashes, void* keys, void** values) {
    (void)keys;
    (void)values;
    free(hashes); /* blob base */
}

/* ≥0 live index; -(insert_idx+1) if absent. */
static int32_t rt_dict_probe_int(RtDict* d, void* key, uint32_t tagged) {
    int32_t ikey = (int32_t)(intptr_t)key;
    uint32_t mask = (uint32_t)(d->capacity - 1);
    uint32_t i = tagged & mask;
    int32_t first_tomb = -1;
    uint32_t* hashes = d->hashes;
    int32_t* keys = (int32_t*)d->keys;
    for (;;) {
        uint32_t h = hashes[i];
        if (h == RT_DICT_HASH_EMPTY) {
            int32_t ins = first_tomb >= 0 ? first_tomb : (int32_t)i;
            return -(ins + 1);
        }
        if (h == RT_DICT_HASH_TOMB) {
            if (first_tomb < 0) first_tomb = (int32_t)i;
        } else if (h == tagged && keys[i] == ikey) {
            return (int32_t)i;
        }
        i = (i + 1u) & mask;
    }
}

static int32_t rt_dict_probe_long(RtDict* d, void* key, uint32_t tagged) {
    int64_t lkey = (int64_t)(uintptr_t)key;
    uint32_t mask = (uint32_t)(d->capacity - 1);
    uint32_t i = tagged & mask;
    int32_t first_tomb = -1;
    uint32_t* hashes = d->hashes;
    int64_t* keys = (int64_t*)d->keys;
    for (;;) {
        uint32_t h = hashes[i];
        if (h == RT_DICT_HASH_EMPTY) {
            int32_t ins = first_tomb >= 0 ? first_tomb : (int32_t)i;
            return -(ins + 1);
        }
        if (h == RT_DICT_HASH_TOMB) {
            if (first_tomb < 0) first_tomb = (int32_t)i;
        } else if (h == tagged && keys[i] == lkey) {
            return (int32_t)i;
        }
        i = (i + 1u) & mask;
    }
}

static int32_t rt_dict_probe_gen(RtDict* d, void* key, uint32_t tagged) {
    uint32_t mask = (uint32_t)(d->capacity - 1);
    uint32_t i = tagged & mask;
    int32_t first_tomb = -1;
    uint32_t* hashes = d->hashes;
    void** keys = (void**)d->keys;
    for (;;) {
        uint32_t h = hashes[i];
        if (h == RT_DICT_HASH_EMPTY) {
            int32_t ins = first_tomb >= 0 ? first_tomb : (int32_t)i;
            return -(ins + 1);
        }
        if (h == RT_DICT_HASH_TOMB) {
            if (first_tomb < 0) first_tomb = (int32_t)i;
        } else if (h == tagged && d->eq(keys[i], key)) {
            return (int32_t)i;
        }
        i = (i + 1u) & mask;
    }
}

static int32_t rt_dict_probe(RtDict* d, void* key, uint32_t tagged) {
    if (d->int_keys == 1) return rt_dict_probe_int(d, key, tagged);
    if (d->int_keys == 2) return rt_dict_probe_long(d, key, tagged);
    return rt_dict_probe_gen(d, key, tagged);
}

static void rt_dict_insert_at(RtDict* d, int32_t idx, void* key, void* value, uint32_t tagged) {
    if (d->hashes[idx] == RT_DICT_HASH_TOMB) d->tombstones--;
    d->hashes[idx] = tagged;
    if (d->int_keys == 1) {
        ((int32_t*)d->keys)[idx] = (int32_t)(intptr_t)key;
    } else if (d->int_keys == 2) {
        ((int64_t*)d->keys)[idx] = (int64_t)(uintptr_t)key;
    } else {
        ((void**)d->keys)[idx] = key;
    }
    d->values[idx] = value;
    d->size++;
}

static void rt_dict_rehash(RtDict* d, int32_t new_cap) {
    uint32_t* old_h = d->hashes;
    void* old_k = d->keys;
    void** old_v = d->values;
    int32_t old_cap = d->capacity;
    int32_t int_keys = d->int_keys;
    if (!rt_dict_alloc_tables(d, new_cap)) rt_panic("oom");
    d->size = 0;
    d->tombstones = 0;
    uint32_t mask = (uint32_t)(new_cap - 1);
    void** nv = d->values;
    if (int_keys == 1) {
        int32_t* ok = (int32_t*)old_k;
        int32_t* nk = (int32_t*)d->keys;
        for (int32_t i = 0; i < old_cap; i++) {
            uint32_t h = old_h[i];
            if (h < 2u) continue;
            uint32_t j = h & mask;
            while (d->hashes[j] != RT_DICT_HASH_EMPTY) {
                j = (j + 1u) & mask;
            }
            d->hashes[j] = h;
            nk[j] = ok[i];
            nv[j] = old_v[i];
            d->size++;
        }
    } else if (int_keys == 2) {
        int64_t* ok = (int64_t*)old_k;
        int64_t* nk = (int64_t*)d->keys;
        for (int32_t i = 0; i < old_cap; i++) {
            uint32_t h = old_h[i];
            if (h < 2u) continue;
            uint32_t j = h & mask;
            while (d->hashes[j] != RT_DICT_HASH_EMPTY) {
                j = (j + 1u) & mask;
            }
            d->hashes[j] = h;
            nk[j] = ok[i];
            nv[j] = old_v[i];
            d->size++;
        }
    } else {
        void** ok = (void**)old_k;
        void** nk = (void**)d->keys;
        for (int32_t i = 0; i < old_cap; i++) {
            uint32_t h = old_h[i];
            if (h < 2u) continue;
            uint32_t j = h & mask;
            while (d->hashes[j] != RT_DICT_HASH_EMPTY) {
                j = (j + 1u) & mask;
            }
            d->hashes[j] = h;
            nk[j] = ok[i];
            nv[j] = old_v[i];
            d->size++;
        }
    }
    rt_dict_free_tables(old_h, old_k, old_v);
}

static void rt_dict_grow_if_needed(RtDict* d) {
    if (!rt_dict_should_resize(d)) return;
    /* int_keys hotpath: 4× grow cuts rehash count during dense fill (G8 N~1.5e5). */
    int32_t new_cap = d->capacity * (d->int_keys ? 4 : 2);
    if (d->tombstones > d->size && d->capacity > RT_DICT_INITIAL_CAP) {
        new_cap = d->capacity;
    }
    rt_dict_rehash(d, new_cap);
}

static void* rt_dict_create_impl(rt_hash_fn hash, rt_eq_fn eq, int32_t owned) {
    RtDict* d = (RtDict*)calloc(1, sizeof(RtDict));
    if (!d) return NULL;
    d->hash = hash;
    d->eq = eq;
    d->owned = owned;
    /* 经 IAT 蹦床解析后再判定键档（见 rt_dict_resolve_fn）。 */
    d->int_keys = (rt_dict_fn_is((void*)hash, (void*)rt_hash_int)
                   && rt_dict_fn_is((void*)eq, (void*)rt_eq_int))
                      ? 1
                      : (rt_dict_fn_is((void*)hash, (void*)rt_hash_long)
                         && rt_dict_fn_is((void*)eq, (void*)rt_eq_int))
                            ? 2
                            : 0;
    d->str_keys = rt_dict_fn_is((void*)hash, (void*)rt_hash_str) ? 1 : 0;
    {
        int32_t init = d->int_keys ? RT_DICT_INITIAL_CAP_INT : RT_DICT_INITIAL_CAP;
        if (!rt_dict_alloc_tables(d, init)) {
            free(d);
            return NULL;
        }
    }
    d->size = 0;
    d->tombstones = 0;
    return d;
}

void* rt_dict_create(rt_hash_fn hash, rt_eq_fn eq) {
    return rt_dict_create_impl(hash, eq, 0);
}

void* rt_dict_create_owned(rt_hash_fn hash, rt_eq_fn eq) {
    return rt_dict_create_impl(hash, eq, 1);
}

void rt_dict_ensure_capacity(void* dict, int32_t capacity) {
    if (!dict || capacity <= 0) return;
    RtDict* d = (RtDict*)dict;
    /* 保持负载因子 ≤ 0.75 的最小 2 次幂容量（对齐 rt_dict_should_resize 语义）。 */
    int64_t need = ((int64_t)capacity * RT_DICT_LOAD_FACTOR_DEN)
                   / RT_DICT_LOAD_FACTOR_NUM + 1;
    int32_t cap = d->capacity;
    while (cap < need) cap <<= 1;
    if (cap > d->capacity) {
        rt_dict_rehash(d, cap); /* rehash 迁移活项，size 不变 */
    }
}

void rt_dict_set(void* dict, void* key, void* value) {
    if (!dict) return;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    int32_t idx = rt_dict_probe(d, key, tagged);
    if (idx >= 0) {
        /* RFC 051 S3b：覆盖既有条目——owned 时新值先 inc（new==old 自覆盖
         * 净零）、槽位先写新值、旧值后 dec（旧值 finalizer 重入读到新值）。
         * RFC 052 S4：class 键同律（键对象身份不变时 net zero）。 */
        if (d->owned) {
            rt_arc_inc(value);
            void* old = d->values[idx];
            d->values[idx] = value;
            rt_arc_dec(old);
        } else {
            d->values[idx] = value;
        }
        return;
    }
    if (rt_dict_should_resize(d)) {
        rt_dict_grow_if_needed(d);
        idx = rt_dict_probe(d, key, tagged);
        if (idx >= 0) {
            if (d->owned) {
                rt_arc_inc(value);
                void* old = d->values[idx];
                d->values[idx] = value;
                rt_arc_dec(old);
            } else {
                d->values[idx] = value;
            }
            return;
        }
    }
    if (d->owned) rt_arc_inc(value); /* 存储侧自持 +1（codegen 不再预 inc） */
    if (rt_dict_owns_class_keys(d)) rt_arc_inc(key);
    rt_dict_insert_at(d, -idx - 1, key, value, tagged);
}

int32_t rt_dict_try_add(void* dict, void* key, void* value) {
    if (!dict) return 0;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    int32_t idx = rt_dict_probe(d, key, tagged);
    if (idx >= 0) return 0; /* 重复键不存储：owned 亦不 inc（无孤儿 +1） */
    if (rt_dict_should_resize(d)) {
        rt_dict_grow_if_needed(d);
        idx = rt_dict_probe(d, key, tagged);
        if (idx >= 0) return 0;
    }
    if (d->owned) rt_arc_inc(value);
    if (rt_dict_owns_class_keys(d)) rt_arc_inc(key);
    rt_dict_insert_at(d, -idx - 1, key, value, tagged);
    return 1;
}

int32_t rt_dict_try_get_value(void* dict, void* key, void** out_value) {
    if (!dict || !out_value) return 0;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    int32_t idx = rt_dict_probe(d, key, tagged);
    if (idx < 0) {
        *out_value = NULL;
        return 0;
    }
    *out_value = d->values[idx];
    return 1;
}

void* rt_dict_get(void* dict, void* key) {
    if (!dict) return NULL;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    int32_t idx = rt_dict_probe(d, key, tagged);
    return idx < 0 ? NULL : d->values[idx];
}

int32_t rt_dict_contains(void* dict, void* key) {
    if (!dict) return 0;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    return rt_dict_probe(d, key, tagged) >= 0;
}

int32_t rt_dict_contains_value(void* dict, void* value, int32_t (*eq)(void* a, void* b)) {
    if (!dict) return 0;
    RtDict* d = (RtDict*)dict;
    for (int32_t i = 0; i < d->capacity; i++) {
        uint32_t h = d->hashes[i];
        if (h == RT_DICT_HASH_EMPTY || h == RT_DICT_HASH_TOMB) continue;
        void* v = d->values[i];
        if (eq) {
            if (eq(v, value)) return 1;
        } else if (v == value) {
            return 1;
        }
    }
    return 0;
}


int32_t rt_dict_count(void* dict) {
    if (!dict) return 0;
    return ((RtDict*)dict)->size;
}

int32_t rt_dict_remove(void* dict, void* key) {
    if (!dict) return 0;
    RtDict* d = (RtDict*)dict;
    uint32_t tagged = rt_dict_tag_hash(rt_dict_raw_hash(d, key));
    int32_t idx = rt_dict_probe(d, key, tagged);
    if (idx < 0) return 0;
    void* removed = d->values[idx];
    void* removed_key = NULL;
    d->hashes[idx] = RT_DICT_HASH_TOMB;
    if (d->int_keys == 1) {
        ((int32_t*)d->keys)[idx] = 0;
    } else if (d->int_keys == 2) {
        ((int64_t*)d->keys)[idx] = 0;
    } else {
        removed_key = ((void**)d->keys)[idx];
        ((void**)d->keys)[idx] = NULL;
    }
    d->values[idx] = NULL; /* 先清槽再 dec：值 finalizer 重入不读半死槽 */
    d->size--;
    d->tombstones++;
    if (d->owned) rt_arc_dec(removed); /* RFC 051 S3b：释放被移除条目值 */
    if (rt_dict_owns_class_keys(d)) rt_arc_dec(removed_key);
    return 1;
}

void rt_dict_clear(void* dict) {
    if (!dict) return;
    RtDict* d = (RtDict*)dict;
    if (d->owned) {
        /* RFC 051 S3b：owned 清空须释放每个活条目值（先清槽再 dec）。
         * RFC 052 S4：class 键一并释放。 */
        int32_t own_keys = rt_dict_owns_class_keys(d);
        for (int32_t i = 0; i < d->capacity; i++) {
            if (d->hashes[i] < 2u) continue;
            void* v = d->values[i];
            d->values[i] = NULL;
            if (own_keys) {
                void* k = ((void**)d->keys)[i];
                ((void**)d->keys)[i] = NULL;
                rt_arc_dec(k);
            }
            rt_arc_dec(v);
        }
    }
    memset(d->hashes, 0, (size_t)d->capacity * sizeof(uint32_t));
    memset(d->keys, 0, rt_dict_key_bytes(d, d->capacity));
    memset(d->values, 0, rt_dict_val_bytes(d->capacity));
    d->size = 0;
    d->tombstones = 0;
}

void rt_dict_destroy(void* dict) {
    if (!dict) return;
    RtDict* d = (RtDict*)dict;
    if (d->owned) {
        int32_t own_keys = rt_dict_owns_class_keys(d);
        for (int32_t i = 0; i < d->capacity; i++) {
            if (d->hashes[i] < 2u) continue;
            void* v = d->values[i];
            d->values[i] = NULL;
            if (own_keys) {
                void* k = ((void**)d->keys)[i];
                ((void**)d->keys)[i] = NULL;
                rt_arc_dec(k);
            }
            rt_arc_dec(v);
        }
    }
    rt_dict_free_tables(d->hashes, d->keys, d->values);
    free(d);
}

void* rt_dict_keys(void* dict) {
    if (!dict) return NULL;
    RtDict* d = (RtDict*)dict;
    /* RFC 052 S4：owned + class 键 → create_refs + 逐元素 retain；
     * int/long/string 键与 legacy → scalar（值类型/借用，无元素 ARC）。 */
    int32_t use_refs = rt_dict_owns_class_keys(d);
    void* arr = use_refs
        ? rt_array_create_refs(d->size, (int32_t)sizeof(void*))
        : rt_array_create(d->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    if (d->int_keys == 1) {
        int32_t* keys = (int32_t*)d->keys;
        for (int32_t i = 0; i < d->capacity && idx < d->size; i++) {
            if (d->hashes[i] >= 2u) {
                items[idx++] = (void*)(uintptr_t)(intptr_t)keys[i];
            }
        }
    } else if (d->int_keys == 2) {
        int64_t* keys = (int64_t*)d->keys;
        for (int32_t i = 0; i < d->capacity && idx < d->size; i++) {
            if (d->hashes[i] >= 2u) {
                items[idx++] = (void*)(uintptr_t)keys[i];
            }
        }
    } else {
        void** keys = (void**)d->keys;
        for (int32_t i = 0; i < d->capacity && idx < d->size; i++) {
            if (d->hashes[i] >= 2u) {
                void* k = keys[i];
                if (use_refs) rt_arc_inc(k);
                items[idx++] = k;
            }
        }
    }
    return arr;
}

void* rt_dict_values(void* dict) {
    if (!dict) return NULL;
    RtDict* d = (RtDict*)dict;
    void* arr = d->owned
        ? rt_array_create_refs(d->size, (int32_t)sizeof(void*))
        : rt_array_create(d->size, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    int32_t idx = 0;
    for (int32_t i = 0; i < d->capacity && idx < d->size; i++) {
        if (d->hashes[i] >= 2u) {
            void* v = d->values[i];
            if (d->owned) rt_arc_inc(v); /* RFC 052 S4：快照逐元素 +1 */
            items[idx++] = v;
        }
    }
    return arr;
}

typedef struct RtDictEnumerator {
    RtDict* dict;
    int32_t slot_idx;
} RtDictEnumerator;

void* rt_dict_get_enumerator(void* dict) {
    if (!dict) return NULL;
    RtDictEnumerator* e = (RtDictEnumerator*)calloc(1, sizeof(RtDictEnumerator));
    if (!e) return NULL;
    e->dict = (RtDict*)dict;
    e->slot_idx = -1;
    return e;
}

int32_t rt_dict_enumerator_move_next(void* handle) {
    RtDictEnumerator* e = (RtDictEnumerator*)handle;
    if (!e) return 0;
    while (++e->slot_idx < e->dict->capacity) {
        if (e->dict->hashes[e->slot_idx] >= 2u) return 1;
    }
    return 0;
}

void* rt_dict_enumerator_get_key(void* handle) {
    RtDictEnumerator* e = (RtDictEnumerator*)handle;
    if (!e || e->slot_idx < 0 || e->slot_idx >= e->dict->capacity) return NULL;
    if (e->dict->hashes[e->slot_idx] < 2u) return NULL;
    if (e->dict->int_keys == 1) {
        return (void*)(uintptr_t)(intptr_t)((int32_t*)e->dict->keys)[e->slot_idx];
    }
    if (e->dict->int_keys == 2) {
        return (void*)(uintptr_t)((int64_t*)e->dict->keys)[e->slot_idx];
    }
    return ((void**)e->dict->keys)[e->slot_idx];
}

void* rt_dict_enumerator_get_value(void* handle) {
    RtDictEnumerator* e = (RtDictEnumerator*)handle;
    if (!e || e->slot_idx < 0 || e->slot_idx >= e->dict->capacity) return NULL;
    if (e->dict->hashes[e->slot_idx] < 2u) return NULL;
    return e->dict->values[e->slot_idx];
}
