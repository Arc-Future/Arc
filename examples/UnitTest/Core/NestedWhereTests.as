namespace UnitTest.Core;

using Arc;
using Arc.QIF;

/// <summary>
/// 嵌套 where 子句单元测试：覆盖 NestedWhere 示例。
/// 验证多 where 子句中类型参数交叉引用的约束解析与实例化。
/// </summary>

interface IBag<U> {
    U First();
}

class Item {
    public string Label;

    public Item(string label) {
        Label = label;
    }
}

class Bag : IBag<Item> {
    private Item _item;

    public Bag(Item item) {
        _item = item;
    }

    public Item First() {
        return _item;
    }
}

class Repo<T, U> where T : IBag<U> where U : class {
    private T _store;

    public Repo(T store) {
        _store = store;
    }

    public U Peek() {
        return _store.First();
    }
}

public class NestedWhereTests
{
    [Fact]
    public void CrossReferencingWhereClauses()
    {
        Repo<Bag, Item> repo = new Repo<Bag, Item>(new Bag(new Item("nested")));
        Item x = repo.Peek();
        Assert.True(x.Label == "nested");
    }
}
