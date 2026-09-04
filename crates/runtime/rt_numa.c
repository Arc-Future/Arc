// NUMA 感知调度 ABI 实现（RFC 009 M5）。
//
// 多平台差异化实现：
//   - Linux: sysfs 拓扑查询 + mbind/set_mempolicy 内存绑定 + pthread_setaffinity_np 线程绑定
//   - Windows: GetNumaHighestNodeNumber + GetNumaProcessorNodeEx + SetThreadGroupAffinity + VirtualAllocExNuma
//   - macOS / 其他平台: 降级为单一 NUMA node（无错误，仅无性能收益）
//
// 平台差异化处理原则（RFC 009 §5.4）：
//   1. 拓扑查询平台相关——Linux 读 sysfs，Windows 调 NUMAAPI，macOS 降级
//   2. 线程绑定平台相关——Linux 用 CPU_SET，Windows 用 GROUP_AFFINITY
//   3. 内存分配平台相关——Linux 用 mbind，Windows 用 VirtualAllocExNuma
//   4. 所有 ABI 函数在所有平台均可用——不支持时降级，永不报错
//   5. numa_aware=0 时全部走标准 malloc/free，无额外开销

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ---- 平台检测 ---- */
#if defined(__linux__)
  #define RT_NUMA_LINUX 1
#elif defined(_WIN32) || defined(_WIN64)
  #define RT_NUMA_WINDOWS 1
#else
  /* macOS / FreeBSD / 其他：降级为单一 NUMA node */
  #define RT_NUMA_STUB 1
#endif

/* ============================================================
 * Linux NUMA 实现
 * ============================================================ */
#if defined(RT_NUMA_LINUX)

#include <stdio.h>
#include <sched.h>
#include <unistd.h>

/* numa.h / numaif.h 可能不存在于非 libnuma 环境；仅使用 syscall 接口
 * 以避免对外部 libnuma 的依赖。核心 syscall：
 *   - set_mempolicy(mode, nodemask, maxnode)
 *   - mbind(addr, len, mode, nodemask, maxnode, flags)
 * 这些通过 <numaif.h> 声明；若不可用则降级。 */
#ifdef __has_include
  #if __has_include(<numaif.h>)
    #include <numaif.h>
    #define RT_NUMA_HAVE_NUMAIF 1
  #endif
#endif

/* 缓存的 NUMA 拓扑（首次查询后缓存，线程安全 via pthread_once） */
static int32_t g_numa_node_count = 0;
static int32_t g_numa_cpu_count = 0;
/* cpu_to_node[cpu] = node id；-1 表示未知 */
static int32_t g_numa_cpu_to_node[256] = { [0 ... 255] = -1 };

static void rt_numa_init_topology(void) {
    /* 读取 /sys/devices/system/node/online 获取 NUMA node 列表
     * 格式如 "0-3" 或 "0,2,4"。统计 node 数量取最大 node id + 1。 */
    FILE* f = fopen("/sys/devices/system/node/online", "r");
    if (f) {
        char buf[256];
        if (fgets(buf, sizeof(buf), f)) {
            /* 解析 "0-3" 格式：取最大值 +1 */
            int32_t max_node = 0;
            char* p = buf;
            while (*p) {
                if (*p >= '0' && *p <= '9') {
                    int32_t val = 0;
                    while (*p >= '0' && *p <= '9') {
                        val = val * 10 + (*p - '0');
                        p++;
                    }
                    if (val > max_node) max_node = val;
                    /* 跳过可能的 '-' 和第二个数字 */
                    if (*p == '-') {
                        p++;
                        int32_t val2 = 0;
                        while (*p >= '0' && *p <= '9') {
                            val2 = val2 * 10 + (*p - '0');
                            p++;
                        }
                        if (val2 > max_node) max_node = val2;
                    }
                } else {
                    p++;
                }
            }
            g_numa_node_count = max_node + 1;
        }
        fclose(f);
    }
    if (g_numa_node_count <= 0) g_numa_node_count = 1;

    /* 读取每个 CPU 的 physical_package_id 作为 node id */
    long ncpus = sysconf(_SC_NPROCESSORS_CONF);
    if (ncpus <= 0) ncpus = 1;
    if (ncpus > 256) ncpus = 256;  /* 数组上限保护 */
    g_numa_cpu_count = (int32_t)ncpus;

    for (int32_t cpu = 0; cpu < g_numa_cpu_count; cpu++) {
        char path[128];
        snprintf(path, sizeof(path),
                 "/sys/devices/system/cpu/cpu%d/topology/physical_package_id", cpu);
        FILE* cf = fopen(path, "r");
        if (cf) {
            int32_t node = -1;
            if (fscanf(cf, "%d", &node) == 1 && node >= 0) {
                g_numa_cpu_to_node[cpu] = node;
            }
            fclose(cf);
        }
        if (g_numa_cpu_to_node[cpu] < 0) {
            /* 查询失败：映射到 node 0 */
            g_numa_cpu_to_node[cpu] = 0;
        }
    }
}

