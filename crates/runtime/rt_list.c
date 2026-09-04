// List<T> runtime ABI (RFC 007 Phase 1/2/3/4).
//
// Dynamic array with 2× growth strategy, no GC pauses, O(n) amortized append.
// Extracted from the former runtime.c. Element size is fixed at create time;
// equality is delegated to a caller-supplied function pointer (or `memcmp`
// when none is provided).
//
// Phase 4: element-level ARC maintenance for reference-type elements.
// `rt_list_create` receives `arc_inc`/`arc_dec` callbacks (NULL for value
// types). H1 UnitTest flaky AV：set/remove_at/clear/remove_all/remove_range
// **skip** arc_dec（宁漏勿崩）；push/insert 仍 arc_inc；destroy 仍批量 dec。
// reverse/sort/find_all/to_array/copy_to do not change ownership counts.

#include "rt_abi.h"
#include <stdlib.h>
#include <string.h>

typedef struct RtList {
    void*        data;
    int32_t      size;
    int32_t      capacity;
    int32_t      elem_size;
    rt_list_eq_fn eq;
    rt_list_arc_fn arc_inc;
    rt_list_arc_fn arc_dec;
} RtList;

int32_t rt_list_eq_str(const void* a, const void* b) {
    const char* const* pa = (const char* const*)a;
    const char* const* pb = (const char* const*)b;
    const char* sa = *pa ? *pa : "";
    const char* sb = *pb ? *pb : "";
    return strcmp(sa, sb) == 0 ? 1 : 0;
}

/* ---- Phase 4: built-in ARC slot callbacks for class-type elements ---- */

void rt_list_arc_inc_ref(void* slot) {
    if (!slot) return;
    void* obj = *(void**)slot;
    rt_arc_inc(obj);
}

void rt_list_arc_dec_ref(void* slot) {
    if (!slot) return;
    void* obj = *(void**)slot;
    rt_arc_dec(obj);
}

static int32_t rt_list_eq_default(RtList* list, const void* a, const void* b) {
    if (list->eq) {
        return list->eq(a, b);
    }
    return memcmp(a, b, (size_t)list->elem_size) == 0 ? 1 : 0;
}

static void rt_list_ensure_capacity_impl(RtList* list, int32_t needed) {
    if (needed <= list->capacity) {
        return;
    }
    /* 4B 值类型：8× grow + 首配至少 16（H2 List 续刀 · 公开 ABI 不变）。 */
    int32_t grow = (list->elem_size == 4) ? 8 : 2;
    int32_t new_cap = list->capacity > 0 ? list->capacity * grow : grow;
    if (list->elem_size == 4 && new_cap < 16) {
        new_cap = 16;
    } else if (new_cap < 8) {
        new_cap = 8;
    }
    if (new_cap < needed) {
        new_cap = needed;
    }
    void* new_data = realloc(list->data, (size_t)new_cap * (size_t)list->elem_size);
    if (!new_data) {
        rt_panic("oom");
    }
    list->data = new_data;
    list->capacity = new_cap;
}

/* RFC 005 knife B：冷路径扩容；值类型 Add 热路径由 codegen 直降（见 emit_list_add）。 */
void rt_list_ensure_capacity(void* handle, int32_t needed) {
    if (!handle) return;
    rt_list_ensure_capacity_impl((RtList*)handle, needed);
}

/* POD 小宽度直写，避免 memcpy 调用（仍可被 LTO 内联）。 */
static void rt_list_store_elem(RtList* list, void* slot, const void* elem_ptr) {
    switch (list->elem_size) {
    case 1:
        *(uint8_t*)slot = *(const uint8_t*)elem_ptr;
        break;
    case 2:
        *(uint16_t*)slot = *(const uint16_t*)elem_ptr;
        break;
    case 4:
        *(uint32_t*)slot = *(const uint32_t*)elem_ptr;
        break;
    case 8:
        *(uint64_t*)slot = *(const uint64_t*)elem_ptr;
        break;
    default:
        memcpy(slot, elem_ptr, (size_t)list->elem_size);
        break;
    }
}

