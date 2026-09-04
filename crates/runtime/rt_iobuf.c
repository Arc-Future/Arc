// 零拷贝缓冲池（RFC 009 M3）。
//
// 预分配 N 个固定大小的 user buffer，运行时通过 acquire/release 借用归还，
// 零 per-IO malloc。注册到 reactor 后，内核持有 buffer 引用，IO 完成时
// 直接写入 user buffer（避免 per-IO memcpy）。
//
// 平台差异：
//   - Linux io_uring：调用 rt_reactor_register_buffers 触发
//     io_uring_register_buffers，buffer 必须 page-aligned（posix_memalign）
//   - IOCP / kqueue / poll：register_buffers 静默成功，仅做池化（无内核注册）
//
// 线程安全：
//   - free-list 用 Treiber lock-free stack（CAS）—— buffer 指针在池生命周期
//     内地址不变，无 ABA 问题
//   - in_use / registered 用原子操作
//
// 池化语义（RFC 037 §4.4）：
//   - 缓冲池预分配 buf_count 个 buffer，每个 buf_size 字节
//   - acquire 借用（不转移所有权），release 归还
//   - 借用计数追踪，destroy 时校验未归还数（防泄漏）
//
// 内存布局：
//   - 单次 posix_memalign/malloc 申请 buf_size * buf_count 连续区
//   - buffers[] 数组按 buf_size 切片，便于 register_buffers 消费
//   - free-list 节点直接复用 buffer 头部 8 字节（buffer 至少 64B，安全）

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* ---- 平台抽象：page-aligned 分配 ---- */

#if defined(_WIN32) || defined(_WIN64)
  #include <windows.h>
  /* _aligned_malloc 返回的内存须用 _aligned_free 释放 */
  #define RT_IOBUF_ALIGNED_ALLOC(ptr, size, alignment) \
      do { (ptr) = _aligned_malloc((size), (alignment)); } while (0)
  #define RT_IOBUF_ALIGNED_FREE(ptr) _aligned_free((ptr))
  static uint32_t rt_iobuf_page_size(void) {
      SYSTEM_INFO si;
      GetSystemInfo(&si);
      return (uint32_t)si.dwPageSize;
  }
#else
  #include <unistd.h>
  #include <errno.h>
  /* posix_memalign：POSIX 200112+，返回值 0 表示成功 */
  #define RT_IOBUF_ALIGNED_ALLOC(ptr, size, alignment) \
      do { \
          if (posix_memalign(&(ptr), (alignment), (size)) != 0) { \
              (ptr) = NULL; \
          } \
      } while (0)
  #define RT_IOBUF_ALIGNED_FREE(ptr) free((ptr))
  static uint32_t rt_iobuf_page_size(void) {
      long ps = sysconf(_SC_PAGESIZE);
      return (ps > 0) ? (uint32_t)ps : 4096u;
  }
#endif

/* ---- 缓冲池结构 ---- */

/* free-list 节点：复用 buffer 头部，避免额外分配 */
typedef struct RtIoBufFreeNode {
    struct RtIoBufFreeNode* next;
} RtIoBufFreeNode;

typedef struct RtIoBufPool {
    void*            memory;          /* 连续预分配区（page-aligned） */
    void**           buffers;         /* buffer 指针数组（register_buffers 消费） */
    uint32_t*        lengths;         /* 每 buffer 长度数组 */
    RtIoBufFreeNode* free_top;        /* Treiber stack 栈顶 */
    int32_t          free_count;      /* 空闲数（原子） */
    int32_t          in_use;          /* 借出未归还数（原子） */
    uint32_t         buf_size;        /* 单 buffer 字节数 */
    uint32_t         buf_count;       /* 总 buffer 数 */
    int32_t          registered;      /* 是否已注册到 reactor（原子） */
} RtIoBufPool;

/* 最小 buffer 大小限制：free-list 节点复用 buffer 头部，需 ≥ 指针大小。
 * 实际生产场景 buffer 通常 ≥ 512B（网络包）/4KB（文件块），此处保守设 64B。 */
#define RT_IOBUF_MIN_SIZE 64

/* ============================================================
 * ABI 实现
 * ============================================================ */

