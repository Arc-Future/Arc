//! L1 批量：集合核心回归集（6 case）。
//!
//! 从 collections_core_batch_e2e.rs 提取，保留原始语法。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_collections_core_batch() {
    assert_compiles_batch(
        "collections_core",
        &[
            (
                "queue_stack_basic",
                r#"using Arc;
using Arc.Collections;

void Main() {
    Queue<int> q = new Queue<int>();
    q.Enqueue(1);
    q.Enqueue(2);
    q.Enqueue(3);
    if (q.Count != 3) { Console.WriteLine("Q_COUNT_FAIL"); return; }
    if (q.Dequeue() != 1) { Console.WriteLine("Q_DEQ_FAIL"); return; }
    if (q.Peek() != 2) { Console.WriteLine("Q_PEEK_FAIL"); return; }
    q.Clear();
    if (q.Count != 0) { Console.WriteLine("Q_CLEAR_FAIL"); return; }
    Console.WriteLine("QUEUE_OK");

    Stack<int> s = new Stack<int>();
    s.Push(1);
    s.Push(2);
    s.Push(3);
    if (s.Count != 3) { Console.WriteLine("S_COUNT_FAIL"); return; }
    if (s.Pop() != 3) { Console.WriteLine("S_POP_FAIL"); return; }
    if (s.Peek() != 2) { Console.WriteLine("S_PEEK_FAIL"); return; }
    s.Clear();
    if (s.Count != 0) { Console.WriteLine("S_CLEAR_FAIL"); return; }
    Console.WriteLine("STACK_OK");
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
    if (c.Count != 3) { Console.WriteLine("C_COUNT_FAIL"); return; }
    if (c[0] != 10) { Console.WriteLine("C_IDX0_FAIL"); return; }
    if (!c.Contains(20)) { Console.WriteLine("C_CONTAINS_FAIL"); return; }
    if (c.IndexOf(30) != 2) { Console.WriteLine("C_INDEXOF_FAIL"); return; }
    c.Clear();
    if (c.Count != 0) { Console.WriteLine("C_CLEAR_FAIL"); return; }
    Console.WriteLine("COLLECTION_OK");
}
"#,
            ),
            (
                "linked_list_basic",
                r#"using Arc;
using Arc.Collections;

void Main() {
    LinkedList<int> list = new LinkedList<int>();
    list.AddLast(10);
    list.AddLast(20);
    list.AddFirst(5);
    if (list.Count != 3) { Console.WriteLine("LL_COUNT_FAIL"); return; }
    if (!list.Contains(20)) { Console.WriteLine("LL_CONTAINS_FAIL"); return; }
    LinkedListNode<int> first = list.First;
    if (first.Value != 5) { Console.WriteLine("LL_FIRST_FAIL"); return; }
    LinkedListNode<int> last = list.Last;
    if (last.Value != 20) { Console.WriteLine("LL_LAST_FAIL"); return; }
    list.Clear();
    if (list.Count != 0) { Console.WriteLine("LL_CLEAR_FAIL"); return; }
    Console.WriteLine("LINKEDLIST_OK");
}
"#,
            ),
            (
                "sorted_dictionary_basic",
                r#"using Arc;
using Arc.Collections;

void Main() {
    SortedDictionary<int, int> d = new SortedDictionary<int, int>();
    d.Add(30, 300);
    d.Add(10, 100);
    d.Add(20, 200);
    if (d.Count != 3) { Console.WriteLine("SD_COUNT_FAIL"); return; }
    if (!d.ContainsKey(20)) { Console.WriteLine("SD_CONTAINS_FAIL"); return; }
    if (d[10] != 100) { Console.WriteLine("SD_GET_FAIL"); return; }
    int got = 0;
    if (!d.TryGetValue(20, out got)) { Console.WriteLine("SD_TRY_FAIL"); return; }
    if (got != 200) { Console.WriteLine("SD_TRY_VAL"); return; }
    d.Clear();
    if (d.Count != 0) { Console.WriteLine("SD_CLEAR_FAIL"); return; }
    Console.WriteLine("SORTEDDICT_OK");
}
"#,
            ),
            (
                "sorted_set_basic",
                r#"using Arc;
using Arc.Collections;

void Main() {
    SortedSet<int> s = new SortedSet<int>();
    s.Add(30);
    s.Add(10);
    s.Add(20);
    if (s.Count != 3) { Console.WriteLine("SS_COUNT_FAIL"); return; }
    if (!s.Contains(20)) { Console.WriteLine("SS_CONTAINS_FAIL"); return; }
    if (s.Min != 10) { Console.WriteLine("SS_MIN_FAIL"); return; }
    if (s.Max != 30) { Console.WriteLine("SS_MAX_FAIL"); return; }
    s.Clear();
    if (s.Count != 0) { Console.WriteLine("SS_CLEAR_FAIL"); return; }
    Console.WriteLine("SORTEDSET_OK");
}
"#,
            ),
            (
                "readonly_collection_basic",
                r#"using Arc;
using Arc.Collections;

void Main() {
    List<int> list = new List<int>();
    list.Add(10);
    list.Add(20);
    list.Add(30);
    ReadOnlyCollection<int> roc = new ReadOnlyCollection<int>(list);
    if (roc.Count != 3) { Console.WriteLine("ROC_COUNT_FAIL"); return; }
    if (roc[0] != 10) { Console.WriteLine("ROC_IDX0_FAIL"); return; }
    if (!roc.Contains(10)) { Console.WriteLine("ROC_CONTAINS_FAIL"); return; }
    int[] buf = [0, 0, 0, 0, 0];
    roc.CopyTo(buf, 1);
    if (buf[1] != 10 || buf[2] != 20 || buf[3] != 30) { Console.WriteLine("ROC_COPYTO_FAIL"); return; }
    list.Add(40);
    if (roc.Count != 4) { Console.WriteLine("ROC_LIVE_COUNT"); return; }
    Console.WriteLine("READONLYCOLL_OK");
}
"#,
            ),
        ],
    );
}
