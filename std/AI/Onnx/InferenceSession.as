// InferenceSession — ONNX Runtime 推理会话（内部实现细节，实现 IAIModel）。
//
// 生命周期：构造时创建 Ort::Env + Ort::Session（加载模型）；RunAsync 执行推理；
// Dispose 释放 session 与 env。每个 InferenceSession 自持一个 Env，彼此隔离。
//
// 加载失败 / 推理失败抛 <see cref="OnnxException"/>；库不可用抛
// <see cref="OnnxNotAvailableException"/>。业务侧经公开 <see cref="OnnxModelFactory"/>
// 获得本会话（以 <see cref="Arc.AI.IAIModel"/> 面消费，不感知后端差异）。
//
// 异步：<see cref="RunAsync"/> 为唯一执行入口（异步为主，带取消令牌）。ONNX
// session.Run 为同步阻塞原生调用，且**非并发安全**（同一 session 并发 Run 数据竞争）
// ——本实现以 <c>Lock</c> 串行化 + 调用线程直接执行（async-over-sync，对齐
// SocketsHttpHandler.SendAsync 的 Task.FromResult(同步体) 惯例；Task.Run 线程池卸载
// 因「lambda 内异常无法经 C trampoline 展开」语言缺口后置，见宣称纪律）。
namespace Arc.AI.Onnx;

using Arc;
using Arc.Collections;
using Arc.Threading;
using Arc.Text;

/// <summary>ONNX Runtime 推理会话（内部实现细节；经 <see cref="Arc.AI.IAIModel"/> 消费）。
/// 实现 <see cref="IDisposable"/> 与 <see cref="Arc.AI.IAIModel"/>。</summary>
internal class InferenceSession : IAIModel {
    /// <summary>输入/输出名缓冲容量（字节；模型张量名通常远小于此）。</summary>
    private const int NameBufferSize = 256;

    /// <summary>输入/输出形状探针初始维度容量（>8 时二次放大查询）。</summary>
    private const int ShapeProbeCap = 8;

    private NativePtr _env;
    private NativePtr _session;
    private Lock _lock;