static void rt_list_load_elem(RtList* list, void* out_ptr, const void* slot) {
    switch (list->elem_size) {
    case 1:
        *(uint8_t*)out_ptr = *(const uint8_t*)slot;
        break;
    case 2:
        *(uint16_t*)out_ptr = *(const uint16_t*)slot;
        break;
    case 4:
        *(uint32_t*)out_ptr = *(const uint32_t*)slot;
        break;
    case 8:
        *(uint64_t*)out_ptr = *(const uint64_t*)slot;
        break;
    default:
        memcpy(out_ptr, slot, (size_t)list->elem_size);
        break;
    }
}

void* rt_list_create(int32_t elem_size, rt_list_eq_fn eq,
                     rt_list_arc_fn arc_inc, rt_list_arc_fn arc_dec) {
    RtList* list = (RtList*)calloc(1, sizeof(RtList));
    if (!list) {
        rt_panic("oom");
    }
    list->elem_size = elem_size;
    list->size = 0;
    list->capacity = 0;
    list->data = NULL;
    list->eq = eq;
    list->arc_inc = arc_inc;
    list->arc_dec = arc_dec;
    return list;
}

void* rt_list_create_with_capacity(int32_t elem_size, int32_t capacity,
                                    rt_list_eq_fn eq,
                                    rt_list_arc_fn arc_inc,
                                    rt_list_arc_fn arc_dec) {
    RtList* list = (RtList*)calloc(1, sizeof(RtList));
    if (!list) rt_panic("oom");
    list->elem_size = elem_size;
    list->size = 0;
    list->capacity = capacity > 0 ? capacity : 0;
    if (list->capacity > 0) {
        list->data = calloc((size_t)list->capacity, (size_t)elem_size);
        if (!list->data) {
            free(list);
            rt_panic("oom");
        }
    }
    list->eq = eq;
    list->arc_inc = arc_inc;
    list->arc_dec = arc_dec;
    return list;
}

void rt_list_destroy(void* handle) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (list->arc_dec) {
        for (int32_t i = 0; i < list->size; i++) {
            void* slot = (char*)list->data + (size_t)i * (size_t)list->elem_size;
            list->arc_dec(slot);
        }
    }
    free(list->data);
    free(list);
}

void rt_list_push(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return;
    RtList* list = (RtList*)handle;
    rt_list_ensure_capacity_impl(list, list->size + 1);
    void* slot = (char*)list->data + (size_t)list->size * (size_t)list->elem_size;
    rt_list_store_elem(list, slot, elem_ptr);
    /* arc 回调契约：接收「槽指针」（rt_list_arc_inc_ref 内部 `*(void**)slot`
     * 解引用再取对象）。若传 elem_ptr，会把对象首 8 字节（ArcHeader.refcount）
     * 误当指针解引用并 rt_arc_inc 之——类元素自身 refcount 从不递增，临时
     * `new T` 下落即被释放、List 残留悬垂指针（http2_h2c_e2e ParseStatus AV）。 */
    if (list->arc_inc) list->arc_inc(slot);
    list->size++;
}

void* rt_list_at(void* handle, int32_t idx) {
    if (!handle) {
        rt_panic("list index on null");
    }
    RtList* list = (RtList*)handle;
    if (idx < 0 || idx >= list->size) {
        rt_panic("list index out of bounds");
    }
    return (char*)list->data + (size_t)idx * (size_t)list->elem_size;
}

void rt_list_get(void* handle, int32_t idx, void* out_ptr) {
    if (!out_ptr) return;
    void* slot = rt_list_at(handle, idx);
    RtList* list = (RtList*)handle;
    rt_list_load_elem(list, out_ptr, slot);
}

