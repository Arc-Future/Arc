// Atomic reference counting ABI (RFC 015 Phase B + RFC 005 weak references +
// RFC 005 M3/M4 cycle collector).
//
// ArcHeader layout (16 bytes — unchanged from pre-RFC-090 ABI):
//   offset 0: _Atomic int32_t refcount   (strong refs)
//   offset 4: _Atomic int32_t weakcount  (RFC 005; reuses the 4B padding slot
//                                         that preceded the 8B-aligned vtable)
//   offset 8: const void* vtable
//
// Keeping the header at 16B means all existing field offsets (HEADER_SIZE = 16
// in crates/typeck/src/layout.rs) stay valid; only the previously-unused
// padding bytes between refcount and vtable now carry a meaning. This narrows
// the RFC 005 §2.1 ABI impact from "24B + offset shift" to "16B, zero shift".
//
// Weak ref semantics (RFC 005 §2.2):
//   - rt_arc_dec on last strong ref: if weakcount == 0, free; else keep the
//     header (and the allocation) so surviving Weak<T> slots can still
//     atomically observe refcount == 0 and report "target gone" via TryGet.
//   - rt_arc_weak_create(target): inc target.weakcount and stash the target
//     pointer in a malloc'd RtWeak slot returned to the caller. The slot is
//     opaque to Arc user code — the codegen stores it inside the Weak<T>
//     Arc object's _target field (offset 16).
//   - rt_arc_weak_try_get(slot): atomic CAS-inc on target.refcount; returns
//     the target pointer (now strong-retained) or NULL if refcount already 0.
//   - rt_arc_weak_destroy(slot): dec target.weakcount; if it drops to 0 and
//     refcount is also 0, free the target header; always free the slot.
//
// Cycle collector (RFC 005 M3+M4 · Nim ORC 试删):
//   - `rt_arc_dec` on an object that can participate in a cycle (vtable slot 2
//     = `__walk_{cname}` is non-null) registers it as a potential cycle root
//     when an edge into it is removed (refcount > 0 after the dec) — the Nim
//     ORC `rememberCycle` trigger (register on edge destruction), not just the
//     rc→0 case. This is what makes a *pure* cycle (members stuck at rc=1,
//     each referenced only by the other) observable at all: with header-only
//     local drops the members never reach rc=0, so a strict rc→0 trigger would
//     never see them.
//   - When refcount drops to 0 (prev==1) and the object has a walk fn, no
//     Weak<T> observes it, and cycle collection is enabled, the object is
//     *deferred* (pushed to the candidate queue, finalizer NOT run) instead of
//     freed. Deferral also fixes the mutual-cycle finalizer UAF: A→B,B→A where
//     one member hits 0 no longer frees its header before the other's
//     finalizer decs it.
//   - `rt_arc_collect_cycles()` runs the trial-deletion: DFS the reachable
//     closure of every candidate via `rt_arc_walk_fields`, count intra-closure
//     incoming refs, and free the closure iff every member's
//     (real_rc − intra_closure_incoming) is 0. Otherwise the closure is kept
//     and its candidates are dropped (may leak, never dangles).
//
// DFS stack safety (RFC 005 §2.5):
//   - The trial-deletion mark pass is a DFS over strong references. A
//     pathologically deep object graph (e.g. a deep UI tree) used to recurse
//     ~one C stack frame per graph level via `collect_visit` and could
//     overflow the C stack.
//   - The DFS is now iterative, driven by an explicit per-thread stack
//     (`g_dfs_stack`); the C stack depth stays constant regardless of the
//     object graph's shape or depth. The stack capacity is provably bounded:
//     each object is pushed at most once and the closure never exceeds
//     COLLECT_MAX nodes, so DFS_STACK_MAX = COLLECT_MAX always suffices (a
//     defensive check still abandons the round if the bound is ever exceeded).
//   - Overflow semantics: when the closure outgrows COLLECT_MAX, the whole
//     round is abandoned — every candidate pin is released and the queue is
//     cleared, nothing is freed. The cycle leaks until a future pass; it never
//     dangles and is never partially freed. Closures at or below the bound are
//     analyzed exactly as before: identical visited set, intra-closure counts
//     and garbage verdicts (the finalize/free order of a garbage closure may
//     differ from the old recursion's — semantically unobservable, since every
//     member's refcount is zeroed before any finalizer runs).
//
// Concurrency posture (RFC 005 §2.2 方案 B — TLS/per-thread queues):
// the candidate queue, in-flight marker and closure analysis arrays are
// `_Thread_local`, so `rt_arc_dec` registers into the *calling* thread's
// queue and `rt_arc_collect_cycles` only collects the *calling* thread's
// queue — zero shared-write contention on the hot path. A registered
// candidate holds an internal +1 refcount pin (see candidate_push) so a
// collect pass on another thread can never free a queued object out from
// under its owner; cross-thread cycles therefore keep the documented leak
// posture (RFC 005) and never dangle.
//
// RFC 005 milestone ② (always-on): `rt_arc.c` is now compiled with
// `-DARC_CYCLE_COLLECTION` unconditionally (codegen llvm_ir/mod.rs), so
// `g_cycle_collection_enabled` defaults to 1 in every binary and the
// collector is active by default, user-invisible. `rt_arc_set_cycle_collection`
// remains as a test/diagnostic switch (RFC 005 §2.4) to construct an "off"
// control; when the collector is disabled `rt_arc_dec` is bit-for-bit the
// pre-RFC-032 behavior (free immediately on rc→0), so G8 hot paths are
// unaffected in that control.

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h> /* GetModuleHandleExW（野指针哨兵探针，取证后随探针一并移除） */
#include <intrin.h> /* _ReturnAddress（野指针哨兵探针，取证后随探针一并移除） */
#endif

typedef struct {
    _Atomic int32_t refcount;
    _Atomic int32_t weakcount;
    const void* vtable;
} ArcHeader;