/* 线程安全的惰性初始化 */
#include <pthread.h>
static pthread_once_t g_numa_once = PTHREAD_ONCE_INIT;
static void rt_numa_ensure_init(void) {
    pthread_once(&g_numa_once, rt_numa_init_topology);
}

int32_t rt_numa_node_count(void) {
    rt_numa_ensure_init();
    return g_numa_node_count;
}

int32_t rt_numa_cpu_to_node(int32_t cpu) {
    rt_numa_ensure_init();
    if (cpu < 0 || cpu >= g_numa_cpu_count) return 0;
    return g_numa_cpu_to_node[cpu];
}

void rt_numa_bind_worker(int32_t worker_id, int32_t node) {
    /* 将 worker 线程绑定到指定 NUMA node 的 CPU 集合。
     * 遍历所有 CPU，收集属于该 node 的 CPU，用 pthread_setaffinity_np 绑定。 */
    (void)worker_id;
    rt_numa_ensure_init();

    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    int32_t found = 0;
    for (int32_t cpu = 0; cpu < g_numa_cpu_count; cpu++) {
        if (g_numa_cpu_to_node[cpu] == node) {
            CPU_SET(cpu, &cpuset);
            found++;
        }
    }
    if (found > 0) {
        pthread_setaffinity_np(pthread_self(), sizeof(cpu_set_t), &cpuset);
    }
}

void* rt_numa_alloc_on_node(uint64_t size, int32_t node) {
    /* 在指定 NUMA node 上分配内存。
     * 优先使用 mbind 策略；若不可用则降级为普通 malloc。
     * first-touch 策略：分配时不立即绑定，由首次访问线程的本地性决定。 */
    void* ptr = malloc((size_t)size);
    if (ptr && node >= 0) {
#ifdef RT_NUMA_HAVE_NUMAIF
        /* 用 mbind 设置 MPOL_BIND 策略，将内存页绑定到指定 node。
         * nodemask = (1 << node) */
        unsigned long nodemask = (1UL << node);
        mbind(ptr, (size_t)size, MPOL_BIND, &nodemask, sizeof(nodemask) * 8, 0);
#else
        /* 无 numaif.h：first-touch 降级——由首次访问线程的 NUMA 本地性决定。
         * 配合 rt_numa_bind_worker 可达近似效果。 */
        (void)node;
#endif
    }
    return ptr;
}

void rt_numa_free(void* ptr) {
    free(ptr);
}

/* ============================================================
 * Windows NUMA 实现
 * ============================================================ */
#elif defined(RT_NUMA_WINDOWS)

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static int32_t g_numa_node_count = 0;

static void rt_numa_ensure_init(void) {
    /* Windows NUMA 拓扑通过 GetNumaHighestNodeNumber 查询 */
    if (g_numa_node_count > 0) return;
    ULONG highest_node = 0;
    if (GetNumaHighestNodeNumber(&highest_node) && highest_node > 0) {
        g_numa_node_count = (int32_t)(highest_node + 1);
    } else {
        g_numa_node_count = 1;  /* 单一 NUMA node 或查询失败 */
    }
}

int32_t rt_numa_node_count(void) {
    rt_numa_ensure_init();
    return g_numa_node_count;
}

int32_t rt_numa_cpu_to_node(int32_t cpu) {
    rt_numa_ensure_init();
    /* Windows: GetNumaProcessorNodeEx 需要 PROCESSOR_NUMBER 结构。
     * 简化实现：使用旧的 GetNumaProcessorNode（仅支持 64 CPU 以内）。 */
    if (g_numa_node_count <= 1) return 0;
    /* 对于超过 64 CPU 的系统（多 group），降级为 node 0。
     * 完整实现需遍历 group + GetNumaProcessorNodeEx。 */
    if (cpu < 0 || cpu >= 64) return 0;
    UCHAR node = 0;
    /* GetNumaProcessorNode 在新版 Windows 已废弃，
     * 但作为兼容降级路径仍可用。失败时返回 node 0。 */
    typedef BOOL (WINAPI *PFN_GetNumaProcessorNode)(UCHAR, PUCHAR);
    static PFN_GetNumaProcessorNode pfn = NULL;
    if (!pfn) {
        HMODULE h = GetModuleHandleW(L"kernel32.dll");
        if (h) pfn = (PFN_GetNumaProcessorNode)GetProcAddress(h, "GetNumaProcessorNode");
    }
    if (pfn && pfn((UCHAR)cpu, &node)) {
        return (int32_t)node;
    }
    return 0;
}

