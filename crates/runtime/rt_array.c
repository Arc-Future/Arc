// Runtime-length array ABI (RFC 015 Phase B · RFC 052 ArcHeader 化).
//
// Layout (RFC 052 §3)：
//   ArcHeader{rc@0, weak@4, vtable@8} + length@16 + elem_size@20 + payload@24
//
// 公开返回值仍是 *payload* 指针（与既有 GEP / C 消费方一致）：
//   length/elem_size 仍在 payload-8（与旧 8B 头同位）；
//   ArcHeader 在 payload-24；rt_array_retain/release 经 -24 走 rt_arc_inc/dec。
//
// vtable：
//   slot0 = typeinfo 占位（NULL：数组无 RtTypeInfo 全局）
//   slot1 = finalizer（仅 walk 元素；free 由 rt_arc_dec 完成）
//   slot2 = walker（循环收集；标量/string NULL；class 元素走 refs walker）

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* finalize_nested 在定义前调用 release。 */
void rt_array_release(void* payload);

enum {
    RT_ARRAY_ARC_BYTES = 16,
    RT_ARRAY_LEN_BYTES = 8,
    RT_ARRAY_PAYLOAD_OFF = 24, /* ArcHeader(16) + length(4) + elem_size(4) */
};

typedef struct {
    int32_t length;
    int32_t elem_size;
} RtArrayLenHeader;

typedef struct {
    _Atomic int32_t refcount;
    _Atomic int32_t weakcount;
    const void* vtable;
    int32_t length;
    int32_t elem_size;
    /* payload follows at +24 */
} RtArrayObject;

static RtArrayLenHeader* rt_array_header(void* payload) {
    if (!payload) return NULL;
    return (RtArrayLenHeader*)((char*)payload - RT_ARRAY_LEN_BYTES);
}

static RtArrayObject* rt_array_obj(void* payload) {
    if (!payload) return NULL;
    return (RtArrayObject*)((char*)payload - RT_ARRAY_PAYLOAD_OFF);
}

void* rt_array_payload_of(void* obj) {
    if (!obj) return NULL;
    return (char*)obj + RT_ARRAY_PAYLOAD_OFF;
}

void* rt_array_obj_of(void* payload) {
    return (void*)rt_array_obj(payload);
}

/* ---- finalizers（不 free；rt_arc_dec 负责释放对象块）---- */

static void rt_array_finalize_scalar(void* obj) {
    (void)obj;
}

static void rt_array_finalize_refs(void* obj) {
    RtArrayObject* a = (RtArrayObject*)obj;
    if (!a || a->elem_size != (int32_t)sizeof(void*)) return;
    void** items = (void**)((char*)obj + RT_ARRAY_PAYLOAD_OFF);
    for (int32_t i = 0; i < a->length; i++) {
        void* e = items[i];
        items[i] = NULL;
        rt_arc_dec(e);
    }
}

static void rt_array_finalize_nested(void* obj) {
    /* 元素为嵌套数组 payload 指针 → rt_array_release（非 rt_arc_dec）。 */
    RtArrayObject* a = (RtArrayObject*)obj;
    if (!a || a->elem_size != (int32_t)sizeof(void*)) return;
    void** items = (void**)((char*)obj + RT_ARRAY_PAYLOAD_OFF);
    for (int32_t i = 0; i < a->length; i++) {
        void* e = items[i];
        items[i] = NULL;
        rt_array_release(e);
    }
}

static void rt_array_walk_refs(void* obj, void (*visit)(void* ctx, void* field), void* ctx) {
    RtArrayObject* a = (RtArrayObject*)obj;
    if (!a || !visit || a->elem_size != (int32_t)sizeof(void*)) return;
    void** items = (void**)((char*)obj + RT_ARRAY_PAYLOAD_OFF);
    for (int32_t i = 0; i < a->length; i++) {
        if (items[i]) visit(ctx, items[i]);
    }
}

static const void* const __arc_array_scalar_vtable[] = {
    NULL,
    rt_array_finalize_scalar,
    NULL,
};

static const void* const __arc_array_refs_vtable[] = {
    NULL,
    rt_array_finalize_refs,
    rt_array_walk_refs,
};