// RFC 005 §2.2: opaque weak-ref slot. Returned by rt_arc_weak_create and
// consumed by rt_arc_weak_try_get / rt_arc_weak_destroy. The codegen treats
// the pointer as an opaque handle stored in the Weak<T> Arc object.
//
// RFC 017 §2.6 (热卸载 Weak<T> 边界语义 · 宿主侧弱登记表):
//   `generation` 为可选的模块代数标签（0 = 未关联模块，即宿主/单体内弱引用）。
//   模块边界 `Weak<T>` 由宿主经 rt_library_weak_register(gen, slot) 登记进
//   ALC 宿主内存并盖上代数；模块卸载时 rt_library_unload_hot 对已登记槽位
//   调用 rt_arc_weak_neutralize —— 槽位 target 置空（幂等）→ 卸载后
//   TryGet() 确定性返回 NULL（观察 tombstone 头语义，禁悬垂复活）。
typedef struct {
    void* target;
    int32_t generation; /* RFC 017 §2.6: 关联模块代数；0 = 未关联 */
} RtWeak;

// ---- RFC 005 M3/M4: cycle collector state ----

// RFC 005 milestone ②（always-on）：codegen 以 `-DARC_CYCLE_COLLECTION`
// **无条件恒编译**（llvm_ir/mod.rs），故本开关在每二进制内默认 1——收集器
// 编译进每个二进制、默认开启、用户无感（RFC 005 §0/§2.1）。`#ifdef` 分支
// 保留以支持脱离 codegen 直接编译 rt_arc.c 的降级路径（此时默认 0，行为与
// RFC 005 之前逐位一致）。
// 读点保留（RFC 005 §2.4）：`rt_arc_set_cycle_collection` 为测试/诊断专用
// 开关，可构造「关闭对照」；用户面无任何关闭循环收集的途径。
#ifdef ARC_CYCLE_COLLECTION
static _Atomic int32_t g_cycle_collection_enabled = 1;
#else
static _Atomic int32_t g_cycle_collection_enabled = 0;
#endif

// RFC 005 §2.2 方案 B（TLS / per-thread 队列）：候选队列、收集重入守卫、
// 闭包分析数组全部为本线程私有状态——`rt_arc_dec` 写调用线程队列，
// `rt_arc_collect_cycles` 仅收集调用线程队列，热路径零共享写竞争。
//
// `_Thread_local` 可用性：rt_arc.c 由 clang 编译（含 Windows MSVC target），
// clang 的 `_Thread_local` 在 MSVC target 下由 LLVM 后端映射为
// `__declspec(thread)` 等价实现，可直接使用；原生 MSVC `cl` 自 VS2019 16.8
// 起才支持 `_Thread_local`，因此对「非 clang 的 MSVC」回退 `__declspec(thread)`。
#if defined(_MSC_VER) && !defined(__clang__)
#define ARC_TLS __declspec(thread)
#else
#define ARC_TLS _Thread_local
#endif

// Candidate queue (Nim ORC "potential cycle roots"). Fixed-size, no lock —
// per-thread TLS private (see file header / RFC 005 §2.2).
#define CANDIDATE_MAX 256
// RFC 005 §2.1 / milestone ②（阈值触发）：候选入队计数达 `CYCLE_TRIGGER_THRESHOLD`
// 即触发本线程一次试删收集——远早于队列满（256），限界去重扫描（n≤32）与滞留
// 内存时间窗上界（RFC 005 §1.2 结论 4）。收集后队列清空，计数自然重置。满队列
// 强制 drain（CANDIDATE_MAX）仅作兜底（阈值触发后正常操作下不再可达）。可调常量，
// 默认 32（RFC 005 §2.1 裁决）。
#define CYCLE_TRIGGER_THRESHOLD 32
ARC_TLS static void* g_candidates[CANDIDATE_MAX];
ARC_TLS static int32_t g_candidate_count = 0;

// Re-entrancy guard (per-thread TLS): finalizers fired during a collect pass
// call `rt_arc_dec` on closure members; without the guard they would
// re-register/re-defer and mutate the queue we are currently draining.
ARC_TLS static int32_t g_collecting = 0;

// Per-pass closure analysis arrays (per-thread TLS).
#define COLLECT_MAX 1024
ARC_TLS static void* g_visited[COLLECT_MAX];
ARC_TLS static int32_t g_intra[COLLECT_MAX];

// Explicit DFS stack (RFC 005 §2.5): the trial-deletion mark pass is driven
// iteratively from this stack instead of recursing on the C stack, so a deep
// object graph can never overflow the C stack. The capacity is provably
// bounded — each object is pushed at most once and the closure never exceeds
// COLLECT_MAX nodes — so DFS_STACK_MAX = COLLECT_MAX always suffices. It is a
// named macro so a future change can lower the stack bound independently of
// the analysis-set bound (a lower bound simply abandons deep closures earlier;
// still never dangles).
#define DFS_STACK_MAX COLLECT_MAX
ARC_TLS static void* g_dfs_stack[DFS_STACK_MAX];

