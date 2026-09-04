// IreeSession — IREE Runtime 推理会话（内部实现细节，实现 IAIModel）。
//
// 生命周期：构造时创建 IREE runtime instance + 加载 .vmfb 模块；RunAsync 执行推理；
// Dispose 释放模块与 instance。每个 IreeSession 自持一个 instance，彼此隔离。
//
// IREE 函数为**位置形参**调用（区别于 ONNX 命名输入）：本会话以
// <paramref name="functionName"/> 定位 .vmfb 中待执行的导出函数，输入按位置提供，
// 返回全部输出（位置序）。InputCount/OutputCount 经 shim 的
// iree_invoke_arg_count 查询；GetInputName 返回空串（契约不暴露形参名）。
//
// 加载失败 / 推理失败抛 <see cref="IreeException"/>；库不可用抛
// <see cref="IreeNotAvailableException"/>。业务侧经公开 <see cref="IreeModelFactory"/>
// 获得本会话（以 <see cref="Arc.AI.IAIModel"/> 面消费，不感知后端差异）。
//
// 异步：<see cref="RunAsync"/> 为唯一执行入口（异步为主，带取消令牌）。IREE
// invoke 为同步阻塞原生调用，且同一会话**非并发安全**（并发 invoke 数据竞争）
// ——本实现以 <c>Lock</c> 串行化 + 调用线程直接执行（async-over-sync，对齐
// SocketsHttpHandler.SendAsync / InferenceSession 的 Task.FromResult(同步体) 惯例；
// Task.Run 线程池卸载因「lambda 内异常无法经 C trampoline 展开」语言缺口后置）。
namespace Arc.AI.Iree;

using Arc;
using Arc.AI;
using Arc.Collections;
using Arc.Threading;

/// <summary>IREE Runtime 推理会话（内部实现细节；经 <see cref="Arc.AI.IAIModel"/> 消费）。
/// 实现 <see cref="IDisposable"/> 与 <see cref="Arc.AI.IAIModel"/>。</summary>
internal class IreeSession : IAIModel {
    private NativePtr _instance;
    private NativePtr _module;
    private string _functionName;
    private Lock _lock;

    /// <summary>创建 IREE runtime + 加载 .vmfb 模块。</summary>
    /// <param name="modulePath">.vmfb 模块文件路径。</param>
    /// <param name="functionName">待执行的导出函数名（位置形参调用）。</param>
    public IreeSession(string modulePath, string functionName) {
        if (modulePath == null) {
            throw new ArgumentNullException("modulePath");
        }
        if (functionName == null) {
            throw new ArgumentNullException("functionName");
        }
        IreeNative.EnsureAvailable();
        _lock = new Lock();
        _functionName = functionName;

        NativePtr inst = null;
        int rc = iree.iree_create_runtime(2, "arc", out inst);
        IreeNative.ThrowIfFailed(rc);
        _instance = inst;

        NativePtr m = null;
        rc = iree.iree_load_module(_instance, modulePath, out m);
        if (rc != 0) {
            iree.iree_release_runtime(_instance);
            _instance = null;
            IreeNative.ThrowIfFailed(rc);
        }
        _module = m;
    }

    /// <summary>IREE Runtime native 库是否可用（`load="auto"` 门闩，用于可选功能灰化）。</summary>
    public static bool IsAvailable {
        get { return Native.IsAvailable("iree"); }
    }

    /// <summary>模型输入张量数量（经 shim 函数 I/O 计数查询）。</summary>
    public int InputCount {
        get {
            int c = 0;
            int o = 0;
            int rc = iree.iree_invoke_arg_count(_module, _functionName, out c, out o);
            IreeNative.ThrowIfFailed(rc);
            return c;
        }
    }

