//! L1 compile-only batch: Dictionary ops (builtin + zero-boxing).
//! Extracted from arc-integration e2e tests (dictionary, dictionary_zero_boxing).

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_l1_dictionary_batch() {
    assert_compiles_batch(
        "l1_dictionary_batch",
        &[
            // dictionary: basic ops (QIF source, converted)
            (
                "dict_basic_ops",
                r#"using Arc;
using Arc.Collections;
void Main() {
    Dictionary<string,int> d = new Dictionary<string,int>();
    d["k"] = 1;
    bool has = d.ContainsKey("k");
    bool removed = d.Remove("k");
    Console.WriteLine("ok");
}
"#,
            ),
            // dictionary_zero_boxing: user-type key with IEquatable/IHashable
            (
                "dict_zero_boxing_point",
                r#"using Arc;
using Arc.Collections;

class Point : IEquatable<Point>, IHashable<Point> {
    public int X;
    public int Y;

    public Point(int x, int y) {
        X = x;
        Y = y;
    }

    public static bool Equals(Point a, Point b) {
        return a.X == b.X && a.Y == b.Y;
    }

    public static int GetHashCode(Point p) {
        return p.X * 31 + p.Y;
    }
}

void Main() {
    Dictionary<Point, string> dict = new Dictionary<Point, string>();

    Point k1 = new Point(1, 2);
    Point k2 = new Point(3, 4);
    Point k3 = new Point(1, 2);

    dict[k1] = "origin";
    dict[k2] = "other";

    bool hasK1 = dict.ContainsKey(k1);
    string v1 = dict[k1];
    bool hasK2 = dict.ContainsKey(k2);
    string v2 = dict[k2];
    bool hasK3 = dict.ContainsKey(k3);
    string v3 = dict[k3];
    Point missing = new Point(5, 6);
    bool hasMissing = dict.ContainsKey(missing);
    Console.WriteLine("ok");
}
"#,
            ),
        ],
    );
}
