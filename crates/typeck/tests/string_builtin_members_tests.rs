//! RFC 071 三硬 / ArmlDemo 推进：`string`（内置 `TypeId::String`，不注册
//! type table）的实例成员/方法解析回归测试。
//!
//! 修复前 `s.ToString()` 落入 registry 方法解析（`resolve_method` →
//! `collect_method_overloads("string", …)`）→ `OopError::UndefinedType("string")`，
//! 因为 string 是 `TypeId` 枚举变体、不在 `registry.types` 表中。修复在
//! primitives 拦截路径（`check_expr.rs` `TypeId::Int | … | TypeId::String`）
//! 补上 `TypeId::String`，与 int/long 等同类成员方法共用同一拦截。
//!
//! 通过 parse → hir → typeck 完整管线驱动，最小 class 覆盖：
//! - `s.ToString()` / `s.Length` / `s.Equals(...)` 不再报 `undefined type string`

use hir::HirBuilder;
use parse::Parser;
use typeck::TypeChecker;

/// 驱动 parse → hir → typeck 管线，返回 `Result`（Err 即编译错误集合）。
fn check_src(src: &str) -> Result<(), Vec<typeck::TypeError>> {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module).map(|_| ())
}

#[test]
fn string_instance_methods_typecheck() {
    let result = check_src(
        r#"
class App {
    int Main() {
        string s = "x";
        string t = s.ToString();
        int len = s.Length;
        return len;
    }
}
"#,
    );
    assert!(
        result.is_ok(),
        "`s.ToString()` / `s.Length` 应通过 typeck（string 是内置 TypeId，不走 registry），实际: {:?}",
        result.err().map(|e| format!("{e:?}"))
    );
}

#[test]
fn string_equals_compare_to_typecheck() {
    let result = check_src(
        r#"
class App {
    bool Main() {
        string a = "x";
        string b = "y";
        bool eq = a.Equals(b);
        int cmp = a.CompareTo(b);
        int h = a.GetHashCode();
        return eq;
    }
}
"#,
    );
    assert!(
        result.is_ok(),
        "`string` 的 Equals/CompareTo/GetHashCode 应通过 typeck，实际: {:?}",
        result.err().map(|e| format!("{e:?}"))
    );
}

#[test]
fn primitive_int_tostring_sanity() {
    // 对照组：int 基元方法拦截不受影响。
    let result = check_src(
        r#"
class App {
    int Main() {
        int i = 1;
        string t = i.ToString();
        int h = i.GetHashCode();
        return h;
    }
}
"#,
    );
    assert!(
        result.is_ok(),
        "`int` 的 ToString/GetHashCode 拦截应保持可用，实际: {:?}",
        result.err().map(|e| format!("{e:?}"))
    );
}
