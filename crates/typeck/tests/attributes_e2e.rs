//! RFC 028 M1 端到端测试：属性收集与查询。
//!
//! 通过 parse → hir → typeck 完整管线驱动属性收集，验证：
//! - 合法属性被正确注册到 `AttributeTable`
//! - `class_def_id` / `member_def_id` 反查表填充正确
//! - `table_types()` / `find_attr()` / `has_attr()` 查询 API 行为
//! - 校验失败时 `check_module` 返回错误（目标不合法 / 参数类型错误 / 参数过多）
//! - 无效属性不入表（避免下游基于错误数据构建映射）

use hir::HirBuilder;
use parse::Parser;
use typeck::{ResolvedArg, TypeChecker, TypeError};

/// 驱动 parse → hir → typeck 管线，返回 `TypeChecker`（即使有错误也返回，
/// 便于检查 `attribute_table` 状态）。仅用于失败用例需要 `result` 时改用
/// `check_src_result`。
fn check_src(src: &str) -> TypeChecker {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let _ = tc.check_module(&module);
    tc
}

/// 同 `check_src`，但返回 `check_module` 的 `Result` 以便断言错误。
fn check_src_result(src: &str) -> (TypeChecker, Result<Vec<typeck::TypedFn>, Vec<TypeError>>) {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    (tc, result)
}

/// 断言 `errors` 中包含匹配 `pred` 的 `TypeError::Oop` 条目。
fn has_oop_error<F>(errors: &[TypeError], pred: F) -> bool
where
    F: Fn(&str) -> bool,
{
    errors.iter().any(|e| match e {
        TypeError::Oop(msg) => pred(msg),
        _ => false,
    })
}

// ============================================================================
// 合法用法 e2e：验证属性被正确收集到 attribute_table
// ============================================================================

#[test]
fn class_with_table_and_column_attrs_collected() {
    let src = r#"
[Table("users")]
class User {
    [Column("id")]
    [Key]
    public int Id;

    [Column("name")]
    [Required]
    public string Name;

    [Column("age")]
    public int Age;
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    // 类自身：[Table("users")]
    let user_def_id = tc.class_def_id("User").expect("User DefId must exist");
    assert!(table.has_attr(user_def_id, "Table"));
    let table_attr = table.find_attr(user_def_id, "Table").expect("Table attr");
    let table_name = table_attr.args[0].as_string().expect("table name string");
    assert_eq!(table_name, "users");

    // table_types() 返回 User
    let tables = table.table_types();
    assert_eq!(tables.len(), 1, "exactly one [Table]-marked type");
    assert_eq!(tables[0].0, user_def_id);
    assert_eq!(tables[0].1, Some("users".to_string()));

    // Id 字段：[Column("id")] + [Key]
    let id_def_id = tc.member_def_id("User", "Id").expect("Id DefId");
    assert!(table.has_attr(id_def_id, "Column"));
    assert!(table.has_attr(id_def_id, "Key"));
    let id_attrs = table.get_attrs(id_def_id);
    assert_eq!(id_attrs.len(), 2, "Id has two attributes");
    let column_attr = table.find_attr(id_def_id, "Column").unwrap();
    let col_name = column_attr.args[0].as_string().unwrap();
    assert_eq!(col_name, "id");

    // Name 字段：[Column("name")] + [Required]
    let name_def_id = tc.member_def_id("User", "Name").expect("Name DefId");
    assert!(table.has_attr(name_def_id, "Column"));
    assert!(table.has_attr(name_def_id, "Required"));
    assert!(!table.has_attr(name_def_id, "Key"));

    // Age 字段：仅 [Column("age")]
    let age_def_id = tc.member_def_id("User", "Age").expect("Age DefId");
    assert!(table.has_attr(age_def_id, "Column"));
    assert!(!table.has_attr(age_def_id, "Key"));
    assert!(!table.has_attr(age_def_id, "Required"));
}

#[test]
fn struct_with_table_attr_no_args() {
    let src = r#"
[Table]
struct LogEntry {
    [Column]
    string Message;
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    let log_def_id = tc.class_def_id("LogEntry").expect("LogEntry DefId");
    assert!(table.has_attr(log_def_id, "Table"));
    let table_attr = table.find_attr(log_def_id, "Table").unwrap();
    assert!(
        table_attr.args.is_empty(),
        "[Table] no-arg form has empty args"
    );

    // table_types() 中 LogEntry 的 table name 为 None（无参 → ORM 调用方回退到类型名）
    let tables = table.table_types();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].0, log_def_id);
    assert_eq!(tables[0].1, None);

    // Message 字段：[Column] 无参
    let msg_def_id = tc
        .member_def_id("LogEntry", "Message")
        .expect("Message DefId");
    let column_attr = table.find_attr(msg_def_id, "Column").unwrap();
    assert!(column_attr.args.is_empty());
}

