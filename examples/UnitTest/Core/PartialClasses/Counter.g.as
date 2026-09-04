namespace UnitTest.Core.Partial;

// 模拟代码生成器输出的代码（如 UI 控件属性、事件注册）。
public partial class Counter {
    private int _maxValue = 100;

    public void Reset() {
        _count = 0;
    }

    public bool IsFull {
        get { return _count >= _maxValue; }
    }
}
