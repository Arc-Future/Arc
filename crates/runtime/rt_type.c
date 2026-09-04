// 运行时类型判断 + 完整反射元数据 ABI（RFC 018 M1 + RFC 018 M1）。
//
// vtable slot 0 放 const RtTypeInfo* typeinfo（RFC 006 修订），
// rt_obj_isa 遍历 parent 链比对 type_id，实现 class 层级的 `is` 测试。
// RFC 018 M1 扩展 RtTypeInfo 为完整元数据描述，并新增 ABI 查询函数。
//
// 内存布局假设（与 codegen emit_aggregate / emit_vtables 协同）：
//
//   Arc 对象布局（has_vtable == true）：
//     offset 0:  _Atomic int32_t refcount   (4B)
//     offset 8:  const void* vtable         (8B, 指向 vtable 全局)
//     offset 16: 字段区 ...
//
//   vtable 布局（RFC 018 D5 修订）：
//     slot 0: const RtTypeInfo* typeinfo   ← 新增
//     slot 1: dtor placeholder (ptr null)  ← RFC 006 原槽 0，语义保留
//     slot 2+: virtual methods             ← RFC 006 原 slot 1+ 平移
//
//   RtTypeInfo 布局（全局常量 @.typeinfo.{Class}）：
//     RFC 018 M1: type_id + parent
//     RFC 018 M1: 扩展为完整元数据描述（见 rt_abi.h RtTypeInfo 定义）
//
// 编译期 is 折叠（RFC 018 D8）由 typeck 在静态类型已知时直接产出常量，
// 此处仅处理运行时无法折叠的场景（基类指针测试子类）。
//
// RFC 018 M1 查询 ABI：
//   - rt_type_by_name/rt_type_by_id：先查基元静态表，再查用户注册表
//   - rt_type_find_method/field/property：遍历 declared_* 数组 + parent 链
//   - rt_type_is_subtype：直接遍历 parent 链
//   - rt_type_register：供 codegen 启动期注册用户类型 typeinfo 全局
//
// **type_id 一致性**：typeck 在编译期通过同一 FNV-1a 32 位算法计算 type_id；
// runtime 在初始化期通过 rt_fnv1a_32 计算基元 type_id。两端算法必须保持一致
// （hash 基 0x811c9dc5 + prime 0x01000193）。typeck 端实现见
// crates/typeck 的 type_name_to_id。

#include "rt_abi.h"

#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <stdatomic.h>

// ArcHeader 复用 rt_arc.c / rt_box.c 同一布局——为避免 C 文件间符号冲突，
// 此处本地 typedef（C 编译器允许同一 struct typedef 多次定义，前提是布局一致）。
typedef struct {
    _Atomic int32_t refcount;
    const void* vtable;
} ArcHeader_;

// vtable 偏移：ArcHeader.vtable 位于对象 offset 8（refcount 4B + padding 4B）
#define ARC_VTABLE_OFFSET 8

int32_t rt_obj_isa(void* obj, const RtTypeInfo* target) {
    if (!obj || !target) return 0;

    // 加载对象 vtable 指针（offset 8）
    const void* const* vtable_ptr =
        (const void* const*)((char*)obj + ARC_VTABLE_OFFSET);
    const void* vtable = *vtable_ptr;
    if (!vtable) return 0;

    // vtable slot 0 = const RtTypeInfo* typeinfo（RFC 018 D5）
    const RtTypeInfo* obj_ti = (const RtTypeInfo*)(*(const void* const*)vtable);
    if (!obj_ti) return 0;

    // RFC 017 M1: 当目标类型是接口时，遍历 implemented_interfaces 数组
    if (target->kind == RT_TYPE_KIND_INTERFACE) {
        const RtTypeInfo* cur = obj_ti;
        while (cur) {
            if (cur->implemented_interfaces && cur->interface_count > 0) {
                for (int32_t i = 0; i < cur->interface_count; ++i) {
                    const RtTypeInfo* iface = cur->implemented_interfaces[i];
                    if (iface && iface->type_id == target->type_id) return 1;
                }
            }
            // 也检查当前类型自身是否直接就是该接口类型（接口 typeinfo 的 parent 为 NULL）
            if (cur->type_id == target->type_id) return 1;
            cur = cur->parent;
        }
        return 0;
    }

    // 遍历 parent 链比对 type_id（class 层级）
    while (obj_ti) {
        if (obj_ti->type_id == target->type_id) return 1;
        obj_ti = obj_ti->parent;
    }
    return 0;
}

