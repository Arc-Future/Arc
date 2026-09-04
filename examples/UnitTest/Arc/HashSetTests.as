namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// HashSet&lt;T&gt; 单元测试：CRUD + 集合运算 / 判定（非 Skip）。
/// </summary>
public class HashSetTests
{
    [Fact]
    public void NewHashSet_IsEmpty()
    {
        HashSet<int> s = new HashSet<int>();
        Assert.Equal(0, s.Count);
    }

    [Fact]
    public void Add_UniqueAndDuplicate()
    {
        HashSet<int> s = new HashSet<int>();
        Assert.True(s.Add(1));
        Assert.True(s.Add(2));
        Assert.False(s.Add(1));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains(1));
        Assert.False(s.Contains(99));
    }

    [Fact]
    public void Add_String_UniqueAndDuplicate()
    {
        HashSet<string> s = new HashSet<string>();
        Assert.True(s.Add("a"));
        Assert.True(s.Add("b"));
        Assert.False(s.Add("a"));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains("a"));
        Assert.True(s.Contains("b"));
        Assert.False(s.Contains("x"));
        Assert.True(s.Remove("a"));
        Assert.False(s.Contains("a"));
        Assert.Equal(1, s.Count);
    }

    [Fact]
    public void Remove_And_Clear()
    {
        HashSet<int> s = new HashSet<int>();
        s.Add(10);
        s.Add(20);
        Assert.True(s.Remove(10));
        Assert.False(s.Contains(10));
        Assert.Equal(1, s.Count);
        Assert.False(s.Remove(99));
        s.Clear();
        Assert.Equal(0, s.Count);
    }

    [Fact]
    public void UnionWith_Merges()
    {
        HashSet<int> a = new HashSet<int>();
        a.Add(1);
        a.Add(2);
        HashSet<int> b = new HashSet<int>();
        b.Add(2);
        b.Add(3);
        a.UnionWith(b);
        Assert.Equal(3, a.Count);
        Assert.True(a.Contains(1));
        Assert.True(a.Contains(2));
        Assert.True(a.Contains(3));
    }

    [Fact]
    public void IntersectWith_KeepsOverlap()
    {
        HashSet<int> a = new HashSet<int>();
        a.Add(1);
        a.Add(2);
        a.Add(3);
        HashSet<int> b = new HashSet<int>();
        b.Add(2);
        b.Add(4);
        a.IntersectWith(b);
        Assert.Equal(1, a.Count);
        Assert.True(a.Contains(2));
        Assert.False(a.Contains(1));
    }

    [Fact]
    public void ExceptWith_RemovesPresent()
    {
        HashSet<int> a = new HashSet<int>();
        a.Add(1);
        a.Add(2);
        a.Add(3);
        HashSet<int> b = new HashSet<int>();
        b.Add(2);
        a.ExceptWith(b);
        Assert.Equal(2, a.Count);
        Assert.False(a.Contains(2));
        Assert.True(a.Contains(1));
        Assert.True(a.Contains(3));
    }

    [Fact]
    public void SetEquals_And_Overlaps()
    {
        HashSet<int> a = new HashSet<int>();
        a.Add(1);
        a.Add(2);
        HashSet<int> b = new HashSet<int>();
        b.Add(2);
        b.Add(1);
        Assert.True(a.SetEquals(b));
        HashSet<int> c = new HashSet<int>();
        c.Add(2);
        c.Add(9);
        Assert.True(a.Overlaps(c));
        HashSet<int> d = new HashSet<int>();
        d.Add(8);
        Assert.False(a.Overlaps(d));
    }

    [Fact]
    public void IsSubsetOf_And_IsSupersetOf()
    {
        HashSet<int> a = new HashSet<int>();
        a.Add(1);
        a.Add(2);
        HashSet<int> b = new HashSet<int>();
        b.Add(1);
        b.Add(2);
        b.Add(3);
        Assert.True(a.IsSubsetOf(b));
        Assert.True(b.IsSupersetOf(a));
        Assert.True(a.IsProperSubsetOf(b));
        Assert.True(b.IsProperSupersetOf(a));
        Assert.False(a.IsProperSubsetOf(a));
    }

    // ── 跨类型路径锁定（codegen ABI：标量统一装箱，不只 int）──

    [Fact]
    public void Add_Long_UniqueAndDuplicate()
    {
        HashSet<long> s = new HashSet<long>();
        Assert.True(s.Add((long)10));
        Assert.False(s.Add((long)10));
        Assert.Equal(1, s.Count);
        Assert.True(s.Contains((long)10));
        Assert.False(s.Contains((long)11));
    }

    [Fact]
    public void Add_Short_UniqueAndDuplicate()
    {
        HashSet<short> s = new HashSet<short>();
        Assert.True(s.Add((short)7));
        Assert.False(s.Add((short)7));
        Assert.Equal(1, s.Count);
        Assert.True(s.Contains((short)7));
    }

    [Fact]
    public void Add_Byte_UniqueAndDuplicate()
    {
        HashSet<byte> s = new HashSet<byte>();
        Assert.True(s.Add((byte)3));
        Assert.False(s.Add((byte)3));
        Assert.Equal(1, s.Count);
        Assert.True(s.Contains((byte)3));
    }

    [Fact]
    public void Add_Bool_UniqueAndDuplicate()
    {
        HashSet<bool> s = new HashSet<bool>();
        Assert.True(s.Add(true));
        Assert.False(s.Add(true));
        Assert.True(s.Add(false));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains(true));
        Assert.True(s.Contains(false));
    }

    [Fact]
    public void Add_Char_UniqueAndDuplicate()
    {
        HashSet<char> s = new HashSet<char>();
        Assert.True(s.Add('a'));
        Assert.False(s.Add('a'));
        Assert.True(s.Add('b'));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains('a'));
        Assert.False(s.Contains('z'));
    }

    [Fact]
    public void Add_Double_UniqueAndDuplicate()
    {
        HashSet<double> s = new HashSet<double>();
        Assert.True(s.Add(1.5));
        Assert.False(s.Add(1.5));
        Assert.True(s.Add(2.5));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains(1.5));
        Assert.False(s.Contains(9.9));
    }

    [Fact]
    public void Add_Float_UniqueAndDuplicate()
    {
        HashSet<float> s = new HashSet<float>();
        Assert.True(s.Add((float)1.5));
        Assert.False(s.Add((float)1.5));
        Assert.Equal(1, s.Count);
        Assert.True(s.Contains((float)1.5));
    }

    [Fact]
    public void Grow_500_Elements_Survive()
    {
        HashSet<int> s = new HashSet<int>();
        int n = 0;
        while (n < 500) {
            s.Add(n * 3);
            n = n + 1;
        }
        Assert.Equal(500, s.Count);
        Assert.True(s.Contains(3 * 250));
        Assert.False(s.Contains(3 * 500));
    }

    [Fact]
    public void Churn_100_AddRemove_Cycle()
    {
        HashSet<int> s = new HashSet<int>();
        for (int i = 0; i < 100; i = i + 1) {
            Assert.True(s.Add(i));
        }
        Assert.Equal(100, s.Count);
        int removed = 0;
        for (int i = 0; i < 100; i = i + 1) {
            if (s.Remove(i)) {
                removed = removed + 1;
            }
        }
        Assert.Equal(100, removed);
        Assert.Equal(0, s.Count);
    }

    [Fact]
    public void Add_Long_HighBitsDistinct()
    {
        HashSet<long> s = new HashSet<long>();
        long k1 = 4294967297;   // 0x1_0000_0001
        long k2 = 8589934593;   // 0x2_0000_0001
        Assert.True(s.Add(k1));
        Assert.True(s.Add(k2));
        Assert.Equal(2, s.Count);
        Assert.True(s.Contains(k1));
        Assert.True(s.Contains(k2));
        Assert.False(s.Contains((long)1));
        Assert.True(s.Remove(k1));
        Assert.False(s.Contains(k1));
        Assert.Equal(1, s.Count);
        Assert.True(s.Contains(k2));
    }
}
