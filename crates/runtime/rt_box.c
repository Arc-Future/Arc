// FFI Marshal 装箱 ABI（RFC 016 v2 M2 / RFC 016 M3 同期推进）。
//
// ArcBox 内存布局（v2 简化版，移除 type_id 字段——反射永久剔除）：
//
//   ┌────────────────┐  offset 0
//   │ ArcHeader       │   _Atomic int32_t refcount  (4B)
//   │                 │   const void* vtable        (8B, 4B padding 在前)
//   ├────────────────┤  offset 16
//   │ payload_size    │   int32_t (4B) — 装箱时记录的 payload 字节数
//   ├────────────────┤  offset 20
//   │ _padding        │   int32_t (4B) — 保证 payload 8B 对齐
//   ├────────────────┤  offset 24
//   │ payload[N]      │   实际负载数据
//   └────────────────┘
//
// 共享 ArcHeader 布局意味着 rt_arc_inc/rt_arc_dec 可直接管理 ArcBox 生命周期——
// rt_box_destroy 即 rt_arc_dec 的 alias，无独立实现。
//
// unboxing 校验：rt_box_unbox 比较 expected_size 与 payload_size，
// 不匹配则调用 rt_panic_at（与 RFC 014 一致）。

#include "rt_abi.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    _Atomic int32_t refcount;
    const void* vtable;
} ArcHeader;

typedef struct {
    ArcHeader header;
    int32_t payload_size;
    int32_t _padding;
    // payload follows at offset 24, 8B aligned
} ArcBoxHeader;

static const size_t ARCX_BOX_HEADER_SIZE = sizeof(ArcBoxHeader);

void* rt_box_create(int32_t payload_size, int32_t payload_align) {
    // v2 假设 payload_align <= 8（ArcBoxHeader 自身 8B 对齐，覆盖所有基元/struct 场景）
    (void)payload_align;
    size_t total = ARCX_BOX_HEADER_SIZE + (size_t)payload_size;
    void* box = malloc(total);
    if (!box) return NULL;
    ArcBoxHeader* h = (ArcBoxHeader*)box;
    atomic_init(&h->header.refcount, 1);
    h->header.vtable = NULL;       // v2 移除 type_id，反射永久剔除
    h->payload_size = payload_size;
    h->_padding = 0;
    return box;  // 返回 ArcHeader 起始指针，与 rt_arc_inc/dec 兼容
}

void rt_box_destroy(void* box_ptr) {
    // rt_box_destroy 是 rt_arc_dec 的 alias——ArcBox 共享 ArcHeader 布局。
    // 字段级 ARC dec 由 codegen 内联发射（arc_drop.rs），此处仅 dec refcount + free。
    rt_arc_dec(box_ptr);
}

int32_t rt_box_unbox(void* box_ptr, int32_t expected_size,
                     void* out_ptr, int32_t out_size) {
    if (!box_ptr || !out_ptr) return -1;
    ArcBoxHeader* h = (ArcBoxHeader*)box_ptr;
    // v2 size 校验（替代 v1 type_id 校验，反射永久剔除）
    if (expected_size != h->payload_size) {
        // 调用方应在 unboxing 前已通过 codegen 嵌入源位置常量
        // 此处用通用 panic；带源位置的 panic 由 codegen 直接发射 rt_panic_at
        rt_panic("InvalidCastException: unboxing size mismatch");
        return -2;
    }
    int32_t copy_size = expected_size < out_size ? expected_size : out_size;
    void* payload = (char*)box_ptr + ARCX_BOX_HEADER_SIZE;
    memcpy(out_ptr, payload, (size_t)copy_size);
    return 0;
}
