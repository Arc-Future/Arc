namespace UnitTest.Arc;

using Arc;
using Arc.QIF;

public class ArrayTests
{
    [Fact]
    public void Sort_BinarySearch()
    {
        int[] a = [3, 1, 2];
        Array.Sort(a);
        Assert.Equal(1, a[0]);
        Assert.Equal(2, a[1]);
        Assert.Equal(3, a[2]);
        Assert.Equal(1, Array.BinarySearch(a, 2));
        Assert.Equal(-1, Array.BinarySearch(a, 0));
    }

    [Fact]
    public void FindAll_ConvertAll()
    {
        int[] a = [1, 2, 3, 4];
        int[] evens = Array.FindAll(a, x => x % 2 == 0);
        Assert.Equal(2, evens.Length);
        Assert.Equal(2, evens[0]);
        Assert.Equal(4, evens[1]);
        int[] squared = Array.ConvertAll(a, x => x * x);
        Assert.Equal(4, squared.Length);
        Assert.Equal(1, squared[0]);
        Assert.Equal(4, squared[1]);
        Assert.Equal(9, squared[2]);
        Assert.Equal(16, squared[3]);
    }
}
