namespace UnitTest.Arc;

using Arc;
using Arc.Collections;
using Arc.QIF;

/// <summary>
/// List&lt;T&gt; 高级方法单元测试：排序/数组转换/批量操作/二分查找/逆序/谓词批量。
/// </summary>
public class ListTests
{
    [Fact]
    public void NewList_IsEmpty()
    {
        List<int> list = new List<int>();
        Assert.Equal(0, list.Count);
    }

    [Fact]
    public void Add_OneElement()
    {
        List<int> list = new List<int>();
        list.Add(42);
        Assert.Equal(1, list.Count);
    }

    [Fact]
    public void Add_MultipleElements()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        Assert.Equal(3, list.Count);
    }

    [Fact]
    public void GetItem_FirstElement()
    {
        List<int> list = new List<int>();
        list.Add(10);
        Assert.Equal(10, list[0]);
    }

    [Fact]
    public void GetItem_LastElement()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        Assert.Equal(3, list[2]);
    }

    [Fact]
    public void SetItem_ModifyExisting()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list[0] = 99;
        Assert.Equal(99, list[0]);
    }

    [Fact]
    public void Contains_Found()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        Assert.True(list.Contains(2));
    }

    [Fact]
    public void Contains_NotFound()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        Assert.False(list.Contains(99));
    }

    [Fact]
    public void IndexOf_Found()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        list.Add(30);
        Assert.Equal(1, list.IndexOf(20));
    }

    [Fact]
    public void IndexOf_NotFound()
    {
        List<int> list = new List<int>();
        list.Add(10);
        Assert.Equal(-1, list.IndexOf(99));
    }

    [Fact]
    public void Insert_AtHead()
    {
        List<int> list = new List<int>();
        list.Add(2);
        list.Insert(0, 1);
        Assert.Equal(1, list[0]);
        Assert.Equal(2, list[1]);
    }

    [Fact]
    public void Insert_InMiddle()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(3);
        list.Insert(1, 2);
        Assert.Equal(2, list[1]);
        Assert.Equal(3, list.Count);
    }

    [Fact]
    public void RemoveAt_First()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.RemoveAt(0);
        Assert.Equal(2, list.Count);
        Assert.Equal(2, list[0]);
    }

    [Fact]
    public void Remove_Found()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        list.Add(30);
        Assert.True(list.Remove(20));
        Assert.Equal(2, list.Count);
        Assert.False(list.Contains(20));
    }

    [Fact]
    public void Remove_NotFound()
    {
        List<int> list = new List<int>();
        list.Add(10);
        Assert.False(list.Remove(99));
        Assert.Equal(1, list.Count);
    }

    [Fact]
    public void Clear_EmptiesList()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Clear();
        Assert.Equal(0, list.Count);
    }

    [Fact]
    public void StringList_AddAndGet()
    {
        List<string> list = new List<string>();
        list.Add("alpha");
        list.Add("beta");
        Assert.Equal(2, list.Count);
        Assert.True(list[0] == "alpha");
        Assert.True(list[1] == "beta");
    }

    [Fact]
    public void FindIndex_Found()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        list.Add(30);
        Assert.Equal(1, list.FindIndex(x => x == 20));
        Assert.Equal(-1, list.FindIndex(x => x == 99));
    }

    [Fact]
    public void FindLastIndex_And_LastIndexOf()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(1);
        Assert.Equal(2, list.FindLastIndex(x => x == 1));
        Assert.Equal(2, list.LastIndexOf(1));
        Assert.Equal(-1, list.LastIndexOf(99));
    }

    [Fact]
    public void TrueForAll_Mixed()
    {
        List<int> list = new List<int>();
        list.Add(2);
        list.Add(4);
        list.Add(6);
        Assert.True(list.TrueForAll(x => x % 2 == 0));
        Assert.False(list.TrueForAll(x => x > 4));
        List<int> empty = new List<int>();
        Assert.True(empty.TrueForAll(x => x == 0));
    }

    // ── 排序 ──

    [Fact]
    public void Sort_Ascending_Numbers()
    {
        List<int> list = new List<int>();
        list.Add(5);
        list.Add(3);
        list.Add(8);
        list.Add(1);
        list.Sort();
        Assert.Equal(1, list[0]);
        Assert.Equal(3, list[1]);
        Assert.Equal(5, list[2]);
        Assert.Equal(8, list[3]);
    }

    [Fact]
    public void Sort_WithComparer_Descending()
    {
        List<int> list = new List<int>();
        list.Add(5);
        list.Add(3);
        list.Add(8);
        list.Sort((a, b) => b - a);
        Assert.Equal(8, list[0]);
        Assert.Equal(5, list[1]);
        Assert.Equal(3, list[2]);
    }

    [Fact]
    public void Sort_Strings()
    {
        List<string> list = new List<string>();
        list.Add("banana");
        list.Add("apple");
        list.Add("cherry");
        list.Sort();
        Assert.True(list[0] == "apple");
        Assert.True(list[1] == "banana");
        Assert.True(list[2] == "cherry");
    }

    // ── 数组转换 / 拷贝 ──

    [Fact]
    public void ToArray_CopiesElements()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        int[] arr = list.ToArray();
        Assert.True(arr.Length == 3);
        Assert.Equal(1, arr[0]);
        Assert.Equal(3, arr[2]);
    }

    [Fact]
    public void CopyTo_Array_AtOffset()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        int[] dst = new int[4];
        dst[0] = 99;
        list.CopyTo(dst, 1);
        Assert.Equal(99, dst[0]);
        Assert.Equal(10, dst[1]);
        Assert.Equal(20, dst[2]);
        Assert.Equal(0, dst[3]);
    }

    // ── 批量操作 ──

    [Fact]
    public void AddRange_Appends()
    {
        List<int> a = new List<int>();
        a.Add(1);
        a.Add(2);
        List<int> b = new List<int>();
        b.Add(3);
        b.Add(4);
        a.AddRange(b);
        Assert.Equal(4, a.Count);
        Assert.Equal(1, a[0]);
        Assert.Equal(4, a[3]);
    }

    [Fact]
    public void InsertRange_AtMiddle()
    {
        List<int> a = new List<int>();
        a.Add(1);
        a.Add(4);
        List<int> mid = new List<int>();
        mid.Add(2);
        mid.Add(3);
        a.InsertRange(1, mid);
        Assert.Equal(4, a.Count);
        Assert.Equal(1, a[0]);
        Assert.Equal(2, a[1]);
        Assert.Equal(3, a[2]);
        Assert.Equal(4, a[3]);
    }

    [Fact]
    public void RemoveRange_RemovesSlice()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.Add(4);
        list.Add(5);
        list.RemoveRange(1, 2);
        Assert.Equal(3, list.Count);
        Assert.Equal(1, list[0]);
        Assert.Equal(4, list[1]);
        Assert.Equal(5, list[2]);
    }

    [Fact]
    public void GetRange_ReturnsCopy()
    {
        List<int> list = new List<int>();
        list.Add(0);
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.Add(4);
        List<int> slice = list.GetRange(1, 3);
        Assert.Equal(3, slice.Count);
        Assert.Equal(1, slice[0]);
        Assert.Equal(3, slice[2]);
        // 源列表不受影响
        Assert.Equal(5, list.Count);
    }

    // ── 二分查找（前提：列表已排序）──

    [Fact]
    public void BinarySearch_FoundAndNotFound()
    {
        List<int> list = new List<int>();
        list.Add(10);
        list.Add(20);
        list.Add(30);
        Assert.Equal(1, list.BinarySearch(20));
        // 未找到返回插入点补码（负数）
        Assert.True(list.BinarySearch(25) < 0);
        Assert.True(list.BinarySearch(5) < 0);
    }

    // ── 逆序 ──

    [Fact]
    public void Reverse_ReversesOrder()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.Reverse();
        Assert.Equal(3, list[0]);
        Assert.Equal(2, list[1]);
        Assert.Equal(1, list[2]);
    }

    // ── 谓词批量 ──

    [Fact]
    public void RemoveAll_RemovesMatching()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.Add(4);
        list.Add(5);
        int removed = list.RemoveAll(x => x % 2 == 1);
        Assert.Equal(3, removed);
        Assert.Equal(2, list.Count);
        Assert.Equal(2, list[0]);
        Assert.Equal(4, list[1]);
    }

    [Fact]
    public void FindAll_ReturnsNewList()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        list.Add(4);
        List<int> evens = list.FindAll(x => x % 2 == 0);
        Assert.Equal(2, evens.Count);
        Assert.Equal(2, evens[0]);
        Assert.Equal(4, evens[1]);
        Assert.Equal(4, list.Count); // 源列表不变
    }

    [Fact]
    public void Exists_Predicate()
    {
        List<int> list = new List<int>();
        list.Add(2);
        list.Add(4);
        list.Add(6);
        Assert.True(list.Exists(x => x == 4));
        Assert.False(list.Exists(x => x == 5));
    }

    [Fact]
    public void ForEach_AppliesAction()
    {
        List<int> list = new List<int>();
        list.Add(1);
        list.Add(2);
        list.Add(3);
        int sum = 0;
        list.ForEach(x => { sum = sum + x; });
        Assert.Equal(6, sum);
    }
}
