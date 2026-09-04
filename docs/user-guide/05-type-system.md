# 05 类型系统

Arc 类型系统在编译期完成推断、检查与单态化。`crates/typeck` 实现本规范。

## 基本类型

| 类型 | 说明 | C 后端映射 |
|------|------|------------|
| `void` | 无值；仅作返回类型 | `void` |
| `int` | 32 位有符号整数 | `int32_t` |
| `float` | 32 位 IEEE 754 单精度浮点 | `float` |
| `double` | 64 位 IEEE 754 双精度浮点 | `double` |
| `bool` | 布尔 | `int32_t`（0/1） |
| `string` | UTF-8 字符串句柄；`+` 拼接；`==`/`!=` 比较；`.Length` 返回 `int`（**UTF-8 码元/字节数**）；只读索引 `s[i]` → `char`（同码元单位，越界 `'\0'`；对齐 C# `string[i]` 表面，存储为 UTF-8 而非 UTF-16）；`string.Compare(a,b)` 返回 `int`（经 `rt_str_*`） | `const char*` |
| `object` | 所有引用类型的基类；值类型赋值给 `object` 时自动装箱 | `void*` |

浮点字面量默认 `double`（C# 惯例）：`3.14` 类型为 `double`。`float` 类型变量通过显式声明获得，赋值时发生隐式收窄。

> **设计决策**：Arc **不支持**数字字面量后缀（如 `f`/`L`/`u`/`m`）与数字分隔符（`_`），需通过显式类型转换（如 `(float)1.5`）使用相关类型。此符合「显式 > 隐式」准则，**不添加**后缀支持。
>
> `decimal` 类型**不作为编译器原语**：通过 `std.Decimal` 结构体（打包尾数 + scale + 运算符重载）提供，满足金融域需求，不领域倒逼语言层改洞（架构红线：领域逻辑归 std，编译器仅提供通用机制）。

## 字面量

| 字面量 | 类型 | 示例 |
|--------|------|------|
| 整数字面量 | `int` | `42`、`0`、`-1` |
| 浮点字面量 | `double` | `3.14`、`0.5`、`-2.718` |
| 布尔字面量 | `bool` | `true`、`false` |
| 字符串字面量 | `string` | `"hello"`、`"with \"escape\""`、`@"c:\path"`（`""` → `"`；`\` 字面；可多行） |
| `null` 字面量 | `T?`（上下文推断） | `null` |

## 禁止的类型

| 类型 | 状态 | 说明 |
|------|------|------|
| `(T1, T2, ...)` 元组 | 禁止 | project_memory 硬约束；parser 层直接报错 `expected "-> (tuple types not supported)"` |
| `int?`/`double?` 值类型可空 | 已实现（指针装箱） | 通过指针装箱支持值类型可空：`null` → `null ptr`，非空值 → 指向栈分配值的指针；codegen 自动处理装箱/拆箱与 null 检查 |
| `unsafe` 裸指针 | 禁止 | MVP 不提供 `unsafe` 块，无绕过 borrowck 路径 |

## 命名类型

用户定义的 `struct`、`class`、`interface`、`enum` 构成命名类型。类型名在声明作用域内唯一。

```as
struct Vec2 {
    public int X;
    public int Y;
}