// RFC 005 P2：热路径 O(1) 去重/查重 —— TLS open-addressing hash sets。
//
// 研究文档 §A.1/§A.2 分解出的三大线性扫描全部降为 O(1)：
//   1. `candidate_push` 去重扫描（阈值 32 后 n≤32，平均 ~16 次比较/注册）
//      → TLS 候选 hash set（`g_cand_hash`）O(1) 成员测试；
//   2. 收集 pass 的 `g_visited` 线性查重（closure²）→ TLS epoch-stamped
//      visit map（`g_visit_map`）O(1) 指针→visited 下标映射；
//   3. verdict 的 `candidate_is_queued` 线性扫描 → 与 1 共用候选 hash set。
//
// 选型（open addressing + generation/tombstone，无分配、确定性输出）：
//   - 两表均 `_Thread_local` 固定容量、容量 = 2×队列/闭包上界 ⇒ 装载因子
//     ≤ 0.5，探测链平均 ~1.5 槽；键 = 对象指针，`arc_ptr_hash` 乘性哈希
//     （Knuth 0x9E3779B1，取乘积高 log2 位），无随机种子 ⇒ 与地址无关、
//     结果可复现（测试校验和不受影响；finalize/free 顺序仍由稠密 g_visited
//     发现序驱动，不改用哈希遍历序）。
//   - 候选表 `g_cand_hash`：每槽 {ptr, gen}。槽**在当前代**
//     （gen == g_cand_hash_gen）且 ptr 非空 = 活跃；当前代且 ptr==NULL =
//     墓碑（探测链继续、可复用）；非当前代 = 过期（终止探测链、可复用）。
//     队列清空（收集 pass 结束）= 世代 +1，O(1) 全表作废；同一代内删除置
//     墓碑（探测不中断，正确性等价 backward-shift）。gen 为 uint32，近环绕
//     时整表 memset 兜底一次（否则远古槽会被误认回当前代）。
//   - visit map `g_visit_map`：每槽 {index, epoch}（index 映射到稠密
//     g_visited/g_intra）。pass 起始 epoch+1 即完成整表「清空」（O(1)），
//     pass 内无删除故无需墓碑；查找遇首个非当前 epoch 槽即终止探测链。
//
// 内存布局：候选表 512×16B=8KB + visit map 2048×8B=16KB 追加到既有 TLS
// （候选队列 2KB + 分析数组 8KB+4KB + DFS 栈 8KB ≈ 22KB/线程），合计
// ~46KB/线程；仅线程私有，与既有 TLS 设计一致（RFC 005 §2.2 方案 B）。
#define CANDIDATE_HASH_LOG2 9       // 512 槽 = 2 × CANDIDATE_MAX（装载 ≤ 0.5）
#define CANDIDATE_HASH_CAP (1u << CANDIDATE_HASH_LOG2)
#define CANDIDATE_HASH_MASK (CANDIDATE_HASH_CAP - 1)

typedef struct {
    void* ptr;         // 队列中候选；NULL = 当前代墓碑（探测链继续）
    uint32_t gen;      // 插入世代；≠ 当前世代 ⇒ 过期（终止探测链、可复用）
} CandidateSlot;

ARC_TLS static CandidateSlot g_cand_hash[CANDIDATE_HASH_CAP];
ARC_TLS static uint32_t g_cand_hash_gen = 1;

#define VISIT_HASH_LOG2 11          // 2048 槽 = 2 × COLLECT_MAX（装载 ≤ 0.5）
#define VISIT_HASH_CAP (1u << VISIT_HASH_LOG2)
#define VISIT_HASH_MASK (VISIT_HASH_CAP - 1)

typedef struct {
    int32_t index;     // 本 pass 的 g_visited 下标（映射 ptr → visited/intra）
    uint32_t epoch;    // 插入的 pass 世代；≠ 当前世代 ⇒ 过期
} VisitSlot;

ARC_TLS static VisitSlot g_visit_map[VISIT_HASH_CAP];
ARC_TLS static uint32_t g_visit_epoch = 1;

// 乘性指针哈希：右移对齐位（堆分配 16B 对齐）后乘黄金分割常数，取乘积高
// log2 位作下标。确定性（无随机种子），分布与地址布局解耦。
static inline uint32_t arc_ptr_hash(uintptr_t p, unsigned log2_cap) {
    uint32_t h = (uint32_t)(p >> 4);
    h *= 0x9E3779B1u;
    return h >> (32 - log2_cap);
}

// 候选 hash set：当前代成员测试。遇首个非当前代槽即判定不在（终止探测链）。
// 探测以 CANDIDATE_HASH_CAP 为上界（防御：装载 ≤ 0.5 下实际 ~1.5 槽即止）。
static int candidate_hash_contains(void* ptr) {
    uint32_t g = g_cand_hash_gen;
    uint32_t idx = arc_ptr_hash((uintptr_t)ptr, CANDIDATE_HASH_LOG2);
    for (uint32_t i = 0; i < CANDIDATE_HASH_CAP; i++) {
        CandidateSlot* s = &g_cand_hash[(idx + i) & CANDIDATE_HASH_MASK];
        if (s->gen != g) return 0;        // 过期槽终止探测链
        if (s->ptr == ptr) return 1;
    }
    return 0;                             // 防御：不可能（装载 ≤ 0.5）
}

// 候选 hash set：插入（或复用过期/墓碑槽）。返回 1 成功 / 0 已存在 /
// -1 表耗尽（防御，装载 ≤ 0.5 下不可达）。
static int candidate_hash_insert(void* ptr) {
    uint32_t g = g_cand_hash_gen;
    uint32_t idx = arc_ptr_hash((uintptr_t)ptr, CANDIDATE_HASH_LOG2);
    for (uint32_t i = 0; i < CANDIDATE_HASH_CAP; i++) {
        CandidateSlot* s = &g_cand_hash[(idx + i) & CANDIDATE_HASH_MASK];
        if (s->gen != g) {                // 过期 ⇒ 复用
            s->ptr = ptr;
            s->gen = g;
            return 1;
        }
        if (s->ptr == NULL) {             // 当前代墓碑 ⇒ 复用
            s->ptr = ptr;
            return 1;
        }
        if (s->ptr == ptr) return 0;      // 已存在（去重）
    }
    return -1;                            // 防御：不可能
}

// 候选 hash set：删除（置墓碑，保持探测链）。返回 1 存在 / 0 不存在。
static int candidate_hash_remove(void* ptr) {
    uint32_t g = g_cand_hash_gen;
    uint32_t idx = arc_ptr_hash((uintptr_t)ptr, CANDIDATE_HASH_LOG2);
    for (uint32_t i = 0; i < CANDIDATE_HASH_CAP; i++) {
        CandidateSlot* s = &g_cand_hash[(idx + i) & CANDIDATE_HASH_MASK];
        if (s->gen != g) return 0;        // 过期槽终止探测链 ⇒ 不在
        if (s->ptr == ptr) {
            s->ptr = NULL;                // 墓碑：探测链继续、插入可复用
            return 1;
        }
    }
    return 0;                             // 防御：不可能
}

