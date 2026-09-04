namespace UnitTest.Core;

using Arc;
using Arc.QIF;
using UnitTest.Core.MultiFile;

/// <summary>
/// 跨文件命名空间单元测试：覆盖 MultiFile 示例。
/// 验证编译器跨文件发现类型、跨 namespace 解析、实例方法调用、静态属性访问。
/// </summary>
public class MultiFileTests
{
    [Fact]
    public void CrossFile_InstanceMethod()
    {
        GreetingService svc = new GreetingService("Hi, ");
        string msg = svc.Greet("Arc");
        Assert.True(msg == "Hi, Arc");
    }

    [Fact]
    public void CrossFile_StaticProperty()
    {
        string msg = GreetingService.DefaultMessage;
        Assert.True(msg == "Hello from another file!");
    }
}
