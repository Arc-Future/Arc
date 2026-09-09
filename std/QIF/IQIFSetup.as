namespace Arc.QIF;

/// <summary>
/// 测试前置初始化。实现此接口的测试类在每方法执行前自动调用 Setup()。
/// 对标 XUnit 构造函数模式（xUnit 以构造函数替代显式 Setup）的显式面。
/// 非 <c>IClassFixture</c>——共享 fixture 生命周期不在本面（RFC 032 边界）。
/// </summary>
public interface IQIFSetup {
    void Setup();
}
