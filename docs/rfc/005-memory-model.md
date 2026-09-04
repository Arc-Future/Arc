# RFC 005 内存模型与资源安全

## 背景

Arc 拒绝全局停顿式垃圾回收，采用**静态所有权 + 引用计数（ARC）**混合模型。目标：释放时机可预测、无运行时停顿、编译期捕获移动/借用错误。用户面无裸指针。

## 设计决策

### 值类型：`struct`

- 栈分配（或内联于父对象）。
- 赋值与传参按**自动 Copy 判定**决定语义；移动时源变量不可再使用（`UseAfterMove` 编译期报错）。
- 离开作用域时按字段逆序析构。

#### 自动 Copy 判定

纯值 `struct`（字段**传递闭包**内不含 `class` 句柄）的赋值与传参为**逐字段复制**（Copy），源变量仍可用；含 `class` 句柄（传递闭包）的 `struct` 保持**移动**语义。

- 判定规则一句话：看字段声明——字段全为基元 / `enum` / 其他纯值 `struct` → Copy；任一字段（含嵌套 `struct` 字段）为 `class` 引用 → 移动。
- Copy 是**结构性自动判定**（编译器从字段类型推导）：用户无 `Copy` 修饰符、无 derive；规则可预测，一眼看字段即知。
- Copy 路径 = 纯 `memcpy`（无句柄字段即无 `rt_arc_inc`/`rt_arc_dec`）：不引入隐藏计数开销，AOT 确定性不受影响。
- 移动语义的动机不变：无 GC 前提下，句柄所有权必须单线（见「与 ARC 的协作」）。Copy 类型的所有权**明确定义**（逐字段复制、无共享、无计数调整），不违反「未定义所有权的双重复制语义」禁令——该禁令针对的是复制/移动规则混乱并存且无所有权定义的状态。
- 自引用 `struct`（`struct A { A a; }`）为布局错误：借用检查的递归判定以 visited 防环短路，布局 / codegen 层拒绝。

```as
struct Buffer {
    public byte[] Data;   // class 句柄字段 → 移动语义
}

void demo() {
    Buffer a = new Buffer() { Data = ... };
    Buffer b = a;   // 移动：a 不可再用
}
```

```as
struct Point {
    public int X;
    public int Y;
}   // 纯值字段 → Copy

void copy_demo() {
    Point a = new Point() { X = 1, Y = 2 };
    Point b = a;   // 复制：a 仍可用（对齐 C# 复制语义）
    b.X = 10;      // 不影响 a.X
}
```

### 引用类型：`class`

- 堆分配；多变量可共享同一实例。
- 生命周期由 ARC 管理：`rt_arc_inc` / `rt_arc_dec`。
- 最后一个引用释放时析构并回收堆块。

```as
class Document {
    public string Title;
}

void share() {
    var a = new Document();
    var b = a;   // 共享所有权，引用计数 +1
}
```

### 借用约束（编译期）

在所有权不变前提下，编译器允许临时借用（**用户源码无 `&` 关键字**）：

| 概念 | 含义 |
|------|------|
| 不可变借用 | 共享只读访问；可同时存在多个 |
| 可变借用 | 独占可变访问；与任何其他借用互斥 |

规则：
1. 同一时刻，可变借用与任何其他借用不可共存。
2. 借用生命周期不得超过被借值的有效期。
3. 不可将局部变量的引用逃逸到更长生命周期。

**NLL（非词法生命周期）无条件启用**：借用按 last-use 终止，无开关。违反时编译期报 `AlreadyBorrowed` / `MutablyBorrowed`。

### 函数返回值所有权（借用返回）

函数/属性返回值是**借用**：codegen 在返回路径不 inc（`new`/拷贝的计数由生产方完成），调用方把返回值存入命名局部后，**局部 epilogue 按「新引用」dec**——借用返回 + 局部 dec 是计数不对称的根源。契约：

