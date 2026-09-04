# RFC 007 集合、字符串与数值

## 背景

定义 Arc 的语言级集合表达式、字符串插值与数值类型面。目标：容器操作零成本、字符串操作可预测、数值类型明确。

## 设计决策

### 集合表达式（`[...]` 唯一惯用法）

| 形式 | 示例 | 元素类型来源 | `var` 可用 |
|------|------|--------------|------------|
| 集合表达式 `[...]` | `int[] nums = [1, 2];` | 声明中的 `T[]` 或元素推导 | 是（`var v = [1, 2]` → `int[]`） |
| spread `..` | `var all = [..a, ..b];` | 展开既有集合 | 是 |
| 嵌套 | `var m = [[1, 2], [3, 4]];` | 元素推导 | 是 |
| 对象构造 | `new User()` / `new User() { Name = "Bob" }` | `new` 后类型 | 是 |

**硬拒绝**：`new T[] { }`、`new[] { }`、前导类型 + `{ }`、裸 `{ }`——均须改用 `[...]`。

### 集合类型

| 类型 | 说明 |
|------|------|
| `T[]` | 固定长度数组；元素类型 invariant |
| `Arc.Collections.List<T>` | 动态数组；`list[i]` / `list[i]=v` / `Add` / `Count` / `foreach` |
| `Arc.Collections.Dictionary<K,V>` | 关联表；`dict[k]` / `dict[k]=v` / `Contains` |

```as
using Arc.Collections;
Dictionary<string, int> counts = new Dictionary<string, int>();
counts["alpha"] = 1;
int v = counts["alpha"];
bool has = counts.Contains("alpha");

List<int> nums = new List<int>();
nums.Add(10);
nums.Add(20);
nums[0] = 11;
int sum = 0;
foreach (var n in nums) {
    sum = sum + n;
}
```

- `List<T>` 为编译器内置 facade（方法体空，实现位于运行时 `rt_list_*`），首批完整支持 `List<int>` / `List<string>` 单态化；引用类型元素由 codegen 自动维护 ARC。
- 索引器 `list[i]` 在 MIR 降为 `get_Item`/`set_Item`，codegen 直访 buffer（无 `rt_list_get` 调用/alloca）；`dict[k]` 内联 `rt_dict_*`。
- `foreach` 对 `List<T>` 脱糖为索引循环（`get_Count` + `Get(idx)`），零迭代器对象分配。
- 内部缓冲区扩容机制见 [内存模型与资源安全](005-memory-model.md) 末尾一句。

### 字符串

- 编码：**UTF-8 码元**（非 C# UTF-16）；`s[i]` → `char`（同码元单位）。
- `+` 拼接；`==`/`!=` 比较；`.Length` 返回 `int`（UTF-8 码元/字节数）；`.Compare(a, b)` 返回 `int`。
- 只读索引越界返回 `'\0'`。
- **`$"..."` C# 风格插值**：对齐 / 格式说明符 / verbatim；编译期脱糖为
  `new StringBuilder().Append(...)...ToString()` 链（分段追加摊还 O(n)，避免
  `string + string` 链每次整串拷贝的 O(n²)）；零洞（纯字面量）直接折叠为常量串。

```as
string name = "Arc";
string greeting = $"Hello, {name}!";
```

- 操作：`Split` / `Join` / `Substring` / `Trim` / `Pad` / `Compare` / `StartsWith` / `EndsWith` 等。
- `StringBuilder` 经 `rt_text_sb_*`。

### 数值类型

| 类型 | 说明 |
|------|------|
| `int` | 32 位有符号整数 |
| `float` | 32 位 IEEE 754 单精度 |
| `double` | 64 位 IEEE 754 双精度 |
| `bool` | 布尔 |

- 浮点字面量默认 `double`；`float` 经显式声明与隐式收窄获得。
- **无数字字面量后缀**（`f`/`L`/`u`/`m`）与数字分隔符（`_`）；需显式类型转换（如 `(float)1.5`）。
- `decimal` 非编译器原语，由 `std.Decimal` 结构体（打包尾数 + scale + 运算符重载）提供。

**数值隐式转换**（赋值/参数/返回值场景）：`int→float`、`int→double`、`float→double` 为安全拓宽；`double→float` 为收窄；`double→int`、`float→int` **禁止隐式**（需显式 `as`）。

**算术提升**：Binary 运算任一操作数为 `double` → `double`；否则任一为 `float` → `float`；否则 `int`。

### Generic Math（数值泛型零装箱）

通过 `static abstract` 接口成员实现数值泛型零装箱：

- `INumber<T>` / `IAddable` / `ISubtractable` / `IMultiplicable` / `IDivisible` + `IEquatable` / `IHashable` / `IComparable`。
- 基元类型编译器内置隐式实现。
- 单态化后直接 LLVM 指令，零虚分派、零装箱。
- 运算符重载用 `T.Add` 方法形式，不引入 `operator+` 语法糖双轨。

## 边界

- 集合内部缓冲区、元素级 ARC 见 [内存模型与资源安全](005-memory-model.md)。
- 运算符语法见 [词法与语法](003-lexicon-syntax.md)。
- 字符串插值的文化感知格式化**永拒**（`FormattableString`）。

---

上一节：[006 对象模型](006-object-model.md) · 下一节：[008 委托、闭包与方法组](008-delegates-closures.md)