// =====================================================================
// RFC 004 P0 后续 Sprint：动态 downcast（object → 接口）itable 查找
// =====================================================================
//
// 返回 obj 实际类型实现 target 接口的 itable 指针；未实现返回 NULL。
// 与 rt_obj_isa 接口遍历同源，但返回平行 interface_itables[i] 而非 1/0：
//   - 接口目标：沿 typeinfo（含 parent 链）在 implemented_interfaces 中
//     比对 type_id，命中返回 interface_itables[i]（class 为 @.itable.{C}_{Iface}、
//     struct 为 @.itable.{Struct}_Box_{Iface}；标记接口该槽为 null）。
//   - 非接口目标：不支持（downcast 语义仅针对接口视图），返回 NULL。
const void* rt_obj_to_iface(void* obj, const RtTypeInfo* target_iface) {
    if (!obj || !target_iface) return NULL;
    if (target_iface->kind != RT_TYPE_KIND_INTERFACE) return NULL;

    const void* const* vtable_ptr =
        (const void* const*)((char*)obj + ARC_VTABLE_OFFSET);
    const void* vtable = *vtable_ptr;
    if (!vtable) {
        return NULL;
    }

    const RtTypeInfo* obj_ti = (const RtTypeInfo*)(*(const void* const*)vtable);
    if (!obj_ti) {
        return NULL;
    }

    const RtTypeInfo* cur = obj_ti;
    while (cur) {
        if (cur->implemented_interfaces && cur->interface_count > 0) {
            for (int32_t i = 0; i < cur->interface_count; ++i) {
                const RtTypeInfo* iface = cur->implemented_interfaces[i];
                if (iface && iface->type_id == target_iface->type_id) {
                    // 平行 interface_itables[i] 可能是 null（标记接口无 itable）
                    // ——调用方（codegen）据此判定 downcast 失败。
                    return cur->interface_itables ? cur->interface_itables[i] : NULL;
                }
            }
        }
        cur = cur->parent;
    }
    return NULL;
}

// =====================================================================
// RFC 006 M3: string→object 装箱（object 槽持有 string 有合法 vtable）
// =====================================================================
// object 槽中的 string 值 = 堆分配 ArcStringBox：ArcHeader(vtable =
// &__arc_string_vtable) + char* 负载。vtable slot0 = &rt_typeinfo_string，
// 使 rt_obj_isa 能识别 `o is string`（读 offset8 vtable → slot0 typeinfo →
// type_id 匹配），且对其它类型判别（`o is List<T>` 等）安全返回 0
// （rt_typeinfo_string 无 parent、type_id 不匹配）。
//
// 该 box 非 ARC 管理（object 槽现有语义，arc_class_place 对 Object 返回 false），
// DP 替换时 box 泄漏记为已知边界（RFC 006 §7 诚实边界）。

// 基元 typeinfo 前向声明（定义见下方「基元 typeinfo 静态表」区块）——上移至
// 首次引用（__arc_string_vtable slot0）之前。RFC 017 阶段一（ALC 共享 dll）：
// **static**（TU 局部）——共享库边界上数据符号的导入 thunk 是「指向数据的
// 指针」而非数据本身，codegen 引用数据符号会别名 thunk 导致语义错误。
// codegen 经 rt_typeinfo_prim(id) / rt_box_vtable(id) 函数符号按需查询，
// 两函数与本表同 TU，地址语义不变。
static RtTypeInfo rt_typeinfo_int;
static RtTypeInfo rt_typeinfo_long;
static RtTypeInfo rt_typeinfo_short;
static RtTypeInfo rt_typeinfo_byte;
static RtTypeInfo rt_typeinfo_char;
static RtTypeInfo rt_typeinfo_float;
static RtTypeInfo rt_typeinfo_double;
static RtTypeInfo rt_typeinfo_bool;
static RtTypeInfo rt_typeinfo_string;
static RtTypeInfo rt_typeinfo_void;
static RtTypeInfo rt_typeinfo_object;

typedef struct {
    _Atomic int32_t refcount;
    const void* vtable;   /* = __arc_string_vtable */
    const char* str;
} ArcStringBox;

