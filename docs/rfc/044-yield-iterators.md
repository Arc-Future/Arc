# RFC 044 yield 迭代器

## 背景

对标 C# `yield` 语句（[yield 语句 — C# 参考](https://learn.microsoft.com/zh-cn/dotnet/csharp/language-reference/statements/yield)）。Arc 生产 `IEnumerable<T>` / `IAsyncEnumerable<T>` 序列的**语言级惯用法**是 `yield return` / `yield break`：编译器合成状态机，开发者只写线性控制流。手写枚举器类仅作互操作与编译器内部基础设施场景（如 `ListEnumerator` 挂接 runtime ABI），AsyncStream 构建器仅作推模型适配（网络/IO 线程回调侧生产）。

## 设计决策

### 单一惯用法

`yield return` / `yield break` 是生产序列（`IEnumerable<T>` / `IEnumerator<T>`）与异步序列（`IAsyncEnumerable<T>`）的**唯一语言级惯用法**：

- 含 `yield` 的方法（返回上述接口之一）由编译器自动分类为**迭代器方法**并合成状态机类；
- 手写枚举器降级为**互操作与编译器内部基础设施**场景（如 `ListEnumerator` 挂接 runtime ABI）；
- AsyncStream 保留其**推模型适配**职责（网络/IO 线程回调侧生产），不再是拉模型序列的书写惯用法；
- std 序列生产以 yield 迭代器为单一惯用法：`SseDecoder.Decode`、`DeepSeekEventStream.Events`、`OpenAIEventStream.Events`、`AgnesEventStream.Events`、`AIStreamEvent.ErrorStream` 均为原生 yield 迭代器。

### 上下文敏感关键字

`yield` 不进保留字表：仅当**语句起始位置**且后随 `return` / `break` 时识别为 yield 语句；其余位置（表达式、标识符声明、成员名）仍作普通标识符，零破坏既有兼容。

### 迭代器方法分类

含 `yield` 的方法体，其声明返回类型必须是以下之一（违者精确报错）：

| 返回类型 | 类别 | 合成物 |
|---|---|---|
| `IEnumerable<T>` | 同步可枚举 | 状态机类（实现 `IEnumerable<T>` + `IEnumerator<T>`），原方法体重写为 `return new <SM>(...)` |
| `IEnumerator<T>` | 同步枚举器 | 同上，但不提供 `GetEnumerator` |
| `IAsyncEnumerable<T>` | 异步可枚举 | 状态机类（实现 `IAsyncEnumerable<T>` + `IAsyncEnumerator<T>`），`MoveNextAsync` 为 `async Task<bool>` |

非迭代器方法（返回类型不在上表）体内出现 `yield` → 精确报错。`async` 修饰仅允许出现在 `IAsyncEnumerable<T>` 迭代器方法上（脱糖先于 typeck，原方法改写为同步返回合成实例，绕开「async 必须 Task」常规约束；该约束本身不变）。

### 状态机合成模式

对标 C# 编译器（Roslyn）合成模式，实现于 hir 层（AST→AST 脱糖，`crates/hir/src/yield_desugar/`），下游 typeck/MIR/codegen 见到的只是普通类，**零新语言机制、零下游感知**：

1. **CFG 切分**：方法体按结构化控制流（`if`/`while`/`for`/`foreach`/`switch`/`break`/`continue`/`return;`）构建微型 CFG；每个 `yield return` 是一个挂起点，其后继基本块获得一个状态号（0 为入口，1..N 为恢复点，-1 为终结）。
2. **变量提升**：方法参数与局部变量提升为状态机字段（`__prm_*` / `__loc_*`，alpha 换名防遮蔽）；引用同步改写。提升字段裸访问经既有 `this` 隐式字段解析（`rewrite_bare_instance_field`），无新解析规则。**实例迭代器方法内显式 `this.X` 成员访问**：`this` 重写为宿主引用字段 `__host`（类型 = 宿主类），合成类经构造末参 `__this` 捕获宿主实引用；再枚举时 `GetEnumerator`/`GetAsyncEnumerator` 以 `__host` 重放。宿主 private 成员访问由 typeck `synth_hosts` 放行（等价 C# 嵌套状态机可见性）。
3. **驱动循环**：`MoveNext()` / `MoveNextAsync()` 体 = `while (true) { switch (__state) { case N: …; } }`；每个基本块一个 case，块内语句后随终结边（`__state = T; break;` 重派发 / `if (c) { __state = A; } else { __state = B; } break;` 条件边 / `__current = v; __state = R; return true;` 挂起边 / `__state = -1; return false;` 终结边）。用户 `switch` 的模式/守卫原样保留在分派 case 中——恢复点直接命中 case 内代码，不重复求值判别式。
4. **再枚举安全**：`GetEnumerator()` / `GetAsyncEnumerator(ct)` 返回**新鲜实例**（经构造函数重放捕获参数），状态从 0 起步，天然可重复枚举。

