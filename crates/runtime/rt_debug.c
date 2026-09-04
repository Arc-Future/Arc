/* rt_debug.c — Runtime debug symbolization (RFC 017 M2 / D5.2)
 *
 * Provides address → symbol resolution for rt_backtrace / Exception.StackTrace.
 *
 * Primary path: embedded `__arc_dbg_table` emitted by codegen for every build
 * (independent of DWARF -g). Contains (fn_ptr, name, file, line, col).
 * Works on all platforms (Windows MSVC/MinGW, POSIX), no file I/O, no extra
 * linker deps.
 *
 * ARC frame folding: symbols starting with "rt_" or "__arc_" are skipped
 * in backtraces (runtime-internal frames).
 *
 * Lookup uses largest fn_ptr <= addr, clamped to RT_DBG_MAX_FN_BYTES so
 * addresses in foreign code are not falsely attributed to the nearest Arc fn.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdatomic.h>

/* Reject matches farther than this from the function start (mis-attribution guard). */
#define RT_DBG_MAX_FN_BYTES (256u * 1024u)

/* 模块 dbg 表 registry 容量（对齐 rt_library.c 的 RT_LIB_REGISTRY_CAPACITY） */
#define RT_DBG_MODULE_CAPACITY 256

/* ---- Embedded debug table types (must match codegen) ---- */

typedef struct ArcDbgEntry {
    void* fn_ptr;       /* function start address */
    const char* name;   /* source-level symbol name */
    const char* file;   /* source file path */
    int32_t line;       /* source line (0 = unknown) */
    int32_t col;        /* source column (0 = unknown) */
} ArcDbgEntry;

/* Always emitted by codegen (populated even without -g). */
extern ArcDbgEntry __arc_dbg_table[];
extern int32_t __arc_dbg_count;

/* Weak fallbacks (RFC 017 stage 1): arc_runtime links standalone, where no
 * module object supplies these symbols. The empty table makes rt_debug_lookup
 * a not-found no-op; statically linked builds (and plugin modules today) carry
 * the strong codegen-emitted definitions, which win over these at link time. */
__attribute__((weak)) ArcDbgEntry __arc_dbg_table[1] = {{0}};
__attribute__((weak)) int32_t __arc_dbg_count = 0;

/* ---- 模块 dbg 表 registry（RFC 017 阶段一：runtime 单副本共享）----
 *
 * 插件 dll 改为导入引用 arc_runtime 后，codegen 发射的 `__arc_dbg_table` 不再
 * 被 rt_debug.o 链接期就地解析（共享 runtime 看不到各模块数据段）——
 * `rt_library_load` 在加载期把模块 dbg 表登记进本 registry，`rt_debug_lookup`
 * 在内嵌表未命中后串联搜索各已登记模块。
 *
 * 登记/注销按「每代数槽一条」配对：同路径重复加载（OS 句柄引用计数）会产生
 * 多个代数槽共享同一份表内存，每槽各登记一条，卸载时各注销一条——条目数与
 * OS 引用计数同步，最后一条注销才从符号化视野移除表，与真实 dlclose 时点
 * 天然对齐（同表重复条目仅是 lookup 重复扫描，正确性无碍）。 */

typedef struct {
    void* handle;                /* 登记来源 OS 句柄（注销配对键） */
    const ArcDbgEntry* table;
    int32_t count;
} RtDbgModule;

static RtDbgModule g_dbg_modules[RT_DBG_MODULE_CAPACITY];
static atomic_flag g_dbg_registry_lock = ATOMIC_FLAG_INIT;

static void rt_dbg_lock(void) {
    while (atomic_flag_test_and_set_explicit(&g_dbg_registry_lock,
                                             memory_order_acquire)) {
        /* 自旋让步 */
    }
}

static void rt_dbg_unlock(void) {
    atomic_flag_clear_explicit(&g_dbg_registry_lock, memory_order_release);
}

/* ---- Public API ---- */

/* 登记一个模块 dbg 表（rt_library_load 加载期调用）。
 * 返回 1 = 登记成功；0 = 参数无效 / registry 满（dbg 为诊断能力，失败不阻塞加载）。 */
