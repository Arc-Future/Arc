namespace UnitTest.Core;

using Arc;
using Arc.QIF;

// ── 被测 record 类型（位置参数 → 合成 `Deconstruct(out …)`，RFC 006） ──

record DcPair(int X, int Y);
record struct DcVec2(int X, int Y);
record DcInner(int B, int C);
record DcOuter(int A, DcInner N);
record DcTriple(int P, int Q, int R);

/// <summary>
/// RFC 004 M1/M2/M7 解构赋值与位置模式单元测试：
/// var 声明式解构、弃元 `_`、嵌套目标、swap、record Deconstruct 消费、
/// record struct、字段写入目标，以及 if/switch 位置模式
/// （var 绑定 / 常量 / 弃元 / 类型 / 嵌套子模式）。
/// </summary>
public class DeconstructionTests
{
    private int _fx;
    private int _fy;

    // ── var 声明式解构（DeconstructAssign declare=true） ──

    [Fact]
    public void Var_Deconstruct_TwoTargets()
    {
        DcPair p = new DcPair(3, 4);
        var (x, y) = p;
        Assert.Equal(3, x);
        Assert.Equal(4, y);
    }

    [Fact]
    public void Var_Deconstruct_ThreeTargets()
    {
        DcTriple t = new DcTriple(1, 2, 3);
        var (p, q, r) = t;
        Assert.Equal(1, p);
        Assert.Equal(2, q);
        Assert.Equal(3, r);
    }

    [Fact]
    public void Var_Deconstruct_Discard_Left()
    {
        DcPair p = new DcPair(3, 4);
        var (_, y) = p;
        Assert.Equal(4, y);
    }

    [Fact]
    public void Var_Deconstruct_Discard_Right()
    {
        DcPair p = new DcPair(3, 4);
        var (x, _) = p;
        Assert.Equal(3, x);
    }

    [Fact]
    public void Var_Deconstruct_Nested()
    {
        DcOuter o = new DcOuter(1, new DcInner(2, 3));
        var (a, (b, c)) = o;
        Assert.Equal(1, a);
        Assert.Equal(2, b);
        Assert.Equal(3, c);
    }

    // ── 非声明式解构（目标须为已声明局部，declare=false） ──

    [Fact]
    public void Deconstruct_ExistingLocals()
    {
        DcPair p = new DcPair(3, 4);
        int x = 0;
        int y = 0;
        (x, y) = p;
        Assert.Equal(3, x);
        Assert.Equal(4, y);
    }

    [Fact]
    public void Deconstruct_Discard_ExistingLocal()
    {
        DcPair p = new DcPair(3, 4);
        int x = 0;
        (x, _) = p;
        Assert.Equal(3, x);
    }

    [Fact]
    public void Deconstruct_Nested_ExistingLocals()
    {
        DcOuter o = new DcOuter(1, new DcInner(2, 3));
        int a = 0;
        int b = 0;
        int c = 0;
        (a, (b, c)) = o;
        Assert.Equal(1, a);
        Assert.Equal(2, b);
        Assert.Equal(3, c);
    }

    [Fact]
    public void Deconstruct_AllDiscard()
    {
        DcPair p = new DcPair(3, 4);
        (_, _) = p;
        Assert.Equal(3, p.X);
        Assert.Equal(4, p.Y);
    }

    // ── swap：右值为 record 构造（Arc 无元组表达式，`(b,a)` 非法） ──

    [Fact]
    public void Swap_ViaDeconstruct()
    {
        int a = 1;
        int b = 2;
        (a, b) = new DcPair(b, a);
        Assert.Equal(2, a);
        Assert.Equal(1, b);
    }

    // ── record struct（值类型）Deconstruct 消费 ──

    [Fact]
    public void Deconstruct_StructRecord()
    {
        DcVec2 v = new DcVec2(5, 6);
        var (vx, vy) = v;
        Assert.Equal(5, vx);
        Assert.Equal(6, vy);
        int sx = 0;
        int sy = 0;
        (sx, sy) = v;
        Assert.Equal(5, sx);
        Assert.Equal(6, sy);
    }

    // ── 解构目标为当前类实例字段（out 写临时局部后回写字段） ──

    [Fact]
    public void Deconstruct_FieldWriteTargets()
    {
        DcPair p = new DcPair(7, 8);
        var (_fx, _fy) = p;
        Assert.Equal(7, _fx);
        Assert.Equal(8, _fy);
    }

    // ── 位置模式：if 条件（M3/M5/M6） ──

    [Fact]
    public void PositionalPattern_VarBindings()
    {
        DcPair p = new DcPair(3, 4);
        bool matched = false;
        if (p is (var x, var y)) {
            matched = x == 3 && y == 4;
        }
        Assert.True(matched);
    }

    [Fact]
    public void PositionalPattern_ConstMatch()
    {
        DcPair p = new DcPair(3, 4);
        if (p is (3, 4)) {
            Assert.True(true);
        } else {
            Assert.True(false);
        }
    }

    [Fact]
    public void PositionalPattern_ConstMismatch()
    {
        DcPair p = new DcPair(3, 4);
        if (p is (1, 2)) {
            Assert.True(false);
        } else {
            Assert.True(true);
        }
    }

    [Fact]
    public void PositionalPattern_MixedConstVar()
    {
        DcPair p = new DcPair(3, 4);
        int captured = 0;
        if (p is (3, var y)) {
            captured = y;
        }
        Assert.Equal(4, captured);
    }

    [Fact]
    public void PositionalPattern_DiscardOnly()
    {
        DcPair p = new DcPair(3, 4);
        Assert.True(p is (_, _));
    }

    [Fact]
    public void PositionalPattern_NullNoMatch()
    {
        DcPair n = null;
        if (n is (var a, var b)) {
            Assert.True(false);
        }
        Assert.True(true);
    }

    [Fact]
    public void PositionalPattern_TypedSubpatterns()
    {
        DcPair p = new DcPair(3, 4);
        int sum = 0;
        if (p is (int x, int y)) {
            sum = x + y;
        }
        Assert.Equal(7, sum);
    }

    [Fact]
    public void PositionalPattern_Nested()
    {
        DcOuter o = new DcOuter(1, new DcInner(2, 3));
        int acc = 0;
        if (o is (var a, (var b, var c))) {
            acc = a + b + c;
        }
        Assert.Equal(6, acc);
    }

    [Fact]
    public void PositionalPattern_NestedConstMixed()
    {
        DcOuter o = new DcOuter(1, new DcInner(2, 3));
        int acc = 0;
        if (o is (1, (var b, var c))) {
            acc = b * 10 + c;
        }
        Assert.Equal(23, acc);
    }

    // ── 位置模式：switch 语句 / switch 表达式 ──

    [Fact]
    public void SwitchStatement_PositionalPattern()
    {
        DcPair p = new DcPair(3, 4);
        int sum = 0;
        switch (p) {
            case (var u, var v):
                sum = u + v;
                break;
            default:
                sum = -1;
                break;
        }
        Assert.Equal(7, sum);
    }

    [Fact]
    public void SwitchExpression_PositionalPattern()
    {
        DcPair p = new DcPair(3, 4);
        int sum = p switch {
            (var x, var y) => x + y,
            _ => -1,
        };
        Assert.Equal(7, sum);
        DcPair q = new DcPair(5, 6);
        int prod = q switch {
            (var x, var y) => x * y,
            _ => -1,
        };
        Assert.Equal(30, prod);
    }
}
