namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

/// <summary>
/// Tensor&lt;T&gt; 门面（RFC 021 Phase 1）：创建、索引、Add/Sub/Mul/Matmul。
/// </summary>
public class TensorTests
{
    [Fact]
    public void GetSet_AndShape()
    {
        Tensor<double> a = new Tensor<double>(2, 3);
        a.Set(0, 0, 1.0);
        a.Set(0, 1, 2.0);
        a.Set(1, 2, 3.0);
        Assert.True(a.Get(0, 0) == 1.0);
        Assert.True(a.Get(0, 1) == 2.0);
        Assert.True(a.Get(1, 2) == 3.0);
        Assert.Equal(2, a.Rank);
        Assert.Equal(2, a.Rows);
        Assert.Equal(3, a.Cols);
        Assert.Equal(6, a.Total);
    }

    [Fact]
    public void ElementWise_AddSubMul()
    {
        Tensor<double> b = new Tensor<double>(2, 2);
        b.Set(0, 0, 1.0);
        b.Set(0, 1, 2.0);
        b.Set(1, 0, 3.0);
        b.Set(1, 1, 4.0);

        Tensor<double> c = new Tensor<double>(2, 2);
        c.Set(0, 0, 10.0);
        c.Set(0, 1, 20.0);
        c.Set(1, 0, 30.0);
        c.Set(1, 1, 40.0);

        Tensor<double> sum = b.Add(c);
        Assert.True(sum.Get(0, 0) == 11.0 && sum.Get(0, 1) == 22.0);
        Assert.True(sum.Get(1, 0) == 33.0 && sum.Get(1, 1) == 44.0);

        Tensor<double> diff = c.Sub(b);
        Assert.True(diff.Get(0, 0) == 9.0 && diff.Get(1, 1) == 36.0);

        Tensor<double> had = b.Mul(b);
        Assert.True(had.Get(0, 0) == 1.0 && had.Get(1, 1) == 16.0);
    }

    [Fact]
    public void Matmul_Identity()
    {
        Tensor<double> ident = new Tensor<double>(2, 2);
        ident.Set(0, 0, 1.0);
        ident.Set(0, 1, 0.0);
        ident.Set(1, 0, 0.0);
        ident.Set(1, 1, 1.0);

        Tensor<double> e = new Tensor<double>(2, 2);
        e.Set(0, 0, 5.0);
        e.Set(0, 1, 6.0);
        e.Set(1, 0, 7.0);
        e.Set(1, 1, 8.0);

        Tensor<double> prod = ident.Matmul(e);
        Assert.True(prod.Get(0, 0) == 5.0 && prod.Get(0, 1) == 6.0);
        Assert.True(prod.Get(1, 0) == 7.0 && prod.Get(1, 1) == 8.0);
    }
}
