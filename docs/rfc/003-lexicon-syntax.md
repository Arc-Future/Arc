# RFC 003 词法与语法

## 背景

定义 Arc 源文件的词法单元与句法结构。所有示例为 normative 表面语法；类型语义见 [类型系统](004-type-system.md)，编码对照见 [语法表面与编码标准](002-surface-contract.md)。

## 设计决策

### 词法

**源文件**：扩展名 `.as`；UTF-8 编码；换行 `\n` 或 `\r\n`（编译器归一化）。

**标识符**：`[_a-zA-Z][_a-zA-Z0-9]*`。

**保留关键字（节选）**：

```
void int bool string var struct class record interface enum
public private protected internal static operator
async await Task
if else while for foreach switch case default break continue return
new expression from where orderby select
with
```

说明：
- `where` 兼具 LINQ 查询子句与泛型约束子句双重语义（后者如 `class C<T> where T : IComparable<T>`）。
- `break` / `continue` 作用于最近一层 `while` / `for` / `foreach`；switch case 末尾的 `break` 由解析器消费、不入语句 AST。
- `record` / `record struct` 与后缀 `with { … }`、位置 `Deconstruct` 见 [对象模型](006-object-model.md)；**`record class` 已硬拒**。

**字面量**：

| 类别 | 示例 |
|------|------|
| 整型 | `0`, `42`, `-1` |
| 布尔 | `true`, `false` |
| 字符串 | `"Hello, Arc"`、`@"c:\path"`（verbatim：`""` → `"`、`\` 字面、可多行） |

**注释**：

```as
// 行注释
/* 块注释 */
```

### 前导类型句法

所有类型位置采用**类型在前**：

```as
int count = 0;
string message = "ready";
void Main() { }
Task<int> compute();
IEnumerable<User> loadUsers();
```

函数声明形式：`ReturnType name(ParamType param1, ParamType param2) { ... }`。

### 顶层声明

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

### 语句

```as
void demo() {
    int x = 1;
    x += 1;        // 内置复合赋值：脱糖为 x = x + 1
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

    Lock gate = new Lock();
    lock (gate) {          // lock 语句糖 → Monitor.Enter/Exit + try/finally
        x = x + 1;
    }

    return;
}
```

**内置复合赋值**（`+=` / `-=` / `*=` / `/=`）：语句形，脱糖为 `lhs = lhs op rhs`；适用于局部、字段、属性与索引器左值；**禁止**用户 `operator+=`。与 `++` / `--` 同一惯用法。

### 表达式

```as
var sum = a + b;
var ok = flag && other;
var created = new Rectangle(10, 20);
var result = compute()?;   // 错误传播，见类型系统
```

### 运算符重载

用户运算符仅允许 `+ - * / %`、一元 `-`、`== !=`，声明为 `public static T operator +(T a, T b)`，解析归一为 `op_*` 静态方法。**仍拒**转换运算符与 `true`/`false`。

### Query comprehension 语法

```as
var query = from x in source
            where x.Active
            orderby x.Name
            select x.Name;
```

等价于 LINQ 方法链（见 [表达式树与查询语言](011-expression-trees-query.md)）。

### 表达式树（`Expression<T>`）

Queryable 路径要求 Lambda 类型为 `Expression<Func<...>>` 或使用前导类型声明：

```as
Expression<Func<User, bool>> pred = u => u.Age >= 18;
var chain = users.Where(u => u.Active).Select(u => u.Name);  // IQueryable 链
```

普通 Lambda（无 `Expression<T>` 类型）用于 Enumerable 路径的运行时委托。

### 异步函数

```as
async Task<int> fetch() {
    return 42;
}

async Task<void> main() {
    var v = await fetch();
}
```

详见 [异步与并发模型](009-async-concurrency.md)。

### 语法摘要

| 构造 | 形式 |
|------|------|
| 函数 | `Ret name(Params) { Body }` |
| 方法 | `Access Ret name(Params) { Body }` |
| 字段 | `Access Type name;` |
| 局部 | `Type name = expr;` 或 `var name = expr;` |
| Query | `from ... in ... where ... select ...` |

## 边界

- 类型与约束见 [类型系统](004-type-system.md)。
- 运算符相关的数值提升规则见 [集合、字符串与数值](007-collections-strings-numerics.md)。
- 集合表达式 `[...]` 见 [集合、字符串与数值](007-collections-strings-numerics.md)。

---

上一节：[002 语法表面与编码标准](002-surface-contract.md) · 下一节：[004 类型系统](004-type-system.md)