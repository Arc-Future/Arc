namespace Arc.QIF;

/// <summary>
/// 测试前置初始化。实现此接口的测试类在每方法执行前自动调用 Setup()。
/// 对标 XUnit 构造函数模式（xUnit 以构造函数替代显式 Setup）。
///
/// Phase 2c: 由 __QIFTestHost.Main() 生成代码自动检测并调用。
/// </summary>
public interface IQIFSetup {
    void Setup();
}