// 候选 hash set 世代推进（O(1) 全表作废）。uint32 近环绕时整表 memset
// 兜底一次，防止远古槽被误认回当前代。
static void candidate_hash_advance_gen(void) {
    uint32_t g = g_cand_hash_gen;
    if (g == UINT32_MAX) {
        memset(g_cand_hash, 0, sizeof(g_cand_hash));
        g_cand_hash_gen = 1;
    } else {
        g_cand_hash_gen = g + 1;
    }
}

// 释放队列中每个剩余候选的 pin 并清空队列（per-thread TLS），同时作废候选
// hash set（世代 +1）。收集 pass 的溢出放弃路径与正常收尾路径共用。
static void candidate_unpin(void* ptr);   // defined below (queue helpers)
static void candidate_queue_clear(void) {
    for (int32_t i = 0; i < g_candidate_count; i++) {
        candidate_unpin(g_candidates[i]);
    }
    g_candidate_count = 0;
    candidate_hash_advance_gen();
}

// visit map 世代推进（O(1) 每 pass「清空」）。近环绕时整表 memset 兜底一次。
// 返回本 pass 使用的世代戳。
static uint32_t visit_map_advance_epoch(void) {
    uint32_t e = g_visit_epoch;
    if (e == UINT32_MAX) {
        memset(g_visit_map, 0, sizeof(g_visit_map));
        g_visit_epoch = 1;
    } else {
        g_visit_epoch = e + 1;
    }
    return g_visit_epoch;
}

// RFC 005 milestone ① cross-thread safety（§2.2 方案 B 补充 — candidate pin）：
//
// 纯 TLS 化后遗留一个跨线程 use-after-free 隐患：线程 A 队列中登记的对象
// M（如 A→B→A 环的成员，rc 由 A 的 dec 降为 1）可能被线程 B 的 collect pass
// 经字段 walk 到达并判定为垃圾后 free——A 后续 collect 会 walk 已 free 的 M。
//
// 修复：候选登记时对对象持有**内部引用（pin）**——`candidate_push` 对
// refcount +1、出队（`candidate_remove` / collect 队列清空）时 −1。于是：
//   - 任一队列持有某对象的 pin ⇒ 该对象 rc ≥ 1 ⇒ 任何其他线程的试删判定
//     `rc − pin − intra ≠ 0`，**不可能被其他线程 free**（跨线程环因此维持
//     RFC 005 的文档化泄漏姿态，绝不悬垂）；
//   - 试删时对本队列候选减 1，与 pin 的 +1 正好抵消 ⇒ 单线程语义与改造前
//     逐位一致（身份变换）。
//
// 已知限制（记录于 RFC 005 §2.2 注记）：dec 的 fetch_sub 与随后的 pin
// fetch_add 之间有一个亚微秒窗口，理论上另一线程的试删可能读到未含 pin 的
// rc 并误判。RFC 005 里程碑②（always-on）复核结论：该窗口在 -D 构建下与
// 里程碑①时相同（翻转不改变其可达性）——「本线程独占对象」所有权姿态使另一
// 线程试删本队列候选不可达（候选仅本线程登记/收集；字段 walk 只经强引用，
// 跨线程共享对象仍维持 pin 保护），e2e 场景无 AV/DF；留待后续里程碑按需收紧。
typedef struct {
    void** visited;
    int32_t* intra;         // parallel to visited: intra-closure incoming refs
    void** stack;           // explicit DFS stack (RFC 005 §2.5)
    VisitSlot* visit_map;   // RFC 005 P2：O(1) ptr→visited 下标映射（epoch-stamped）
    uint32_t visit_epoch;   // 本 pass 的世代戳（pass 起始 +1 即完成整表清空）
    int32_t visited_count;
    int32_t stack_count;    // number of nodes pending on the DFS stack
    int32_t overflow;       // set when the fixed analysis set fills up
    void* current;          // object whose fields are being walked right now
} WalkState;

void rt_arc_collect_cycles(void);

// ---- candidate queue helpers (per-thread TLS) ----

// Release the candidate pin (internal +1 refcount) an object holds while
// queued. Only called for entries that are actually being dropped from the
// queue (the object is still alive by its real refs, or logically dead and
// deliberately leaked — never dangled).
static void candidate_unpin(void* ptr) {
    ArcHeader* h = (ArcHeader*)ptr;
    atomic_fetch_sub_explicit(&h->refcount, 1, memory_order_relaxed);
}

static void candidate_remove(void* ptr) {
    // RFC 005 P2：O(1) 存在性判定（hash set）；下方队列扫描仅对 hash 确认在队
    // 者执行（小队列 ≤ CANDIDATE_MAX，阈值 32 下实际 ≤ 32），非热路径。
    if (!candidate_hash_remove(ptr)) return;
    for (int32_t i = 0; i < g_candidate_count; i++) {
        if (g_candidates[i] == ptr) {
            g_candidates[i] = g_candidates[g_candidate_count - 1];
            g_candidate_count--;
            candidate_unpin(ptr);
            return;
        }
    }
}

// Like candidate_remove but WITHOUT unpinning — used by the collect pass for
// garbage-closure members whose refcount was already zeroed (the pin is
// subsumed into the zero; the memory is freed moments later). The O(1) hash
// presence test lets non-candidate visited nodes (the common case) skip the
// queue scan entirely.
static void candidate_remove_no_unpin(void* ptr) {
    if (!candidate_hash_remove(ptr)) return;
    for (int32_t i = 0; i < g_candidate_count; i++) {
        if (g_candidates[i] == ptr) {
            g_candidates[i] = g_candidates[g_candidate_count - 1];
            g_candidate_count--;
            return;
        }
    }
}

static int candidate_is_queued(void* ptr) {
    return candidate_hash_contains(ptr);
}