static const void* const __arc_string_vtable[] = {
    &rt_typeinfo_string,   /* slot0: RtTypeInfo* for string（RFC 018 D5 布局） */
    NULL,
};

void* rt_string_box(const char* s) {
    ArcStringBox* b = (ArcStringBox*)malloc(sizeof(ArcStringBox));
    if (!b) return NULL;
    atomic_init(&b->refcount, 1);
    b->vtable = __arc_string_vtable;
    b->str = s;
    return b;
}

const char* rt_string_unbox(void* obj) {
    if (!obj) return NULL;
    ArcStringBox* b = (ArcStringBox*)obj;
    if (b->vtable != __arc_string_vtable) return NULL;
    return b->str;
}

// =====================================================================
// RFC 018 M1: 基元 typeinfo 静态表
// =====================================================================
//
// 基元类型（int/long/short/byte/char/float/double/bool/string/void/object）
// 不由 codegen 发射 typeinfo 全局——它们是语言内置类型，由 C 运行时静态持有。
//
// type_id 由 rt_fnv1a_32 在初始化期计算（与 typeck 编译期算法一致）。
//
// **物理边界**（RFC 018 §3.3）：基元 typeinfo 的所有 declared_* 数组为空指针，
// count = 0；不持有任何成员元数据（基元类型无字段/方法/属性/构造函数）。

// FNV-1a 32 位哈希（与 RFC 026 type_name_to_id 实现一致）
static int32_t rt_fnv1a_32(const char* s) {
    uint32_t hash = 0x811c9dc5u;
    while (*s) {
        hash ^= (uint8_t)*s++;
        hash *= 0x01000193u;
    }
    return (int32_t)hash;
}

// 基元 typeinfo 定义（前向声明见文件头部 ArcStringBox 之前；type_id 由
// rt_type_init 在启动期填充，字段顺序对齐 RtTypeInfo 结构定义）。
// kind 取值：int/long/short/byte/char/float/double/bool = PRIMITIVE
//            string/object = CLASS（语言内置引用类型）
//            void = OTHER
// RFC 018 §5.2.2：基元 typeinfo 由 runtime 静态初始化。

// 基元 typeinfo 表（用于 rt_type_by_name / rt_type_by_id 线性查找）
static const RtTypeInfo* rt_primitive_table[] = {
    &rt_typeinfo_int,
    &rt_typeinfo_long,
    &rt_typeinfo_short,
    &rt_typeinfo_byte,
    &rt_typeinfo_char,
    &rt_typeinfo_float,
    &rt_typeinfo_double,
    &rt_typeinfo_bool,
    &rt_typeinfo_string,
    &rt_typeinfo_void,
    &rt_typeinfo_object,
};
#define RT_PRIMITIVE_COUNT \
    (sizeof(rt_primitive_table) / sizeof(rt_primitive_table[0]))

// 初始化单个基元 typeinfo（无 parent / 无成员）
static void rt_init_primitive(RtTypeInfo* ti, const char* name, int32_t kind) {
    ti->type_id = rt_fnv1a_32(name);
    ti->parent = NULL;
    ti->name = name;
    ti->full_name = name;       /* 基元类型无命名空间 */
    ti->ns = "";
    ti->kind = kind;
    ti->flags = 0;
    ti->declared_methods = NULL;
    ti->declared_method_count = 0;
    ti->declared_fields = NULL;
    ti->declared_field_count = 0;
    ti->declared_properties = NULL;
    ti->declared_property_count = 0;
    ti->declared_events = NULL;
    ti->declared_event_count = 0;
    ti->declared_constructors = NULL;
    ti->declared_ctor_count = 0;
    ti->implemented_interfaces = NULL;
    ti->interface_count = 0;
    ti->element_type = NULL;
    ti->declared_nested_types = NULL;
    ti->nested_type_count = 0;
    ti->attributes = NULL;
    ti->attribute_count = 0;
    ti->interface_itables = NULL;
}

