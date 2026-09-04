namespace UnitTest.Core;

using Arc;
using Arc.QIF;

struct B1 {
    public int X;
    public B1(int x) { this.X = x; }
    public int GetX() { return X; }
    public static B1 Make(int x) { return new B1(x); }
    public static B1 Id(B1 v) { return v; }
}

struct B2 {
    public readonly int X;
    public B2(int x) { X = x; }
}

record struct B3(int X, int Y);

struct O1 {
    public int X;
    public O1(int x) { this.X = x; }
    public static O1 operator +(O1 a, O1 b) { return new O1(a.X + b.X); }
    public static bool operator ==(O1 a, O1 b) { return a.X == b.X; }
    public static bool operator !=(O1 a, O1 b) { return !(a == b); }
}

public class ProbeTests {
    [Fact]
    public void P1_DirectCtorRead() { B1 v = new B1(3); Assert.Equal(3, v.X); }

    [Fact]
    public void P2_InstanceMethodRead() { B1 v = new B1(3); Assert.Equal(3, v.GetX()); }

    [Fact]
    public void P3_StaticRetStruct() { B1 v = B1.Make(5); Assert.Equal(5, v.X); }

    [Fact]
    public void P4_ByValArgThenRet() { B1 v = new B1(7); B1 w = B1.Id(v); Assert.Equal(7, w.X); }

    [Fact]
    public void P5_ReadonlyFieldRead() { B2 v = new B2(3); Assert.Equal(3, v.X); }

    [Fact]
    public void P6_RecordCtorRead() { B3 v = new B3(3, 4); Assert.Equal(3, v.X); }

    [Fact]
    public void P7_OperatorRet() { O1 a = new O1(2); O1 b = new O1(4); O1 s = a + b; Assert.Equal(6, s.X); }

    [Fact]
    public void P8_OperatorEq() { O1 a = new O1(2); O1 b = new O1(2); Assert.True(a == b); }
}
