namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Native callback trampoline 端到端验证（RFC 047 M1）。
/// 
/// 验证 codegen 将无捕获 lambda 正确转换为 C ABI 函数指针：
///   1. 识别委托类型为 native callback
///   2. 生成 trampoline 函数（剥离 env 参数，匹配 C ABI 签名）
///   3. C 端通过函数指针调用 trampoline → 返回正确结果
/// 
/// 这是编译器基础设施的核心验证点——trampoline 路径直接影响
/// 所有 FFI 回调场景的正确性。
/// </summary>
public class NativeCallbackTests
{
    /// <summary>无捕获 lambda → C 函数指针（减法）。</summary>
    [Fact]
    public void CallbackTrampoline_Subtract()
    {
        int result = arc_test.rt_native_test_call_cb((a, b) => a - b, 10, 3);
        Assert.Equal(7, result);
    }

    /// <summary>验证不同 lambda 体（乘法）也能正确生成 trampoline。</summary>
    [Fact]
    public void CallbackTrampoline_Multiply()
    {
        int mul = arc_test.rt_native_test_call_cb((a, b) => a * b, 4, 5);
        Assert.Equal(20, mul);
    }

    /// <summary>加法作为控制组，验证 trampoline 不是仅对减法硬编码。</summary>
    [Fact]
    public void CallbackTrampoline_Add()
    {
        int sum = arc_test.rt_native_test_call_cb((a, b) => a + b, 100, 200);
        Assert.Equal(300, sum);
    }
}
