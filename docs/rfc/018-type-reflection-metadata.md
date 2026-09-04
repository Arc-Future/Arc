# RFC 018 类型体系与反射元数据

## 背景

Arc 引入完整 `Type` 体系与反射**元数据**描述层（对齐 C# `System.Reflection`），供调试、DI、序列化、ORM 等消费 `typeof(T)` 得到的运行时类型信息。设计核心二分：**元数据描述保留、动态操作永久剔除**——反射**调用**（`Invoke`/`GetValue`/`SetValue`/`CreateInstance`）不在 Arc 中，杜绝基于字符串的反射调用面。`*Info` 物理上不含函数指针/字段偏移（ABI 层面无法 Invoke），`typeof(T)` 返回 `Type`（编译期 codegen 发射 rodata）。信条④「代码即数据」——元数据描述保留、动态操作拒绝。

## 设计决策

### `typeof(T)` → `Type`

`typeof(T)` 于编译期由 codegen 发射只读 rodata（`RtTypeInfo`），返回 `Type`；类型标识语义统一以 `TypeId` 为唯一载体。

```as
using Arc;

Type t = typeof(List<int>);
```

### Type 体系与可见性

| 面 | 类型 | 可见性 |
|----|------|--------|
| 抽象元数据面 | `Type`/`MemberInfo`/`MethodInfo`/`FieldInfo`/`PropertyInfo` | public（对标 .NET 公共元数据面） |
| 实现面 | `RuntimeType`/`RuntimeMethodInfo`/`RuntimeFieldInfo`/`RuntimePropertyInfo` | **internal**，仅标准库反射层内部使用；对外仅暴露抽象基类 |

- **抽象面与实现解耦**：用户只见 `Type` 等抽象契约，不见 `Runtime*` 实现；`Runtime*` 负责承载编译期发射的元数据，不暴露给开发者。
- **internal 类型不暴露**：`typeof` 不得暴露包内 `internal` 类型；可见性以库设计意图为准（见 [020 标准库架构与拆分](020-std-architecture.md)）。
- `typeof(T)` 与 `nameof` 配合用于本地化/属性定位；自定义属性与签名类型的消费面为工具链另轨。

### 只读元数据栈

| 组件 | 语义 |
|------|------|
| `RtTypeInfo` C ABI | 编译期经 codegen 发射只读 rodata；以 `TypeId` 语义作为类型元数据唯一载体 |
| `*Info` 类 | 承载只读元数据（名称、成员、签名）；**无函数指针、无字段偏移** |
| vtable slot 0 | 类型对象根：`typeinfo` |
| Expression 桥接 | `Expression.Type` 等强类型化，DI 容器等消费 MethodInfo 桥接 |

### 无反射调用（永久剔除）

反射动态操作层（`Invoke`/`GetValue`/`SetValue`/`CreateInstance`）**永久不支持**——编译期确定，无基于字符串的反射分派。`*Info` 无函数指针/字段偏移，ABI 层面无法 Invoke；这是结构性保障，非实现缺口。

### 内部类型访问控制

`RuntimeType`/`RuntimeMethodInfo`/`RuntimeFieldInfo`/`RuntimePropertyInfo` 实现类必须设为 **internal**，仅标准库反射层内部使用；对外 API 仅暴露 `Type`/`MethodInfo` 等抽象基类。内部实现细节不得暴露干扰开发者编码体验。

## 边界

- 本篇只讲**元数据发射（rodata 布局、`RtTypeInfo` codegen 发射、无反射调用）**；`typeof`/`Type` **用户面**与元数据**消费**见 [028 类型反射面](028-type-reflection.md)。
- 语言级类型系统（类型、泛型、可空、模式匹配）见 [004 类型系统](004-type-system.md)。
- 对象模型与 vtable 布局见 [006 对象模型](006-object-model.md)。
- 可见性纪律（internal 边界、R0/R1/R2）见 [020 标准库架构与拆分](020-std-architecture.md)。

---
上一节：[017 编译产物、包体系与类型身份](017-build-artifacts-packages.md) · 下一节：[019 自举路线图](019-self-hosting.md)