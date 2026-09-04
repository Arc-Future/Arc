//! L1 批量：集合容器回归集（Queue/Stack/Collection/LinkedList/SortedDictionary/SortedSet）。
//!
//! 所有 case 合并为单次 assert_compiles_batch（一次 std 加载 + 一次 clang 链接）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_collections_batch() {
    assert_compiles_batch(
        "collections",
        &[
            (
                "queue_stack",
                r#"using Arc;
using Arc.Collections;
void Main() {
    Queue<int> q = new Queue<int>();
    q.Enqueue(1);
    q.Enqueue(2);
    q.Enqueue(3);
    int c = q.Count;
    Console.WriteLine(c.ToString());
}
"#,
            ),
            (
                "stack_basic",
                r#"using Arc;
using Arc.Collections;
void Main() {
    Stack<int> s = new Stack<int>();
    s.Push(1);
    s.Push(2);
    s.Push(3);
    int top = s.Pop();
    Console.WriteLine(top.ToString());
}
"#,
            ),
            (
                "collection_basic",
                r#"using Arc;
using Arc.Collections;
void Main() {
    Collection<int> c = new Collection<int>();
    c.Add(10);
    c.Add(20);
    c.Add(30);
    int n = c.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "linked_list",
                r#"using Arc;
using Arc.Collections;
void Main() {
    LinkedList<int> list = new LinkedList<int>();
    list.AddLast(10);
    list.AddLast(20);
    list.AddFirst(5);
    int n = list.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "sorted_dictionary",
                r#"using Arc;
using Arc.Collections;
void Main() {
    SortedDictionary<int, int> d = new SortedDictionary<int, int>();
    d.Add(30, 300);
    d.Add(10, 100);
    d.Add(20, 200);
    int n = d.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "sorted_set",
                r#"using Arc;
using Arc.Collections;
void Main() {
    SortedSet<int> s = new SortedSet<int>();
    s.Add(30);
    s.Add(10);
    s.Add(20);
    int n = s.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "readonly_collection",
                r#"using Arc;
using Arc.Collections;
void Main() {
    List<int> list = new List<int>();
    list.Add(10);
    list.Add(20);
    list.Add(30);
    ReadOnlyCollection<int> roc = new ReadOnlyCollection<int>(list);
    int n = roc.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "dictionary_basic",
                r#"using Arc;
using Arc.Collections;
void Main() {
    Dictionary<string, int> d = new Dictionary<string, int>();
    d["key"] = 42;
    int v = d["key"];
    Console.WriteLine(v.ToString());
}
"#,
            ),
            (
                "list_generic",
                r#"using Arc;
using Arc.Collections;
void Main() {
    List<string> names = new List<string>();
    names.Add("Alice");
    names.Add("Bob");
    names.Add("Charlie");
    int n = names.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
            (
                "hash_set",
                r#"using Arc;
using Arc.Collections;
void Main() {
    HashSet<int> s = new HashSet<int>();
    s.Add(1);
    s.Add(2);
    s.Add(3);
    s.Add(2);
    int n = s.Count;
    Console.WriteLine(n.ToString());
}
"#,
            ),
        ],
    );
}
