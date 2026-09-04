// Thread + 同步原语 ABI (RFC 009 M5.5).
//
// 平台抽象：Thread / Mutex / Semaphore / Monitor。
// Windows 基于 CreateThread / CRITICAL_SECTION / Semaphore / CONDITION_VARIABLE；
// POSIX 基于 pthread_create / pthread_mutex_t / sem_t / pthread_cond_t。
//
// ## 设计要点
//
// - Thread：显式 OS 线程（非 goroutine / virtual thread），Start/Join 语义对齐 C#
// - Mutex：非递归互斥锁（与 C# Mutex 不同，C# Mutex 是 Win32 递归 mutex；
//   Arc Mutex 采用 POSIX 默认非递归语义，避免误用；递归需求用 Monitor）
// - Semaphore：计数信号量，初始/最大值由构造函数指定
// - Monitor：基于 mutex + condvar，Enter/Exit/Wait/Pulse/PulseAll；
//   Lock 类实例作为 Monitor 的目标对象（专用 Lock 类，非任意 object）
//
// ## lock 语句糖
//
// `lock (myLock) { ... }` 脱糖为 Monitor.Enter/Exit + try/finally，
// 由 codegen 实现（本文件仅提供 Monitor ABI）。

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#ifdef _WIN32
  #include <windows.h>
  #include <process.h> /* _beginthreadex */
#else
  #include <pthread.h>
  #include <semaphore.h>
  #include <time.h>
  #include <unistd.h>
#endif

/* ---- 丢失唤醒取证：Monitor 阻塞诊断侧表（临时：随诊断计数器整体回收）----
 * 等待侧：Enter 即将阻塞前记录 {tid, obj, since}（离开阻塞清除）。
 * 持有侧：Enter 成功后登记 obj→owner tid，Leave 前清除。
 * 转储（rt_threadpool.c rt_diag_maybe_dump）输出全部等待者与持锁者，
 * 直接呈现「谁等哪把锁、锁被谁持有」的死锁图。 */
typedef struct rt_mon_diag_slot {
    volatile long      tid;      /* 0=空闲 */
    void*              obj;
    volatile long long since;
} rt_mon_diag_slot;

#define RT_MON_DIAG_SLOTS 128
static rt_mon_diag_slot g_mon_waiters[RT_MON_DIAG_SLOTS];

typedef struct rt_mon_owner {
    void*              obj;
    volatile long      tid;
    volatile long long since;   /* 取证：持有起始时刻（held_ms 追踪） */
} rt_mon_owner;

#define RT_MON_OWNER_SLOTS 256
static rt_mon_owner g_mon_owners[RT_MON_OWNER_SLOTS];
static long g_mon_diag_seq = 0;

static int rt_mon_diag_waiter_begin(void* obj) {
    long tid = (long)GetCurrentThreadId();
    int slot = (int)((unsigned long)tid % RT_MON_DIAG_SLOTS);
    for (int probe = 0; probe < RT_MON_DIAG_SLOTS; probe++) {
        rt_mon_diag_slot* s = &g_mon_waiters[(slot + probe) % RT_MON_DIAG_SLOTS];
        long idle = 0;
        if (_InterlockedCompareExchange(&s->tid, tid, 0) == 0) {
            s->obj = obj;
            s->since = (long long)GetTickCount64();
            return 1;
        }
        (void)idle;
    }
    return 0;
}

static void rt_mon_diag_waiter_end(void) {
    long tid = (long)GetCurrentThreadId();
    int slot = (int)((unsigned long)tid % RT_MON_DIAG_SLOTS);
    for (int probe = 0; probe < RT_MON_DIAG_SLOTS; probe++) {
        rt_mon_diag_slot* s = &g_mon_waiters[(slot + probe) % RT_MON_DIAG_SLOTS];
        if (s->tid == tid) {
            s->tid = 0;
            s->obj = NULL;
            s->since = 0;
            return;
        }
    }
}

static void rt_mon_diag_owner_set(void* obj) {
    long tid = (long)GetCurrentThreadId();
    long seq = _InterlockedIncrement(&g_mon_diag_seq);
    rt_mon_owner* o = &g_mon_owners[seq % RT_MON_OWNER_SLOTS];
    o->obj = obj;
    o->tid = tid;
    o->since = (long long)GetTickCount64();
}

/* 取证查询：当前线程持有的 Monitor obj（无则 NULL）。 */
void* rt_mon_diag_current_owner_obj_of(long tid);

