// ConcurrentDictionary<K,V> implementation (RFC 024 M1).
//
// Design (2026-08-03 轨道 G 性能重构 · RFC 036 §2.3 / V1-SPRINT 轨道 G):
//   - Striped locks: 固定 RT_CD_STRIPES 把常驻自旋锁（CAS + 让步退避）分片池，
//     桶通过 hash 分片共享；无 malloc/无销毁，锁对象常驻 cache；无竞争往返
//     ~5-10ns（SRWLOCK 的 ~30-40ns 是 1t 基准 per-op 主成本）
//     （单线程 TryAdd 的 per-bucket 冷 cache + resize mutex 创建是 6.3× 主成本）。
//   - Lock-free read：table 指针原子发布（immutable table），读路径一次 acquire load
//     即得一致 (bucket_count, buckets)；resize 只串行迁移，读不碰 table_lock。
//   - 写路径先取 stripe 锁、再 load table → 消除「快照后 resize 已发布」的丢失窗口
//     （比旧版 table_lock 快照 + 桶锁的两步时序更严谨）。
//   - Safe reclamation: 删除节点仅标记 deleted（key=NULL），不立即 free；
//     旧 table 整体延迟释放（deferred list），读路径快照后永不 use-after-free。
//   - 公开 ABI 不变（rt_abi.h 未动）；语义对齐 RFC 024 §4.1（stale-but-safe 读）。
//
// 旧设计（保留注释供追溯）：per-bucket mutex、table_lock 每 op 快照。
//   - 每 op 取 table_lock（SRW 全局锁）→ resize 时交换 buckets 指针；
//   - 每桶一个 SRWLOCK，resize 为每个新桶 rt_mutex_create（~24ns×桶数）；
//   - 单线程 TryAdd 实测 ~165-470 ns/op vs .NET ~26 ns/op（6.3×）。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#else
#include <sched.h>
#include <time.h>
#endif

#define RT_CD_STRIPES 64 /* 2^6，桶→stripe 用位与 */

/* ---- 轻量 stripe 锁（自旋 + 让步退避） ----
 * 单线程/低竞争下 SRWLOCK 的 Acquire+Release 往返 ~30-40ns；用 CAS 自旋锁
 * （~5-10ns 无竞争往返）降低 per-op 锁开销（concurrent_dict_1t 基准主成本）。
 * 临界区为极短的桶操作；自旋达阈值后切线程/休眠让步，多线程下仍正确
 * （见 lock_all_stripes：resize 持 table_lock 后逐 stripe 加锁，锁序无环）。
 */
typedef struct rt_cd_lock {
    volatile long state;  /* 0 = free，1 = held */
} rt_cd_lock_t;

static inline void rt_cd_lock_init(rt_cd_lock_t* l) { l->state = 0; }

static inline void rt_cd_lock_acquire(rt_cd_lock_t* l) {
    for (int spin = 0; ; spin++) {
        if (__atomic_load_n(&l->state, __ATOMIC_ACQUIRE) == 0 &&
            __sync_val_compare_and_swap(&l->state, 0, 1) == 0)
            return;
        if (spin < 100) {
#if defined(__x86_64__) || defined(_M_X64) || defined(__i386__)
            __asm__ __volatile__("pause");
#endif
        } else if (spin < 1000) {
#ifdef _WIN32
            SwitchToThread();
#else
            sched_yield();
#endif
        } else {
#ifdef _WIN32
            Sleep(0);
#else
            struct timespec ts;
            ts.tv_sec = 0;
            ts.tv_nsec = 2000000L;
            nanosleep(&ts, NULL);
#endif
        }
    }
}

static inline void rt_cd_lock_release(rt_cd_lock_t* l) {
    __atomic_store_n(&l->state, 0, __ATOMIC_RELEASE);
}

/* ---- Power-of-two bucket count helpers ----
 * 桶数为 2 的幂 → 热路径用位与掩码替代取模（idiv ~20-30 周期，mask 1 周期）。
 * 对齐 .NET ConcurrentDictionary 的 FastMod 思路；语义不变（桶只是索引分片）。
 * 碰撞控制：写入路径对 hash 做低位混合（hash ^ hash>>16），缓解用户 hash
 * 低位分布差的问题。 */
#define RT_CD_MIN_BUCKETS 16

static int32_t next_pow2(int32_t n) {
    if (n < RT_CD_MIN_BUCKETS) n = RT_CD_MIN_BUCKETS;
    int32_t p = 1;
    while (p < n) p <<= 1;
    return p;
}

