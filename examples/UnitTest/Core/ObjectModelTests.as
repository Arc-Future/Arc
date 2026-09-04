namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 对象模型最小契约：类方法 + 接口派发。
/// </summary>

interface IShape {
    int Area();
}

class ObjRectangle : IShape {
    public int Width;
    private int Height;

    public ObjRectangle(int width, int height) {
        Width = width;
        Height = height;
    }

    public int Area() {
        return Width * Height;
    }
}

class ObjSquare : ObjRectangle {
    public ObjSquare(int side) : base(side, side) {
    }
}

/// RFC 069 M5：自定义 init 体 + 对象初始化器。
class InitCounter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
    public InitCounter() {}
}

/// RFC 069 M5+：record + 自定义 init × `with`。
record InitRecordCounter {
    int _n;
    public int N {
        get { return _n; }
        init { _n = value; }
    }
    public InitRecordCounter(int n) { _n = n; }
}

public class ObjectModelTests
{
    [Fact]
    public void Class_Method()
    {
        ObjRectangle r = new ObjRectangle(5, 10);
        Assert.Equal(50, r.Area());
    }

    [Fact]
    public void InitAccessor_CustomObjectInitializer()
    {
        InitCounter c = new InitCounter() { N = 7 };
        Assert.Equal(7, c.N);
    }

    [Fact]
    public void InitAccessor_WithCustomInit_M5Plus()
    {
        InitRecordCounter c = new InitRecordCounter(1);
        InitRecordCounter d = c with { N = 7 };
        Assert.Equal(7, d.N);
        Assert.Equal(1, c.N);
    }

    [Fact]
    public void Interface_Dispatch()
    {
        IShape shape = new ObjRectangle(6, 7);
        Assert.Equal(42, shape.Area());
    }

    [Fact]
    public void Inheritance_InterfaceDispatch()
    {
        IShape shape = new ObjSquare(4);
        Assert.Equal(16, shape.Area());
    }
}
