# 枚举能力增强（位运算 · Flags 特性 · 编译期工具方法）

> **注（2026-08-29）**：原 `crates/arc-integration` 已退场（a2627a0f）。本文所引
> `cargo test -p arc-integration ...` 验证命令不再可用；现行验证矩阵为
> `cargo test --workspace`（运行时面 `cargo test -p arc-tests --features full-rt`），
> 详见仓库根 `CHANGELOG.md`。

> 本子项承载 [004 类型系统(../../004-type-system.md) 的**枚举能力增强**——在现有 enum 基础（`int32_t` 判别值、`IEquatable`/`IHashable`）之上，引入位运算体系、`[Flags]` 标记特性，以及编译期烘焙的枚举工具方法。

## 动机

C# `System.Enum` 提供成熟的枚举位运算、Flags 组合、反射-free 工具方法体系。Arc 当前 enum 仅支持 `==`/`!=` 比较和 `switch` 模式匹配，缺少以下能力：

1. 位运算 `|` `&` `^` `~` 产生组合枚举值（`FileAccess.Read | FileAccess.Write`）
2. 复合赋值 `|=` `&=` `^=`（`flags |= Flag.Value`）
3. `[Flags]` 特性标记位域枚举（编译期校验规范、运行时影响 `ToString` 等行为）
4. `Enum.HasFlag` / `Enum.IsDefined` / `Enum.GetNames` / `Enum.GetValues` 等零反射工具方法

## 设计决策

### D1. 位运算

**决策**：允许 `|` `&` `^` `~` 作用于同类型枚举，结果类型为原枚举类型。

- 枚举底层为 `int32_t`，位运算在 `i32` 上进行，typeck 校验两操作数为同一命名枚举类型，返回该枚举类型。
- `~` 单目位取反：操作数为枚举类型，结果为同一枚举类型。
- `<<` `>>` 移位：支持枚举左移/右移，结果类型为枚举（用于定义 `Flag = 1 << n` 模式）。

```as
[Flags]
public enum FileAccess {
    None    = 0,
    Read    = 1,
    Write   = 2,
    Execute = 4,
}

FileAccess rw = FileAccess.Read | FileAccess.Write;  // 3
FileAccess rwx = rw | FileAccess.Execute;             // 7
bool canRead = (rw & FileAccess.Read) == FileAccess.Read; // true
```

### D2. 复合赋值

**决策**：`target |= expr` 脱糖为 `target = target | expr`（`&=` `^=` 同理）。与 `+=` `-=` `*=` `/=` 共用同一 `try_parse_compound_assign` 路径，不引入 AST 新变体。

- 新增 lexer token：`|=` `&=` `^=`
- 扩展 parser `try_parse_compound_assign`：`|=` → `BinOp::BitOr`，`&=` → `BinOp::BitAnd`，`^=` → `BinOp::BitXor`

### D3. `[Flags]` 特性

**决策**：`FlagsAttribute` 位于 `Arc` 命名空间（`Arc.FlagsAttribute`，别名 `[Flags]`），标记枚举为位域组合。

- `[Flags]` 不改变编译器行为，仅作为运行时元数据（`AttributeTargets.Enum`）。
- 编译期不校验成员值是否满足 2 的幂（由开发者自行保证，对齐 C# 惯例）。
- 未来可扩展 `[Flags]` 影响 `Enum.ToString()` 输出格式（逗号分隔组合名）。

```as
namespace Arc;

/// <summary>
/// 标记枚举为位域组合（对齐 System.FlagsAttribute）。
/// </summary>
[AttributeUsage(AttributeTargets.Enum)]
public class FlagsAttribute : Attribute {
}
```

### D4. 枚举工具方法

**决策**：`Enum` 静态类扩展以下方法，均按 `Enum.GetOptions<T>()` 模式**编译期烘焙**（零反射、零运行时开销）。

| 方法 | 签名 | 语义 |
|------|------|------|
| `HasFlag` | `Enum.HasFlag<T>(T value, T flag) -> bool` | `(value & flag) == flag` |
| `IsDefined` | `Enum.IsDefined<T>(T value) -> bool` | 判别值是否在已知成员集中 |
| `GetNames` | `Enum.GetNames<T>() -> string[]` | 返回成员名数组（按声明顺序） |
| `GetValues` | `Enum.GetValues<T>() -> T[]` | 返回成员值数组（按声明顺序） |

#### D4.1 `HasFlag`

`HasFlag` 编译期烘焙为 `(value & flag) == flag` 表达式——typeck 在 `Enum.HasFlag<MyEnum>(value, MyEnum.Flag)` 调用点特化方法体，等价于直接写 `(value & flag) == flag`。编译器不添加特殊语法糖。

#### D4.2 `IsDefined`

`IsDefined` 编译期烘焙为 `switch (value) { case Member1: return true; ... default: return false; }`——已知枚举的所有成员值，编译期生成 switch 穷举比较。

#### D4.3 `GetNames` / `GetValues`

`GetNames` 编译期烘焙为 `new string[] { "Member1", "Member2", ... }`。
`GetValues` 编译期烘焙为 `new T[] { T.Member1, T.Member2, ... }`。

两者均**编译期固定数组**（`GlobalArray` 或 `new []` 字面量），零反射。

### D5. 约束与边界

- 位运算仅限**同一枚举类型**（`E | E` → `E`）。不同枚举类型间位运算为编译错误。
- 枚举与 `int` 之间**不自动转换**——`(int)E.Member` 显式转换。
- `[Flags]` 为可选标记，不标记的枚举也可参与位运算（C# 与 Rust 均如此）。
- 复合赋值 `|=` `&=` `^=` 仅对枚举和数值类型有效（非布尔，非引用类型）。
- `GetNames`/`GetValues` 返回数组为**新分配**，调用方不应缓存（对齐 C# `Enum.GetValues` 行为）。

## 代码示例

### Flags 枚举 + 位运算

```as
using Arc;

[Flags]
public enum Permissions {
    None      = 0,
    Read      = 1,
    Write     = 2,
    Execute   = 4,
    All       = Read | Write | Execute,
}

void Main() {
    Permissions p = Permissions.Read | Permissions.Write;
    Permissions p2 = p | Permissions.Execute;

    // 复合赋值
    Permissions p3 = Permissions.None;
    p3 |= Permissions.Read;
    p3 |= Permissions.Write;

    // HasFlag
    bool canRead = Enum.HasFlag(p3, Permissions.Read);
    Console.WriteLine(canRead ? "can read" : "cannot read");

    // IsDefined
    bool defined = Enum.IsDefined(Permissions.Read);  // true
    bool defined2 = Enum.IsDefined((Permissions)999);  // false

    // GetNames / GetValues
    string[] names = Enum.GetNames<Permissions>();
    Permissions[] values = Enum.GetValues<Permissions>();
}
```

## 实现分解

| 组件 | 变更 | 验收 |
|------|------|------|
| lexer | 新增 `|=` `&=` `^=` token | `cargo test` |
| parser | `try_parse_compound_assign` 扩展 | `cargo test` + compound_assign_e2e |
| typeck | `check_expr.rs` 位运算支持 enum；`operator_overload.rs` 扩展 `is_builtin_binary_path` | `cargo test` + enum_flags_e2e |
| std | `FlagsAttribute.as` | 编译通过 |
| std | `Enum.as` 扩展 `HasFlag`/`IsDefined`/`GetNames`/`GetValues` | 编译通过 + e2e |
| e2e | `enum_flags_e2e.rs` 覆盖全部场景 | `cargo test -p arc-integration` |

---

[返回 004 子项索引](index.md) · [返回 RFC 索引](../../index.md)