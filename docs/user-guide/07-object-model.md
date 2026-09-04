# 07 对象模型

Arc 对象模型支持 `class`、`interface`、单继承与接口多实现。实现分散在 `crates/oop`（类型注册与访问检查）与 `crates/codegen`（vtable 布局）。

## 类

```as
class Rectangle {
    public int Width;
    private int Height;

    public Rectangle(int width, int height) {
        Width = width;
        Height = height;
    }

    public int Area() {
        return Width * Height;
    }
}
```

- 实例由 `new ClassName(...)` 在堆上创建
- 字段默认按声明顺序布局；访问控制由编译期检查

## Primary constructor（含 `: Base(args)` 与 `ref`/`out`/`in`）

对齐 C# 12 `class C(int x)` 表面，**与 `record` 解耦**。

```as
public class Point(int x, int y) {
    public int X() { return x; }
    public int Y() { return y; }
}

var p = new Point(3, 4);
Assert.Equal(3, p.X());

// ref/out/in：不字段捕获；可用于字段初始化器或 `: Base(args)`
public class RefHolder(ref int x) {
    public int Snapshot = x;
}
public class OutBase {
    public int N;
    public OutBase(out int n) { n = 42; N = n; }
}
public class OutDerived(out int x) : OutBase(out x) { }
```

**语义（parser 脱糖，下游复用显式构造器管线）**：

| 项 | 行为 |
|----|------|
| 声明位置 | 类名与泛型参数之后、基类列表之前：`class C<T>(T v) : Base` |
| 按值捕获 | 按值参数注入同名 `private` 实例字段；方法体可无限定名访问 |
| `ref`/`out`/`in` | **禁止捕获**（对齐 C# CS9109）；保留在合成 ctor 形参；可用于字段初始化器与 `: Base(args)`；实例成员中引用 → 未定义名硬错误 |
| 合成构造 | 可见性与类声明一致；按值参数体为 `this.param = param;`；by-ref 无赋值语句 |
| `: Base(args)` | 声明侧基类构造实参写入合成 ctor 的 `base_args`；typeck 与显式 `: base(...)` 同路径；无 primary 时禁止 `: Base(...)` |
| 调用 | `new C(args)` / `new C(ref v)` 等走既有 `__ctor` 路径；ctor 形参按 `TypeId::Ref` 指针 ABI |

**仍排除（编译错误或未实现）**：`static class` primary；扩展接收者 `this`；按值参数与同名字段/属性或同 arity 显式构造冲突；`record` / `struct` primary；按值参数的惰性捕获（本子集对按值参数**总是**捕获）。

## 记录类型（`record`）

`record` 是引用类型（**`record class` 同义拼写已硬拒**，单一惯用法）；`record struct` 是值类型。位置参数在解析期脱糖为公共 `{ get; init; }` 自动属性与构造器：

```as
record Point(int X, int Y);
record struct Vec2(int X, int Y);

void Main() {
    Point p = new Point(3, 4);
    Point q = p with { X = 10 };
    bool same = new Point(1, 2) == new Point(1, 2);
    int x; int y;
    p.Deconstruct(out x, out y);
    (x, y) = p; // 解构赋值：等价于上一行（非元组类型）
    var (a, _) = p; // 声明 + 弃元
    if (p is (var px, var py)) { /* 位置模式 */ }
    Vec2 v = new Vec2(5, 6);
    Console.WriteLine(p.X);
}
```

体形式亦可：`record Person { public string Name { get; set; } … }`（体成员与 `class` 同构）。

**合成成员**：`with` 浅拷贝并覆盖列出的实例字段/auto-init 属性（仅 record；ctor 重构）；合成 `Equals` / `GetHashCode`（同组实例字段；`* 31` 组合）；同类型 `==`/`!=` 为值比较（record 引用类型为 null 安全；普通 `class` 仍为引用身份）；位置参数合成 `Deconstruct(out …)`（record 与 record struct）；`record struct` 走 struct 管线。

**解构赋值**：`(x, y) = expr;` 脱糖为 `expr.Deconstruct(out …)`；支持弃元 `_` 与 `var (x, y) = expr`（typeck 引入局部）。**不是**元组类型——`(T1, T2)` 类型仍禁止。

