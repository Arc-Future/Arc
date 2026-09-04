// ArgumentOutOfRangeException — 参数超出范围异常（RFC 027 M0）
namespace Arc;

/// <summary>参数值超出允许范围时抛出。</summary>
public class ArgumentOutOfRangeException : ArgumentException {
    // 一参 ctor 的 Message 对标 C#："Specified argument was out of range of valid values. (Parameter 'xxx')"；
    // 参数名走 ParamName 字段，不得误作 message 传基类。
    public ArgumentOutOfRangeException(string paramName) : base("Specified argument was out of range of valid values. (Parameter '" + paramName + "')") {
        this.ParamName = "";
        this.ParamName = paramName;
    }

    public ArgumentOutOfRangeException(string paramName, string message) : base(message) {
        this.ParamName = "";
        this.ParamName = paramName;
    }
}