static void candidate_push(void* ptr) {
    // RFC 005 P2：去重由 O(n) 线性扫描改为 TLS hash set O(1) 成员测试——对象每个
    // 队列世代最多登记一次（去重语义不变：不重复 pin，verdict 仍按单 pin 扣减）。
    if (candidate_hash_contains(ptr)) return;
    if (g_candidate_count >= CANDIDATE_MAX) {
        // Queue full — drain first. RFC 005 milestone ②：阈值触发（见下）使
        // 队列正常操作下不再顶满，此分支仅作兜底（backstop）。被推对象要么
        // 存活（rc > 0）要么是全新 rc→0 延迟释放对象（无入边），drain 收集
        // 不可能误释放它（见文件头分析）。
        rt_arc_collect_cycles();
        if (candidate_hash_contains(ptr)) return;
        if (g_candidate_count >= CANDIDATE_MAX) return; // still full: skip
    }
    if (candidate_hash_insert(ptr) != 1) {
        // 防御：仅当 hash 表耗尽（装载 ≤ 0.5 按构造不可达）才进入。drain 一次
        // 复用槽位后重试；仍满则放弃登记（对象靠自身真实 rc 存活，绝不悬垂）。
        rt_arc_collect_cycles();
        if (candidate_hash_contains(ptr)) return;
        if (candidate_hash_insert(ptr) != 1) return;
    }
    // Candidate pin (RFC 005 milestone ①): keep the object alive while queued
    // so a collect pass on another thread can never free it out from under
    // this queue. Released by candidate_remove / the queue clear at pass end.
    ArcHeader* h = (ArcHeader*)ptr;
    atomic_fetch_add_explicit(&h->refcount, 1, memory_order_relaxed);
    g_candidates[g_candidate_count++] = ptr;
    // RFC 005 milestone ②（阈值触发）：候选入队计数达 CYCLE_TRIGGER_THRESHOLD
    // 即触发本线程一次试删收集。`rt_arc_collect_cycles` 结束后队列被清空
    // （计数归零），阈值计数自然重置，不会重复触发。语义与手动/满队列收集
    // 一致：试删失败对象清出队列（可能泄漏，绝不悬垂）。
    // 里程碑⑥遗留边界复核（队列满 drain + Weak 幽灵遍历）：阈值触发使队列
    // 远在满（256）之前即被清空，「满 drain 立即回收」窗口被结构性消除；
    // 试删闭包闭合不变量（垃圾闭包外无任何强引用）保证后续收集不可能经强
    // 引用 walk 已释放成员（Weak 观察者保留的 zombie header 内存仍有效，
    // rc==0 不可被 TryGet 升级），边界确认消除。
    if (g_candidate_count >= CYCLE_TRIGGER_THRESHOLD) {
        rt_arc_collect_cycles();
    }
}

void rt_arc_inc(void* ptr) {
    if (!ptr) return;
    /* RFC 050 三层守卫（取代临时野指针哨兵）：
     * 1) 下界哨兵——小整数/状态值被当对象指针无害化（channels #12 实证 0x2）；
     * 2) opaque magic——模式 A 句柄（对象头自描述）禁计数；
     * 3) kind——预留第二身份维度（未来 kind 扩展不再改守卫）。 */
    if ((uintptr_t)ptr < RT_PTR_FLOOR) return;
    if (*(const uint32_t*)ptr == RT_OBJ_MAGIC) return;
    ArcHeader* h = (ArcHeader*)ptr;
    atomic_fetch_add_explicit(&h->refcount, 1, memory_order_relaxed);
}

void rt_arc_dec(void* ptr) {
    if (!ptr) return;
    /* 同 inc 的三层守卫（RFC 050）：dec 的误判危害更大（1→0 走释放分支）， */
    if ((uintptr_t)ptr < RT_PTR_FLOOR) return;
    if (*(const uint32_t*)ptr == RT_OBJ_MAGIC) return;
    ArcHeader* h = (ArcHeader*)ptr;
    int32_t prev = atomic_fetch_sub_explicit(&h->refcount, 1, memory_order_acq_rel);
    if (prev == 1) {
        // Refcount dropped from 1 → 0; this thread exclusively owns the object.
        // RFC 006 M3：调用 vtable slot 1 finalizer（若有）统一释放嵌套 class
        // 字段引用——修复 header-only drop 的字段泄漏。局部 drop 由 codegen
        // 提前 dec（不单独 dec 字段），归零时才触发 finalizer，无双释放。
        const void** vt = (const void**)h->vtable;
        // RFC 005: if any Weak<T> slot still references this header (weakcount
        // > 0), keep the allocation alive so TryGet can deterministically
        // observe "target gone" (refcount == 0) and return null. The last
        // rt_arc_weak_destroy will free the header when weakcount also hits 0.
        int32_t wc = atomic_load_explicit(&h->weakcount, memory_order_acquire);
        // RFC 005 M3 (deferred free): cycle-capable object with no Weak<T>
        // observer and collection enabled → push to the candidate queue
        // instead of freeing. The finalizer is deliberately NOT run here;
        // rt_arc_collect_cycles fires it on confirmed garbage.
        if (g_cycle_collection_enabled && vt && vt[2] && wc == 0 && !g_collecting) {
            candidate_push(ptr);
            return;
        }
        // Normal path: drop any stale queue entry (the object is dead now) and
        // finalize + free (or keep the header for Weak<T> observers). Inside a
        // collect pass (`g_collecting`) a cycle-capable object reaching this
        // branch is a non-walked field of a garbage member; if it were still
        // queued anywhere, its candidate pin would keep rc ≥ 1 and it would
        // not have hit prev==1, so no stale entry can survive the free.
        candidate_remove(ptr);
        if (vt) {
            void (*finalize)(void*) = (void (*)(void*))vt[1];
            if (finalize) {
                finalize(ptr);
            }
        }
        // Field-level ARC decrement is emitted inline by codegen (arc_drop.rs)
        // before calling this function, so we only manage the header here.
        if (wc == 0) {
            if (getenv("ARC_DBG_FREE")) {
                static void* g_freed_set[4096];
                static int g_freed_n = 0;
                int dup = 0;
                for (int fi = 0; fi < g_freed_n; fi++) {
                    if (g_freed_set[fi] == ptr) { dup = 1; break; }
                }
                if (g_freed_n < 4096) { g_freed_set[g_freed_n++] = ptr; }
                fprintf(stderr, "rt_arc_dec:free%s ptr=%p vt=%p\n", dup ? " DUP" : "", ptr, (void*)h->vtable);
            }
            free(ptr);
        }
    } else if (prev > 1 && g_cycle_collection_enabled && !g_collecting) {
        // RFC 005 M3 (candidate registration): an edge into a cycle-capable
        // object was removed (rc still > 0). Register it as a potential cycle
        // root — the Nim ORC `rememberCycle` trigger. A pure cycle's members
        // sit at rc=1 after their last external ref is dropped, so without this
        // they would never be scanned.
        const void** vt = (const void**)h->vtable;
        if (vt && vt[2]) {
            candidate_push(ptr);
        }
    }
}