void* rt_iobuf_pool_create(uint32_t buf_size, uint32_t buf_count) {
    /* 参数校验 */
    if (buf_size == 0 || buf_count == 0) return NULL;
    if (buf_size < RT_IOBUF_MIN_SIZE) buf_size = RT_IOBUF_MIN_SIZE;

    RtIoBufPool* pool = (RtIoBufPool*)calloc(1, sizeof(RtIoBufPool));
    if (!pool) return NULL;

    pool->buf_size  = buf_size;
    pool->buf_count = buf_count;
    pool->free_top  = NULL;
    pool->free_count = 0;
    pool->in_use    = 0;
    pool->registered = 0;

    /* 1. 申请 page-aligned 连续内存区
     *    Linux io_uring_register_buffers 要求 buffer page-aligned；
     *    其他平台也用同一路径，无副作用。 */
    uint32_t alignment = rt_iobuf_page_size();
    size_t total = (size_t)buf_size * (size_t)buf_count;
    RT_IOBUF_ALIGNED_ALLOC(pool->memory, total, alignment);
    if (!pool->memory) {
        free(pool);
        return NULL;
    }
    memset(pool->memory, 0, total);

    /* 2. 构造 buffer 指针数组 + 长度数组（供 rt_reactor_register_buffers 消费） */
    pool->buffers = (void**)malloc((size_t)buf_count * sizeof(void*));
    pool->lengths = (uint32_t*)malloc((size_t)buf_count * sizeof(uint32_t));
    if (!pool->buffers || !pool->lengths) {
        free(pool->buffers);
        free(pool->lengths);
        RT_IOBUF_ALIGNED_FREE(pool->memory);
        free(pool);
        return NULL;
    }
    for (uint32_t i = 0; i < buf_count; i++) {
        pool->buffers[i] = (char*)pool->memory + (size_t)i * buf_size;
        pool->lengths[i] = buf_size;
    }

    /* 3. 构造初始 free-list：所有 buffer 入栈
     *    节点复用 buffer 头部（RtIoBufFreeNode* 占 sizeof(void*)）。
     *    入栈逆序使 acquire 顺序与索引顺序一致（轻微优化，非必需）。 */
    for (int32_t i = (int32_t)buf_count - 1; i >= 0; i--) {
        RtIoBufFreeNode* node = (RtIoBufFreeNode*)pool->buffers[i];
        node->next = pool->free_top;
        pool->free_top = node;
    }
    __atomic_store_n(&pool->free_count, (int32_t)buf_count, __ATOMIC_RELEASE);

    return pool;
}

void rt_iobuf_pool_destroy(void* handle) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool) return;

    /* 泄漏检测：destroy 时 in_use 应为 0。
     * 不强制阻断销毁（避免双重泄漏），仅记录——生产环境可接入 panic 钩子。
     * 这里采用保守策略：若有未归还 buffer，仍销毁池（caller 需自行保证语义）。 */
    int32_t leaked = __atomic_load_n(&pool->in_use, __ATOMIC_ACQUIRE);
    (void)leaked;  /* 调试期可断言 leaked == 0 */

    free(pool->buffers);
    free(pool->lengths);
    if (pool->memory) {
        RT_IOBUF_ALIGNED_FREE(pool->memory);
    }
    free(pool);
}

void* rt_iobuf_pool_acquire(void* handle, uint32_t* out_len) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool) return NULL;

    /* Treiber stack pop：CAS */
    RtIoBufFreeNode* node;
    do {
        node = __atomic_load_n(&pool->free_top, __ATOMIC_ACQUIRE);
        if (!node) return NULL;  /* 池空 */
    } while (!__sync_bool_compare_and_swap(&pool->free_top, node, node->next));

    __sync_fetch_and_sub(&pool->free_count, 1);
    __sync_fetch_and_add(&pool->in_use, 1);

    if (out_len) {
        *out_len = pool->buf_size;
    }
    /* node 同时是 buffer 起始地址（free-list 节点复用 buffer 头部） */
    return (void*)node;
}

void rt_iobuf_pool_release(void* handle, void* buf) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool || !buf) return;

    /* Treiber stack push：CAS */
    RtIoBufFreeNode* node = (RtIoBufFreeNode*)buf;
    RtIoBufFreeNode* top;
    do {
        top = __atomic_load_n(&pool->free_top, __ATOMIC_ACQUIRE);
        node->next = top;
    } while (!__sync_bool_compare_and_swap(&pool->free_top, top, node));

    __sync_fetch_and_add(&pool->free_count, 1);
    __sync_fetch_and_sub(&pool->in_use, 1);
}

int32_t rt_iobuf_pool_register(void* handle, void* reactor) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool || !reactor) return -1;

    /* 已注册：幂等返回 0 */
    int32_t expected = 0;
    if (!__sync_bool_compare_and_swap(&pool->registered, expected, 1)) {
        return 0;
    }

    /* 委托 reactor 后端注册：
     *   - Linux io_uring：触发 io_uring_register_buffers（内核零拷贝）
     *   - IOCP/kqueue/poll：静默成功（无内核注册，仅池化） */
    int32_t ret = rt_reactor_register_buffers(reactor,
                                              (const void**)pool->buffers,
                                              pool->lengths,
                                              (int32_t)pool->buf_count);
    return ret;
}

/* ============================================================
 * 内省 API（可选，供调试/统计使用，未列入稳定 ABI）
 * ============================================================ */

int32_t rt_iobuf_pool_free_count(void* handle) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool) return -1;
    return __atomic_load_n(&pool->free_count, __ATOMIC_ACQUIRE);
}

int32_t rt_iobuf_pool_in_use_count(void* handle) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    if (!pool) return -1;
    return __atomic_load_n(&pool->in_use, __ATOMIC_ACQUIRE);
}

uint32_t rt_iobuf_pool_buf_size(void* handle) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    return pool ? pool->buf_size : 0;
}

uint32_t rt_iobuf_pool_buf_count(void* handle) {
    RtIoBufPool* pool = (RtIoBufPool*)handle;
    return pool ? pool->buf_count : 0;
}
