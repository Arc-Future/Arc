//! RFC 061 M2 unit tests: static field typeck semantics.
//!
//! Covers:
//! - Static method accessing static field (legal)
//! - Static method accessing instance field (error)
//! - Static method using `this` (error)
//! - Instance method accessing static field (legal)
//! - `TypedFn.is_static` flag correctness
//! - `TypedFn.class_fields` filtering by static/instance

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

/// Helper: extract error message strings from check_module result.
fn error_messages(src: &str) -> Vec<String> {
    match check_module(src) {
        Ok(_) => vec![],
        Err(errs) => errs.iter().map(|e| e.to_string()).collect(),
    }
}

/// Helper: find a TypedFn by name substring.
fn find_typed_fn<'a>(fns: &'a [TypedFn], name_substring: &str) -> Option<&'a TypedFn> {
    fns.iter()
        .find(|f| f.name.as_str().contains(name_substring))
}

// ============================================================================
// Legal: static method accesses static field
// ============================================================================

/// Static method reads static field - legal, no typeck error.
#[test]
fn static_method_reads_static_field_ok() {
    let src = r#"
class Counter {
    private static int _count = 0;
    public static int GetCount() {
        return _count;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "static method reading static field should be allowed, got: {:?}",
        result.err()
    );
}

/// Static method writes static field - legal.
#[test]
fn static_method_writes_static_field_ok() {
    let src = r#"
class Counter {
    private static int _count = 0;
    public static void Increment() {
        _count = _count + 1;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "static method writing static field should be allowed, got: {:?}",
        result.err()
    );
}

/// Static method reads `static readonly` field - legal (read only).
#[test]
fn static_method_reads_static_readonly_ok() {
    let src = r#"
class Config {
    private static readonly int _max = 100;
    public static int GetMax() {
        return _max;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "static method reading static readonly field should be allowed, got: {:?}",
        result.err()
    );
}

/// Static method reads `const` field - legal (const implies static).
#[test]
fn static_method_reads_const_field_ok() {
    let src = r#"
class MathLib {
    public const int Pi = 3;
    public static int GetPi() {
        return Pi;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "static method reading const field should be allowed, got: {:?}",
        result.err()
    );
}

// ============================================================================
// Illegal: static method accesses instance field
// ============================================================================

/// Static method reads instance field - error.
#[test]
fn static_method_reads_instance_field_rejected() {
    let src = r#"
class Bad {
    private int _instance = 0;
    public static int Get() {
        return _instance;
    }
}
"#;
    let msgs = error_messages(src);
    assert!(
        msgs.iter()
            .any(|m| m.contains("instance field") && m.contains("_instance")),
        "expected error about instance field `_instance`, got: {msgs:?}"
    );
}

/// Static method writes instance field - error.
#[test]
fn static_method_writes_instance_field_rejected() {
    let src = r#"
class Bad {
    private int _instance = 0;
    public static void Set() {
        _instance = 42;
    }
}
"#;
    let msgs = error_messages(src);
    assert!(
        msgs.iter()
            .any(|m| m.contains("instance field") && m.contains("_instance")),
        "expected error about instance field `_instance`, got: {msgs:?}"
    );
}

/// Static method uses `this` - error.
#[test]
fn static_method_uses_this_rejected() {
    let src = r#"
class Bad {
    private int _instance = 0;
    public static int Get() {
        return this._instance;
    }
}
"#;
    let msgs = error_messages(src);
    assert!(
        msgs.iter()
            .any(|m| m.contains("`this`") && m.contains("static")),
        "expected error about `this` in static method, got: {msgs:?}"
    );
}

// ============================================================================
// Instance method accesses static field (legal)
// ============================================================================

/// Instance method reads static field - legal (static fields visible to all methods).
#[test]
fn instance_method_reads_static_field_ok() {
    let src = r#"
class Counter {
    private static int _total = 0;
    private int _local = 0;
    public int Combined() {
        return _total + _local;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "instance method reading static field should be allowed, got: {:?}",
        result.err()
    );
}

/// Instance method writes static field - legal.
#[test]
fn instance_method_writes_static_field_ok() {
    let src = r#"
class Counter {
    private static int _total = 0;
    public void Bump() {
        _total = _total + 1;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "instance method writing static field should be allowed, got: {:?}",
        result.err()
    );
}

// ============================================================================
// TypedFn.is_static flag and class_fields filtering
// ============================================================================

/// `TypedFn.is_static` flag: static method is true, instance method is false.
#[test]
fn typed_fn_is_static_flag_correct() {
    let src = r#"
class Counter {
    private static int _count = 0;
    private int _instance = 0;
    public static int StaticMethod() { return _count; }
    public int InstanceMethod() { return _instance; }
}
"#;
    let fns = check_module(src).expect("expected no typeck errors");
    let static_fn =
        find_typed_fn(&fns, "StaticMethod").expect("StaticMethod typed fn should exist");
    assert!(
        static_fn.is_static,
        "StaticMethod should have is_static == true"
    );
    let instance_fn =
        find_typed_fn(&fns, "InstanceMethod").expect("InstanceMethod typed fn should exist");
    assert!(
        !instance_fn.is_static,
        "InstanceMethod should have is_static == false"
    );
}

/// Static method `class_fields` contains only static fields (incl. const), not instance fields.
#[test]
fn typed_fn_class_fields_filtered_for_static_method() {
    let src = r#"
class Counter {
    private static int _count = 0;
    private static readonly int _max = 100;
    public const int K = 42;
    private int _instance = 0;
    public static int Get() { return _count; }
}
"#;
    let fns = check_module(src).expect("expected no typeck errors");
    let static_fn = find_typed_fn(&fns, "Get").expect("Get typed fn should exist");
    assert!(static_fn.is_static, "Get should be static");
    assert!(
        static_fn
            .class_fields
            .iter()
            .any(|f| f.as_str() == "_count"),
        "class_fields should contain static field `_count`, got: {:?}",
        static_fn.class_fields
    );
    assert!(
        static_fn.class_fields.iter().any(|f| f.as_str() == "_max"),
        "class_fields should contain static readonly field `_max`, got: {:?}",
        static_fn.class_fields
    );
    assert!(
        static_fn.class_fields.iter().any(|f| f.as_str() == "K"),
        "class_fields should contain const field `K`, got: {:?}",
        static_fn.class_fields
    );
    assert!(
        !static_fn
            .class_fields
            .iter()
            .any(|f| f.as_str() == "_instance"),
        "class_fields should NOT contain instance field `_instance`, got: {:?}",
        static_fn.class_fields
    );
}

/// Instance method `class_fields` contains all non-const fields (static + instance).
#[test]
fn typed_fn_class_fields_contains_all_for_instance_method() {
    let src = r#"
class Counter {
    private static int _count = 0;
    private int _instance = 0;
    public const int K = 42;
    public int Combined() { return _count + _instance; }
}
"#;
    let fns = check_module(src).expect("expected no typeck errors");
    let instance_fn = find_typed_fn(&fns, "Combined").expect("Combined typed fn should exist");
    assert!(!instance_fn.is_static, "Combined should be instance method");
    assert!(
        instance_fn
            .class_fields
            .iter()
            .any(|f| f.as_str() == "_count"),
        "class_fields should contain static field `_count` for instance method, got: {:?}",
        instance_fn.class_fields
    );
    assert!(
        instance_fn
            .class_fields
            .iter()
            .any(|f| f.as_str() == "_instance"),
        "class_fields should contain instance field `_instance` for instance method, got: {:?}",
        instance_fn.class_fields
    );
    assert!(
        !instance_fn.class_fields.iter().any(|f| f.as_str() == "K"),
        "class_fields should NOT contain const field `K` (const uses const_values), got: {:?}",
        instance_fn.class_fields
    );
}

/// Constructor `is_static` is always false, `class_fields` contains all non-const fields.
#[test]
fn typed_fn_ctor_is_not_static() {
    let src = r#"
class Counter {
    private static int _count = 0;
    private int _instance = 0;
    public Counter() { _instance = 1; }
}
"#;
    let fns = check_module(src).expect("expected no typeck errors");
    let ctor_fn =
        find_typed_fn(&fns, "__ctor::Counter").expect("Counter ctor typed fn should exist");
    assert!(!ctor_fn.is_static, "ctor should have is_static == false");
    assert!(
        ctor_fn
            .class_fields
            .iter()
            .any(|f| f.as_str() == "_instance"),
        "ctor class_fields should contain instance field, got: {:?}",
        ctor_fn.class_fields
    );
}

// ============================================================================
// Local variable shadows instance field in static method
// ============================================================================

/// Static method has local variable shadowing instance field name - no error.
#[test]
fn static_method_local_shadows_instance_field_ok() {
    let src = r#"
class Counter {
    private int _val = 0;
    public static int Process() {
        int _val = 42;
        return _val;
    }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "local variable shadowing instance field in static method should be allowed, got: {:?}",
        result.err()
    );
}

// ============================================================================
// Static property getter/setter
// ============================================================================

/// Static property getter reads static field - legal.
#[test]
fn static_property_getter_reads_static_field_ok() {
    let src = r#"
class Config {
    private static int _value = 42;
    public static int Value { get { return _value; } }
}
"#;
    let result = check_module(src);
    assert!(
        result.is_ok(),
        "static property getter reading static field should be allowed, got: {:?}",
        result.err()
    );
}

/// Static property getter reads instance field - error.
#[test]
fn static_property_getter_reads_instance_field_rejected() {
    let src = r#"
class Bad {
    private int _instance = 0;
    public static int Value { get { return _instance; } }
}
"#;
    let msgs = error_messages(src);
    assert!(
        msgs.iter()
            .any(|m| m.contains("instance field") && m.contains("_instance")),
        "expected error about instance field in static property getter, got: {msgs:?}"
    );
}