static inline uint32_t rt_cd_mix_hash(uint32_t h) {
    return h ^ (h >> 16);
}

static inline uint32_t rt_cd_bucket_index(uint32_t mixed, int32_t bucket_count) {
    return mixed & (uint32_t)(bucket_count - 1);
}

/* ---- Node ---- */

typedef struct rt_cd_node {
    void*              key;        // NULL = deleted (safe reclamation marker)
    void*              value;
    int32_t            key_hash;
    struct rt_cd_node* next;
} rt_cd_node_t;

/* ---- Bucket ---- */

typedef struct rt_cd_bucket {
    rt_cd_node_t*  head;           // 链头（锁由 stripe 分片提供，不再每桶一把）
} rt_cd_bucket_t;

/* ---- Immutable table ----
 * 原子发布的只读快照：{ bucket_count, buckets } 同块读出，读路径一次
 * load_table() 即得一致对；resize 构建新 table 后原子发布，旧 table 延迟释放。
 */
typedef struct rt_cd_table {
    int32_t          bucket_count;
    rt_cd_bucket_t*  buckets;
    int32_t          per_stripe_threshold;  // bucket_count / RT_CD_STRIPES（热路径免除法）
} rt_cd_table_t;

/* ---- Dictionary handle ---- */

typedef struct rt_cd_deferred {
    rt_cd_table_t*        table;     // 旧 table（含 buckets 数组）
    struct rt_cd_deferred* next;
} rt_cd_deferred_t;

/* ---- Node pool（批次分配，替代逐节点 calloc） ----
 * 节点生命周期：插入后跨 resize 代存活（resize 迁移不 free），仅 clear 时回收。
 * 批次按 2 的幂批量 calloc；回收的节点进 freelist 复用。池按 stripe 分片，
 * 分配在持 stripe 锁的域内完成（无共享池锁），回收仅在 clear 的全 stripe
 * 锁域内发生（含并入该域的 deferred 表释放）；resize 走 table_lock→stripes，
 * 不触池，无锁序环）。 */
#define RT_CD_NODE_BATCH 1024

typedef struct rt_cd_node_batch {
    struct rt_cd_node_batch* next;
    rt_cd_node_t             nodes[RT_CD_NODE_BATCH];
} rt_cd_node_batch_t;

/* 每 stripe 独立的节点池（无共享 node_lock）：
 * 写路径已在持 stripe 锁的域内分配节点，故池访问天然互斥——省去逐次插入
 * 的全局 mutex（单线程 TryAdd 的 node_lock 锁开销是热路径主成本之一）。 */
typedef struct rt_cd_pool {
    rt_cd_node_batch_t* node_batches;  // 本 stripe 的批次链表
    rt_cd_node_t*       free_nodes;    // clear 回收节点（统一进 stripe 0 池）
    int32_t             batch_idx;     // 当前批内已分配数
} rt_cd_pool_t;

typedef struct rt_concurrent_dict_t {
    uint32_t              (*hash_fn)(void*);
    int32_t               (*eq_fn)(void*, void*);
    rt_cd_table_t*        table;           // 原子 load/store（lock-free read）
    rt_cd_lock_t          stripe_locks[RT_CD_STRIPES];  // 分片自旋锁（常驻、无 malloc）
    int32_t               stripe_counts[RT_CD_STRIPES];  // 每 stripe 计数（stripe 锁内非原子增减）
    void*                 table_lock;      // 仅序列化 resize（迁移互斥）
    rt_cd_deferred_t*     deferred_old_tables;  // 旧 table 延迟释放链表
    rt_cd_pool_t          pools[RT_CD_STRIPES];  // 每 stripe 节点池（无共享锁）
} rt_concurrent_dict_t;

/* 分配节点：从 stripe_idx 对应池取（优先 freelist，其次当前批次）。
 * 调用方须已持有该 stripe 锁，无需额外互斥。返回零初始化节点。 */
static rt_cd_node_t* rt_cd_node_alloc(rt_concurrent_dict_t* d, int32_t stripe_idx) {
    rt_cd_pool_t* pool = &d->pools[stripe_idx];
    rt_cd_node_t* n = pool->free_nodes;
    if (n) {
        pool->free_nodes = n->next;
        return n;
    }
    if (!pool->node_batches || pool->batch_idx >= RT_CD_NODE_BATCH) {
        rt_cd_node_batch_t* b =
            (rt_cd_node_batch_t*)calloc(1, sizeof(rt_cd_node_batch_t));
        if (!b) return NULL;
        b->next = pool->node_batches;
        pool->node_batches = b;
        pool->batch_idx = 0;
    }
    n = &pool->node_batches->nodes[pool->batch_idx++];
    n->key = NULL;
    n->value = NULL;
    n->key_hash = 0;
    n->next = NULL;
    return n;
}

