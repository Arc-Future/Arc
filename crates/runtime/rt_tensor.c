// RtTensor runtime implementation (RFC 021 Phase 1).
//
// Pure C, no Arc dependencies. Compiled by clang alongside runtime.c.
// Element type dispatched by elem_size (4 = float, 8 = double).

#include "rt_tensor.h"
#include <stdlib.h>
#include <string.h>

static RtTensor* alloc_tensor(int32_t rows, int32_t cols, int32_t elem_size) {
    RtTensor* t = (RtTensor*)malloc(sizeof(RtTensor));
    t->rank = 2;
    t->shape = (int32_t*)malloc(sizeof(int32_t) * 2);
    t->shape[0] = rows;
    t->shape[1] = cols;
    t->elem_size = elem_size;
    t->total = rows * cols;
    t->data = calloc((size_t)t->total, (size_t)elem_size);
    return t;
}

void* rt_tensor_create(int32_t rows, int32_t cols, int32_t elem_size) {
    return alloc_tensor(rows, cols, elem_size);
}

void rt_tensor_destroy(void* handle) {
    RtTensor* t = (RtTensor*)handle;
    if (!t) return;
    free(t->data);
    free(t->shape);
    free(t);
}

int32_t rt_tensor_rank(void* handle) {
    return ((RtTensor*)handle)->rank;
}

int32_t rt_tensor_rows(void* handle) {
    return ((RtTensor*)handle)->shape[0];
}

int32_t rt_tensor_cols(void* handle) {
    return ((RtTensor*)handle)->shape[1];
}

int32_t rt_tensor_total(void* handle) {
    return ((RtTensor*)handle)->total;
}

void rt_tensor_get(void* handle, int32_t i, int32_t j, void* out_ptr) {
    RtTensor* t = (RtTensor*)handle;
    int32_t idx = i * t->shape[1] + j;
    memcpy(out_ptr, (char*)t->data + (size_t)idx * t->elem_size, (size_t)t->elem_size);
}

void rt_tensor_set(void* handle, int32_t i, int32_t j, const void* elem_ptr) {
    RtTensor* t = (RtTensor*)handle;
    int32_t idx = i * t->shape[1] + j;
    memcpy((char*)t->data + (size_t)idx * t->elem_size, elem_ptr, (size_t)t->elem_size);
}

// Element-wise binary op macro: dispatch float vs double by elem_size.
#define ELEMENTWISE(op_name, op)                                        \
    void* rt_tensor_##op_name(void* a, void* b) {                       \
        RtTensor* ta = (RtTensor*)a;                                    \
        RtTensor* tb = (RtTensor*)b;                                    \
        RtTensor* tc = alloc_tensor(ta->shape[0], ta->shape[1], ta->elem_size); \
        int32_t n = ta->total;                                          \
        if (ta->elem_size == 4) {                                       \
            float* da = (float*)ta->data;                               \
            float* db = (float*)tb->data;                               \
            float* dc = (float*)tc->data;                               \
            for (int32_t i = 0; i < n; i++) dc[i] = da[i] op db[i];     \
        } else {                                                        \
            double* da = (double*)ta->data;                             \
            double* db = (double*)tb->data;                             \
            double* dc = (double*)tc->data;                             \
            for (int32_t i = 0; i < n; i++) dc[i] = da[i] op db[i];     \
        }                                                               \
        return tc;                                                      \
    }

ELEMENTWISE(add, +)
ELEMENTWISE(sub, -)
ELEMENTWISE(mul, *)

void* rt_tensor_matmul(void* a, void* b) {
    RtTensor* ta = (RtTensor*)a;
    RtTensor* tb = (RtTensor*)b;
    int32_t M = ta->shape[0];
    int32_t K = ta->shape[1];
    int32_t N = tb->shape[1];
    RtTensor* tc = alloc_tensor(M, N, ta->elem_size);
    if (ta->elem_size == 4) {
        float* da = (float*)ta->data;
        float* db = (float*)tb->data;
        float* dc = (float*)tc->data;
        for (int32_t i = 0; i < M; i++) {
            for (int32_t j = 0; j < N; j++) {
                float sum = 0.0f;
                for (int32_t k = 0; k < K; k++) {
                    sum += da[i * K + k] * db[k * N + j];
                }
                dc[i * N + j] = sum;
            }
        }
    } else {
        double* da = (double*)ta->data;
        double* db = (double*)tb->data;
        double* dc = (double*)tc->data;
        for (int32_t i = 0; i < M; i++) {
            for (int32_t j = 0; j < N; j++) {
                double sum = 0.0;
                for (int32_t k = 0; k < K; k++) {
                    sum += da[i * K + k] * db[k * N + j];
                }
                dc[i * N + j] = sum;
            }
        }
    }
    return tc;
}
