// ConcurrentDictionary<K,V> implementation (RFC 024 M1).
//
// Design (2026-08-03 轨道 G 性能重构 · RFC 036 §2.3 / V1-SPRINT 轨道 G):
//   - Striped locks: 固定 RT_CD_STRIPES 把常驻自旋锁（CAS + 让步退避）分片池，
//     桶通过 hash 分片共享；无 malloc/无锁竞争，锁对象常驻 cache；无竞争往返
//     ~5-10ns（SRWLOCK 的 ~30-40ns 是 1t 基准 per-op 主成本）
//     （单线程 TryAdd 的 per-bucket 锁内 cache + resize mutex 创建是 6.3× 主成本）。
//   - Lock-free read：table 指针原子发布（immutable table），读路径一次 acquire load
//     即得一致对 (bucket_count, buckets)；resize 只串行迁移，读不碰 table_lock。
//   - 写路径先取 stripe 锁、再 load table → 消除「快照后 resize 已发布」的丢失窗口
//     （比旧版 table_lock 快照 + 桶锁的两步时序更严谨）。
//   - Safe reclamation: 删除节点仅标记 deleted（key=NULL），不立即 free，
//     鏃?table 整体延迟释放（deferred list），读路径快照后永不 use-after-free銆?
//   - 公开 ABI 不变（rt_abi.h 未动）；语义对齐 RFC 024 §4.1（stale-but-safe 读）。
//
// 旧设计（保留注释供追溯）：per-bucket mutex、table_lock 每 op 快照。
//   - 每 op 取 table_lock（SRW 全局锁）→ resize 时交换 buckets 指针；
//   - 每桶一个 SRWLOCK，resize 为每个新桶 rt_mutex_create（~24ns×桶数）；
//   - 单线程 TryAdd 实测 ~165-470 ns/op vs .NET ~26 ns/op（6.3×）。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* RFC 051 S3d: value-ownership variant (rt_concurrent_dict_create_*_owned).
 *
 * Protocol (storage-side ownership, mirrors rt_dict/SortedDictionary owned):
 *   - Values must be ArcHeader objects (class instances / iface fat boxes).
 *   - Insert (TryAdd new / set new key / GetOrAdd miss / AddOrUpdate insert)
 *     retains the value once for storage, UNDER the bucket stripe lock.
 *   - Remove/overwrite/TryUpdate/clear/destroy release the removed entry
 *     value (rt_arc_dec) AFTER the stripe lock is released (finalizer
 *     reentry safe: slot already points at the new value / node already
 *     dead). TryRemove does NOT dec: the storage-held +1 transfers to the
 *     caller's out slot (ownership move, caller epilogue dec pairs it).
 *   - Read arms (TryGetValue / get_or_default / GetOrAdd hit-return) borrow:
 *     the value is retained (rt_arc_inc) while holding the stripe lock and
 *     the caller-side post-call inc is REMOVED in codegen for ref values -
 *     lock-held inc serializes with the lock-held release of remove/overwrite,
 *     closing the read-vs-remove interleaving that could inc a freed object.
 *     Legacy (scalar/string values) keeps the lock-free read path and the
 *     create_owned-less ABI untouched (RFC 024 s4.1 stale-but-safe).
 *   - clear/destroy walk live entries while holding all stripes (or under
 *     the exclusive teardown contract), collect the retained values, and
 *     release them after unlocking; the `clearing` guard makes a value
 *     finalizer that re-enters clear/destroy a no-op.
 *   - Snapshot helpers (keys/values/to_array) remain borrow views (no
 *     per-element retain) - same documented semantics as Dictionary.
 */

#ifdef _WIN32
#include <windows.h>
#else
#include <sched.h>
#include <time.h>
#endif

#define RT_CD_STRIPES 64 /* 2^6，桶→stripe 用位与 */

/* ---- 轻量 stripe 锁（自旋 + 让步退避） ----
 * 单线程低竞争下 SRWLOCK 的 Acquire+Release 往返 ~30-40ns；用 CAS 自旋锁
 * （~5-10ns 无竞争往返）降低 per-op 锁开销（concurrent_dict_1t 基准主成本）。
 * 临界区为极短的桶操作；自旋达阈值后切线程休眠让步，多线程下仍正确
 * （见 lock_all_stripes：resize 持 table_lock 后取 stripe 加锁，锁序无环）。
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
 * 低位分布差的问题。*/
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
    void*              key;        /* key storage; NULL is a VALID int key 0 (inttoptr) */
    void*              value;
    int32_t            key_hash;
    int32_t            dead;       /* tombstone marker (removed): key=NULL safe-reclamation
                                      marker must not collide with live int key 0 - RFC
                                      051 S3d fix: liveness = !dead, key NULL live allowed */
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

