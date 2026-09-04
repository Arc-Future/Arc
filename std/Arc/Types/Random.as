// RFC 027 M4: Random -- pure Arc pseudo-random number generator.
//
// Aligns with C# System.Random surface (Next / NextBytes / NextDouble).
// Algorithm: 32-bit Numerical Recipes LCG (a=1664525, c=1013904223).
// Not crypto-secure (use Arc.Security CSPRNG).
//
// Honesty: Arc lexer/parser has no bitwise AND `&` / shift / xor for PRNG
// kernels. xoshiro256** therefore cannot be expressed; LCG + NextBytes use
// only +/*/% arithmetic (`Next() % 256` per byte). Shared / CSPRNG 后置.

namespace Arc.Types;

public class Random {
    private long _state;

    public Random() {
        long seed = rt_resources.rt_os_now_ticks();
        if (seed == 0) { seed = 1; }
        _state = seed;
    }

    public Random(int seed) {
        long s = (long)seed;
        if (s == 0) { s = 1; }
        _state = s;
    }

    public int Next() {
        long r = this.NextRaw();
        if (r < 0) { r = 0 - r; }
        return (int)(r % 2147483647);
    }

    public int Next(int maxValue) {
        if (maxValue <= 0) { return 0; }
        return this.Next() % maxValue;
    }

    public int Next(int minValue, int maxValue) {
        if (minValue >= maxValue) { return minValue; }
        int range = maxValue - minValue;
        return minValue + (this.Next() % range);
    }

    public double NextDouble() {
        return (double)this.Next() / 2147483647.0;
    }

    public long NextInt64() {
        return this.NextRaw();
    }

    /// <summary>
    /// 用伪随机字节填充 <paramref name="buffer"/>（每字节一次 <c>Next() % 256</c>；
    /// 无 bitwise；同 seed 可复现）。null → <see cref="ArgumentNullException"/>。
    /// </summary>
    public void NextBytes(byte[] buffer) {
        if (buffer == null) {
            throw new ArgumentNullException("buffer");
        }
        int i = 0;
        while (i < buffer.Length) {
            buffer[i] = (byte)(this.Next() % 256);
            i = i + 1;
        }
    }

    private long NextRaw() {
        _state = _state * 1664525 + 1013904223;
        return _state;
    }
}
