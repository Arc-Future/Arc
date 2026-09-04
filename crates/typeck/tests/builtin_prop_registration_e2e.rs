//! 注册一致性审计：`[Builtin]` 自动属性必须注册为 **custom 访问器**
//! （`get_X` 入方法表、**不生成 backing field**）。
//!
//! 防回归网：属性访问器形态判定已收敛为单一事实源
//! `registry::property_has_custom_accessors`（registry / check_class /
//! check_generics 全部调用点共用）。本测试覆盖三条注册路径：
//!   1. 非泛型类（`register_class`）
//!   2. 泛型单态化类（`register_monomorphized_class`——历史漏判点：
//!      `List<T>.Count` 曾注册为 backing field → MIR FieldGet 读 `RtList*`
//!      垃圾偏移 → 运行期静默错乱）
//!   3. struct（`register_item`——历史缺口：曾无 `[Builtin]` 分支）
//!
//! 任一路径若把 `[Builtin]` 自动属性注册为字段 / 漏注册 getter，此处即红。

use hir::HirBuilder;
use parse::Parser;
use typeck::TypeChecker;

/// 驱动 parse → hir → typeck 管线，返回 `TypeChecker`。
fn check_src(src: &str) -> TypeChecker {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let _ = tc.check_module(&module);
    tc
}

/// 断言 `[Builtin]` 自动属性注册为 custom 访问器：`get_X` 入方法表、无 backing field。
fn assert_custom_accessor(tc: &TypeChecker, class: &str, prop: &str) {
    let nom = tc
        .registry()
        .types
        .get(class)
        .unwrap_or_else(|| panic!("type `{class}` not registered"));
    let getter = format!("get_{prop}");
    assert!(
        nom.methods.contains_key(getter.as_str()),
        "`{class}.{prop}` 未注册 getter `{getter}`（注册路径漏判 [Builtin] 自动属性）"
    );
    assert!(
        !nom.fields.contains_key(prop),
        "`{class}.{prop}` 被注册为 backing field（应走 custom 访问器、无字段）"
    );
}

/// 对照：普通自动属性保持 backing field、不注册 getter。
fn assert_plain_auto_prop(tc: &TypeChecker, class: &str, prop: &str) {
    let nom = tc
        .registry()
        .types
        .get(class)
        .unwrap_or_else(|| panic!("type `{class}` not registered"));
    let getter = format!("get_{prop}");
    assert!(
        nom.fields.contains_key(prop),
        "`{class}.{prop}` 应保持 backing field"
    );
    assert!(
        !nom.methods.contains_key(getter.as_str()),
        "`{class}.{prop}` 不应注册 getter（普通自动属性）"
    );
}

/// 最小属性体系 preamble（对齐 macro_e2e 的 STD_PREAMBLE，补 BuiltinAttribute）。
const STD_PREAMBLE: &str = r#"
class Attribute {
    public Attribute() {}
}

class AttributeTargets {
    public const int Class = 1;
    public const int Struct = 2;
    public const int Method = 16;
    public const int Property = 32;
    public const int All = 511;
}

class AttributeUsageAttribute : Attribute {
    public AttributeUsageAttribute(int validOn) {}
}

class BuiltinAttribute : Attribute {
    public BuiltinAttribute() {}
    public string ABI { get; set; }
}
"#;

#[test]
fn builtin_auto_prop_non_generic_class_registers_custom_accessor() {
    let src = format!(
        r#"{preamble}
class Probe {{
    [Builtin(ABI = "rt_probe_alive")]
    public bool Alive {{ get; }}

    [Builtin(ABI = "rt_probe_id")]
    public static int Id {{ get; }}

    // 对照：无 [Builtin] 的自动属性保持 backing field
    public string Label {{ get; }}

    [Builtin(ABI = "rt_probe_rw")]
    public int ReadWrite {{ get; set; }}
}}
"#,
        preamble = STD_PREAMBLE
    );
    let tc = check_src(&src);
    assert_custom_accessor(&tc, "Probe", "Alive");
    assert_custom_accessor(&tc, "Probe", "Id");
    assert_custom_accessor(&tc, "Probe", "ReadWrite");
    assert_plain_auto_prop(&tc, "Probe", "Label");
}

#[test]
fn builtin_auto_prop_generic_mono_registers_custom_accessor() {
    let src = format!(
        r#"{preamble}
class Box<T> {{
    [Builtin(ABI = "rt_box_count")]
    public int Count {{ get; }}

    [Builtin(ABI = "rt_box_first")]
    public T First {{ get; }}

    // 对照：无 [Builtin] 的自动属性保持 backing field
    public string Tag {{ get; }}
}}

void Use() {{
    Box<int> b = new Box<int>();
    int c = b.Count;
    int f = b.First;
    string t = b.Tag;
}}
"#,
        preamble = STD_PREAMBLE
    );
    let tc = check_src(&src);
    // 单态化后必须与 register_class 语义一致（历史漏判点）。
    assert_custom_accessor(&tc, "Box_int", "Count");
    assert_custom_accessor(&tc, "Box_int", "First");
    assert_plain_auto_prop(&tc, "Box_int", "Tag");
}

#[test]
fn builtin_auto_prop_struct_registers_custom_accessor() {
    let src = format!(
        r#"{preamble}
struct Probe {{
    [Builtin(ABI = "rt_probe_len")]
    public int Len {{ get; }}

    // 对照：无 [Builtin] 的自动属性保持 backing field
    public string Label {{ get; }}
}}
"#,
        preamble = STD_PREAMBLE
    );
    let tc = check_src(&src);
    // struct 注册路径（register_item）与 class 语义一致。
    assert_custom_accessor(&tc, "Probe", "Len");
    assert_plain_auto_prop(&tc, "Probe", "Label");
}
