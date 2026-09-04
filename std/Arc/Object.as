// Object — FFI Marshal 专用根类型 stub（RFC 016 v2 M1）。
namespace Arc;

/// <summary>
/// FFI Marshal 专用根类型（RFC 016 v2 定位收窄）。
///
/// typeck 将小写 `object` 标识符识别为内置 TypeId::Object，在 lower_type 早期
/// 拦截，不走 registry 查找。此 `class Object` 声明仅作为类型系统锚点：
/// - M1（已实施）：引用类型（class/string/array/Task 等）隐式是 object 子类型，
///   `object o = new Foo();` 合法；`is_subtype(sub, TypeId::Object)` 短路判定。
/// - M2（已实施 2026-07-17，与 RFC 016 M3 `void*` ↔ `object` marshal 子项同期落地）：
///   FFI 边界值类型装箱——`void*` 形参/返回值 marshal 时自动插入 Box/Unbox 节点；
///   ArcBox 布局为 ArcHeader + payload_size + _padding + payload（v2 移除 type_id
///   字段，反射调用永久剔除；payload 起始 offset 24，详见 RFC 016 §15）。
///
/// **v2 永久剔除项**（由其他 RFC 承接或永不引入）：
/// - ~~ToString/Equals/GetHashCode 内置方法~~ → RFC 004 variant 模式匹配
///   + RFC 004 IEquatable&lt;T&gt;/IHashable&lt;T&gt;/INumber&lt;T&gt;
/// - ~~vtable 槽位 0/1/2 预留~~ → 维持 RFC 006 原状（dtor 占槽位 0）
/// - ~~通用容器基础（List&lt;object&gt;）~~ → RFC 004 variant 标签联合
/// - ~~反射**调用**体系（`GetType()` / `MemberwiseClone()` / `Invoke()` / `GetValue()` / `SetValue()`）~~ →
///   永久剔除；反射**元数据描述**体系（`Type` / `MemberInfo` 等）由 RFC 018 引入，
///   作为只读元数据快照供 LSP/Debugger/QIF/序列化等场景消费
///   （详见 [RFC 018](../../../docs/rfc/018-type-reflection-metadata.md)）
/// - ~~ArcBox.type_id 字段~~ → 仍永久剔除——obj.GetType() 运行时反查类型永久不支持，
///   Type 实例仅通过 `typeof(T)` 编译期获取
///
/// M1 阶段本 stub 无方法体，仅声明 class 以便 typeck 识别 `Object` 命名路径。
/// 直接实例化 `new Object()` 在 M1 不支持（object 是 FFI 边界抽象，非具体类）。
/// </summary>
public class Object {
}