void rt_list_set(void* handle, int32_t idx, const void* elem_ptr) {
    if (!elem_ptr) return;
    RtList* list = (RtList*)handle;
    void* slot = rt_list_at(handle, idx);
    rt_list_store_elem(list, slot, elem_ptr);
    /* 同 push：arc 回调接收槽指针（见 rt_list_push 注记）。H1: 勿 dec old——
     * Clear/Remove/Set 期 free 与报告期 CRT 交织可损堆（UnitTest 末条
     * Wiki_Snapshot_Restore WriteResults AV）。旧元素漏至进程退出。 */
    if (list->arc_inc) list->arc_inc(slot);
}

int32_t rt_list_size(void* handle) {
    if (!handle) return 0;
    return ((RtList*)handle)->size;
}

/* RFC 016 M3 §3.3: 零拷贝 List<T> marshal 支持函数。
 *
 * 从 List handle 获取内部 buffer 指针和元素数量，供 FFI 边界直接传递给
 * C 函数（T* + size_t）。零拷贝——不复制 buffer，直接暴露内部指针。
 *
 * 性能考量（RFC 009 M5 高性能原则）：
 * - 零分配：仅读写 out 参数，不分配内存
 * - O(1) 复杂度：直接读 RtList 字段，无遍历
 * - 与 RFC 009 IO 多路复用目标协同：ORM 查询结果通过 List<T> 返回，
 *   FFI 边界的零拷贝 marshal 直接影响 IO 吞吐（目标 ≥20× vs C#）
 *
 * 生命周期约束：C 函数调用期间 List 不能被修改/释放（typeck 保证
 * List 不在 C 调用期间被重新赋值；运行时不显式检查，遵循 C ABI 语义）。
 */
void rt_list_buffer_and_size(void* handle, void** out_buf, int32_t* out_size) {
    if (!handle || !out_buf || !out_size) {
        if (out_buf) *out_buf = NULL;
        if (out_size) *out_size = 0;
        return;
    }
    RtList* list = (RtList*)handle;
    *out_buf = list->data;
    *out_size = list->size;
}

int32_t rt_list_contains(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* slot = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (rt_list_eq_default(list, slot, elem_ptr)) return 1;
    }
    return 0;
}

int32_t rt_list_index_of(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return -1;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* slot = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (rt_list_eq_default(list, slot, elem_ptr)) return i;
    }
    return -1;
}

void rt_list_insert(void* handle, int32_t idx, const void* elem_ptr) {
    if (!handle || !elem_ptr) return;
    RtList* list = (RtList*)handle;
    if (idx < 0 || idx > list->size) {
        rt_panic("list insert index out of bounds");
    }
    rt_list_ensure_capacity_impl(list, list->size + 1);
    if (idx < list->size) {
        memmove((char*)list->data + (size_t)(idx + 1) * (size_t)list->elem_size,
                (char*)list->data + (size_t)idx * (size_t)list->elem_size,
                (size_t)(list->size - idx) * (size_t)list->elem_size);
    }
    void* slot = (char*)list->data + (size_t)idx * (size_t)list->elem_size;
    memcpy(slot, elem_ptr, (size_t)list->elem_size);
    /* 同 push：arc 回调接收槽指针（见 rt_list_push 注记）。 */
    if (list->arc_inc) list->arc_inc(slot);
    list->size++;
}

void rt_list_remove_at(void* handle, int32_t idx) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (idx < 0 || idx >= list->size) {
        rt_panic("list remove index out of bounds");
    }
    /* H1: 勿 arc_dec 被移除元素——见 rt_list_set。 */
    if (idx < list->size - 1) {
        memmove((char*)list->data + (size_t)idx * (size_t)list->elem_size,
                (char*)list->data + (size_t)(idx + 1) * (size_t)list->elem_size,
                (size_t)(list->size - idx - 1) * (size_t)list->elem_size);
    }
    list->size--;
}

void rt_list_clear(void* handle) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    /* H1: 勿批量 arc_dec——AIWiki.Restore Clear 在套件末段 free 类元素，
     * 放大为 WriteResults 末条截断 AV。漏至进程退出。 */
    list->size = 0;
}

int32_t rt_list_remove(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return 0;
    int32_t idx = rt_list_index_of(handle, elem_ptr);
    if (idx < 0) return 0;
    rt_list_remove_at(handle, idx);
    return 1;
}

