# RFC 046 通道——多生产者/多消费者通信

## 背景

std 现有并发通信面存在明确空档：`AsyncStream<T>`（[008](008-delegates-closures.md)）是**单消费者**推拉适配器（多消费者禁止共用一个流）；`BlockingCollection<T>`（[024](024-concurrent-collections.md)）提供**同步阻塞**（线程级）生产者-消费者容器，且其 `TakeAsync` 被明确排除在设计面外；`Concurrent*` 集合只有非阻塞 `Try*` 语义。多生产者/多消费者（MPMC）场景——工作队列、事件扇出、管道中间级——缺少一个 **Task 原生、异步挂起、自带背压**的通信原语。

设计目标：对标 `System.Threading.Channels` 精华（抽象工厂、读写端分离、完成信号、四种背压模式），以 Arc 单一惯用法收敛公开面，与 `Arc.Threading` 同族公开范式一致。

## 设计决策

### 类型面（`Arc.Threading.Channels`，七公开类型 + internal 实现层）

| 类型 | 形状 | 说明 |
|------|------|------|
| `Channel<T>` | class | 通道句柄；`Reader` / `Writer` 只读属性（构造期绑定，自定义 getter）；构造函数收 internal 读写端，工厂独占实例化 |
| `Channels` | static class | 工厂枢纽：`CreateBounded<T>(int)` / `CreateBounded<T>(BoundedChannelOptions)` / `CreateUnbounded<T>()` |
| `ChannelReader<T>` | abstract | 读端契约（方法形态）：`CanCount()` / `Count()` / `Completion()` / `TryRead` / `ReadAsync` / `ReadAllAsync` |
| `ChannelWriter<T>` | abstract | 写端契约：`TryWrite` / `WriteAsync` / `Complete` |
| `BoundedChannelOptions` | class | `Capacity`（ctor 设定）+ `FullMode`（默认 `Wait`） |
| `BoundedChannelFullMode` | enum | `Wait` / `DropOldest` / `DropNewest` / `DropWrite`（`Wait` 为 0 值默认） |
| `ChannelClosedException` | Exception 派生 | 通道终结后读写抛出 |
| `ChannelCore<T>` 等 6 internal 类型 | internal | 状态机核心、读写端实现、等待者、枚举器；经工厂创建，不进公开面 |

命名规则：Arc 不支持同名泛型/非泛型类，故 .NET 的非泛型 `Channel` 工厂枢纽命名为 `Channels`（静态类，无静态字段）。`Channel<T>` 构造函数收 internal 读写端实现——外部无法绕过工厂实例化；具体实现层（`ChannelCore<T>` 等）不进公开面，保留未来 native `rt_channel_*` 快路径的替换空间。

### 核心语义

| 操作 | 语义 |
|------|------|
| `TryWrite(item)` | 有空位即入队（**读等待者存在时直付**：绕过缓冲直接交付首个未终结读等待者）；满时按 `FullMode`：`Wait`→false，`DropOldest`→逐出最旧后写入，`DropNewest`/`DropWrite`→丢弃传入元素并返回 true；通道已终结→false |
| `WriteAsync(item, ct)` | 快路径同 `TryWrite`；`Wait` 模式且满时挂起写等待者（FIFO），消费者腾出空位后由其**收纳等待者元素入缓冲**再唤醒（O(1) 交接，无竞态重试环）；通道终结后抛 `ChannelClosedException` |
| `TryRead(out item)` | 缓冲有值即出队；出队后依次执行**空位回收**（首个写等待者元素入缓冲并唤醒）与**完成判定**（已终结且排尽→完成信号）；空返回 false（含已终结排尽） |
| `ReadAsync(ct)` | 快路径同 `TryRead`；空且未终结挂起读等待者；已终结：以 `Complete(error)` 的 error 抛出，无 error 抛 `ChannelClosedException` |
| `Complete(error = null)` | 终结写端：挂起中的读等待者以 error（无 error 则 `ChannelClosedException`）失败，写等待者以 `ChannelClosedException` 失败；缓冲中已产出值仍可被消费完毕；排尽后 `Completion` 完成；重复终结抛 `ChannelClosedException` |
| `Completion` | 完成信号 Task：终结且排尽后正常完成；以 error 终结则该 Task 携带 error 失败 |
| `Count` / `CanCount` | 缓冲中当前元素数（直付中的元素不计入）；两实现均支持计数 |
| `ReadAllAsync(ct)` | 异步枚举全部元素（`IAsyncEnumerable<T>`，基类默认实现）；`ChannelClosedException` 归为序列结束，其余异常（含取消）原样传播 |

### 背压（BoundedChannelFullMode）

