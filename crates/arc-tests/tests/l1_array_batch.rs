//! L1 compile-only batch: Array builtin ops, element store, invariant.
//! Extracted from arc-integration e2e tests (array_builtin, array_elem_store, array_invariant).

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_l1_array_batch() {
    assert_compiles_batch(
        "l1_array_batch",
        &[
            // array_builtin: Copy / Clear / Reverse / IndexOf
            (
                "array_copy_clear_reverse",
                r#"using Arc;
void Main() {
    int[] src = [1, 2, 3, 4]; int[] dst = [0, 0, 0, 0];
    Array.Copy(src, 1, dst, 0, 3);
    int[] a = [10, 20, 30, 40]; Array.Clear(a, 1, 2);
    int[] b = [1, 2, 3, 4]; Array.Reverse(b);
    int[] c = [5, 7, 9, 7];
    int i1 = Array.IndexOf(c, 7);
    int i2 = Array.IndexOf(c, 3);
    int i3 = Array.IndexOf(c, 5);
    Console.WriteLine("ok");
}
"#,
            ),
            // array_builtin: LastIndexOf / Empty / Resize
            (
                "array_lastindexof_resize",
                r#"using Arc;
void Main() {
    int[] a = [5, 7, 9, 7];
    int li1 = Array.LastIndexOf(a, 7);
    int li2 = Array.LastIndexOf(a, 3);
    int li3 = Array.LastIndexOf(a, 5);
    int[] e = Array.Empty();
    int len = e.Length;
    int[] g = [1, 2, 3];
    Array.Resize(ref g, 5);
    int len2 = g.Length;
    Array.Resize(ref g, 2);
    int len3 = g.Length;
    int[] n = null; Array.Resize(ref n, 3);
    int nlen = n.Length;
    Console.WriteLine("ok");
}
"#,
            ),
            // array_builtin: Predicate Exists/Find/ForEach
            (
                "array_predicate_exists_find",
                r#"using Arc;
void Main() {
    int[] a = [2, 3, 4, 5];
    bool ex1 = Array.Exists(a, x => x % 2 != 0);
    bool ex2 = Array.Exists(a, x => x == 99);
    int f1 = Array.Find(a, x => x % 2 != 0);
    int f2 = Array.FindLast(a, x => x % 2 != 0);
    int fi1 = Array.FindIndex(a, x => x % 2 != 0);
    int fi2 = Array.FindLastIndex(a, x => x % 2 != 0);
    bool tf1 = Array.TrueForAll(a, x => x > 0);
    bool tf2 = Array.TrueForAll(a, x => x % 2 != 0);
    int[] empty = [];
    bool tf3 = Array.TrueForAll(empty, x => x == 0);
    Array.ForEach(a, x => Console.WriteLine(x.ToString()));
    Console.WriteLine("ok");
}
"#,
            ),
            // array_builtin: Sort / BinarySearch
            (
                "array_sort_binary_search",
                r#"using Arc;
void Main() {
    int[] a = [3, 1, 4, 1, 5];
    Array.Sort(a);
    int[] s = [1, 2, 3, 5];
    int bs1 = Array.BinarySearch(s, 2);
    int bs2 = Array.BinarySearch(s, 4);
    Console.WriteLine("ok");
}
"#,
            ),
            // array_builtin: FindAll / ConvertAll
            (
                "array_findall_convertall",
                r#"using Arc;
void Main() {
    int[] a = [1, 2, 3, 4, 5];
    int[] odds = Array.FindAll(a, x => x % 2 != 0);
    int oddsLen = odds.Length;
    int[] none = Array.FindAll(a, x => x > 100);
    int noneLen = none.Length;
    int[] d = Array.ConvertAll(a, x => x * 2);
    int dLen = d.Length;
    Console.WriteLine("ok");
}
"#,
            ),
            // array_elem_store: const index assign
            (
                "array_elem_const_index",
                r#"using Arc;
void Main() {
    int[] a = [10, 20, 30];
    a[1] = 99;
    int v0 = a[0];
    int v1 = a[1];
    int v2 = a[2];
    Console.WriteLine("ok");
}
"#,
            ),
            // array_elem_store: var index assign
            (
                "array_elem_var_index",
                r#"using Arc;
void Main() {
    int[] a = [1, 2, 3, 4];
    int i = 2;
    a[i] = 42;
    int v0 = a[0];
    int v1 = a[1];
    int v2 = a[2];
    int v3 = a[3];
    Console.WriteLine("ok");
}
"#,
            ),
            // array_elem_store: datetime leap md pattern
            (
                "array_elem_leap_md",
                r#"using Arc;
void Main() {
    int[] md = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    bool leap = true;
    if (leap) { md[1] = 29; }
    int md1 = md[1];
    int md0 = md[0];
    int rem = 59;
    int month = 1;
    int i2 = 0;
    bool go = true;
    while (go) {
        if (i2 >= 12) { go = false; }
        else {
            if (rem < md[i2]) { go = false; }
            else {
                rem = rem - md[i2];
                month = month + 1;
                i2 = i2 + 1;
            }
        }
    }
    int day = rem + 1;
    Console.WriteLine("ok");
}
"#,
            ),
            // array_invariant: same elem type assign (compile-ok)
            (
                "array_same_elem_type",
                r#"using Arc;
void Main() {
    Dog[] a = [new Dog()];
    Dog[] b = a;
    int id = b[0].Id();
    Console.WriteLine("ok");
}

class Dog { public Dog() {} public int Id() { return 1; } }
"#,
            ),
        ],
    );
}