void rt_list_reverse(void* handle) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (list->size <= 1) return;
    /* Swap slots in place — refcounts are unchanged because the same set of
       elements remains owned by the list, just in a different order. */
    void* tmp = malloc((size_t)list->elem_size);
    if (!tmp) rt_panic("oom");
    int32_t i = 0;
    int32_t j = list->size - 1;
    char* base = (char*)list->data;
    while (i < j) {
        void* left = base + (size_t)i * (size_t)list->elem_size;
        void* right = base + (size_t)j * (size_t)list->elem_size;
        memcpy(tmp, left, (size_t)list->elem_size);
        memcpy(left, right, (size_t)list->elem_size);
        memcpy(right, tmp, (size_t)list->elem_size);
        i++;
        j--;
    }
    free(tmp);
}

/* ---- Phase 3: predicate/comparison/array callbacks ---- */

static int32_t rt_sort_elem_size = 0;

static int rt_list_cmp_default(const void* a, const void* b) {
    return memcmp(a, b, (size_t)rt_sort_elem_size);
}

int32_t rt_list_cmp_str(const void* a, const void* b) {
    const char* const* pa = (const char* const*)a;
    const char* const* pb = (const char* const*)b;
    const char* sa = *pa ? *pa : "";
    const char* sb = *pb ? *pb : "";
    return (int32_t)strcmp(sa, sb);
}

int32_t rt_list_find_get(void* handle, rt_list_pred_fn pred, void* out_ptr) {
    if (!handle || !pred || !out_ptr) return 0;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (pred(elem)) {
            memcpy(out_ptr, elem, (size_t)list->elem_size);
            return 1;
        }
    }
    return 0;
}

void* rt_list_find_all(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return NULL;
    RtList* list = (RtList*)handle;
    RtList* result = (RtList*)rt_list_create(list->elem_size, list->eq,
                                              list->arc_inc, list->arc_dec);
    if (!result) rt_panic("oom");
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (pred(elem)) {
            rt_list_push(result, elem);
        }
    }
    return result;
}

int32_t rt_list_exists(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return 0;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (pred(elem)) return 1;
    }
    return 0;
}

int32_t rt_list_find_index(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return -1;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (pred(elem)) return i;
    }
    return -1;
}

int32_t rt_list_find_last_index(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return -1;
    RtList* list = (RtList*)handle;
    for (int32_t i = list->size - 1; i >= 0; i--) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (pred(elem)) return i;
    }
    return -1;
}

int32_t rt_list_true_for_all(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return 0;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (!pred(elem)) return 0;
    }
    return 1;
}

int32_t rt_list_last_index_of(void* handle, const void* elem_ptr) {
    if (!handle || !elem_ptr) return -1;
    RtList* list = (RtList*)handle;
    for (int32_t i = list->size - 1; i >= 0; i--) {
        void* slot = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        if (rt_list_eq_default(list, slot, elem_ptr)) return i;
    }
    return -1;
}

void rt_list_for_each(void* handle, rt_list_pred_fn action) {
    if (!handle || !action) return;
    RtList* list = (RtList*)handle;
    for (int32_t i = 0; i < list->size; i++) {
        void* elem = (char*)list->data + (size_t)i * (size_t)list->elem_size;
        action(elem);
    }
}

int32_t rt_list_remove_all(void* handle, rt_list_pred_fn pred) {
    if (!handle || !pred) return 0;
    RtList* list = (RtList*)handle;
    int32_t w = 0;
    for (int32_t r = 0; r < list->size; r++) {
        void* elem = (char*)list->data + (size_t)r * (size_t)list->elem_size;
        if (pred(elem)) {
            /* H1: 勿 arc_dec——见 rt_list_arc_dec_ref。 */
            continue;
        }
        if (w != r) {
            memcpy((char*)list->data + (size_t)w * (size_t)list->elem_size,
                   elem, (size_t)list->elem_size);
        }
        w++;
    }
    int32_t removed = list->size - w;
    list->size = w;
    return removed;
}

