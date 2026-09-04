//! L1 批量：变体类型回归集（10 case）。
//!
//! 从 variants_batch_e2e.rs 提取，改为 L1 纯编译期测试。

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_variants_batch() {
    assert_compiles_batch(
        "variants",
        &[
            (
                "variant_basic",
                r#"using Arc;

variant VbShape { | Circle of int | Square of int }

void Main() {
    VbShape s = VbShape.Circle(5);
    int r = s switch { VbShape.Circle(n) => n, VbShape.Square(n) => n };
    if (r == 5) {
        Console.WriteLine("variant_basic_ok");
    }
}
"#,
            ),
            (
                "variant_generic",
                r#"using Arc;

variant VtOption<T> {
    | Some of T
    | None
}

void Main() {
    var some = VtOption<int>.Some(42);
    int v1 = some switch {
        VtOption<int>.Some(n) => n,
        VtOption<int>.None => 0
    };
    if (v1 != 42) {
        Console.WriteLine("fail:some");
        return;
    }
    var none = VtOption<int>.None;
    int v2 = none switch {
        VtOption<int>.Some(n) => n,
        VtOption<int>.None => -1
    };
    if (v2 != -1) {
        Console.WriteLine("fail:none");
        return;
    }
    Console.WriteLine("variant_generic_ok");
}
"#,
            ),
            (
                "variant_struct_payload",
                r#"using Arc;

struct VsPoint { public int X; public int Y; }

variant VsShape {
    | Circle of int
    | Rect of VsPoint
    | Nil
}

void Main() {
    VsShape c = VsShape.Circle(42);
    int v1 = c switch { VsShape.Circle(n) => n, VsShape.Rect(p) => 0, VsShape.Nil => -1 };
    if (v1 != 42) {
        Console.WriteLine("fail:circle");
        return;
    }
    VsShape nil = VsShape.Nil;
    int v2 = nil switch { VsShape.Circle(n) => 0, VsShape.Rect(p) => -1, VsShape.Nil => 1 };
    if (v2 != 1) {
        Console.WriteLine("fail:nil");
        return;
    }
    Console.WriteLine("variant_struct_ok");
}
"#,
            ),
            (
                "variant_construct_only",
                r#"using Arc;

variant VcoContent { | None | Text of string }

void Main() {
    VcoContent c = VcoContent.Text("Hello");
    Console.WriteLine("variant_construct_ok");
}
"#,
            ),
            (
                "variant_string_switch",
                r#"using Arc;

variant VssContent { | None | Text of string }

void Main() {
    VssContent c = VssContent.Text("Click");
    string r = c switch {
        VssContent.Text(s) => s,
        VssContent.None => "none"
    };
    if (r == "Click") {
        Console.WriteLine("variant_string_ok");
    }
}
"#,
            ),
            (
                "variant_int_switch",
                r#"using Arc;

variant VisContent { | None | Text of string | Num of int }

void Main() {
    VisContent c = VisContent.Num(42);
    int r = c switch {
        VisContent.Num(n) => n,
        _ => 0
    };
    if (r == 42) {
        Console.WriteLine("variant_int_ok");
    }
}
"#,
            ),
            (
                "variant_implicit_let",
                r#"using Arc;

variant VilContent { | None | Text of string | Element of int }

void Main() {
    VilContent c = VilContent.Text("Click");
    string r = c switch {
        VilContent.Text(s) => s,
        _ => "other"
    };
    if (r == "Click") {
        Console.WriteLine("implicit_let_ok");
    }
}
"#,
            ),
            (
                "variant_implicit_param",
                r#"using Arc;

variant VipContent { | None | Text of string | Element of int }

void Consume(VipContent c) { }

void Main() {
    Consume(VipContent.Text("Hello"));
    Console.WriteLine("implicit_param_ok");
}
"#,
            ),
            (
                "variant_implicit_return",
                r#"using Arc;

variant VirContent { | None | Text of string | Element of int }

VirContent Make() {
    return VirContent.Text("OK");
}

void Main() {
    VirContent c = Make();
    string r = c switch {
        VirContent.Text(s) => s,
        _ => "other"
    };
    if (r == "OK") {
        Console.WriteLine("implicit_return_ok");
    }
}
"#,
            ),
            (
                "variant_list",
                r#"using Arc;

variant VlValue {
    | Int of int
    | Nil
}

void Main() {
    VlValue v = VlValue.Int(42);
    int r = v switch {
        VlValue.Int(n) => n,
        VlValue.Nil => 0
    };
    if (r == 42) {
        Console.WriteLine("variant_list_ok");
    }
}
"#,
            ),
        ],
    );
}
