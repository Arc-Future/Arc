# 附录 A 术语表

| 术语 | 定义 |
|------|------|
| Arc | 面向人机协作时代的纯 AOT 系统级编程语言；编译器命令与语言同名 |
| `.as` | Arc 源文件扩展名 |
| 前导类型 | 类型声明写在名称之前，如 `int count` |
| AOT | Ahead-of-Time，编译期生成原生机器码，无 JIT |
| ARC | Automatic Reference Counting，引用计数管理 `class` 生命周期 |
| 所有权 | 每个值有唯一所有者；移动后原绑定失效 |
| 借用 | 在不转移所有权下临时访问；由 `borrowck` 在编译期验证，用户面无 `&` 语法 |
| `struct` | 值类型，栈分配，移动语义 |
| `class` | 引用类型，堆分配，共享所有权 |
| `interface` | 契约类型，支持多态 dispatch |
| `Task<T>` | 异步计算句柄 |
| `async`/`await` | 异步函数与等待运算符 |
| Query | 声明式数据变换语法（`from`/`where`/`select` 等） |
| Enumerable 路径 | `IEnumerable<T>` 上的零成本迭代脱糖 |
| Queryable 路径 | `IQueryable<T>` 上的编译期表达式树化 |
| `expression` | 标记表达式树 Lambda 的关键字（C# 对齐） |
| ExpressionTree | 编译期表达式树 IR（`crates/expr`） |
| Provider | 消费 Queryable 表达式树的执行后端 |
| HIR | 高级中间表示（`crates/hir`） |
| MIR | 中级中间表示（`crates/mir`） |
| `rt_*` | 运行时 C ABI 符号前缀 |
| 能力 | 对外部效应（I/O 等）的显式声明与约束 |
| 结构化诊断 | 带 span 与标签的机器友好编译错误 |
| RFC | Request for Comments，设计决策记录 |
| 单态化 | 泛型实例化时生成具体类型专用代码 |
| rodata | 只读数据段，存放序列化的表达式树 |

相关：[附录 B 符号约定](appendix-notation.md) · [返回目录](index.md)