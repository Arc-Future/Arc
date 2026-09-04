namespace Arc.QIF;

using Arc;

/// <summary>测试分类标记属性。对标 XUnit [Trait("name", "value")]。
/// 可用于类级和方法级。</summary>
[AttributeUsage(AttributeTargets.All, AllowMultiple = true)]
public class TraitAttribute : Attribute {
    public string Name { get; }
    public string Value { get; }

    public TraitAttribute(string name, string value) {
        Name = name;
        Value = value;
    }
}
