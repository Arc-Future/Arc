namespace UnitTest.QIF;

using Arc.QIF;

/// <summary>
/// [Theory] + [InlineData] 稳定面（非 Skip）。空体 Theory 不算证据。
/// </summary>
public class TheoryTests
{
    [Theory]
    [InlineData(1, 2, 3)]
    [InlineData(10, 20, 30)]
    public void Add_Int_InlineData(int a, int b, int expected)
    {
        Assert.Equal(expected, a + b);
    }

    [Theory]
    [InlineData("hello", "hello")]
    [InlineData("arc", "arc")]
    public void Equal_String_InlineData(string a, string b)
    {
        Assert.Equal(a, b);
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void Bool_Identity_InlineData(bool value)
    {
        if (value) {
            Assert.True(value);
        } else {
            Assert.False(value);
        }
    }
}
