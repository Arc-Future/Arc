// Runtime-length array ABI (RFC 015 Phase B).
//
// Closes the gap noted in the foundation audit (M5): the previous runtime
// only supported compile-time constant-length arrays. `rt_array_create`
// allocates a heap array with a runtime-determined element count, returning
// a pointer the codegen can index directly. The array carries its length
// in a header so bounds checks and `Length` queries work without a separate
// side table.
//
// Layout:
//   [0..4)   int32_t length    (element count)
//   [4..8)   int32_t elem_size (for safe memcpy in `rt_array_clone`)
//   [8..)    T payload[length]
//
// The returned pointer is to the *payload* (offset 8), so generated
// `getelementptr` indexing works the same as for compile-time arrays.
// `rt_array_destroy` accepts the payload pointer and backs up to the header
// before freeing.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct {
    int32_t length;
    int32_t elem_size;
    /* payload follows */
} RtArrayHeader;

static RtArrayHeader* rt_array_header(void* payload) {
    if (!payload) return NULL;
    return (RtArrayHeader*)((char*)payload - sizeof(RtArrayHeader));
}

void* rt_array_create(int32_t cap, int32_t elem_size) {
    if (cap < 0 || elem_size <= 0) {
        rt_panic("rt_array_create: invalid cap or elem_size");
    }
    size_t header = sizeof(RtArrayHeader);
    size_t bytes = header + (size_t)cap * (size_t)elem_size;
    RtArrayHeader* h = (RtArrayHeader*)malloc(bytes);
    if (!h) {
        rt_panic("oom");
    }
    h->length = cap;
    h->elem_size = elem_size;
    memset((char*)h + header, 0, (size_t)cap * (size_t)elem_size);
    return (char*)h + header;
}

int32_t rt_array_length(void* payload) {
    RtArrayHeader* h = rt_array_header(payload);
    return h ? h->length : 0;
}

void rt_array_destroy(void* payload) {
    RtArrayHeader* h = rt_array_header(payload);
    if (h) {
        free(h);
    }
}

// ---- P5-F: Array utility methods ----

// Copy `length` elements from src[srcOffset] to dst[dstOffset].
// Safe: uses element size from header for memmove, no overflow on overlap.
void rt_array_copy(void* src, int32_t src_offset,
                   void* dst, int32_t dst_offset,
                   int32_t length) {
    RtArrayHeader* sh = rt_array_header(src);
    RtArrayHeader* dh = rt_array_header(dst);
    if (!sh || !dh) { rt_panic("rt_array_copy: null array"); }
    int32_t elem_size = sh->elem_size;
    if (src_offset < 0 || dst_offset < 0 || length < 0) { rt_panic("rt_array_copy: negative param"); }
    if (src_offset + length > sh->length) { rt_panic("rt_array_copy: src out of bounds"); }
    if (dst_offset + length > dh->length) { rt_panic("rt_array_copy: dst out of bounds"); }
    memmove((char*)dst + dst_offset * elem_size,
            (char*)src + src_offset * elem_size,
            (size_t)length * (size_t)elem_size);
}

// Zero out `length` elements starting at `offset`.
void rt_array_clear(void* payload, int32_t offset, int32_t length) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h) { rt_panic("rt_array_clear: null array"); }
    if (offset < 0 || length < 0) { rt_panic("rt_array_clear: negative param"); }
    if (offset + length > h->length) { rt_panic("rt_array_clear: out of bounds"); }
    memset((char*)payload + offset * h->elem_size, 0,
           (size_t)length * (size_t)h->elem_size);
}

// Reverse elements in place (by element size).
void rt_array_reverse(void* payload) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h) { rt_panic("rt_array_reverse: null array"); }
    int32_t len = h->length;
    int32_t es = h->elem_size;
    if (len <= 1) return;
    char* buf = (char*)payload;
    char* tmp = (char*)malloc(es);
    if (!tmp) { rt_panic("oom"); }
    for (int32_t i = 0; i < len / 2; i++) {
        memcpy(tmp, buf + i * es, es);
        memcpy(buf + i * es, buf + (len - 1 - i) * es, es);
        memcpy(buf + (len - 1 - i) * es, tmp, es);
    }
    free(tmp);
}

// IndexOf for int arrays (most common use case).
int32_t rt_array_index_of_int(void* payload, int32_t value) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h) return -1;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = 0; i < h->length; i++) {
        if (data[i] == value) return i;
    }
    return -1;
}