void rt_numa_bind_worker(int32_t worker_id, int32_t node) {
    /* 将 worker 线程绑定到指定 NUMA node。
     * Windows 使用 SetThreadGroupAffinity 设置线程亲和性。 */
    (void)worker_id;
    rt_numa_ensure_init();
    if (g_numa_node_count <= 1 || node < 0) return;

    /* 构造 GROUP_AFFINITY：收集属于该 node 的处理器。
     * 简化：对单 group 系统（≤64 CPU），设置 mask 包含该 node 所有 CPU。 */
    /* 遍历 CPU 0-63，查询其 node，匹配则置位 */
    typedef BOOL (WINAPI *PFN_GetNumaProcessorNode)(UCHAR, PUCHAR);
    static PFN_GetNumaProcessorNode pfn_get = NULL;
    if (!pfn_get) {
        HMODULE h = GetModuleHandleW(L"kernel32.dll");
        if (h) pfn_get = (PFN_GetNumaProcessorNode)GetProcAddress(h, "GetNumaProcessorNode");
    }

    GROUP_AFFINITY ga;
    memset(&ga, 0, sizeof(ga));
    ga.Group = 0;  /* 单 group 假设；多 group 系统需更复杂处理 */

    if (pfn_get) {
        for (int32_t cpu = 0; cpu < 64; cpu++) {
            UCHAR cpu_node = 0;
            if (pfn_get((UCHAR)cpu, &cpu_node) && (int32_t)cpu_node == node) {
                ga.Mask |= (KAFFINITY)(1ULL << cpu);
            }
        }
    }
    if (ga.Mask != 0) {
        SetThreadGroupAffinity(GetCurrentThread(), &ga, NULL);
    }
}

void* rt_numa_alloc_on_node(uint64_t size, int32_t node) {
    /* Windows: VirtualAllocExNuma 在指定 node 分配内存。
     * 降级：node<=0 或函数不可用时走标准 malloc。 */
    rt_numa_ensure_init();
    if (g_numa_node_count <= 1 || node < 0) {
        return malloc((size_t)size);
    }
    /* 动态加载 VirtualAllocExNuma（Windows Vista+ NUMA API） */
    typedef LPVOID (WINAPI *PFN_VirtualAllocExNuma)(HANDLE, LPVOID, SIZE_T, DWORD, DWORD, DWORD);
    static PFN_VirtualAllocExNuma pfn = NULL;
    if (!pfn) {
        HMODULE h = GetModuleHandleW(L"kernel32.dll");
        if (h) pfn = (PFN_VirtualAllocExNuma)GetProcAddress(h, "VirtualAllocExNuma");
    }
    if (pfn) {
        DWORD alloc_size = ((DWORD)((size + 0xFFFF) & ~0xFFFF));  /* 64KB 对齐 */
        LPVOID ptr = pfn(GetCurrentProcess(), NULL, alloc_size,
                         MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE, (DWORD)node);
        if (ptr) return ptr;
    }
    /* 降级：标准 malloc */
    return malloc((size_t)size);
}

void rt_numa_free(void* ptr) {
    free(ptr);
}

/* ============================================================
 * macOS / 其他平台：降级桩实现
 * ============================================================ */
#else  /* RT_NUMA_STUB */

int32_t rt_numa_node_count(void) {
    /* macOS 不支持 NUMA：单一 node */
    return 1;
}

int32_t rt_numa_cpu_to_node(int32_t cpu) {
    /* 全部映射到 node 0 */
    (void)cpu;
    return 0;
}

void rt_numa_bind_worker(int32_t worker_id, int32_t node) {
    /* no-op：macOS 不支持 NUMA 绑定 */
    (void)worker_id;
    (void)node;
}

void* rt_numa_alloc_on_node(uint64_t size, int32_t node) {
    /* 走标准 malloc，忽略 node 参数 */
    (void)node;
    return malloc((size_t)size);
}

void rt_numa_free(void* ptr) {
    free(ptr);
}

#endif
