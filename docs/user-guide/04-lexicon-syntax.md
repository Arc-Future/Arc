# 04 词法与语法

本节定义 Arc 源文件的词法单元与句法结构。所有示例均为 normative 表面语法；语义见后续各节。

**编码与命名对照**（C# 采纳 / Rust 拒绝）见 [03 编码与语法标准](03-encoding-standard.md)。

## 词法

### 源文件

- 扩展名：**`.as`**（规范与工具链唯一扩展名）
- 编码：UTF-8
- 换行：`\n` 或 `\r\n`（编译器归一化）

### 标识符与关键字

标识符：`[_a-zA-Z][_a-zA-Z0-9]*`

保留关键字（节选）：

```
void int bool string var struct class record interface enum
public private protected internal static operator
async await Task
if else while for foreach switch case default break continue return
new expression from where orderby select
with
```

注：`where` 兼具 LINQ 查询子句与泛型约束子句双重语义。后者形式为 `class C<T> where T : IComparable<T>`。`operator` 用于用户运算符重载。

`break` / `continue`：作用于最近一层 `while` / `for` / `foreach`（C# 对齐）。switch case 末尾的 `break` 由解析器消费、不入语句 AST。

内置复合赋值 `+=` / `-=` / `*=` / `/=`：语句形，脱糖为 `lhs = lhs op rhs`；适用于局部、字段、属性与索引器左值；**禁止**用户 `operator+=`。用户 `operator +` 等经脱糖可参与 `+=`。与 `++` / `--` 同一惯用法。

用户运算符重载：`public static T operator +(T a, T b)` 等；解析归一为 `op_*` 静态方法；允许 `+ - * / %`、一元 `-`、`== !=`；**仍拒**转换运算符与 `true`/`false`。

`record` / `record struct` 与后缀 `with { … }`、位置 `Deconstruct` 见 [07 对象模型](07-object-model.md)。解构赋值 `(x, y) = expr`、弃元 `_`、声明式 `var (x, y) = expr`、位置模式 `is`/`switch`（含 switch 表达式）见 [07 对象模型](07-object-model.md)。`init` 访问器见 [07 对象模型](07-object-model.md)。

### 字面量

| 类别 | 示例 |
|------|------|
| 整型 | `0`, `42`, `-1` |
| 布尔 | `true`, `false` |
| 字符串 | `"Hello, Arc"` |

### 注释

```as
// 行注释

/* 块注释 */
```

## 前导类型句法

Arc 在所有类型位置采用**类型在前**：

```as
int count = 0;
string message = "ready";
void Main() { }
Task<int> compute();
IEnumerable<User> loadUsers();
```

函数声明：

```as
ReturnType name(ParamType param1, ParamType param2) {
    // 函数体
}
```

## 顶层声明

```as
struct Point {
    public int X;
    public int Y;
}

interface Drawable {
    void Draw();
}

class Sprite : Drawable {
    public int X;
    private int Y;

    public Sprite(int x, int y) {
    }

    public void Draw() {
        Console.WriteLine("draw");
    }
}

enum Status {
    Idle,
    Running,
    Done,
}
```

## 语句

```as
void demo() {
    int x = 1;
    x += 1;       // 内置复合赋值：脱糖为 x = x + 1
    x = x + 1;

    if (x > 0) {
        Console.WriteLine("positive");
    } else {
        Console.WriteLine("non-positive");
    }

    while (x > 0) {
        x -= 1;
    }

    foreach (var item in collection) {
        Console.WriteLine(item);
    }

    // `lock` 语句糖 → Monitor.Enter/Exit + try/finally
    Lock gate = new Lock();
    lock (gate) {
        x = x + 1;
    }

    return;
}
```

## 表达式

```as
var sum = a + b;
var ok = flag && other;
var created = new Rectangle(10, 20);
var result = compute()?;   // 错误传播，见[类型系统](05-type-system.md)
```

## Query comprehension 语法

```as
var query = from x in source
            where x.Active
            orderby x.Name
            select x.Name;
```

等价于 LINQ 方法链（见[查询语言](09-query-language.md)）。

## 表达式树（`Expression<T>`）

Queryable 路径要求 Lambda 的类型为 `Expression<Func<...>>`，或使用前导类型声明：

```as
Expression<Func<User, bool>> pred = u => u.Age >= 18;
var chain = users.Where(u => u.Active).Select(u => u.Name);  // IQueryable 链
```

普通 Lambda（无 `Expression<T>` 类型）用于 Enumerable 路径的运行时委托。

## 异步函数

```as
async Task<int> fetch() {
    return 42;
}

async Task<void> main() {
    var v = await fetch();
}
```

详见[异步与任务](08-async-tasks.md)。

## 语法摘要

| 构造 | 形式 |
|------|------|
| 函数 | `Ret name(Params) { Body }` |
| 方法 | `Access Ret name(Params) { Body }` |
| 字段 | `Access Type name;` |
| 局部 | `Type name = expr;` 或 `var name = expr;` |
| Query | `from ... in ... where ... select ...` |

---

上一节：[03 编码与语法标准](03-encoding-standard.md) · 下一节：[05 类型系统](05-type-system.md)