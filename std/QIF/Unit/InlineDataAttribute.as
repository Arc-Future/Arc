namespace Arc.QIF;

using Arc;
using Arc.Collections;

/// <summary>内联参数数据属性。对标 XUnit [InlineData]。
/// 支持 int / string / bool / double 四种字面量类型参数。
/// </summary>
[AttributeUsage(AttributeTargets.Method, AllowMultiple = true)]
public class InlineDataAttribute : Attribute {

    public InlineDataAttribute() { }
    // int 参数
    public InlineDataAttribute(int a) { Data.Add(a.ToString()); }
    public InlineDataAttribute(int a, int b) { Data.Add(a.ToString()); Data.Add(b.ToString()); }
    public InlineDataAttribute(int a, int b, int c) { Data.Add(a.ToString()); Data.Add(b.ToString()); Data.Add(c.ToString()); }
    // string 参数
    public InlineDataAttribute(string a) { Data.Add(a); }
    public InlineDataAttribute(string a, string b) { Data.Add(a); Data.Add(b); }
    public InlineDataAttribute(string a, string b, string c) { Data.Add(a); Data.Add(b); Data.Add(c); }
    // bool 参数
    public InlineDataAttribute(bool a) { Data.Add(a.ToString()); }
    public InlineDataAttribute(bool a, bool b) { Data.Add(a.ToString()); Data.Add(b.ToString()); }
    public InlineDataAttribute(bool a, bool b, bool c) { Data.Add(a.ToString()); Data.Add(b.ToString()); Data.Add(c.ToString()); }
    // double 参数
    public InlineDataAttribute(double a) { Data.Add(a.ToString()); }
    public InlineDataAttribute(double a, double b) { Data.Add(a.ToString()); Data.Add(b.ToString()); }
    public InlineDataAttribute(double a, double b, double c) { Data.Add(a.ToString()); Data.Add(b.ToString()); Data.Add(c.ToString()); }

    public List<string> Data { get; } = new List<string>();
    public string Serialized { get; set; } = "";
}
