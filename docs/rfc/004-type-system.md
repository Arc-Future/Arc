# RFC 004 类型系统

## 背景

Arc 类型系统在编译期完成推断、检查与单态化。编译通过的程序具备类型安全——无隐式危险转换、无未检查的可空访问、无运行时类型错误。

## 设计决策

### 基元类型

| 类型 | 说明 |
|------|------|
| `void` | 无值；仅作返回类型 |
| `int` | 32 位有符号整数 |
| `float` | 32 位 IEEE 754 单精度浮点 |
| `double` | 64 位 IEEE 754 双精度浮点 |
| `bool` | 布尔 |
| `string` | UTF-8 字符串句柄；`+` 拼接；`==`/`!=`；`.Length` 返回 `int`（**UTF-8 码元/字节数**）；只读索引 `s[i]` → `char`（同码元单位，越界 `'\0'`） |
| `object` | 所有引用类型的基类；值类型赋值给 `object` 时自动装箱 |

浮点字面量默认 `double`（C# 惯例）；`float` 变量通过显式声明获得，赋值时发生隐式收窄。

**数字字面量设计**：Arc **不支持**数字字面量后缀（`f`/`L`/`u`/`m`）与数字分隔符（`_`），需通过显式类型转换（如 `(float)1.5`）使用相关类型。`decimal` 不作为编译器原语，通过 `std.Decimal` 结构体（打包尾数 + scale + 运算符重载）提供，领域逻辑归 std、不领域倒逼语言层改洞。

### 字面量类型

| 字面量 | 类型 | 示例 |
|--------|------|------|
| 整数字面量 | `int` | `42`、`0`、`-1` |
| 浮点字面量 | `double` | `3.14`、`0.5` |
| 布尔字面量 | `bool` | `true`、`false` |
| 字符串字面量 | `string` | `"hello"`、`@"c:\path"` |
| `null` 字面量 | `T?`（上下文推断） | `null` |

### 禁止的类型

| 类型 | 状态 | 说明 |
|------|------|------|
| `(T1, T2, ...)` 元组 | **禁止** | parser 层直接报错（`expected "-> (tuple types not supported)"`） |
| `unsafe` 裸指针 | **禁止** | 无 `unsafe` 块，无绕过 borrowck 路径 |

（值类型可空 `int?`/`double?` 非禁止类型，其布局与装箱语义见下文「值类型视图 ABI」。）

### 命名类型

用户定义的 `struct`、`class`、`interface`、`enum` 构成命名类型，类型名在声明作用域内唯一。`enum` 底层类型为 `int32_t`，discriminant 从 0 起按声明顺序递增，可通过显式 `= N` 覆盖：

```as
enum Color {
    Red,    // 0
    Green,  // 1
    Blue = 5,
}
```

**枚举能力增强**：位运算（`|` `&` `^` `~`，结果类型为原枚举）、复合赋值（`|=` `&=` `^=`）、`[Flags]` 位域特性，以及编译期烘焙的 `Enum.HasFlag`/`IsDefined`/`GetNames`/`GetValues` 工具方法——设计详见 [枚举能力增强](004-type-system/references/enum-capabilities.md)。

### variant（和式类型）

`variant` 是**值类型和式类型**（标签联合）：栈分配 `{ tag, payload }` 布局，零装箱，payload 按最大 case 内联。访问控制与 `struct`/`class`/`enum` 一致（`public variant` / `internal variant`）：

```as
public variant SetterValue {
    | String of string
    | Number of double
    | Boolean of bool
    | Element of Element
    | Binding of Binding
    | StaticResource of string
    | TemplateBinding of string
}
```

**case 声明两种形式并存**（语义等价，均合法）：

| 形式 | 语法 | 适用 |
|------|------|------|
| 单载荷 | `\| CaseName of PayloadType` | 每 case 携带一个值 |
| 元组多载荷 | `CaseName(T1, T2, ...)` | 每 case 携带多个值，如 `variant Result<T,E> { Ok(T), Err(E) }` |

**构造**：

- **隐式构造**：赋值/传参处载荷类型唯一可辨识时，typeck 自动重写为 `VariantName.CaseName(payload)`（如 `setter.Value = "Red"` → `SetterValue.String("Red")`）；载荷类型不唯一时歧义，编译期报错。
- **显式构造**：`VariantName.CaseName(payload)`。
- **泛型**：支持泛型参数，编译期**单态化**，无运行时类型擦除（对齐上文「泛型」）。

