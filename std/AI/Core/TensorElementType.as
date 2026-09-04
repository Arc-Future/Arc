// RFC 041 §1.5: Arc.AI 共享抽象核心 — 张量元素数据类型。
//
// 后端无关的元素类型（区别于 Arc.AI.Onnx.TensorElementType 的 ONNX 数值码与
// IREE buffer_view 元素码）。仅覆盖宿主可承载的类型化缓冲类型；后端适配器负责
// 各自原生元素码 → 本枚举映射。单一惯用法：业务侧经 Arc.AI.Tensor 统一消费。
namespace Arc.AI;

/// <summary>宿主张量元素数据类型（后端无关）。</summary>
public enum TensorElementType {
    /// <summary>未定义。</summary>
    Undefined = 0,

    /// <summary>单精度浮点。</summary>
    Float32 = 1,

    /// <summary>双精度浮点。</summary>
    Float64 = 2,

    /// <summary>有符号 32 位整数。</summary>
    Int32 = 3,

    /// <summary>有符号 64 位整数。</summary>
    Int64 = 4,

    /// <summary>无符号 8 位整数（byte）。</summary>
    UInt8 = 5,

    /// <summary>有符号 16 位整数（PCM int16 音频等；RFC 041 §7.4 P1）。</summary>
    Int16 = 6,
}