void* rt_mon_diag_current_owner_obj(void) {
    return rt_mon_diag_current_owner_obj_of((long)GetCurrentThreadId());
}

/* 取证查询：指定线程持有的 Monitor obj（无则 NULL）。 */
void* rt_mon_diag_current_owner_obj_of(long tid) {
    for (int i = 0; i < RT_MON_OWNER_SLOTS; i++) {
        if (g_mon_owners[i].tid == tid && g_mon_owners[i].obj) {
            return g_mon_owners[i].obj;
        }
    }
    return NULL;
}

static void rt_mon_diag_owner_clear(void* obj) {
    long tid = (long)GetCurrentThreadId();
    for (int i = 0; i < RT_MON_OWNER_SLOTS; i++) {
        rt_mon_owner* o = &g_mon_owners[i];
        if (o->obj == obj && o->tid == tid) {
            o->tid = 0;
            o->obj = NULL;
            return;
        }
    }
}

/* 诊断转储：全部 Monitor 等待者（tid/obj/等待时长）与持锁者（obj→tid）。
 * 持有者附跨线程取栈（rt_threadpool.c 定义）——临界区内阻塞点直接可见。 */
void rt_diag_thread_stack(unsigned long tid, const char* tag);

void rt_mon_diag_dump(void) {
    long long now = (long long)GetTickCount64();
    for (int i = 0; i < RT_MON_DIAG_SLOTS; i++) {
        rt_mon_diag_slot* s = &g_mon_waiters[i];
        long tid = s->tid;
        if (tid) {
            fprintf(stderr, "[mon-wait] tid=%ld obj=%p waited_ms=%lld\n",
                    tid, s->obj, now - s->since);
        }
    }
    for (int i = 0; i < RT_MON_OWNER_SLOTS; i++) {
        rt_mon_owner* o = &g_mon_owners[i];
        if (o->tid) {
            fprintf(stderr, "[mon-held] obj=%p tid=%ld held_ms=%lld\n", o->obj,
                    o->tid, now - o->since);
            rt_diag_thread_stack((unsigned long)o->tid, "mon-held");
        }
    }
}

#ifdef _WIN32
  #include <process.h> /* _beginthreadex（重复包含无害） */
#else
  #include <time.h>
#endif

/* ---- Thread ABI ---- */

typedef void (*rt_thread_fn)(void* data);

typedef struct rt_thread_handle {
#ifdef _WIN32
    HANDLE handle;
#else
    pthread_t handle;
    int     valid;
#endif
} rt_thread_handle;

#ifdef _WIN32
typedef struct {
    rt_thread_fn fn;
    void*        data;
} rt_thread_arg;

static unsigned __stdcall rt_thread_trampoline(void* arg) {
    rt_thread_arg* a = (rt_thread_arg*)arg;
    a->fn(a->data);
    /* H1: 勿 free(a)——与 thread handle / Task.Run trampoline 同策，漏至退出。 */
    return 0;
}
#else
typedef struct {
    rt_thread_fn fn;
    void*        data;
} rt_thread_arg;

static void* rt_thread_trampoline(void* arg) {
    rt_thread_arg* a = (rt_thread_arg*)arg;
    a->fn(a->data);
    /* H1: 勿 free(a)——与 Windows 路径同策。 */
    return NULL;
}
#endif

void* rt_thread_create(rt_thread_fn fn, void* data) {
    rt_thread_arg* arg = (rt_thread_arg*)malloc(sizeof(rt_thread_arg));
    if (!arg) return NULL;
    arg->fn = fn;
    arg->data = data;

    rt_thread_handle* h = RT_OPAQUE_NEW(rt_thread_handle);
    if (!h) { free(arg); return NULL; }

#ifdef _WIN32
    uintptr_t th = _beginthreadex(NULL, 0, rt_thread_trampoline, arg, 0, NULL);
    if (!th) { free(arg); rt_obj_free(h); return NULL; }
    h->handle = (HANDLE)th;
#else
    if (pthread_create(&h->handle, NULL, rt_thread_trampoline, arg) != 0) {
        free(arg); free(h); return NULL;
    }
    h->valid = 1;
#endif
    return h;
}

