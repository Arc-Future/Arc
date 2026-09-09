namespace Arc.QIF;

using Arc;

/// <summary>
/// 测试集合分组属性。对标 xUnit <c>[Collection]</c>。
/// <para>
/// 并行语义（仅 <c>arc test --parallel</c>）：同名集合内串行，不同集合间并行；
/// 未标注时每个测试类自成隐式集合（对标 xUnit 默认）。默认入口仍串行（RFC 032 确定性优先）。
/// </para>
/// </summary>
[AttributeUsage(AttributeTargets.Class, AllowMultiple = false)]
public class CollectionAttribute : Attribute {
    public string Name { get; }
    public CollectionAttribute(string name) { Name = name; }
}