| 模式 | 满 时 行 为 |
|------|------------|
| `Wait` | 生产者异步挂起直至消费者腾出空位（默认；真背压） |
| `DropOldest` | 逐出缓冲中最旧元素，写入新元素 |
| `DropNewest` | 丢弃传入元素（缓冲保持不变），调用视为成功 |
| `DropWrite` | 丢弃传入元素，调用视为成功 |

无界通道（`CreateUnbounded<T>`）写端永不等待；`FullMode` 不适用。

### 取消

进入即检查（`ThrowIfCancellationRequested`，编译器反糖 OCE）；挂起中经 `CancellationToken.Register` 协作取消——取消回调在锁内将等待者标记为已终结并以哨兵值唤醒，`ReadAsync`/`WriteAsync` 唤醒后依等待者状态分派：已交付→返回；已取消→`ThrowIfCancellationRequested` 抛 OCE。登记与注册之间的窗口由注册后复查 `IsCancellationRequested` 闭合。注册不可注销（CT 现无注册句柄），等待者终态守卫使回调幂等，无泄漏路径。

### 实现与分层

纯 Arc 实现（`std/Arc/Threading/Channels/`）：`Lock` + `Monitor` 串行化全部状态迁移，`TaskCompletionSource` 承载读写等待者挂起（SetResult/SetException/哨兵唤醒均为已验证通道），无界缓冲为倍增环形数组。**零 codegen/runtime 变更**——架构红线（编译器只发射机制，高级抽象全部由 std 构建）的正向实例；与 `BlockingCollection`（Builtin facade → 纯 C runtime，同步阻塞）互补分层。

### 诚实差异（对齐 .NET 之处与偏差）

- 读端契约的 `CanCount` / `Count` / `Completion` 为**方法形态**：泛型基类抽象属性 override 触发编译器挂死缺陷（见实现前置），方法 override 为已验证路径；编译器修复后可回升属性形态。
- `Completion()` 为 `Task<bool>`：TCS 无法承载 void 完成信号（`SetResult(void)` 不可调用），`Task<bool>` 与 `AsyncStream` 的 `Task<bool>` 信号同族；正常终结恒 true，异常终结以 error 失败。
- 不设 `UnboundedChannelOptions`：无可诚实承载的成员（空旋钮类）；`CreateUnbounded<T>()` 无参工厂覆盖。
- 不设 `SingleReader` / `SingleWriter`：Monitor 串行化实现下为假旋钮；未来 native 快路径引入时随 RFC 扩展选项面。
- 不设 `WaitToReadAsync` / `WaitToWriteAsync` / `TryComplete`：select 组合与软终结不在本设计面内；`ReadAsync`/`TryRead`/`Complete` 覆盖既有用例（单一惯用法）。
- 直付语义（读等待者存在时新元素绕过缓冲）与 .NET 一致；跨消费者不保证全局 FIFO 序（MPMC 本质），单一读端内 FIFO 保证。
- `Channel<T>` 为具体类而非抽象基：Arc 泛型类图单态化成熟度下的务实形态，工厂独占实例化由 internal 构造签名保证；具体实现层（internal）仍可整体替换为 native 快路径。

```as
using Arc;
using Arc.Threading.Channels;

class Pipeline {
    private Channel<int> _channel;

    public Pipeline() {
        _channel = Channels.CreateBounded<int>(16);
    }

    public async Task Produce(int seed) {
        for (int i = 0; i < 10; i++) {
            await _channel.Writer.WriteAsync(seed * 100 + i);
        }
    }

    public async Task<int> Consume() {
        int sum = 0;
        IAsyncEnumerator<int> e = _channel.Reader.ReadAllAsync().GetAsyncEnumerator(CancellationToken.None);
        while (true) {
            bool more = await e.MoveNextAsync();
            if (!more) {
                break;
            }
            sum = sum + e.Current;
        }
        return sum;
    }

    public void Seal() {
        _channel.Writer.Complete();
    }
}

async Task<void> Main() {
    Pipeline pipeline = new Pipeline();
    Task p1 = pipeline.Produce(1);
    Task p2 = pipeline.Produce(2);
    pipeline.Seal();
    int a = await pipeline.Consume();
    int b = await pipeline.Consume();
    await Task.WhenAll(p1, p2);
    Console.WriteLine((a + b).ToString());
}
```

## 实现前置与缺口收口账本

本设计的 std 实现已就绪（`std/Arc/Threading/Channels/`）。收口过程中定位并修复/登记如下编译器与运行期缺口：

### 已修复（本变更集，配套回归 `l2_mono_default_args`）

