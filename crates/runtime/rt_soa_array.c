// SoA (Structure of Arrays) 数据布局 ABI 实现（RFC 009 M4）。
//
// 为 [SoA] struct 数组提供独立字段数组的内存布局，消除 SIMD gather 浪费，
// 提升 cache 局部性。配合 codegen 的 GEP 重排 + LLVM auto-vectorize 实现
// SoA vs AoS 20× 突破（RFC 009 §0.2 性能指标矩阵）。
//
// ## 内存布局
//
//   rt_soa_array 描述符：
//     { int32_t length; int32_t num_fields; void** field_arrays; }
//
//   field_arrays[f] 指向第 f 个字段的连续数组（length × field_size[f] 字节），
//   每个 field_array 起始地址对齐到 64 字节（cache-line 对齐）。
//
// ## 多平台说明
//
// 平台无关的 malloc/free + memset；无 SIMD/线程/系统调用依赖。
// 性能收益由 codegen GEP 重排实现（平台相关：x86-64 AVX2/AVX-512、AArch64 NEON）。
//
// ## 设计要点
//
// - 字段数组按 typeck StructLayout.fields 声明顺序排列
// - field_sizes[] 由 codegen 按字段类型大小填充（double=8, int=4, ptr=8, ...）
// - 64 字节 cache-line 对齐（aligned_alloc / _aligned_malloc）
// - 释放时一次 rt_soa_free 内部遍历释放所有字段数组 + 描述符

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

/* cache-line 对齐分配：跨平台抽象 */
static void* rt_soa_aligned_alloc(size_t size) {
#ifdef _WIN32
    return _aligned_malloc(size, 64);
#else
    /* aligned_alloc 要求 size 是 alignment 的整数倍；向上取整到 64 字节 */
    size_t aligned_size = (size + 63) & ~(size_t)63;
    return aligned_alloc(64, aligned_size);
#endif
}

static void rt_soa_aligned_free(void* ptr) {
#ifdef _WIN32
    _aligned_free(ptr);
#else
    free(ptr);
#endif
}

rt_soa_array* rt_soa_array_create(int32_t length, int32_t num_fields, const int32_t* field_sizes) {
    if (length < 0 || num_fields <= 0 || field_sizes == NULL) {
        return NULL;
    }

    /* 分配描述符 */
    rt_soa_array* arr = (rt_soa_array*)malloc(sizeof(rt_soa_array));
    if (arr == NULL) {
        return NULL;
    }
    arr->length = length;
    arr->num_fields = num_fields;

    /* 分配 field_arrays 指针数组 */
    arr->field_arrays = (void**)calloc((size_t)num_fields, sizeof(void*));
    if (arr->field_arrays == NULL) {
        free(arr);
        return NULL;
    }

    /* 为每个字段分配连续数组（cache-line 对齐，零初始化） */
    for (int32_t f = 0; f < num_fields; f++) {
        int32_t fsize = field_sizes[f];
        if (fsize <= 0) {
            /* 无效字段大小；置 NULL，后续 rt_soa_field_ptr 返回 NULL */
            arr->field_arrays[f] = NULL;
            continue;
        }
        size_t total = (size_t)length * (size_t)fsize;
        void* field_buf = rt_soa_aligned_alloc(total);
        if (field_buf == NULL) {
            /* 分配失败：回滚已分配的字段数组 */
            for (int32_t g = 0; g < f; g++) {
                rt_soa_aligned_free(arr->field_arrays[g]);
            }
            free(arr->field_arrays);
            free(arr);
            return NULL;
        }
        memset(field_buf, 0, total);
        arr->field_arrays[f] = field_buf;
    }

    return arr;
}

void* rt_soa_field_ptr(rt_soa_array* arr, int32_t field_idx) {
    if (arr == NULL || field_idx < 0 || field_idx >= arr->num_fields) {
        return NULL;
    }
    return arr->field_arrays[field_idx];
}

int32_t rt_soa_length(rt_soa_array* arr) {
    if (arr == NULL) {
        return 0;
    }
    return arr->length;
}

void rt_soa_free(rt_soa_array* arr) {
    if (arr == NULL) {
        return;
    }
    if (arr->field_arrays != NULL) {
        for (int32_t f = 0; f < arr->num_fields; f++) {
            if (arr->field_arrays[f] != NULL) {
                rt_soa_aligned_free(arr->field_arrays[f]);
            }
        }
        free(arr->field_arrays);
    }
    free(arr);
}