**匹配与穷尽（与 C# 一致）**：

- 匹配用 `switch` 语句/表达式，`case VariantName.CaseName(payloadVar):` 在分支内绑定载荷变量（对齐 `is T name` 类型窄化），另有 `default`。
- `switch` **表达式**必须穷尽：覆盖全部 case 或含 `default`，否则编译期报错。
- `switch` **语句**不强制穷尽：未匹配落入 `default`，无 `default` 则无操作。
- `default` 存在时穷尽要求被豁免。

**与 `enum` / `record` / `class` 的边界**：

| 类型 | 定位 | 载荷 | 用途 |
|------|------|------|------|
| `enum` | 无载荷判别类型（底层 `int32_t`） | 无 | 封闭命名常量集 |
| `variant` | 带载荷和式类型（标签联合） | 每 case 可携带一/多个值 | 结果/选项/指令/表达式等可穷尽和式 |
| `record` | 引用类型（`record struct` 为值类型） | 位置参数 | 具名多字段数据（见 [对象模型](006-object-model.md)） |
| `class`/`interface` | 开放继承层次 | 方法/字段 | 多态对象（见 [对象模型](006-object-model.md)） |

`variant` 的赋值/传参默认**移动**、按字段逆序析构，对齐 [内存模型与资源安全](005-memory-model.md) 的 `struct` 值类型语义；内存布局细则见 005。

可穷尽封闭集合由 `variant`（带载荷和式）与 `enum`（无载荷判别）单一惯用法承载；C# 15 的 `closed` 封闭层次修饰符**不引入**（`sealed` 仅存在于反射元数据，见 [对象模型](006-object-model.md) 禁止项）。

### 引用与借用（语义）

Arc 用户源码**无** `&T` / `&mut T` 表面语法。编译器在 borrowck 内部追踪不可变与可变借用，规则见 [内存模型与资源安全](005-memory-model.md)。

### 函数类型与回调

函数类型使用 C# 风格委托 `Func<T, R>`、`Action<T>`：

```as
int apply(Func<int, int> f, int x) {
    return f(x);
}
```

方法在类型系统中视为首个参数为接收者的函数。方法组（签名兼容的自由函数名、静态 `C.Foo`、实例 `obj.Foo`）可作委托值，typeck 脱糖为 lambda。详见 [委托、闭包与方法组](008-delegates-closures.md)。

### 泛型

泛型参数在类型名或函数名后声明，编译期**单态化**，每个 `(T)` 实例化产生独立代码，无运行时类型擦除：

```as
struct Box<T> {
    public T Value;
}

T identity<T>(T value) {
    return value;
}
```

**泛型约束（where 子句）**：

| 约束形式 | 语义 |
|---------|------|
| `where T : IInterface` | 接口约束（含泛型接口） |
| `where T : BaseClass` | 基类约束 |
| `where T : class` | 引用类型约束 |
| `where T : struct` | 值类型约束 |
| `where T : new()` | 构造约束；须为同 param 最后一个约束 |
| `where T : A, B` | 多约束组合，全部满足 |

约束违规时编译期报 `ConstraintNotSatisfied`。基元类型对内置接口（`IComparable`/`IEquatable`）隐式满足。

### 集合

| 类型 | 说明 |
|------|------|
| `T[]` | 固定长度数组；元素类型 **invariant**（拒 C# 数组协变） |
| `T[][]` | 交错数组（数组的数组）；`new T[n][]` 分配外层，内层数组各自 `new T[k]` 填充；元素为 `T[]` 指针，复用一维数组 ABI |
| `Arc.Collections.List<T>` | 动态数组；`list[i]` / `list[i]=v` / `Add` / `Count` / `foreach` |
| `Arc.Collections.Dictionary<K,V>` | 关联表；`dict[k]` / `dict[k]=v` / `Contains` |

索引器 `list[i]` 在 MIR 降为 `get_Item`/`set_Item`，codegen 直访 buffer；`foreach` 对 `List<T>` 脱糖为索引循环，零迭代器对象分配。详见 [集合、字符串与数值](007-collections-strings-numerics.md)。

