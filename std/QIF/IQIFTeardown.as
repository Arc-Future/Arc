namespace Arc.QIF;

/// <summary>
/// 测试后置清理。实现此接口的测试类在每方法执行后自动调用 Teardown()
/// （含异常路径，由独立的 try-catch 保护）。
/// 对标 XUnit <c>IDisposable.Dispose</c> 的显式面；非集合级 fixture。
/// </summary>
public interface IQIFTeardown {
    void Teardown();
}
