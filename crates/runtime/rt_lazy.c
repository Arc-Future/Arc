//! RFC 006 A3 S2：`static readonly` 惰性初始化 guard（类级状态机）。
//!
//! 为 `static readonly` + 非编译期常量初始化器的字段提供线程安全惰性初始化：
//! 首次访问才构造一次，并发访问无重复初始化、无部分可见。
//!
//! 状态机（state 为类级全局 i32，由 codegen 发射）：
//!   0 = 未初始化，1 = 初始化中（某线程持有），2 = 已初始化
//!
//! 约定 codegen 生成序列：
//!   if (!rt_lazy_is_initialized(&state)) {
//!       if (rt_lazy_init_begin(&state)) {
//!           <store 初始化器到 @__static_<Class>_<field>>
//!           rt_lazy_init_commit(&state);
//!       }
//!   }
//!   <load @__static_<Class>_<field>>
//!
//! - 快速路径 `rt_lazy_is_initialized`：单原子 acquire 读（对齐 C# beforefieldinit）。
//! - 慢路径：`rt_lazy_init_begin` 经 0→1 CAS 赢得初始化权；若他线程持有则
//!   自旋让步等待至已初始化后返回 0。`rt_lazy_init_commit` 以 release 发布
//!   state=2，使后续 acquire 读的线程可见全部初始化 store（无部分可见）。
//! 零动态分配、零急切设置——无需运行时互斥量创建（免 main 前初始化顺序问题）。
//!
//! # 内存序契约（RFC 006 A3 S4）
//! - 读快速路径：`load atomic ... acquire`——单原子读，与 `commit` 的 release store
//!   构成 acquire/release 同步，把初始化器 store 顺序化到该读之前的后续 load 前
//!   （x86 TSO 下 acquire/release 免费，快速路径即单内存读）。
//! - 慢路径赢得权：`atomic_compare_exchange 0→1`，成功序 acq_rel，失败序 relaxed。
//! - 他线程等待：acquire 自旋读 state 直到 ==2（与 commit 的 release 同步）。
//! - 初始化器 store（codegen 发射的普通 store）位于 `begin` 与 `commit` 之间，
//!   由上述 acquire/release 保证发布；读线程在 acquire 读 state==2 后再普通 load 字段。
//!
//! # 已知边界（std::call_once 对齐）
//! - **同类递归访问死锁**：某惰性字段的初始化器若读取**同类的另一惰性字段**
//!   （state 保持 1），内层 `rt_lazy_init_begin` 会永久自旋。跨类读取不受影响
//!   （各自独立 state）。与 C++ `std::call_once` 一致，属显式限制。
//! - **初始化抛异常后 state 滞留 1**：若初始化器抛异常，state 不会回退，后续访问
//!   会永久自旋（需 codegen 在 unwinding 时回退 state=0 以允许重试，暂未实现）。
//!   为显式已知限制，未来 S 里程碑处理。

#include <stdatomic.h>
#include <stdint.h>
#include "rt_abi.h"
#ifdef _WIN32
  #include <windows.h>          /* SwitchToThread */
  #if defined(_MSC_VER) && (defined(_M_X64) || defined(_M_IX86))
    #include <emmintrin.h>      /* _mm_pause（MSVC / clang-cl） */
  #endif
#else
  #include <sched.h>            /* sched_yield */
#endif

/* PAUSE：x86 自旋时降低功耗/总线竞争；非 x86 退化为空转（镜像 rt_task.c） */
static void rt_lazy_pause(void) {
#if defined(_MSC_VER) && defined(_M_X64)
    _mm_pause();
#elif defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#else
    /* 非 x86：空转即自旋 */
#endif
}

/* yield：让出 CPU 给其他可运行线程 */
static void rt_lazy_yield(void) {
#ifdef _WIN32
    SwitchToThread();
#else
    sched_yield();
#endif
}

/* 快速路径：单原子 acquire 读，返回是否已初始化（==2） */
int32_t rt_lazy_is_initialized(int32_t* state) {
    return atomic_load_explicit((atomic_int*)state, memory_order_acquire) == 2 ? 1 : 0;
}

/* 慢路径：赢得初始化权返回 1；否则让步等待至已初始化返回 0。
 * （state 保持 1 仅当初始化器执行中；一次性、通常极短。） */
int32_t rt_lazy_init_begin(int32_t* state) {
    int32_t expected = 0;
    if (atomic_compare_exchange_strong_explicit(
            (atomic_int*)state, &expected, 1,
            memory_order_acq_rel, memory_order_relaxed)) {
        return 1; /* 本线程赢得初始化权，须在 commit 前执行初始化器 */
    }
    /* 其它线程正在初始化（1）或已初始化（2）：等待至已初始化 */
    while (atomic_load_explicit((atomic_int*)state, memory_order_acquire) != 2) {
        rt_lazy_pause();
        rt_lazy_yield();
    }
    return 0;
}

/* 发布：release store state=2，使后续 acquire 读可见全部初始化 store */
void rt_lazy_init_commit(int32_t* state) {
    atomic_store_explicit((atomic_int*)state, 2, memory_order_release);
}
