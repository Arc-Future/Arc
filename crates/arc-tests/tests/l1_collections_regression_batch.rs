use arc_tests::assert_compiles_batch;

#[test]
fn compiles_collections_regression_batch() {
    assert_compiles_batch(
        "collections_regression",
        &[
            (
                "list_weak_repro",
                r#"using Arc;
using Arc.Collections;

class Foo {
    public int Value;
    public Foo(int v) {
        Value = v;
    }
}

void Main() {
    List<Weak<Foo>> list = new List<Weak<Foo>>();
    Foo f = new Foo(42);
    list.Add(new Weak<Foo>(f));

    Weak<Foo> x = list[0];
    Foo got = x.TryGet();
    if (got == null) {
        Console.WriteLine("fail null tryget");
        return;
    }
    if (got.Value != 42) {
        Console.WriteLine("fail value");
        return;
    }
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "weak_null_early_return",
                r#"using Arc;

class Node {
    public int Id;
    public Weak<Node> Link;

    public Node(int id) {
        this.Id = id;
    }

    public Node? Resolve() {
        if (this.Link == null) {
            return null;
        }
        return this.Link.TryGet();
    }
}

void Main() {
    Node root = new Node(1);
    if (root.Resolve() != null) {
        Console.WriteLine("fail:expect_null");
        return;
    }

    Node child = new Node(2);
    child.Link = new Weak<Node>(root);
    Node got = child.Resolve();
    if (got == null || got.Id != 1) {
        Console.WriteLine("fail:expect_root");
        return;
    }

    Console.WriteLine("ok");
}
"#,
            ),
            (
                "list_weak_lifecycle",
                r#"using Arc;
using Arc.Collections;

class Foo2 {
    public int Value;
    public Foo2(int v) {
        Value = v;
    }
}

void Main() {
    List<Weak<Foo2>> list = new List<Weak<Foo2>>();
    Foo2 f0 = new Foo2(10);
    Foo2 f1 = new Foo2(20);
    Foo2 f2 = new Foo2(30);
    list.Add(new Weak<Foo2>(f0));
    list.Add(new Weak<Foo2>(f1));
    list.Add(new Weak<Foo2>(f2));

    for (int i = 0; i < 3; i++) {
        Weak<Foo2> w = list[i];
        Foo2 got = w.TryGet();
        if (got == null) {
            Console.WriteLine("fail null i=" + i.ToString());
            return;
        }
        int expected = (i + 1) * 10;
        if (got.Value != expected) {
            Console.WriteLine("fail value i=" + i.ToString());
            return;
        }
    }

    Weak<Foo2> again = list[2];
    Foo2 againGot = again.TryGet();
    if (againGot == null || againGot.Value != 30) {
        Console.WriteLine("fail second access");
        return;
    }

    list.RemoveAt(1);
    Weak<Foo2> afterRemove = list[1];
    Foo2 afterRemoveGot = afterRemove.TryGet();
    if (afterRemoveGot == null || afterRemoveGot.Value != 30) {
        Console.WriteLine("fail after removeat");
        return;
    }

    list.Clear();
    Console.WriteLine("ok");
}
"#,
            ),
            (
                "list_class_add_stack",
                r#"using Arc;
using Arc.Collections;

class Item {
    public int Value;
    public Item(int v) { Value = v; }
}

void Main() {
    int cap = 200000;
    List<Item> items = new List<Item>(cap);
    int total = 0;
    for (int i = 0; i < cap; i++) {
        Item it = new Item(i);
        items.Add(it);
        total += it.Value;
    }
    Console.WriteLine("count=" + items.Count.ToString() + " total=" + total.ToString());
}
"#,
            ),
            (
                "array_jagged",
                r#"using Arc;

void Main() {
    int[][] j = new int[2][];
    if (j.Length != 2) { Console.WriteLine("jagged_fail:len1:" + j.Length); return; }
    j[0] = new int[3];
    j[1] = new int[2];
    j[0][1] = 42;
    j[1][0] = 7;
    if (j[0][1] != 42) { Console.WriteLine("jagged_fail:set1"); return; }
    if (j[1][0] != 7) { Console.WriteLine("jagged_fail:set2"); return; }
    if (j[0].Length != 3) { Console.WriteLine("jagged_fail:rowlen0:" + j[0].Length); return; }
    if (j[1].Length != 2) { Console.WriteLine("jagged_fail:rowlen1:" + j[1].Length); return; }

    j[0] = [1, 2, 3];
    j[1] = [4, 5];
    if (j[0][2] != 3) { Console.WriteLine("jagged_fail:colassign"); return; }
    if (j[1][1] != 5) { Console.WriteLine("jagged_fail:colassign2"); return; }

    int[][] g = [[1, 2], [3, 4, 5]];
    if (g.Length != 2) { Console.WriteLine("jagged_fail:nestedlen:" + g.Length); return; }
    if (g[1].Length != 3) { Console.WriteLine("jagged_fail:rowlen2:" + g[1].Length); return; }
    if (g[1][2] != 5) { Console.WriteLine("jagged_fail:nestedget"); return; }

    int[][][] cube = new int[2][][];
    cube[0] = new int[1][];
    cube[0][0] = new int[2];
    cube[0][0][1] = 99;
    if (cube[0][0][1] != 99) { Console.WriteLine("jagged_fail:cube"); return; }

    Console.WriteLine("array_jagged_e2e_ok");
}
"#,
            ),
            (
                "array_new",
                r#"using Arc;

void Main() {
    int[] a = new int[5];
    if (a.Length != 5) { Console.WriteLine("array_new_fail:len1:" + a.Length); return; }
    if (a[3] != 0) { Console.WriteLine("array_new_fail:zero1"); return; }
    a[3] = 42;
    if (a[3] != 42) { Console.WriteLine("array_new_fail:set1"); return; }

    int n = 7;
    byte[] b = new byte[n];
    if (b.Length != 7) { Console.WriteLine("array_new_fail:len2:" + b.Length); return; }
    if (b[6] != 0) { Console.WriteLine("array_new_fail:zero2"); return; }
    b[1] = 255;
    if (b[1] != 255) { Console.WriteLine("array_new_fail:set2"); return; }

    long[] l = new long[3];
    if (l.Length != 3) { Console.WriteLine("array_new_fail:len3:" + l.Length); return; }
    if (l[2] != 0) { Console.WriteLine("array_new_fail:zero3"); return; }
    l[0] = 1234567890123;
    if (l[0] != 1234567890123) { Console.WriteLine("array_new_fail:set3"); return; }

    Console.WriteLine("array_new_e2e_ok");
}
"#,
            ),
            (
                "index_elem_type_matrix",
                r#"using Arc;
using Arc.Collections;

class Tool
{
    public string Name { get; set; }
    public Tool(string name)
    {
        this.Name = name;
    }
}

class Bag
{
    public List<string> StringListField;
    public List<string> StringListProp { get; set; }
    public List<Tool> ObjListField;
}

class Container
{
    public List<string> StringListField;
}

class Outer
{
    public Container Container;
}

class StringBox
{
    private List<string> _items;
    public StringBox()
    {
        _items = new List<string>();
        _items.Add("");
        _items.Add("");
        _items.Add("");
    }
    public string this[int index]
    {
        get { return _items[index]; }
        set { _items[index] = value; }
    }
}

static class Cases
{
    public static void Cell1FieldReceiver()
    {
        Bag b = new Bag();
        b.StringListField = new List<string>();
        b.StringListField.Add("a");
        if (b.StringListField[0] == "a")
        {
            Console.WriteLine("cell1 ok");
        }
        else
        {
            Console.WriteLine("cell1 FAIL");
        }
    }

    public static void Cell2PropertyGetter()
    {
        Bag b = new Bag();
        b.StringListProp = new List<string>();
        b.StringListProp.Add("b");
        if (b.StringListProp[0] == "b")
        {
            Console.WriteLine("cell2 ok");
        }
        else
        {
            Console.WriteLine("cell2 FAIL");
        }
    }

    public static void Cell3ChainedNested()
    {
        Outer o = new Outer();
        o.Container = new Container();
        o.Container.StringListField = new List<string>();
        o.Container.StringListField.Add("c");
        if (o.Container.StringListField[0] == "c")
        {
            Console.WriteLine("cell3 ok");
        }
        else
        {
            Console.WriteLine("cell3 FAIL");
        }
    }

    public static void Cell4ObjListFieldMember()
    {
        Bag b = new Bag();
        b.ObjListField = new List<Tool>();
        b.ObjListField.Add(new Tool("hammer"));
        if (b.ObjListField[0].Name == "hammer")
        {
            Console.WriteLine("cell4 ok");
        }
        else
        {
            Console.WriteLine("cell4 FAIL");
        }
    }

    public static void Cell5StringChars()
    {
        string s = "abc";
        char c = s[0];
        if (c == 'a')
        {
            Console.WriteLine("cell5 ok");
        }
        else
        {
            Console.WriteLine("cell5 FAIL");
        }
    }

    public static void Cell6DictionaryStringValue()
    {
        Dictionary<string, string> d = new Dictionary<string, string>();
        d["k"] = "v";
        if (d["k"] == "v")
        {
            Console.WriteLine("cell6 ok");
        }
        else
        {
            Console.WriteLine("cell6 FAIL");
        }
    }

    public static void Cell7IndexNotEqualLiteral()
    {
        Bag b = new Bag();
        b.StringListField = new List<string>();
        b.StringListField.Add("read_file");
        b.StringListField.Add("list_dir");
        if (b.StringListField[0] != "write_file" && b.StringListField[1] == "list_dir")
        {
            Console.WriteLine("cell7 ok");
        }
        else
        {
            Console.WriteLine("cell7 FAIL");
        }
    }

    public static void Cell8ListStringWrite()
    {
        Bag b = new Bag();
        b.StringListField = new List<string>();
        b.StringListField.Add("old");
        b.StringListField[0] = "x";
        if (b.StringListField[0] == "x")
        {
            Console.WriteLine("cell8 ok");
        }
        else
        {
            Console.WriteLine("cell8 FAIL");
        }
    }

    public static void Cell9CustomIndexer()
    {
        StringBox box = new StringBox();
        box[0] = "hello";
        if (box[0] == "hello")
        {
            Console.WriteLine("cell9 ok");
        }
        else
        {
            Console.WriteLine("cell9 FAIL");
        }
    }
}

void Main()
{
    Cases.Cell1FieldReceiver();
    Cases.Cell2PropertyGetter();
    Cases.Cell3ChainedNested();
    Cases.Cell4ObjListFieldMember();
    Cases.Cell5StringChars();
    Cases.Cell6DictionaryStringValue();
    Cases.Cell7IndexNotEqualLiteral();
    Cases.Cell8ListStringWrite();
    Cases.Cell9CustomIndexer();
}
"#,
            ),
        ],
    );
}