**位置模式**：`e is (var x, var y)` / `case (var x, _)` / `e switch { (var x, var y) => …, _ => … }` 在 typeck 脱糖为非 null 守卫（class）+ `Deconstruct`；子模式仅 `var`/`_`。

**IEquatable / IHashable**：`record` / `record struct` 隐式实现 `IEquatable<R>` / `IHashable<R>`（static abstract）：合成 `static Equals(R,R)` / `static GetHashCode(R)`，转发实例方法；可用于 `Dictionary<R,V>` 等零装箱键。引用类型静态 Equals 含 null 安全；struct 无 null 守卫。

**诚实边界**：位置参数 → `{ get; init; }`；解构赋值 / 位置模式 ✅；属性模式**立宪硬拒绝**；**`init` 访问器**：auto / 自定义 init / `required` / 自定义 init 对象初始化器 / `with`×自定义 init；普通 `class` 的 primary constructor 不在本能力内。

## 访问控制

| 修饰符 | 含义 |
|--------|------|
| `public` | 任意可见代码可访问 |
| `private` | 仅声明类内部 |
| `protected` | 声明类及派生类 |
| `internal` | 同一程序集/模块（与 crate 边界对齐）；被访问方包可在 `arc.toml` 声明 `[package].internals_visible_to = ["X"]` 使包 X 跨包访问其 `internal`（对标 C# `[assembly: InternalsVisibleTo]`，用于测试程序验证 internal 实现） |

`crates/oop::AccessContext` 在 typeck 阶段验证字段与方法访问。

## 接口

```as
interface IShape {
    int Area();
    string Name();
}

interface IDrawable {
    void Draw();
}

class Square : IShape, IDrawable {
    public int Side;

    public int Area() { return Side * Side; }
    public string Name() { return "square"; }
    public void Draw() { Console.WriteLine("square"); }
}
```

接口定义契约；实现类必须提供全部成员。

## 继承

- **单继承**：`class Derived : Base` 仅允许一个基类
- **多接口**：逗号分隔多个 `interface`
- 派生类可 `override` 虚方法；基类方法默认虚 dispatch

```as
class Square : Rectangle {
    protected override int Area() {
        return Width * Width;
    }
}
```

## 多态

通过接口或基类类型调用时，使用动态分派：

```as
void useShape(IShape s) {
    Console.WriteLine(s.Name());
    var a = s.Area();
}
```

静态类型决定可调用成员集；动态类型决定实际实现。

### 接口值 ABI（fat pointer）

接口类型的值是 **fat pointer**：指向栈/堆上的 `{ ptr obj, ptr itable }` 二元组（以 `ptr` 传递）。

| 阶段 | 约定 |
|------|------|
| MIR | class→interface 赋值经 `MakeIface`（静态类型已声明该接口）或 `MakeIfaceDyn`（基类静态类型、runtime 按 `type_id` 选 itable）；**variance 接口→接口**经 `AdaptIface`（比较源 itable 指针、重绑定到适配器 itable）；参数经 `MirOperand::Iface`。派生类赋值时 itable 落在**直接声明**该接口的祖先类（`@.itable.{Provider}_{Iface}`）；协变/逆变视图另有 `@.itable.{Class}_{VarianceView}` |
| codegen | 接口方法调用只从已有 fat pointer 取 itable slot；**禁止**再按具体类把接收者重建为 fat pointer（否则会把 fat 地址误当 `obj` → 运行时崩溃） |
| 协变/逆变 / 适配器 thunk | variance 目标 itable 的槽位指向 **适配器 thunk**：协变——concrete 返回 class、视图期望接口时 thunk 内 `MakeIface`（零参与带参均转发形参）；concrete 返回接口 A、视图期望 B 时 thunk 内 itable 重绑定。逆变——视图更窄的实参在 thunk 内包装/重绑定为 concrete 更宽的形参。调用点不再做返回值包装兜底 |
| 类虚方法 | 独立走 `ArcHeader.vtable`；与接口 itable 路径解耦 |
| 类型测试 | `obj is IFoo` / `obj is IFoo x`：`rt_obj_isa` 遍历 class `implemented_interfaces`（含 AST 接口继承父接口与 variance 协变/逆变视图）；声明绑定后须 fat pointer（见上） |

