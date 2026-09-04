// Dynamic Library Loading ABI (RFC 017 D8 v1.0 + RFC 017 热卸载闭环).
//
// 动态库加载 ABI 的跨平台实现。设计原则：Arc 动态库对齐 C# 程序集（Assembly）
// 模型，动态库 = 干净的库逻辑 + 引用链接信息，不引入 Rust 风格的复杂插件机制。
//
// ## ABI 列表
//
// | ABI                  | Linux/macOS/OHos      | Windows                |
// |----------------------|-----------------------|------------------------|
// | rt_library_load(p)   | dlopen(p, RTLD_NOW)   | LoadLibraryW(p)        |
// | rt_library_sym(h, n) | dlsym(h, n)           | GetProcAddress(h, n)   |
// | rt_library_unload(h) | dlclose(h)            | FreeLibrary(h)         |
// | rt_library_unload_hot(h) | dlclose(h)         | FreeLibrary(h)         |
//
// ## 错误语义
//
// - load/sym 失败返回 NULL；具体错误由平台 errno/GetLastError 维护
// - unload 是 NULL 安全的（NULL 句柄直接返回）
//
// ## 生命周期（RFC 017 热卸载闭环，逆向 RFC 017 D8 决策 13）
//
// 热卸载闭环（RFC 017 §2）：可回收 ALC + ARC 根扫描 + 模块级代数引用计数。
// 每个模块实例持有单调递增 `generation`（per-handle 代数）；同路径重复加载获得
// 新代数、旧代数失效（tombstone）。
//
// - **模块登记表**：`g_lib_modules[]` 维护模块状态机
//   `INVALID → ACTIVE → FREEZING → TOMBSTONED`；`dlclose` 仅在
//   「ledger 归零 + 根扫描通过 + 在途调用收敛」后执行。
// - **代数引用计数**（RFC 017 §2.2 方案 B）：跨模块外部强引用经
//   `rt_library_ref_register/unregister(gen)` 原子 ledger 登记；卸载强制归零检测，
//   非零 → 拒绝卸载（`RT_LIBRARY_UNLOAD_HANGING`）。
// - **根扫描**（RFC 017 §2.3）：`rt_library_root_add/remove/scan(gen)` 枚举已登记
//   模块根并沿 strong 字段遍历可达闭包（复用 `rt_arc_walk_fields`）；无全堆扫描。
// - **E_UNLOAD_HANGING_REF 硬错误**（RFC 017 §2.4）：卸载后访问已卸载模块符号
//   （`rt_library_sym` / `rt_library_get_meta`）→ `rt_panic` 硬错误，禁静默。
//
// ## 链接要求
//
// - Linux：需链接 -ldl（dlopen/dlsym/dlclose 在 libdl）
// - macOS：无需额外链接（dlopen 在 libc 内置）
// - Windows：无需额外链接（LoadLibraryW 在 kernel32，已隐式链接）
// - OHos：无需额外链接（dlopen 在 libc 内置）

#include "rt_abi.h"

#include <stdatomic.h>
#include <stdio.h>
#include <string.h>

/* ---- 平台头文件 ---- */

#if defined(_WIN32) || defined(_WIN64)
    #define RT_LIBRARY_WINDOWS 1
    #include <windows.h>
#else
    #include <dlfcn.h>
    #include <sched.h>
#endif

/* ---- Assembly 执行上下文（RFC 017 M1）---- */

static void* g_assembly_executing = NULL;

void rt_assembly_set_executing(void* assembly_ptr) {
    g_assembly_executing = assembly_ptr;
}

void* rt_assembly_get_executing(void) {
    return g_assembly_executing;
}

/* ---- 模块登记表 + 代数引用计数 + 根扫描（RFC 017 热卸载闭环）---- */

#define RT_LIB_REGISTRY_CAPACITY 256   /* 模块登记表容量（代数 = slot+1） */
#define RT_LIB_PATH_MAX          512
#define RT_LIB_MAX_ROOTS         64
#define RT_LIB_MAX_WEAK          64     /* 模块边界弱登记表容量（RFC 017 §2.6） */
#define RT_LIB_SCAN_MAX          1024   /* 根扫描可达闭包上限（防爆炸） */
#define RT_LIB_INFLIGHT_SPIN_MAX 100000 /* 在途调用等待上界（自旋让步） */

/* 卸载返回值（rt_library_unload_hot） */
#define RT_LIBRARY_UNLOAD_OK       1   /* 卸载成功 */
#define RT_LIBRARY_UNLOAD_HANGING  0   /* 存在外部强引用，拒绝卸载（报告） */
#define RT_LIBRARY_UNLOAD_INFLIGHT (-1) /* 在途调用未收敛，拒绝卸载 */
#define RT_LIBRARY_UNLOAD_INVALID  (-2) /* 句柄无效 / 已被并发卸载 */

typedef enum {
    RT_LIB_STATE_INVALID = 0,  /* 空槽 */
    RT_LIB_STATE_ACTIVE,       /* 可加载/可调用/可登记 */
    RT_LIB_STATE_FREEZING,     /* 卸载进行中：拒绝新登记/新调用，等待在途收敛 */
    RT_LIB_STATE_TOMBSTONED    /* 已卸载：句柄访问 → E_UNLOAD_HANGING_REF */
} RtLibState;