/* 回收节点到 stripe 0 池的 freelist。仅在 clear 的全 stripe 锁域内调用
 * （含并入该域的 free_deferred_tables），与并发 alloc 无竞争
 * （alloc 需要取 stripe 锁，clear 持有全部 stripe 锁）。 */
static void rt_cd_node_recycle(rt_concurrent_dict_t* d, rt_cd_node_t* n) {
    n->key = NULL;
    n->value = NULL;
    n->key_hash = 0;
    n->next = d->pools[0].free_nodes;
    d->pools[0].free_nodes = n;
}

/* ---- Atomic helpers ---- */

static int32_t atomic_read(volatile int32_t* p) {
    return __sync_add_and_fetch((volatile int32_t*)p, 0);
}

static rt_cd_table_t* load_table(rt_concurrent_dict_t* d) {
    return (rt_cd_table_t*)__atomic_load_n(&d->table, __ATOMIC_ACQUIRE);
}

static void publish_table(rt_concurrent_dict_t* d, rt_cd_table_t* t) {
    __atomic_store_n(&d->table, t, __ATOMIC_RELEASE);
}

/* ---- Stripe lock helpers ---- */

static void lock_all_stripes(rt_concurrent_dict_t* d) {
    for (int32_t i = 0; i < RT_CD_STRIPES; i++)
        rt_cd_lock_acquire(&d->stripe_locks[i]);
}

static void unlock_all_stripes(rt_concurrent_dict_t* d) {
    for (int32_t i = RT_CD_STRIPES - 1; i >= 0; i--)
        rt_cd_lock_release(&d->stripe_locks[i]);
}

/* ---- Create ---- */

void* rt_concurrent_dict_create(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t bucket_count)
{
    if (bucket_count <= 0) bucket_count = 31;
    int32_t n = next_pow2(bucket_count);

    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)calloc(1, sizeof(*d));
    if (!d) return NULL;
    d->hash_fn = hash;
    d->eq_fn = eq;
    d->deferred_old_tables = NULL;

    rt_cd_table_t* t = (rt_cd_table_t*)calloc(1, sizeof(*t));
    if (!t) { free(d); return NULL; }
    t->bucket_count = n;
    t->buckets = (rt_cd_bucket_t*)calloc((size_t)n, sizeof(rt_cd_bucket_t));
    if (!t->buckets) { free(t); free(d); return NULL; }
    t->per_stripe_threshold = n / RT_CD_STRIPES;
    if (t->per_stripe_threshold < 1) t->per_stripe_threshold = 1;

    for (int32_t i = 0; i < RT_CD_STRIPES; i++)
        rt_cd_lock_init(&d->stripe_locks[i]);
    d->table_lock = rt_mutex_create();
    if (!d->table_lock) {
        free(t->buckets);
        free(t);
        free(d);
        return NULL;
    }
    d->table = t;
    return d;
}

void* rt_concurrent_dict_create_level(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t concurrency_level)
{
    return rt_concurrent_dict_create(hash, eq, concurrency_level * 4);
}

void* rt_concurrent_dict_create_level_cap(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*),
    int32_t concurrency_level, int32_t capacity)
{
    int32_t n = concurrency_level > 0 ? concurrency_level * 4 : 31;
    if (capacity > n) n = capacity;
    return rt_concurrent_dict_create(hash, eq, n);
}

/* ---- Internal: find live node in bucket (caller holds bucket's stripe lock) ---- */

static rt_cd_node_t* find_in_bucket(rt_cd_bucket_t* b, void* key, int32_t hash,
    int32_t (*eq)(void*, void*))
{
    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (n->key && n->key_hash == hash && (n->key == key || eq(n->key, key)))
            return n;
    }
    return NULL;
}

/* ---- Resize ----
 * 在 table_lock 保护下迁移（与写路径的 stripe 锁互斥：写路径先取 stripe 锁，
 * 故迁移持有全部 stripe 锁即可排除并发写）。构建新 table 后原子发布；
 * 旧 table 挂到 deferred 链表延迟释放（读路径快照后永不 UAF）。
 */
