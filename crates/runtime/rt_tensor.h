// RtTensor: runtime representation of Arc Tensor<T> (RFC 021 Phase 1).
//
// All Arc Tensor<T> instances hold an opaque `_handle` (i32) pointing to
// an RtTensor struct allocated on the heap. Element type is distinguished
// by `elem_size` (4 = float, 8 = double).

#ifndef RT_TENSOR_H
#define RT_TENSOR_H

#include <stdint.h>

typedef struct RtTensor {
    void*    data;        // contiguous row-major element buffer
    int32_t  rank;         // dimension count (2 for matrices)
    int32_t* shape;        // shape array (length = rank)
    int32_t  elem_size;    // sizeof(element): 4 = float, 8 = double
    int32_t  total;        // element count = product of shape
} RtTensor;

// Lifecycle
void* rt_tensor_create(int32_t rows, int32_t cols, int32_t elem_size);
void  rt_tensor_destroy(void* handle);

// Shape queries
int32_t rt_tensor_rank(void* handle);
int32_t rt_tensor_rows(void* handle);
int32_t rt_tensor_cols(void* handle);
int32_t rt_tensor_total(void* handle);

// Element access (2D indexing)
void rt_tensor_get(void* handle, int32_t i, int32_t j, void* out_ptr);
void rt_tensor_set(void* handle, int32_t i, int32_t j, const void* elem_ptr);

// Element-wise arithmetic (Hadamard). Returns new tensor; inputs unchanged.
void* rt_tensor_add(void* a, void* b);
void* rt_tensor_sub(void* a, void* b);
void* rt_tensor_mul(void* a, void* b);

// Matrix multiplication (rank=2). a: M×K, b: K×N → result: M×N.
void* rt_tensor_matmul(void* a, void* b);

#endif // RT_TENSOR_H