/* ---- Node pool（批量分配，替代逐节点 calloc）----
 * 节点生命周期：插入后经 resize 代存活（resize 迁移不 free），至 clear 时回收。
 * 批量按 2 的幂 calloc；回收的节点经 freelist 复用。池按 stripe 分片，
 * 分配在持 stripe 锁的域内完成（无共享池锁），回收仅在 clear 的全 stripe
 * 锁域内发生（含并入该域的 deferred 表释放）；resize 走 table_lock→stripes，
 * 不触池，无锁序环）。*/
#define RT_CD_NODE_BATCH 1024

typedef struct rt_cd_node_batch {
    struct rt_cd_node_batch* next;
    rt_cd_node_t             nodes[RT_CD_NODE_BATCH];
} rt_cd_node_batch_t;

/* 姣?stripe 独立的节点池（无共享 node_lock）：
 * 写路径已在持 stripe 锁的域内分配节点，故池访问天然互斥——省去逐次插入
 * 的全局 mutex（单线程 TryAdd 的 node_lock 锁开销是热路径主成本之一）。*/
typedef struct rt_cd_pool {
    rt_cd_node_batch_t* node_batches;  // 每 stripe 的批量链
    rt_cd_node_t*       free_nodes;    // clear 回收节点（统一进 stripe 0 池）
    int32_t             batch_idx;     // 当前批内已分配数
} rt_cd_pool_t;

typedef struct rt_concurrent_dict_t {
    uint32_t              (*hash_fn)(void*);
    int32_t               (*eq_fn)(void*, void*);
    rt_cd_table_t*        table;           // 原子 load/store（lock-free read）
    rt_cd_lock_t          stripe_locks[RT_CD_STRIPES];  // 分片自旋锁（常驻、无 malloc）
    int32_t               stripe_counts[RT_CD_STRIPES];  // 姣?stripe 计数（stripe 锁内非原子增减）
    void*                 table_lock;      // 仅序列化 resize（迁移互斥）
    rt_cd_deferred_t*     deferred_old_tables;  // 鏃?table 延迟释放链表
    rt_cd_pool_t          pools[RT_CD_STRIPES];  // 每 stripe 节点池（无共享锁）
    int32_t               owned;    /* RFC 051 S3d: value-ownership variant flag */
    int32_t               clearing; /* RFC 051 S3d: clear/destroy teardown guard
                                       (value finalizer re-entry no-op) */
} rt_concurrent_dict_t;

/* 分配节点：从 stripe_idx 对应池取（优先 freelist，其次当前批次）。
 * 调用方须已持有该 stripe 锁，无需额外互斥。返回零初始化节点。*/
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
    n->dead = 0;
    n->next = NULL;
    return n;
}

/* 回收节点到 stripe 0 池的 freelist。仅在 clear 的全 stripe 锁域内调用
 * （含并入该域的 free_deferred_tables），与并发 alloc 无竞争
 * （alloc 需要取 stripe 锁，clear 持有全部 stripe 锁）。*/
static void rt_cd_node_recycle(rt_concurrent_dict_t* d, rt_cd_node_t* n) {
    n->key = NULL;
    n->value = NULL;
    n->key_hash = 0;
    n->dead = 0;
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

static void* rt_concurrent_dict_create_impl(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t bucket_count,
    int32_t owned)
{
    if (bucket_count <= 0) bucket_count = 31;
    int32_t n = next_pow2(bucket_count);

    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)calloc(1, sizeof(*d));
    if (!d) return NULL;
    d->hash_fn = hash;
    d->eq_fn = eq;
    d->deferred_old_tables = NULL;
    d->owned = owned;
    d->clearing = 0;

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

/* RFC 051 S3d: value-ownership variants. See the header note above the
 * includes for the full protocol. Scalar/string value dictionaries must use
 * the legacy create fns (no ARC maintenance, lock-free reads preserved). */
void* rt_concurrent_dict_create_owned(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t bucket_count)
{
    return rt_concurrent_dict_create_impl(hash, eq, bucket_count, 1);
}

void* rt_concurrent_dict_create_level_owned(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t concurrency_level)
{
    return rt_concurrent_dict_create_impl(hash, eq, concurrency_level * 4, 1);
}

void* rt_concurrent_dict_create_level_cap_owned(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*),
    int32_t concurrency_level, int32_t capacity)
{
    int32_t n = concurrency_level > 0 ? concurrency_level * 4 : 31;
    if (capacity > n) n = capacity;
    return rt_concurrent_dict_create_impl(hash, eq, n, 1);
}