派生类继承基类已实现的接口时，仍可赋给接口变量（`is_subtype` 沿基类链成立）；itable 符号落在声明接口的那一层。`class : IChild` 且 `interface IChild : IBase` 时，layout 将 `IBase` 纳入 `interfaces` / `implemented_interfaces`，使 `obj is IBase` 与 `MakeIface` 到 `IBase` 可用。泛型同理：`interface IChild<T> : IBase<T>` 单态后 `IChild_int.base_types` 保留代入的 `IBase<int>`，layout 将 `IBase_int` 纳入传递闭包；**variance 合成基类**亦写入 class `interfaces`，以发射适配器 itable（重载收集跳过非 AST variance 基类）。

## 与所有权的关系

- `class` 实例为 ARC 管理堆对象
- 以接口类型传递时复制 fat pointer（内部 `obj` 仍受 ARC 管理）
- 临时只读/可变访问由 `borrowck` 在内部验证，用户面不书写 `&` 借用语法

## 与类型检查器的集成

`crates/oop::TypeRegistry` 维护类与接口层次；`typeck` 解析成员访问、实现关系与 override 兼容性。

## 示例：组合与接口隔离

```as
class DataContext {
    public IQueryProvider Provider;
}

interface IQueryProvider {
    IQueryable<T> CreateQuery<T>();
}
```

Provider 模式将 Queryable 数据源与表达式树消费解耦（见[表达式树](10-expression-trees.md)）。

## 扩展方法

Arc 支持 C# 风格的扩展方法，通过 `static class` + `this T receiver` 首参声明，调用时脱糖为 `Container::Method(receiver, ...)`。

### 作用域与可见性

- **同命名空间可见**：扩展方法所在命名空间与调用点相同时始终可见（`enclosing` 规则）
- **using 导入可见**：`using N;` 将命名空间内扩展方法纳入作用域（`ExtensionScope` 三匹配规则：精确/前缀/末尾段）
- **不可见**：未 using 导入的跨命名空间扩展方法不可见

### 泛型扩展

支持 `static T Foo<T>(this T x)` 形式的泛型扩展方法。调用时由 `unify_receiver` 推断类型实参，`instantiate_generic_extension_fn` 单态化，MIR 脱糖复用 `mangle_generic`。

### 冲突消解

多个候选扩展方法时按 C# 优先级规则消解：
- 规则 1：更具体的接收者类型优先（子类优先于父类）
- 规则 2：同命名空间扩展优先
- 并列时报 `AmbiguousExtensionCall` 错误

注：C# 规则 3（显式 using 优先于隐式）与规则 4（类内扩展优先）在 Arc 不适用——Arc 无隐式 using、不允许类内扩展。

## 方法重载

同名方法按参数类型与 arity 区分（C# 最小规则）。`typeck` 的 `resolve_method_overload` 与 MIR 静态调用 lower（`user_type_static_method_func`）均按实参类型选唯一候选并 mangle 为 `Class::Method_paramTy…`（如 `Assert::Equal_string`）。多候选无法消歧时报 `AmbiguousOverload`。

用户 `static class`（如 `Assert`）不得经 `check_native_method` 的「按 arity 取首签名」路径——该路径仅服务 native 契约模块（`native_caps`）。

**MIR 实参类型与静态重载（边界）**：静态重载以 `infer_type_from_expr` 得到的实参 `TypeId` 消歧。嵌套字段链（如 `b.Value.Value`）须沿链解析每一层字段/属性类型，不得把内层 `Ident` 的类名当作整条链的类型。推断失败时，MIR 仅在「唯一静态签名」或「同 arity 唯一静态签名」时回退；**禁止**在多个同 arity 候选中按注册顺序取首个（会掩盖嵌套字段推断缺陷，并可能误选 `Equal(int,int)`）。正确路径仍是加固 `Expr::Field` 推断。

---

上一节：[06 内存与资源](06-memory-resources.md) · 下一节：[08 异步与任务](08-async-tasks.md)