# 06 内存与资源

Arc 拒绝全局停顿式垃圾回收，采用**静态所有权 + 引用计数（ARC）** 混合模型。

## 值类型：`struct`

- 栈分配（或内联于父对象）
- 赋值与传参默认**移动**；源变量在移动后不可再使用
- 离开作用域时按字段逆序析构

```as
struct Buffer {
    public int Length;
}

void use(Buffer b) {
    // b 按值接收，调用后外部原绑定若已移动则不可用
}

void demo() {
    Buffer a = new Buffer() { Length = 10 };
    Buffer b = a;       // 移动：a 不可再用
    use(b);
}
```

`crates/borrowck` 对移动后使用报告 `UseAfterMove` 错误。

## 引用类型：`class`

- 堆分配
- 多变量可共享同一实例
- 生命周期由 ARC 管理：`rt_arc_inc` / `rt_arc_dec`（见[运行时 ABI](12-runtime-abi.md)）
- 最后一个引用释放时析构并回收堆块

```as
class Document {
    public string Title;
}

void share() {
    var a = new Document();
    var b = a;   // 共享所有权，引用计数 +1
}
```

## 借用

在所有权不变前提下，编译器允许临时借用（**用户源码无 `&` 关键字**；规则在 `borrowck` 内部执行）：

| 概念 | 含义 |
|------|------|
| 不可变借用 | 共享只读访问；可同时存在多个 |
| 可变借用 | 独占可变访问；与任何其他借用互斥 |

规则：

1. 同一时刻，可变借用与任何其他借用不可共存
2. 借用生命周期不得超过被借值的有效期
3. 不可将局部变量的引用逃逸到更长生命周期

违反时 `borrowck` 报告 `AlreadyBorrowed` 或 `MutablyBorrowed`。

## 切片视图借用

`Span<T>` / `ReadOnlySpan<T>` 是**语言内建 ref-like 值类型**（非可装箱 `class`、非用户可写 `ref struct` 关键字）：

- 逻辑表示 `{ data, length }` 胖指针；**用户面无裸指针 / `unsafe`**
- 从 `T[]` 经 `AsSpan` / `AsReadOnlySpan` 零元素拷贝构造
- **禁逃逸**：禁止写入 `class` 字段（诊断 `E_SPAN_ESCAPE`）；禁止捕获进堆上闭包
- `Span` → `ReadOnlySpan` 隐式转换；反向禁止；`ReadOnlySpan` 索引只读
- **已接线用户面**：`Length` / `IsEmpty` / `this[i]` / `Slice(start)` / `Slice(start,length)` / `AsReadOnly` / `Empty`（`Span<T>.Empty` 或 `[]`）/ `CopyTo(Span)` / `TryCopyTo(Span)` / `ToArray()` / `Fill` / `Clear` / **`foreach`（索引脱糖 · 零堆）**
- 契约骨架：`std/Arc/Span.as` · `std/Arc/ReadOnlySpan.as`

## 与 ARC 的协作

- `struct` 可包含 `class` 句柄；句柄复制调整引用计数
- `struct` 移动时，内嵌句柄所有权随结构体一并转移
- 循环引用需显式打破（弱引用等扩展在 RFC 中跟踪）；MVP 不自动回收环

## 资源确定性

| 操作 | 释放时机 |
|------|----------|
| `struct` 离开作用域 | 立即析构 |
| `class` 最后一次 `dec` | 立即析构 |
| 全局/静态 | 进程退出时（MVP） |

无 STW（stop-the-world）回收阶段；系统软件可依赖延迟上界。

## 运行时支持

`crates/runtime/runtime.c` 提供：

```c
void rt_arc_inc(void* ptr);
void rt_arc_dec(void* ptr);
void rt_panic(const char* msg);
```

codegen 在 `class` 复制、丢弃与字段赋值处插入 inc/dec。

## 容器内部缓冲区扩容

`Arc.Collections.List<T>` 与 `Dictionary<K,V>` 采用**不透明句柄模式**：Arc 源码层仅持有 `intptr_t _handle`，内部缓冲区由运行时 `rt_list_*`/`rt_dict_*` 管理。

| 机制 | 说明 |
|------|------|
| 扩容策略 | `realloc` + 2× 增长；`size == capacity` 时触发，`new_cap = max(capacity * 2, 8)` |
| 延迟上界 | 同步 `realloc`，无 GC 暂停；原地扩展零拷贝，重新分配 O(n) 拷贝 |
| 外部引用有效性 | 扩容仅重分配内部 `data` 指针，不改容器对象地址；外部引用始终有效 |
| 元素级 ARC | `T` 为引用类型时，codegen 在 `Add`/`Set` 处生成 `rt_arc_inc`；`rt_list_destroy` 内部循环对每个元素 `rt_arc_dec`。值类型无 refcount 开销 |
| 容器自身 ARC | `List<T>`/`Dictionary<K,V>` 为 `class`，对象头由 ARC 接管；`rt_arc_dec` 归零时触发 `rt_*_destroy` 释放内部缓冲区 |

## 禁止项

- 未定义所有权的双重复制语义
- 绕过 borrowck 的裸指针算术（MVP 不提供 `unsafe` 块）
- 隐式全局 GC

---

上一节：[05 类型系统](05-type-system.md) · 下一节：[07 对象模型](07-object-model.md)