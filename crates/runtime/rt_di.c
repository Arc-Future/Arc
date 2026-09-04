// DI Container runtime bridge (RFC 023 M1 v0.8 / RFC 018 M5).
//
// v0.6: Full DI container implementation in C (descriptor tables, resolution,
//        lifetime caching, scope management, 236 lines).
// v0.7: DI container logic migrated to pure Arc (std/DI/ServiceCollection.as,
//        ServiceProvider.as, ServiceScope.as). This file reduced to a single
//        ABI bridge: rt_di_resolve.
// v0.8 (RFC 018 M5): codegen 工厂改为直接构造 RuntimeType + itable GetService(Type)，
//        不再经本桥接传递已删除的语言层 TypeId struct。rt_di_resolve 保留为
//        兼容入口：用 rt_type_by_id 查找 RtTypeInfo*，分配最小 RuntimeType 壳后
//        调用 GetService。

#include "rt_abi.h"
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// ---- Data structures ----

/// IServiceProvider itable layout.
///
/// IServiceProvider（std/Arc/IServiceProvider.as，RFC 018 M4+）：
///   - object? GetService(Type serviceType)
///   - object? GetKeyedService(Type serviceType, object? key)
///
/// itable 按声明顺序排列：slot 0 = GetService, slot 1 = GetKeyedService.
typedef void* (*get_service_fn)(void* self, void* service_type);

/// 最小 RuntimeType 壳：与 codegen 发射的 class 对象头对齐。
/// layout: [0..4) refcount | [8..16) vtable | [16..24) _typeInfoHandle (i64)
/// （RuntimeType 仅含 _typeInfoHandle；父类 MemberInfo/Type 无实例字段时 handle@16。）
typedef struct {
    int32_t refcount;
    int32_t _pad;
    void* vtable;
    int64_t type_info_handle;
} arc_runtime_type_shim;

// ---- ABI ----

/// DI 依赖解析桥接 —— 通过 IServiceProvider itable 递归解析依赖。
///
/// 由旧版工厂或外部调用者使用；当前 codegen（RFC 018 M5）已内联 RuntimeType
/// 构造 + GetService，通常不再调用本函数。
///
/// sp 为 IServiceProvider fat pointer（{ ptr obj, ptr itable }），
/// type_id 为依赖类型的 FNV-1a 哈希（Type.TypeId / RtTypeInfo.type_id）。
void* rt_di_resolve(void* sp_fat, int32_t type_id) {
    if (!sp_fat) return NULL;

    const RtTypeInfo* ti = rt_type_by_id(type_id);
    if (!ti) return NULL;

    void** fat = (void**)sp_fat;
    void*  obj = fat[0];
    void** itable = (void**)fat[1];
    if (!obj || !itable) return NULL;

    get_service_fn get_service = (get_service_fn)itable[0];
    if (!get_service) return NULL;

    // 分配 RuntimeType 壳：vtable 置 null（GetService 内 Type.TypeId 由
    // codegen 拦截器经 _typeInfoHandle 读 RtTypeInfo.type_id，不经虚调用）。
    // 若走虚分派路径则需 @.vtable.RuntimeType——彼时应由 codegen 内联路径承担。
    arc_runtime_type_shim* rt =
        (arc_runtime_type_shim*)calloc(1, sizeof(arc_runtime_type_shim));
    if (!rt) return NULL;
    rt->refcount = 1;
    rt->vtable = NULL;
    rt->type_info_handle = (int64_t)(intptr_t)ti;

    void* result = get_service(obj, rt);
    // RuntimeType 为引用类型；GetService 返回后调用方持有服务实例。
    // shim 的生命周期：若 GetService 未 retain 参数则此处释放。
    // Arc GetService 仅读 TypeId，不存储 serviceType → 可立即 free。
    free(rt);
    return result;
}
