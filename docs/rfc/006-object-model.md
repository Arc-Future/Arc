# RFC 006 对象模型

## 背景

Arc 对象模型支持 `class`、`interface`、单继承与接口多实现，零开销虚分派，类型层次清晰，无运行时反射。

## 设计决策

### 类

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

- 实例由 `new ClassName(...)` 在堆上创建。
- 字段默认按声明顺序布局；访问控制由编译期检查。

### Primary constructor

对齐 C# 12 `class C(int x)` 表面，**与 `record` 解耦**：

```as
public class Point(int x, int y) {
    public int X() { return x; }
    public int Y() { return y; }
}
```

| 项 | 行为 |
|----|------|
| 声明位置 | 类名与泛型参数之后、基类列表之前：`class C<T>(T v) : Base` |
| 按值捕获 | 按值参数注入同名 `private` 实例字段；方法体可无限定名访问 |
| `ref`/`out`/`in` | **禁止捕获**；保留在合成 ctor 形参；可用于字段初始化器与 `: Base(args)` |
| `: Base(args)` | 声明侧基类构造实参写入合成 ctor；无 primary 时禁止 `: Base(...)` |
| 调用 | `new C(args)` / `new C(ref v)` 走既有 `__ctor` 路径 |

**仍排除**：`static class` primary；扩展接收者 `this`；与同名字段/同 arity 显式构造冲突；`record` / `struct` primary。

### 记录类型（`record`）

`record` 是引用类型（**`record class` 同义拼写已硬拒**）；`record struct` 是值类型。位置参数脱糖为 `{ get; init; }` 自动属性与构造器：

```as
record Point(int X, int Y);
record struct Vec2(int X, int Y);

Point q = p with { X = 10 };
bool same = new Point(1, 2) == new Point(1, 2);
p.Deconstruct(out int x, out int y);
var (a, _) = p;   // 声明 + 弃元
```

- `with` 浅拷贝并覆盖列出的实例字段 / auto-init 属性（仅 record）。
- 合成 `Equals` / `GetHashCode`（同组实例字段）；同类型 `==`/`!=` 为值比较（`record` 引用类型 null 安全；普通 `class` 仍为引用身份）。
- 位置参数合成 `Deconstruct(out …)`；提供解构赋值 `(x, y) = expr` 与位置模式 `e is (var x, var y)`。
- 隐式实现 `IEquatable<R>` / `IHashable<R>`，可用于 `Dictionary<R,V>` 等零装箱键。
- **属性模式 `{ Prop: … }` 立宪硬拒绝**（与解构/switch 重叠）。

### 合成哈希契约（`GetHashCode`）

`GetHashCode` 与 `Equals` 构成"业务相等"在哈希表算法层的**标准化投影**：算法先以哈希定位桶、再以 `Equals` 精确判定。契约分六维，前四维为语义约束，后两维为算法选择。

**硬契约（唯一正确性约束）**：`a.Equals(b)` ⇒ `GetHashCode(a) == GetHashCode(b)`。反之不保证（碰撞合法）。破坏即字典/集合错乱。

**一致性（与 `Equals` 同源）**：哈希基于与 `Equals` **完全相同**的实例字段集合与顺序。可变字段作键在哈希后变更会导致查表失败，属使用者禁忌（文档警示，与 C# 一致）。

**稳定性**：进程内同一对象多次哈希结果一致；**跨进程/跨启动不保证**——为每进程随机种子（防哈希碰撞 DoS）预留空间。

**基元哈希**：

| 类型 | 规则 |
|------|------|
| `int`/`uint` | 返回自身（均匀，桶索引直接取低位） |
| `long`/`ulong`/`double` | 取低 32 位（`double` 先 bitcast 为整型） |
| `float` | bitcast 为 `int` |
| 浮点 `NaN` | 归一化为单一规范位型，保证可查表（`NaN` ≠ `NaN` 但哈希须一致） |
| `string` | 内容哈希（`rt_hash_str`），见 [集合、字符串与数值](007-collections-strings-numerics.md) / [运行时 ABI](014-runtime-abi.md) |

**组合算法（record 合成）**：`record` / `record struct` 合成 `GetHashCode` 采用 C# `HashCode` 式组合——黄金分割常数 `0x9E3779B1` 逐字段合并 + xxHash finalizer 雪崩：

```
hash = 0
hash = hash * 0x9E3779B1 + (uint)field0.GetHashCode()
hash = hash * 0x9E3779B1 + (uint)field1.GetHashCode()
…逐字段…
hash ^= hash >> 15; hash *= 0x2C1B3C6D
hash ^= hash >> 12; hash *= 0x297A2D39
hash ^= hash >> 15
返回 (int)hash
```

