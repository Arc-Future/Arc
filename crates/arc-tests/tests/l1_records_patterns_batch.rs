//! L1 批量：记录类型模式匹配回归集（9 case）。
//!
//! 从 records_patterns_batch_e2e.rs 提取，改为 L1 纯编译期测试。
//! 注意：涉及 Dictionary 的 case 暂排除（依赖 Arc.Collections 运行时支持）。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_records_patterns_batch() {
    assert_compiles_batch(
        "records_patterns",
        &[
            (
                "pos_pattern_m3",
                r#"using Arc;

record PpmPoint(int X, int Y);
record struct PpmVec2(int X, int Y);

void Main() {
    PpmPoint p = new PpmPoint(3, 4);
    if (p is (var x, var y)) {
        if (x != 3 || y != 4) {
            Console.WriteLine("fail is var");
            return;
        }
    } else {
        Console.WriteLine("fail is match");
        return;
    }
    if (!(p is (_, _))) {
        Console.WriteLine("fail is discard");
        return;
    }
    PpmPoint n = null;
    if (n is (var a, var b)) {
        Console.WriteLine("fail null is");
        return;
    }
    int sum = 0;
    switch (p) {
        case (var u, var v):
            sum = u + v;
            break;
        default:
            Console.WriteLine("fail switch");
            return;
    }
    if (sum != 7) {
        Console.WriteLine("fail switch sum");
        return;
    }
    PpmVec2 v = new PpmVec2(5, 6);
    if (v is (var vx, _)) {
        if (vx != 5) {
            Console.WriteLine("fail struct is");
            return;
        }
    } else {
        Console.WriteLine("fail struct match");
        return;
    }
    Console.WriteLine("positional pattern m3 ok");
}
"#,
            ),
            (
                "pos_switch_expr_m4",
                r#"using Arc;

record PsePoint(int X, int Y);
record struct PseVec2(int X, int Y);

int ClassSum(PsePoint p) {
    return p switch {
        (var x, var y) => x + y,
        _ => -1,
    };
}

int NullSum(PsePoint n) {
    return n switch {
        (var a, var b) => a + b,
        _ => 0,
    };
}

int StructX(PseVec2 v) {
    return v switch {
        (var x, _) => x,
    };
}

void Main() {
    PsePoint p = new PsePoint(3, 4);
    if (ClassSum(p) != 7) {
        Console.WriteLine("fail class switch expr");
        return;
    }
    PsePoint n = null;
    if (NullSum(n) != 0) {
        Console.WriteLine("fail null switch expr");
        return;
    }
    PseVec2 v = new PseVec2(5, 6);
    if (StructX(v) != 5) {
        Console.WriteLine("fail struct switch expr");
        return;
    }
    Console.WriteLine("positional switch expr m4 ok");
}
"#,
            ),
            (
                "pos_typed_m5",
                r#"using Arc;

record PtmPoint(int X, int Y);

bool TypedIs(PtmPoint p) {
    if (p is (int x, int y)) {
        return x + y == 7;
    }
    return false;
}

int TypedSwitch(PtmPoint p) {
    return p switch {
        (int a, int b) => a + b,
        _ => -1,
    };
}

void Main() {
    PtmPoint p = new PtmPoint(3, 4);
    if (!TypedIs(p)) {
        Console.WriteLine("fail typed is");
        return;
    }
    if (TypedSwitch(p) != 7) {
        Console.WriteLine("fail typed switch expr");
        return;
    }
    Console.WriteLine("positional typed m5 ok");
}
"#,
            ),
            (
                "pos_const_nested_m6",
                r#"using Arc;

record PcnPoint(int X, int Y);
record PcnSegment(PcnPoint A, PcnPoint B);

bool ConstIs(PcnPoint p) {
    if (p is (3, 4)) {
        return true;
    }
    return false;
}

bool ConstMiss(PcnPoint p) {
    if (p is (1, 2)) {
        return true;
    }
    return false;
}

int ConstSwitch(PcnPoint p) {
    return p switch {
        (3, var y) => y,
        _ => -1,
    };
}

bool NestedIs(PcnSegment s) {
    if (s is ((var x1, var y1), (var x2, var y2))) {
        return x1 + y1 + x2 + y2 == 10;
    }
    return false;
}

void Main() {
    PcnPoint p = new PcnPoint(3, 4);
    if (!ConstIs(p)) {
        Console.WriteLine("fail const is");
        return;
    }
    if (ConstMiss(p)) {
        Console.WriteLine("fail const miss");
        return;
    }
    if (ConstSwitch(p) != 4) {
        Console.WriteLine("fail const switch");
        return;
    }
    PcnSegment s = new PcnSegment(new PcnPoint(1, 2), new PcnPoint(3, 4));
    if (!NestedIs(s)) {
        Console.WriteLine("fail nested is");
        return;
    }
    Console.WriteLine("positional const nested m6 ok");
}
"#,
            ),
            (
                "pos_multi_switch",
                r#"using Arc;

record PmsPoint(int X, int Y);

void Main() {
    PmsPoint p = new PmsPoint(3, 4);
    int a = p switch {
        (var x, var y) => x + y,
        _ => -1,
    };
    int b = p switch {
        (var u, var v) => u * v,
        _ => -1,
    };
    if (a != 7) {
        Console.WriteLine("fail multi a");
        return;
    }
    if (b != 12) {
        Console.WriteLine("fail multi b");
        return;
    }
    Console.WriteLine("positional multi switch ok");
}
"#,
            ),
            (
                "nested_decon_m7",
                r#"using Arc;

record NdmPoint(int X, int Y);
record NdmSegment(NdmPoint A, NdmPoint B);

void Main() {
    NdmSegment s = new NdmSegment(new NdmPoint(1, 2), new NdmPoint(3, 4));
    NdmPoint a = new NdmPoint(0, 0);
    int x = 0;
    int y = 0;
    (a, (x, y)) = s;
    if (a.X != 1 || a.Y != 2) {
        Console.WriteLine("fail a");
        return;
    }
    if (x != 3 || y != 4) {
        Console.WriteLine("fail xy");
        return;
    }
    var (a2, (x2, y2)) = s;
    if (a2.X != 1 || x2 != 3 || y2 != 4) {
        Console.WriteLine("fail var nested");
        return;
    }
    Console.WriteLine("nested deconstruct assign m7 ok");
}
"#,
            ),
            (
                "record_pos_init_m4",
                r#"using Arc;

record RpiPoint(int X, int Y);

void Main() {
    RpiPoint p = new RpiPoint(3, 4);
    if (p.X != 3 || p.Y != 4) {
        Console.WriteLine("fail access");
        return;
    }
    RpiPoint q = p with { X = 10 };
    if (q.X != 10 || q.Y != 4) {
        Console.WriteLine("fail with");
        return;
    }
    RpiPoint same = new RpiPoint(3, 4);
    if (!(p == same)) {
        Console.WriteLine("fail eq");
        return;
    }
    int x;
    int y;
    (x, y) = p;
    if (x != 3 || y != 4) {
        Console.WriteLine("fail decon");
        return;
    }
    Console.WriteLine("record positional init m4 ok");
}
"#,
            ),
        ],
    );
}
