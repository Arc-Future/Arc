namespace Arc.QIF;

using Arc;

/// <summary>L1 单元测试 Theory 标记属性。对标 XUnit [Theory]。</summary>
[AttributeUsage(AttributeTargets.Method)]
public class TheoryAttribute : Attribute {
    public string DisplayName;
    public string Skip;
    /// <summary>
    /// 单测超时毫秒；0 = 使用 runner 默认。正值覆盖套件默认超时。
    /// </summary>
    public int Timeout;

    public TheoryAttribute() {
        this.DisplayName = "";
        this.Skip = "";
        this.Timeout = 0;
    }
}