组合**顺序敏感**（`(a,b) ≠ (b,a)`）、含非零种子、具备雪崩；区别于 Java 式 `*31`（无种子、无雪崩、分布弱）。

**边界**：
- 密码学哈希（`Arc.Security` 的 Hash/HMAC）与此**不同**——此处为哈希表桶索引，无抗碰撞安全要求；密码学哈希见 [加密与安全](026-cryptography-security.md)。
- 是否启用每进程随机种子（防 DoS）须与"跨进程稳定标识（如 `type_id`）"界定边界；边界落在 [成熟度](036-maturity.md) 门禁内，实现决策见 实现规划。

### 访问控制

| 修饰符 | 含义 |
|--------|------|
| `public` | 任意可见代码可访问 |
| `private` | 仅声明类内部 |
| `protected` | 声明类及派生类 |
| `internal` | 同一程序集/模块；被访问方可经 `arc.toml` 的 `internals_visible_to` 跨包开放 |

### 接口

```as
interface IShape {
    int Area();
    string Name();
}

class Square : IShape {
    public int Side;
    public int Area() { return Side * Side; }
    public string Name() { return "square"; }
}
```

接口定义契约；实现类必须提供全部成员。接口可声明 `Type Name { get; }` 属性，访问用 `.Name`。

### 继承与多态

- **单继承**：`class Derived : Base` 仅一个基类；**多接口**逗号分隔。
- 派生类可 `override` 虚方法；基类方法默认虚 dispatch。
- 静态类型决定可调用成员集；动态类型决定实际实现。
- **`base` 调用**：`base.Method(args)` 为**非虚静态分派**到**直接基类**
  对该方法的实现（C# base 语义：跳过派生类覆写）。直接基类若 `override` 了该方法则
  命中其覆写体；未覆写则命中继承链上的原声明实现。接收者是当前实例（与 `this`
  同一对象），仅分派/静态类型不同。`base` 仅可用于实例方法上下文，静态方法内报错。
  裸 `Method(args)` 与 `this.Method(args)` 仍为虚分派（命中最派生实现）。

```as
class Square : Rectangle {
    protected override int Area() {
        return Width * Width;
    }
}
```

### 虚分派（vtable）

- vtable 复用 `ArcHeader.vtable`；slot0 = typeinfo / slot1 = finalizer / slot2 = walk / slot3+ = 方法。
- `has_vtable` 标记；无虚类零开销。
- 类虚方法独立走 `ArcHeader.vtable`，与接口 itable 路径解耦。
- **槽位身份 = 完整签名**（对齐 C# MethodTable 槽语义）：虚方法重载各占其槽（键 = 名 + 形参类型），`override` 复用基类同签名槽位并更新该槽为最派生实现；签名不匹配的 `override` 编译期报 `NoMatchingOverrideBase`。调用点按已解析重载的形参类型计算槽位，同名不同签名的调用互不劫持。
- **默认虚 dispatch**：派生类声明与基类虚/抽象方法**同签名**的实例方法即视为覆写（无需显式 `override` 关键字），vtable 槽位实现解析到最派生类（如 `DeepSeekChatClient.CompleteAsync` 无 override 关键字仍命中派生实现）。显式 `override` 语义一致。
- **抽象实现要求（对齐 C# CS0534）**：**非抽象（具体）类必须实现继承链上全部抽象方法**——本类声明与各级基类（含多级链）声明的 `abstract` / `override abstract` 方法均须在声明点之下（更派生侧）存在非抽象的匹配实现（`Override` / `Virtual` / 普通同签名方法，或抽象属性由同类型 public 字段满足，如自动属性 override）。漏实现 → 编译期 `OOP: abstract method … in non-abstract class …`。**抽象类可不实现继承的抽象方法**（继续抽象）；`override abstract` 由更下层具体类接管，避免重复报错。接口抽象成员由 itable 覆盖检查负责，不在此重复。

### 接口值 ABI（fat pointer）

接口类型的值是 **fat pointer**：`{ ptr obj, ptr itable }` 二元组（以 `ptr` 传递）。

