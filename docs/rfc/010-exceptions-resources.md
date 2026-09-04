# RFC 010 异常与资源管理

## 背景

Arc 采用 **zero-cost 异常处理**：异常在未抛出路径零运行时开销（LLVM `invoke`/`landingpad` + 最小 personality），跨 FFI/async 正确传播，finally 在深层 unwind 也执行，catch 类型过滤按 C# 语义生效。资源确定性释放经 `using` / `IDisposable`。

## 设计决策

### zero-cost EH（invoke / landingpad）

| 决策 | 内容 |
|------|------|
| 调用点 | 每个 may-throw 调用点（词法 `nounwind` 分析谓词判定）在 try / try-finally 区域内：`call` → `invoke ... to label %normal unwind label %lpad`；非 may-throw 调用保持 `call` |
| landingpad | `%lpad` 块：`landingpad { ptr, i32 } catch ptr null`（catch-all selector） |
| personality | 模块级声明并附加到 may-throw 用户函数：Windows `__CxxFrameHandler3`（SEH 主平台）/ POSIX `__gxx_personality_v0`（Itanium） |
| 穿透帧 | 无 try/finally 的中间 may-throw 函数保持 `call` + 函数级 personality + `uwtable`，unwind 相位穿透 |
| nounwind | 仅「可证明不抛」的函数标 `nounwind`；误标会阻止 unwind 穿透 |

**未抛出路径零开销**：invoke 仅存在于 try/finally 区域内，区域外 `call` 不变；无 setjmp env、无帧注册。

### 异常对象与线程安全

- 异常对象传递 **per-thread（TLS）**：`rt_exception` TLS 化，landingpad 触发后经 `rt_get_exception()` 取回。
- 异常对象与普通 `class` 同布局（可被循环收集器字段 walk 覆盖）；throw 处持有 +1，catch 绑定/释放按 ARC 常规路径。
- 未处理异常收敛到 `rt_panic("unhandled exception")`。
- `rt_throw` / `rt_get_exception` / `rt_format_stacktrace` 符号保留（仅实现换为 native raise）。

### `try` / `catch` / `finally`

C# 对齐表面，**单 catch 子句**（Arc 单一惯用法）：

```as
try {
    var v = load();
} catch (FormatException e) when (e.Message != "") {
    Console.WriteLine(e.ToString());
} finally {
    cleanup();
}
```

| 语义 | 规则 |
|------|------|
| catch 类型过滤 | landingpad 入口经 vtable `is` 检查：`catch (T e)` 仅匹配 `T` 及其派生类；不匹配 → rethrow 继续 unwind |
| `when` | 类型过滤通过后求值 `when (cond)`；false → 继续向外传播 |
| finally | **恒执行**：无论异常来自同帧 throw、深层 callee、FFI 或 await 提取；正常路径 / 同函数 return / 同函数 throw 仍**编译期内联**（零开销），深层 unwind 经 cleanup landingpad 执行后 `resume` |
| 多 catch | 维持单 catch 子句，不引入 C# 多 catch 链 |

### 与 async 状态机协作

- resume 函数内发射 landingpad 区域，覆盖 try 区域跨 await 拆分后两侧（state-0 与 resume 块）。
- 异常必在某个 resume 调用执行期间抛出：faulted Task 的异常由 await 提取点 rethrow（`rt_task_get_exception`），同步 throw 在首次调用内——catch/finally 总在同一 resume 调用内完成。
- 不引入跨状态机调用的通用异常持久化。

### 与 ARC / 循环收集协作

- unwind 期间析构：landingpad / cleanup 内对活跃局部发射 `rt_arc_dec`；`rt_arc_dec` 已 `nounwind`（不重入 unwind，防无限递归）。
- 循环收集延迟释放不因 EH 机制改变；异常路径上 rc→0 对象照常延迟至下次收集。
- finally 内显式 `Dispose` 仍是确定性释放手段。

### `using` / `IDisposable`

```as
// using 语句糖 → try/finally + Dispose
using (var reader = OpenReader()) {
    Process(reader);
}

// using var：作用域结束时释放
using var conn = OpenConnection();
Query(conn);
```

- `using` 将实现 `IDisposable` 的资源在作用域结束时确定性释放，是资源管理的第一惯用法。
- 与 ARC 协作：确定性释放手段优先于引用计数回退。

## 边界

- 内存确定性释放与 ARC 析构时序见 [内存模型与资源安全](005-memory-model.md)。
- 异步异常传播见 [异步与并发模型](009-async-concurrency.md)。
- 运行时 `rt_throw` / `rt_get_exception` 符号面见 [运行时 ABI](014-runtime-abi.md)。

## 禁止项

- **不引入 C++ 完整 typeinfo 匹配**（personality 保持最小 catch-all；类型过滤在生成代码 vtable 检查层）。
- **不依赖 setjmp/longjmp 全栈扫描**。
- **不引入多 catch 子句链**（单一惯用法）。
- **不引入 `unsafe` 用户面**。

---

上一节：[009 异步与并发模型](009-async-concurrency.md) · 下一节：[011 表达式树与查询语言](011-expression-trees-query.md)