    /// <summary>模型输出张量数量（经 shim 函数 I/O 计数查询）。</summary>
    public int OutputCount {
        get {
            int c = 0;
            int o = 0;
            int rc = iree.iree_invoke_arg_count(_module, _functionName, out c, out o);
            IreeNative.ThrowIfFailed(rc);
            return o;
        }
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量名。
    /// IREE 函数为位置形参调用，契约不暴露形参名，故返回空串（对齐接口默认）。</summary>
    public string GetInputName(int index) {
        return "";
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量元素类型。IREE 函数契约不暴露
    /// 形参类型，诚实返回 <see cref="TensorElementType.Undefined"/>（对齐 GetInputName 空串边界）。</summary>
    public TensorElementType GetInputElementType(int index) {
        return TensorElementType.Undefined;
    }

    /// <summary>取第 <paramref name="index"/> 个输出张量元素类型。IREE 函数契约不暴露
    /// 形参类型，诚实返回 <see cref="TensorElementType.Undefined"/>。</summary>
    public TensorElementType GetOutputElementType(int index) {
        return TensorElementType.Undefined;
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量形状。IREE 函数契约不暴露
    /// 形参形状，诚实返回空表（对齐 GetInputName 空串边界）。</summary>
    public List<long> GetInputShape(int index) {
        List<long> empty = new List<long>();
        return empty;
    }

    /// <summary>取第 <paramref name="index"/> 个输出张量形状。IREE 函数契约不暴露
    /// 形参形状，诚实返回空表。</summary>
    public List<long> GetOutputShape(int index) {
        List<long> empty = new List<long>();
        return empty;
    }

    /// <summary>
    /// 异步执行推理（唯一执行入口）。输入按位置提供（数量须等于 <see cref="InputCount"/>，
    /// 位置序映射到函数形参）；返回全部输出（位置序）。会话非并发安全——本实现以
    /// Lock 串行化并发调用。
    /// </summary>
    /// <param name="inputs">按位置提供的输入张量。</param>
    /// <param name="cancellationToken">协作式取消令牌（已取消抛 <see cref="OperationCanceledException"/>）。</param>
    /// <returns>位置序输出张量列表。</returns>
    public Task<List<Tensor>> RunAsync(List<Tensor> inputs, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        if (inputs == null) {
            throw new ArgumentNullException("inputs");
        }
        if (inputs.Count != this.InputCount) {
            throw new ArgumentException("input count mismatch: expected " + this.InputCount + " got " + inputs.Count);
        }

        // 会话串行化：IREE 会话非并发安全，同一 runner 并发 invoke 须串行。
        lock (_lock) {
            return Task.FromResult(this.RunIree(inputs));
        }
    }

    /// <summary>同步执行核心（调用方已持 <c>_lock</c>）：转换 → 原生 invoke → 转换回宿主。</summary>
    private List<Tensor> RunIree(List<Tensor> inputs) {
        // 位置序输入 → IreeBufferView（宿主拷贝进 IREE 自有存储）。
        List<IreeBufferView> inViews = new List<IreeBufferView>();
        int i = 0;
        while (i < inputs.Count) {
            inViews.Add(this.ToIree(inputs[i]));
            i = i + 1;
        }

        // 输入句柄（List<long> 携带 NativePtr 值）。
        List<long> inValues = new List<long>();
        i = 0;
        while (i < inViews.Count) {
            inValues.Add((long)inViews[i].Handle);
            i = i + 1;
        }

        // 输出：缓冲容量 = OutputCount（IREE 输出张量数量经函数 I/O 计数已知）。
        List<long> outValues = IreeNative.AllocLongs((long)this.OutputCount);
        int outCount = 0;
        int rc = iree.iree_invoke(_module, _functionName, inValues, outValues, out outCount);
        IreeNative.ThrowIfFailed(rc);

        // 输出 IreeBufferView → 宿主张量（转换后释放输出句柄）。
        List<Tensor> result = new List<Tensor>();
        int p = 0;
        while (p < outCount) {
            IreeBufferView bv = new IreeBufferView((NativePtr)outValues[p], true);
            result.Add(this.ToArc(bv));
            bv.Dispose();
            p = p + 1;
        }

        // 释放输入 IreeBufferView（IREE 已把数据拷入自有存储，可即释）。
        i = 0;
        while (i < inViews.Count) {
            inViews[i].Dispose();
            i = i + 1;
        }

        return result;
    }

    // ── Tensor 互转（后端适配：Arc.AI.Tensor ↔ IreeBufferView）──

    /// <summary>宿主张量 → IreeBufferView（按元素类型读宿主缓冲 + 形状创建）。</summary>
    private IreeBufferView ToIree(Tensor t) {
        TensorElementType et = t.ElementType;
        if (et == TensorElementType.Float32) { return IreeBufferView.CreateFloat(t.Shape, t.ReadFloat()); }
        if (et == TensorElementType.Float64) { return IreeBufferView.CreateDouble(t.Shape, t.ReadDouble()); }
        if (et == TensorElementType.Int32) { return IreeBufferView.CreateInt32(t.Shape, t.ReadInt32()); }
        if (et == TensorElementType.Int64) { return IreeBufferView.CreateInt64(t.Shape, t.ReadInt64()); }
        if (et == TensorElementType.UInt8) { return IreeBufferView.CreateByte(t.Shape, t.ReadByte()); }
        throw new IreeException("Unsupported input tensor element type: " + et);
    }

    /// <summary>IreeBufferView → 宿主张量（读回原生数据为宿主缓冲）。</summary>
    private Tensor ToArc(IreeBufferView bv) {
        TensorElementType et = bv.ElementType;
        if (et == TensorElementType.Float32) { return Tensor.CreateFloat(bv.Shape, bv.ReadFloat()); }
        if (et == TensorElementType.Float64) { return Tensor.CreateDouble(bv.Shape, bv.ReadDouble()); }
        if (et == TensorElementType.Int32) { return Tensor.CreateInt32(bv.Shape, bv.ReadInt32()); }
        if (et == TensorElementType.Int64) { return Tensor.CreateInt64(bv.Shape, bv.ReadInt64()); }
        if (et == TensorElementType.UInt8) { return Tensor.CreateByte(bv.Shape, bv.ReadByte()); }
        throw new IreeException("Unsupported output tensor element type: " + et);
    }

    /// <summary>释放 module 与 instance 句柄（幂等）。</summary>
    public void Dispose() {
        if (_module != null) {
            iree.iree_release_module(_module);
            _module = null;
        }
        if (_instance != null) {
            iree.iree_release_runtime(_instance);
            _instance = null;
        }
    }
}