多维矩形数组 `T[,]`（`Rank` / `GetLength(dim)` / `a[i,j]` 索引）**尚未支持**：需新 rank 元数据与连续矩形内存 ABI，另立 RFC。当前矩阵/数值场景以交错数组 `T[][]` 为推荐形态。

### 异步类型

异步函数返回 `Task<T>`，表示尚未完成或已完成的异步计算；`Task<void>` 表示无返回值的异步过程。详见 [异步与并发模型](009-async-concurrency.md)。

### 查询类型

| 类型 | 语义 |
|------|------|
| `IEnumerable<T>` | 可枚举序列；Query 走 Enumerable 路径 |
| `IQueryable<T>` | 可查询数据源；Query 走 Queryable 路径 |
| `Expression<Func<...>>` | 表达式树包装的函数类型 |

双路径分派由接收者静态类型决定。详见 [表达式树与查询语言](011-expression-trees-query.md)。

### 类型推断

`var` 从初始化表达式推断类型：

```as
var n = 42;        // int
var s = "Arc";     // string
var t = load();    // 由 load 返回类型决定
```

下列场景**不得**使用 `var`：
- 无初始化器的声明
- 推断结果依赖后续语句
- 公共 API 签名（必须显式类型）

### 错误类型与 `?`

可失败操作返回包含错误信息的类型。`?` 运算符在错误时提前返回，成功时解包值：

```as
var v = load()?;
```

### 可空类型与流分析

| 语法 | 语义 |
|------|------|
| `T?` | 可空标注 |
| `null` | 空字面量 |
| `??` | 空合并 |
| `?.` | 空条件访问 |
| `!.` | 强制解引用 |

编译时流分析收窄（`NullFlowState` → `TypeFlowState`），目标「编译通过 ⇒ 运行时不 NRE」。空条件赋值 `a?.B = x` 一次求值。值类型可空 `T?`（如 `int?`）为内联 `{ HasValue, Value }` 布局，`??`/`==`/装箱语义见上文「值类型视图 ABI」；引用类型可空 `T?`（如 `string?`）为 `ptr`（`null` 或句柄）。

### 模式匹配

| 模式 | 语义 |
|------|------|
| `is` 表达式 | `expr is Type` / `expr is Type name` / `expr is null` / `expr is <literal>`（常量模式） |
| 常量模式 | `is 5` / `is "a"` / `is true` / `is 'c'`；匹配语义为**值相等** `==`（数值 `icmp`，string `rt_str_equals`，char 按判别值；对齐 C# 常量模式，非引用相等） |
| 类型窄化 | `if (x is T n)` then 分支窄化 |
| switch 语句/表达式 | 类型 / 常量 / null / var / variant（定义见上文「variant（和式类型）」）模式 + `when` |
| 解构赋值 | `(x,y)=e`；弃元 `_`；声明式 `var (x,y)=e`；位置模式 |

switch 表达式为**单一惯用法**（Rust `match` 不复活）。switch **表达式**必须穷尽（覆盖全部 case 或含 `default`，否则编译期报错）；switch **语句**不强制穷尽（未匹配落入 `default`，无 `default` 则无操作）。运行时类型判断经 `rt_obj_isa`（vtable slot0 = typeinfo）。解构**不是**元组类型——`(T1,T2)` 类型仍禁止。

### 类型相等与转换

- 命名类型同一性由名称与泛型参数结构决定。
- 子类型关系由 `class` 继承与 `interface` 实现建立（见 [对象模型](006-object-model.md)）。
- 禁止隐式危险转换；必要转换须显式。

**数值隐式转换**：

| 源类型 | 目标类型 | 性质 |
|--------|----------|------|
| `int` | `float` | 安全拓宽 |
| `int` | `double` | 安全拓宽 |
| `float` | `double` | 安全拓宽 |
| `double` | `float` | 收窄（无 `f` 后缀，`float` 变量经隐式收窄） |
| `double` | `int` | **禁止**隐式；需显式 `as` |
| `float` | `int` | **禁止**隐式；需显式 `as` |

Binary 算术（`+`/`-`/`*`/`/`/`%`）结果类型：任一操作数为 `double` → `double`；否则任一为 `float` → `float`；否则 `int`。

### 值类型视图 ABI（装箱 / 接口 / 可空 / enum）

