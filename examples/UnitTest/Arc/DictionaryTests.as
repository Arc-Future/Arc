namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// Dictionary 全面单元测试：覆盖 Dictionary 示例的所有键值类型配对。
/// 验证 string/int/long/float/double/bool 作为键，
/// int/string/long 作为值的 Dictionary 操作。
/// </summary>
public class DictionaryTests
{
    // ── Dictionary<string, int> ──

    [Fact]
    public void Dict_StringInt_Basic()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;

        Assert.True(dict.ContainsKey("alpha"));
        Assert.Equal(1, dict["alpha"]);
        Assert.True(dict.ContainsKey("beta"));
        Assert.Equal(2, dict["beta"]);
    }

    [Fact]
    public void Dict_StringInt_TryGetValue()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;

        int val;
        Assert.True(dict.TryGetValue("alpha", out val));
        Assert.Equal(1, val);

        Assert.True(dict.TryGetValue("beta", out val));
        Assert.Equal(2, val);

        Assert.False(dict.TryGetValue("gamma", out val));
        Assert.Equal(0, val);
    }

    [Fact]
    public void Dict_StringInt_Remove()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;

        Assert.True(dict.Remove("alpha"));
        Assert.False(dict.ContainsKey("alpha"));
        Assert.True(dict.ContainsKey("beta"));
        Assert.False(dict.Remove("gamma"));
    }

    [Fact]
    public void Dict_StringInt_Clear()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;
        dict.Clear();

        Assert.Equal(0, dict.Count);
        Assert.False(dict.ContainsKey("beta"));
    }

    // ── Dictionary<string, string> ──

    [Fact]
    public void Dict_StringString_Basic()
    {
        Dictionary<string, string> dict = new Dictionary<string, string>();
        dict["name"] = "Alice";

        Assert.True(dict.ContainsKey("name"));
        Assert.True(dict["name"] == "Alice");
    }

    [Fact]
    public void Dict_StringString_TryGetValue()
    {
        Dictionary<string, string> dict = new Dictionary<string, string>();
        dict["name"] = "Alice";

        string val = "";
        Assert.True(dict.TryGetValue("name", out val));
        Assert.True(val == "Alice");

        Assert.False(dict.TryGetValue("missing", out val));
    }

    [Fact]
    public void Dict_StringString_Remove()
    {
        Dictionary<string, string> dict = new Dictionary<string, string>();
        dict["name"] = "Alice";

        Assert.True(dict.Remove("name"));
        Assert.False(dict.ContainsKey("name"));
        Assert.False(dict.Remove("missing"));
    }

    // ── Dictionary<int, string> — int key ──

    [Fact]
    public void Dict_IntString_Basic()
    {
        Dictionary<int, string> dict = new Dictionary<int, string>();
        dict[1] = "one";
        dict[2] = "two";

        Assert.True(dict.ContainsKey(1));
        Assert.True(dict[1] == "one");
        Assert.True(dict[2] == "two");
    }

    [Fact]
    public void Dict_IntString_TryGetValue()
    {
        Dictionary<int, string> dict = new Dictionary<int, string>();
        dict[1] = "one";
        dict[2] = "two";

        string val = "";
        Assert.True(dict.TryGetValue(1, out val));
        Assert.True(val == "one");

        Assert.True(dict.TryGetValue(2, out val));
        Assert.True(val == "two");

        Assert.False(dict.TryGetValue(3, out val));
    }

    [Fact]
    public void Dict_IntString_Remove()
    {
        Dictionary<int, string> dict = new Dictionary<int, string>();
        dict[1] = "one";
        dict[2] = "two";

        Assert.True(dict.Remove(1));
        Assert.False(dict.ContainsKey(1));
        Assert.True(dict.ContainsKey(2));
        Assert.False(dict.Remove(3));
    }

    // ── Dictionary<string, long> — long value ──

    [Fact]
    public void Dict_StringLong_Basic()
    {
        Dictionary<string, long> dict = new Dictionary<string, long>();
        dict["big"] = 5000000000;

        Assert.True(dict.ContainsKey("big"));
        Assert.True(dict["big"] == 5000000000);
    }

    [Fact]
    public void Dict_StringLong_TryGetValue()
    {
        Dictionary<string, long> dict = new Dictionary<string, long>();
        dict["big"] = 5000000000;

        long val;
        Assert.True(dict.TryGetValue("big", out val));
        Assert.True(val == 5000000000);

        Assert.False(dict.TryGetValue("missing", out val));
        Assert.True(val == 0);
    }

    [Fact]
    public void Dict_StringLong_Remove()
    {
        Dictionary<string, long> dict = new Dictionary<string, long>();
        dict["big"] = 5000000000;

        Assert.True(dict.Remove("big"));
        Assert.False(dict.ContainsKey("big"));
        Assert.False(dict.Remove("missing"));
    }

    // ── Dictionary<float, int> — float key ──

    [Fact]
    public void Dict_FloatInt_Basic()
    {
        Dictionary<float, int> dict = new Dictionary<float, int>();
        float fkey1 = 1.5;
        float fkey2 = 2.25;
        dict[fkey1] = 10;
        dict[fkey2] = 20;

        Assert.True(dict.ContainsKey(fkey1));
        Assert.Equal(10, dict[fkey1]);
        Assert.True(dict.ContainsKey(fkey2));
        Assert.Equal(20, dict[fkey2]);
    }

    [Fact]
    public void Dict_FloatInt_TryGetValue()
    {
        Dictionary<float, int> dict = new Dictionary<float, int>();
        float fkey1 = 1.5;
        float fkey2 = 2.25;
        dict[fkey1] = 10;
        dict[fkey2] = 20;

        int val;
        Assert.True(dict.TryGetValue(fkey1, out val));
        Assert.Equal(10, val);

        Assert.True(dict.TryGetValue(fkey2, out val));
        Assert.Equal(20, val);

        float fkey3 = 3.0;
        Assert.False(dict.TryGetValue(fkey3, out val));
        Assert.Equal(0, val);
    }

    [Fact]
    public void Dict_FloatInt_Remove()
    {
        Dictionary<float, int> dict = new Dictionary<float, int>();
        float fkey1 = 1.5;
        float fkey2 = 2.25;
        float fkey3 = 3.0;
        dict[fkey1] = 10;
        dict[fkey2] = 20;

        Assert.True(dict.Remove(fkey1));
        Assert.False(dict.ContainsKey(fkey1));
        Assert.True(dict.ContainsKey(fkey2));
        Assert.False(dict.Remove(fkey3));
    }

    // ── Dictionary<double, int> — double key ──

    [Fact]
    public void Dict_DoubleInt_Basic()
    {
        Dictionary<double, int> dict = new Dictionary<double, int>();
        dict[3.14159] = 100;
        dict[2.71828] = 200;

        Assert.True(dict.ContainsKey(3.14159));
        Assert.Equal(100, dict[3.14159]);
        Assert.True(dict.ContainsKey(2.71828));
        Assert.Equal(200, dict[2.71828]);
    }

    [Fact]
    public void Dict_DoubleInt_TryGetValue()
    {
        Dictionary<double, int> dict = new Dictionary<double, int>();
        dict[3.14159] = 100;
        dict[2.71828] = 200;

        int val;
        Assert.True(dict.TryGetValue(3.14159, out val));
        Assert.Equal(100, val);

        Assert.True(dict.TryGetValue(2.71828, out val));
        Assert.Equal(200, val);

        Assert.False(dict.TryGetValue(1.0, out val));
        Assert.Equal(0, val);
    }

    [Fact]
    public void Dict_DoubleInt_Remove()
    {
        Dictionary<double, int> dict = new Dictionary<double, int>();
        dict[3.14159] = 100;
        dict[2.71828] = 200;

        Assert.True(dict.Remove(3.14159));
        Assert.False(dict.ContainsKey(3.14159));
        Assert.True(dict.ContainsKey(2.71828));
        Assert.False(dict.Remove(1.0));
    }

    // ── Dictionary<bool, int> — bool key ──

    [Fact]
    public void Dict_BoolInt_Basic()
    {
        Dictionary<bool, int> dict = new Dictionary<bool, int>();
        dict[true] = 1;
        dict[false] = 0;

        Assert.True(dict.ContainsKey(true));
        Assert.True(dict.ContainsKey(false));
        Assert.Equal(1, dict[true]);
        Assert.Equal(0, dict[false]);
    }

    [Fact]
    public void Dict_BoolInt_TryGetValue()
    {
        Dictionary<bool, int> dict = new Dictionary<bool, int>();
        dict[true] = 1;
        dict[false] = 0;

        int val;
        Assert.True(dict.TryGetValue(true, out val));
        Assert.Equal(1, val);

        Assert.True(dict.TryGetValue(false, out val));
        Assert.Equal(0, val);
    }

    [Fact]
    public void Dict_BoolInt_Remove()
    {
        Dictionary<bool, int> dict = new Dictionary<bool, int>();
        dict[true] = 1;
        dict[false] = 0;

        Assert.True(dict.Remove(true));
        Assert.False(dict.ContainsKey(true));
        Assert.True(dict.ContainsKey(false));
    }

    [Fact]
    public void Dict_StringInt_ContainsValue()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        dict["alpha"] = 1;
        dict["beta"] = 2;
        Assert.True(dict.ContainsValue(1));
        Assert.True(dict.ContainsValue(2));
        Assert.False(dict.ContainsValue(99));
    }

    [Fact]
    public void Dict_StringInt_Add_And_Keys()
    {
        Dictionary<string, int> dict = new Dictionary<string, int>();
        Assert.True(dict.Add("alpha", 1));
        Assert.False(dict.Add("alpha", 99));
        Assert.Equal(1, dict["alpha"]);
        Assert.True(dict.Add("beta", 2));
        string[] keys = dict.Keys;
        Assert.Equal(2, keys.Length);
        Assert.Equal(2, dict.Count);
    }

    // ── Dictionary<long, V> — long 键高位不同（P0 修复：禁止低 32 位截断误判相等）──

    [Fact]
    public void Dict_LongKey_HighBitsDistinct()
    {
        Dictionary<long, int> dict = new Dictionary<long, int>();
        long k1 = 4294967297;   // 0x1_0000_0001
        long k2 = 8589934593;   // 0x2_0000_0001
        dict[k1] = 10;
        dict[k2] = 20;
        Assert.Equal(2, dict.Count);
        Assert.True(dict.ContainsKey(k1));
        Assert.True(dict.ContainsKey(k2));
        Assert.Equal(10, dict[k1]);
        Assert.Equal(20, dict[k2]);
        Assert.True(dict.Remove(k1));
        Assert.False(dict.ContainsKey(k1));
        Assert.True(dict.ContainsKey(k2));
        Assert.Equal(1, dict.Count);
    }
}
