namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// RFC 003：用户运算符重载精华子集（非 Skip）。
/// </summary>
struct OpVec2 {
    public int X;
    public int Y;

    public OpVec2(int x, int y) {
        this.X = x;
        this.Y = y;
    }

    public static OpVec2 operator +(OpVec2 a, OpVec2 b) {
        return new OpVec2(a.X + b.X, a.Y + b.Y);
    }

    public static OpVec2 operator -(OpVec2 a) {
        return new OpVec2(-a.X, -a.Y);
    }

    public static bool operator ==(OpVec2 a, OpVec2 b) {
        return a.X == b.X && a.Y == b.Y;
    }

    public static bool operator !=(OpVec2 a, OpVec2 b) {
        return !(a == b);
    }
}

struct OpVec3 {
    public int X;
    public int Y;
    public int Z;

    public OpVec3(int x, int y, int z) {
        this.X = x;
        this.Y = y;
        this.Z = z;
    }

    public static OpVec3 operator *(OpVec3 a, OpVec3 b) {
        return new OpVec3(a.X * b.X, a.Y * b.Y, a.Z * b.Z);
    }

    public static OpVec3 operator /(OpVec3 a, OpVec3 b) {
        return new OpVec3(a.X / b.X, a.Y / b.Y, a.Z / b.Z);
    }

    public static OpVec3 operator %(OpVec3 a, OpVec3 b) {
        return new OpVec3(a.X % b.X, a.Y % b.Y, a.Z % b.Z);
    }

    public static bool operator ==(OpVec3 a, OpVec3 b) {
        return a.X == b.X && a.Y == b.Y && a.Z == b.Z;
    }

    public static bool operator !=(OpVec3 a, OpVec3 b) {
        return !(a == b);
    }
}

public class OperatorOverloadTests {
    [Fact]
    public void PlusUnaryEqCompound() {
        OpVec2 a = new OpVec2(2, 3);
        OpVec2 b = new OpVec2(4, 5);
        OpVec2 s = a + b;
        Assert.Equal(6, s.X);
        Assert.Equal(8, s.Y);
        OpVec2 n = -a;
        Assert.Equal(-2, n.X);
        Assert.Equal(-3, n.Y);
        Assert.True(a == new OpVec2(2, 3));
        Assert.True(a != b);
        a += b;
        Assert.Equal(6, a.X);
        Assert.Equal(8, a.Y);
    }

    [Fact]
    public void MulDivModOverload() {
        OpVec3 a = new OpVec3(6, 8, 9);
        OpVec3 b = new OpVec3(2, 3, 4);
        OpVec3 m = a * b;
        Assert.Equal(12, m.X);
        Assert.Equal(24, m.Y);
        Assert.Equal(36, m.Z);
        OpVec3 d = a / b;
        Assert.Equal(3, d.X);
        Assert.Equal(2, d.Y);
        Assert.Equal(2, d.Z);
        OpVec3 r = a % b;
        Assert.Equal(0, r.X);
        Assert.Equal(2, r.Y);
        Assert.Equal(1, r.Z);
    }

    [Fact]
    public void MulCompoundAssignDesugars() {
        OpVec3 a = new OpVec3(2, 3, 4);
        OpVec3 b = new OpVec3(3, 4, 5);
        a *= b;
        Assert.Equal(6, a.X);
        Assert.Equal(12, a.Y);
        Assert.Equal(20, a.Z);
        a /= new OpVec3(2, 4, 10);
        Assert.Equal(3, a.X);
        Assert.Equal(3, a.Y);
        Assert.Equal(2, a.Z);
    }

    [Fact]
    public void EqIneqConsistency() {
        OpVec3 a = new OpVec3(1, 2, 3);
        OpVec3 b = new OpVec3(1, 2, 3);
        OpVec3 c = new OpVec3(1, 2, 4);
        Assert.True(a == b);
        Assert.False(a != b);
        Assert.True(a != c);
        Assert.False(a == c);
        Assert.True(a == new OpVec3(1, 2, 3));
        Assert.False(a == new OpVec3(3, 2, 1));
    }
}