static void resize(rt_concurrent_dict_t* d) {
    rt_mutex_lock(d->table_lock);

    rt_cd_table_t* cur = load_table(d);
    int32_t new_n = cur->bucket_count * 4; /* 幂二翻两倍：4× 时末代桶阵 ~1MB 可驻 L2，8× 的 4MB 反致随机桶访存出 L2 */
    if (new_n <= cur->bucket_count) { rt_mutex_unlock(d->table_lock); return; }

    rt_cd_table_t* neu = (rt_cd_table_t*)calloc(1, sizeof(*neu));
    if (!neu) { rt_mutex_unlock(d->table_lock); return; }
    neu->bucket_count = new_n;
    neu->buckets = (rt_cd_bucket_t*)calloc((size_t)new_n, sizeof(rt_cd_bucket_t));
    if (!neu->buckets) {
        free(neu);
        rt_mutex_unlock(d->table_lock);
        return;
    }
    neu->per_stripe_threshold = new_n / RT_CD_STRIPES;
    if (neu->per_stripe_threshold < 1) neu->per_stripe_threshold = 1;

    /* 持全部 stripe 锁：排除所有写路径，读路径照常 lock-free（stale-but-safe） */
    lock_all_stripes(d);

    for (int32_t i = 0; i < cur->bucket_count; i++) {
        rt_cd_node_t* n = cur->buckets[i].head;
        cur->buckets[i].head = NULL;
        while (n) {
            rt_cd_node_t* next = n->next;
            if (n->key) {
                int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)n->key_hash), new_n);
                n->next = neu->buckets[bidx].head;
                neu->buckets[bidx].head = n;
            } else {
                /* deleted 墓碑：留在旧 table，重建旧桶链以便 deferred 释放
                 * （绝不提前 free；读路径快照后可能仍持指针）。 */
                n->next = cur->buckets[i].head;
                cur->buckets[i].head = n;
            }
            n = next;
        }
    }

    publish_table(d, neu);
    unlock_all_stripes(d);

    rt_cd_deferred_t* node = (rt_cd_deferred_t*)calloc(1, sizeof(*node));
    if (node) {
        node->table = cur;
        node->next = d->deferred_old_tables;
        d->deferred_old_tables = node;
    } else {
        /* 内存不足：泄漏旧 table（安全降级——仅内存浪费，无 use-after-free） */
    }

    rt_mutex_unlock(d->table_lock);
}

/* 回收所有 deferred 旧 table 的节点并释放表结构（仅在 clear 无并发时调用；
 * 节点归池（批次内存），不逐节点 free——批次随 dict 生命周期持有。 */
static void free_deferred_tables(rt_concurrent_dict_t* d) {
    rt_cd_deferred_t* cur = d->deferred_old_tables;
    while (cur) {
        rt_cd_deferred_t* next = cur->next;
        for (int32_t i = 0; i < cur->table->bucket_count; i++) {
            rt_cd_node_t* n = cur->table->buckets[i].head;
            while (n) {
                rt_cd_node_t* nxt = n->next;
                rt_cd_node_recycle(d, n);
                n = nxt;
            }
        }
        free(cur->table->buckets);
        free(cur->table);
        free(cur);
        cur = next;
    }
    d->deferred_old_tables = NULL;
}

/* ---- API ---- */
//
// 写路径统一约定：先取 stripe 锁，再 load_table（resize 持全部 stripe 锁发布，
// 故「取锁后 load」得到稳定 table，消除快照后 resize 的丢失窗口）。

int32_t rt_concurrent_dict_try_add(void* dict, void* key, void* value) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    if (find_in_bucket(b, key, hash, d->eq_fn)) {
        rt_cd_lock_release(slock);
        return 0;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    n->key = key;
    n->value = value;
    n->key_hash = hash;
    n->next = b->head;
    b->head = n;

    int32_t new_count = ++d->stripe_counts[stripe];
    rt_cd_lock_release(slock);

    if (new_count > t->per_stripe_threshold) {
        resize(d);
    }
    return 1;
}

/* Lock-free read（一次 acquire load table）：遍历桶链。可能读到迁移中的旧代桶
 * （迁移后旧桶 head=NULL）→ 视为「不存在」（RFC 024 §4.1 stale-but-safe）。
 * 绝不解引用已释放内存：旧 table 整体延迟释放（仅 clear 无并发时释放）。
 * miss 须清 out 槽（对齐 rt_dict_try_get_value；禁未初始化泄漏 → Assert.Equal(0,v) 假红）。 */
