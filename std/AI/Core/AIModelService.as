// AIModelService — 统一服务基座（RFC 041 §7.3）。
//
// 抽象基类 + 统一执行骨架（与 AIToolHandler 同构；纯 interface 跨程序集分派存在
// 已知编译器缺口，遵循既有抽象基类惯用法）。门面内部域实现只写「语义翻译 + 调用」，
// 超时/重试/预算/序列化/统计一处落地：
//
//   ExecuteAsync 骨架 = Acquire → 超时(Task.WhenAny) → 重试(幂等, 指数退避) →
//   序列化(单实例锁) → 执行(work) → 释放(handle.Dispose) → 统计(RecordRun)。
//
//   流式骨架（RFC 041 §7.9）：ExecuteStreamAsync 持流级句柄（Acquire/EnterCall
//   贯穿流生命周期），域子面回调内逐块调 ExecuteBlockAsync（序列化 + 块级超时
//   + 块级统计；TTS 非幂等 retry=false / ASR 幂等 retry=true）；任何退出路径
//   保证在途收敛 + 句柄释放。
//
//   逐请求模型覆盖（RFC 041 §7.5，已落地）：modelOverride 非空 → 覆盖构造绑定
//   ModelId，取句柄/在途/统计/释放与错误消息一律记到实际执行模型（ResolveModelId
//   统一解析）。
//
// 错误收敛：非 AI 异常包装为 AIModelInferenceException（携带 AIModelError，不裸透
// 原生状态）；OperationCanceledException 收敛为 AIModelCancelledException；AI 异常
// 原样传播。重试仅 Timeout/Inference（RFC §7.3 可重试子类型）；取消/预算/加载
// 不可重试。超时抛弃的在途任务不可强杀（原生 runner 不可中断），诚实边界。
namespace Arc.AI;

using Arc.Diagnostics;
using Arc.Threading;

/// <summary>
/// 统一模型服务基座（RFC 041 §7.3）：抽象基类承载统一执行骨架，域实现（P2 门面
/// 子面）派生本类只写语义翻译 + 调用。经 <see cref="ExecuteAsync"/> 统一执行；
/// 执行期可经 modelOverride 逐请求覆盖绑定模型（RFC 041 §7.5，已落地）。
/// </summary>
public abstract class AIModelService {
    private AIModelRegistry _registry;
    private string _modelId;
    private AIModelServiceOptions _options;
    private Lock _runLock;

    /// <summary>构造服务基座（绑定注册表 + 模型 + 选项）。</summary>
    protected AIModelService(AIModelRegistry registry, string modelId, AIModelServiceOptions options) {
        if (registry == null) {
            throw new ArgumentNullException("registry");
        }
        if (modelId == null || modelId == "") {
            throw new ArgumentException("modelId is required");
        }
        _registry = registry;
        _modelId = modelId;
        _options = options != null ? options : AIModelServiceOptions.Default;
        _runLock = new Lock();
    }

    /// <summary>绑定注册表（域实现读取统计/预算）。</summary>
    protected AIModelRegistry Registry {
        get { return _registry; }
    }

    /// <summary>绑定模型唯一键。</summary>
    protected string ModelId {
        get { return _modelId; }
    }

    /// <summary>服务选项（域实现读取超时/重试/成本档位）。</summary>
    protected AIModelServiceOptions Options {
        get { return _options; }
    }