### 泛型迭代器方法

迭代器方法可携带**类型参数**——顶层泛型函数、非泛型类内泛型方法、泛型类内任意方法均支持，合成状态机类继承全部类型参数：

- **合成类泛型** = 宿主类泛型（若有）+ 方法泛型（按声明顺序拼接），状态机类以 `class __Yield_X_0<T1, T2, …>` 声明；
- **返回类型替换**：原 `IEnumerable<T>` / `IAsyncEnumerable<T>` 中的元素类型依方法/宿主泛型实参实例化，合成类基接口（`IEnumerable<T>`/`IEnumerator<T>`/`IAsyncEnumerable<T>`/`IAsyncEnumerator<T>`）同样携带类型参数；
- **约束传播**：方法 where 子句原样复制到合成类，维持泛型安全；
- **`this` 捕获 × 泛型宿主**：宿主类泛型参数代入 `__host` 字段类型（`__host : Stepper<T>`），泛型宿主内 `this.X` 成员访问正常捕获；
- **实例化**：下游 typeck 复用既有 `instantiate_generic_class` 单态化管线；多具体类型实例化（如 `Flow<int>` / `Flow<string>`）各自独立合成、mangle 去重；
- **接口实现判定**：泛型态状态机类经 `register_parametrized_generic_stub` 以 stub 形态注册（清除接口 bases 防 itable 虚分派），子类型判定回退到类模板 AST bases（`template_subtype_of_interface`）证明其实现 `IEnumerable<T>` / `IAsyncEnumerable<T>`。

### 与 async 状态机管线的关系

两级正交：yield 切分在 hir（AST 级），await 切分在 codegen（MIR CFG 级，`emit_async_sm` 既有管线）。异步迭代器方法体内的 `await` 表达式原样落入合成的 `async Task<bool> MoveNextAsync()` 中，由既有 async 状态机按挂起点二次切分——yield 层不感知 await，await 层不感知 yield。

### 消费协议

- 同步：`IEnumerator<T> e = seq.GetEnumerator(); while (e.MoveNext()) { … e.Current … }`
- 异步：`IAsyncEnumerator<T> e = seq.GetAsyncEnumerator(CancellationToken.None); while (await e.MoveNextAsync()) { … e.Current … }`（对齐 RFC 008 AsyncStream 既有拉取惯用法）
- `foreach` 消费 yield 序列**已支持**：`IEnumerable<T>` 接收者走 GetEnumerator 协议路径（MIR `TypedStmt::For` 枚举展开；方法调用返回的 `IEnumerable_<T>` mangle 名经 `enumerable_elem` 解码元素类型）。

## 边界（诚实标注）

以下为不支持项，编译器给出精确诊断（非静默错误结果）。

