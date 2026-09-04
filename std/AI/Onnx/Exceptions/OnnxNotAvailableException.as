// OnnxNotAvailableException — ONNX Runtime native 库不可用时抛出。
//
// 与 NativeLibraryNotFoundException 语义对齐（RFC 034 / RFC 038）：`load =
// "auto"` 门闩（Native.IsAvailable("onnx") == false）时，若开发者未用门闩
// 优雅降级而直接构造会话，则抛本类型。推荐在业务侧先经
// <see cref="OnnxNative"/>.IsAvailable 门闩做可选功能灰化。
namespace Arc.AI.Onnx;

using Arc;

/// <summary>ONNX Runtime native 库不可用（未安装 / ARC_ONNX_LIB 未配置 / 符号缺失）。</summary>
public class OnnxNotAvailableException : SystemException {
    public OnnxNotAvailableException() : base() { }
    public OnnxNotAvailableException(string message) : base(message) { }
    public OnnxNotAvailableException(string message, Exception? innerException) : base(message, innerException) { }
}