- class→interface 赋值经 `MakeIface`（静态类型已声明）或 `MakeIfaceDyn`（基类静态类型，runtime 按 `type_id` 选 itable）。
- **继承接口传播**：派生类继承基类的接口实现并发射**自己的 itable**（`@.itable.{Derived}_{Iface}`），其槽位沿 override 链解析命中派生类实现；接口赋值/`is` 类型测试据此命中最派生实现，而非基类的直接声明 itable。
- 接口方法调用只从已有 fat pointer 取 itable slot；**禁止**再按具体类重建 fat pointer。
- 协变/逆变视图经 `AdaptIface` 与适配器 thunk；目标 itable 槽位指向 thunk。
- 类型测试 `obj is IFoo` 经 `rt_obj_isa`，遍历 class `implemented_interfaces`（含 AST 接口继承与 variance 视图）。**接口静态类型的 scrutinee 必须先取 fat pointer 首槽的对象指针**（`UnboxIface`）再传 `rt_obj_isa`——直接传 fat pointer 盒地址会把 itable 指针误当 vtable）。
- **`is` 模式绑定的接口重绑定**：`if (ib is IChild cc)` 中 `ib` 静态类型为父接口/`object` 时，`cc = (IChild)ib` 须按源 itable 指针重绑定到子接口 itable（父→子方向 `AdaptIface`），否则 `cc` 复用父接口 fat pointer、按子接口扁平槽位索引读取父接口 itable 越界（AV）。接口→接口**任一方向**（子→父 / 父→子）均经 `AdaptIface` 重绑定。
- **接口继承槽位布局**（COM 式）：`IChild : IBase` 的 itable 槽位 = 父接口方法在前（沿 AST 继承链、按签名去重），子接口自身方法在后；发射（`emit_itables`）与查找（`iface_method_index`）共享同一扁平布局，经 `IChild` 引用调用父接口方法命中正确槽位。

### 接口泛型方法分派

接口可声明**泛型方法**（含 `where` 类型参数约束），实现类以泛型方法实现：

```as
interface IGetter {
    T Get<T>(T seed) where T : ISeed;
}

class Foo : IGetter {
    public T Get<T>(T seed) where T : ISeed {
        return seed;
    }
}

IGetter g = new Foo();
Seed s = new Seed(42);
Seed r = g.Get<Seed>(s);   // 经接口引用分派到 Foo::Get<Seed>
```

**分派契约（AOT · 全程序封闭世界）**：接口泛型方法**无法**经固定 itable 槽位以单一符号分派——方法 ABI 取决于类型实参 `T`，槽位无类型实参即无单一目标函数。实现采用**按实例化扩充 itable 槽位 + 全实现者单态化**：

1. **实例化收集（编译期）**：MIR 收集全部接口接收者泛型方法调用站点（如 `g.Get<Seed>` → 实例化键 `Get__Seed`）；**单态化后的封闭程序**中实例化集是有限且确定的（含嵌套泛型 `Sink<int>.Run` 体内 `g.Get<T>` 在 `Sink<int>` 单态化后收敛为 `g.Get<int>`）。
2. **全实现者单态化**：对每个实例化键 `Get__Seed`，枚举该接口**全部实现类**（registry 具体类闭集，含泛型类的具体实例如 `Box_int`），为每类生成 `C::Get__Seed`（自模板 `C::Get` 克隆 + 类型实参替换；体内 `@T_*` 成员符号随替换解除）。
3. **按实例化扩充 itable**：每个实现类的 itable 在普通方法槽后按全局确定顺序追加 `Get__Seed` 槽位，指向该类的单态化实现 `C::Get__Seed`。槽位 ABI 即单态化签名（`{ obj, T }` → `T`），与调用点完全一致，**无**适配器 thunk。
4. **调用点**：与普通接口方法同路径——`emit_iface_method_call` 取 fat pointer itable，按实例化名查槽位做间接调用。**零运行时分派开销、无隐藏 `type_id` 形参**；动态多态由既有 itable 语义保证（fat pointer 已携带具体类 itable）。

**槽位顺序**：普通接口方法（声明序）→ 属性（声明序）→ 泛型方法实例化（按 `(接口, 实例化名)` 排序）。发射（`emit_itables`）与查找（`iface_method_index`）共享同一排序，跨实现类槽位布局一致。

**约束**：
- 泛型方法**不进类 vtable**（C# 亦禁止泛型方法 virtual/override）；仅经接口 itable 分派。
- 接口泛型方法返回值/形参中的类型参数 `T` 仅以**具体实例化**进入 ABI；模板自身不独立成函数（体内触碰类型参数成员的模板由 `drop_non_emittable_generic_templates` 剔除，仅作单态化克隆源）。
- 封闭程序假设：接口实例化集与实现类闭集均须在编译期可枚举；**运行期新增实现类**（`--dynamic` 库运行时装载）不在本契约内（与 [成熟度](036-maturity.md) 宣称纪律一致）。

### `partial` class

跨文件合并，合并键 `(namespace, name, generic_arity)`；bases 合并 + where 合并 + 重复成员检测。服务 UI code-behind 与 Source Generator。

### 静态成员与静态类

- `static class` 仅含静态成员，不可实例化；扩展方法容器必须为 `static class`。
- 静态字段 / 静态方法 / 静态属性构成类级状态面。