// 一次性初始化所有基元 typeinfo（幂等）。由 rt_env_init 在进程启动时调用，
// 替代缺失的 GCC/Clang constructor 属性（跨平台可移植）。
void rt_type_init(void) {
    static int32_t initialized = 0;
    if (initialized) return;
    initialized = 1;

    rt_init_primitive(&rt_typeinfo_int,    "int",    RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_long,   "long",   RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_short,  "short",  RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_byte,   "byte",   RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_char,   "char",   RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_float,  "float",  RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_double, "double", RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_bool,   "bool",   RT_TYPE_KIND_PRIMITIVE);
    rt_init_primitive(&rt_typeinfo_string, "string", RT_TYPE_KIND_CLASS);
    rt_init_primitive(&rt_typeinfo_void,   "void",   RT_TYPE_KIND_OTHER);
    rt_init_primitive(&rt_typeinfo_object, "object", RT_TYPE_KIND_CLASS);
}

// RFC 017 阶段一（ALC 共享 dll）：基元 typeinfo 按 id 查询（导出面唯一入口）。
// id 序与 rt_primitive_table 一致：int=0 long=1 short=2 byte=3 char=4
// float=5 double=6 bool=7 string=8 void=9 object=10；越界返回 NULL。
// 反射代码（RuntimeType getter 拦截路径）可能先于 rt_env_init 触达，
// 故此处自触发 rt_type_init（幂等）。
const RtTypeInfo* rt_typeinfo_prim(int32_t id) {
    rt_type_init();
    if (id < 0 || (size_t)id >= RT_PRIMITIVE_COUNT) return NULL;
    return rt_primitive_table[id];
}

// 基元装箱 vtable 按 id 查询。表内嵌本 TU 的 rt_typeinfo_<prim> 地址
// （static 化后跨 TU 不可引用，故必须定义于 rt_type.c 而非 rt_box.c）。
// 仅覆盖可装箱基元（int/long/short/byte/char/float/double/bool），
// string/void/object 无此表（string 装箱走 rt_string_box 专用路径）。
static const void* const rt_box_vtables[8][3] = {
    { &rt_typeinfo_int,    NULL, NULL },  /* 0 int    */
    { &rt_typeinfo_long,   NULL, NULL },  /* 1 long   */
    { &rt_typeinfo_short,  NULL, NULL },  /* 2 short  */
    { &rt_typeinfo_byte,   NULL, NULL },  /* 3 byte   */
    { &rt_typeinfo_char,   NULL, NULL },  /* 4 char   */
    { &rt_typeinfo_float,  NULL, NULL },  /* 5 float  */
    { &rt_typeinfo_double, NULL, NULL },  /* 6 double */
    { &rt_typeinfo_bool,   NULL, NULL },  /* 7 bool   */
};

const void* rt_box_vtable(int32_t id) {
    if (id < 0 || (size_t)id >= sizeof(rt_box_vtables) / sizeof(rt_box_vtables[0]))
        return NULL;
    return rt_box_vtables[id];
}

// =====================================================================
// RFC 018 M1: 用户类型 typeinfo 注册表
// =====================================================================
//
// 用户定义的 class/struct/interface/enum 由 codegen 发射为 @.typeinfo.{Type}
// 全局常量。运行时无法通过 dlsym 查找符号（跨平台问题），因此由 codegen
// 在 .ctor 段插入对 rt_type_register 的调用，将用户 typeinfo 注册到本表。
//
// M1 范围：动态表大小上限 4096，线性查找（足够覆盖中型项目）。
// M3+ 评估：可升级为 hash 表（按 type_id 分桶）以支持大型项目。

#define RT_USER_TYPE_TABLE_CAP 4096
static const RtTypeInfo* rt_user_type_table[RT_USER_TYPE_TABLE_CAP];
static int32_t rt_user_type_count = 0;

// codegen 在 .ctor 段调用此函数注册用户类型 typeinfo（幂等：重复注册返回 1）
int32_t rt_type_register(const RtTypeInfo* ti) {
    if (!ti) return 0;
    rt_type_init();  /* 确保基元表已初始化 */
    // 幂等检查：已注册则跳过
    for (int32_t i = 0; i < rt_user_type_count; ++i) {
        if (rt_user_type_table[i] == ti) return 1;
        if (rt_user_type_table[i]->type_id == ti->type_id) return 0;
    }
    if (rt_user_type_count >= RT_USER_TYPE_TABLE_CAP) return 0;
    rt_user_type_table[rt_user_type_count++] = ti;
    return 1;
}

// =====================================================================
// RFC 018 M1: ABI 查询函数实现
// =====================================================================