值类型（基元、`struct`、`enum`）可经**三个视图**进入引用世界：`object`（装箱视图）、`interface`（接口视图）、`T?`（可空视图）。三视图共用**同一装箱机制**——codegen 单一 `value_type_box`/`value_type_unbox` 管线，禁止三处分别 patch。深度布局、分派细则与实现分解见 [值类型视图 ABI 深度设计](004-type-system/references/value-type-view-abi.md)。

#### 装箱视图（值类型 → `object`）

- 值类型赋值给 `object` 时**自动装箱**（隐式，无需显式 `box`，对齐 C#）。
- 布局：`ArcHeader`（`refcount = 1`、`vtable = 装箱类型 typeinfo`）+ typeinfo + 内联值拷贝（逐字段浅拷贝，内嵌 `class` 句柄随拷贝 `rt_arc_inc`）。
- 装箱是**读 + 拷贝**（非移动），源值装箱后仍可用。
- 拆箱：显式 `(T)obj` 经 typeinfo 校验后取出内联值拷贝，不匹配编译期/运行时报错；`o is T` 经 typeinfo 判定（对齐 `rt_obj_isa`）。

#### 接口视图（值类型 → `interface`）

- 值类型赋值给 `interface` 时自动装箱为**堆盒**，构造 fat pointer `{ 盒指针, @.itable.{装箱类型}_{接口} }`，盒上 itable 槽位指向值类型的接口方法实现。
- **约束调用 vs 装箱调用**（对齐 .NET `constrained.`）：静态类型已知为值类型且实现接口方法 → 发射**约束调用**（byref 直调、零装箱）；静态类型仅为 `object`/接口、须运行时择实现 → **装箱调用**。两者共享同一 fat pointer 形态与 itable 槽位布局，无第二套分派机制。
- **动态 downcast**（`object → (Iface)`）：`object o = square; (IShape)o` 经 `rt_obj_to_iface` 读 boxed typeinfo 的 `interface_itables`（`RtTypeInfo` 追加字段）恢复接口；失败抛 `InvalidCastException`（`rt_panic`），非崩溃；`(I)null` → null。静态 `IShape i = square` 仍走固定 itable `MakeIface`（零成本，不受影响）。

#### 可空视图（`T?` 值类型）

- `T?`（`T` 为值类型）为**内联值类型**，布局 `{ bool HasValue; T Value; }`（对齐 .NET `Nullable<T>`）。
- `int? a = 42;` → `{ HasValue = true, Value = 42 }`；`int? a = null;` → `{ HasValue = false }`。
- `a ?? d` → `a.HasValue ? a.Value : d`（无指针解引用）；`a == b`/`a != b` 逐字段比较。
- 装箱：`(int?)42` 装箱为 `int` 盒；`(int?)null` 装箱为 `null`（对齐 C#「boxed `Nullable<T>` ≡ boxed `T` / `null`」恒等式）。
- 引用类型可空 `string?` 仍为 `ptr`（`null` 或句柄），与值类型可空区分，不再混用「指针装箱」表示。

#### enum 哈希/相等

- `enum` 隐式满足 `IEquatable<E>` / `IHashable<E>`（值语义，discriminant = `int32`）。
- `E == E` 为判别值比较；`GetHashCode` 返回判别值（标量哈希）。
- `Dictionary<E, V>` 零装箱：键走标量 `rt_hash_int`/`rt_eq_int` 快路径，与基元键一致。

#### 与移动语义 / Copy 的交互

`struct` 赋值默认移动（[005](005-memory-model.md)）。装箱**不移动源值**，而是对值类型执行一次**隐式 Copy**（逐字段浅拷贝 + 内嵌句柄 `rt_arc_inc`）——该 Copy 语义与 `record struct` 合成拷贝同源，是值类型进入引用世界的唯一边界操作；装箱后源 `struct` 仍可用（不触发 `UseAfterMove`）。

## 边界

- 所有权、移动、借用与生命周期见 [内存模型与资源安全](005-memory-model.md)。
- class/interface/record 层次见 [对象模型](006-object-model.md)。
- 数值字面量细节见 [集合、字符串与数值](007-collections-strings-numerics.md)。
- 值类型装箱/接口/可空 ABI 的深度布局、约束调用分派、统一管线与实现分解见 [004 references](004-type-system/references/index.md)。

---

上一节：[003 词法与语法](003-lexicon-syntax.md) · 下一节：[005 内存模型与资源安全](005-memory-model.md)