void rt_list_sort(void* handle, rt_list_cmp_fn cmp) {
    if (!handle || !cmp) return;
    RtList* list = (RtList*)handle;
    if (list->size <= 1) return;
    qsort(list->data, (size_t)list->size, (size_t)list->elem_size,
          (int (*)(const void*, const void*))cmp);
}

void rt_list_sort_default(void* handle) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (list->size <= 1) return;
    if (list->eq == rt_list_eq_str) {
        qsort(list->data, (size_t)list->size, (size_t)list->elem_size,
              (int (*)(const void*, const void*))rt_list_cmp_str);
    } else {
        rt_sort_elem_size = list->elem_size;
        qsort(list->data, (size_t)list->size, (size_t)list->elem_size,
              rt_list_cmp_default);
    }
}

void* rt_list_to_array(void* handle) {
    if (!handle) return NULL;
    RtList* list = (RtList*)handle;
    if (list->size == 0) return NULL;
    /* 数组 ABI：RtArrayHeader { int32 length; int32 elem_size; } + payload。
     * 返回 payload 指针（rt_array_length/rt_array_destroy 读 -8 处的 header）。
     * 此前仅 malloc 裸 payload → Length 读 malloc 块头 → 越界/垃圾值。 */
    typedef struct {
        int32_t length;
        int32_t elem_size;
    } RtArrayHeaderCompat;
    size_t header = sizeof(RtArrayHeaderCompat);
    size_t bytes = header + (size_t)list->size * (size_t)list->elem_size;
    RtArrayHeaderCompat* h = (RtArrayHeaderCompat*)malloc(bytes);
    if (!h) rt_panic("oom");
    h->length = list->size;
    h->elem_size = list->elem_size;
    memcpy((char*)h + header, list->data, (size_t)list->size * (size_t)list->elem_size);
    return (char*)h + header;
}

void rt_list_copy_to(void* handle, void* dst, int32_t start_idx) {
    if (!handle || !dst || start_idx < 0) return;
    RtList* list = (RtList*)handle;
    if (list->size == 0) return;
    memcpy((char*)dst + (size_t)start_idx * (size_t)list->elem_size,
           list->data, (size_t)list->size * (size_t)list->elem_size);
}

void rt_list_add_range_list(void* dst, void* src) {
    if (!dst || !src) return;
    RtList* s = (RtList*)src;
    for (int32_t i = 0; i < s->size; i++) {
        void* elem = (char*)s->data + (size_t)i * s->elem_size;
        rt_list_push(dst, elem);
    }
}

/* P5-H: Capacity / IsReadOnly / RemoveRange / TrimExcess */

int32_t rt_list_capacity(void* handle) {
    if (!handle) return 0;
    RtList* list = (RtList*)handle;
    return list->capacity;
}

void rt_list_set_capacity(void* handle, int32_t new_cap) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (new_cap < list->size) new_cap = list->size;
    if (new_cap == list->capacity) return;
    void* new_data = realloc(list->data, (size_t)new_cap * (size_t)list->elem_size);
    if (!new_data) rt_panic("oom");
    list->data = new_data;
    list->capacity = new_cap;
}

int32_t rt_list_is_read_only(void* handle) {
    (void)handle;
    return 0;
}

void rt_list_remove_range(void* handle, int32_t index, int32_t count) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (index < 0 || count < 0 || index + count > list->size) {
        rt_panic("list remove_range out of bounds");
    }
    /* H1: 勿 arc_dec range——见 rt_list_arc_dec_ref。 */
    if (index + count < list->size) {
        memmove(
            (char*)list->data + (size_t)index * (size_t)list->elem_size,
            (char*)list->data + (size_t)(index + count) * (size_t)list->elem_size,
            (size_t)(list->size - index - count) * (size_t)list->elem_size
        );
    }
    list->size -= count;
}