    /// <summary>
    /// 统一执行骨架：Acquire（懒加载 + refcount+1）→ 在途登记 → 序列化执行
    /// <paramref name="work"/> → 超时/重试/错误收敛 → 统计 → 释放句柄。任何退出
    /// 路径都保证在途收敛 + 句柄释放（refcount−1，归零且策略放行 → 可卸载）。
    /// </summary>
    /// <param name="work">执行体（经 runner 消费，返回语义结果；可捕获调用方取消）。</param>
    /// <param name="ct">协作式取消令牌（已取消抛 <see cref="AIModelCancelledException"/>）。</param>
    /// <param name="modelOverride">逐请求模型覆盖（RFC 041 §7.5）：非 null 且非 ""
    /// 时覆盖构造绑定 ModelId，Acquire/在途/统计/释放记到该模型；null 走绑定默认。</param>
    /// <returns>语义执行结果（域实现返回其域结果类型，派生自 <see cref="AIModelResult"/>）。</returns>
    protected async Task<AIModelResult> ExecuteAsync(
        Func<IAIModel, Task<AIModelResult>> work, CancellationToken ct, string? modelOverride) {
        ct.ThrowIfCancellationRequested();
        // 实际执行模型：逐请求覆盖优先，否则构造绑定默认；骨架内取句柄/在途/
        // 统计/释放与错误消息一律经 effectiveId，保证统计与预算落在实际执行模型。
        string effectiveId = this.ResolveModelId(modelOverride);
        Stopwatch sw = Stopwatch.StartNew();
        AIModelHandle? handle = null;
        bool entered = false;
        // 委托经容器类在异步骨架内传递：async 状态机把 Func 形参直接透传/调用
        // 会损坏闭包环境（编译器缺陷，对齐 Arc/Types/Lazy.as 规避先例），类形参
        // 安全，调用点再从容器取本地 Func 调用。
        AIModelWork holder = new AIModelWork();
        holder.Work = work;
        try {
            AIModelHandle acquired = _registry.Acquire(effectiveId);
            handle = acquired;
            _registry.EnterCall(effectiveId);
            entered = true;
            AIModelResult result = await this.RunWithRetryAsync(holder, acquired.Runner, effectiveId, _options.MaxRetries);
            if (_options.TrackUsage) {
                _registry.RecordRun(effectiveId, sw.ElapsedMilliseconds);
            }
            return result;
        } finally {
            if (entered) {
                _registry.ExitCall(effectiveId);
            }
            if (handle != null) {
                handle.Dispose();
            }
        }
    }

    /// <summary>重试循环（幂等推理）：执行 work → 超时 → 可重试错误按指数退避重试。
    /// 错误消息经 <paramref name="modelId"/> 标识实际执行模型（含逐请求覆盖）。</summary>
    private async Task<AIModelResult> RunWithRetryAsync(
        AIModelWork holder, IAIModel runner, string modelId, int maxRetries) {
        int attempt = 0;
        while (true) {
            try {
                // 序列化（单实例锁）：仅同步执行段持锁（对齐 InferenceSession 既有模式）。
                Task<AIModelResult> workTask = this.RunSerializedAsync(holder, runner);
                return await this.AwaitWithTimeoutAsync(workTask, modelId);
            } catch (Exception ex) {
                // 收敛统一错误层次（解析器不支持多 catch 子句，单 catch + 类型分支）：
                // 可重试 AI 子类型（Timeout/Inference）按 maxRetries 指数退避重试；
                // 取消收敛为 Cancelled；其余非 AI 异常包装为 Inference 错误。
                string errMsg = ex != null && ex.Message != null ? ex.Message : "unknown error";
                if (ex is AIModelException) {
                    AIModelException aiEx = (AIModelException)ex;
                    if (!AIModelService.IsRetryable(aiEx) || attempt >= maxRetries) {
                        throw ex;
                    }
                } else if (ex is OperationCanceledException) {
                    throw new AIModelCancelledException("model call cancelled: " + modelId);
                } else if (attempt >= maxRetries) {
                    throw new AIModelInferenceException(
                        "model inference failed: " + modelId + ": " + errMsg,
                        AIModelError.FromException(ex));
                }
                attempt = attempt + 1;
                await Task.Delay(this.BackoffMs(attempt));
            }
        }
    }