static const void* const __arc_array_nested_vtable[] = {
    NULL,
    rt_array_finalize_nested,
    NULL, /* 嵌套数组环由内层 class walker 承载 */
};

const void* rt_array_vtable_scalar(void) { return __arc_array_scalar_vtable; }
const void* rt_array_vtable_refs(void) { return __arc_array_refs_vtable; }
const void* rt_array_vtable_nested(void) { return __arc_array_nested_vtable; }

static void* rt_array_create_with_vtable(int32_t cap, int32_t elem_size, const void* vtable) {
    if (cap < 0 || elem_size <= 0) {
        rt_panic("rt_array_create: invalid cap or elem_size");
    }
    size_t bytes = (size_t)RT_ARRAY_PAYLOAD_OFF + (size_t)cap * (size_t)elem_size;
    RtArrayObject* obj = (RtArrayObject*)calloc(1, bytes);
    if (!obj) {
        rt_panic("oom");
    }
    atomic_init(&obj->refcount, 1);
    atomic_init(&obj->weakcount, 0);
    obj->vtable = vtable ? vtable : __arc_array_scalar_vtable;
    obj->length = cap;
    obj->elem_size = elem_size;
    return (char*)obj + RT_ARRAY_PAYLOAD_OFF;
}

void* rt_array_create(int32_t cap, int32_t elem_size) {
    return rt_array_create_with_vtable(cap, elem_size, __arc_array_scalar_vtable);
}

void* rt_array_create_refs(int32_t cap, int32_t elem_size) {
    return rt_array_create_with_vtable(cap, elem_size, __arc_array_refs_vtable);
}

void* rt_array_create_nested(int32_t cap, int32_t elem_size) {
    return rt_array_create_with_vtable(cap, elem_size, __arc_array_nested_vtable);
}

void rt_array_retain(void* payload) {
    void* obj = rt_array_obj(payload);
    if (obj) rt_arc_inc(obj);
}

void rt_array_release(void* payload) {
    void* obj = rt_array_obj(payload);
    if (obj) rt_arc_dec(obj);
}

/* List 槽回调：slot 存 payload 指针（非 ArcHeader）。 */
void rt_array_arc_inc_ref(void* slot) {
    if (!slot) return;
    rt_array_retain(*(void**)slot);
}

void rt_array_arc_dec_ref(void* slot) {
    if (!slot) return;
    rt_array_release(*(void**)slot);
}

int32_t rt_array_length(void* payload) {
    RtArrayLenHeader* h = rt_array_header(payload);
    return h ? h->length : 0;
}

void rt_array_destroy(void* payload) {
    /* 兼容旧调用点：等价于 release（rc 1→0 → finalizer + free）。 */
    rt_array_release(payload);
}

// ---- P5-F: Array utility methods ----

void rt_array_copy(void* src, int32_t src_offset,
                   void* dst, int32_t dst_offset,
                   int32_t length) {
    RtArrayLenHeader* sh = rt_array_header(src);
    RtArrayLenHeader* dh = rt_array_header(dst);
    if (!sh || !dh) { rt_panic("rt_array_copy: null array"); }
    int32_t elem_size = sh->elem_size;
    if (src_offset < 0 || dst_offset < 0 || length < 0) { rt_panic("rt_array_copy: negative param"); }
    if (src_offset + length > sh->length) { rt_panic("rt_array_copy: src out of bounds"); }
    if (dst_offset + length > dh->length) { rt_panic("rt_array_copy: dst out of bounds"); }
    memmove((char*)dst + dst_offset * elem_size,
            (char*)src + src_offset * elem_size,
            (size_t)length * (size_t)elem_size);
}

void rt_array_clear(void* payload, int32_t offset, int32_t length) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h) { rt_panic("rt_array_clear: null array"); }
    if (offset < 0 || length < 0) { rt_panic("rt_array_clear: negative param"); }
    if (offset + length > h->length) { rt_panic("rt_array_clear: out of bounds"); }
    memset((char*)payload + offset * h->elem_size, 0,
           (size_t)length * (size_t)h->elem_size);
}

