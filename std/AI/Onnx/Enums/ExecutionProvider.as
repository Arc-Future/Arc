// ExecutionProvider — ONNX Runtime 执行提供程序（后端加速）。
//
// 对齐 Microsoft.ML.OnnxRuntime.SessionOptions.AppendExecutionProvider 的
// 可选后端语义。CPU 为基线（恒可用、开箱即用）；DirectML 为 Windows GPU
// 加速后端（经 onnx_options_append_dml 附加，仅 Windows + DirectX 12 设备）。
namespace Arc.AI.Onnx;

/// <summary>执行提供程序（后端加速器）。</summary>
public enum ExecutionProvider {
    /// <summary>CPU（基线，恒可用）。追加无操作——CPU 是默认 EP。</summary>
    Cpu,

    /// <summary>DirectML（Windows GPU，DirectX 12）。追加失败时推理会回退，但仍可运行。</summary>
    DirectML,
}
