# RFC 008 委托、闭包与方法组

## 背景

定义 Arc 的函数值机制：委托类型、lambda、捕获语义、方法组转换与异步流。回调是覆盖非 UI 事件场景的一等公民，作为拒绝 C# `event` 的替代之一。

## 设计决策

### 委托类型

函数类型使用 C# 风格委托 `Func<T, R>` / `Action<T>`：

```as
int apply(Func<int, int> f, int x) {
    return f(x);
}

Action<string> log = (msg) => Console.WriteLine(msg);
```

方法是函数的特例——在类型系统中视为首个参数为接收者的函数。

### Lambda

- 普通 `=>` lambda 用于运行时委托（Enumerable 路径）。
- `Expression<Func<...>>` 类型的 lambda 在编译期树化（见 [表达式树与查询语言](011-expression-trees-query.md)）。

### 捕获语义

- lambda 捕获外部变量，typeck 脱糖为闭包对象。
- 捕获分为 **by-ref** 与 **by-value** 两类；实例/扩展组捕获亦走同一捕获管线。
- **`this` 捕获（隐式成员访问）**：lambda 体内显式 `this.X`、裸字段名与**裸实例方法调用**（如 `new Thread(() => WatchExit())`）均触发 `this` 捕获（按值捕获 this 指针，见 [002](002-surface-contract.md)）；裸实例方法调用在降级时重写为 `this.Method()`，与裸字段走同一捕获路径——不触发捕获会使宿主类上下文（owner）不传播、调用降级为自由函数调用（符号错位 + 可达性缺失被树摇）。
- **裸静态成员引用**：lambda 体内裸引用 owner 类静态方法/静态属性/静态字段**不捕获 `this`**（保持无 env 的零开销函数指针路径），但 owner 仍传播进 lambda 降级上下文，供裸名解析为限定符号（`Owner::Method`）；缺 owner 会降级为自由函数调用（符号错位）。
- **嵌套 lambda**：内层 lambda 引用宿主实例成员时，`this` 经外层闭包 env 传递捕获（编译器自动传递，C# 嵌套闭包同构）；内层裸实例方法调用同样沿此路径生效。
- 跨函数实参的捕获受 ABI 限制：捕获 `Func` 跨函数实参暂不支持（与显式捕获 lambda 相同）。
- 捕获进堆上闭包的跨度受限（ref-like `Span` 不可捕获，见 [内存模型与资源安全](005-memory-model.md)）。

### 方法组

期望 `Func`/`Action` 时，可将签名兼容的表达式用作委托值，typeck 脱糖为 lambda：

| 形式 | 示例 |
|------|------|
| 自由函数名 | `Result` |
| 静态 `C.Foo` | `C.Foo` |
| 实例 `obj.Foo` | `obj.Foo` |
| 简单接收者扩展方法组（无括号） | `r.Ext` |

**硬拒绝**：复杂实例接收者（`new`/嵌套 Field）、命名空间限定静态、`Expression<>` 方法组、泛型扩展组。签名不匹配与未定义名硬错误，禁止静默。

### 可选参数边界

`TypeId::Func` **不**携带形参默认槽。带默认的 lambda 仅允许立即调用；赋给 `Func`/`Action`、作实参/返回值或入表达式树为**硬错误**。

### AsyncStream（异步事件流）

以 **拉模型（pull-based）异步序列**为核心契约：消费者按需驱动生产者推进，天然背压；生产者逐步产出值。覆盖 UI 事件流、AI 流式推理（TTS 音频块/ASR 转写段）、IO 管道三类场景，作为拒绝 C# `event` 后的替代机制之一（连同响应式属性绑定与委托可调用）。

#### 接口契约（std `Arc.Collections`）

```as
public interface IAsyncEnumerator<out T> {
    T Current();
    Task<bool> MoveNextAsync();
}
public interface IAsyncEnumerable<out T> {
    IAsyncEnumerator<T> GetAsyncEnumerator(CancellationToken cancellationToken);
}
```

对齐 C# `System.Collections.Generic.IAsyncEnumerable<T>`，三点 Arc 化裁剪（单一惯用法）：