void rt_thread_join(void* thread) {
    rt_thread_handle* h = (rt_thread_handle*)thread;
    if (!h) return;
#ifdef _WIN32
    WaitForSingleObject(h->handle, INFINITE);
    CloseHandle(h->handle);
    h->handle = NULL;
#else
    if (h->valid) pthread_join(h->handle, NULL);
    h->valid = 0;
#endif
    /* H1: 勿 free(h)——ShutdownDefaultPool→join_live 在 WriteResults 前 Join，
     * 报告期 free 句柄与 CRT 分配交织可损堆；漏至进程退出。 */
}

void rt_thread_detach(void* thread) {
    rt_thread_handle* h = (rt_thread_handle*)thread;
    if (!h) return;
#ifdef _WIN32
    if (h->handle) {
        CloseHandle(h->handle);
        h->handle = NULL;
    }
#else
    if (h->valid) pthread_detach(h->handle);
    h->valid = 0;
#endif
    /* H1: 勿 free(h)——与 join 同策。 */
}

void rt_thread_sleep(uint64_t milliseconds) {
#ifdef _WIN32
    Sleep((DWORD)milliseconds);
#else
    struct timespec ts;
    ts.tv_sec = (time_t)(milliseconds / 1000);
    ts.tv_nsec = (long)((milliseconds % 1000) * 1000000L);
    nanosleep(&ts, NULL);
#endif
}

void* rt_thread_current(void) {
    /* 返回当前线程句柄（不拥有所有权，join/detach 不可用于 current） */
#ifdef _WIN32
    /* GetCurrentThread 返回伪句柄，不能 CloseHandle；包装为不持有模式 */
    static __declspec(thread) rt_thread_handle self;
    self.handle = GetCurrentThread();
    return &self;
#else
    static _Thread_local rt_thread_handle self;
    self.handle = pthread_self();
    self.valid = 1;
    return &self;
#endif
}

int64_t rt_thread_current_id(void) {
#ifdef _WIN32
    return (int64_t)GetCurrentThreadId();
#else
    return (int64_t)pthread_self();
#endif
}

/* ---- Mutex ABI ----
 * 非递归互斥（对齐 facade / RFC 009）：Windows 用 SRWLOCK（独占、不可重入），
 * 禁止 CRITICAL_SECTION（同线程可重入，违背 Mutex 契约）。
 */

void* rt_mutex_create(void) {
#ifdef _WIN32
    SRWLOCK* m = (SRWLOCK*)malloc(sizeof(SRWLOCK));
    if (!m) return NULL;
    InitializeSRWLock(m);
    return m;
#else
    pthread_mutex_t* m = (pthread_mutex_t*)malloc(sizeof(pthread_mutex_t));
    if (!m) return NULL;
    pthread_mutex_init(m, NULL);
    return m;
#endif
}

void rt_mutex_lock(void* mutex) {
    if (!mutex) return;
#ifdef _WIN32
    AcquireSRWLockExclusive((SRWLOCK*)mutex);
#else
    pthread_mutex_lock((pthread_mutex_t*)mutex);
#endif
}

int32_t rt_mutex_try_lock(void* mutex) {
    if (!mutex) return 0;
#ifdef _WIN32
    return TryAcquireSRWLockExclusive((SRWLOCK*)mutex) ? 1 : 0;
#else
    return pthread_mutex_trylock((pthread_mutex_t*)mutex) == 0 ? 1 : 0;
#endif
}

void rt_mutex_unlock(void* mutex) {
    if (!mutex) return;
#ifdef _WIN32
    ReleaseSRWLockExclusive((SRWLOCK*)mutex);
#else
    pthread_mutex_unlock((pthread_mutex_t*)mutex);
#endif
}

void rt_mutex_destroy(void* mutex) {
    if (!mutex) return;
#ifdef _WIN32
    /* SRWLOCK 无需 Delete；直接释放堆块 */
#else
    pthread_mutex_destroy((pthread_mutex_t*)mutex);
#endif
    rt_obj_free(mutex);
}

/* ---- Semaphore ABI ----
 *
 * Semaphore 句柄包装为 rt_semaphore_obj——Arc 对象布局兼容（refcount 在 offset 0），
 * 避免 rt_arc_dec 将 Windows HANDLE 视为指针解引用导致访问违规。
 */
typedef struct {
    _Atomic int32_t refcount;  /* Arc header offset 0 */
    const void* vtable;        /* Arc header offset 8 */
    void* handle;              /* Windows: HANDLE; POSIX: sem_t* */
} rt_semaphore_obj;

