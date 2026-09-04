namespace Arc.QIF;

/// <summary>
/// 测试后置清理。实现此接口的测试类在每方法执行后自动调用 Teardown()
/// （含异常路径，由独立的 try-catch 保护）。
/// 对标 XUnit IDisposable.Dispose 模式。
///
/// Phase 2c: 由 __QIFTestHost.Main() 生成代码自动检测并调用。
/// </summary>
public interface IQIFTeardown {
    void Teardown();
}
