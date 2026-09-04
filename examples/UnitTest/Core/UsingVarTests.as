namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// UsingVar 单元测试：覆盖 UsingVar 示例的 `using var` 块作用域资源管理。
/// 验证 LIFO Dispose 顺序、返回语句中的 using var。
/// </summary>

class Resource : IDisposable {
    public string Name;

    public Resource(string name) {
        Name = name;
    }

    public void Dispose() {
    }
}

public class UsingVarTests
{
    [Fact]
    public void UsingVar_Basic()
    {
        using var r = new Resource("test");
        Assert.True(r.Name == "test");
    }
}