class Node {
    public int Value;
}
```

### `enum` 底层类型

`enum` 的底层类型为 `int32_t`（C 后端映射）。discriminant 从 0 起按声明顺序递增，可通过显式 `= N` 覆盖。`TypeId` 中 `enum` 无独立变体，通过 `TypeId::Named(name)` + `layouts.enums.contains(name)` 判定。

```as
enum Color {
    Red,    // 0
    Green,  // 1
    Blue = 5,
}
```

## 引用与借用（语义）

Arc 用户源码**无** `&T` / `&mut T` 表面语法。编译器在 `borrowck` 内部追踪不可变与可变借用，规则见[内存与资源](06-memory-resources.md)。

## 函数类型

函数类型使用 C# 风格委托，如 `Func<T, R>`、`Action<T>`：

```as
int apply(Func<int, int> f, int x) {
    return f(x);
}
```

方法在类型系统中视为首个参数为接收者的函数。

**方法组**：期望 `Func`/`Action` 时，可将签名兼容的**自由函数名**、**静态 `C.Foo`**、**实例 `obj.Foo`**、**简单接收者扩展方法组**（无括号）用作委托值；typeck 脱糖为 lambda（实例/扩展组捕获走 lambda 捕获）。**硬拒绝立宪**：复杂实例接收者（`new`/嵌套 Field）、命名空间限定静态 `Ns.Type.Foo`、`Expression<>` 方法组、泛型扩展组。捕获 Func 跨函数实参仍受 ABI 缺口限制（与显式捕获 lambda 相同）。签名不匹配与未定义名硬错误，禁止静默。

**可选参数边界**：`TypeId::Func` **不**携带形参默认槽。带默认的 lambda 仅允许立即调用（IIFE）；赋给 `Func`/`Action`、作实参/返回值或入表达式树为**硬错误**。经委托变量省略实参须独立演进，不在当前范围。

## 泛型

泛型参数在类型名或函数名后声明，编译期单态化：

```as
struct Box<T> {
    public T Value;
}

T identity<T>(T value) {
    return value;
}
```

单态化后每个 `(T)` 实例化产生独立代码，无运行时类型擦除。

### 泛型约束（where 子句）

泛型类型参数可通过 `where` 子句施加约束，约束在泛型实例化时由 typeck 校验。支持的约束种类：

| 约束形式 | 语义 | 校验机制 |
|---------|------|----------|
| `where T : IInterface` | 接口约束（含泛型接口） | `is_subtype` 判定 |
| `where T : BaseClass` | 基类约束 | `is_subtype` 判定 |
| `where T : class` | 引用类型约束 | `is_reference_type` 判定 |
| `where T : struct` | 值类型约束 | `is_value_type` 判定 |
| `where T : new()` | 构造约束 | `NominalType.constructors` 元数据查询；值类型隐式满足；`new()` 必须是同 param 最后一个约束 |
| `where T : A, B` | 多约束组合 | 多 TypeConstraint 共享 param，全部满足 |

约束违规时报 `ConstraintNotSatisfied`。基元类型对内置接口（IComparable/IEquatable）的隐式满足通过精确 mangle 后缀校验实现。详见 [13 标准库架构](13-standard-library.md)。

## 集合（MVP）

| 类型 | 说明 |
|------|------|
| `T[]` | 固定长度数组字面量与索引读写（MIR `IndexGet`/`IndexSet` → GEP±load/store）；**元素类型 invariant**（拒 C# 数组协变） |
| `Arc.Collections.List<T>` | 动态数组；C# 索引器 `list[i]` / `list[i]=v` → codegen **直访** `RtList.data`（bounds+GEP）；`Add`/`Count`；`foreach` |
| `Arc.Collections.Dictionary<K,V>` | 关联表；C# 索引器 `dict[k]` / `dict[k]=v`（`get_Item`/`set_Item` → 直连 `rt_dict_get`/`rt_dict_set`）；`Contains` |

```as
using Arc.Collections;
Dictionary<string, int> counts = new Dictionary<string, int>();
counts["alpha"] = 1;
int v = counts["alpha"];
bool has = counts.Contains("alpha");
```

`List<T>` 为编译器内置 facade（方法体空，实现位于运行时 `rt_list_*`）。首批完整支持 `List<int>`/`List<string>` 单态化，引用类型元素由 codegen 自动维护 ARC：

```as
using Arc.Collections;
List<int> nums = new List<int>();
nums.Add(10);
nums.Add(20);
nums[0] = 11;
int first = nums[0];
int sum = 0;
foreach (var n in nums) {
    sum = sum + n;
}
```

索引器 `list[i]` 在 MIR 降为 `get_Item`/`set_Item`，codegen **直访 buffer**（无 `rt_list_get` 调用/alloca）；`dict[k]` 内联 `rt_dict_*`。`foreach` 对 `List<T>` 脱糖为索引循环（`get_Count` + `Get(idx)`），零迭代器对象分配。

## `Task<T>`

异步函数返回 `Task<T>`，表示尚未完成或已完成的异步计算：

```as
async Task<int> load() { return 1; }
async Task<void> run() {
    var v = await load();  // v: int
}
```

`Task<void>` 表示无有意义返回值的异步过程。

## Query 相关类型

| 类型 | 语义 |
|------|------|
| `IEnumerable<T>` | 可枚举序列；Query 走 Enumerable 路径 |
| `IQueryable<T>` | 可查询数据源；Query 走 Queryable 路径 |
| `Expression<Func<...>>` | 表达式树包装的函数类型 |

双路径分派由接收者静态类型决定：

```as
// typeck 选择 LinqPath::Enumerable
IEnumerable<User> users = ...;
users.Where(u => u.Active);