void* rt_semaphore_create(int32_t initial, int32_t maximum) {
    /* RFC 050 M-b：refcount@0 是历史假头死字段（豁免清单后仅 create 写 1）；
     * opaque 头接管身份后 magic/kind 双拦误计数，refcount 字段保留仅作布局兼容。 */
    rt_semaphore_obj* obj = RT_OPAQUE_NEW(rt_semaphore_obj);
    if (!obj) return NULL;
    obj->refcount = 1;
#ifdef _WIN32
    HANDLE s = CreateSemaphoreA(NULL, (LONG)initial, (LONG)maximum, NULL);
    obj->handle = (void*)s;
#else
    sem_t* s = (sem_t*)malloc(sizeof(sem_t));
    if (!s) { rt_obj_free(obj); return NULL; }
    if (sem_init(s, 0, (unsigned)initial) != 0) {
        free(s);
        rt_obj_free(obj);
        return NULL;
    }
    (void)maximum;  /* POSIX sem 不支持最大值约束 */
    obj->handle = (void*)s;
#endif
    return obj;
}

void rt_semaphore_wait(void* sem) {
    if (!sem) return;
    rt_semaphore_obj* obj = (rt_semaphore_obj*)sem;
#ifdef _WIN32
    WaitForSingleObject((HANDLE)obj->handle, INFINITE);
#else
    while (sem_wait((sem_t*)obj->handle) == -1 && errno == EINTR) { /* retry on EINTR */ }
#endif
}

int32_t rt_semaphore_wait_timeout(void* sem, uint64_t ms) {
    if (!sem) return 0;
    rt_semaphore_obj* obj = (rt_semaphore_obj*)sem;
#ifdef _WIN32
    DWORD r = WaitForSingleObject((HANDLE)obj->handle, (DWORD)ms);
    return r == WAIT_OBJECT_0 ? 1 : 0;
#else
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    ts.tv_sec += (time_t)(ms / 1000);
    ts.tv_nsec += (long)((ms % 1000) * 1000000L);
    if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; }
    int r = sem_timedwait((sem_t*)obj->handle, &ts);
    return r == 0 ? 1 : 0;
#endif
}

void rt_semaphore_release(void* sem) {
    if (!sem) return;
    rt_semaphore_obj* obj = (rt_semaphore_obj*)sem;
#ifdef _WIN32
    ReleaseSemaphore((HANDLE)obj->handle, 1, NULL);
#else
    sem_post((sem_t*)obj->handle);
#endif
}

/* std P3：批量释放（Semaphore.Release(int)）。Windows ReleaseSemaphore 原生
 * 支持 count；POSIX sem_post 每次 +1，需循环。count <= 0 为 no-op。 */
void rt_semaphore_release_n(void* sem, int32_t count) {
    if (!sem || count <= 0) return;
    rt_semaphore_obj* obj = (rt_semaphore_obj*)sem;
#ifdef _WIN32
    ReleaseSemaphore((HANDLE)obj->handle, (LONG)count, NULL);
#else
    for (int32_t i = 0; i < count; i++) {
        sem_post((sem_t*)obj->handle);
    }
#endif
}

void rt_semaphore_destroy(void* sem) {
    if (!sem) return;
    rt_semaphore_obj* obj = (rt_semaphore_obj*)sem;
#ifdef _WIN32
    CloseHandle((HANDLE)obj->handle);
#else
    sem_destroy((sem_t*)obj->handle);
    free(obj->handle);
#endif
    rt_obj_free(obj);
}

/* ---- Monitor ABI ----
 *
 * Monitor 基于 mutex + condvar。Lock 类实例在 runtime 侧映射为 rt_monitor_obj：
 *   typedef struct { mutex; cond; } rt_monitor_obj;
 * Monitor.Enter/Exit 操作 mutex；Wait/Pulse 操作 condvar。
 *
 * 与 C# Monitor 不同（C# Monitor 作用于任意 object），Arc Monitor 仅作用于
 * Lock 类实例（RFC 009 §7.2 D6 双轨设计：专用 Lock 类避免为所有 class 实例
 * 追加 sync-block 头开销）。
 */

typedef struct rt_monitor_obj {
#ifdef _WIN32
    CRITICAL_SECTION    mutex;
    CONDITION_VARIABLE  cond;
#else
    pthread_mutex_t     mutex;
    pthread_cond_t      cond;
#endif
    int32_t             initialized;
} rt_monitor_obj;