#[test]
fn property_with_maxlength_attr() {
    let src = r#"
class Product {
    [MaxLength(100)]
    public string Name { get; set; }
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    let prop_def_id = tc
        .member_def_id("Product", "Name")
        .expect("Name property DefId");
    let attr = table
        .find_attr(prop_def_id, "MaxLength")
        .expect("MaxLength attr");
    let length = attr.args[0].as_int().expect("int length");
    assert_eq!(length, 100);
}

#[test]
fn static_class_attr_collected() {
    let src = r#"
[Table("config")]
static class Config {
    static string Get(string key) { return key; }
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    let config_def_id = tc.class_def_id("Config").expect("Config DefId");
    assert!(table.has_attr(config_def_id, "Table"));
    let table_attr = table.find_attr(config_def_id, "Table").unwrap();
    assert_eq!(table_attr.args[0].as_string(), Some("config"));
}

#[test]
fn interface_attr_collected() {
    let src = r#"
[Service]
interface IFoo {
    [Required]
    string Name { get; set; }
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    // [Service] 是非内置属性，validate_builtin 直接 Ok(())，会注册到 attribute_table
    let iface_def_id = tc.class_def_id("IFoo").expect("IFoo DefId");
    assert!(table.has_attr(iface_def_id, "Service"));

    // property 上的 [Required] 内置属性
    let prop_def_id = tc
        .member_def_id("IFoo", "Name")
        .expect("Name property DefId");
    assert!(table.has_attr(prop_def_id, "Required"));
}

// ============================================================================
// 校验失败 e2e：验证错误被报告且无效属性不入表
// ============================================================================

#[test]
fn target_mismatch_table_on_method_reports_error() {
    let src = r#"
class Foo {
    [Table("bad")]
    void Bar() {}
}
"#;
    let (tc, result) = check_src_result(src);
    assert!(result.is_err(), "must error on [Table] on method");
    let errors = result.unwrap_err();
    assert!(has_oop_error(&errors, |m| {
        m.contains("target mismatch") && m.contains("Table")
    }));

    // 无效属性不入表
    let bar_def_id = tc.member_def_id("Foo", "Bar");
    if let Some(bar_def_id) = bar_def_id {
        assert!(
            !tc.attribute_table().has_attr(bar_def_id, "Table"),
            "invalid attr must not be registered"
        );
    }
}

#[test]
fn arg_type_mismatch_maxlength_with_string_reports_error() {
    let src = r#"
class Foo {
    [MaxLength("abc")]
    int x;
}
"#;
    let (tc, result) = check_src_result(src);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(has_oop_error(&errors, |m| {
        m.contains("argument mismatch") && m.contains("MaxLength")
    }));

    // 无效属性不入表
    if let Some(x_def_id) = tc.member_def_id("Foo", "x") {
        assert!(!tc.attribute_table().has_attr(x_def_id, "MaxLength"));
    }
}

#[test]
fn extra_args_on_key_reports_error() {
    let src = r#"
class Foo {
    [Key("extra")]
    int x;
}
"#;
    let (_tc, result) = check_src_result(src);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(has_oop_error(&errors, |m| {
        m.contains("argument mismatch") && m.contains("Key")
    }));
}

#[test]
fn column_on_class_target_reports_error() {
    let src = r#"
[Column("bad")]
class Foo {}
"#;
    let (_tc, result) = check_src_result(src);
    assert!(result.is_err(), "[Column] on class must error");
    let errors = result.unwrap_err();
    assert!(has_oop_error(&errors, |m| {
        m.contains("target mismatch") && m.contains("Column")
    }));
}

// ============================================================================
// 查询 API 行为验证
// ============================================================================

#[test]
fn class_def_id_unknown_returns_none() {
    let src = r#"
class Foo {}
"#;
    let tc = check_src(src);
    assert!(tc.class_def_id("Nonexistent").is_none());
}

#[test]
fn member_def_id_unknown_returns_none() {
    let src = r#"
class Foo {
    int x;
}
"#;
    let tc = check_src(src);
    assert!(tc.member_def_id("Foo", "nonexistent").is_none());
    assert!(tc.member_def_id("Nonexistent", "x").is_none());
}

#[test]
fn find_attr_unknown_returns_none() {
    let src = r#"
[Table("t")]
class Foo {}
"#;
    let tc = check_src(src);
    let foo_def_id = tc.class_def_id("Foo").unwrap();
    let table = tc.attribute_table();
    assert!(table.find_attr(foo_def_id, "NonexistentAttr").is_none());
    // 未知 DefId
    assert!(table
        .find_attr(typeck::BUILTIN_ATTR_TYPE, "Table")
        .is_none());
}

#[test]
fn resolved_arg_accessors_work_in_e2e() {
    let src = r#"
[Table("users")]
class User {
    [MaxLength(50)]
    public string Name;
}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();

    let user_def_id = tc.class_def_id("User").unwrap();
    let table_attr = table.find_attr(user_def_id, "Table").unwrap();
    // String 变体
    assert_eq!(table_attr.args[0].as_string(), Some("users"));
    assert_eq!(table_attr.args[0].as_int(), None);

    let name_def_id = tc.member_def_id("User", "Name").unwrap();
    let max_attr = table.find_attr(name_def_id, "MaxLength").unwrap();
    // Int 变体
    assert_eq!(max_attr.args[0].as_int(), Some(50));
    assert_eq!(max_attr.args[0].as_string(), None);
    // ResolvedArg 类型直接匹配
    assert!(matches!(max_attr.args[0], ResolvedArg::Int(50)));
}

#[test]
fn non_builtin_attr_passes_through_and_registers() {
    let src = r#"
[Foo]
class Bar {}
"#;
    let tc = check_src(src);
    let table = tc.attribute_table();
    let bar_def_id = tc.class_def_id("Bar").unwrap();
    // [Foo] 不是内置属性，validate_builtin 直接 Ok(())，会注册
    assert!(table.has_attr(bar_def_id, "Foo"));
}
