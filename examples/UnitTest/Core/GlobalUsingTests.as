/// <summary>
/// 全局 using 单元测试：覆盖 GlobalUsing 示例。
/// 本文件故意不含 `using Arc;` 或 `using Arc.QIF;`，依赖 GlobalUsings.as 提供全局导入。
/// 验证 global using 机制对 IDisposable、[Fact]、Assert.* 的解析正确性。
/// </summary>

namespace UnitTest.Core;

class DisposableBox : IDisposable {
    public int Id;

    public DisposableBox(int id) {
        Id = id;
    }

    public void Dispose() {
    }
}

public class GlobalUsingTests
{
    [Fact]
    public void UsingVar_WithoutLocalImport()
    {
        using var r = new DisposableBox(42);
        Assert.Equal(42, r.Id);
    }
}
