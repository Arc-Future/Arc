# RFC 028 类型反射面

## 背景

Arc 需要以 `typeof(T)` 获得运行时的类型元数据，供调试、DI、序列化、ORM 等消费。设计目标：`typeof`/`Type` 用户面、只读反射元数据消费、反射**调用**永久剔除（元数据保留、不经反射促发执行）、internal 类型不暴露。

## 设计决策

### typeof / Type 用户面

`typeof(T)` 返回 `Type`；`Type` 元数据经只读 rodata（`RtTypeInfo`）承载。

```as
using Arc;

Type t = typeof(List<int>);
```

| 面 | 类型 | 可见性 |
|----|------|--------|
| 抽象元数据面 | `Type`/`MemberInfo`/`MethodInfo`/`FieldInfo`/`PropertyInfo` | public（对标 .NET 公共元数据面） |
| 工具链注入面 | `RuntimeType`/`RuntimeMethodInfo`/`RuntimeFieldInfo`/`RuntimePropertyInfo` | 语义定位 internal（`typeof` 降级构造属工具链注入契约面） |

**设计决策**：

- **抽象面与实现解耦**（对标 .NET）：用户只见 `Type` 等抽象契约，不见 `Runtime*` 实现；`Runtime*` 因 `typeof` 降级构造属**工具链注入契约面**而 public，语义定位 internal。
- **internal 类型不暴露**：`typeof` 不得暴露包内 `internal` 类型；可见性以库设计意图为准（见 [020 标准库架构与拆分](020-std-architecture.md)）。
- **元数据消费**：`*Info` 类承载只读元数据（名称、成员、签名），无函数指针。
- **反射调用剔除**：反射**调用**永久剔除——元数据保留供读取，但不经反射促发任意方法执行；这在编译期确定，杜绝基于字符串的反射调用面。
- `typeof(T)` 与 `nameof` 配合用于本地化/属性定位；自定义属性与签名类型的消费面为工具链另轨。

### 与工具链的关系

`typeof` 降级生成 `RuntimeType` 构造调用进用户程序，属工具链注入面（R2）；`Runtime*` 收窄需随生成路径改进（生成代码改经抽象面/公共工厂构造后 internal 化）。测试程序经 `InternalsVisibleTo` 验证反射内实现。

## 边界

- 本文档讲 `typeof`/`Type` 用户面与元数据**消费**；反射元数据的**发射**（rodata 布局、`RtTypeInfo`）见 [018 类型体系与反射元数据](018-type-reflection-metadata.md)。
- 可见性纪律（internal 边界、R0/R1/R2）见 [020 标准库架构与拆分](020-std-architecture.md)。

---

上一节：[027 本地化与资源](027-localization-resources.md) · 下一节：[029 图像与图形](029-imaging-graphics.md)