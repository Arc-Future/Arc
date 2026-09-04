//! RFC 037 M1 单元测试：partial class 合并语义。
//!
//! 覆盖：
//! - 跨 namespace 声明的 partial class 合并（field/property/method/ctor 累加）
//! - 重复字段检测（报错）
//! - 重复方法（同签名）检测（报错）
//! - vis 不匹配（报错）
//! - 单声明 partial（警告：未与其它声明合并）
//! - 非 partial 同名 class 仍按重复定义报错（保持原行为）

use hir::HirBuilder;
use parse::Parser;
use typeck::*;

fn check_module(src: &str) -> Result<Vec<TypedFn>, Vec<TypeError>> {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
}

/// 合法 partial class：两个声明合并，构造函数与业务方法分布在两侧。
#[test]
fn partial_class_merge_ok() {
    let src = r#"
namespace App;
partial class Counter {
    int _count = 0;
    public int Count { get { return _count; } }
    public void Increment() { _count = _count + 1; }
}
partial class Counter {
    public Counter() { }
    public void Decrement() { _count = _count - 1; }
}
"#;
    assert!(
        check_module(src).is_ok(),
        "expected partial merge to succeed"
    );
}

/// 重复字段在两个 partial 声明中 → 报错。
#[test]
fn partial_class_duplicate_field_rejected() {
    let src = r#"
namespace App;
partial class Holder {
    int x;
}
partial class Holder {
    int x;
}
"#;
    let r = check_module(src);
    assert!(r.is_err(), "expected duplicate field error");
    let errs = r.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.to_string().contains("duplicate field") && e.to_string().contains("Holder")),
        "expected duplicate field error, got: {errs:?}"
    );
}

/// 重复方法（同签名）在两个 partial 声明中 → 报错。
/// （不同签名视为合法重载，不在此测试覆盖范围）
#[test]
fn partial_class_duplicate_method_rejected() {
    let src = r#"
namespace App;
partial class Holder {
    public void Do() { }
}
partial class Holder {
    public void Do() { }
}
"#;
    let r = check_module(src);
    assert!(r.is_err(), "expected duplicate method error");
    let errs = r.unwrap_err();
    assert!(
        errs.iter().any(|e| e.to_string().contains("duplicate method") && e.to_string().contains("Holder")),
        "expected duplicate method error, got: {errs:?}"
    );
}

/// vis 不匹配 → 报错。
#[test]
fn partial_class_visibility_mismatch_rejected() {
    let src = r#"
namespace App;
public partial class Holder { }
private partial class Holder { }
"#;
    let r = check_module(src);
    assert!(r.is_err(), "expected visibility mismatch error");
    let errs = r.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.to_string().contains("visibility mismatch")
                && e.to_string().contains("Holder")),
        "expected visibility mismatch error, got: {errs:?}"
    );
}

/// 单声明 partial：应产生"only has one declaration"错误（提示用户移除 partial 修饰符）。
#[test]
fn partial_class_single_declaration_warns() {
    let src = r#"
namespace App;
partial class Lonely { }
"#;
    let r = check_module(src);
    assert!(r.is_err(), "expected single-declaration error");
    let errs = r.unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.to_string().contains("only has one declaration")
                && e.to_string().contains("Lonely")),
        "expected single-declaration error, got: {errs:?}"
    );
}

/// 非 partial 同名 class 仍按重复定义报错（HIR 层 DuplicateDefinition）。
#[test]
fn non_partial_same_name_rejected() {
    let src = r#"
namespace App;
class Dup { }
class Dup { }
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    // HIR 层应直接报 DuplicateDefinition
    assert!(hir.lower_program(&program).is_err());
}

/// 合并后的实例字段初始化器应注入合成默认构造函数 body。
#[test]
fn partial_class_field_inits_in_default_ctor() {
    let src = r#"
namespace App;
partial class Counter {
    int _count = 0;
    public int Count { get { return _count; } }
}
partial class Counter {
    int _maxValue = 100;
    public bool IsFull { get { return _count >= _maxValue; } }
}
"#;
    let fns = check_module(src).expect("partial merge with field inits");
    let ctor = fns
        .iter()
        .find(|f| f.name.as_str() == "__ctor::Counter")
        .expect("expected synthesized __ctor::Counter with field inits");
    let body = ctor.body.as_ref().expect("ctor body");
    assert!(
        body.stmts.len() >= 2,
        "expected at least two field-init assigns, got {} stmts",
        body.stmts.len()
    );
}
