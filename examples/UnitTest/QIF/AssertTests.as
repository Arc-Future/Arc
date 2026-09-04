namespace UnitTest.QIF;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// QIF Assert 稳定面自测（通过路径 + Assert.Skip 机制）。
/// 故意失败路径不放在默认套件（会弄红）；失败处理由专用集成测试覆盖。
/// 禁止用 [Fact(Skip)] 掩盖未实现缺口——本文件无 Fact(Skip)。
/// </summary>
[Trait("category", "unit")]
public class AssertTests
{
    // ── Equal / NotEqual ──

    [Fact]
    public void Equal_Int_Pass()
    {
        Assert.Equal(1, 1);
    }

    [Fact]
    public void Equal_Int_Negative_Pass()
    {
        Assert.Equal(-42, -42);
    }

    [Fact]
    public void Equal_LargeInt_Pass()
    {
        Assert.Equal(2147483647, 2147483647);
    }

    [Fact]
    public void Equal_String_Pass()
    {
        Assert.Equal("arc", "arc");
    }

    [Fact]
    public void Equal_String_Empty_Pass()
    {
        Assert.Equal("", "");
    }

    [Fact]
    public void Equal_Long_Pass()
    {
        long a = 9;
        long b = 9;
        Assert.Equal(a, b);
    }

    [Fact]
    public void Equal_LargeLong_Pass()
    {
        long a = 123456789012345;
        long b = 123456789012345;
        Assert.Equal(a, b);
    }

    [Fact]
    public void Equal_Double_Delta_Pass()
    {
        Assert.Equal(1.0, 1.0004, 0.001);
    }

    [Fact]
    public void Equal_Double_Exactly_Pass()
    {
        Assert.Equal(3.14, 3.14, 0.0);
    }

    [Fact]
    public void NotEqual_Int_Pass()
    {
        Assert.NotEqual(1, 2);
    }

    [Fact]
    public void NotEqual_String_Pass()
    {
        Assert.NotEqual("a", "b");
    }

    [Fact]
    public void NotEqual_Long_Pass()
    {
        long a = 1;
        long b = 2;
        Assert.NotEqual(a, b);
    }

    // ── True / False ──

    [Fact]
    public void True_Pass()
    {
        Assert.True(true);
    }

    [Fact]
    public void False_Pass()
    {
        Assert.False(false);
    }

    [Fact]
    public void True_WithMessage()
    {
        Assert.True(1 == 1, "One equals one");
    }

    [Fact]
    public void False_WithMessage()
    {
        Assert.False(1 == 2, "One does not equal two");
    }

    // ── Null / NotNull ──

    [Fact]
    public void Null_Pass()
    {
        string s = null;
        Assert.Null(s);
    }

    [Fact]
    public void Null_IntrinsicNull_Pass()
    {
        object o = null;
        Assert.Null(o);
    }

    [Fact]
    public void NotNull_Pass()
    {
        Assert.NotNull("x");
    }

    [Fact]
    public void NotNull_WithMessage_Pass()
    {
        Assert.NotNull(42, "Should not be null");
    }

    // ── Greater / Less / InRange ──

    [Fact]
    public void Greater_Pass()
    {
        Assert.Greater(5, 3);
    }

    [Fact]
    public void Greater_Negative_Pass()
    {
        Assert.Greater(-3, -5);
    }

    [Fact]
    public void GreaterOrEqual_Pass()
    {
        Assert.GreaterOrEqual(5, 5);
    }

    [Fact]
    public void GreaterOrEqual_StrictGreater_Pass()
    {
        Assert.GreaterOrEqual(10, 5);
    }

    [Fact]
    public void Less_Pass()
    {
        Assert.Less(3, 5);
    }

    [Fact]
    public void Less_Negative_Pass()
    {
        Assert.Less(-5, -3);
    }

    [Fact]
    public void LessOrEqual_Pass()
    {
        Assert.LessOrEqual(3, 3);
    }

    [Fact]
    public void LessOrEqual_StrictLess_Pass()
    {
        Assert.LessOrEqual(5, 10);
    }

    [Fact]
    public void InRange_Pass()
    {
        Assert.InRange(5, 1, 10);
    }

    [Fact]
    public void InRange_LowerBound_Pass()
    {
        Assert.InRange(1, 1, 10);
    }

    [Fact]
    public void InRange_UpperBound_Pass()
    {
        Assert.InRange(10, 1, 10);
    }

    [Fact]
    public void NotInRange_Pass()
    {
        Assert.NotInRange(0, 1, 10);
    }

    [Fact]
    public void NotInRange_Below_Pass()
    {
        Assert.NotInRange(-5, 1, 10);
    }