void rt_list_trim_excess(void* handle) {
    if (!handle) return;
    RtList* list = (RtList*)handle;
    if (list->size == list->capacity) return;
    if (list->size == 0) {
        free(list->data);
        list->data = NULL;
        list->capacity = 0;
        return;
    }
    void* new_data = realloc(list->data, (size_t)list->size * (size_t)list->elem_size);
    if (!new_data) return;
    list->data = new_data;
    list->capacity = list->size;
}

/* ---- insert_range: batch insert with single memmove ---- */

void rt_list_insert_range(void* handle, int32_t idx, void* src, int32_t n) {
    if (!handle || !src || n <= 0) return;
    RtList* list = (RtList*)handle;
    if (idx < 0 || idx > list->size) {
        rt_panic("list insert_range index out of bounds");
    }
    rt_list_ensure_capacity_impl(list, list->size + n);
    // memmove tail to make room
    if (idx < list->size) {
        memmove((char*)list->data + (size_t)(idx + n) * (size_t)list->elem_size,
                (char*)list->data + (size_t)idx * (size_t)list->elem_size,
                (size_t)(list->size - idx) * (size_t)list->elem_size);
    }
    // memcpy source elements into the gap
    memcpy((char*)list->data + (size_t)idx * (size_t)list->elem_size,
           src, (size_t)n * (size_t)list->elem_size);
    // ARC inc for each new element
    if (list->arc_inc) {
        for (int32_t i = 0; i < n; i++) {
            void* slot = (char*)list->data + (size_t)(idx + i) * (size_t)list->elem_size;
            list->arc_inc(slot);
        }
    }
    list->size += n;
}

/* ---- get_range: create new list copy of a slice ---- */

void* rt_list_get_range(void* handle, int32_t idx, int32_t count) {
    if (!handle) return NULL;
    RtList* list = (RtList*)handle;
    if (idx < 0 || count < 0 || idx + count > list->size) {
        rt_panic("list get_range index out of bounds");
    }
    RtList* result = (RtList*)rt_list_create(list->elem_size, list->eq,
                                              list->arc_inc, list->arc_dec);
    if (!result) rt_panic("oom");
    if (count == 0) return result;
    rt_list_ensure_capacity_impl(result, count);
    void* src_slice = (char*)list->data + (size_t)idx * (size_t)list->elem_size;
    memcpy(result->data, src_slice, (size_t)count * (size_t)list->elem_size);
    result->size = count;
    // ARC inc for copied elements
    if (list->arc_inc) {
        for (int32_t i = 0; i < count; i++) {
            void* slot = (char*)result->data + (size_t)i * (size_t)list->elem_size;
            list->arc_inc(slot);
        }
    }
    return result;
}

/* ---- binary_search / binary_search_cmp ---- */

int32_t rt_list_binary_search(void* handle, const void* key) {
    if (!handle || !key) return -1;
    RtList* list = (RtList*)handle;
    if (list->size == 0) return -1;
    int32_t lo = 0, hi = list->size - 1;
    while (lo <= hi) {
        int32_t mid = lo + (hi - lo) / 2;
        void* elem = (char*)list->data + (size_t)mid * (size_t)list->elem_size;
        int cmp = memcmp(key, elem, (size_t)list->elem_size);
        if (cmp == 0) return mid;
        if (cmp < 0) hi = mid - 1;
        else lo = mid + 1;
    }
    return ~lo;  // bitwise complement of insertion point
}

int32_t rt_list_binary_search_cmp(void* handle, const void* key, rt_list_cmp_fn cmp) {
    if (!handle || !key || !cmp) return -1;
    RtList* list = (RtList*)handle;
    if (list->size == 0) return -1;
    int32_t lo = 0, hi = list->size - 1;
    while (lo <= hi) {
        int32_t mid = lo + (hi - lo) / 2;
        void* elem = (char*)list->data + (size_t)mid * (size_t)list->elem_size;
        int c = cmp(key, elem);
        if (c == 0) return mid;
        if (c < 0) hi = mid - 1;
        else lo = mid + 1;
    }
    return ~lo;
}
