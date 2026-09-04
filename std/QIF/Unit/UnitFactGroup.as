namespace Arc.QIF;

using Arc;
using Arc.Collections;

/// <summary>
/// 单元测试用例数据载体。对标 XUnit TestCase 元数据模型。
/// L1 Unit 测试体系专用；L2+ 测试模式同理派生。
/// </summary>
internal class UnitFactGroup {

    public UnitFactGroup(string className, string methodName) {
        ClassName = className; MethodName = methodName;
        Kind = QIFTestKind.Fact; Order = 0;
    }

    public UnitFactGroup(string className, string methodName, QIFTestKind kind, int order) {
        ClassName = className; MethodName = methodName;
        Kind = kind; Order = order;
    }

    public string ClassName { get; }
    public string MethodName { get; }
    public QIFTestKind Kind { get; }
    public int Order { get; }
    public string DisplayName { get; set; } = "";
    public string SkipReason { get; set; } = "";
    public List<string> InlineData { get; } = new List<string>();
    public int TimeoutMs { get; set; }
    public List<string> Traits { get; } = new List<string>();

    public void AddInlineData(string d) { InlineData.Add(d); }
    public void AddTrait(string t) { Traits.Add(t); }

    public string FullName {
        get {
            if (DisplayName != "") { return DisplayName; }
            else { return ClassName + "." + MethodName; }
        }
    }
    public bool IsSkipped { get { return SkipReason != ""; } }
    public bool HasTimeout { get { return TimeoutMs > 0; } }
}