    /// <summary>
    /// 解析实际执行模型（RFC 041 §7.5 逐请求覆盖）：<paramref name="modelOverride"/>
    /// 非空且非 "" 时覆盖构造绑定 ModelId，否则走绑定默认。批式/流式骨架与域子面
    /// 回显统一经本方法（null/"" 归一——string? 与字面量直接比较有类型检查缺口）。
    /// </summary>
    protected string ResolveModelId(string? modelOverride) {
        string requested = modelOverride ?? "";
        return requested != "" ? requested : _modelId;
    }

    /// <summary>
    /// 流式块执行原语（RFC 041 §7.9）：单块经序列化锁 + 块级超时执行，错误收敛同
    /// 统一骨架，块级统计（TrackUsage → RecordRun）在此落地。调用方（流式域子面）
    /// 已持有流级句柄（<see cref="ExecuteStreamAsync"/>），本原语只管单块执行；
    /// <paramref name="retry"/> = false 时单次执行不重试（TTS 非幂等硬规则），
    /// true 时块级重试按 Options.MaxRetries（ASR 幂等，出块前完成）。
    /// </summary>
    /// <param name="work">单块执行体（经 runner 消费，返回块/段语义结果）。</param>
    /// <param name="runner">流级持有句柄的运行器。</param>
    /// <param name="modelId">实际执行模型（含逐请求覆盖），错误消息与统计标识用。</param>
    /// <param name="retry">是否启用块级重试（幂等域 true / 非幂等域 false）。</param>
    protected async Task<AIModelResult> ExecuteBlockAsync(
        Func<IAIModel, Task<AIModelResult>> work, IAIModel runner,
        string modelId, bool retry) {
        // 委托经容器类传递（编译器缺陷规避，对齐 ExecuteAsync 注释）。
        AIModelWork holder = new AIModelWork();
        holder.Work = work;
        int maxRetries = retry ? _options.MaxRetries : 0;
        Stopwatch sw = Stopwatch.StartNew();
        AIModelResult result = await this.RunWithRetryAsync(holder, runner, modelId, maxRetries);
        if (_options.TrackUsage) {
            _registry.RecordRun(modelId, sw.ElapsedMilliseconds);
        }
        return result;
    }

    /// <summary>
    /// 流式编排骨架（RFC 041 §7.9）：Acquire 流级句柄 + 在途登记贯穿流生命周期，
    /// 域子面回调 <paramref name="streamWork"/> 内分块逐块执行（经
    /// <see cref="ExecuteBlockAsync"/>）并增量投递 sink；任何退出路径（成功/取消/
    /// 失败）保证在途收敛 + 句柄释放。错误不在此收敛投递——域子面 catch 后经
    /// <see cref="ToStreamError"/> 收敛并投递 sink.OnError（已产出块不撤回）。
    /// </summary>
    /// <param name="streamWork">流式执行体（持 runner 分块循环；取消经抛
    /// OperationCanceledException 传播，由域子面收敛；返回值无语义恒 null
    /// ——无返回 async lambda 无法转换 <c>Func&lt;X, Task&gt;</c> 属编译器缺口，
    /// 经带值签名规避）。</param>
    /// <param name="ct">协作式取消令牌。</param>
    /// <param name="modelOverride">逐请求模型覆盖（RFC 041 §7.5）。</param>
    protected async Task ExecuteStreamAsync(
        Func<IAIModel, Task<AIModelResult>> streamWork, CancellationToken ct, string? modelOverride) {
        ct.ThrowIfCancellationRequested();
        string effectiveId = this.ResolveModelId(modelOverride);
        AIModelHandle? handle = null;
        bool entered = false;
        // 委托经容器类传递（编译器缺陷规避，对齐 ExecuteAsync 注释）。
        AIModelWork holder = new AIModelWork();
        holder.Work = streamWork;
        try {
            AIModelHandle acquired = _registry.Acquire(effectiveId);
            handle = acquired;
            _registry.EnterCall(effectiveId);
            entered = true;
            Func<IAIModel, Task<AIModelResult>> work = holder.Work;
            await work(acquired.Runner);
        } finally {
            if (entered) {
                _registry.ExitCall(effectiveId);
            }
            if (handle != null) {
                handle.Dispose();
            }
        }
    }