void  rt_monitor_enter(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m) return;
    if (!m->initialized) {
        /* Lock() 构造时已初始化，此处为防御性检查 */
#ifdef _WIN32
        InitializeCriticalSection(&m->mutex);
        InitializeConditionVariable(&m->cond);
#else
        pthread_mutex_init(&m->mutex, NULL);
        pthread_cond_init(&m->cond, NULL);
#endif
        m->initialized = 1;
    }
#ifdef _WIN32
    if (!TryEnterCriticalSection(&m->mutex)) {
        /* 即将阻塞：登记等待者（转储呈现死锁图） */
        rt_mon_diag_waiter_begin(obj);
        EnterCriticalSection(&m->mutex);
        rt_mon_diag_waiter_end();
    }
    rt_mon_diag_owner_set(obj);
#else
    pthread_mutex_lock(&m->mutex);
#endif
}

void  rt_monitor_exit(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return;
#ifdef _WIN32
    rt_mon_diag_owner_clear(obj);
    LeaveCriticalSection(&m->mutex);
#else
    pthread_mutex_unlock(&m->mutex);
#endif
}

int32_t rt_monitor_try_enter(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return 0;
#ifdef _WIN32
    return TryEnterCriticalSection(&m->mutex) ? 1 : 0;
#else
    return pthread_mutex_trylock(&m->mutex) == 0 ? 1 : 0;
#endif
}

void  rt_monitor_wait(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return;
#ifdef _WIN32
    SleepConditionVariableCS(&m->cond, &m->mutex, INFINITE);
#else
    pthread_cond_wait(&m->cond, &m->mutex);
#endif
}

void  rt_monitor_pulse(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return;
#ifdef _WIN32
    WakeConditionVariable(&m->cond);
#else
    pthread_cond_signal(&m->cond);
#endif
}

void  rt_monitor_pulse_all(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return;
#ifdef _WIN32
    WakeAllConditionVariable(&m->cond);
#else
    pthread_cond_broadcast(&m->cond);
#endif
}

/* Lock 类构造：runtime 侧初始化 rt_monitor_obj。
 * Arc 的 `new Lock()` 经 codegen 拦截为 rt_lock_create()。
 * RFC 050 M-a：opaque 统一头试点——对象自描述身份，ARC 误计数物理无害。 */
void* rt_lock_create(void) {
    rt_monitor_obj* m = (rt_monitor_obj*)rt_obj_alloc_opaque(sizeof(rt_monitor_obj));
    if (!m) return NULL;
#ifdef _WIN32
    InitializeCriticalSection(&m->mutex);
    InitializeConditionVariable(&m->cond);
#else
    pthread_mutex_init(&m->mutex, NULL);
    pthread_cond_init(&m->cond, NULL);
#endif
    m->initialized = 1;
    return m;
}

void rt_lock_destroy(void* obj) {
    rt_monitor_obj* m = (rt_monitor_obj*)obj;
    if (!m || !m->initialized) return;
#ifdef _WIN32
    DeleteCriticalSection(&m->mutex);
#else
    pthread_mutex_destroy(&m->mutex);
    pthread_cond_destroy(&m->cond);
#endif
    m->initialized = 0;
    free(m);
}

/* ---- Thread handle（Arc Thread 类 facade 支持） ---- */

typedef struct rt_thread_handle_full {
    rt_thread_fn      fn;
    void*             data;
    rt_thread_handle* os_handle;  /* rt_thread_create 返回的 OS 句柄 */
    int32_t           started;    /* 1 = 已 Start */
    int32_t           joined;     /* 1 = 已 Join */
} rt_thread_handle_full;

/* H1 live 表：Start 登记、Join 移除；Drop/destroy 未 Join 时留表，
 * rt_thread_join_live（报告前）统一 Join + free。禁止 destroy 路径 detach。 */
