// IreeNotAvailableException — IREE Runtime native 库不可用时抛出。
//
// 与 NativeLibraryNotFoundException 语义对齐（RFC 034 / RFC 038）：`load =
// "auto"` 门闩（Native.IsAvailable("iree") == false）时，若开发者未用门闩
// 优雅降级而直接构造/启用后端，则抛本类型。推荐在业务侧先经
// <see cref="IreeModelFactory"/>.IsAvailable 门闩做可选功能灰化。
namespace Arc.AI.Iree;

using Arc;

/// <summary>IREE Runtime native 库不可用（未安装 / ARC_IREE_LIB 未配置 / 符号缺失）。</summary>
public class IreeNotAvailableException : SystemException {
    public IreeNotAvailableException() : base() { }
    public IreeNotAvailableException(string message) : base(message) { }
    public IreeNotAvailableException(string message, Exception? innerException) : base(message, innerException) { }
}
