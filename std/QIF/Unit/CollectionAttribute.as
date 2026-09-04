namespace Arc.QIF;

using Arc;

/// <summary>测试集合分组属性。对标 XUnit [Collection]。</summary>
[AttributeUsage(AttributeTargets.Class, AllowMultiple = true)]
public class CollectionAttribute : Attribute {
    public string Name { get; }
    public CollectionAttribute(string name) { Name = name; }
}