- **`yield` 出现在 `try`/`finally` 内**——**已支持**：finally 内联到 try 区域终止/落出块（挂起点不执行）；提前终止的 Dispose 链依赖 `IEnumerator` 接口契约扩展（std/Arc Stable 冻结面，循 RFC 036 流程另议）。`try`/`catch` 内 yield 仍拒绝；`using`/`lock` 内 yield 仍拒绝。
- **`yield` 出现在 lambda / 表达式块**——拒绝（yield 仅语句位置）。
- **迭代器方法体内 `var` 局部声明**——**已支持**：合成类字段类型后置解析——HIR 以 `Type::Infer` 字段提升，typeck 从状态机方法体首次赋值推断回填（`__infer__` 哨兵）。
- **迭代器方法体内 `foreach`**——**已支持**：枚举器协议展开（GetEnumerator → MoveNext → Current），迭代变量与枚举器为 Infer 字段后置推断；`__enum_<var>` 字段命名，嵌套/顺序 foreach 由 `var` 唯一性约束。
- **迭代器方法体内解构声明**——**已支持**：解构目标提升为 Infer 字段，Deconstruct out 经临时局部回写（MIR RefArg 仅支持局部地址）。
- **`ref`/`out`/`in` 参数**——拒绝（请先读入局部变量）。
- **`this` 捕获（见[状态机合成模式](#状态机合成模式)；泛型宿主类内捕获见[泛型迭代器方法](#泛型迭代器方法)）的剩余边界**——仅限显式 `this.X` 成员访问：裸私有字段引用在脱糖层无名字解析，与 `Console`/`Math` 等外部裸标识符不可区分，仍拒绝；static/顶层函数内 `this` 均精确拒绝。
- **迭代器方法体内 `return expr;`**——拒绝（C# 同样禁止；`return;` 等价 `yield break;`）。
- **迭代器方法体内 `switch` 类型绑定模式**——**已支持**：`case T n` / `case var n` / variant / 位置绑定提升为字段；object 槽绑定经 MIR 拆箱（rt_string_unbox / rt_box_unbox），字段写入跨状态机块存活（局部绑定在 case 块间 scope 隔离不可见）。
- **MoveNext 重入守卫**（running 态再入抛异常）省略；**Dispose/Reset 语义**省略（接口无此成员，对齐 std 契约）。取消经迭代器方法参数于调用点捕获（生产者侧单通道）；合成 `GetAsyncEnumerator` 的 ct 形参仅为接口契约兼容，不注入生产流程（`[EnumeratorCancellation]` 机制拒绝，见下）。
- **迭代器方法体内 try/catch**：拒绝。流内异常收敛惯用法：非抛错边界 helper（普通 async 方法，try/catch 把异常转为结果值），迭代器线性消费结果值——`SseDecoder.ReadAsync` / `DeepSeekEventStream.StartAsync`/`ParseLine` 等即此模式。
- **手写枚举器场景**：`AsyncStreamEnumerator`（推模型适配本职）与 `ListEnumerator`（runtime ABI 互操作本职）；其余 std 序列生产（`SseDecoder`、`DeepSeekEventStream`、`OpenAIEventStream`、`AgnesEventStream`）均为原生 yield 迭代器。

## 禁止项

- **`yield return` 处于 try-catch**：按 C# 语义整体拒绝（try-finally 内 yield 的 Dispose 合成链推迟至 EH 跨挂起点机制落地，不提供部分支持）。
- **`[EnumeratorCancellation]`**：取消令牌参数化传播属 C# 历史兼容机制；取消经迭代器方法参数于调用点显式传递（单一通道），`GetAsyncEnumerator` 的 ct 形参仅服务于手写枚举器契约（如 AsyncStream 推模型适配）。
- **`IEnumerable` 非泛型形式 / `IEnumerator` 非泛型形式作迭代器返回类型**：仅泛型形式。
- **半吊子降级**（如 yield 静默收集为 List 缓冲）：违反惰性语义，禁止。

## 冻结面合规

纯增量特性：新增上下文敏感关键字识别、两个 AST 语句变体、hir 独立脱糖 pass 与合成类注入；不修改任何既有语句/类型/语义，不触及 `rt_*` ABI 与 `std/Arc` Stable 面。typeck/MIR 中 yield 节点仅保留防御性 internal-error 臂（脱糖遗漏即报，不静默）。

## 成熟度

- H1 底层稳定：脱糖为纯 AST 变换，无新运行时依赖；状态机即普通类，走既有 vtable/itable 装配。
- H2 极致性能：单层 switch 分派 + LLVM 优化；无装箱、无运行时解释器、无协程 runtime。
- H3 高阶可编译：`std` 与用户代码以 yield 表达序列生产为单一惯用法——AI 流式全链（SSE 解码 / Provider 事件流 / 冷错误流）即此模式；泛型迭代器方法（顶层泛型 / 泛型类内方法 / 异步泛型）为设计面内的完整能力。

---

上一节：[043 Coding Agent Harness 工程](043-harness.md)