int32_t rt_debug_module_register(void* handle, const void* table, int32_t count) {
    if (!handle || !table || count <= 0) {
        return 0;
    }

    rt_dbg_lock();
    int32_t slot = -1;
    for (int32_t i = 0; i < RT_DBG_MODULE_CAPACITY; i++) {
        if (!g_dbg_modules[i].handle) {
            slot = i;
            break;
        }
    }
    if (slot >= 0) {
        g_dbg_modules[slot].handle = handle;
        g_dbg_modules[slot].table = (const ArcDbgEntry*)table;
        g_dbg_modules[slot].count = count;
    }
    rt_dbg_unlock();
    return slot >= 0 ? 1 : 0;
}

/* 注销一个模块 dbg 表（rt_library_unload / rt_library_unload_hot 在
 * FreeLibrary/dlclose 之前调用——表内存位于模块数据段，dlclose 后失效）。
 * 仅移除一个匹配条目：同句柄多代数槽各持一条注册，按槽配对移除。
 * 未登记句柄为幂等 no-op。 */
void rt_debug_module_unregister(void* handle) {
    if (!handle) {
        return;
    }

    rt_dbg_lock();
    for (int32_t i = 0; i < RT_DBG_MODULE_CAPACITY; i++) {
        if (g_dbg_modules[i].handle == handle) {
            g_dbg_modules[i].handle = NULL;
            g_dbg_modules[i].table = NULL;
            g_dbg_modules[i].count = 0;
            break;
        }
    }
    rt_dbg_unlock();
}

/* 在单张 dbg 表内查找（largest fn_ptr <= addr，RT_DBG_MAX_FN_BYTES 防误归属）。
 * Returns 1 if found (fills out params), 0 if not found. */
static int32_t rt_debug_search_table(const ArcDbgEntry* table,
                                     int32_t count,
                                     uint64_t addr,
                                     const char** symbol,
                                     const char** file,
                                     int32_t* line,
                                     int32_t* col) {
    if (!table || count <= 0) {
        return 0;
    }

    const ArcDbgEntry* best = NULL;
    for (int32_t i = 0; i < count; i++) {
        uint64_t fn_addr = (uint64_t)(uintptr_t)table[i].fn_ptr;
        if (fn_addr <= addr) {
            if (!best || fn_addr > (uint64_t)(uintptr_t)best->fn_ptr) {
                best = &table[i];
            }
        }
    }

    if (!best) {
        return 0;
    }

    uint64_t fn_addr = (uint64_t)(uintptr_t)best->fn_ptr;
    if (addr - fn_addr >= (uint64_t)RT_DBG_MAX_FN_BYTES) {
        return 0;
    }

    if (symbol) *symbol = best->name;
    if (file) *file = best->file;
    if (line) *line = best->line;
    if (col) *col = best->col;
    return 1;
}

/* Look up an address for symbolization: embedded table first (statically
 * linked main image / legacy embedded-rt plugins), then every registered
 * module table (RFC 017 stage 1 shared-runtime form).
 * Returns 1 if found (fills out params), 0 if not found. */
int32_t rt_debug_lookup(uint64_t addr,
                        const char** symbol,
                        const char** file,
                        int32_t* line,
                        int32_t* col) {
    if (rt_debug_search_table(__arc_dbg_table, __arc_dbg_count, addr,
                              symbol, file, line, col)) {
        return 1;
    }

    rt_dbg_lock();
    for (int32_t i = 0; i < RT_DBG_MODULE_CAPACITY; i++) {
        const RtDbgModule* m = &g_dbg_modules[i];
        if (m->handle &&
            rt_debug_search_table(m->table, m->count, addr,
                                  symbol, file, line, col)) {
            rt_dbg_unlock();
            return 1;
        }
    }
    rt_dbg_unlock();
    return 0;
}

/* Check if a symbol name is a runtime-internal frame (for ARC frame folding).
 * Returns 1 if the symbol should be skipped in backtraces, 0 otherwise.
 */
int32_t rt_debug_is_arc_frame(const char* symbol) {
    if (!symbol) return 0;

    /* Runtime functions follow the rt_ prefix convention. */
    if (strncmp(symbol, "rt_", 3) == 0) return 1;
    if (strncmp(symbol, "__arc_", 6) == 0) return 1;

    return 0;
}