// typeck 选择 LinqPath::Queryable；普通 => 按接收者/形参树化（无 expression 关键字）
IQueryable<User> query = ...;
query.Where(u => u.Active);
```

## 类型推断

`var` 从初始化表达式推断类型：

```as
var n = 42;        // int
var s = "Arc";     // string
var t = load();    // 由 load 返回类型决定
```

下列场景不得使用 `var`：

- 无初始化器的声明
- 推断结果依赖后续语句
- 公共 API 签名（必须使用显式类型）

## 错误类型与 `?`

可失败操作返回包含错误信息的类型（MVP 阶段与 `int` 或专用 Result 类型对齐，以实现为准）。`?` 运算符在错误时提前返回，成功时解包值。

## 类型相等与转换

- 命名类型同一性由名称与泛型参数结构决定
- 子类型关系由 `class` 继承与 `interface` 实现建立（见[对象模型](07-object-model.md)）
- 禁止隐式危险转换；必要转换须显式

### 数值隐式转换

数值类型在赋值、参数传递、返回值场景下允许以下隐式转换：

| 源类型 | 目标类型 | 性质 | 说明 |
|--------|----------|------|------|
| `int` | `float` | 安全拓宽 | C# 标准 |
| `int` | `double` | 安全拓宽 | C# 标准 |
| `float` | `double` | 安全拓宽 | C# 标准 |
| `double` | `float` | 收窄 | Arc 设计：无 `f` 字面量后缀，`float` 变量通过隐式收窄从 `double` 字面量获取值 |
| `double` | `int` | — | **禁止**隐式；需显式 `as` |
| `float` | `int` | — | **禁止**隐式；需显式 `as` |

Binary 算术运算（`+`/`-`/`*`/`/`/`%`）结果类型由 `numeric_promote` 决定：任一操作数为 `double` → `double`；否则任一为 `float` → `float`；否则 `int`。

## 与编译器 IR 的对应

| 规范概念 | `TypeId` 变体（typeck） |
|----------|-------------------------|
| `int`/`float`/`double`/`bool`/`string`/`void` | `Int`/`Float`/`Double`/`Bool`/`String`/`Void` |
| `object` 基类 | `Named("object")`（codegen 映射 `void*`） |
| `enum` | `Named(name)`（`layouts.enums` 判定） |
| `Task<T>` | `Task { inner }` |
| `IEnumerable<T>` | `IEnumerable { inner }` |
| `IQueryable<T>` | `IQueryable { inner }` |
| `Expression<...>` | `Expression { inner }`（编译器内部） |
| `T?`（引用类型可空） | `Nullable { inner }` |

---

上一节：[04 词法与语法](04-lexicon-syntax.md) · 下一节：[06 内存与资源](06-memory-resources.md)