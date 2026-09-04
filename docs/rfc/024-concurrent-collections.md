# RFC 024 并发集合

## 背景

多线程环境下的共享容器需要无锁或低竞争的原语。设计目标：`Arc.Collections.Concurrent` 五类型一接口，全部 Builtin facade → 纯 C 运行时（`rt_concurrent_*`），对标 C# `System.Collections.Concurrent` 精华并规避其历史缺陷（D1–D7）。

## 设计决策

### 五类型一接口

| 类型 | 载体 | 说明 |
|------|------|------|
| `ConcurrentDictionary<K,V>` | `rt_concurrent_dict_*` | 并发关联表；`TryAdd`/`TryGetValue`/`TryRemove`/`TryUpdate`/`GetOrAdd(V)`/`Keys` |
| `ConcurrentQueue<T>` | `rt_concurrent_queue_*` | 并发队列；`TryPeek` |
| `ConcurrentBag<T>` | `rt_concurrent_bag_*` | 并发包 |
| `ConcurrentStack<T>` | `rt_concurrent_stack_*` | 并发栈 |
| `BlockingCollection<T>` | `rt_concurrent_blocking_*` | 阻塞集合；`TryAdd`/`TryTake`/`IsCompleted` |
| `IConcurrentCollection<T>` | — | 统一抽象；多态 ctor 经其构造具体类型 |

**设计决策**：

- **全 Builtin facade → 纯 C runtime**：所有并发类型经 `[Builtin(ABI="rt_concurrent_*")]` 直射，无 Arc 侧手写算法；Stable 面链接即真符号，禁 panic 半物化或静默 0。
- **确定性锁原语**：`Monitor.TryEnter`/`Mutex.TryLock` 与 `lock {}` 糖提供并发互斥；`Interlocked` 提供 `Increment`/`Exchange`/`CompareExchange`（int）。`Monitor` Pulse/Wait 供高级同步。
- **压力可证伪**：并发数据结构的压力行为以并发基准用例验证（不依赖性能阈值）。
- 定制比较器 ctor、批次出队（batch dequeue）、`TakeAsync`、背压切换不在本设计面内。

```as
using Arc.Collections;
using Arc.Collections.Concurrent;

var dict = new ConcurrentDictionary<string, int>();
dict.TryAdd("a", 1);
int v;
if (dict.TryGetValue("a", out v)) { /* … */ }
dict.TryRemove("a", out v);
```

### 与调度器配合

并发集合与 `ThreadPoolScheduler`/任务调度配合使用；`lock {}` 与 `Monitor` 为线程间同步的单一惯用法。并发深度下的 hazard/epoch 回收不在本设计面内（交由运行时回收语义）。

## 边界

- 本文档只讲并发集合**类型**；线程、`Thread`/`Monitor`/`Parallel`/`Interlocked`/`Lazy<T>` 与调度模型见语言并发规范（见 [009 异步与并发模型](009-async-concurrency.md)）。
- 非并发容器（`List`/`Dictionary`/`HashSet` 等）见 [021 集合、IO 与文本](021-collections-io-text.md)。
- DI 生命周期与作用域见 [023 数学、张量与依赖注入](023-math-tensor-di.md)。

---

上一节：[023 数学、张量与依赖注入](023-math-tensor-di.md) · 下一节：[025 网络协议层](025-networking.md)