    /// <summary>流式路径错误收敛（RFC 041 §7.9）：AI 异常原样、取消收敛 Cancelled、
    /// 其余包装 Inference——供域子面 catch 后投递 sink.OnError。</summary>
    protected static AIModelException ToStreamError(Exception ex, string modelId) {
        if (ex is AIModelException) {
            return (AIModelException)ex;
        }
        if (ex is OperationCanceledException) {
            return new AIModelCancelledException("model stream cancelled: " + modelId);
        }
        string errMsg = ex != null && ex.Message != null ? ex.Message : "unknown error";
        return new AIModelInferenceException(
            "model stream failed: " + modelId + ": " + errMsg,
            AIModelError.FromException(ex));
    }

    /// <summary>序列化执行：单实例锁内调度 work（仅同步段持锁，await 前释放）。</summary>
    private Task<AIModelResult> RunSerializedAsync(
        AIModelWork holder, IAIModel runner) {
        lock (_runLock) {
            // 本地装载后调用（编译器缺陷规避，对齐 ExecuteAsync 容器注释）。
            Func<IAIModel, Task<AIModelResult>> work = holder.Work;
            return work(runner);
        }
    }

    /// <summary>超时竞争：TimeoutMs 内未完成抛 <see cref="AIModelTimeoutException"/>
    /// （可重试；消息标识实际执行模型）。同步 runner（常见路径）经首轮 poll 即完成，
    /// 直接返回且不创建 Delay 定时器——避免孤儿定时器把 EventLoop 拖到 TimeoutMs
    /// 之后才退出（进程收尾空等）。</summary>
    private async Task<AIModelResult> AwaitWithTimeoutAsync(Task<AIModelResult> workTask, string modelId) {
        if (_options.TimeoutMs <= 0) {
            return await workTask;
        }
        // 快速路径：先 poll 一次 workTask（同步 Wait(1ms) 驱动状态机；同步 runner
        // 首轮即完成、立即返回，无阻塞）。同步 runner（常见路径）不创建 Delay
        // 定时器——避免孤儿定时器把 EventLoop 拖到 TimeoutMs 之后才退出（进程收尾
        // 空等）。真正异步的 workTask 保持 PENDING，落到慢路径再挂超时定时器。
        if (workTask.Wait(1)) {
            return await workTask;
        }
        // 慢路径：真实异步执行，与超时定时器竞争。
        await Task.WhenAny(workTask, Task.Delay(_options.TimeoutMs));
        if (!workTask.IsCompleted && !workTask.IsFaulted) {
            throw new AIModelTimeoutException("model call timed out after "
                + _options.TimeoutMs + " ms: " + modelId);
        }
        return await workTask;
    }

    /// <summary>指数退避：backoff * 2^(attempt-1)。</summary>
    private int BackoffMs(int attempt) {
        if (_options.RetryBackoffMs <= 0) {
            return 0;
        }
        int backoff = _options.RetryBackoffMs;
        int i = 1;
        while (i < attempt) {
            backoff = backoff * 2;
            i = i + 1;
        }
        return backoff;
    }

    /// <summary>可重试子类型：Timeout / Inference（RFC §7.3 映射表）。取消/预算/加载不可重试。</summary>
    private static bool IsRetryable(AIModelException ex) {
        return ex is AIModelTimeoutException || ex is AIModelInferenceException;
    }
}

/// <summary>服务骨架内部委托容器（编译器缺陷规避：async 状态机透传 Func 形参
/// 损坏闭包环境，经类形参 + 调用点本地装载规避；对齐 Arc/Types/Lazy.as 先例）。</summary>
internal class AIModelWork {
    /// <summary>待调用的执行体。</summary>
    public Func<IAIModel, Task<AIModelResult>> Work;
}