void* rt_concurrent_dict_create(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t bucket_count)
{
    return rt_concurrent_dict_create_impl(hash, eq, bucket_count, 0);
}

void* rt_concurrent_dict_create_level(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*), int32_t concurrency_level)
{
    return rt_concurrent_dict_create_impl(hash, eq, concurrency_level * 4, 0);
}

void* rt_concurrent_dict_create_level_cap(
    uint32_t (*hash)(void*), int32_t (*eq)(void*, void*),
    int32_t concurrency_level, int32_t capacity)
{
    int32_t n = concurrency_level > 0 ? concurrency_level * 4 : 31;
    if (capacity > n) n = capacity;
    return rt_concurrent_dict_create_impl(hash, eq, n, 0);
}

/* ---- Internal: find live node in bucket (caller holds bucket's stripe lock) ---- */

static rt_cd_node_t* find_in_bucket(rt_cd_bucket_t* b, void* key, int32_t hash,
    int32_t (*eq)(void*, void*))
{
    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (!n->dead && n->key_hash == hash && (n->key == key || eq(n->key, key)))
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
    int32_t new_n = cur->bucket_count * 4; /* 幂二翻两倍：4× 末代桶内存 ~1MB 可驻 L2；2× 增长下 4MB 反致随机桶访存出 L2 */
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

    /* 持全部 stripe 锁：排除所有写路径，读路径照常 lock-free（stale-but-safe）*/
    lock_all_stripes(d);

    for (int32_t i = 0; i < cur->bucket_count; i++) {
        rt_cd_node_t* n = cur->buckets[i].head;
        cur->buckets[i].head = NULL;
        while (n) {
            rt_cd_node_t* next = n->next;
            if (!n->dead) {
                int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)n->key_hash), new_n);
                n->next = neu->buckets[bidx].head;
                neu->buckets[bidx].head = n;
            } else {
                /* deleted 墓碑：留在旧 table，重建旧桶链以便 deferred 释放
                 * （绝不提前 free；读路径快照后可能仍持指针）。*/
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
        /* 内存不足：泄漏旧 table（安全降级——仅内存浪费，无 use-after-free）*/
    }

    rt_mutex_unlock(d->table_lock);
}

/* 回收所有 deferred 旧 table 的节点并释放表结构（仅在 clear 无并发时调用；
 * 节点归池（批次内存），不逐节点 free——批次随 dict 生命周期持有。*/
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
// 写路径统一约定：先取 stripe 锁，再 load table（resize 持全部 stripe 锁发布，
// 故「取锁后 load」得到稳定 table，消除快照后 resize 的丢失窗口）。

int32_t rt_concurrent_dict_try_add(void* dict, void* key, void* value) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    if (find_in_bucket(b, key, hash, d->eq_fn)) {
        rt_cd_lock_release(slock);
        return 0;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    /* RFC 051 S3d (owned): storage-side +1 for the inserted entry. Dup-fail
     * paths above never retain (no orphan +1 on failed TryAdd). */
    if (d->owned) rt_arc_inc(value);
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
 * miss 须清 out 槽（对齐 rt_dict_try_get_value；禁未初始化泄漏 → Assert.Equal(0,v) 假红）。*/
/* TryGetValue (borrow): owned variant retains the value under the stripe
 * lock (serialized with remove/overwrite releases - no inc-after-free);
 * legacy keeps the lock-free path (RFC 024 s4.1 stale-but-safe). Caller-side
 * borrow inc is removed in codegen for ref values (RFC 051 S3d). */
int32_t rt_concurrent_dict_try_get(void* dict, void* key, void** out_value) {
    if (!out_value) return 0;
    *out_value = NULL;
    if (!dict) return 0;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    rt_cd_lock_t* rlock = NULL;
    if (d->owned) {
        rlock = &d->stripe_locks[(uint32_t)hash & (RT_CD_STRIPES - 1)];
        rt_cd_lock_acquire(rlock);
    }
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];
    int32_t found = 0;

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (!n->dead && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key))) {
            *out_value = n->value;
            if (d->owned) rt_arc_inc(n->value); /* borrow retain under lock */
            found = 1;
            break;
        }
    }
    if (rlock) rt_cd_lock_release(rlock);
    return found;
}

