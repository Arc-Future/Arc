namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// Collection 模块单元测试：Dictionary、foreach 循环、集合 spread、字符串操作。
/// 覆盖 ListTests.as 未覆盖的集合场景（含 `[...]` 集合表达式；原 examples/VarInference 已收敛至此面）。
/// </summary>
public class CollectionTests
{
    // ── Dictionary 构造 ──

    [Fact]
    public void NewDictionary_IsEmpty()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        Assert.Equal(0, dict.Count);
    }

    // ── Dictionary 索引器 set_Item / get_Item ──

    [Fact]
    public void Dictionary_SetAndGet()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        Assert.Equal(1, dict["alpha"]);
    }

    [Fact]
    public void Dictionary_MultipleKeys()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;
        Assert.Equal(2, dict.Count);
        Assert.Equal(1, dict["alpha"]);
        Assert.Equal(2, dict["beta"]);
    }

    [Fact]
    public void Dictionary_OverwriteKey()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["x"] = 10;
        dict["x"] = 99;
        Assert.Equal(99, dict["x"]);
        Assert.Equal(1, dict.Count);
    }

    // ── Dictionary ContainsKey ──

    [Fact]
    public void Dictionary_ContainsKey_Found()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        Assert.True(dict.ContainsKey("alpha"));
    }

    [Fact]
    public void Dictionary_ContainsKey_NotFound()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        Assert.False(dict.ContainsKey("missing"));
    }

    // ── Dictionary Remove ──

    [Fact]
    public void Dictionary_Remove_Existing()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["a"] = 1;
        dict["b"] = 2;
        bool removed = dict.Remove("a");
        Assert.True(removed);
        Assert.Equal(1, dict.Count);
        Assert.False(dict.ContainsKey("a"));
    }

    [Fact]
    public void Dictionary_Remove_NonExisting()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["a"] = 1;
        bool removed = dict.Remove("b");
        Assert.False(removed);
        Assert.Equal(1, dict.Count);
    }

    // ── foreach 枚举（值类型元素 List<int>）──

    [Fact]
    public void Foreach_IntList_Sum()
    {
        List<int> nums = new List<int>();
        nums.Add(10);
        nums.Add(20);
        nums.Add(30);
        nums.Add(40);
        nums.Add(50);
        nums.Add(60);
        nums.Add(70);
        nums.Add(80);
        nums.Add(90);
        nums.Add(100);

        int sum = 0;
        foreach (var n in nums) {
            sum = sum + n;
        }
        Assert.Equal(550, sum);
    }

    // ── foreach 枚举（引用类型元素 List<string>）──

    [Fact]
    public void Foreach_StringList_Elements()
    {
        List<string> names = new List<string>();
        names.Add("alpha");
        names.Add("beta");
        names.Add("gamma");

        Assert.Equal(3, names.Count);
        Assert.True(names[0] == "alpha");
        Assert.True(names[2] == "gamma");

        int count = 0;
        foreach (var n in names) {
            count = count + 1;
        }
        Assert.Equal(3, count);
    }

    // ── 集合表达式 spread ──

    [Fact]
    public void Spread_MergeTwoArrays()
    {
        int[] a = [1, 2];
        int[] b = [3, 4];
        int[] merged = [..a, ..b];
        Assert.Equal(4, merged.Length);
        Assert.Equal(1, merged[0]);
        Assert.Equal(2, merged[1]);
        Assert.Equal(3, merged[2]);
        Assert.Equal(4, merged[3]);
    }

    [Fact]
    public void Spread_MixedLiteralAndSpread()
    {
        int[] a = [1, 2];
        int[] b = [3, 4];
        int[] mixed = [..a, 99, ..b];
        Assert.Equal(5, mixed.Length);
        Assert.Equal(99, mixed[2]);
    }

    // ── string 操作 ──

    [Fact]
    public void String_Length()
    {
        string s = "Hello, collections!";
        Assert.Equal(19, s.Length);
    }

    [Fact]
    public void String_Compare_Same()
    {
        int cmp = string.Compare("hello", "hello");
        Assert.Equal(0, cmp);
    }

    [Fact]
    public void String_Concatenation()
    {
        string greeting = "Hello, " + "collections!";
        Assert.Equal(19, greeting.Length);
    }
}
