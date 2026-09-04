namespace Arc.QIF;

/// <summary>
/// 测试输出捕获。实现此接口并注入到测试方法参数，
/// WriteLine 输出的内容被 QIFRunner 捕获关联到对应 QIFResult.Output。
/// 对标 XUnit ITestOutputHelper。
/// </summary>
public interface IQIFOutput {
    void WriteLine(string message);
}