void rt_concurrent_dict_set(void* dict, void* key, void* value) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* old = existing->value;
        /* RFC 051 S3d (owned): retain the new value before the slot swap so
         * the slot is consistent while the old value is released (outside
         * the lock, below - finalizer re-entry safe). */
        if (d->owned) rt_arc_inc(value);
        existing->value = value;
        rt_cd_lock_release(slock);
        if (d->owned) rt_arc_dec(old);
        return;
    } else {
        rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
        if (!n) { rt_cd_lock_release(slock); return; }
        /* RFC 051 S3d (owned): storage-side +1 for the new key */
        if (d->owned) rt_arc_inc(value);
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
    if (!d) return NULL;
    int32_t hash = d->hash_fn(key);
    rt_cd_lock_t* rlock = NULL;
    if (d->owned) {
        rlock = &d->stripe_locks[(uint32_t)hash & (RT_CD_STRIPES - 1)];
        rt_cd_lock_acquire(rlock);
    }
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];
    void* result = NULL;

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (!n->dead && n->key_hash == hash && d->eq_fn(n->key, key)) {
            result = n->value;
            if (d->owned) rt_arc_inc(n->value); /* RFC 051 S3d borrow retain */
            break;
        }
    }
    if (rlock) rt_cd_lock_release(rlock);
    return result;
}

/* Safe removal (logical delete): 找到节点后仅标记 key=NULL / value=NULL，不脱链。
 * 读路径 lock-free 遍历时跳过 deleted 节点（RFC 024 §4.1 stale-but-safe）；
 * 节点留在当前代桶中作为墓碑，resize 迁移时不带走（随旧 table 延迟释放），
 * clear 时整体 free。绝不立即 free：快照中的读路径可能仍持指针。*/
int32_t rt_concurrent_dict_try_remove(void* dict, void* key, void** out_value) {
    /* RFC 051 S3d: value ownership MOVE on hit - the storage-held +1 of the
     * removed entry transfers to *out_value (no dec here; the caller's out
     * local epilogue dec pairs it). Miss clears out (NULL). */
    if (!out_value) return 0;
    *out_value = NULL; /* miss 须清 out（与 try_get / rt_dict 同契约） */
    if (!dict) return 0;
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (!n->dead && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key))) {
            *out_value = n->value;
            n->key = NULL;
            n->value = NULL;
            n->dead = 1;
            --d->stripe_counts[stripe];
            rt_cd_lock_release(slock);
            return 1;
        }
    }
    rt_cd_lock_release(slock);
    return 0;
}

