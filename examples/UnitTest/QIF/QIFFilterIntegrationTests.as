namespace UnitTest.QIF;

using Arc;
using Arc.QIF;

/// <summary>
/// QIF 过滤表达式集成测试。
/// 通过 Trait/ClassName/Kind 标记验证 --filter、--namespace、--kind CLI 选项。
/// </summary>
[Trait("category", "integration")]
[Trait("filter", "trait")]
public class QIFFilterIntegrationTests
{
    [Fact]
    [Trait("feature", "filter")]
    public void Filter_Trait_CategoryIntegration_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    [Trait("feature", "filter")]
    [Trait("priority", "high")]
    public void Filter_MultipleTraits_BothMatch_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    public void Filter_ClassName_Contains_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    public void Filter_Kind_Fact_Pass()
    {
        Assert.Equal(1, 1);
    }

    [Theory]
    [InlineData(1, 1, 2)]
    public void Filter_Kind_Theory_Pass(int a, int b, int sum)
    {
        Assert.Equal(sum, a + b);
    }

    [Fact]
    [Trait("feature", "filter")]
    [Trait("priority", "high")]
    public void Filter_And_TraitAndClass_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    [Trait("feature", "filter")]
    public void Filter_Or_TraitOrName_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    [Trait("exclude", "yes")]
    public void Filter_Not_ExcludeTrait_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    [Trait("category", "integration")]
    [Trait("feature", "complex")]
    public void Filter_Complex_ParensAndOr_Pass()
    {
        Assert.True(true);
    }
}

[Trait("category", "integration")]
[Trait("filter", "classname")]
public class QIFFilterOtherTests
{
    [Fact]
    public void Filter_OtherClass_Basic_Pass()
    {
        Assert.Equal(42, 42);
    }

    [Fact]
    [Trait("feature", "filter")]
    public void Filter_OtherClass_WithTrait_Pass()
    {
        Assert.NotEqual(1, 2);
    }
}

[Trait("category", "integration")]
[Trait("filter", "kind")]
public class QIFTheoryFilterTests
{
    [Theory]
    [InlineData(1, 2, 3)]
    [InlineData(10, 20, 30)]
    public void Theory_Int_Add_Pass(int a, int b, int expected)
    {
        Assert.Equal(expected, a + b);
    }

    [Theory]
    [InlineData("hello", "hello")]
    [InlineData("arc", "arc")]
    public void Theory_String_Equal_Pass(string a, string b)
    {
        Assert.Equal(a, b);
    }

    [Fact]
    public void Fact_SameClass_Pass()
    {
        Assert.True(true);
    }
}

[Trait("category", "unit")]
public class QIFNamespaceTests
{
    [Fact]
    public void Namespace_QIF_Basic_Pass()
    {
        Assert.Equal("QIF", "QIF");
    }

    [Fact]
    public void Namespace_QIF_Another_Pass()
    {
        Assert.NotEqual("A", "B");
    }
}
