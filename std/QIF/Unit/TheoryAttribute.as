namespace Arc.QIF;

using Arc;

/// <summary>L1 单元测试 Theory 标记属性。对标 XUnit [Theory]。</summary>
[AttributeUsage(AttributeTargets.Method)]
public class TheoryAttribute : Attribute {
    public string DisplayName;
    public string Skip;

    public TheoryAttribute() { this.DisplayName = ""; this.Skip = ""; }
}