/* TryUpdate: CAS on value 鈥?update only if current value equals comparisonValue */
int32_t rt_concurrent_dict_try_update(void* dict, void* key, void* newValue,
    void* comparisonValue)
{
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    int32_t hash = d->hash_fn(key);
    int32_t stripe = (uint32_t)hash & (RT_CD_STRIPES - 1);
    rt_cd_lock_t* slock = &d->stripe_locks[stripe];
    rt_cd_lock_acquire(slock);
    rt_cd_table_t* t = load_table(d);
    (void)t;
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
    {
        void* old = existing->value;
        /* RFC 051 S3d (owned): retain new, swap slot, release old outside lock */
        if (d->owned) rt_arc_inc(newValue);
        existing->value = newValue;
        rt_cd_lock_release(slock);
        if (d->owned) rt_arc_dec(old);
    }
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
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* hit = existing->value;
        if (d->owned) rt_arc_inc(hit); /* RFC 051 S3d: borrow retain under lock */
        rt_cd_lock_release(slock);
        return hit;
    }
    void* value = factory(key);
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    /* RFC 051 S3d (owned): storage +1 and borrow +1 for the returned value */
    if (d->owned) { rt_arc_inc(value); rt_arc_inc(value); }
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
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* hit = existing->value;
        if (d->owned) rt_arc_inc(hit); /* RFC 051 S3d: borrow retain under lock */
        rt_cd_lock_release(slock);
        return hit;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) { rt_cd_lock_release(slock); return 0; }
    /* RFC 051 S3d (owned): storage +1 and borrow +1 for the returned value */
    if (d->owned) { rt_arc_inc(value); rt_arc_inc(value); }
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
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* newVal = updateFactory(key, existing->value);
        void* old = existing->value;
        /* RFC 051 S3d (owned): retain new, swap slot, release old after
         * unlock (returned value carries the borrow +1 below) */
        if (d->owned) { rt_arc_inc(newVal); rt_arc_inc(newVal); }
        existing->value = newVal;
        rt_cd_lock_release(slock);
        if (d->owned) rt_arc_dec(old);
        return newVal;
    }
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) {
        /* RFC 051 S3d (owned): alloc failure still returns a borrow (+1) */
        if (d->owned) rt_arc_inc(addValue);
        rt_cd_lock_release(slock);
        return addValue;
    }
    /* RFC 051 S3d (owned): storage +1 and borrow +1 for the returned value */
    if (d->owned) { rt_arc_inc(addValue); rt_arc_inc(addValue); }
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
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    rt_cd_node_t* existing = find_in_bucket(b, key, hash, d->eq_fn);
    if (existing) {
        void* newVal = updateFactory(key, existing->value);
        void* old = existing->value;
        /* RFC 051 S3d (owned): retain new, swap slot, release old after unlock */
        if (d->owned) { rt_arc_inc(newVal); rt_arc_inc(newVal); }
        existing->value = newVal;
        rt_cd_lock_release(slock);
        if (d->owned) rt_arc_dec(old);
        return newVal;
    }
    void* newVal = addValueFactory(key);
    rt_cd_node_t* n = rt_cd_node_alloc(d, stripe);
    if (!n) {
        /* RFC 051 S3d (owned): alloc failure still returns a borrow (+1) */
        if (d->owned) rt_arc_inc(newVal);
        rt_cd_lock_release(slock);
        return newVal;
    }
    /* RFC 051 S3d (owned): storage +1 and borrow +1 for the returned value */
    if (d->owned) { rt_arc_inc(newVal); rt_arc_inc(newVal); }
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
    (void)t;
    int32_t bidx = (int32_t)rt_cd_bucket_index(rt_cd_mix_hash((uint32_t)hash), t->bucket_count);
    rt_cd_bucket_t* b = &t->buckets[bidx];

    for (rt_cd_node_t* n = b->head; n; n = n->next) {
        if (!n->dead && n->key_hash == hash && (n->key == key || d->eq_fn(n->key, key)))
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
    if (!d) return;
    lock_all_stripes(d);
    /* RFC 051 S3d: value finalizer re-entry during our release is a no-op */
    if (d->clearing) { unlock_all_stripes(d); return; }
    d->clearing = 1;
    rt_cd_table_t* t = load_table(d);
    (void)t;
    /* RFC 051 S3d (owned): collect live entry values before recycling the
     * nodes; released below after the stripes are unlocked (finalizer
     * re-entry safe - no lock is held during rt_arc_dec). */
    void** vals = NULL;
    int32_t nvals = 0;
    int32_t k = 0;
    if (d->owned) {
        int32_t cap = 0;
        for (int32_t i = 0; i < t->bucket_count; i++) {
            for (rt_cd_node_t* m = t->buckets[i].head; m; m = m->next) {
                if (!m->dead) cap++;
            }
        }
        if (cap > 0) {
            vals = (void**)calloc((size_t)cap, sizeof(void*));
            if (vals) nvals = cap;
        }
    }
    for (int32_t i = 0; i < t->bucket_count; i++) {
        rt_cd_bucket_t* b = &t->buckets[i];
        rt_cd_node_t* n = b->head;
        while (n) {
            rt_cd_node_t* next = n->next;
            if (vals && !n->dead) vals[k++] = n->value;
            rt_cd_node_recycle(d, n);
            n = next;
        }
        b->head = NULL;
    }
    memset(d->stripe_counts, 0, sizeof(d->stripe_counts));
    /* deferred 旧表节点回收须在持有全部 stripe 锁的域内完成——回收无独立
     * 池锁（alloc 需 stripe 锁，故此时无并发 alloc 与之竞争）。*/
    free_deferred_tables(d);
    unlock_all_stripes(d);
    /* RFC 051 S3d (owned): release collected entry values outside the locks */
    if (vals) {
        for (int32_t i = 0; i < nvals; i++) rt_arc_dec(vals[i]);
        free(vals);
    }
    d->clearing = 0;
}

