namespace Arc.QIF;

using Arc;

/// <summary>L1 单元测试 Fact 标记属性。对标 XUnit [Fact]。</summary>
[AttributeUsage(AttributeTargets.Method)]
public class FactAttribute : Attribute {
    public string DisplayName;
    public string Skip;

    public FactAttribute() { this.DisplayName = ""; this.Skip = ""; }
}