    [Fact]
    public void NotInRange_Above_Pass()
    {
        Assert.NotInRange(15, 1, 10);
    }

    // ── Contains / StartsWith / EndsWith（字符串路径）──

    [Fact]
    public void Contains_Substring_Pass()
    {
        Assert.Contains("arc", "dlang-arc");
    }

    [Fact]
    public void Contains_EmptySubstring_Pass()
    {
        Assert.Contains("", "anything");
    }

    [Fact]
    public void StartsWith_Pass()
    {
        Assert.StartsWith("QIF_", "QIF_SKIP: reason");
    }

    [Fact]
    public void StartsWith_ExactMatch_Pass()
    {
        Assert.StartsWith("hello", "hello");
    }

    [Fact]
    public void EndsWith_Pass()
    {
        Assert.EndsWith(".as", "MathTests.as");
    }

    [Fact]
    public void EndsWith_ExactMatch_Pass()
    {
        Assert.EndsWith("world", "hello world");
    }

    [Fact]
    public void DoesNotContain_Substring_Pass()
    {
        Assert.DoesNotContain("orm", "dlang-arc-qif");
    }

    [Fact]
    public void DoesNotContain_NotPresent_Pass()
    {
        Assert.DoesNotContain("xyz", "abcdef");
    }

    // ── Empty / NotEmpty / Single（List<T> 静态泛型）──

    [Fact]
    public void Empty_Pass()
    {
        List<int> xs = new List<int>();
        Assert.Empty(xs);
    }

    [Fact]
    public void Empty_StringList_Pass()
    {
        List<string> xs = new List<string>();
        Assert.Empty(xs);
    }

    [Fact]
    public void NotEmpty_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        Assert.NotEmpty(xs);
    }

    [Fact]
    public void NotEmpty_Multiple_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        Assert.NotEmpty(xs);
    }

    [Fact]
    public void Single_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(42);
        Assert.Single(xs);
    }

    [Fact]
    public void Single_StringList_Pass()
    {
        List<string> xs = new List<string>();
        xs.Add("only");
        Assert.Single(xs);
    }

    // ── Single(predicate)：Arc MIR lowering 暂不支持 泛型 + Func<T,bool> 组合 ──
    // TODO: 待 Arc 泛型委托支持完善后恢复

    // ── List 元素路径（Contains / DoesNotContain）──

    [Fact]
    public void Contains_List_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        xs.Add(3);
        Assert.Contains(2, xs);
    }

    [Fact]
    public void Contains_List_FirstElement_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(10);
        xs.Add(20);
        Assert.Contains(10, xs);
    }

    [Fact]
    public void Contains_List_LastElement_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(10);
        xs.Add(20);
        Assert.Contains(20, xs);
    }

    [Fact]
    public void DoesNotContain_List_Pass()
    {
        List<int> xs = new List<int>();
        xs.Add(1);
        xs.Add(2);
        Assert.DoesNotContain(99, xs);
    }

    [Fact]
    public void SequenceEqual_Pass()
    {
        List<int> a = new List<int>();
        a.Add(1);
        a.Add(2);
        List<int> b = new List<int>();
        b.Add(1);
        b.Add(2);
        Assert.SequenceEqual(a, b);
    }

    [Fact]
    public void SequenceEqual_EmptyLists_Pass()
    {
        List<int> a = new List<int>();
        List<int> b = new List<int>();
        Assert.SequenceEqual(a, b);
    }

    [Fact]
    public void SequenceEqual_SingleElement_Pass()
    {
        List<string> a = new List<string>();
        a.Add("hello");
        List<string> b = new List<string>();
        b.Add("hello");
        Assert.SequenceEqual(a, b);
    }

    // ── All / Any：Arc MIR lowering 暂不支持 泛型 + Func<T,bool> 组合 ──
    // TODO: 待 Arc 泛型委托支持完善后恢复

    // ── Assert.Fail ──

    [Fact]
    public void Fail_ThrowsException()
    {
        bool caught = false;
        try {
            Assert.Fail("deliberate failure");
        } catch (Exception ex) {
            caught = true;
            Assert.True(ex.Message.Contains("deliberate failure"));
        }
        Assert.True(caught);
    }

    // ── Assert.Skip：抛 QIF_SKIP: 前缀，host 记为 Skipped（非 Fail）──

    [Fact]
    public void Skip_RecordsAsSkipped()
    {
        Assert.Skip("Always skipped");
    }

    [Fact]
    public void Skip_WithReason_RecordsAsSkipped()
    {
        Assert.Skip("Feature not yet implemented");
    }
}
