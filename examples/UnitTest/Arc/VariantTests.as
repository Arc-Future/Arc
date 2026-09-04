namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

variant ShapeVariant {
    | Circle of int
    | Square of int
    | Empty
}

/// <summary>
/// Variant 代数数据类型单元测试：构造 + switch 匹配（原 Deferred）。
/// </summary>
public class VariantTests
{
    [Fact]
    public void Variant_Circle_Construction()
    {
        ShapeVariant s = ShapeVariant.Circle(5);
        bool isCircle = false;
        switch (s) {
            case ShapeVariant.Circle(r): isCircle = true; break;
            case ShapeVariant.Square(side): break;
            case ShapeVariant.Empty: break;
        }
        Assert.True(isCircle);
    }

    [Fact]
    public void Variant_Circle_ExtractPayload()
    {
        ShapeVariant s = ShapeVariant.Circle(5);
        int radius = 0;
        switch (s) {
            case ShapeVariant.Circle(r): radius = r; break;
            case ShapeVariant.Square(side): break;
            case ShapeVariant.Empty: break;
        }
        Assert.Equal(5, radius);
    }

    [Fact]
    public void Variant_Square()
    {
        ShapeVariant s = ShapeVariant.Square(10);
        int side = 0;
        switch (s) {
            case ShapeVariant.Circle(r): break;
            case ShapeVariant.Square(sideLen): side = sideLen; break;
            case ShapeVariant.Empty: break;
        }
        Assert.Equal(10, side);
    }

    [Fact]
    public void Variant_Empty()
    {
        ShapeVariant s = ShapeVariant.Empty;
        bool isEmpty = false;
        switch (s) {
            case ShapeVariant.Circle(r): break;
            case ShapeVariant.Square(side): break;
            case ShapeVariant.Empty: isEmpty = true; break;
        }
        Assert.True(isEmpty);
    }
}