### 属性访问器

```as
// 自动属性（字段后备）
public int Value { get; set; }
public int ReadOnly { get; }

// 自定义访问器
public int Computed {
    get { return _value * 2; }
    set { _value = value; }
}

// 表达式体
public int Doubled => Value * 2;
```

- `init` 访问器：auto / 自定义 init / `required` / 自定义 init 对象初始化器 / `with`×自定义 init；支持对象初始化器 `new Point() { X = 1 }`。
- 索引器 `T this[params] { get; set; }` 用 `obj[i]` 访问。

### 扩展方法

通过 `static class` + `this T receiver` 首参声明，调用脱糖为 `Container::Method(receiver, ...)`：

- 同命名空间可见（enclosing 规则）；`using N;` 导入可见。
- 支持泛型扩展 `static T Foo<T>(this T x)`，由 `unify_receiver` 推断类型实参并单态化。
- 冲突消解：更具体接收者优先；同命名空间优先；并列报 `AmbiguousExtensionCall`。

> **实现决策编号对照**（历史编号；部分历史代码注释将本组决策误标为「RFC 010」——扩展方法决策实际登记于本文档本节）：
>
> - **决策 #7（泛型扩展方法支持）**：泛型扩展方法保留泛型参数名供接收者推断（`unify_receiver`）；调用点触发单态化（`instantiate_generic_extension_fn`），对同一模板的不同接收者类型产生不同 mangled call_name（如 `FooExt::Id_int`）；模板本身不 emit 方法体（含未解析的类型参数），仅单态化实例有合法 body；支持 `where` 约束（单态化后约束清空）。
> - **决策 #8（候选集合化 + 优先级消解）**：规则 1 同命名空间优先；规则 2 更具体接收者优先；规则 3 接收者类型并列且无同命名空间优势时，报 `AmbiguousExtensionCall`。
> - **决策 #9（命名空间可见性）**：同命名空间内扩展方法始终可见，无需 `using` 导入；`using App;` 前缀匹配 `App.Extensions` 等命名空间即可见。

### 方法重载

同名方法按参数类型与 arity 区分；多候选无法消歧时编译期报 `AmbiguousOverload`。静态调用按实参类型选唯一候选并 mangle 为 `Class::Method_paramTy...`。

**normal form 优先于 expanded form（对齐 C# §Overload resolution）**：调用点存在多个可绑定候选时，能以 **normal form**（定参精确绑定、无 params 标注）匹配的候选优先于需 **expanded form**（`params ReadOnlySpan<T>` 展开）的候选——如 `Sum(int)` 与 `Sum(params ReadOnlySpan<int>)` 对 `c.Sum(5)` 命中定参重载。仅当无 normal form 候选时才采用 params 展开候选；多个 normal form（或全为 expanded form）候选并列仍报歧义。

**泛型方法实例化符号**：泛型方法显式实例化（`g.M<int>(…)`，含类型推断改写后的隐式实例化）的符号以**模板基底** mangle 为 `Class::Method_paramTy...__T0__T1`——基底取模板形参（保留泛型占位符，如 `GenHost::F_T`），类型实参以 `__` 追加。这与同名非泛型重载的定参 mangle（`GenHost::F_int`）**命名空间分离**：`F<int>` 实例化 → `GenHost::F_T__int`，`F(int)` 定参 → `GenHost::F_int`，互不撞名；单态化从模板克隆 `__` 后缀实例化体（`try_create_mono_body`）。隐式调用（`g.M(7)`）按 C# 语义**优先非泛型重载**；仅显式 `g.M<int>(…)` 或唯一泛型候选命中时走实例化符号。静态（含扩展）与实例路径共用此规则（统一经 `method_generic_template_link_name` 取模板基底）。

## 边界

- 内存分配、ARC、借用见 [内存模型与资源安全](005-memory-model.md)。
- 泛型约束见 [类型系统](004-type-system.md)。
- 委托与回调见 [委托、闭包与方法组](008-delegates-closures.md)。

## 禁止项

- **`record class`** 同义拼写（单一惯用法）。
- **属性模式** `{ Prop: … }`。
- **运行时反射调用**（`obj.GetType()` 反射写；元数据仅只读，见 [类型体系与反射元数据](018-type-reflection-metadata.md)）。
- **`closed` 类型修饰符**（C# 15 封闭层次）：**不引入**；可穷尽封闭集合由 `variant`（带载荷和式）与 `enum`（无载荷判别）单一惯用法承载，见 [类型系统](004-type-system.md)。

---

上一节：[005 内存模型与资源安全](005-memory-model.md) · 下一节：[007 集合、字符串与数值](007-collections-strings-numerics.md)