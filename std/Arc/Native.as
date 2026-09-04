// Native — native 互操作门面（RFC 016 运行时库加载统一模型）
// 对齐 C# 的互操作生态风格：静态门面查询运行时加载状态。
namespace Arc;

/// <summary>
/// 查询/断言 `load = "runtime"`（或 `auto` 降级）native 模块的运行时加载状态。
///
/// - <see cref="IsAvailable"/>：模块是否加载成功（首次查询触发一次懒解析）。
///   未安装对应组件时返回 false——用于「安装检测 / 可选功能降级」门闩。
/// - <see cref="ThrowIfUnavailable"/>：模块不可用时抛
///   <see cref="NativeLibraryNotFoundException"/>（codegen 间接调用失败路径调用）。
/// </summary>
public static class Native {
    /// <summary>
    /// 查询 native 模块是否可用（加载成功）。模块名为 `.ani` 中声明的模块名，
    /// 如 <c>Native.IsAvailable("gpu")</c>。首次查询触发懒解析（候选路径依次
    /// <c>rt_library_load</c> + 逐符号 <c>rt_library_sym</c>），之后幂等。
    ///
    /// 本方法由 codegen 内联发射（编译期常量模块名直读 per-module 状态；
    /// 静态链接模块恒 true，未知名称恒 false），定义体不会被执行。
    /// </summary>
    public static bool IsAvailable(string moduleName) {
        return false;
    }

    /// <summary>
    /// 模块不可用时抛出 <see cref="NativeLibraryNotFoundException"/>。
    /// 仅由 codegen 生成的间接调用失败路径调用（该路径已确认模块未加载）。
    /// </summary>
    public static void ThrowIfUnavailable(string moduleName) {
        throw new NativeLibraryNotFoundException(
            "Native module '" + moduleName +
            "' is not available: the required library could not be loaded on this machine.");
    }
}
