// ArgumentNullException — 参数为 null 异常（RFC 027 M0）
namespace Arc;

/// <summary>将 null 传递给不接受 null 的参数时抛出。</summary>
public class ArgumentNullException : ArgumentException {
    // 一参 ctor 的 Message 对标 C#："Value cannot be null. (Parameter 'xxx')"；
    // 参数名走 ParamName 字段，不得误作 message 传基类。
    public ArgumentNullException(string paramName) : base("Value cannot be null. (Parameter '" + paramName + "')") {
        this.ParamName = "";
        this.ParamName = paramName;
    }

    public ArgumentNullException(string paramName, string message) : base(message) {
        this.ParamName = "";
        this.ParamName = paramName;
    }
}