int32_t rt_concurrent_dict_try_get(void* dict, void* key, void** out_value) {
    if (!out_value) return 0;
    *out_value = NULL;
    if (!dict) return 0;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (n->key && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key))) {
            *out_value = n->value;
            return 1;
        }
    }
    return 0;
}

void rt_concurrent_dict_set(void* dict, void* key, void* value) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        existing->value = value;
    } else {
        rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
        if (!n) { rt_cd_lock_release(slock); return; }
        n->key = key;
        n->value = value;
        n->key_hash = hash;
        n->next = b->head;
        b->head = n;
        ++d->stripe_counts[stripe];
    }
    rt_cd_lock_release(slock);
}

void* rt_concurrent_dict_get_or_default(void* dict, void* key) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (n->key && n->key_hash == hash && d->eq_fn(n->key, key))
            return n->value;
    }
    return NULL;
}

/* Safe removal (logical delete): 找到节点后仅标记 key=NULL / value=NULL，不脱链。
 * 读路径 lock-free 遍历时跳过 deleted 节点（RFC 024 §4.1 stale-but-safe）；
 * 节点留在当前代桶中作为墓碑，resize 迁移时不带走（随旧 table 延迟释放），
 * clear 时整体 free。绝不立即 free：快照中的读路径可能仍持指针。 */
int32_t rt_concurrent_dict_try_remove(void* dict, void* key, void** out_value) {
    if (!out_value) return 0;
    *out_value = NULL; /* miss 须清 out（与 try_get / rt_dict 同契约） */
    if (!dict) return 0;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (n->key && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key))) {
            *out_value = n->value;
            n->key = NULL;
            n->value = NULL;
            --d->stripe_counts[stripe];
            rt_cd_lock_release(slock);
            return 1;
        }
    }
    rt_cd_lock_release(slock);
    return 0;
}

/* TryUpdate: CAS on value — update only if current value equals comparisonValue */
int32_t rt_concurrent_dict_try_update(void* dict, void* key, void* newValue,
    void* comparisonValue)
{
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (!existing) {
        rt_cd_lock_release(slock);
        return 0;
    }
    if (existing->value != comparisonValue) {
        rt_cd_lock_release(slock);
        return 0;
    }
    existing->value = newValue;
    rt_cd_lock_release(slock);
    return 1;
}

void* rt_concurrent_dict_get_or_add(void* dict, void* key,
    void* (*factory)(void*))
{
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        rt_cd_lock_release(slock);
        return existing->value;
    }
    void* value = factory(key);
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    n->key = key;
    n->value = value;
    n->key_hash = hash;
    n->next = b->head;
    b->head = n;
    ++d->stripe_counts[stripe];
    rt_cd_lock_release(slock);
    return value;
}

/* GetOrAdd with simple value (no delegate) */
void* rt_concurrent_dict_get_or_add_val(void* dict, void* key, void* value) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        rt_cd_lock_release(slock);
        return existing->value;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    n->key = key;
    n->value = value;
    n->key_hash = hash;
    n->next = b->head;
    b->head = n;
    ++d->stripe_counts[stripe];
    rt_cd_lock_release(slock);
    return value;
}

/* AddOrUpdate: addValue + updateFactory, both executed under bucket stripe lock */
void* rt_concurrent_dict_add_or_update(void* dict, void* key, void* addValue,
    void* (*updateFactory)(void*, void*))
{
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* newVal = updateFactory(key, existing->value);
        existing->value = newVal;
        rt_cd_lock_release(slock);
        return newVal;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return addValue; }
    n->key = key;
    n->value = addValue;
    n->key_hash = hash;
    n->next = b->head;
    b->head = n;
    ++d->stripe_counts[stripe];
    rt_cd_lock_release(slock);
    return addValue;
}

/* AddOrUpdate with factory for both paths */
void* rt_concurrent_dict_add_or_update_pf(void* dict, void* key,
    void* (*addValueFactory)(void*),
    void* (*updateFactory)(void*, void*))
{
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* newVal = updateFactory(key, existing->value);
        existing->value = newVal;
        rt_cd_lock_release(slock);
        return newVal;
    }
    void* newVal = addValueFactory(key);
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return newVal; }
    n->key = key;
    n->value = newVal;
    n->key_hash = hash;
    n->next = b->head;
    b->head = n;
    ++d->stripe_counts[stripe];
    rt_cd_lock_release(slock);
    return newVal;
}

int32_t rt_concurrent_dict_contains(void* dict, void* key) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    rt_cd_table_t* t = load_table(d);
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (n->key && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key)))
            return 1;
    }
    return 0;
}