int32_t rt_arc_count(void* ptr) {
    if (!ptr) return 0;
    ArcHeader* h = (ArcHeader*)ptr;
    return atomic_load_explicit(&h->refcount, memory_order_acquire);
}

void rt_arc_walk_fields(void* obj, void (*visit)(void* ctx, void* field), void* ctx) {
    if (!obj || !visit) return;
    ArcHeader* h = (ArcHeader*)obj;
    const void** vt = (const void**)h->vtable;
    if (!vt) return;
    // slot 2 = walk 函数（RFC 004）；无 class 字段的 class 为 null。
    void (*walk)(void*, void (*)(void*, void*), void*) = (void (*)(void*, void (*)(void*, void*), void*))vt[2];
    if (walk) {
        walk(obj, visit, ctx);
    }
}

/* ---- RFC 050 统一对象头：模式 A 句柄的 opaque 分配 ---- */

void* rt_obj_alloc_opaque(size_t biz_size) {
    RtOpaqueHead* head = (RtOpaqueHead*)malloc(sizeof(RtOpaqueHead) + biz_size);
    if (!head) return NULL;
    head->magic = RT_OBJ_MAGIC;
    head->kind = RT_OBJKIND_OPAQUE;
    head->reserved[0] = 0;
    head->reserved[1] = 0;
    return (char*)head + sizeof(RtOpaqueHead);
}

void rt_obj_free(void* biz_ptr) {
    if (!biz_ptr) return;
    free((char*)biz_ptr - sizeof(RtOpaqueHead));
}

// RFC 047（透明对象图迁移 · 热重载 L3）：vtable 头重绑原语。
// 仅改写 ArcHeader offset 8 的 vtable 指针——refcount/weakcount 与对象地址
// 均不变，故全部引用值（字段/数组元素/局部/弱槽 target）无需修改，跨代
// 引用边天然成立。非原子写：调用方（rt_library_migrate_instances）保证
// 迁移窗口处于 Freeze 态（无并发访问，in-flight 已收敛）。
// 兼容性（字段布局/vtable 形状）由调用方经 __arc_vtable_registry 双重判定
// 先行保证；此处仅做参数防御。
int32_t rt_arc_retype(void* obj, const void* new_vtable) {
    if (!obj || !new_vtable) return -1;
    ArcHeader* h = (ArcHeader*)obj;
    if (!h->vtable) return -1; /* rodata 字面量/非 ARC 对象不可重绑 */
    h->vtable = new_vtable;
    return 0;
}

const void* rt_arc_vtable_of(void* obj) {
    if (!obj) return NULL;
    return ((ArcHeader*)obj)->vtable;
}

// ---- RFC 005 §2.2: Weak<T> runtime support ----

void* rt_arc_weak_create(void* target) {
    // Arc language rule: `new Weak<T>(null)` is rejected at typeck
    // (E_WEAK_NULL_TARGET). Defensive null check here keeps the C ABI robust
    // if a future caller bypasses typeck (e.g. FFI interop).
    if (!target) return NULL;
    ArcHeader* h = (ArcHeader*)target;
    atomic_fetch_add_explicit(&h->weakcount, 1, memory_order_relaxed);
    RtWeak* slot = (RtWeak*)malloc(sizeof(RtWeak));
    slot->target = target;
    slot->generation = 0; /* 默认未关联模块；边界登记由 rt_library_weak_register 盖上 */
    return slot;
}

/* RFC 017 §2.6: 读取/写入槽位的模块代数标签。0 = 未关联。 */
int32_t rt_arc_weak_generation(void* weakslot) {
    if (!weakslot) return 0;
    RtWeak* slot = (RtWeak*)weakslot;
    return slot->generation;
}

void rt_arc_weak_set_generation(void* weakslot, int32_t generation) {
    if (!weakslot) return;
    RtWeak* slot = (RtWeak*)weakslot;
    slot->generation = generation;
}

/* RFC 017 §2.6: 中和槽位——target 置空（幂等）+ 归还目标 weakcount。模块卸载
 * 路径在登记表锁内调用；此后 TryGet() 确定性返回 NULL（观察 tombstone 头
 * 语义）。target 位于共享堆（dlclose 不释放堆），原子递减 weakcount 安全；
 * 归零后目标 header 随最后一次强引用释放正常回收（不泄漏）。 */
void rt_arc_weak_neutralize(void* weakslot) {
    if (!weakslot) return;
    RtWeak* slot = (RtWeak*)weakslot;
    void* target = slot->target;
    slot->target = NULL;
    if (target) {
        ArcHeader* h = (ArcHeader*)target;
        atomic_fetch_sub_explicit(&h->weakcount, 1, memory_order_acq_rel);
    }
}

