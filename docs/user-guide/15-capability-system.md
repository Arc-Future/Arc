# 15 能力系统

Arc 将**外部效应**（I/O、网络、窗口、文件系统等）设计为可审计的显式能力。智能体与人类读者均可从 API 表面判断程序可能触达的边界，无需全项目数据流分析。

## 原则

凡离开进程沙箱的效应，应通过标准库类型或能力声明暴露，而非隐式全局副作用。Arc 用户代码首选 **C# 风格 API**（如 `Console.WriteLine`），底层映射至 `rt_*` ABI，但不在表面语法引入 `println` 式内建。

## 能力映射

| 能力 | 用户 API | 运行时 |
|------|----------|--------|
| 标准输出 | `Console.WriteLine` | `rt_println` |
| 文件 I/O | 当前不提供 | `rt_file_*` |
| 网络 | 当前不提供 | 平台 socket |
| 窗口 | `Window.Run` | 平台窗口实现 |

## 能力声明

能力通过 **`std/` 源码类型 + 编译器 lowering** 实现，能力效应经 std API 可见：

```as
// 效应经 std API 可见
void Main() {
    Console.WriteLine("hello");
}
```

### namespace 级能力声明

在 namespace 声明处通过 `capability` 子句声明能力（namespace 路径后、`{` 或 `;` 前的可选子句）。`capability` 是上下文关键字，不引入新 Token。

```as
// block 形式
namespace myapp.io capability io {
    public class FileProcessor { ... }
}

// file-scoped 形式
namespace myapp.io capability io;

public class FileProcessor { ... }
```

行为约定：

- 嵌套 namespace 沿父链继承能力（取并集）
- 跨文件多次声明同一 namespace 时 capabilities 取并集
- 无 `capability` 标签的 native module 兼容所有 namespace

### 与 `.ani` 契约的协同

`.ani` 契约在 `native module` 上声明 `capability` 标签：

```
native module libsqlite3 capability io.Db {
    fn open(string filename) -> int;
}
```

typeck 在调用 `libsqlite3.open(...)` 时校验：当前 enclosing namespace 的有效能力集（沿父链继承的并集）必须包含 `io.Db`，否则报错：

```
namespace ["myapp"] 未声明能力 `io.Db`，无法调用 native 模块 `libsqlite3` 的方法 `open`；
请在 namespace 声明处添加 `capability io.Db`，或使用已声明该能力的 namespace
```

## 演进形态

- **函数级子句**（规划）：`void log(string msg) capability Console { ... }`，细化到方法粒度。
- **沙箱 / Agent**（规划）：默认拒绝未声明能力；opt-in 白名单。

## 与 borrowck 的关系

- borrowck 处理**内存**所有权与借用，不替代能力检查。
- 能力系统关注**外部世界**可达性；二者正交。

---

上一节：[14 结构化诊断](14-structured-diagnostics.md) · 下一节：[16 编译器 CLI](16-compiler-cli.md)