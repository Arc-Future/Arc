// GraphOptimizationLevel — 会话图优化级别。
//
// 对齐 ONNX Runtime GraphOptimizationLevel 枚举数值
// （ORT_DISABLE_ALL=0 · ORT_ENABLE_BASIC=1 · ORT_ENABLE_EXTENDED=2 ·
// ORT_ENABLE_ALL=3），与 onnx_options_set_graph_opt_level 的 level 参数一致。
// 数值直接映射底层 C 枚举，勿改动。
namespace Arc.AI.Onnx;

/// <summary>图优化级别（数值对齐 ONNX Runtime GraphOptimizationLevel）。</summary>
public enum GraphOptimizationLevel {
    /// <summary>禁用所有优化。</summary>
    Disable = 0,

    /// <summary>基础图优化（常量折叠、冗余消除等）。</summary>
    Basic = 1,

    /// <summary>扩展图优化（融合等，推荐推理默认）。</summary>
    Extended = 2,

    /// <summary>全部优化（布局/权重预打包等）。</summary>
    All = 3,
}