void* rt_arc_weak_try_get(void* weakslot) {
    if (!weakslot) return NULL;
    RtWeak* slot = (RtWeak*)weakslot;
    void* target = slot->target;
    if (!target) return NULL;
    ArcHeader* h = (ArcHeader*)target;
    // Atomic "upgrade weak → strong": only succeed if refcount > 0.
    // If refcount == 0 (or went negative during a collect pass's finalizer
    // cascade on an object kept alive solely for Weak<T> observers) the object
    // is logically dead; we must not hand out a new strong reference. CAS loop
    // prevents lost updates against concurrent rt_arc_inc / rt_arc_dec.
    int32_t cur;
    do {
        cur = atomic_load_explicit(&h->refcount, memory_order_acquire);
        if (cur <= 0) return NULL;
    } while (!atomic_compare_exchange_weak_explicit(
        &h->refcount, &cur, cur + 1,
        memory_order_acq_rel, memory_order_acquire));
    return target;
}

void rt_arc_weak_destroy(void* weakslot) {
    if (!weakslot) return;
    RtWeak* slot = (RtWeak*)weakslot;
    /* RFC 017 §2.6: 若槽位登记进模块弱登记表，先移除（锁内幂等；未登记
     * no-op）。已中和槽位 target 为 NULL，此处仅释放槽位本身。 */
    rt_library_weak_untrack(weakslot);
    void* target = slot->target;
    if (target) {
        ArcHeader* h = (ArcHeader*)target;
        int32_t prev = atomic_fetch_sub_explicit(&h->weakcount, 1, memory_order_acq_rel);
        if (prev == 1) {
            // weakcount dropped 1 → 0. If strong refs are also gone, the
            // header was kept alive solely for Weak<T> observers — now free it.
            // `<= 0`: a collect pass's finalizer cascade may have pushed a
            // kept-for-Weak garbage object's refcount from 0 to −1; it is
            // logically dead either way.
            // RFC 005 milestone ①: release this thread's candidate pin first,
            // so a logically-dead header kept alive only by a pin (base rc 0 +
            // pin) is freed here too. Live candidates (base rc ≥ 1) keep rc ≥ 1
            // after the unpin and are correctly retained.
            candidate_remove(target);
            int32_t rc = atomic_load_explicit(&h->refcount, memory_order_acquire);
            if (rc <= 0) {
                free(target);
            }
        }
    }
    free(slot);
}

// ---- RFC 005 M3/M4: cycle collector ----

// Runtime toggle for the cycle collector. Returns the previous state.
int32_t rt_arc_set_cycle_collection(int32_t enabled) {
    return atomic_exchange_explicit(
        &g_cycle_collection_enabled, enabled ? 1 : 0, memory_order_acq_rel);
}

// RFC 005 P2：visit map —— 本 pass 内 ptr → g_visited 下标的 O(1) 查找/插入。
// epoch-stamped open addressing：slot.epoch != 本 pass 世代 ⇒ 过期（终止探测
// 链、可复用）；pass 起始世代 +1 即整表清空（visit_map_advance_epoch）。pass
// 内无删除，无需墓碑。返回 visited 下标，或 -1（不在本 pass 闭包内）。
static int32_t visit_map_find(void* ptr, const WalkState* ws) {
    uint32_t idx = arc_ptr_hash((uintptr_t)ptr, VISIT_HASH_LOG2);
    for (uint32_t i = 0; i < VISIT_HASH_CAP; i++) {
        VisitSlot* s = &ws->visit_map[(idx + i) & VISIT_HASH_MASK];
        if (s->epoch != ws->visit_epoch) return -1;   // 过期槽终止探测链
        if (ws->visited[s->index] == ptr) return s->index;
    }
    return -1;                                        // 防御：不可能
}

// 记录 ptr → index（本 pass）。返回 1 成功 / 0 已存在（去重）/ -1 表耗尽（防御）。
static int visit_map_insert(void* ptr, int32_t index, const WalkState* ws) {
    uint32_t idx = arc_ptr_hash((uintptr_t)ptr, VISIT_HASH_LOG2);
    for (uint32_t i = 0; i < VISIT_HASH_CAP; i++) {
        VisitSlot* s = &ws->visit_map[(idx + i) & VISIT_HASH_MASK];
        if (s->epoch != ws->visit_epoch) {            // 过期 ⇒ 复用
            s->index = index;
            s->epoch = ws->visit_epoch;
            return 1;
        }
        if (ws->visited[s->index] == ptr) return 0;   // 已存在
    }
    return -1;                                        // 防御：不可能
}

// Visit callback for the iterative DFS: counts the edge from `ws->current` to
// `field` and, for a newly-discovered object, marks it visited and pushes it
// onto the explicit DFS stack. It never recurses — the C stack stays at
// constant depth no matter how deep the object graph is (RFC 005 §2.5). The
// drain loop in rt_arc_collect_cycles pops each node and walks its fields.
// Membership (RFC 005 P2) is O(1) via the epoch-stamped visit map instead of a
// linear scan over g_visited.
static void collect_visit(void* ctx, void* field) {
    WalkState* ws = (WalkState*)ctx;
    if (!field) return;
    int32_t idx = visit_map_find(field, ws);
    if (idx >= 0) {
        ws->intra[idx]++;         // intra-closure incoming edge
        return;
    }
    if (ws->visited_count >= COLLECT_MAX) {
        ws->overflow = 1;           // closure too large: abandon this round
        return;
    }
    if (visit_map_insert(field, ws->visited_count, ws) != 1) {
        // Defensive: unreachable (visit map is 2× the closure bound, LF ≤ 0.5).
        // Abandon conservatively — same posture as the COLLECT_MAX overflow.
        ws->overflow = 1;
        return;
    }
    ws->visited[ws->visited_count] = field;
    ws->intra[ws->visited_count] = 1;   // the edge from ws->current → field
    ws->visited_count++;
    if (ws->stack_count >= DFS_STACK_MAX) {
        // Defensive: unreachable today (each object is pushed at most once and
        // visited_count ≤ COLLECT_MAX = DFS_STACK_MAX), but keeps the DFS from
        // ever running past the fixed stack if the analysis logic is edited.
        ws->overflow = 1;
        return;
    }
    ws->stack[ws->stack_count++] = field;
}