// 按全名查找类型——先查基元表，再查用户注册表
const RtTypeInfo* rt_type_by_name(const char* full_name) {
    if (!full_name) return NULL;
    rt_type_init();  /* 懒初始化基元表 */

    // 基元类型查找
    for (size_t i = 0; i < RT_PRIMITIVE_COUNT; ++i) {
        const RtTypeInfo* ti = rt_primitive_table[i];
        if (strcmp(ti->full_name, full_name) == 0) return ti;
    }

    // 用户类型查找
    for (int32_t i = 0; i < rt_user_type_count; ++i) {
        const RtTypeInfo* ti = rt_user_type_table[i];
        if (ti->full_name && strcmp(ti->full_name, full_name) == 0) return ti;
    }
    return NULL;
}

// 按类型 ID 查找——先查基元表，再查用户注册表
const RtTypeInfo* rt_type_by_id(int32_t type_id) {
    rt_type_init();  /* 懒初始化基元表 */

    // 基元类型查找
    for (size_t i = 0; i < RT_PRIMITIVE_COUNT; ++i) {
        const RtTypeInfo* ti = rt_primitive_table[i];
        if (ti->type_id == type_id) return ti;
    }

    // 用户类型查找
    for (int32_t i = 0; i < rt_user_type_count; ++i) {
        const RtTypeInfo* ti = rt_user_type_table[i];
        if (ti->type_id == type_id) return ti;
    }
    return NULL;
}

// 遍历基类链查找方法（含继承）
const RtMethodInfo* rt_type_find_method(
    const RtTypeInfo* type, const char* name, int32_t param_count) {
    if (!type || !name) return NULL;

    const RtTypeInfo* cur = type;
    while (cur) {
        for (int32_t i = 0; i < cur->declared_method_count; ++i) {
            const RtMethodInfo* m = &cur->declared_methods[i];
            if (strcmp(m->name, name) != 0) continue;
            if (param_count >= 0 && m->parameter_count != param_count) continue;
            return m;
        }
        cur = cur->parent;
    }
    return NULL;
}

// 遍历基类链查找字段（含继承）
const RtFieldInfo* rt_type_find_field(
    const RtTypeInfo* type, const char* name) {
    if (!type || !name) return NULL;

    const RtTypeInfo* cur = type;
    while (cur) {
        for (int32_t i = 0; i < cur->declared_field_count; ++i) {
            const RtFieldInfo* f = &cur->declared_fields[i];
            if (strcmp(f->name, name) == 0) return f;
        }
        cur = cur->parent;
    }
    return NULL;
}

// 遍历基类链查找属性（含继承）
const RtPropertyInfo* rt_type_find_property(
    const RtTypeInfo* type, const char* name) {
    if (!type || !name) return NULL;

    const RtTypeInfo* cur = type;
    while (cur) {
        for (int32_t i = 0; i < cur->declared_property_count; ++i) {
            const RtPropertyInfo* p = &cur->declared_properties[i];
            if (strcmp(p->name, name) == 0) return p;
        }
        cur = cur->parent;
    }
    return NULL;
}

// 判断 type 是否是 base 的子类型（含自身）
int32_t rt_type_is_subtype(const RtTypeInfo* type, const RtTypeInfo* base) {
    if (!type || !base) return 0;

    const RtTypeInfo* cur = type;
    while (cur) {
        if (cur->type_id == base->type_id) return 1;
        cur = cur->parent;
    }
    return 0;
}

// =====================================================================
// RFC 018 M2: RtTypeInfo 字段直接查询 ABI
// =====================================================================
//
// 这些函数直接从 RtTypeInfo* 读取指定字段。codegen 在拦截
// RuntimeType 的 [Builtin] getter 时发射对这些函数的调用。
// 零堆分配，O(1) 复杂度，仅指针解引用。

const char* rt_type_get_name(const RtTypeInfo* ti) {
    return ti ? ti->name : "";
}

const char* rt_type_get_full_name(const RtTypeInfo* ti) {
    return ti ? ti->full_name : "";
}

int32_t rt_type_get_kind(const RtTypeInfo* ti) {
    return ti ? ti->kind : RT_TYPE_KIND_CLASS;
}

const RtTypeInfo* rt_type_get_base(const RtTypeInfo* ti) {
    return ti ? ti->parent : NULL;
}
