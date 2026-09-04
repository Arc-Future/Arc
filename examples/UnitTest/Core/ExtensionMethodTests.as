namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 泛型扩展方法单元测试：在 ObjectModelTests.as 的非泛型部分基础上，
/// 验证 RFC 010 的泛型扩展方法（GenericExtensions.Identity<T> / DescribeType<T>）。
/// 
/// 注：扩展方法对基元类型（int/string/bool）的接收者暂不支持。
/// </summary>

class NamedBox {
    public string Label;

    public NamedBox(string label) {
        Label = label;
    }
}

public static class BoxExtensions {
    public static T Identity<T>(this T x) {
        return x;
    }

    public static string Describe<T>(this T x) {
        return "boxed";
    }
}

public class ExtensionMethodTests
{
    [Fact]
    public void GenericExtension_Identity_ClassReceiver()
    {
        NamedBox b = new NamedBox("hello");
        NamedBox result = b.Identity<NamedBox>();
        Assert.True(result.Label == "hello");
    }

    [Fact]
    public void GenericExtension_DescribeType_ClassReceiver()
    {
        NamedBox b = new NamedBox("test");
        string desc = b.Describe<NamedBox>();
        Assert.True(desc == "boxed");
    }
}