// Seed the iterative DFS from one candidate root. The root is marked visited
// (intra incoming = 0) and pushed onto the explicit DFS stack; its fields are
// walked later by the drain loop in rt_arc_collect_cycles. Incoming intra refs
// are counted when other closure members reference it.
static void collect_from_root(void* root, WalkState* ws) {
    if (visit_map_find(root, ws) >= 0) return;
    if (ws->visited_count >= COLLECT_MAX) {
        ws->overflow = 1;
        return;
    }
    if (visit_map_insert(root, ws->visited_count, ws) != 1) {
        // Defensive: unreachable (visit map LF ≤ 0.5); abandon conservatively.
        ws->overflow = 1;
        return;
    }
    ws->visited[ws->visited_count] = root;
    ws->intra[ws->visited_count] = 0;
    ws->visited_count++;
    if (ws->stack_count >= DFS_STACK_MAX) {
        // Defensive bound, same rationale as in collect_visit.
        ws->overflow = 1;
        return;
    }
    ws->stack[ws->stack_count++] = root;
}

void rt_arc_collect_cycles(void) {
    if (!atomic_load_explicit(&g_cycle_collection_enabled, memory_order_acquire)) {
        return;   // inert when disabled
    }
    if (g_collecting) return;   // per-thread reentrancy guard
    g_collecting = 1;

    // 1. Build the union closure of every candidate (trial-deletion mark).
    //    Iterative DFS driven from the explicit stack (RFC 005 §2.5): every
    //    candidate root is marked and pushed, then the stack is drained — each
    //    popped node's fields are walked and newly discovered fields are
    //    marked and re-pushed. No C-stack recursion, so a deep object graph
    //    cannot overflow the C stack; a closure larger than COLLECT_MAX nodes
    //    sets `overflow` and the round is abandoned below (leak, never dangle).
    //    Membership (RFC 005 P2) is O(1) via the epoch-stamped visit map; the
    //    pass epoch is advanced here, which O(1)-clears the whole map.
    WalkState ws;
    ws.visited = g_visited;
    ws.intra = g_intra;
    ws.stack = g_dfs_stack;
    ws.visit_map = g_visit_map;
    ws.visit_epoch = visit_map_advance_epoch();
    ws.visited_count = 0;
    ws.stack_count = 0;
    ws.overflow = 0;
    ws.current = NULL;
    for (int32_t i = 0; i < g_candidate_count && !ws.overflow; i++) {
        collect_from_root(g_candidates[i], &ws);
    }
    while (ws.stack_count > 0 && !ws.overflow) {
        void* node = ws.stack[--ws.stack_count];
        ws.current = node;
        rt_arc_walk_fields(node, collect_visit, &ws);
    }

    if (ws.overflow) {
        // Closure exceeds the fixed analysis set — conservative: keep all.
        // Release every candidate pin, then clear the queue and the O(1)
        // candidate hash set (generation advance).
        candidate_queue_clear();
        g_collecting = 0;
        return;
    }

    // 2. Trial deletion: an object is garbage iff its real refcount minus the
    // intra-closure incoming references is 0. No rc is modified here, so the
    // keep-alive path needs no mark restoration.
    // RFC 005 milestone ①: a candidate carries a +1 pin (candidate_push);
    // subtract it here so the verdict reflects the object's true external rc.
    int32_t all_garbage = ws.visited_count > 0;
    for (int32_t i = 0; i < ws.visited_count && all_garbage; i++) {
        ArcHeader* h = (ArcHeader*)ws.visited[i];
        int32_t rc = atomic_load_explicit(&h->refcount, memory_order_acquire);
        int32_t pin = candidate_is_queued(ws.visited[i]) ? 1 : 0;
        if (rc - pin - ws.intra[i] != 0) {
            all_garbage = 0;
        }
    }

    if (all_garbage) {
        // 3. Collect: zero every garbage refcount first so the finalizer field
        // decs hit prev==0 and are no-ops (no double-free), then fire the
        // finalizer and free. A garbage closure is closed under references
        // (every member has rc_after == 0), so no live object is ever dec'd.
        for (int32_t i = 0; i < ws.visited_count; i++) {
            ArcHeader* h = (ArcHeader*)ws.visited[i];
            atomic_store_explicit(&h->refcount, 0, memory_order_release);
        }
        for (int32_t i = 0; i < ws.visited_count; i++) {
            ArcHeader* h = (ArcHeader*)ws.visited[i];
            // RFC 005 milestone ①: drop the queue entry WITHOUT unpinning —
            // the pin is subsumed into the zeroed refcount and the memory is
            // freed below (or kept for Weak<T> observers at rc 0).
            candidate_remove_no_unpin(ws.visited[i]);
            int32_t wc = atomic_load_explicit(&h->weakcount, memory_order_acquire);
            if (wc == 0) {
                const void** vt = (const void**)h->vtable;
                if (vt && vt[1]) {
                    void (*finalize)(void*) = (void (*)(void*))vt[1];
                    finalize(ws.visited[i]);
                }
                free(ws.visited[i]);
            }
            // wc > 0: header kept for Weak<T> observers; refcount is already 0
            // so TryGet deterministically reports the target as gone, and the
            // last rt_arc_weak_destroy frees the header.
        }
    }
    // else: keep alive. Simplest policy: drop the candidates from the queue
    // (they may leak until re-registered by a future dec — never dangle).

    // Clear the queue, releasing each remaining candidate's pin (and advancing
    // the O(1) candidate hash generation). Kept candidates stay alive by their
    // real refs (rc ≥ 1); deferred rc==0 candidates simply leak — never dangle.
    candidate_queue_clear();
    g_collecting = 0;
}
