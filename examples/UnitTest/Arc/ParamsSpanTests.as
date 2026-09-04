using Arc;
using Arc.QIF;

/// RFC 078/077：`params ReadOnlySpan<T>` 栈脱糖（非 Skip）。
public class ParamsSpanTests {
    private int Sum(params ReadOnlySpan<int> xs) {
        int total = 0;
        for (int i = 0; i < xs.Length; i++) {
            total = total + xs[i];
        }
        return total;
    }

    [Fact]
    public void Params_Empty_And_Pack() {
        Assert.Equal(0, this.Sum());
        Assert.Equal(6, this.Sum(1, 2, 3));
    }

    [Fact]
    public void Params_PassThrough_ReadOnlySpan() {
        int[] buf = [7, 8, 9];
        Assert.Equal(24, this.Sum(buf.AsReadOnlySpan()));
    }
}
