namespace UnitTest.Core.Partial;

// 模拟用户编写的业务逻辑代码（业务字段、事件处理器）。
public partial class Counter {
    private int _count = 0;

    public int Count {
        get { return _count; }
    }

    public void Increment() {
        _count = _count + 1;
    }

    public void Decrement() {
        _count = _count - 1;
    }
}