typedef struct {
    _Atomic(int32_t) state;       /* RtLibState */
    _Atomic(void*) handle;        /* 关联 OS 句柄；地址复用时由 load 置空旧条目 */
    _Atomic(int32_t) in_flight;   /* 模块代码在途调用计数 */
    _Atomic(int32_t) ledger;      /* 跨模块外部强引用计数 */
    void* roots[RT_LIB_MAX_ROOTS]; /* 模块根（模块静态持有的 class 引用） */
    int32_t root_count;
    /* RFC 017 §2.3: codegen 自动发射的模块根元数据表（`__arc_module_roots`——
     * 静态字段 class 引用槽位地址数组；`rt_library_load` 自动发现）。根扫描
     * 懒读取槽位当前对象并遍历；卸载前统一释放。 */
    void* const* codegen_roots;
    int32_t codegen_root_count;
    void* weak_slots[RT_LIB_MAX_WEAK]; /* RFC 017 §2.6: 边界弱槽位登记表 */
    int32_t weak_count;
    /* RFC 017 阶段一任务⑥：__arc_module_init 触发状态（两阶段）——
     * 0 = 未启动；1 = 本槽负责 init（执行中）；2 = init 已完成。
     * 同 OS 句柄（LoadLibrary 引用计数）的多代数槽共享模块内存，静态初始化器
     * `__sinit_<Class>` 无重入防护，仅最小代数槽执行（见 rt_library_load）。 */
    _Atomic(int32_t) init_done;
    char path[RT_LIB_PATH_MAX];
} RtLibModule;

typedef struct {
    void* visited[RT_LIB_SCAN_MAX];
    int32_t count;
} RtScanCtx;

static RtLibModule g_lib_modules[RT_LIB_REGISTRY_CAPACITY];
static atomic_flag g_lib_registry_lock = ATOMIC_FLAG_INIT;

static void rt_lib_yield(void) {
#if defined(RT_LIBRARY_WINDOWS)
    SwitchToThread();
#else
    sched_yield();
#endif
}

static void rt_lib_lock(void) {
    while (atomic_flag_test_and_set_explicit(
               &g_lib_registry_lock, memory_order_acquire)) {
        rt_lib_yield();
    }
}

static void rt_lib_unlock(void) {
    atomic_flag_clear_explicit(&g_lib_registry_lock, memory_order_release);
}

static RtLibModule* rt_lib_module(int32_t generation) {
    if (generation <= 0 || generation > RT_LIB_REGISTRY_CAPACITY) return NULL;
    return &g_lib_modules[generation - 1];
}

/* 按句柄定位模块条目（匹配任意非空槽；调用方自行判定状态）。 */
static RtLibModule* rt_lib_find_by_handle(void* handle) {
    for (int32_t i = 0; i < RT_LIB_REGISTRY_CAPACITY; i++) {
        RtLibModule* m = &g_lib_modules[i];
        if (atomic_load(&m->handle) == handle &&
            atomic_load(&m->state) != RT_LIB_STATE_INVALID) {
            return m;
        }
    }
    return NULL;
}

/* 原始符号解析（无 tombstone 检查）——供 load 路径发现 codegen 模块根表。 */
static void* rt_lib_resolve_symbol(void* handle, const char* name) {
    if (!handle || !name) return NULL;
#if defined(RT_LIBRARY_WINDOWS)
    return (void*)GetProcAddress((HMODULE)handle, name);
#else
    return dlsym(handle, name);
#endif
}

/* E_UNLOAD_HANGING_REF 硬错误：错误信息含模块路径 + 代数 + 触达点。 */
static void rt_lib_report_hanging(void* handle, const char* access,
                                  RtLibModule* m) {
    char buf[576];
    int32_t gen = (int32_t)(m - g_lib_modules) + 1;
    (void)handle;
    snprintf(buf, sizeof(buf),
             "E_UNLOAD_HANGING_REF: module '%s' (generation %d) accessed via "
             "%s after unload",
             m->path[0] ? m->path : "<unknown>", gen, access);
    rt_panic(buf);
}

/* 根扫描回调：沿 strong 字段 DFS 遍历可达闭包（环防护 + 上限）。 */
static void rt_lib_scan_visit(void* ctx, void* field) {
    RtScanCtx* c = (RtScanCtx*)ctx;
    if (!field) return;
    if (c->count >= RT_LIB_SCAN_MAX) return;
    for (int32_t i = 0; i < c->count; i++) {
        if (c->visited[i] == field) return;
    }
    c->visited[c->count++] = field;
    rt_arc_walk_fields(field, rt_lib_scan_visit, c);
}

/* 释放模块根持有的 class 引用（dlclose 前统一触发）。 */
static void rt_lib_release_roots(RtLibModule* m) {
    rt_lib_lock();
    for (int32_t i = 0; i < m->root_count; i++) {
        void* root = m->roots[i];
        if (root) rt_arc_dec(root);
        m->roots[i] = NULL;
    }
    m->root_count = 0;
    /* RFC 017 §2.3：释放 codegen 模块根表槽位当前持有的 class 引用（槽位位于
     * 库数据段，随 dlclose 释放；引用释放须在 dlclose 前统一触发）。 */
    for (int32_t i = 0; i < m->codegen_root_count; i++) {
        void* slot = m->codegen_roots[i];
        if (!slot) continue;
        void* obj = *(void**)slot;
        if (obj) rt_arc_dec(obj);
    }
    m->codegen_roots = NULL;
    m->codegen_root_count = 0;
    rt_lib_unlock();
}

/* RFC 017 §2.6: 卸载路径中和登记在模块上的边界弱槽位——target 置空
 * （观察 tombstone 头语义）。此后宿主持有的 Weak<T>.TryGet() 确定性返回
 * NULL；禁悬垂复活（模块对象 vtable 已卸载后 TryGet 复活即 AV）。 */
static void rt_lib_neutralize_weak(RtLibModule* m) {
    rt_lib_lock();
    for (int32_t i = 0; i < m->weak_count; i++) {
        void* slot = m->weak_slots[i];
        if (slot) rt_arc_weak_neutralize(slot);
        m->weak_slots[i] = NULL;
    }
    m->weak_count = 0;
    rt_lib_unlock();
}

/* ---- ABI 实现 ---- */