1. **抽象 out 形参方法的确定性赋值误判**（`check_class.rs`）：无方法体（abstract）的 out 形参方法被无条件执行 out-flow 检查——泛型基类抽象 `TryRead(out T)` 在单态化注册期被判失败，级联拖垮整个类图（`Channel_int` 成员表为空的表象根因）。修复：无体方法跳过该检查。
2. **单态化类方法默认值折叠丢失**（`check_generics.rs`）：`register_monomorphized_class` 的 OopSig 构造将 `default` 硬编码 None——单态化泛型类上省略默认实参的调用报 no matching overload。修复：对齐 `method_sig_from_ast` 折叠。
3. **省略实参的 default(T) 填充**（`check_call_bind.rs`）：填充以 null 字面量表达，值类型/stub 槽（CancellationToken）拒绝 null。修复：Null 默认值以 `default(槽类型)` 表达。

### 已规避（设计侧按已验证形态收敛，随编译器演进回升）

- 读端契约成员为方法形态：泛型基类抽象属性 override 触发编译器挂死（非泛型 `Stream` 同形无此问题）。
- 枚举器持 `Func<Task<T>>` 委托：泛型类交叉引用（Reader ⇄ Enumerator）令单态化注册中断。
- `await` 作用于局部变量：await 直接作用于调用表达式的协程 lowering 有 SSA 支配缺口（`await _readOne()` → 先落 `pending` 局部）。
- None 令牌守卫：`ct.CanBeCanceled` 为假时不 Register（默认令牌无 cts 载体）。
- 不用 `int.MaxValue` 哨兵（原语成员访问 MIR lowering 缺口）与 `Queue<T>.Count`（可达性裁剪 arc-prune-001 对嵌套泛型单态过度裁剪）；等待者队列以显式计数器控长。

### 剩余缺口（唤醒链 native 崩溃，验收 harness 已 `#[ignore]` 挂账）

**Wait 模式挂起写唤醒链 native 崩溃**（0xC0000005）：有界通道写端首次挂起（`WriteEnqueue` → 等待者入队 → 消费者 `AdmitWriters` 收纳 → 退锁 → `SetResult` 唤醒 → 写端协程续跑）链路崩溃，"W0 begin" 后无输出。证据链：读端挂起（ReadEnqueue suspend-registered）→ 生产端 TryWrite space → 崩溃；`l2_channels_batch` 案例 1（无界 MPMC，写端全内联完成）通过、案例 2+（挂起写）崩溃；`ServeReader` 借出协议（退锁后 SetResult）不改变崩溃。下一步：native 级协程唤醒链取证（coro env 槽位/Task&lt;bool&gt; 结果提取/双层 await 续跑），修复后去 `#[ignore]` 即达可宣称状态。

> 2026-09-02 进展注记：**同域**的 accept-null「提取过早」缺陷已根治（await lowering 改 re-poll 提取 + 第二挂起点/重挂起，见 [048 §6.2](048-named-pipes.md)；`l2_net_batch` 修复后 6/6 全绿）。
>
> 2026-09-02 二次注记（取证三层穿透，详见 stability architecture review (internal record)）：① 第一层已修——`parse_queue_elem` 对嵌套泛型元素（`Queue_ChannelReaderWaiter_int`）的 `contains('_')` 拒绝使 Enqueue 分发落回 stub 空体（数据黑洞）→ `ServeReader` 读 NULL waiter 0xC0000005；② 第二层已定位待修——修复推进后暴露 `rt_arc_inc(0x2)`：泛型 async 方法（`WriteAsync<T>`）的 T 参数单态化为值类型后仍被判 class 做帧授予 inc（`arc_class_place` 收到未单态化类型）。三层取证由常备基建完成（`ARC_CRASH_PROBE` VEH + `ARC_DEBUG_LINK_SYMBOLS` PDB + publics 符号化）。架构级根治设计（统一对象头 D1 / 判定布局化 D2 / 唤醒协议收敛 D3 / 取证常态化 D4）见评审文档；`bounded_backpressure` 去 `#[ignore]` 以 #12 修复后复验为准。

## 边界

- 本文档只讲 MPMC 通道**类型与语义**；线程原语与调度模型见 [009 异步与并发模型](009-async-concurrency.md)，并发集合（同步阻塞面）见 [024 并发集合](024-concurrent-collections.md)，单消费者推拉适配器见 [008 委托、闭包与方法组](008-delegates-closures.md)。
- select 组合（多通道同时等待）、`WaitToReadAsync`/`WaitToWriteAsync` 轮询原语、优先级/去重策略、native `rt_channel_*` 快路径不在本设计面内。
- 通道为进程内通信原语；跨进程/跨节点消息传递见 [025 网络协议层](025-networking.md) 与 [042 P2P 网络](042-p2p.md)。

---

上一节：[045 插件内核](045-chord.md) · [返回 RFC 索引](index.md)
