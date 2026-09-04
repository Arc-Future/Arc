// NativeLibraryNotFoundException — 运行时加载 native 模块失败时抛出（RFC 016）
// 对标 C# 互操作生态的库缺失异常语义。
namespace Arc;

/// <summary>
/// 调用 `load = "runtime"`（或 `auto` 降级）且未能加载成功的 native 模块
/// 符号时抛出。开发者应使用 <see cref="Native.IsAvailable"/> 作门闩优雅降级
/// （可选功能灰化），而非依赖异常做流程控制。
/// </summary>
public class NativeLibraryNotFoundException : SystemException {
    public NativeLibraryNotFoundException(string message) : base(message) { }
}