int32_t rt_concurrent_dict_count(void* dict) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t total = 0;
    for (int32_t i = 0; i < RT_CD_STRIPES; i++)
        total += atomic_read(&d->stripe_counts[i]);
    return total;
}

void rt_concurrent_dict_clear(void* dict) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    lock_all_stripes(d);
    rt_cd_table_t* t = load_table(d);
    for (int32_t i = 0; i < t->bucket_count; i++) {
        rt_cd_bucket_t* b = &t->buckets[i];
        rt_cd_node_t* n = b->head;
        while (n) {
            rt_cd_node_t* next = n->next;
            rt_cd_node_recycle(d, n);
            n = next;
        }
        b->head = NULL;
    }
    memset(d->stripe_counts, 0, sizeof(d->stripe_counts));
    /* deferred 旧表节点回收须在持有全部 stripe 锁的域内完成——回收无独立
     * 池锁（alloc 需 stripe 锁，故此时无并发 alloc 与之竞争）。 */
    free_deferred_tables(d);
    unlock_all_stripes(d);
}

/* ---- snapshot helpers: keys, values, to_array ---- */
//
// 这些 helper 在快照后遍历当前代桶；若遍历期间发生 resize，
// 已快照的旧桶可能为空（节点已迁移）——快照不完整但不崩溃。
// 调用方应理解并发快照的 stale 语义（RFC 024 §4.1）。

/* 遍历当前代全部桶（持全部 stripe 锁）：计数 / 收集 live 节点。
 * max_collect > 0 时收集到该数即停（防两趟遍历间 resize 导致计数漂移越界）。 */
static void rt_cd_walk_live(rt_concurrent_dict_t* d,
    void (*cb)(void* key, void* value, void* ctx), void* ctx,
    int32_t* out_count, int32_t max_collect)
{
    rt_cd_table_t* t = load_table(d);
    int32_t total = 0;
    lock_all_stripes(d);
    for (int32_t i = 0; i < t->bucket_count; i++) {
        for (rt_cd_node_t* n = t->buckets[i].head; n; n = n->next) {
            if (n->key != NULL) {
                total++;
                if (cb && total <= max_collect) cb(n->key, n->value, ctx);
            }
        }
    }
    unlock_all_stripes(d);
    if (out_count) *out_count = total;
}

typedef struct { void** items; int32_t idx; int32_t slot; } cd_collect_t;

static void cd_collect_key(void* key, void* value, void* ctx) {
    (void)value;
    cd_collect_t* c = (cd_collect_t*)ctx;
    c->items[c->idx * c->slot] = key;
    c->idx++;
}
static void cd_collect_value(void* key, void* value, void* ctx) {
    (void)key;
    cd_collect_t* c = (cd_collect_t*)ctx;
    c->items[c->idx * c->slot] = value;
    c->idx++;
}
static void cd_collect_pair(void* key, void* value, void* ctx) {
    cd_collect_t* c = (cd_collect_t*)ctx;
    c->items[c->idx * 2]     = key;
    c->items[c->idx * 2 + 1] = value;
    c->idx++;
}

void* rt_concurrent_dict_keys(void* dict) {
    if (!dict) return NULL;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t cnt = 0;
    rt_cd_walk_live(d, NULL, NULL, &cnt, 0);
    void* arr = rt_array_create(cnt, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    cd_collect_t c = { items, 0, 1 };
    rt_cd_walk_live(d, cd_collect_key, &c, NULL, cnt);
    return arr;
}

void* rt_concurrent_dict_values(void* dict) {
    if (!dict) return NULL;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t cnt = 0;
    rt_cd_walk_live(d, NULL, NULL, &cnt, 0);
    void* arr = rt_array_create(cnt, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    cd_collect_t c = { items, 0, 1 };
    rt_cd_walk_live(d, cd_collect_value, &c, NULL, cnt);
    return arr;
}

void* rt_concurrent_dict_to_array(void* dict) {
    /* ToArray for ConcurrentDictionary returns an array of KeyValuePair-like structs.
     * Each element is a { key_ptr, value_ptr } pair (two void*). */
    if (!dict) return NULL;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t cnt = 0;
    rt_cd_walk_live(d, NULL, NULL, &cnt, 0);
    void* arr = rt_array_create(cnt * 2, (int32_t)sizeof(void*));
    if (!arr) return NULL;
    void** items = (void**)arr;
    cd_collect_t c = { items, 0, 2 };
    rt_cd_walk_live(d, cd_collect_pair, &c, NULL, cnt);
    return arr;
}
