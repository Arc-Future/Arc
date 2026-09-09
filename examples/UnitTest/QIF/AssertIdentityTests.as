namespace UnitTest.QIF;

using Arc;
using Arc.Collections;
using Arc.QIF;
using Arc.Reflection;

/// <summary>
/// Assert 同一性 / 谓词 / 类型身份 稳定面（对标 xUnit Same/NotSame/All/Any/Single(pred)/IsType）。
/// 零 Fact-Skip；失败路径不进默认套件。
/// </summary>
[Trait("category", "unit")]
public class AssertIdentityTests
{
    // ── Same / NotSame ──

    [Fact]
    public void Same_SameInstance_Pass()
    {
        List<int> a = new List<int>();
        List<int> b = a;
        Assert.Same(a, b);
    }

    [Fact]
    public void NotSame_DistinctInstances_Pass()
    {
        List<int> a = new List<int>();
        List<int> b = new List<int>();
        Assert.NotSame(a, b);
    }

    [Fact]
    public void Same_NullBoth_Pass()
    {
        object a = null;
        object b = null;
        Assert.Same(a, b);
    }

    // ── Equal(bool) ──

    [Fact]
    public void Equal_Bool_Pass()
    {
        Assert.Equal(true, true);
        Assert.Equal(false, false);
    }

    [Fact]
    public void NotEqual_Bool_Pass()
    {
        Assert.NotEqual(true, false);
    }

    // ── All / Any / Single(predicate) ──

    [Fact]
    public void All_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        Assert.All(xs, x => x > 0);
    }

    [Fact]
    public void Any_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(-1);
        xs.Add(2);
        Assert.Any(xs, x => x > 0);
    }

    [Fact]
    public void Single_Predicate_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        Assert.Single(xs, x => x == 2);
    }

    // ── IsType（typeof 身份）/ IsException ──

    [Fact]
    public void IsType_TypeofIdentity_Pass()
    {
        Assert.IsType(typeof(Exception), typeof(Exception));
    }

    [Fact]
    public void IsException_Pass()
    {
        Exception ex = new Exception("x");
        Assert.IsException(ex);
    }

    // ── DisplayName 冒烟（宿主展示名；不改变断言语义）──

    [Fact(DisplayName = "identity.display-name")]
    public void DisplayName_DoesNotAffectSemantics()
    {
        Assert.True(true);
    }
}
