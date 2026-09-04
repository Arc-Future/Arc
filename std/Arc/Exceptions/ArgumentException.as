// ArgumentException — 参数异常基类（RFC 027 M0）
// 对标 C# System.ArgumentException（两参 ctor 暂缓，见下）。
namespace Arc;

/// <summary>
/// 参数无效时抛出的异常基类。
/// 派生类：ArgumentNullException、ArgumentOutOfRangeException。
///
/// **公开面诚实约束（L2）**：
/// - ParamName 为可写字段（空串 = 无参数名）。
/// - **不**提供 `ArgumentException(string message, string paramName)`：该两参 ctor
///   在与派生类同 TU 编译时触发 0xC0000005（codegen 多 string 形参 ctor 债）。
///   需要 ParamName 时：用派生异常，或 `new ArgumentException(msg)` 后写 `ParamName`。
/// </summary>
public class ArgumentException : SystemException {
    /// <summary>导致异常的参数名；无参数名时为空串。</summary>
    public string ParamName;

    public ArgumentException(string message) : base(message) {
        this.ParamName = "";
    }

    public ArgumentException(string message, Exception? innerException)
        : base(message, innerException)
    {
        this.ParamName = "";
    }
}