void rt_array_reverse(void* payload) {
    RtArrayLenHeader* h = rt_array_header(payload);
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

int32_t rt_array_index_of_int(void* payload, int32_t value) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h) return -1;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = 0; i < h->length; i++) {
        if (data[i] == value) return i;
    }
    return -1;
}

int32_t rt_array_last_index_of_int(void* payload, int32_t value) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h) return -1;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (data[i] == value) return i;
    }
    return -1;
}

void rt_array_resize(void** slot, int32_t new_size) {
    if (!slot) { rt_panic("rt_array_resize: null slot"); }
    if (new_size < 0) { rt_panic("rt_array_resize: negative size"); }
    void* old = *slot;
    int32_t elem_size;
    int32_t old_len = 0;
    const void* vtable = __arc_array_scalar_vtable;
    if (old) {
        RtArrayObject* obj = rt_array_obj(old);
        RtArrayLenHeader* h = rt_array_header(old);
        if (!h || !obj) { rt_panic("rt_array_resize: corrupt array"); }
        elem_size = h->elem_size;
        old_len = h->length;
        vtable = obj->vtable;
        if (old_len == new_size) return;
    } else {
        elem_size = (int32_t)sizeof(int32_t);
    }
    void* neu = rt_array_create_with_vtable(new_size, elem_size, vtable);
    int32_t n = old_len < new_size ? old_len : new_size;
    if (old && n > 0) {
        memcpy(neu, old, (size_t)n * (size_t)elem_size);
        /* refs/nested：拷贝后对转移元素 retain，旧数组 release 时 finalizer 会 dec。 */
        if (vtable == __arc_array_refs_vtable || vtable == __arc_array_nested_vtable) {
            void** items = (void**)neu;
            for (int32_t i = 0; i < n; i++) {
                if (vtable == __arc_array_refs_vtable) {
                    rt_arc_inc(items[i]);
                } else {
                    rt_array_retain(items[i]);
                }
            }
        }
    }
    if (old) {
        rt_array_release(old);
    }
    *slot = neu;
}

int32_t rt_array_exists(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(buf + (size_t)i * (size_t)es)) return 1;
    }
    return 0;
}

int32_t rt_array_find_int(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(&data[i])) return data[i];
    }
    return 0;
}

int32_t rt_array_find_last_int(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !pred) return 0;
    int32_t* data = (int32_t*)payload;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (pred(&data[i])) return data[i];
    }
    return 0;
}

int32_t rt_array_find_index(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !pred) return -1;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        if (pred(buf + (size_t)i * (size_t)es)) return i;
    }
    return -1;
}

int32_t rt_array_find_last_index(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !pred) return -1;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = h->length - 1; i >= 0; i--) {
        if (pred(buf + (size_t)i * (size_t)es)) return i;
    }
    return -1;
}

int32_t rt_array_true_for_all(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
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
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || !action) return;
    char* buf = (char*)payload;
    int32_t es = h->elem_size;
    for (int32_t i = 0; i < h->length; i++) {
        action(buf + (size_t)i * (size_t)es);
    }
}

static int rt_array_cmp_int(const void* a, const void* b) {
    int32_t xa = *(const int32_t*)a;
    int32_t xb = *(const int32_t*)b;
    return (xa > xb) - (xa < xb);
}

void rt_array_sort_int(void* payload) {
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || h->length <= 1) return;
    qsort(payload, (size_t)h->length, sizeof(int32_t), rt_array_cmp_int);
}

void* rt_array_find_all_int(void* payload, rt_list_pred_fn pred) {
    RtArrayLenHeader* h = rt_array_header(payload);
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
    RtArrayLenHeader* h = rt_array_header(payload);
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
    RtArrayLenHeader* h = rt_array_header(payload);
    if (!h || h->length == 0) return -1;
    int32_t* data = (int32_t*)payload;
    int32_t lo = 0;
    int32_t hi = h->length - 1;
    while (lo <= hi) {
        int32_t mid = lo + ((hi - lo) >> 1);
        if (data[mid] < value) lo = mid + 1;
        else if (data[mid] > value) hi = mid - 1;
        else return mid;
    }
    return ~lo;
}