/* ---- snapshot helpers: keys, values, to_array ---- */
//
// 这些 helper 在快照后遍历当前代桶；若遍历期间发生 resize，
// 已快照的旧表可能为空（节点已迁移）——快照不完整但不崩溃。
// 调用方应理解并发快照的 stale 语义（RFC 024 §4.1）。

/* 遍历当前代全部桶（持全部 stripe 锁）：计数 / 收集 live 节点。
 * max_collect > 0 时收集到该数即停（防两趟遍历间 resize 导致计数漂移越界）。*/
static void rt_cd_walk_live(rt_concurrent_dict_t* d,
    void (*cb)(void* key, void* value, void* ctx), void* ctx,
    int32_t* out_count, int32_t max_collect)
{
    rt_cd_table_t* t = load_table(d);
    (void)t;
    int32_t total = 0;
    lock_all_stripes(d);
    for (int32_t i = 0; i < t->bucket_count; i++) {
        for (rt_cd_node_t* n = t->buckets[i].head; n; n = n->next) {
            if (!n->dead) {
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

/* RFC 051 S3d: destroy (wrapper vtable finalizer; exclusive teardown).
 * 释放当前/延迟表 + 节点批次 + 锁 + 头。节点全部位于池批次内（含延迟表
 * 墓碑），批次统一释放；条目值释放由 owned 变体另行收集（后续轮）。 */
void rt_concurrent_dict_destroy(void* dict) {
    rt_concurrent_dict_t* d = (rt_concurrent_dict_t*)dict;
    if (!d) return;
    /* RFC 051 S3d: re-entrant destroy (value finalizer firing during our
     * release loop) is a no-op - this frame owns the teardown. */
    if (d->clearing) return;
    d->clearing = 1;
    rt_cd_table_t* t = load_table(d);
    (void)t;
    /* RFC 051 S3d (owned): collect live entry values first; they are
     * released below while the handle and table are still alive - a value
     * finalizer that re-enters sees a consistent, guarded dictionary
     * (clear/destroy no-op; set/remove operate on live structures). */
    void** vals = NULL;
    int32_t nvals = 0;
    if (d->owned) {
        int32_t cap = 0;
        for (int32_t i = 0; i < t->bucket_count; i++) {
            for (rt_cd_node_t* m = t->buckets[i].head; m; m = m->next) {
                if (!m->dead) cap++;
            }
        }
        if (cap > 0) {
            vals = (void**)calloc((size_t)cap, sizeof(void*));
            if (vals) nvals = cap;
        }
    }
    int32_t k = 0;
    if (vals) {
        for (int32_t i = 0; i < t->bucket_count; i++) {
            for (rt_cd_node_t* m = t->buckets[i].head; m; m = m->next) {
                if (!m->dead) vals[k++] = m->value;
            }
        }
    }
    rt_cd_deferred_t* df = d->deferred_old_tables;
    while (df) {
        rt_cd_deferred_t* next = df->next;
        free(df->table->buckets);
        free(df->table);
        free(df);
        df = next;
    }
    d->deferred_old_tables = NULL;
    if (vals) {
        for (int32_t i = 0; i < nvals; i++) rt_arc_dec(vals[i]);
        free(vals);
    }
    free(t->buckets);
    free(t);
    for (int32_t i = 0; i < RT_CD_STRIPES; i++) {
        rt_cd_node_batch_t* b = d->pools[i].node_batches;
        while (b) {
            rt_cd_node_batch_t* nb = b->next;
            free(b);
            b = nb;
        }
    }
    /* table_lock 由 rt_mutex_create 分配（raw malloc/SRWLOCK）——必须 plain
     * free；rt_mutex_destroy 走 rt_obj_free（opaque 头语义）会越界释放
     * malloc 块前 16B → 0xC0000374（既有 rt_* 契约错配，另登记）。 */
    free(d->table_lock);
    free(d);
}