/// 加载动态库。
///
/// path 为平台原生动态库路径（Linux .so / macOS .dylib / Windows .dll）。
/// 失败返回 NULL；具体错误由 errno/GetLastError 维护。
///
/// Linux/macOS 使用 RTLD_NOW（立即解析符号，避免运行时发现缺失符号）+
/// RTLD_LOCAL（不将库符号暴露到全局符号表，避免污染）。
///
/// RFC 017：成功加载后在模块登记表分配新代数（单调递增）。同路径重复加载
/// 获得新代数；若新句柄与某已 tombstone 条目句柄相同（OS 地址复用），
/// 先置空旧条目句柄，避免旧句柄对符号访问的代数误判。
void* rt_library_load(const char* path) {
    if (!path) return NULL;

    void* h = NULL;
#if defined(RT_LIBRARY_WINDOWS)
    /* RFC 017 M3: 使用 MultiByteToWideChar 将 UTF-8 路径转为 LoadLibraryW,
     * 消除 LoadLibraryA 对非 ASCII 路径的截断风险。ABI 签名不变 (const char*)。 */
    int wlen = MultiByteToWideChar(CP_UTF8, 0, path, -1, NULL, 0);
    if (wlen <= 0) return NULL;
    WCHAR* wpath = (WCHAR*)malloc((size_t)wlen * sizeof(WCHAR));
    if (!wpath) return NULL;
    MultiByteToWideChar(CP_UTF8, 0, path, -1, wpath, wlen);
    /* 路径分隔符规范化：Arc 侧构造的库路径可能带正斜杠 `/`（如环境变量目录 +
     * 平台库名拼接）。正斜杠本身 LoadLibrary 可加载，但会让
     * `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR` 无法确定被加载 DLL 的目录——依赖解析
     * 退化为标准顺序（app/system/windows/PATH），自带依赖的同目录 DLL
     * （onnxruntime.dll 等）会漏找，甚至误载 PATH 里错误版本。统一转反斜杠。 */
    for (int i = 0; wpath[i]; i++) {
        if (wpath[i] == L'/') wpath[i] = L'\\';
    }
    /* 依赖解析（插件/自带依赖原生库正确性）：LoadLibraryW 默认用标准搜索顺序
     * 解析被加载 DLL 的**依赖** DLL——**不含被加载 DLL 所在目录**。这会让
     * onnx_shim.dll → onnxruntime.dll（同目录）、wgpu 运行时等自带依赖的
     * 库加载失败 → `Native.IsAvailable` 假阴性。改用 LoadLibraryExW 附加
     *   LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR （搜索被加载 DLL 所在目录，新增行为）
     *   | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS （app 目录 + system32 + windows + PATH，
     *     保留既有标准路径）
     * 只增不减，不破坏既有自包含测试库的解析。
     *
     * 根因约束：`LOAD_LIBRARY_SEARCH_*` 组合标志要求 **全限定绝对路径**——传
     * 相对路径（如 `target/e2e/.../hotlib.dll`）会直接 `ERROR_INVALID_PARAMETER`(87)。
     * 故先用 `GetFullPathNameW` 将（分隔符已规范的）相对/混合路径解析为绝对路径，
     * 再加载；对已是绝对路径的输入为幂等 no-op。这同时修复热卸载 e2e 相对路径
     * 加载失败（RFC 006 A3 S5 验收非回归）。 */
    DWORD abs_len = GetFullPathNameW(wpath, 0, NULL, NULL);
    if (abs_len == 0) { free(wpath); return NULL; }
    WCHAR* wabs = (WCHAR*)malloc(((size_t)abs_len + 1) * sizeof(WCHAR));
    if (!wabs) { free(wpath); return NULL; }
    if (GetFullPathNameW(wpath, abs_len + 1, wabs, NULL) == 0) {
        free(wabs); free(wpath); return NULL;
    }
    HMODULE hmod = LoadLibraryExW(
        wabs, NULL,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    free(wabs);
    free(wpath);
    h = (void*)hmod;
#else
    /* RTLD_NOW：立即解析所有未定义符号，失败时 dlopen 返回 NULL
     * RTLD_LOCAL：不将库符号暴露到全局符号表，避免符号污染 */
    h = dlopen(path, RTLD_NOW | RTLD_LOCAL);
#endif
    if (!h) return NULL;

    rt_lib_lock();
    /* 新句柄与已 tombstone 条目句柄相同 → 置空旧条目句柄（旧代数已死） */
    for (int32_t i = 0; i < RT_LIB_REGISTRY_CAPACITY; i++) {
        RtLibModule* m = &g_lib_modules[i];
        if (atomic_load(&m->state) == RT_LIB_STATE_TOMBSTONED &&
            atomic_load(&m->handle) == h) {
            atomic_store(&m->handle, NULL);
            m->codegen_roots = NULL;
            m->codegen_root_count = 0;
        }
    }
    /* 分配空槽 */
    int32_t slot = -1;
    for (int32_t i = 0; i < RT_LIB_REGISTRY_CAPACITY; i++) {
        if (atomic_load(&g_lib_modules[i].state) == RT_LIB_STATE_INVALID) {
            slot = i;
            break;
        }
    }
    if (slot < 0) {
        rt_lib_unlock();
#if defined(RT_LIBRARY_WINDOWS)
        FreeLibrary((HMODULE)h);
#else
        dlclose(h);
#endif
        return NULL; /* 登记表耗尽（256 个并发活跃模块） */
    }
    RtLibModule* m = &g_lib_modules[slot];
    atomic_store(&m->handle, h);
    atomic_store(&m->state, RT_LIB_STATE_ACTIVE);
    atomic_store(&m->in_flight, 0);
    atomic_store(&m->ledger, 0);
    m->root_count = 0;
    m->codegen_roots = NULL;
    m->codegen_root_count = 0;
    m->weak_count = 0;
    atomic_store(&m->init_done, 0);
    snprintf(m->path, sizeof(m->path), "%s", path);

    /* RFC 017 §2.3：自动发现 codegen 模块根元数据表（静态字段 class 引用
     * 槽位地址数组）。宿主不再需要手动 RegisterModuleRoot。 */
    void* roots_tbl = rt_lib_resolve_symbol(h, "__arc_module_roots");
    void* count_sym = rt_lib_resolve_symbol(h, "__arc_module_roots_count");
    if (roots_tbl && count_sym) {
        m->codegen_roots = (void* const*)roots_tbl;
        int32_t n = *(int32_t*)count_sym;
        m->codegen_root_count = n > RT_LIB_MAX_ROOTS ? RT_LIB_MAX_ROOTS : n;
    }

    /* RFC 017 阶段一任务⑥：dbg 表登记进 rt_debug registry——插件 dll 改为
     * 导入引用 arc_runtime 后，dbg 表不再被 rt_debug.o 链接期就地解析，
     * StackTrace 符号化须由加载期登记接管。每代数槽登记一条，与两个卸载
     * 路径的注销按槽配对（条目存活期与 OS 引用计数同步）。 */
    void* dbg_tbl = rt_lib_resolve_symbol(h, "__arc_dbg_table");
    void* dbg_cnt = rt_lib_resolve_symbol(h, "__arc_dbg_count");
    if (dbg_tbl && dbg_cnt) {
        rt_debug_module_register(h, dbg_tbl, *(int32_t*)dbg_cnt);
    }

    /* RFC 017 阶段一任务⑥：__arc_module_init 触发（句柄级 once 守卫）。
     * 插件 dll 无 main 入口调用点，静态初始化（__sinit_<Class> 拓扑序 +
     * 基元 typeinfo 槽回填）须由加载层触发——对齐 main 入口语义（EventLoop
     * 创建前调用）。
     *
     * 守卫：LoadLibrary/dlopen 按引用计数返回句柄——同路径重复加载产生多个
     * 代数槽共享同一份模块内存，静态初始化器无重入防护，重放即二次执行静态
     * 构造。故仅最小代数槽执行：扫描更小代数的同句柄槽，见 init_done==2（已完成）
     * 则跳过；==1（执行中）则等待收敛（两阶段标记 + 锁外自旋，对齐 in_flight
     * 让步风格）。地址复用（句柄回收后再加载）已由上方 tombstone 清理置空旧槽
     * 句柄，天然 re-arm。
     *
     * init 在 registry 锁外执行：静态初始化器是用户代码，可能重入 rt_library_*
     * 取锁函数（atomic_flag 非重入，锁内执行将自死锁）。 */
    void* init_fn = rt_lib_resolve_symbol(h, "__arc_module_init");
    int32_t run_init = 0;
    int32_t wait_init = 0;
    if (init_fn) {
        int32_t prior_done = 0;
        int32_t prior_running = 0;
        for (int32_t i = 0; i < slot; i++) {
            RtLibModule* o = &g_lib_modules[i];
            int32_t st = atomic_load(&o->state);
            if ((st == RT_LIB_STATE_ACTIVE || st == RT_LIB_STATE_FREEZING) &&
                atomic_load(&o->handle) == h) {
                int32_t done = atomic_load(&o->init_done);
                if (done == 2) {
                    prior_done = 1;
                } else if (done == 1) {
                    prior_running = 1;
                }
            }
        }
        if (!prior_done && !prior_running) {
            run_init = 1;
            atomic_store(&m->init_done, 1);
        } else if (prior_running) {
            wait_init = 1;
        }
    }
    rt_lib_unlock();

    if (run_init) {
        ((void (*)(void))init_fn)();
        atomic_store(&m->init_done, 2);
    } else if (wait_init) {
        /* 同句柄先行槽 init 进行中：有界自旋等待其收敛，避免本槽在静态字段
         * 初始化完成前被使用。超时退化（先行槽 unload 与本 load 极端竞争）
         * 直接返回——生产宿主不并发 load/unload 同一路径。 */
        int32_t spins = 0;
        while (spins < RT_LIB_INFLIGHT_SPIN_MAX) {
            int32_t settled = 0;
            for (int32_t i = 0; i < slot; i++) {
                RtLibModule* o = &g_lib_modules[i];
                int32_t st = atomic_load(&o->state);
                if ((st == RT_LIB_STATE_ACTIVE || st == RT_LIB_STATE_FREEZING) &&
                    atomic_load(&o->handle) == h &&
                    atomic_load(&o->init_done) == 2) {
                    settled = 1;
                    break;
                }
            }
            if (settled) {
                break;
            }
            rt_lib_yield();
            spins++;
        }
    }
    return h;
}

/// 查找动态库中的符号。
///
/// handle 必须为 rt_library_load 返回的有效句柄；name 为符号名。
/// 失败返回 NULL（符号不存在或句柄无效）。
///
/// RFC 017：句柄对应已卸载代数（tombstone）→ `E_UNLOAD_HANGING_REF`
/// 硬错误（`rt_panic`），禁静默。
///
/// 注意：返回的 void* 是函数指针/数据符号的地址，不是 Arc 对象。
/// 调用方需自行确保符号签名与期望一致（编译器核心不感知领域约定符号语义）。
void* rt_library_sym(void* handle, const char* name) {
    if (!handle || !name) return NULL;

    RtLibModule* m = rt_lib_find_by_handle(handle);
    if (m && atomic_load(&m->state) == RT_LIB_STATE_TOMBSTONED) {
        rt_lib_report_hanging(handle, "rt_library_sym", m);
        return NULL; /* rt_panic 终止；防御性返回 */
    }

#if defined(RT_LIBRARY_WINDOWS)
    return (void*)GetProcAddress((HMODULE)handle, name);
#else
    return dlsym(handle, name);
#endif
}

/// 卸载动态库（冷卸载路径，RFC 017 保留既有单 Assembly 冷路径）。
///
/// NULL 安全——NULL 句柄直接返回。
///
/// RFC 017：卸载前置 = 无外部强引用（ledger 归零）+ 无在途调用；违反 →
/// `E_UNLOAD_HANGING_REF` 硬错误（禁静默 dlclose 悬垂）。已卸载句柄再次
/// 卸载为幂等 no-op。
void rt_library_unload(void* handle) {
    if (!handle) return;

    RtLibModule* m = rt_lib_find_by_handle(handle);
    if (m && atomic_load(&m->state) == RT_LIB_STATE_TOMBSTONED) {
        return; /* 幂等：已卸载 */
    }
    if (m) {
        if (atomic_load(&m->ledger) > 0) {
            rt_lib_report_hanging(handle, "rt_library_unload", m);
            return;
        }
        if (atomic_load(&m->in_flight) > 0) {
            rt_lib_report_hanging(handle, "rt_library_unload(in-flight)", m);
            return;
        }
    }

    /* RFC 017 阶段一任务⑥：dbg 表注销——表内存位于模块数据段，须在
     * FreeLibrary/dlclose 之前移出 rt_debug registry（对齐 release_roots 次序）。
     * 按槽配对移除一条：同句柄多代数槽各自持登记，最后一条注销才移除符号化视野。 */
    rt_debug_module_unregister(handle);

#if defined(RT_LIBRARY_WINDOWS)
    FreeLibrary((HMODULE)handle);
#else
    dlclose(handle);
#endif
    if (m) {
        atomic_store(&m->state, RT_LIB_STATE_TOMBSTONED);
    }
}

/// 热卸载闭环（RFC 017 §2.4）：Freeze → 在途收敛 → 归零检测 → 释放根 →
/// dlclose → tombstone。
///
/// 返回：
///   RT_LIBRARY_UNLOAD_OK (1)        — 卸载成功
///   RT_LIBRARY_UNLOAD_HANGING (0)   — 存在外部强引用，拒绝卸载（报告）
///   RT_LIBRARY_UNLOAD_INFLIGHT (-1) — 在途调用未收敛（须在无模块代码执行时发起）
///   RT_LIBRARY_UNLOAD_INVALID (-2)  — 句柄无效 / 已被并发卸载
int32_t rt_library_unload_hot(void* handle) {
    if (!handle) return RT_LIBRARY_UNLOAD_INVALID;

    RtLibModule* m = rt_lib_find_by_handle(handle);
    if (!m) return RT_LIBRARY_UNLOAD_INVALID;

    /* Freeze：CAS ACTIVE → FREEZING，仅一个线程赢得卸载权 */
    int32_t expect = RT_LIB_STATE_ACTIVE;
    if (!atomic_compare_exchange_strong(&m->state, &expect,
                                        RT_LIB_STATE_FREEZING)) {
        return RT_LIBRARY_UNLOAD_INVALID; /* 已被他人卸载 / 正在卸载 */
    }

    /* 等待在途调用收敛（有界；对齐 .NET Freeze 语义） */
    int32_t spins = 0;
    while (atomic_load(&m->in_flight) > 0 && spins < RT_LIB_INFLIGHT_SPIN_MAX) {
        rt_lib_yield();
        spins++;
    }
    if (atomic_load(&m->in_flight) > 0) {
        atomic_store(&m->state, RT_LIB_STATE_ACTIVE); /* 回滚 */
        return RT_LIBRARY_UNLOAD_INFLIGHT;
    }

    /* 归零检测：ledger + 根扫描一致性复核 */
    if (atomic_load(&m->ledger) > 0) {
        atomic_store(&m->state, RT_LIB_STATE_ACTIVE); /* 回滚 */
        return RT_LIBRARY_UNLOAD_HANGING;
    }

    /* RFC 017 §2.6: 中和模块边界弱槽位——target 置空（观察 tombstone 头
     * 语义）。Weak<T> 不阻止卸载（ledger 不计弱引用）；卸载后宿主持有的
     * Weak<T>.TryGet() 确定性返回 NULL，禁悬垂复活。 */
    rt_lib_neutralize_weak(m);

    /* 释放模块根 */
    rt_lib_release_roots(m);

    /* RFC 017 阶段一任务⑥：dbg 表注销——同 rt_library_unload，须在
     * FreeLibrary/dlclose 之前按槽配对移除（对齐 release_roots 次序）。 */
    rt_debug_module_unregister(handle);

#if defined(RT_LIBRARY_WINDOWS)
    FreeLibrary((HMODULE)handle);
#else
    dlclose(handle);
#endif
    atomic_store(&m->state, RT_LIB_STATE_TOMBSTONED);
    return RT_LIBRARY_UNLOAD_OK;
}

/// 查询模块代数（RFC 017 §2.2）。
///
/// handle 为 rt_library_load 返回的句柄；返回模块当前代数
/// （1..RT_LIB_REGISTRY_CAPACITY）。tombstone/未知句柄返回 0。
int32_t rt_library_generation(void* handle) {
    if (!handle) return 0;
    RtLibModule* m = rt_lib_find_by_handle(handle);
    if (!m) return 0;
    int32_t state = atomic_load(&m->state);
    if (state != RT_LIB_STATE_ACTIVE && state != RT_LIB_STATE_FREEZING) {
        return 0;
    }
    return (int32_t)(m - g_lib_modules) + 1;
}

/// 登记跨模块外部强引用（ledger++）。
///
/// 模块边界点（宿主持模块对象引用 / Entry 返回值跨模块传递）调用。
/// 返回 0 = 代数无效或模块已 Freeze/卸载。
int32_t rt_library_ref_register(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    if (atomic_load(&m->state) != RT_LIB_STATE_ACTIVE) return 0;
    atomic_fetch_add(&m->ledger, 1);
    return 1;
}

/// 释放跨模块外部强引用（ledger--）。
///
/// 返回 0 = 计数已为 0（重复释放）或代数无效。
int32_t rt_library_ref_unregister(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    int32_t cur = atomic_load(&m->ledger);
    while (cur > 0) {
        if (atomic_compare_exchange_weak(&m->ledger, &cur, cur - 1)) {
            return 1;
        }
    }
    return 0;
}

/// 查询模块外部强引用计数（0 = 可卸载）。
int32_t rt_library_ref_count(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    return atomic_load(&m->ledger);
}

/// 模块代码在途调用进入 +1。
///
/// Entry / 导出符号调用进入时调用；Freeze/失效代数返回 0（调用被拒绝）。
int32_t rt_library_call_enter(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    if (atomic_load(&m->state) != RT_LIB_STATE_ACTIVE) return 0;
    atomic_fetch_add(&m->in_flight, 1);
    return 1;
}

/// 模块代码在途调用返回 -1（下限 0）。
int32_t rt_library_call_leave(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    int32_t cur = atomic_load(&m->in_flight);
    while (cur > 0) {
        if (atomic_compare_exchange_weak(&m->in_flight, &cur, cur - 1)) {
            break;
        }
    }
    return 1;
}

/// 登记模块根（模块静态持有的 class 引用）。
///
/// 卸载前 `rt_library_release_roots` 统一释放。返回 0 = 无效代数 /
/// 模块已 Freeze / 槽已满；去重幂等。
int32_t rt_library_root_add(int32_t generation, void* root) {
    if (!root) return 0;
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    if (atomic_load(&m->state) != RT_LIB_STATE_ACTIVE) return 0;
    rt_lib_lock();
    for (int32_t i = 0; i < m->root_count; i++) {
        if (m->roots[i] == root) {
            rt_lib_unlock();
            return 1; /* 已登记 */
        }
    }
    if (m->root_count >= RT_LIB_MAX_ROOTS) {
        rt_lib_unlock();
        return 0;
    }
    m->roots[m->root_count++] = root;
    rt_lib_unlock();
    return 1;
}

/// 移除模块根。返回 0 = 未登记 / 代数无效。
int32_t rt_library_root_remove(int32_t generation, void* root) {
    if (!root) return 0;
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    rt_lib_lock();
    for (int32_t i = 0; i < m->root_count; i++) {
        if (m->roots[i] == root) {
            m->roots[i] = m->roots[m->root_count - 1];
            m->root_count--;
            rt_lib_unlock();
            return 1;
        }
    }
    rt_lib_unlock();
    return 0;
}

/// ARC 根扫描（RFC 017 §2.3）：枚举已登记模块根并沿 strong 字段遍历
/// 可达闭包（复用 `rt_arc_walk_fields`），复核 ledger 归零。
///
/// 返回 1 = 可卸载（无模块外强引用）；0 = 不可卸载（外部引用非零 /
/// 代数无效 / 模块已卸载）。不引入全堆扫描（仅模块根可达闭包）。
int32_t rt_library_root_scan(int32_t generation) {
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    int32_t state = atomic_load(&m->state);
    if (state != RT_LIB_STATE_ACTIVE && state != RT_LIB_STATE_FREEZING) {
        return 0;
    }
    if (atomic_load(&m->ledger) > 0) return 0;

    RtScanCtx ctx;
    ctx.count = 0;
    rt_lib_lock();
    for (int32_t i = 0; i < m->root_count; i++) {
        void* root = m->roots[i];
        if (!root) continue;
        rt_arc_walk_fields(root, rt_lib_scan_visit, &ctx);
    }
    /* RFC 017 §2.3：枚举 codegen 模块根表槽位——懒读取槽位当前对象并沿
     * strong 字段遍历（与显式登记根同走 `rt_arc_walk_fields` 环防护 DFS）。 */
    for (int32_t i = 0; i < m->codegen_root_count; i++) {
        void* slot = m->codegen_roots[i];
        if (!slot) continue;
        void* obj = *(void**)slot;
        if (!obj) continue;
        rt_arc_walk_fields(obj, rt_lib_scan_visit, &ctx);
    }
    rt_lib_unlock();
    return 1;
}

/// 登记模块边界弱槽位（RFC 017 §2.6 宿主侧弱登记表）。
///
/// 宿主声明「该 Weak\<T\> 指向本模块对象」：槽位盖上模块代数并登记进模块
/// 弱表；模块卸载时被中和 → 卸载后 TryGet() 返回 NULL。Weak\<T\> **不**阻止
/// 卸载（ledger 不计弱引用）。返回 0 = 无效代数 / 模块已 Freeze / 表满；
/// 已登记视为幂等成功。
int32_t rt_library_weak_register(int32_t generation, void* weakslot) {
    if (!weakslot) return 0;
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    if (atomic_load(&m->state) != RT_LIB_STATE_ACTIVE) return 0;
    rt_lib_lock();
    for (int32_t i = 0; i < m->weak_count; i++) {
        if (m->weak_slots[i] == weakslot) {
            rt_lib_unlock();
            return 1; /* 已登记（幂等） */
        }
    }
    if (m->weak_count >= RT_LIB_MAX_WEAK) {
        rt_lib_unlock();
        return 0;
    }
    m->weak_slots[m->weak_count++] = weakslot;
    rt_arc_weak_set_generation(weakslot, generation);
    rt_lib_unlock();
    return 1;
}

/// 移除模块边界弱槽位登记（显式解除；Weak\<T\> 析构亦自动 untrack）。
/// 返回 0 = 未登记 / 代数无效。
int32_t rt_library_weak_unregister(int32_t generation, void* weakslot) {
    if (!weakslot) return 0;
    RtLibModule* m = rt_lib_module(generation);
    if (!m) return 0;
    rt_lib_lock();
    for (int32_t i = 0; i < m->weak_count; i++) {
        if (m->weak_slots[i] == weakslot) {
            m->weak_slots[i] = m->weak_slots[m->weak_count - 1];
            m->weak_count--;
            rt_arc_weak_set_generation(weakslot, 0);
            rt_lib_unlock();
            return 1;
        }
    }
    rt_lib_unlock();
    return 0;
}

/// 按槽位指针解除登记（`rt_arc_weak_destroy` 析构路径调用；代数无关，
/// 未登记 / 已中和均幂等 no-op）。优先查槽位盖上代数的模块表；未命中再
/// 全表扫描（模块登记槽可能已被 OS 地址复用）。
void rt_library_weak_untrack(void* weakslot) {
    if (!weakslot) return;
    int32_t gen = rt_arc_weak_generation(weakslot);
    if (gen > 0 && rt_library_weak_unregister(gen, weakslot)) {
        return;
    }
    rt_lib_lock();
    for (int32_t i = 0; i < RT_LIB_REGISTRY_CAPACITY; i++) {
        RtLibModule* m = &g_lib_modules[i];
        if (atomic_load(&m->state) == RT_LIB_STATE_INVALID) continue;
        for (int32_t j = 0; j < m->weak_count; j++) {
            if (m->weak_slots[j] == weakslot) {
                m->weak_slots[j] = m->weak_slots[m->weak_count - 1];
                m->weak_count--;
                rt_arc_weak_set_generation(weakslot, 0);
                rt_lib_unlock();
                return;
            }
        }
    }
    rt_lib_unlock();
}

/// 读取动态库的包元数据（RFC 017 M4）。
///
/// 返回指向库内 `@__arc_package_meta` 字符串的指针，格式为
///   "name\0version\0edition\0"
/// 由 Arc 编译器在 `arc build --dynamic` 时从 arc.toml [package] 节嵌入。
///
/// 返回 NULL 表示库未携带元数据（如无 manifest 的单文件编译）。
/// 调用方无需 free 返回指针——它指向已加载库的只读内存映射。
///
/// RFC 017：句柄对应已卸载代数 → `E_UNLOAD_HANGING_REF` 硬错误。
const char* rt_library_get_meta(void* handle) {
    if (!handle) return NULL;

    RtLibModule* m = rt_lib_find_by_handle(handle);
    if (m && atomic_load(&m->state) == RT_LIB_STATE_TOMBSTONED) {
        rt_lib_report_hanging(handle, "rt_library_get_meta", m);
        return NULL;
    }

    return (const char*)rt_library_sym(handle, "__arc_package_meta");
}

/// 按索引读取动态库包元数据字段（RFC 017 M4 修复，additive ABI）。
///
/// meta 格式为 "name\0version\0edition\0[dep1\0dep2\0...]\0"（NUL 分隔，
/// 末尾双 NUL 终止：末字段为显式空串标记结束）。Arc `string` 为纯 C-string——
/// `rt_str_length`/`rt_str_index_of_char` 均用 strlen/strchr，在首个 NUL 处
/// 截断，故宿主无法经 `IndexOf('\0')` 读取 version/edition（已实证损坏：仅
/// name 可读）。本 ABI 用 strchr 按 NUL 分段，按索引返回字段起始指针：
/// 0=name、1=version、2=edition、3+ 为依赖列表。
///
/// 字段本身无内嵌 NUL，返回指针可直接作为 Arc string 使用（指向库只读内存
/// 映射，调用方无需 free）。索引越界（< 0 或超过字段数，即目标为空串终止
/// 字段）返回 NULL；句柄对应已卸载代数 → E_UNLOAD_HANGING_REF 硬错误
/// （经 rt_library_get_meta 检测）。
const char* rt_library_get_meta_field(void* handle, int32_t index) {
    if (index < 0) return NULL;

    const char* meta = rt_library_get_meta(handle);
    if (!meta) return NULL;

    const char* p = meta;
    for (int32_t i = 0; i < index; i++) {
        if (*p == '\0') return NULL; /* 空字段（双 NUL 终止）：字段不足 */
        const char* nul = (const char*)strchr(p, '\0');
        if (!nul) return NULL; /* 字段不足 */
        p = nul + 1;
    }
    /* 越界：目标字段为空串（双 NUL 终止）→ NULL。 */
    return (*p == '\0') ? NULL : p;
}

/* ---- RFC 047: 透明对象图迁移（热重载 L3） ----
 * codegen 发射 `__arc_vtable_registry`（`{name, layout_sig, shape_hash,
 * slot_count}` 数组，仅本 TU 定义的 class）+ `__arc_vtable_registry_count`。
 * vtable 指针不物化于 registry——迁移时按名 `.vtable.{T}` 双侧现场解析
 * （消除「类在 layouts 但 vtable 全局未发射」的未定义符号链接风险）。
 * 迁移 = 构建旧→新 vtable 映射（同名 + 三元全等判定）后沿模块代数根 DFS
 * 重绑（rt_arc_retype：只改头 vtable，不改地址——引用值全部不变）。 */

#define RT_MIGRATE_MAX 256

typedef struct {
    const char* type_name;
    int64_t layout_sig;
    int64_t shape_hash;
    int32_t slot_count;
} RtVtableRegistryEntry;

typedef struct {
    const void* old_vt;
    const void* new_vt;
} RtVtMapping;

typedef struct {
    RtVtMapping* map;
    int32_t map_count;
    void* visited[RT_LIB_SCAN_MAX];
    int32_t visited_count;
    int32_t migrated;
} RtMigrateCtx;

/* 迁移 visit：命中映射 → retype + 沿字段继续（旧 registry 成员类型的对象
 * 才继续走——非成员（宿主/std 对象）不进入外域图）。vtable 读取经
 * rt_arc_vtable_of（ArcHeader 定义不跨编译单元暴露）。 */
static void rt_migrate_visit(void* ctx, void* field) {
    RtMigrateCtx* c = (RtMigrateCtx*)ctx;
    if (!field) return;
    if (c->visited_count >= RT_LIB_SCAN_MAX) return;
    for (int32_t i = 0; i < c->visited_count; i++) {
        if (c->visited[i] == field) return;
    }
    c->visited[c->visited_count++] = field;
    const void* vt = rt_arc_vtable_of(field);
    if (!vt) return;
    for (int32_t i = 0; i < c->map_count; i++) {
        if (c->map[i].old_vt != vt) continue;
        rt_arc_retype(field, c->map[i].new_vt);
        c->migrated++;
        /* 重绑后 walk 经新 vtable slot 2——布局一致故字段遍历偏移等价。 */
        rt_arc_walk_fields(field, rt_migrate_visit, c);
        return;
    }
    /* 未命中 = 非旧代类型（新代/宿主/std 对象）——不进入外域图。 */
}

int32_t rt_library_migrate_instances(int32_t old_generation, int32_t new_generation) {
    if (old_generation <= 0 || new_generation <= 0 ||
        old_generation == new_generation) {
        return -1;
    }
    RtLibModule* mo = rt_lib_module(old_generation);
    RtLibModule* mn = rt_lib_module(new_generation);
    if (!mo || !mn) return -1;
    if (atomic_load(&mo->state) != RT_LIB_STATE_ACTIVE ||
        atomic_load(&mn->state) != RT_LIB_STATE_ACTIVE) {
        return -1;
    }
    void* old_handle = atomic_load(&mo->handle);
    void* new_handle = atomic_load(&mn->handle);
    if (!old_handle || !new_handle) return -1;

    RtVtableRegistryEntry* old_reg =
        (RtVtableRegistryEntry*)rt_lib_resolve_symbol(old_handle, "__arc_vtable_registry");
    int32_t* old_count_p =
        (int32_t*)rt_lib_resolve_symbol(old_handle, "__arc_vtable_registry_count");
    RtVtableRegistryEntry* new_reg =
        (RtVtableRegistryEntry*)rt_lib_resolve_symbol(new_handle, "__arc_vtable_registry");
    int32_t* new_count_p =
        (int32_t*)rt_lib_resolve_symbol(new_handle, "__arc_vtable_registry_count");
    if (!old_reg || !old_count_p || !new_reg || !new_count_p) return -2;
    int32_t old_n = *old_count_p;
    int32_t new_n = *new_count_p;
    int dbg = getenv("ARC_DEBUG_MIGRATE") != NULL;
    if (dbg) {
        fprintf(stderr, "[mig-dbg] old_n=%d new_n=%d\n", old_n, new_n);
        for (int32_t i = 0; i < old_n; i++) {
            fprintf(stderr, "[mig-dbg] old[%d]=%s slots=%d ls=%lld sh=%lld\n", i,
                    old_reg[i].type_name ? old_reg[i].type_name : "<null>",
                    old_reg[i].slot_count,
                    (long long)old_reg[i].layout_sig,
                    (long long)old_reg[i].shape_hash);
        }
        for (int32_t j = 0; j < new_n; j++) {
            fprintf(stderr, "[mig-dbg] new[%d]=%s slots=%d ls=%lld sh=%lld\n", j,
                    new_reg[j].type_name ? new_reg[j].type_name : "<null>",
                    new_reg[j].slot_count,
                    (long long)new_reg[j].layout_sig,
                    (long long)new_reg[j].shape_hash);
        }
    }
    if (old_n < 0 || old_n > RT_MIGRATE_MAX || new_n < 0 || new_n > RT_MIGRATE_MAX) {
        return -2;
    }

    /* 映射构建 + 双重判定（slot_count + shape_hash + layout_sig 全等）；
     * vtable 双侧按名现场解析（`.vtable.{T}`）。任一旧代类型不可迁移 →
     * 整体拒绝（-3），编排器降级 L2 或拒绝换代。 */
    RtVtMapping map[RT_MIGRATE_MAX];
    int32_t map_n = 0;
    char vt_sym[128];
    for (int32_t i = 0; i < old_n; i++) {
        RtVtableRegistryEntry* e = &old_reg[i];
        if (!e->type_name || e->slot_count < 0) continue;
        int32_t j;
        int32_t hit = 0;
        for (j = 0; j < new_n; j++) {
            RtVtableRegistryEntry* n = &new_reg[j];
            if (!n->type_name || strcmp(n->type_name, e->type_name) != 0) continue;
            if (n->slot_count != e->slot_count || n->layout_sig != e->layout_sig ||
                n->shape_hash != e->shape_hash) {
                return -3; /* 不可迁移类型存在 → 整体拒绝（严格语义） */
            }
            int32_t written = snprintf(vt_sym, sizeof(vt_sym), ".vtable.%s",
                                       e->type_name);
            if (written <= 0 || written >= (int32_t)sizeof(vt_sym)) return -2;
            const void* old_vt = rt_lib_resolve_symbol(old_handle, vt_sym);
            const void* new_vt = rt_lib_resolve_symbol(new_handle, vt_sym);
            if (!old_vt || !new_vt) return -3; /* vtable 全局缺失 → 保守拒绝 */
            map[map_n].old_vt = old_vt;
            map[map_n].new_vt = new_vt;
            map_n++;
            hit = 1;
            break;
        }
        if (!hit) return -3; /* 新代缺失同名类型 → 整体拒绝 */
    }
    if (map_n == 0) return 0;

    RtMigrateCtx ctx;
    ctx.map = map;
    ctx.map_count = map_n;
    ctx.visited_count = 0;
    ctx.migrated = 0;

    /* Freeze 态（调用方保证 in-flight 已收敛）：持登记锁遍历 + 重绑。 */
    rt_lib_lock();
    /* 根集 = 显式登记根 + codegen 模块根表槽位当前对象（对齐 release_roots）。 */
    for (int32_t i = 0; i < mo->root_count; i++) {
        if (mo->roots[i]) rt_migrate_visit(&ctx, mo->roots[i]);
    }
    for (int32_t i = 0; i < mo->codegen_root_count; i++) {
        void* slot = mo->codegen_roots[i];
        if (!slot) continue;
        void* obj = *(void**)slot;
        if (obj) rt_migrate_visit(&ctx, obj);
    }
    rt_lib_unlock();
    return ctx.migrated;
}