#define RT_THREAD_LIVE_CAP 512
static rt_thread_handle_full* g_thread_live[RT_THREAD_LIVE_CAP];
static int g_thread_live_n = 0;
#ifdef _WIN32
static CRITICAL_SECTION g_thread_live_cs;
static LONG g_thread_live_cs_once = 0;
static void rt_thread_live_lock(void) {
    if (InterlockedCompareExchange(&g_thread_live_cs_once, 1, 0) == 0) {
        InitializeCriticalSection(&g_thread_live_cs);
        InterlockedExchange(&g_thread_live_cs_once, 2);
    } else {
        while (InterlockedCompareExchange(&g_thread_live_cs_once, 2, 2) != 2) {
            Sleep(0);
        }
    }
    EnterCriticalSection(&g_thread_live_cs);
}
static void rt_thread_live_unlock(void) { LeaveCriticalSection(&g_thread_live_cs); }
#else
static pthread_mutex_t g_thread_live_mu = PTHREAD_MUTEX_INITIALIZER;
static void rt_thread_live_lock(void) { pthread_mutex_lock(&g_thread_live_mu); }
static void rt_thread_live_unlock(void) { pthread_mutex_unlock(&g_thread_live_mu); }
#endif

static void rt_thread_live_add(rt_thread_handle_full* h) {
    if (!h) return;
    rt_thread_live_lock();
    for (int i = 0; i < g_thread_live_n; i++) {
        if (g_thread_live[i] == h) {
            rt_thread_live_unlock();
            return;
        }
    }
    if (g_thread_live_n < RT_THREAD_LIVE_CAP) {
        g_thread_live[g_thread_live_n++] = h;
    }
    rt_thread_live_unlock();
}

static void rt_thread_live_remove(rt_thread_handle_full* h) {
    if (!h) return;
    rt_thread_live_lock();
    for (int i = 0; i < g_thread_live_n; i++) {
        if (g_thread_live[i] == h) {
            g_thread_live[i] = g_thread_live[--g_thread_live_n];
            break;
        }
    }
    rt_thread_live_unlock();
}

void* rt_thread_handle_create(rt_thread_fn fn, void* data) {
    rt_thread_handle_full* h = RT_OPAQUE_NEW(rt_thread_handle_full);
    if (!h) return NULL;
    /* RT_OPAQUE_NEW 不清零——started/joined 的零初值是 Start/Join 状态机前提 */
    memset(h, 0, sizeof(*h));
    h->fn = fn;
    h->data = data;
    return h;
}

void rt_thread_handle_start(void* th) {
    rt_thread_handle_full* h = (rt_thread_handle_full*)th;
    if (!h || h->started) return;
    h->started = 1;
    h->os_handle = (rt_thread_handle*)rt_thread_create(h->fn, h->data);
    rt_thread_live_add(h);
}

void rt_thread_handle_join(void* th) {
    rt_thread_handle_full* h = (rt_thread_handle_full*)th;
    if (!h || !h->started) return;
    /* 锁下领取 os_handle，避免与 rt_thread_join_live 双重 CloseHandle。 */
    rt_thread_handle* os = NULL;
    rt_thread_live_lock();
    if (h->joined) {
        rt_thread_live_unlock();
        return;
    }
    h->joined = 1;
    for (int i = 0; i < g_thread_live_n; i++) {
        if (g_thread_live[i] == h) {
            g_thread_live[i] = g_thread_live[--g_thread_live_n];
            break;
        }
    }
    os = h->os_handle;
    h->os_handle = NULL;
    rt_thread_live_unlock();
    if (os) rt_thread_join(os);
}

int32_t rt_thread_handle_is_alive(void* th) {
    rt_thread_handle_full* h = (rt_thread_handle_full*)th;
    if (!h) return 0;
    return (h->started && !h->joined) ? 1 : 0;
}

void rt_thread_handle_destroy(void* th) {
    rt_thread_handle_full* h = (rt_thread_handle_full*)th;
    if (!h) return;
    /* H1: 永不 free(h)。未 Join 留 live 表由 join_live 收 OS 句柄；
     * 已 Join 亦漏 full 句柄——报告期 free 与 CRT 分配交织可损堆。禁止 detach。 */
    (void)h;
}

void rt_thread_join_live(void) {
    rt_thread_handle* os_batch[RT_THREAD_LIVE_CAP];
    int n = 0;
    rt_thread_live_lock();
    for (int i = 0; i < g_thread_live_n; i++) {
        rt_thread_handle_full* h = g_thread_live[i];
        if (!h || !h->started || h->joined) continue;
        h->joined = 1;
        if (h->os_handle) {
            os_batch[n++] = h->os_handle;
            h->os_handle = NULL;
        }
        /* H1: 勿 free(h)——opaque 别名可能仍持有；漏至进程退出。 */
    }
    g_thread_live_n = 0;
    rt_thread_live_unlock();

    for (int i = 0; i < n; i++) {
        if (os_batch[i]) rt_thread_join(os_batch[i]);
    }
}
