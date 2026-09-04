namespace UnitTest.Core;

using Arc;
using Arc.QIF;

// ── 主构造器（RFC 009 L1）──
// `class C(T x)` 脱糖：按值参数捕获为同名 private 字段（`this.x = x;`），成员内可见。

class PrimaryBox(int size) {
    public int Size => size;
    public int Double() { return size * 2; }
}

class PrimaryGadget(int seed) : PrimaryBox(seed) {
    public int SeedPlus(int d) { return seed + d; }
}

// ── record struct（RFC 006）──

record struct RsPoint(int X, int Y);

// ── readonly struct ──

readonly struct RoVec {
    public readonly int X;
    public readonly int Y;

    public RoVec(int x, int y) {
        X = x;
        Y = y;
    }

    public int Sum() { return X + Y; }
}

// ── required（RFC 006 M3-M4）──

class ReqProfile {
    public required string Name { get; init; }
    public required int Level { get; init; }
}

class ReqCtorInit {
    public required string Title { get; init; }

    public ReqCtorInit(string title) {
        Title = title;
    }
}

// ── per-accessor 可见性 ──

class SealedCounter {
    public int Hits { get; private set; }

    public void Bump() {
        Hits = Hits + 1;
    }
}

class WriteOnceLabel {
    public string Text { get; private set; }

    public WriteOnceLabel(string text) {
        Text = text;
    }

    public void Relabel(string text) {
        Text = text;
    }
}

// ── 表达式体方法 ──

class Greeter {
    public string Hi() => "hi";
    public int Square(int n) => n * n;
    public string Dup(string s) => s + s;
}

// ── 自定义接口 static abstract（RFC 004 M1）──

interface IBlendable<T> {
    static abstract T Mix(T a, T b);
    static abstract T Neutral { get; }
}

class ColorMix : IBlendable<ColorMix> {
    public int Level;

    public ColorMix(int level) {
        Level = level;
    }

    public static ColorMix Mix(ColorMix a, ColorMix b) => new ColorMix((a.Level + b.Level) / 2);
    public static ColorMix Neutral => new ColorMix(0);
}

T Blend<T>(T a, T b) where T : IBlendable<T> {
    return T.Mix(a, b);
}

T BlendNeutral<T>() where T : IBlendable<T> {
    return T.Neutral;
}

int ScaledSum(RoVec v, int k) {
    return v.Sum() * k;
}


public class ModernTypeTests
{
    // ── 主构造器 ──

    [Fact]
    public void PrimaryCtor_CapturedInMethodBody()
    {
        PrimaryBox b = new PrimaryBox(21);
        Assert.Equal(42, b.Double());
    }

    [Fact]
    public void PrimaryCtor_CapturedInPropertyBodied()
    {
        PrimaryBox b = new PrimaryBox(7);
        Assert.Equal(7, b.Size);
    }

    [Fact]
    public void PrimaryCtor_BaseArgs()
    {
        PrimaryGadget g = new PrimaryGadget(10);
        Assert.Equal(10, g.Size);
        Assert.Equal(13, g.SeedPlus(3));
    }

    // ── record struct ──

    [Fact]
    public void RecordStruct_ConstructAndRead()
    {
        RsPoint p = new RsPoint(3, 4);
        Assert.Equal(3, p.X);
        Assert.Equal(4, p.Y);
    }

    [Fact]
    public void RecordStruct_InstanceEquals()
    {
        RsPoint a = new RsPoint(1, 2);
        RsPoint b = new RsPoint(1, 2);
        RsPoint c = new RsPoint(9, 9);
        Assert.True(a.Equals(b));
        Assert.False(a.Equals(c));
    }

    [Fact]
    public void RecordStruct_StaticEquals()
    {
        RsPoint a = new RsPoint(5, 6);
        RsPoint b = new RsPoint(5, 6);
        Assert.True(RsPoint.Equals(a, b));
    }

    [Fact]
    public void RecordStruct_Deconstruct()
    {
        RsPoint p = new RsPoint(11, 22);
        var (x, y) = p;
        Assert.Equal(11, x);
        Assert.Equal(22, y);
    }

    [Fact]
    public void RecordStruct_With()
    {
        RsPoint p = new RsPoint(1, 2);
        RsPoint r = p with { Y = 10 };
        Assert.Equal(1, r.X);
        Assert.Equal(10, r.Y);
        Assert.Equal(2, p.Y);
    }

    [Fact]
    public void RecordStruct_ValueSemantics()
    {
        // 语义边界：struct 赋值为移动语义（借用检查器强制 use-after-move 报错），
        // 故此处验证「with 非破坏性」而非「赋值复制」。
        RsPoint a = new RsPoint(1, 2);
        RsPoint b = a;
        RsPoint c = b with { X = 99 };
        Assert.Equal(1, b.X);
        Assert.Equal(2, b.Y);
        Assert.Equal(99, c.X);
    }


    // ── readonly struct ──

    [Fact]
    public void ReadOnlyStruct_InstanceMembers()
    {
        RoVec v = new RoVec(3, 4);
        Assert.Equal(3, v.X);
        Assert.Equal(4, v.Y);
        Assert.Equal(7, v.Sum());
    }

    [Fact]
    public void ReadOnlyStruct_ValuePassing()
    {
        RoVec v = new RoVec(5, 6);
        Assert.Equal(22, ScaledSum(v, 2));
    }

    // ── required ──

    [Fact]
    public void Required_ObjectInitializer()
    {
        ReqProfile p = new ReqProfile() { Name = "n", Level = 3 };
        Assert.True(p.Name == "n");
        Assert.Equal(3, p.Level);
    }

    [Fact]
    public void Required_CtorBodySatisfies()
    {
        ReqCtorInit c = new ReqCtorInit("t");
        Assert.True(c.Title == "t");
    }

    // ── per-accessor 可见性 ──

    [Fact]
    public void PrivateSet_ExternalReadInternalWrite()
    {
        SealedCounter c = new SealedCounter();
        Assert.Equal(0, c.Hits);
        c.Bump();
        c.Bump();
        Assert.Equal(2, c.Hits);
    }

    [Fact]
    public void PrivateSet_CtorAndRelabel()
    {
        WriteOnceLabel l = new WriteOnceLabel("a");
        Assert.True(l.Text == "a");
        l.Relabel("b");
        Assert.True(l.Text == "b");
    }

    // ── 表达式体方法 ──

    [Fact]
    public void ExprBodied_Methods()
    {
        Greeter g = new Greeter();
        Assert.True(g.Hi() == "hi");
        Assert.Equal(9, g.Square(3));
        Assert.True(g.Dup("ab") == "abab");
    }

    // ── 自定义接口 static abstract ──

    [Fact]
    public void StaticAbstract_CustomInterface_Method()
    {
        ColorMix a = new ColorMix(2);
        ColorMix b = new ColorMix(4);
        ColorMix m = Blend<ColorMix>(a, b);
        Assert.Equal(3, m.Level);
    }

    [Fact]
    public void StaticAbstract_CustomInterface_Property()
    {
        ColorMix n = BlendNeutral<ColorMix>();
        Assert.Equal(0, n.Level);
    }
}
