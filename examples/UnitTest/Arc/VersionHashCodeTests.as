namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Version（不可变版本号）与 HashCode（哈希组合）单元测试。
/// 两者均为纯 Arc 实现、无 ABI 依赖，覆盖开发者解析/比较版本号与组合自定义键哈希的真实场景。
/// </summary>
public class VersionHashCodeTests
{
    // ── Version: Parse ──

    [Fact]
    public void Version_Parse_FourComponents()
    {
        Version v = Version.Parse("1.2.3.4");
        Assert.Equal(1, v.Major);
        Assert.Equal(2, v.Minor);
        Assert.Equal(3, v.Build);
        Assert.Equal(4, v.Revision);
    }

    [Fact]
    public void Version_Parse_TwoComponents()
    {
        Version v = Version.Parse("2.5");
        Assert.Equal(2, v.Major);
        Assert.Equal(5, v.Minor);
        Assert.Equal(-1, v.Build);
        Assert.Equal(-1, v.Revision);
    }

    [Fact]
    public void Version_Parse_ThreeComponents()
    {
        Version v = Version.Parse("1.0.7");
        Assert.Equal(1, v.Major);
        Assert.Equal(0, v.Minor);
        Assert.Equal(7, v.Build);
        Assert.Equal(-1, v.Revision);
    }

    [Fact]
    public void Version_Parse_Empty_Throws()
    {
        // 空串非法（对齐 C# System.Version.Parse：抛 FormatException），
        // 宽松解析由 Version.TryParse 提供。
        Assert.Throws("Version.Parse(\"\")", () => { Version.Parse(""); });
    }

    // ── Version: ToString ──

    [Fact]
    public void Version_ToString_ReflectsComponentCount()
    {
        Assert.Equal("1.2.3.4", Version.Parse("1.2.3.4").ToString());
        Assert.Equal("1.2", Version.Parse("1.2").ToString());
        Assert.Equal("1.0.7", Version.Parse("1.0.7").ToString());
    }

    // ── Version: Compare ──

    [Fact]
    public void Version_Compare_OrdersByComponent()
    {
        Version low = Version.Parse("1.2");
        Version high = Version.Parse("1.3");
        Assert.Equal(-1, Version.Compare(low, high));
        Assert.Equal(1, Version.Compare(high, low));
        Assert.Equal(0, Version.Compare(low, Version.Parse("1.2")));
    }

    [Fact]
    public void Version_Compare_TreatsMissingAsZero()
    {
        // 1.2 与 1.2.0：缺失 build 视为 0，因此相等。
        Assert.Equal(0, Version.Compare(Version.Parse("1.2"), Version.Parse("1.2.0")));
        // 1.2.1 > 1.2（1.2 的 build 视为 0）。
        Assert.Equal(1, Version.Compare(Version.Parse("1.2.1"), Version.Parse("1.2")));
    }

    [Fact]
    public void Version_Compare_MajorDominates()
    {
        Assert.Equal(-1, Version.Compare(Version.Parse("2.9"), Version.Parse("10.0")));
        Assert.Equal(1, Version.Compare(Version.Parse("10.0"), Version.Parse("2.9")));
    }

    // ── Version: Equals ──

    [Fact]
    public void Version_Equals_SameFields()
    {
        Assert.True(Version.Equals(Version.Parse("3.1.4"), Version.Parse("3.1.4")));
        Assert.False(Version.Equals(Version.Parse("3.1.4"), Version.Parse("3.1.5")));
    }

    // ── HashCode: HashValue ──

    [Fact]
    public void HashCode_HashValue_Deterministic()
    {
        Assert.Equal(HashCode.HashValue(42), HashCode.HashValue(42));
        Assert.Equal(HashCode.HashValue("arc"), HashCode.HashValue("arc"));
    }

    [Fact]
    public void HashCode_HashValue_EqualStringsEqualHash()
    {
        // 相等字符串必须产生相等哈希（哈希核心契约）。
        Assert.Equal(HashCode.HashValue("hello"), HashCode.HashValue("hello"));
    }

    [Fact]
    public void HashCode_HashValue_DifferentStringsDiffer()
    {
        // 内容哈希须区分不同内容（非仅长度/恒等退化），否则碰撞率过高不可用。
        Assert.True(HashCode.HashValue("a") != HashCode.HashValue("b"));
        Assert.True(HashCode.HashValue("cat") != HashCode.HashValue("dog"));
        // 长度相同但内容不同也必须区分。
        Assert.True(HashCode.HashValue("ab") != HashCode.HashValue("ba"));
    }

    // ── HashCode: Combine ──

    [Fact]
    public void HashCode_Combine_Deterministic()
    {
        Assert.Equal(HashCode.Combine(1, 2), HashCode.Combine(1, 2));
        Assert.Equal(HashCode.Combine(1, 2, 3), HashCode.Combine(1, 2, 3));
        Assert.Equal(HashCode.Combine(1, 2, 3, 4), HashCode.Combine(1, 2, 3, 4));
    }

    [Fact]
    public void HashCode_Combine_OrderSensitive()
    {
        // 组合对输入顺序敏感——(1,2) 与 (2,1) 应产生不同哈希。
        Assert.True(HashCode.Combine(1, 2) != HashCode.Combine(2, 1));
    }

    [Fact]
    public void HashCode_Combine_MultiArgAlignsWithNested()
    {
        int two = HashCode.Combine(1, 2);
        int three = HashCode.Combine(1, 2, 3);
        Assert.Equal(HashCode.Combine(two, 3), three);
        Assert.Equal(HashCode.Combine(HashCode.Combine(HashCode.Combine(1, 2), 3), 4), HashCode.Combine(1, 2, 3, 4));
    }
}