1. **缓存/容器持有者**不得裸返回内部引用（`return _cache[i];`）：字段/容器持有唯一所有权，调用方 dec 使缓存计数每调用漂移 → 提前 free → 悬垂 dec（free DUP / UAF）。必须经中间局部赋值把借用转新引用：`object? x = _cache[i]; return x;`（赋值是拷贝语义，inc；返回的 x 与容器持有各自独立）。
2. **生产方**返回借引用时（如 `Task<T>.Result` 同步提取，见 RFC 009 结果所有权），须在返回点 retain，与调用方 epilogue dec 配对。
3. 经 `new`/方法调用返回的值视为「移交所有权」，调用方局部 dec 与之配对，不额外 retain。

### 切片视图：`Span<T>` / `ReadOnlySpan<T>`

`Span<T>` / `ReadOnlySpan<T>` 是**语言内建 ref-like 值类型**（非可装箱 `class`、非用户可写 `ref struct`）：

- 逻辑表示 `{ data, length }` 胖指针；**用户面无裸指针 / `unsafe`**。
- 从 `T[]` 经 `AsSpan` / `AsReadOnlySpan` 零元素拷贝构造。
- **B3 禁逃逸**：禁止写入 `class` 字段（`E_SPAN_ESCAPE`）；禁止捕获进堆上闭包。
- `Span` → `ReadOnlySpan` 隐式转换；反向禁止；`ReadOnlySpan` 索引只读。
- 用户面：`Length` / `IsEmpty` / `this[i]` / `Slice` / `CopyTo` / `TryCopyTo` / `ToArray` / `Fill` / `Clear` / `foreach`（索引脱糖 · 零堆）。

### 与 ARC 的协作

- `struct` 可包含 `class` 句柄；句柄复制调整引用计数。
- `struct` 移动时，内嵌句柄所有权随结构体一并转移。
- 循环引用需显式打破或依赖循环收集（见下）。

### 弱引用与循环收集

- **`Weak<T>`**：不强引用、不阻止回收；`TryGet` 原子提升。
- **循环收集器**：Nim ORC 试删模型，**默认 always-on**——阈值触发、用户无感，回收引用环。

### 资源确定性

| 操作 | 释放时机 |
|------|----------|
| `struct` 离开作用域 | 立即析构 |
| `class` 最后一次 `dec` | 立即析构 |
| 全局 / 静态 | 进程退出时 |

无 STW（stop-the-world）回收阶段；系统软件可依赖延迟上界。异常路径上的析构走 `rt_arc_dec` 同步路径（`nounwind`，不重入 unwind），见 [异常与资源管理](010-exceptions-resources.md)。

### 运行时支持

`crates/runtime/runtime.c` 提供 `rt_arc_inc` / `rt_arc_dec` / `rt_panic` 等 ABI；codegen 在 `class` 复制、丢弃与字段赋值处插入 inc/dec。运行时符号面见 [运行时 ABI](014-runtime-abi.md)。

### 容器内部缓冲区

`Arc.Collections.List<T>` 与 `Dictionary<K,V>` 采用**不透明句柄模式**：Arc 源码层仅持有 `intptr_t _handle`，内部缓冲区由运行时 `rt_list_*`/`rt_dict_*` 管理。扩容 `realloc` + 2× 增长，同步无 GC 暂停；扩容仅重分配内部 `data` 指针，不改容器对象地址，外部引用始终有效。元素为引用类型时由 codegen 在 `Add`/`Set` 维护元素级 ARC。详见 [集合、字符串与数值](007-collections-strings-numerics.md)。

## 边界

- class 层次、vtable、接口 fat pointer 见 [对象模型](006-object-model.md)。
- 资源确定性释放的 `using` / `IDisposable` 见 [异常与资源管理](010-exceptions-resources.md)。
- 运行时 `rt_*` ABI 细节见 [运行时 ABI](014-runtime-abi.md)。

## 禁止项

- 未定义所有权的双重复制语义。
- 绕过 borrowck 的裸指针算术（`unsafe` **永久拒绝**；性能以安全 `Span` + Verified FFI + 单态化达成）。
- 隐式全局 GC。

---

上一节：[004 类型系统](004-type-system.md) · 下一节：[006 对象模型](006-object-model.md)