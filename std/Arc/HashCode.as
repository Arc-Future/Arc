namespace Arc;

/// 哈希组合辅助——对标 .NET `System.HashCode` / `HashCode.Combine`。
///
/// 为自定义键类型提供安全、零装箱的多字段哈希组合，避免手写易错且碰撞率高的
/// `(a * 397) ^ b` 混合。字段为基元时先用 `HashValue` 得到其哈希，再用
/// `Combine` 组合。
///
/// 典型用法（自定义键类型实现 `IEquatable&lt;T&gt;, IHashable&lt;T&gt;`）：
/// <code>
/// public static int GetHashCode(Point value) {
///     return HashCode.Combine(
///         HashCode.HashValue(value.X),
///         HashCode.HashValue(value.Y));
/// }
/// </code>
///
/// 混合常数取黄金比例乘法（-1640531527 = 0x9E3779B9），确定性、无状态，
/// 与 .NET `HashCode.Combine` 的思路一致但去随机种子（Arc 组合需跨进程可复现）。
public static class HashCode {
    /// <summary>计算 `int` 的哈希码（基元 static abstract 分派，零装箱）。</summary>
    public static int HashValue(int value) { return int.GetHashCode(value); }

    /// <summary>计算 `long` 的哈希码（基元 static abstract 分派）。</summary>
    public static int HashValue(long value) { return long.GetHashCode(value); }

    /// <summary>计算 `short` 的哈希码（基元 static abstract 分派）。</summary>
    public static int HashValue(short value) { return short.GetHashCode(value); }

    /// <summary>计算 `byte` 的哈希码（基元 static abstract 分派）。</summary>
    public static int HashValue(byte value) { return byte.GetHashCode(value); }

    /// <summary>计算 `float` 的哈希码（基元 static abstract 分派，bit pattern）。</summary>
    public static int HashValue(float value) { return float.GetHashCode(value); }

    /// <summary>计算 `double` 的哈希码（基元 static abstract 分派，bit pattern）。</summary>
    public static int HashValue(double value) { return double.GetHashCode(value); }

    /// <summary>计算 `bool` 的哈希码（基元 static abstract 分派）。</summary>
    public static int HashValue(bool value) { return bool.GetHashCode(value); }

    /// <summary>计算 `char` 的哈希码（基元 static abstract 分派）。</summary>
    public static int HashValue(char value) { return char.GetHashCode(value); }

    /// <summary>计算 `string` 的哈希码（基元 static abstract 分派，rt_string_hash）。</summary>
    public static int HashValue(string value) { return string.GetHashCode(value); }

    /// <summary>组合两个已哈希的 32 位哈希码。</summary>
    public static int Combine(int a, int b) {
        return a * -1640531527 + b;
    }

    /// <summary>组合三个已哈希的 32 位哈希码。</summary>
    public static int Combine(int a, int b, int c) {
        return Combine(Combine(a, b), c);
    }

    /// <summary>组合四个已哈希的 32 位哈希码。</summary>
    public static int Combine(int a, int b, int c, int d) {
        return Combine(Combine(Combine(a, b), c), d);
    }
}