namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 用户自定义索引器 <c>this[...]</c> 单元测试（RFC 060）。
/// 覆盖开发者编写自定义集合、键值容器、矩阵、只读视图等类型的真实场景。
/// </summary>
public class IndexerTests
{
    [Fact]
    public void Indexer_GetSet_BackedByArray()
    {
        IntBag bag = new IntBag(4);
        bag[0] = 10;
        bag[3] = 40;
        Assert.Equal(10, bag[0]);
        Assert.Equal(40, bag[3]);
    }

    [Fact]
    public void Indexer_KeyBased_MapLike()
    {
        KeyBag bag = new KeyBag();
        bag["name"] = "Arc";
        bag["version"] = "1.0";
        Assert.Equal("Arc", bag["name"]);
        Assert.True(bag.Contains("version"));
        Assert.False(bag.Contains("missing"));
    }

    [Fact]
    public void Indexer_ReadOnly()
    {
        ReadOnlyBag bag = new ReadOnlyBag(5, 7, 9);
        Assert.Equal(5, bag[0]);
        Assert.Equal(9, bag[2]);
    }

    [Fact]
    public void Indexer_InterfaceImplementation()
    {
        IIntIndexer bag = new IndexerBag(3);
        bag[1] = 99;
        Assert.Equal(99, bag[1]);
    }

    [Fact]
    public void Indexer_ExpressionBodied()
    {
        Doubled d = new Doubled();
        d.Store(4);
        Assert.Equal(8, d[0]);
        Assert.Equal(8, d[5]);
    }
}

/// 基于数组的可读写整数索引器。
public class IntBag
{
    private int[] _items;

    public IntBag(int capacity)
    {
        _items = new int[capacity];
    }

    public int this[int index]
    {
        get { return _items[index]; }
        set { _items[index] = value; }
    }
}

/// 基于键值存储的索引器（字典式）。
public class KeyBag
{
    private string[] _keys;
    private string[] _values;
    private int _count;

    public KeyBag()
    {
        _keys = new string[8];
        _values = new string[8];
        _count = 0;
    }

    public string this[string key]
    {
        get { return Get(key); }
        set { Set(key, value); }
    }

    public bool Contains(string key)
    {
        int i = 0;
        while (i < _count)
        {
            if (_keys[i] == key)
            {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private string Get(string key)
    {
        int i = 0;
        while (i < _count)
        {
            if (_keys[i] == key)
            {
                return _values[i];
            }
            i = i + 1;
        }
        return null;
    }

    private void Set(string key, string value)
    {
        int i = 0;
        while (i < _count)
        {
            if (_keys[i] == key)
            {
                _values[i] = value;
                return;
            }
            i = i + 1;
        }
        _keys[_count] = key;
        _values[_count] = value;
        _count = _count + 1;
    }
}

/// 只读索引器（仅 get）。
public class ReadOnlyBag
{
    private int[] _items;

    public ReadOnlyBag(int a, int b, int c)
    {
        _items = new int[3];
        _items[0] = a;
        _items[1] = b;
        _items[2] = c;
    }

    public int this[int index]
    {
        get { return _items[index]; }
    }
}

/// 接口索引器声明，由 <see cref="IndexerBag"/> 实现。
public interface IIntIndexer
{
    int this[int index] { get; set; }
}

/// 接口索引器实现类。
public class IndexerBag : IIntIndexer
{
    private int[] _items;

    public IndexerBag(int capacity)
    {
        _items = new int[capacity];
    }

    public int this[int index]
    {
        get { return _items[index]; }
        set { _items[index] = value; }
    }
}

/// 表达式体索引器：get 返回存储值的两倍（验证 <c>T this[...] =&gt; expr;</c>）。
public class Doubled
{
    private int _value = 0;

    public void Store(int v)
    {
        _value = v;
    }

    public int this[int index] => _value * 2;
}
