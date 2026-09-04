// SessionOptions — ONNX Runtime 会话选项。
//
// 封装 shim 的 SessionOptions 句柄（NativePtr），提供线程数 / 图优化级别 /
// 执行提供程序（CPU 基线 + DirectML）配置。构造即创建原生选项；配置方法
// 逐一映射 shim 函数；Dispose 释放原生句柄。传递会话后由 InferenceSession
// 接管读取，调用方仍需负责本对象 Dispose。
namespace Arc.AI.Onnx;

using Arc;

/// <summary>ONNX Runtime 会话选项（线程 / 图优化 / 执行提供程序）。</summary>
public class SessionOptions : IDisposable {

    /// <summary>创建默认会话选项（CPU + 扩展图优化建议由调用方显式设置）。</summary>
    public SessionOptions() {
        OnnxNative.EnsureAvailable();
        NativePtr o = null;
        int rc = onnx.onnx_create_session_options(out o);
        OnnxNative.ThrowIfFailed(rc);
        Handle = o;
    }

    /// <summary>原生 SessionOptions 句柄（内部使用）。</summary>
    internal NativePtr Handle { get; set; }

    /// <summary>设置进程内算子（intra-op）并行线程数。</summary>
    /// <param name="n">线程数（&lt;= 0 使用默认）。</param>
    public void SetIntraOpNumThreads(int n) {
        int rc = onnx.onnx_options_set_intra_op_threads(Handle, n);
        OnnxNative.ThrowIfFailed(rc);
    }

    /// <summary>设置算子间（inter-op）并行线程数。</summary>
    /// <param name="n">线程数（&lt;= 0 使用默认）。</param>
    public void SetInterOpNumThreads(int n) {
        int rc = onnx.onnx_options_set_inter_op_threads(Handle, n);
        OnnxNative.ThrowIfFailed(rc);
    }

    /// <summary>设置图优化级别。推理推荐 <see cref="GraphOptimizationLevel.Extended"/>。</summary>
    public void SetGraphOptimizationLevel(GraphOptimizationLevel level) {
        int rc = onnx.onnx_options_set_graph_opt_level(Handle, (int)level);
        OnnxNative.ThrowIfFailed(rc);
    }

    /// <summary>追加执行提供程序。<see cref="ExecutionProvider.Cpu"/> 为基线（无操作）；
    /// <see cref="ExecutionProvider.DirectML"/> 附加 Windows GPU 后端（设备 0）。</summary>
    public void UseExecutionProvider(ExecutionProvider provider) {
        if (provider == ExecutionProvider.DirectML) {
            int rc = onnx.onnx_options_append_dml(Handle, 0);
            OnnxNative.ThrowIfFailed(rc);
        }
    }

    /// <summary>释放原生 SessionOptions 句柄（幂等）。</summary>
    public void Dispose() {
        if (Handle != null) {
            onnx.onnx_release_session_options(Handle);
            Handle = null;
        }
    }
}