| 决策 | C# | Arc | 理由 |
|------|----|----|------|
| `MoveNextAsync` 返回 | `ValueTask<bool>` | `Task<bool>` | 一语义一写法；同步完成快速路径由 `rt_task_poll` inline 直通提供（已缓存单例 `[0,255]` 覆盖 bool 全域），不引入双轨 |
| 异步释放 | `IAsyncDisposable` | 枚举器释放由生产者侧生命周期管理（`Dispose`/dtor 同步链） | `rt_task` 状态机 env 的析构（`dtor_fn`）本为同步 C 链；M 级别不为流单独引入异步释放协议 |
| 取消传递 | `WithCancellation` 扩展 + `GetAsyncEnumerator(ct)` | `GetAsyncEnumerator(ct)` 显式参数；取消属于生产者参数（生成函数签名接 `ct`），消费侧 `await foreach` 脱糖传 `CancellationToken.None`；需消费者取消时手写循环（底层原语全开放） | 消灭 `WithCancellation`/`ConfigureAwait` 双轨；TTS/ASR 场景取消天然在生产者（Face 持 `ct`） |

#### 消费侧：`await foreach`

```as
await foreach (var x in src) { body }
```

脱糖（typeck 标记，MIR lower 展开；codegen 零新原语——全部复用 `MethodCall`/`Await`/`While`/`TryFinally`）：

```as
{
    IAsyncEnumerator<T> e = src.GetAsyncEnumerator(CancellationToken.None);
    try {
        while (true) {
            bool more = await e.MoveNextAsync();
            if (!more) { break; }
            var x = e.Current();
            body
        }
    } finally { e.Dispose(); }
}
```

挂起点语义：`MoveNextAsync` 的 `Task<bool>` 经标准 `await`（inline poll 直通或 EventLoop 挂起）；循环体内可继续 `await`（多挂起点状态机既有能力）。

#### 生产侧（两阶段）

**P1（库层适配，随本 RFC 交付）**：`AsyncStream<T>`——推拉适配器。sink 推入（`OnNext(v)`/`OnCompleted()`/`OnError(ex)`）→ 内部有界环形缓冲（容量构造指定，满则生产者挂起）→ 消费者 `MoveNextAsync` 拉取。AI 流式门面（`AITtsFace.SynthesizeStreamAsync` 等 sink 契约）经 `ToAsyncEnumerable()` 适配，实现 sink 与 AsyncStream 的单一桥接点。

**P2（编译器合成，独立批次）**：`async IAsyncEnumerable<T>` 方法 + `yield return`/`yield break`。编译器合成枚举器类（C# 编译器同构）：env 复用 async 状态机模型（state/current/提升局部），`MoveNextAsync` 每次经 `rt_task_from_state_machine(env, resume)` 新建 task 句柄、复用 env；`yield return x` 脱糖为 `current = x; state = N+1; set_result_int(task, 1); return READY`（本步完成、env 可再入）；方法末尾 `state = -1; set_result_int(task, 0)`。**runtime 零改动**——同一 env 多次 `from_state_machine` 与 state 驱动的 resume 可再入均为既有语义。

#### 背压与并发边界

- 单消费者：枚举器非线程安全（对齐 C#）；多消费者各自 `GetAsyncEnumerator`。
- 背压：拉模型天然——生产者仅在 `MoveNextAsync` 驱动时推进（P2），或缓冲满时挂起（P1）。
- 错误传播：生产者异常经 `MoveNextAsync` 的 `Task` FAULTED 通道（`rt_task_fault`）抛至消费侧 `await` 点；已产出值不撤回（与 RFC 041 流式契约一致）。

### 错误传播回调

`?` 后缀作错误传播的单一形式：`var v = load()?;` 失败提前返回、成功解包，见 [类型系统](004-type-system.md)。

## 边界

- 异步委托与 `Task` 见 [异步与并发模型](009-async-concurrency.md)。
- lambda 的表达式树化见 [表达式树与查询语言](011-expression-trees-query.md)。
- 对象模型与扩展方法见 [对象模型](006-object-model.md)。

## 禁止项

- **C# `event` 关键字**：历史包袱（lapsed listener 泄漏 / AOT 不友好 / 缺乏组合性），委托足够；以 AsyncStream、响应式绑定与委托可调用替代。
- **多播委托订阅机制**。

---

上一节：[007 集合、字符串与数值](007-collections-strings-numerics.md) · 下一节：[009 异步与并发模型](009-async-concurrency.md)