    /// <summary>使用默认 <see cref="SessionOptions"/> 加载模型（CPU + 默认线程）。</summary>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    public InferenceSession(string modelPath) {
        this.Initialize(modelPath, new SessionOptions(), true);
    }

    /// <summary>使用指定会话选项加载模型。</summary>
    /// <param name="modelPath">.onnx 模型文件路径。</param>
    /// <param name="options">会话选项（调用方负责其 Dispose；会话已拷贝配置）。</param>
    public InferenceSession(string modelPath, SessionOptions options) {
        if (options == null) {
            throw new ArgumentNullException("options");
        }
        this.Initialize(modelPath, options, false);
    }

    private void Initialize(string modelPath, SessionOptions options, bool disposeOptions) {
        if (modelPath == null) {
            throw new ArgumentNullException("modelPath");
        }
        OnnxNative.EnsureAvailable();
        _lock = new Lock();

        NativePtr env = null;
        int rc = onnx.onnx_create_env(2, "arc", out env);
        OnnxNative.ThrowIfFailed(rc);
        _env = env;

        NativePtr s = null;
        rc = onnx.onnx_create_session(_env, modelPath, options.Handle, out s);
        if (rc != 0) {
            onnx.onnx_release_env(_env);
            _env = null;
            OnnxNative.ThrowIfFailed(rc);
        }
        _session = s;

        if (disposeOptions) {
            options.Dispose();
        }
    }

    /// <summary>ONNX Runtime native 库是否可用（`load="auto"` 门闩，用于可选功能灰化）。</summary>
    public static bool IsAvailable {
        get { return Native.IsAvailable("onnx"); }
    }

    /// <summary>模型输入张量数量。</summary>
    public int InputCount {
        get {
            int c = 0;
            int rc = onnx.onnx_session_input_count(_session, out c);
            OnnxNative.ThrowIfFailed(rc);
            return c;
        }
    }

    /// <summary>模型输出张量数量。</summary>
    public int OutputCount {
        get {
            int c = 0;
            int rc = onnx.onnx_session_output_count(_session, out c);
            OnnxNative.ThrowIfFailed(rc);
            return c;
        }
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量名。</summary>
    public string GetInputName(int index) {
        List<byte> buf = OnnxNative.AllocBytes((long)NameBufferSize);
        int len = 0;
        int rc = onnx.onnx_session_get_input_name(_session, index, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return Encoding.GetString(buf.ToArray());
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量元素类型（经 shim 查询，非运行）。</summary>
    public TensorElementType GetInputElementType(int index) {
        int elem = 0;
        int dimCount = 0;
        List<long> probe = OnnxNative.AllocLongs((long)0);
        int rc = onnx.onnx_session_get_input_info(_session, index, out elem, probe, out dimCount);
        OnnxNative.ThrowIfFailed(rc);
        return OnnxNative.FromTensorElementType(elem);
    }

    /// <summary>取第 <paramref name="index"/> 个输出张量元素类型（经 shim 查询，非运行）。</summary>
    public TensorElementType GetOutputElementType(int index) {
        int elem = 0;
        int dimCount = 0;
        List<long> probe = OnnxNative.AllocLongs((long)0);
        int rc = onnx.onnx_session_get_output_info(_session, index, out elem, probe, out dimCount);
        OnnxNative.ThrowIfFailed(rc);
        return OnnxNative.FromTensorElementType(elem);
    }

    /// <summary>取第 <paramref name="index"/> 个输入张量形状（未知维为 -1）。</summary>
    public List<long> GetInputShape(int index) {
        return this.GetSessionShape(true, index);
    }

    /// <summary>取第 <paramref name="index"/> 个输出张量形状（未知维为 -1）。</summary>
    public List<long> GetOutputShape(int index) {
        return this.GetSessionShape(false, index);
    }

    /// <summary>查询某张量的形状：先探针维度数，超容量再放大（对齐 OnnxTensor.QueryMetadata）。</summary>
    private List<long> GetSessionShape(bool input, int index) {
        int elem = 0;
        List<long> probe = OnnxNative.AllocLongs((long)ShapeProbeCap);
        int dimCount = 0;
        int rc = input
            ? onnx.onnx_session_get_input_info(_session, index, out elem, probe, out dimCount)
            : onnx.onnx_session_get_output_info(_session, index, out elem, probe, out dimCount);
        OnnxNative.ThrowIfFailed(rc);
        if (dimCount <= ShapeProbeCap) {
            List<long> shape = new List<long>();
            int i = 0;
            while (i < dimCount) {
                shape.Add(probe[i]);
                i = i + 1;
            }
            return shape;
        }
        List<long> full = OnnxNative.AllocLongs((long)dimCount);
        rc = input
            ? onnx.onnx_session_get_input_info(_session, index, out elem, full, out dimCount)
            : onnx.onnx_session_get_output_info(_session, index, out elem, full, out dimCount);
        OnnxNative.ThrowIfFailed(rc);
        return full;
    }

    /// <summary>取第 <paramref name="index"/> 个输出张量名（内部使用）。</summary>
    private string GetOutputName(int index) {
        List<byte> buf = OnnxNative.AllocBytes((long)NameBufferSize);
        int len = 0;
        int rc = onnx.onnx_session_get_output_name(_session, index, buf, out len);
        OnnxNative.ThrowIfFailed(rc);
        return Encoding.GetString(buf.ToArray());
    }

    /// <summary>
    /// 异步执行推理（唯一执行入口）。输入按位置提供（数量须等于 <see cref="InputCount"/>，
    /// 按 <see cref="GetInputName"/> 映射到 ONNX 命名输入）；返回全部输出（位置序）。
    /// 会话非并发安全——本实现以 Lock 串行化并发调用。
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

        // 会话串行化：ONNX Runtime session 非并发安全，同一 runner 并发 Run 须串行。
        lock (_lock) {
            return Task.FromResult(this.RunOnnx(inputs));
        }
    }

    /// <summary>同步执行核心（调用方已持 <c>_lock</c>）：转换 → 原生 Run → 转换回宿主。</summary>
    private List<Tensor> RunOnnx(List<Tensor> inputs) {
        // 位置序输入 → OnnxTensor（宿主拷贝进 ONNX 自有存储）。
        List<OnnxTensor> inTensors = new List<OnnxTensor>();
        int i = 0;
        while (i < inputs.Count) {
            inTensors.Add(this.ToOnnx(inputs[i]));
            i = i + 1;
        }

        // 输入名 NUL 分隔 blob（位置序；经 GetInputName 映射）。
        List<byte> inNames = new List<byte>();
        i = 0;
        while (i < inputs.Count) {
            string name = this.GetInputName(i);
            byte[] nb = Encoding.GetBytes(name);
            int j = 0;
            while (j < nb.Length) {
                inNames.Add(nb[j]);
                j = j + 1;
            }
            inNames.Add((byte)0);
            i = i + 1;
        }

        // 输入句柄（List<long> 携带 NativePtr 值）。
        List<long> inValues = new List<long>();
        i = 0;
        while (i < inTensors.Count) {
            inValues.Add((long)inTensors[i].Handle);
            i = i + 1;
        }

        // 输出：空名 blob → 产出全部输出；缓冲容量 = OutputCount。
        List<byte> outNames = new List<byte>();
        List<long> outValues = OnnxNative.AllocLongs((long)this.OutputCount);
        int outCount = 0;
        int rc = onnx.onnx_run(_session, inNames, inValues, outNames, outValues, out outCount);
        OnnxNative.ThrowIfFailed(rc);

        // 输出 OnnxTensor → 宿主张量（转换后释放输出句柄）。
        List<Tensor> result = new List<Tensor>();
        int p = 0;
        while (p < outCount) {
            OnnxTensor ot = new OnnxTensor((NativePtr)outValues[p], true);
            result.Add(this.ToArc(ot));
            ot.Dispose();
            p = p + 1;
        }

        // 释放输入 OnnxTensor（ONNX 已把数据拷入自有存储，可即释）。
        i = 0;
        while (i < inTensors.Count) {
            inTensors[i].Dispose();
            i = i + 1;
        }

        return result;
    }

    // ── Tensor 互转（后端适配：Arc.AI.Tensor ↔ OnnxTensor）──

    /// <summary>宿主张量 → OnnxTensor（按元素类型读宿主缓冲 + 形状创建）。</summary>
    private OnnxTensor ToOnnx(Tensor t) {
        TensorElementType et = t.ElementType;
        if (et == TensorElementType.Float32) { return OnnxTensor.CreateFloat(t.Shape, t.ReadFloat()); }
        if (et == TensorElementType.Float64) { return OnnxTensor.CreateDouble(t.Shape, t.ReadDouble()); }
        if (et == TensorElementType.Int32) { return OnnxTensor.CreateInt32(t.Shape, t.ReadInt32()); }
        if (et == TensorElementType.Int64) { return OnnxTensor.CreateInt64(t.Shape, t.ReadInt64()); }
        if (et == TensorElementType.UInt8) { return OnnxTensor.CreateByte(t.Shape, t.ReadByte()); }
        throw new OnnxException("Unsupported input tensor element type: " + et);
    }

    /// <summary>OnnxTensor → 宿主张量（读回原生数据为宿主缓冲）。</summary>
    private Tensor ToArc(OnnxTensor ot) {
        TensorElementType et = ot.ElementType;
        if (et == TensorElementType.Float32) { return Tensor.CreateFloat(ot.Shape, ot.ReadFloat()); }
        if (et == TensorElementType.Float64) { return Tensor.CreateDouble(ot.Shape, ot.ReadDouble()); }
        if (et == TensorElementType.Int32) { return Tensor.CreateInt32(ot.Shape, ot.ReadInt32()); }
        if (et == TensorElementType.Int64) { return Tensor.CreateInt64(ot.Shape, ot.ReadInt64()); }
        if (et == TensorElementType.UInt8) { return Tensor.CreateByte(ot.Shape, ot.ReadByte()); }
        throw new OnnxException("Unsupported output tensor element type: " + et);
    }

    /// <summary>释放 session 与 env 句柄（幂等）。</summary>
    public void Dispose() {
        if (_session != null) {
            onnx.onnx_release_session(_session);
            _session = null;
        }
        if (_env != null) {
            onnx.onnx_release_env(_env);
            _env = null;
        }
    }
}
