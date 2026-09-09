namespace UnitTest.QIF;

using Arc;
using Arc.QIF;

/// <summary>
/// Assert.Throws / DoesNotThrow 稳定面全覆盖。
/// 含 Throws(不限定)、Throws(errorCode)、DoesNotThrow。
/// 泛型 Throws&lt;T&gt; 延后（MIR）；类型身份用 Assert.IsException / IsType(typeof, typeof)。
/// </summary>
[Trait("category", "unit")]
public class AssertThrowsTests
{
    [Fact]
    public void Throws_AnyException_Pass()
    {
        Assert.Throws("boom", () => { throw new Exception("x"); });
    }

    [Fact]
    public void Throws_AnyException_MultipleTypes_Pass()
    {
        Assert.Throws("boom", () => { throw new Exception("invalid"); });
    }

    [Fact]
    public void Throws_ErrorCode_ExactMatch_Pass()
    {
        Assert.Throws("E0340", "test", () => { throw new Exception("E0340: something wrong"); });
    }

    [Fact]
    public void Throws_ErrorCode_Empty_AcceptsAny_Pass()
    {
        Assert.Throws("", "test", () => { throw new Exception("anything"); });
    }

    [Fact]
    public void DoesNotThrow_Pass()
    {
        int x = 1;
        Assert.DoesNotThrow("ok", () => { x = x + 1; });
    }

    [Fact]
    public void DoesNotThrow_EmptyAction_Pass()
    {
        Assert.DoesNotThrow("empty", () => { });
    }
}
