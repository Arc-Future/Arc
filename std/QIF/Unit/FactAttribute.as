namespace Arc.QIF;

using Arc;

/// <summary>L1 单元测试 Fact 标记属性。对标 XUnit [Fact]。</summary>
[AttributeUsage(AttributeTargets.Method)]
public class FactAttribute : Attribute {
    public string DisplayName;
    public string Skip;
    /// <summary>
    /// 单测超时毫秒；0 = 使用 runner 默认（CLI <c>--timeout</c> / <c>[qif].default_timeout</c>）。
    /// 正值覆盖套件默认超时（对标 xUnit Fact.Timeout）。
    /// </summary>
    public int Timeout;

    public FactAttribute() {
        this.DisplayName = "";
        this.Skip = "";
        this.Timeout = 0;
    }
}
