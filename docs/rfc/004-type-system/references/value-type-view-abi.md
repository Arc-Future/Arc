# 值类型视图 ABI 深度设计（装箱 / 接口 / 可空 / enum）

> 本文是 [004 类型系统(../../004-type-system.md) 的**能力子项**，承载「值类型视图 ABI」的深度布局、分派细则、现状核对、统一发射管线与实现分解。004 主文档保留架构级契约；实现细节下沉至此，按需钻取。

## 1. 现状核对（RFC 声明 vs 代码现实）

以下五条为「统一装箱视图」的系统性空洞（[Systematic base capability audit] 根因 A）。逐项给出 RFC 声明、代码现实与裁决。

| # | 能力 | RFC 声明 | 代码现实 | 裁决 |
|---|------|----------|----------|------|
| 1 | 基元 → `object`（`b1_boxing_int`：`object o = 5`） | 004 基元表：`object`「值类型赋值给 `object` 时自动装箱」 | typeck `registry_resolve.rs:490` `param_assignable` 仅放行引用类型（`!is_value_type_name`），值类型无自动装箱 → 报 `expected object, found int` | **已声明未兑现**；装箱为可取语义，**改实现**（自动装箱） |
| 2 | struct → `object`（`vt5_struct_boxing`：`object o = point`） | 同上「值类型自动装箱」 | 同 #1，struct 为值类型 → 拒绝 | **已声明未兑现**；改实现 |
| 3 | struct → `interface`（`vt6_struct_interface`：`IShape i = square; i.Area()`） | 006 接口 fat pointer：`{ ptr obj, ptr itable }`；typeck `registry_validate.rs:685` 允许 `Class \| Struct` 声明接口 | `StructLayout`（`layout.rs:129`）**无** `interfaces`/`method_impl`/`has_vtable`；codegen `emit_make_iface`（`emit_aggregate.rs:304`）假定 ArcHeader（`rt_arc_inc` + `obj+8` 读 vtable），struct 无头 → **AV** | **类型系统接受但 codegen 无发射**；补 struct 接口视图 |
| 4 | 可空值类型（`b2_nullable_a`/`o2_nullable_value`：`int? a = 42; a ?? 0`） | 004 原「禁止的类型」表：`int?`/`double?`「已实现·指针装箱：null → null ptr，非空 → 指向栈分配值」 | codegen 有部分 `??`（`emit_aggregate.rs:862`）/`==`（`emit_binary.rs:532`）路径，但 `int → int?` 的**值物化缺失**——非空值拿不到稳定指针，`a ?? 0` 解引用悬垂 → **AV**（`int? = null` 走 null ptr 正常） | **已声明未兑现**；改为内联 `{ HasValue, Value }` |
| 5 | enum 作哈希键（`vt8_enum_full`/`b6_enum_hashset`：`Dictionary<Color,int>`） | 004 泛型约束：「基元类型对内置接口（IComparable/IEquatable）隐式满足」——仅提基元，未提 enum | `check_generics.rs:1703` `is_primitive_satisfiable_interface` 仅覆盖内置基元（int/long/…），enum（`TypeId::Named`）不满足 `IEquatable`/`IHashable` → `Dictionary<Color,int>` 约束失败 | **缺口**；enum 隐式满足值语义接口 |

> 「已声明未兑现」指 RFC 文本已承诺、代码未兑现；对齐原则：**装箱/可空是可取语义，优先改实现对齐文档**，而非降低文档口径。仅当某项语义被重新裁决（如可空的表示从「指针装箱」改为「内联」）时才同步修订文档口径（本 RFC 已修订 004 相应表述）。

## 2. 装箱视图（值类型 → `object`）

### 2.1 box 布局

值类型装箱为堆对象，布局与既有 `ArcHeader` 类对象同构，可被 `rt_arc_inc`/`rt_arc_dec` 直接管理：

```
┌────────────────────┐  offset 0
│ ArcHeader          │   i32 refcount  (初始 1)
│                    │   ptr vtable     → 装箱类型的 typeinfo（非 NULL，使 `is`/GetType 可用）
├────────────────────┤  offset 16
│ typeinfo payload 头 │   与 typeinfo 同源（或复用 vtable slot0），标识「这是装箱 T」
├────────────────────┤  offset 24（8B 对齐）
│ value（T 的拷贝）    │   逐字段浅拷贝；内嵌 class 句柄随拷贝 rt_arc_inc
└────────────────────┘
```

- 与 [014 运行时 ABI(../../014-runtime-abi.md) 既有 FFI `ArcBox`（`rt_box_create`）的区别：FFI box `vtable = NULL` 且无 typeinfo，仅供 marshal 校验 size；**统一 box 设 `vtable = typeinfo`**，使装箱值成为完整对象（`o is T`、`(T)o`、接口分派均可用）。
- 命名约定（设计建议）：装箱类型的 typeinfo 符号 `@.typeinfo.{T}_Box`，接口 itable `@.itable.{T}_Box_{Iface}`。

### 2.2 自动装箱语义

- **隐式**：值类型赋值/传参/返回给 `object` 处自动插入装箱，无显式 `box` 关键字（对齐 C#；`box`/`unbox` 关键字不引入用户面）。
- **拷贝非移动**：装箱读源值并浅拷贝进 box（见 §6 Copy），源 `struct` 装箱后仍可用。
- **拆箱**：显式 `(T)obj` 经 typeinfo 校验 `T == 装箱类型` 后拷贝出内联值；不匹配编译期报错（静态类型已知时）或运行时报错（`object` 动态时）。

## 3. 接口视图（值类型 → `interface`）

### 3.1 fat pointer 构造

值类型赋值给 `interface` 时，装箱为堆盒后构造 fat pointer：

```
{ ptr 盒指针, ptr @.itable.{T}_Box_{Iface} }
```

- 盒与装箱视图 §2.1 **同一 box**；盒上 itable 槽位指向值类型的接口方法实现（接收者为「盒内值指针」byref 或盒指针 + 偏移）。
- 复用 006 既有 fat pointer 形态与 itable 扁平槽位布局（父接口方法在前、子接口在后），无第二套分派机制。

### 3.2 约束调用 vs 装箱调用（对齐 .NET `constrained.`）

| 静态类型 | 分派方式 | 装箱 | 场景 |
|----------|----------|------|------|
| 值类型（struct/enum，实现接口方法） | **约束调用**：byref 直调 `T::M(&value, …)`，零装箱 | 否 | 泛型 `where T : IShape`、直接 `square.Area()` 经接口静态类型 |
| 仅 `object`/`interface` 静态类型 | **装箱调用**：先装箱，再经盒 itable 间接调用 | 是 | `object o = square; (IShape)o` 后调用 |

两者共享同一 fat pointer 形态、同一 itable 槽位布局、同一接口方法 ABI——仅调用点的「取接收者指针」策略不同（约束调用指向值地址、装箱调用指向盒内值），杜绝双轨分派。

### 3.3 动态 downcast（`object → interface`）

`object o = square; (IShape)o` 是「先装箱 `object` 再恢复接口」的动态类型查询，属 P0 留待后续单目标 Sprint 的缺口。最小必要机制（非反射、非 `as` 全形态）：

- **boxed typeinfo 挂 itable**：`RtTypeInfo` 追加 `interface_itables`（与 `implemented_interfaces` 同索引平行数组）。class `C` 挂 `@.itable.{C}_{Iface}`；struct `T` 挂 `@.itable.{T}_Box_{Iface}`（值接收者 thunk）。
- **运行时判别**：`rt_obj_to_iface(obj, target_iface)` 读 `obj` vtable slot0 typeinfo，沿 typeinfo 在 `implemented_interfaces` 中比对 `type_id`，命中返回平行 `interface_itables[i]`；未命中返回 null。与 `rt_obj_isa` 同源，覆盖 class 与 boxed struct（单一分派路径）。
- **失败语义**：`(IShape)o` 失败（对象未实现接口）→ `rt_panic("InvalidCastException: …")`（对齐 unbox 尺寸不匹配口径），非崩溃；`(I)null` → null 接口引用（合法）。
- **零成本**：静态 `IShape i = square` 仍走 `MakeIface`（固定 itable 符号），不增运行时检查；仅 `object`/基类静态类型 → 接口的动态路径走 `rt_obj_to_iface`。

## 4. 可空视图（`T?` 值类型）

### 4.1 内联布局（对齐 .NET `Nullable<T>`）

`T?`（`T` 为值类型）为内联值类型：

```
struct T? { bool HasValue; T Value; }   // 对齐填充至 8B（如 int? = { i1, i32 }  → 8B）
```

- `int? a = 42;` → `{ HasValue = true, Value = 42 }`；`int? a = null;` → `{ HasValue = false, Value = default }`。
- 替代既有「指针装箱」表示（非空值指针指向栈分配值 → 生命周期悬垂 → AV），从根本上消除悬垂指针问题；`int? = null` 仍正常（`HasValue = false`）。
- `a ?? d` → `select HasValue, Value, d`（无指针解引用）；`a == b`/`a != b` 逐 `HasValue`/`Value` 比较（`==`：`a.HasValue && b.HasValue && a.Value == b.Value`，`!=` 取反）。

### 4.2 装箱恒等式（对齐 C#）

- `object o = (int?)42;` → 装箱为 `int` 盒（**非** `int?` 盒）；`object o = (int?)null;` → `null`。
- 恒等式：**boxed `Nullable<T>` ≡ boxed `T` / `null`**。`Nullable<T>` 自身**不实现任何接口**（对齐 .NET：装箱后 `T` 可能不实现该接口）。

### 4.3 与引用类型可空的边界

- 引用类型可空 `string?` 仍为 `ptr`（`null` 或句柄），沿用既有「可空类型与流分析」。
- 值类型可空 `T?` 与引用类型可空 `T?` 表示不同、**不再混用**（`codegen` 按 `inner` 是否值类型分派布局）。

## 5. enum 哈希/相等

- `enum` 隐式满足 `IEquatable<E>` / `IHashable<E>`（值语义，discriminant = `int32`），扩展 `satisfies_constraint`/`is_primitive_satisfiable_interface` 至 `TypeKind::Enum`。
- `E == E`/`E != E` 为判别值 `i32` 比较；`GetHashCode` 返回判别值（标量）。
- `Dictionary<E, V>` 零装箱：键走 [014(../../014-runtime-abi.md) 标量 `rt_hash_int`/`rt_eq_int` 快路径（`rt_hash_int` 哈希指针位即判别值）。

## 6. 与移动语义 / Copy 的交互（005）

`struct` 赋值默认移动（[005 内存模型(../../005-memory-model.md)）。装箱**不移动源值**：

- 装箱对值类型执行一次**隐式 Copy**：逐字段浅拷贝 + 内嵌 `class` 句柄 `rt_arc_inc`；该 Copy 语义与 `record struct` 合成拷贝同源，是值类型进入引用世界的**唯一边界操作**。
- 装箱后源 `struct` 仍可用，不触发 `UseAfterMove`。
- 接口视图的约束调用（§3.2）同样基于**读借用**（不移动），装箱调用基于 Copy。二者均不改 005 的 move-only 主语义——Copy 仅作为进入引用世界的显式边界，不引入普遍隐式复制。

## 7. 统一发射管线（架构级）

**单一装箱机制，非三处分别 patch**：

```
                ┌──────────────────────────────────────────┐
                │         value_type_box / value_type_unbox   │  （codegen 新增，唯一入口）
                └──────────────────────────────────────────┘
   值类型 → object 赋值 ────────────────┘   │   └────────────────
   值类型 → interface 赋值 ── 装箱 + 构造 fat pointer（复用 §3）
   T? 装箱（boxed Nullable ≡ boxed T）── 装箱 + 恒等式折叠（复用 §4.2）
   FFI marshal（既有 Expr::Box/Unbox）──── 收敛到同一 box 布局（vtable 置 typeinfo）
```

- **收敛点**：`emit_box.rs` 既有 FFI `rt_box_*` 路径、新的 `value_type_box`、可空装箱、接口盒化统一到同一 box 布局（`ArcHeader + typeinfo + 值拷贝`）。FFI 路径的 `vtable = NULL` 提升为 `vtable = typeinfo`（补齐 `is`/接口能力），`payload_size` 校验保留。
- **拆箱对称**：`value_type_unbox` 统一处理 `(T)obj`（typeinfo 校验 + 拷贝）、可空拆箱、FFI `rt_box_unbox`（size 校验）。
- **禁止**：在 `emit_coalesce`/`emit_nullable_value_eq`/`emit_make_iface` 三处分别补可空/接口/装箱逻辑（现状的「三处 patch」正是系统性空洞根源）。

## 8. 实现分解（架构级分阶段）

> 顺序按依赖关系：先统一管线基建（A），再铺三视图（B/C/D）。每阶段**可独立验收**、不回滚先前资产。

### A. 装箱视图 + 统一管线基建

- 新增 `value_type_box`/`value_type_unbox`；typeck 在值类型 → `object` 赋值处插入装箱（取代 `registry_resolve.rs:490` 的「仅引用类型」限制）。
- 收敛 FFI `rt_box_*` 到同一 box 布局（vtable 置 typeinfo）。
- 验收：`b1_boxing_int`、`vt5_struct_boxing`、FFI marshal 回归。

### B. 接口视图

- struct 接口实现收集 + `StructLayout.interfaces`/`method_impl` 补齐；发射 `@.itable.{T}_Box_{Iface}`。
- 约束调用（byref 直调）与装箱调用（盒 itable）两条调用点策略，共享 fat pointer。
- 验收：`vt6_struct_interface`（无 AV、`i.Area()` 正确）。

### C. 可空视图

- `T?`（值类型）改为内联 `{ HasValue, Value }`；重写 `??`/`==`/`!=`（内联字段访问，弃指针解引用）。
- 装箱恒等式（boxed `Nullable<T>` ≡ boxed `T` / `null`）。
- 验收：`b2_nullable_a`、`o2_nullable_value`（`int? a = 42; a ?? 0 == 42`；`int? = null` 回归）。

### D. enum 哈希/相等

- enum 隐式满足 `IEquatable`/`IHashable`；`Dictionary<E,V>` 标量键快路径。
- 验收：`vt8_enum_full`、`b6_enum_hashset`。

## 9. 验收标准（探针清单）

| 探针 | 断言 | 覆盖 |
|------|------|------|
| `b1_boxing_int` | `object o = 5; (int)o == 5; o is int` | A 装箱/拆箱 |
| `vt5_struct_boxing` | `object o = point; (Point)o == point; o is Point` | A struct 装箱 |
| `vt6_struct_interface` | `IShape i = square; i.Area()` 无 AV 且值正确 | B 接口视图 |
| `b2_nullable_a` | `int? a = 42; a ?? 0 == 42` | C 可空 |
| `o2_nullable_value` | `int? a = null; a ?? 0 == 0`（回归：`int? = null`） | C 可空 |
| `vt8_enum_full` | `Dictionary<Color,int>` 增删查正确 | D enum |
| `b6_enum_hashset` | enum 作 `HashSet`/`Dictionary` 键，值语义相等 | D enum |
| 回归 | FFI marshal box/unbox、class→interface fat pointer、`record struct` 字典键 | 三视图不破坏既有路径 |

---

[返回 004 主题入口(../../004-type-system.md) · [references 索引](index.md) · [返回 RFC 索引](../../index.md)