int32_t rt_array_last_index_of_int(void* payload, int32_t value) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h) return -1;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (data[i] == value) return i;
    }
    return -1;
}

// Resize via slot (C# Array.Resize(ref T[], newSize)).
// Null *slot: allocate int32 elements (Stable int[] surface).
// Non-null: reuse header.elem_size; destroy old payload after copy.
void rt_array_resize(void** slot, int32_t new_size) {
    if (!slot) { rt_panic("rt_array_resize: null slot"); }
    if (new_size < 0) { rt_panic("rt_array_resize: negative size"); }
    void* old = *slot;
    int32_t elem_size;
    int32_t old_len = 0;
    if (old) {
        RtArrayHeader* h = rt_array_header(old);
        if (!h) { rt_panic("rt_array_resize: corrupt array"); }
        elem_size = h->elem_size;
        old_len = h->length;
        if (old_len == new_size) return;
    } else {
        elem_size = (int32_t)sizeof(int32_t);
    }
    void* neu = rt_array_create(new_size, elem_size);
    int32_t n = old_len < new_size ? old_len : new_size;
    if (old && n > 0) {
        memcpy(neu, old, (size_t)n * (size_t)elem_size);
    }
    if (old) {
        rt_array_destroy(old);
    }
    *slot = neu;
}

/* Predicate surface (int[] Stable; same pred ABI as rt_list_pred_fn). */

int32_t rt_array_exists(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(buf + (size_t)i * (size_t)es)) return 1;
    }
    return 0;
}

int32_t rt_array_find_int(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(&data[i])) return data[i];
    }
    return 0;
}

int32_t rt_array_find_last_int(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (pred(&data[i])) return data[i];
    }
    return 0;
}

int32_t rt_array_find_index(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return -1;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(buf + (size_t)i * (size_t)es)) return i;
    }
    return -1;
}

int32_t rt_array_find_last_index(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return -1;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (pred(buf + (size_t)i * (size_t)es)) return i;
    }
    return -1;
}

int32_t rt_array_true_for_all(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!pred) return 0;
    if (!h) return 1;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        if (!pred(buf + (size_t)i * (size_t)es)) return 0;
    }
    return 1;
}

void rt_array_for_each(void* payload, rt_list_pred_fn action) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !action) return;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        action(buf + (size_t)i * (size_t)es);
    }
}

static int rt_int_compare(const void* a, const void* b) {
    int32_t x = *(const int32_t*)a;
    int32_t y = *(const int32_t*)b;
    if (x < y) return -1;
    if (x > y) return 1;
    return 0;
}

void rt_array_sort_int(void* payload) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h) { rt_panic("rt_array_sort_int: null array"); }
    if (h->length <= 1) return;
    qsort(payload, (size_t)h->length, sizeof(int32_t), rt_int_compare);
}

void* rt_array_find_all_int(void* payload, rt_list_pred_fn pred) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !pred) return rt_array_create(0, (int32_t)sizeof(int32_t));
    int32_t* data = (int32_t*)payload;
    int32_t count = 0;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(&data[i])) count++;
    }
    void* out = rt_array_create(count, (int32_t)sizeof(int32_t));
    int32_t* od = (int32_t*)out;
    int32_t j = 0;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(&data[i])) od[j++] = data[i];
    }
    return out;
}

void* rt_array_convert_all_int(void* payload, rt_list_pred_fn converter) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || !converter) return rt_array_create(0, (int32_t)sizeof(int32_t));
    int32_t* data = (int32_t*)payload;
    void* out = rt_array_create(h->length, (int32_t)sizeof(int32_t));
    int32_t* od = (int32_t*)out;
    for (int32_t i = 0; i < h->length; i++) {
        od[i] = converter(&data[i]);
    }
    return out;
}

int32_t rt_array_binary_search_int(void* payload, int32_t value) {
    RtArrayHeader* h = rt_array_header(payload);
    if (!h || h->length == 0) return -1;
    int32_t* data = (int32_t*)payload;
    int32_t lo = 0;
    int32_t hi = h->length - 1;
    while (lo <= hi) {
        int32_t mid = lo + ((hi - lo) >> 1);
        // 用比较而非减法求序：全范围 i32（如 FNV-1a type_id）异号相减会回绕，
        // 中点被误判为"小于目标"导致查找偏向右侧而漏命中（DI 扁平查找实测）。
        if (data[mid] < value) lo = mid + 1;
        else if (data[mid] > value) hi = mid - 1;
        else return mid;
    }
    return ~lo;
}

