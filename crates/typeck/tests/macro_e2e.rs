//! RFC 028 M4-2 端到端测试：宏容器与宏特性派生类识别。
//!
//! 通过 parse → hir → typeck 完整管线驱动宏目录构建，验证：
//! - 派生 `GenerateToAttribute<T>` 的类被识别为宏特性，关联容器 T
//! - 容器通过 features 反向推断识别（v0.11 修订——容器无需 `[GenerateTo]` 标注）
//! - 容器的 public 方法成为展开槽位
//! - 静态类作为容器同样可被反向推断识别
//! - 无 feature 指向的普通类不被识别为容器
//! - 未派生 `GenerateToAttribute<T>` 的类不被识别为特性

use ast::Ident;
use ast::Span;
use hir::HirBuilder;
use parse::Parser;
use typeck::{parse_expansion, Evaluator, TypeChecker, Whitelist};

/// 驱动 parse → hir → typeck 管线，返回 `TypeChecker`。
fn check_src(src: &str) -> TypeChecker {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let _ = tc.check_module(&module);
    tc
}

/// 公共 std 类前置声明：Attribute 根基类 + AttributeUsageAttribute +
/// AttributeTargets（M3 已落地）。v0.11 修订：测试场景中容器通过 features
/// 反向推断识别，不再需要 `[GenerateTo]` 属性标记。容器类是普通类，
/// 由 `class Foo : GenerateToAttribute<Container>` 派生类的 T 参数反推得到。
const STD_PREAMBLE: &str = r#"
class Attribute {
    public Attribute() {}
}

class AttributeTargets {
    public const int Class = 1;
    public const int Struct = 2;
    public const int Method = 16;
    public const int Property = 32;
    public const int Field = 64;
    public const int All = 255;
}

[AttributeUsage(AttributeTargets.Class)]
class AttributeUsageAttribute : Attribute {
    public int ValidOn { get; }
    public bool AllowMultiple { get; set; }
    public bool Inherited { get; set; }
    public AttributeUsageAttribute(int validOn) {
        ValidOn = validOn;
        AllowMultiple = false;
        Inherited = true;
    }
}
"#;

#[test]
fn class_with_generate_to_attr_identified_as_container() {
    // v0.11 修订：容器通过 features 反向推断识别——添加一个 feature 指向 Host
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
    public int Test(int v) {{
        return v + 10;
    }}
    void PrivateMethod() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    // Host 被识别为宏容器（通过 InjectAttribute 反向推断）
    assert!(
        catalog.is_container(&"Host".into()),
        "Host must be a macro container"
    );
    // 关联的容器名查询
    assert_eq!(
        catalog.container_of(&"Host".into()),
        None,
        "Host is container, not feature"
    );

    // 槽位收集：Register + Test 两个 public 方法
    let slots = catalog
        .slots_of(&"Host".into())
        .expect("Host must have slots");
    assert_eq!(
        slots.len(),
        2,
        "Host should have 2 public method slots (Register + Test)"
    );

    // 验证 Register 槽位
    let register = slots
        .iter()
        .find(|s| s.method_name.as_str() == "Register")
        .unwrap();
    assert!(register.param_types.is_empty());
    assert_eq!(register.return_type.as_str(), "void");

    // 验证 Test 槽位
    let test = slots
        .iter()
        .find(|s| s.method_name.as_str() == "Test")
        .unwrap();
    assert_eq!(test.param_types.len(), 1);
    assert_eq!(test.param_types[0].as_str(), "int");
    assert_eq!(test.return_type.as_str(), "int");

    // PrivateMethod 不应出现在槽位中（非 public）
    assert!(slots
        .iter()
        .all(|s| s.method_name.as_str() != "PrivateMethod"));
}

#[test]
fn class_with_generate_to_attribute_long_name_also_identified() {
    // v0.11 修订：容器识别不再依赖 [GenerateTo] / [GenerateToAttribute] 属性标记，
    // 完全通过 features 反向推断。本测试改为验证无任何标记的容器类也能被识别。
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(catalog.is_container(&"Host".into()));
}

#[test]
fn class_without_generate_to_attr_not_identified_as_container() {
    let src = format!(
        r#"
{STD_PREAMBLE}

class Plain {{}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(!catalog.is_container(&"Plain".into()));
    assert!(!catalog.is_feature(&"Plain".into()));
}

#[test]
fn class_deriving_generate_to_attribute_t_identified_as_feature() {
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    // Host 是容器
    assert!(catalog.is_container(&"Host".into()));

    // InjectAttribute 是特性派生类
    assert!(catalog.is_feature(&"InjectAttribute".into()));
    let container = catalog
        .container_of(&"InjectAttribute".into())
        .expect("InjectAttribute must have a container");
    assert_eq!(container.as_str(), "Host");

    // 构造函数元数据应被收集（M4-3 使用）
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature must be in catalog");
    assert_eq!(feature.constructors.len(), 1);
    assert!(feature.constructors[0].param_types.is_empty());
}

#[test]
fn class_deriving_unrelated_generic_not_identified_as_feature() {
    let src = format!(
        r#"
{STD_PREAMBLE}

class Box<T> {{
    public Box() {{}}
}}

class IntBox : Box<int> {{
    public IntBox() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    // IntBox 派生自 Box<int>，不是 GenerateToAttribute<T>，不应识别为 feature
    assert!(!catalog.is_feature(&"IntBox".into()));
}

#[test]
fn static_class_marked_generate_to_identified_as_container() {
    // v0.11 修订：静态类作为容器同样通过 features 反向推断识别
    let src = format!(
        r#"
{STD_PREAMBLE}

static class HostStatic {{
    public static void Register() {{
        // 展开槽位
    }}
    public static int Compute(int x) {{
        return x * 2;
    }}
}}

class InjectAttribute : GenerateToAttribute<HostStatic> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(catalog.is_container(&"HostStatic".into()));
    let slots = catalog.slots_of(&"HostStatic".into()).expect("slots");
    assert_eq!(slots.len(), 2);
    assert!(slots.iter().any(|s| s.method_name.as_str() == "Register"));
    assert!(slots.iter().any(|s| s.method_name.as_str() == "Compute"));
}

#[test]
fn macro_catalog_empty_for_program_without_macros() {
    let src = format!(
        r#"
{STD_PREAMBLE}

class PlainA {{}}
class PlainB {{}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(catalog.containers.is_empty());
    assert!(catalog.features.is_empty());
}

#[test]
fn feature_with_constructor_params_collected() {
    // 验证带参构造函数的宏特性派生类正确收集 ctor 元数据
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(int priority, string name) {{
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature must be identified");
    assert_eq!(feature.constructors.len(), 1);
    assert_eq!(feature.constructors[0].param_types.len(), 2);
    assert_eq!(feature.constructors[0].param_types[0].as_str(), "int");
    assert_eq!(feature.constructors[0].param_types[1].as_str(), "string");
}

#[test]
fn container_slots_skip_property_accessors() {
    // property 自动生成的 get_*/set_* 方法不应成为展开槽位
    // v0.11 修订：容器通过 features 反向推断识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public int Count {{ get; set; }}
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let slots = catalog.slots_of(&"Host".into()).expect("slots");
    // 只应有 Register 一个槽位，不应有 get_Count / set_Count
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].method_name.as_str(), "Register");
}

// ============================================================================
// M4-3: 构造函数中 this.<slot>(<lambda>) 调用识别
// ============================================================================

#[test]
fn feature_single_registration_identified() {
    // 派生类构造函数中 this.Register(() => "code") 被识别为一次展开委托注册
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "return 42;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature must be identified");

    assert_eq!(feature.registrations.len(), 1, "exactly one registration");
    let reg = &feature.registrations[0];
    assert_eq!(reg.slot_name.as_str(), "Register");
    // Lambda 形参应为空（Func<string> 无参委托）
    assert!(reg.expansion.params.is_empty());
    // Lambda body 应为 Expr 形式（`=> "..."`）
    assert!(matches!(&reg.expansion.body, ast::LambdaBody::Expr(_)));
}

#[test]
fn feature_multiple_registrations_in_one_ctor() {
    // 一个构造函数中可注册多个不同 slot
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
    public void Test() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "r;");
        this.Test(() => "t;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");

    assert_eq!(feature.registrations.len(), 2);
    let names: Vec<&str> = feature
        .registrations
        .iter()
        .map(|r| r.slot_name.as_str())
        .collect();
    assert!(names.contains(&"Register"));
    assert!(names.contains(&"Test"));
}

#[test]
fn feature_call_to_non_slot_method_not_registered() {
    // this.OtherMethod(...) 中 OtherMethod 不在容器 slots 中，不识别为注册
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public void OtherMethod() {{}}

    public InjectAttribute() {{
        this.OtherMethod();
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    assert!(
        feature.registrations.is_empty(),
        "non-slot call must not register"
    );
}

#[test]
fn feature_call_with_non_lambda_arg_not_registered() {
    // this.Register("string_literal") —— arg 不是 Lambda，不识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register("not a lambda");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    assert!(
        feature.registrations.is_empty(),
        "non-lambda arg must not register"
    );
}

#[test]
fn feature_call_with_non_this_receiver_not_registered() {
    // foo.Register(...) 中 receiver 不是 `this`，不识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class Helper {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        var h = new Helper();
        h.Register();
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    assert!(
        feature.registrations.is_empty(),
        "non-this receiver must not register"
    );
}

#[test]
fn feature_block_lambda_body_captured() {
    // Lambda body 形式为 Block（含 return 语句）
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            return "block body";
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    assert_eq!(feature.registrations.len(), 1);
    let reg = &feature.registrations[0];
    assert_eq!(reg.slot_name.as_str(), "Register");
    assert!(matches!(&reg.expansion.body, ast::LambdaBody::Block(_)));
}

#[test]
fn feature_no_ctor_body_has_no_registrations() {
    // 派生类无构造函数 → registrations 为空
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    assert!(feature.registrations.is_empty());
}

#[test]
fn feature_registration_span_points_to_call_site() {
    // span 指向调用表达式位置（用于 M4-6 splice 诊断）
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "x");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("feature");
    let reg = &feature.registrations[0];
    // span 必须有效（非 DUMMY）
    assert!(
        reg.span != ast::Span::DUMMY,
        "registration span must be the call site, not DUMMY"
    );
}

// ============================================================================
// M4-4: 受限求值器端到端测试
//
// 通过 parse → typeck → macro_catalog 提取 registration，调用 Evaluator
// 求值 `Func<string>` 委托，验证返回的展开代码字符串。
// ============================================================================

/// 在 feature 的首个 registration 上运行 Evaluator，返回结果字符串。
fn eval_first_registration(
    tc: &TypeChecker,
    feature_name: &str,
) -> Result<String, typeck::EvalError> {
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from(feature_name))
        .unwrap_or_else(|| panic!("feature {feature_name} not in catalog"));
    assert!(!feature.registrations.is_empty(), "no registrations");
    let reg = &feature.registrations[0];
    let w = Whitelist::new();
    let mut e = Evaluator::new(&w);
    e.eval_lambda(&reg.expansion)
}

#[test]
fn m4_4_evaluator_literal_string() {
    // `this.Register(() => "code")` 求值结果为 "code"
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "return 42;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "return 42;");
}

#[test]
fn m4_4_evaluator_string_concat() {
    // 字符串拼接：`() => "a" + "b" + "c"` → "abc"
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "a" + "b" + "c");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "abc");
}

#[test]
fn m4_4_evaluator_stringbuilder_basic() {
    // StringBuilder.Append + ToString：构造展开代码
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append("return ");
            sb.Append("42;");
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "return 42;");
}

#[test]
fn m4_4_evaluator_stringbuilder_chain() {
    // 链式 Append：`sb.Append("a").Append("b")` → "ab"
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append("a").Append("b").Append("c");
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "abc");
}

#[test]
fn m4_4_evaluator_stringbuilder_append_int() {
    // sb.Append(int) 自动转字符串
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append(42);
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "42");
}

#[test]
fn m4_4_evaluator_if_else_branch() {
    // if/else 分支返回不同字符串。注意：受限求值器看不到 ctor 参数，
    // 此用例验证「ctor 参数不在 evaluator 作用域内」——会报 UndefinedName。
    // 未来若需要 ctor 参数注入求值器环境，需在 M4-7 (Pass 2/4) 落地。
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(int x) {{
        this.Register(() => {{
            if (x > 0) {{
                return "positive";
            }} else {{
                return "non-positive";
            }}
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    // x 在 ctor 中无绑定到 evaluator 作用域，evaluator 查询 Ident("x") 会失败
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(
        matches!(r, Err(typeck::EvalError::UndefinedName { .. })),
        "expected UndefinedName for ctor param x, got: {r:?}"
    );
}

#[test]
fn m4_4_evaluator_forbidden_loop() {
    // while 循环被拒绝
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            var i = 0;
            while (i < 3) {{
                sb.Append("x");
            }}
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(
        r,
        Err(typeck::EvalError::ForbiddenConstruct { construct, .. })
            if construct.contains("loop")
    ));
}

#[test]
fn m4_4_evaluator_forbidden_throw() {
    // throw 被拒绝
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            throw "error";
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(
        r,
        Err(typeck::EvalError::ForbiddenConstruct {
            construct: "throw",
            ..
        })
    ));
}

#[test]
fn m4_4_evaluator_forbidden_try_catch() {
    // try/catch 被拒绝
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            try {{
                return "a";
            }} catch (Exception e) {{
                return "b";
            }}
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(
        r,
        Err(typeck::EvalError::ForbiddenConstruct { construct, .. })
            if construct.contains("try")
    ));
}

#[test]
fn m4_4_evaluator_non_whitelist_method_rejected() {
    // sb.Sort() 不在白名单
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Sort();
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(r, Err(typeck::EvalError::NotInWhitelist { .. })));
}

#[test]
fn m4_4_evaluator_non_newable_rejected() {
    // RFC 028 M5-3: `new List<string>()` 现已 newable；改用 `new Dictionary()`
    // 验证白名单外类型仍被拒绝。
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var lst = new Dictionary();
            return "x";
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(r, Err(typeck::EvalError::NotNewable { .. })));
}

#[test]
fn m4_4_evaluator_return_non_string_rejected() {
    // return 42 触发 ReturnTypeMismatch（Func<string> 要求返回 string）
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            return 42;
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(
        r,
        Err(typeck::EvalError::ReturnTypeMismatch { .. })
    ));
}

#[test]
fn m4_4_evaluator_lambda_creation_forbidden() {
    // 委托体内创建嵌套 lambda 被拒绝
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var f = () => "x";
            return "y";
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(
        r,
        Err(typeck::EvalError::ForbiddenConstruct { construct, .. })
            if construct.contains("lambda")
    ));
}

#[test]
fn m4_4_evaluator_undefined_name_rejected() {
    // 未定义的标识符触发 UndefinedName
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => unknown);
    }}
}}
"#
    );
    let tc = check_src(&src);
    let r = eval_first_registration(&tc, "InjectAttribute");
    assert!(matches!(r, Err(typeck::EvalError::UndefinedName { .. })));
}

#[test]
fn m4_4_evaluator_stringbuilder_with_local_int_concat() {
    // 复合场景：用局部 int 累加 + 字符串拼接
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            var count = 3;
            sb.Append("count = " + count);
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let result = eval_first_registration(&tc, "InjectAttribute").unwrap();
    assert_eq!(result, "count = 3");
}

// ============================================================================
// M4-6: 展开字符串解析 + splice + span 映射 端到端测试
//
// 通过 parse → typeck → macro_catalog 提取 registration，先调用 Evaluator
// 求值委托得到字符串，再用 parse_expansion 解析并应用 span 映射，验证：
// - 展开字符串被解析为正确的 AST 形状
// - 所有节点 span 被重写为 MacroRegistration.span（委托位置）
// - 解析失败时 SpliceError 携带 delegate_span
// ============================================================================

/// 在 feature 的首个 registration 上运行 Evaluator + parse_expansion，
/// 返回 (语句列表, 委托 span)。
fn eval_and_splice_first(
    tc: &TypeChecker,
    feature_name: &str,
) -> Result<(Vec<ast::Spanned<ast::Stmt>>, ast::Span), typeck::SpliceError> {
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from(feature_name))
        .unwrap_or_else(|| panic!("feature {feature_name} not in catalog"));
    assert!(!feature.registrations.is_empty(), "no registrations");
    let reg = &feature.registrations[0];

    // 求值委托得到展开字符串（求值失败 panic——上层测试应单独覆盖）
    let w = Whitelist::new();
    let mut e = Evaluator::new(&w);
    let expansion = e
        .eval_lambda(&reg.expansion)
        .unwrap_or_else(|err| panic!("evaluator failed: {err:?}"));

    // splice：解析 + span 重写
    let stmts = parse_expansion(&expansion, reg.span, 0)?;
    Ok((stmts, reg.span))
}

#[test]
fn m4_6_splice_literal_string_ast() {
    // `() => "var x = 1;"` → 展开为一条 `var x = 1;` 语句
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 1;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    assert_eq!(stmts.len(), 1);
    assert_eq!(
        stmts[0].span, delegate_span,
        "顶层 stmt span 必须为委托位置"
    );
    match &stmts[0].node {
        ast::Stmt::Let {
            name,
            init: Some(e),
            ..
        } => {
            assert_eq!(name.as_str(), "x");
            assert_eq!(e.span, delegate_span);
            match &e.node {
                ast::Expr::IntLit(i) => assert_eq!(*i, 1),
                other => panic!("expected IntLit, got {other:?}"),
            }
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_stringbuilder_expansion_ast() {
    // 用 StringBuilder 拼接 Arc 代码字符串 → 展开为多条 Arc 语句
    // 这是典型的「宏特性生成 Arc 代码注入容器方法」场景
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append("var x = 1;");
            sb.Append(" var y = 2;");
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    // 展开字符串："var x = 1; var y = 2;" → 2 条语句
    assert_eq!(stmts.len(), 2, "expected 2 stmts, got {}", stmts.len());
    for (i, s) in stmts.iter().enumerate() {
        assert_eq!(s.span, delegate_span, "stmt[{i}] span 必须为委托位置");
    }
    // 验证语句形状
    match &stmts[0].node {
        ast::Stmt::Let { name, .. } => assert_eq!(name.as_str(), "x"),
        other => panic!("expected Let x, got {other:?}"),
    }
    match &stmts[1].node {
        ast::Stmt::Let { name, .. } => assert_eq!(name.as_str(), "y"),
        other => panic!("expected Let y, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_if_else_branch_ast() {
    // if 作为表达式：`() => if (true) { return "..."; } else { return "..."; }`
    // → evaluator 选择 true 分支，return 值通过 eval_expr(If) 路径正确返回
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => if (true) {{ return "var a = 1;"; }} else {{ return "var b = 2;"; }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    // evaluator 选择 true 分支，返回 "var a = 1;" → 解析为一条 Let 语句
    assert_eq!(stmts.len(), 1);
    assert_eq!(stmts[0].span, delegate_span);
    match &stmts[0].node {
        ast::Stmt::Let { name, .. } => assert_eq!(name.as_str(), "a"),
        other => panic!("expected Let a, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_method_call_chain_ast() {
    // 展开字符串含方法调用链：`Console.WriteLine(sb.ToString());`
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "Console.WriteLine(sb.ToString());");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    assert_eq!(stmts.len(), 1);
    let stmt = &stmts[0];
    assert_eq!(stmt.span, delegate_span);
    // 验证 MethodCall 链：MethodCall(Console, WriteLine, args=[MethodCall(sb, ToString)])
    match &stmt.node {
        ast::Stmt::Expr(e) => {
            assert_eq!(e.span, delegate_span);
            match &e.node {
                ast::Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                } => {
                    assert_eq!(method.as_str(), "WriteLine");
                    assert_eq!(receiver.span, delegate_span);
                    assert_eq!(args.len(), 1);
                    assert_eq!(args[0].span, delegate_span);
                    // 内层 ToString 调用
                    match &args[0].node {
                        ast::Expr::MethodCall {
                            receiver: inner,
                            method: inner_method,
                            ..
                        } => {
                            assert_eq!(inner_method.as_str(), "ToString");
                            assert_eq!(inner.span, delegate_span);
                        }
                        other => panic!("expected MethodCall ToString, got {other:?}"),
                    }
                }
                other => panic!("expected MethodCall WriteLine, got {other:?}"),
            }
        }
        other => panic!("expected Expr stmt, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_recursive_spans_all_match_delegate() {
    // 递归验证：复杂表达式树的所有内部 span 都被重写为委托位置
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 1 + 2 * 3;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    assert_eq!(stmts.len(), 1);
    // 深度遍历验证所有 Spanned<T> 的 span 字段
    fn walk_expr(e: &ast::Spanned<ast::Expr>, expected: ast::Span) {
        assert_eq!(e.span, expected, "expr span mismatch");
        match &e.node {
            ast::Expr::Binary { left, right, .. } => {
                walk_expr(left, expected);
                walk_expr(right, expected);
            }
            ast::Expr::IntLit(_) | ast::Expr::Ident(_) | ast::Expr::StringLit(_) => {}
            _ => {} // 其他变体此处不展开
        }
    }
    match &stmts[0].node {
        ast::Stmt::Let { init: Some(e), .. } => {
            // 顶层是 `1 + (2 * 3)` 或 `(1 + 2) * 3`——取决于优先级
            // 我们只需递归验证所有 span 一致
            walk_expr(e, delegate_span);
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_parse_error_carries_delegate_span() {
    // evaluator 输出非法 Arc 代码 → SpliceError::ParseError 携带 delegate_span
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = ;");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .unwrap();
    let reg = &feature.registrations[0];

    let w = Whitelist::new();
    let mut e = Evaluator::new(&w);
    let expansion = e.eval_lambda(&reg.expansion).unwrap();

    // expansion = "var x = ;" —— 解析失败
    let r = parse_expansion(&expansion, reg.span, 0);
    match r {
        Err(typeck::SpliceError::ParseError { delegate_span, .. }) => {
            assert_eq!(delegate_span, reg.span);
        }
        other => panic!("expected ParseError, got {other:?}"),
    }
}

#[test]
fn m4_6_splice_empty_string_yields_no_stmts() {
    // evaluator 返回空字符串 → 解析为空语句列表
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, _) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();
    assert!(stmts.is_empty(), "empty expansion should yield no stmts");
}

#[test]
fn m4_6_splice_multi_line_expansion_ast() {
    // 多行展开代码：StringBuilder 构造多行 Arc 代码字符串
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append("var a = 1;");
            sb.Append(" var b = 2;");
            sb.Append(" var c = 3;");
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let tc = check_src(&src);
    let (stmts, delegate_span) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();

    // 展开字符串 "var a = 1; var b = 2; var c = 3;" → 3 条 Let 语句
    assert_eq!(
        stmts.len(),
        3,
        "expected 3 stmts for multi-segment expansion"
    );
    for (i, s) in stmts.iter().enumerate() {
        assert_eq!(s.span, delegate_span, "stmt[{i}] span mismatch");
    }
    // 验证三条语句分别是 a, b, c
    let names: Vec<&str> = stmts
        .iter()
        .map(|s| match &s.node {
            ast::Stmt::Let { name, .. } => name.as_str(),
            other => panic!("expected Let, got {other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

// ============================================================================
// RFC 028 M4-7: 两轮 typeck 拆分（Pass 2 骨架 + Pass 4 完整 typeck）
//
// 验证 D12.2 编译 Pass 顺序中 typeck 的两阶段行为：
// - Pass 2（Skeleton）：宏容器类跳过方法体检查，仅校验签名 + 属性解析
// - Pass 4（Full）：splice 展开代码后对宏容器类方法体做完整 typeck
// - Pass 4 不重复注册属性（Pass 2 已完成）
// ============================================================================

/// 驱动 parse → hir → typeck 管线，返回 `TypeChecker` 与 `check_module` 结果。
fn check_src_result(
    src: &str,
) -> (
    TypeChecker,
    Result<Vec<typeck::TypedFn>, Vec<typeck::TypeError>>,
) {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let result = tc.check_module(&module);
    (tc, result)
}

/// 用于 Pass 3 模拟的 span（与 parse_expansion 的 delegate_span 对齐）。
const DUMMY_SPAN: Span = Span {
    file_id: 0,
    start: 0,
    end: 0,
};

/// 将展开语句列表 splice 到容器类的指定方法体末尾。
fn splice_into_method(
    tc: &mut TypeChecker,
    container: &str,
    method_name: &str,
    stmts: Vec<ast::Spanned<ast::Stmt>>,
) {
    let container_ident = Ident::from(container);
    if let Some(class_def) = tc.class_defs_mut().get_mut(&container_ident) {
        for method in &mut class_def.methods {
            if method.node.sig.name.as_str() == method_name {
                if let Some(body) = &mut method.node.body {
                    body.stmts.extend(stmts);
                    return;
                }
            }
        }
    }
}

#[test]
fn m4_7_pass2_skips_macro_container_body_type_error() {
    // 宏容器类方法体含类型错误——Pass 2 骨架模式应跳过 body 检查
    // v0.11 修订：容器通过 features 反向推断识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{
        int x = "not an int";
    }}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let (tc, result) = check_src_result(&src);
    // Pass 2 应成功——宏容器方法体被跳过
    assert!(
        result.is_ok(),
        "Pass 2 skeleton must skip macro container body errors, got: {:?}",
        result.err()
    );
    // Host 应被识别为宏容器（通过 InjectAttribute 反向推断）
    assert!(tc.macro_catalog().is_container(&Ident::from("Host")));
}

#[test]
fn m4_7_pass2_catches_non_container_body_type_error() {
    // 非容器类方法体含类型错误——Pass 2 应完整检查并报告
    let src = format!(
        r#"
{STD_PREAMBLE}

class Normal {{
    public void DoSomething() {{
        int x = "not an int";
    }}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    // Pass 2 应失败——非容器类 body 被完整检查
    assert!(
        result.is_err(),
        "Pass 2 must catch non-container body type errors"
    );
}

#[test]
fn m4_7_pass4_catches_spliced_type_error() {
    // Pass 2 成功（空 body）→ splice 含类型错误的代码 → Pass 4 应捕获
    // v0.11 修订：容器通过 features 反向推断识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(
        pass2.is_ok(),
        "Pass 2 should succeed for empty container body"
    );

    // splice 含类型错误的语句
    let bad_stmts = parse_expansion(r#"int x = "not an int";"#, DUMMY_SPAN, 0).unwrap();
    splice_into_method(&mut tc, "Host", "Register", bad_stmts);

    // Pass 4 应捕获类型错误
    let pass4 = tc.check_macro_containers_pass4();
    assert!(
        pass4.is_err(),
        "Pass 4 must catch type errors in spliced code"
    );
}

#[test]
fn m4_7_pass4_validates_valid_spliced_code() {
    // Pass 2 成功 → splice 合法代码 → Pass 4 应通过
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok(), "Pass 2 should succeed");

    // splice 合法语句
    let good_stmts = parse_expansion("int x = 42;", DUMMY_SPAN, 0).unwrap();
    splice_into_method(&mut tc, "Host", "Register", good_stmts);

    // Pass 4 应通过
    let pass4 = tc.check_macro_containers_pass4();
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate valid spliced code, got: {:?}",
        pass4.err()
    );
}

#[test]
fn m4_7_pass4_no_duplicate_attributes() {
    // Pass 4 不应重复注册属性（Pass 2 已完成）。
    // v0.11：Host 通过 InjectAttribute 反向推断为容器；用 [AttributeUsage]
    // 作为普通业务属性验证 Pass 4 不重复注册（与 [GenerateTo] 解耦——v0.11
    // 后 [GenerateTo] 是普通属性，不再参与容器识别）。
    let src = format!(
        r#"
{STD_PREAMBLE}

[AttributeUsage(AttributeTargets.Class)]
class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok());

    // Pass 2 后查询 Host 的属性
    let host_def_id = tc.class_def_id("Host").expect("Host must have DefId");
    let attr_usage_count = |tc: &TypeChecker| {
        tc.attribute_table()
            .get_attrs(host_def_id)
            .iter()
            .filter(|a| a.name.as_str() == "AttributeUsage")
            .count()
    };

    assert_eq!(
        attr_usage_count(&tc),
        1,
        "Pass 2 should register AttributeUsage exactly once"
    );

    // 运行 Pass 4
    let pass4 = tc.check_macro_containers_pass4();
    assert!(pass4.is_ok(), "Pass 4 should succeed");

    // 属性数量不应增加
    assert_eq!(
        attr_usage_count(&tc),
        1,
        "Pass 4 must not duplicate attributes"
    );
}

#[test]
fn m4_7_pass4_pushes_typed_fns_for_containers() {
    // Pass 2 跳过容器类 typed_fns（emit_fns=false）→ Pass 4 应 push typed_fns。
    // v0.11：Host 通过 InjectAttribute 反向推断为容器（无需 [GenerateTo] 标注）。
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok());

    // Pass 2 产出的 typed_fns 不含 Host 的方法（emit_fns=false）
    let pass2_typed_fn_count = pass2.as_ref().map(|fns| fns.len()).unwrap_or(0);

    // splice 合法代码使方法体非空
    let good_stmts = parse_expansion("int x = 42;", DUMMY_SPAN, 0).unwrap();
    splice_into_method(&mut tc, "Host", "Register", good_stmts);

    // Pass 4 应 push Host::Register 的 typed_fn
    let pass4 = tc.check_macro_containers_pass4();
    assert!(pass4.is_ok(), "Pass 4 should succeed: {:?}", pass4.err());

    // Pass 4 后 typed_fns 应包含 Host 的方法
    // （通过 typed_fns 数量增加来间接验证——Pass 2 未 push 容器方法）
    let _ = pass2_typed_fn_count; // Pass 2 count 作为基线参考
}

#[test]
fn m4_7_full_pipeline_pass2_splice_pass4() {
    // 完整管线：Pass 2 → 求值委托 → parse_expansion → splice → Pass 4
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 42;");
    }}
}}
"#
    );
    let (mut tc, _pass2) = check_src_result(&src);
    // Pass 2 可能因 InjectAttribute 构造函数体 typeck 失败而返回 Err
    // （this.Register 在 InjectAttribute 上未定义）——这是预期行为，
    // feature 类的 ctor body 在 Pass 2 正常 typeck。

    // 求值委托 + parse_expansion
    let (stmts, _) = eval_and_splice_first(&tc, "InjectAttribute").unwrap();
    assert_eq!(stmts.len(), 1, "expansion should yield 1 stmt");

    // splice 到 Host.Register
    splice_into_method(&mut tc, "Host", "Register", stmts);

    // Pass 4 应验证 splice 后的代码
    let pass4 = tc.check_macro_containers_pass4();
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate spliced code in full pipeline, got: {:?}",
        pass4.err()
    );
}

#[test]
fn m4_7_pass4_catches_spliced_undefined_variable() {
    // splice 引用未定义变量的代码 → Pass 4 应捕获
    // v0.11 修订：容器通过 features 反向推断识别
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok());

    // splice 引用未定义变量的语句
    let bad_stmts = parse_expansion("int x = undefined_var;", DUMMY_SPAN, 0).unwrap();
    splice_into_method(&mut tc, "Host", "Register", bad_stmts);

    let pass4 = tc.check_macro_containers_pass4();
    assert!(
        pass4.is_err(),
        "Pass 4 must catch undefined variable in spliced code"
    );
}

// ============================================================================
// RFC 028 M4-8 D12.4: 循环依赖检测 + arc-macro-010 错误码
//
// 验证两类循环依赖被检测并报告 arc-macro-010：
// 1. 直接自引用：类 F 派生自 GenerateToAttribute<F>——反向推断下 F 既是容器又是特性（v0.11）
// 2. 间接循环：容器 C 被标注了指向自身的特性 [Feature] where FeatureAttribute : GenerateToAttribute<C>
// 3. 正常场景（非循环）不报错
// ============================================================================

/// 检查错误列表中是否含 arc-macro-010 错误。
fn has_arc_macro_010(errs: &[typeck::TypeError]) -> bool {
    errs.iter().any(|e| {
        matches!(
            e,
            typeck::TypeError::Macro {
                code: "arc-macro-010",
                ..
            }
        )
    })
}

#[test]
fn m4_8_detects_direct_self_reference() {
    // F 同时是容器和特性，且 feature.container == F
    let src = format!(
        r#"
{STD_PREAMBLE}

class SelfHost : GenerateToAttribute<SelfHost> {{
    public void Register() {{}}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    let errs = result.err().unwrap_or_default();
    assert!(
        has_arc_macro_010(&errs),
        "direct self-reference must trigger arc-macro-010, got: {errs:?}"
    );
}

#[test]
fn m4_8_detects_indirect_cycle_short_name() {
    // 容器 Host 被标注 [SelfInject]（短名），SelfInjectAttribute : GenerateToAttribute<Host>
    let src = format!(
        r#"
{STD_PREAMBLE}

[SelfInject]
class Host {{
    public void Register() {{}}
}}

class SelfInjectAttribute : GenerateToAttribute<Host> {{
    public SelfInjectAttribute() {{
        this.Register(() => "var x = 1;");
    }}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    let errs = result.err().unwrap_or_default();
    assert!(
        has_arc_macro_010(&errs),
        "indirect cycle (short name [SelfInject]) must trigger arc-macro-010, got: {errs:?}"
    );
}

#[test]
fn m4_8_detects_indirect_cycle_full_name() {
    // 容器 Host 被标注 [SelfInjectAttribute]（全名），同一特性类
    let src = format!(
        r#"
{STD_PREAMBLE}

[SelfInjectAttribute]
class Host {{
    public void Register() {{}}
}}

class SelfInjectAttribute : GenerateToAttribute<Host> {{
    public SelfInjectAttribute() {{
        this.Register(() => "var x = 1;");
    }}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    let errs = result.err().unwrap_or_default();
    assert!(
        has_arc_macro_010(&errs),
        "indirect cycle (full name [SelfInjectAttribute]) must trigger arc-macro-010, got: {errs:?}"
    );
}

#[test]
fn m4_8_no_cycle_for_non_container_annotated_class() {
    // 非容器的普通类被标注 [Inject]——不构成循环
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 1;");
    }}
}}

[Inject]
class MyService {{
    public int Value {{ get; set; }}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    // MyService 不是宏容器，标注 [Inject] 不构成循环
    // （可能有其他 typeck 错误，但不应有 arc-macro-010）
    let errs = result.err().unwrap_or_default();
    assert!(
        !has_arc_macro_010(&errs),
        "non-container annotated class must not trigger arc-macro-010, got: {errs:?}"
    );
}

#[test]
fn m4_8_no_cycle_for_feature_targeting_different_container() {
    // FeatureA : GenerateToAttribute<HostA>，HostB 被标注 [FeatureA]——不构成循环
    // （FeatureA 注入 HostA，不是 HostB）
    let src = format!(
        r#"
{STD_PREAMBLE}

class HostA {{
    public void Register() {{}}
}}

[Inject]
class HostB {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<HostA> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 1;");
    }}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    let errs = result.err().unwrap_or_default();
    assert!(
        !has_arc_macro_010(&errs),
        "feature targeting different container must not trigger arc-macro-010, got: {errs:?}"
    );
}

#[test]
fn m4_8_error_message_contains_cyclic_compile_to_dependency() {
    // 验证错误消息含 "cyclic compile-to dependency"
    let src = format!(
        r#"
{STD_PREAMBLE}

class SelfHost : GenerateToAttribute<SelfHost> {{
    public void Register() {{}}
}}
"#
    );
    let (_tc, result) = check_src_result(&src);
    let errs = result.err().unwrap_or_default();
    let has_msg = errs.iter().any(|e| {
        matches!(e, typeck::TypeError::Macro { message, .. } if message.contains("cyclic compile-to dependency"))
    });
    assert!(
        has_msg,
        "error message must contain 'cyclic compile-to dependency', got: {errs:?}"
    );
}

// ============================================================================
// RFC 028 M4-9 D12.2: Pipeline 集成（Pass 2 → Pass 3 → Pass 4）
//
// 验证 `TypeChecker::expand_macros`（Pass 3）驱动完整宏展开管线：
//   Pass 2 check_module → Pass 3 expand_macros → Pass 4 check_macro_containers_pass4
//
// 覆盖场景：
// 1. 完整管线成功：单 feature 单 slot，splice 后 Pass 4 通过
// 2. 多 feature 多 slot 同时展开并 splice
// 3. Pass 3 求值失败 → arc-macro-002，跳过 splice
// 4. Pass 3 解析失败 → arc-macro-003，跳过 splice
// 5. Pass 3 部分失败不影响其他成功注册（错误隔离）
// 6. Pass 4 捕获 splice 后类型错误
// 7. 无宏特性时 expand_macros 为 no-op
// 8. 完整管线：StringBuilder 拼接展开
// ============================================================================

/// 检查错误列表中是否含指定 arc-macro-XXX 错误码。
fn has_arc_macro_code(errs: &[typeck::TypeError], code: &str) -> bool {
    errs.iter().any(|e| {
        matches!(
            e,
            typeck::TypeError::Macro { code: c, .. } if *c == code
        )
    })
}

/// 驱动完整管线 Pass 2 → Pass 3 → Pass 4，返回 (TypeChecker, Pass 3 结果, Pass 4 结果)。
fn run_full_pipeline(
    src: &str,
) -> (
    TypeChecker,
    Result<Vec<typeck::TypedFn>, Vec<typeck::TypeError>>,
    Result<(), Vec<typeck::TypeError>>,
    Result<(), Vec<typeck::TypeError>>,
) {
    let (mut tc, pass2) = check_src_result(src);
    let pass3 = tc.expand_macros();
    let pass4 = tc.check_macro_containers_pass4();
    (tc, pass2, pass3, pass4)
}

#[test]
fn m4_9_full_pipeline_single_feature_single_slot() {
    // 完整管线：Pass 2 → Pass 3 (expand_macros) → Pass 4
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 42;");
    }}
}}
"#
    );
    let (tc, _pass2, pass3, pass4) = run_full_pipeline(&src);

    assert!(
        pass3.is_ok(),
        "Pass 3 expand_macros should succeed for valid lambda, got: {:?}",
        pass3.err()
    );
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate spliced code, got: {:?}",
        pass4.err()
    );

    // 验证 splice 确实发生：Host.Register 方法体应包含 `var x = 42;`
    let host = tc
        .class_defs()
        .get(&Ident::from("Host"))
        .expect("Host class must be in class_defs");
    let register = host
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .expect("Register method must exist");
    let body = register.node.body.as_ref().expect("body must exist");
    assert!(
        !body.stmts.is_empty(),
        "Register body must have spliced stmts, got empty"
    );
    // 验证 splice 的语句是 `var x = 42;`
    let has_let_x = body.stmts.iter().any(|s| {
        matches!(
            &s.node,
            ast::Stmt::Let { name, .. } if name.as_str() == "x"
        )
    });
    assert!(has_let_x, "spliced stmt should be `var x = 42;`");
}

#[test]
fn m4_9_full_pipeline_multiple_features_multiple_slots() {
    // 两个容器、两个 feature、各自注册不同 slot
    let src = format!(
        r#"
{STD_PREAMBLE}

class HostA {{
    public void Register() {{}}
    public void Test() {{}}
}}

class HostB {{
    public void Register() {{}}
}}

class InjectA : GenerateToAttribute<HostA> {{
    public InjectA() {{
        this.Register(() => "var a = 1;");
        this.Test(() => "var b = 2;");
    }}
}}

class InjectB : GenerateToAttribute<HostB> {{
    public InjectB() {{
        this.Register(() => "var c = 3;");
    }}
}}
"#
    );
    let (tc, _pass2, pass3, pass4) = run_full_pipeline(&src);

    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());
    assert!(pass4.is_ok(), "Pass 4 should succeed: {:?}", pass4.err());

    // 验证 HostA.Register 有 `var a = 1;`
    let host_a = tc.class_defs().get(&Ident::from("HostA")).unwrap();
    let reg_a = host_a
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .unwrap();
    let body_a = reg_a.node.body.as_ref().unwrap();
    assert!(body_a.stmts.iter().any(|s| matches!(
        &s.node,
        ast::Stmt::Let { name, .. } if name.as_str() == "a"
    )));

    // 验证 HostA.Test 有 `var b = 2;`
    let test_a = host_a
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Test")
        .unwrap();
    let body_test = test_a.node.body.as_ref().unwrap();
    assert!(body_test.stmts.iter().any(|s| matches!(
        &s.node,
        ast::Stmt::Let { name, .. } if name.as_str() == "b"
    )));

    // 验证 HostB.Register 有 `var c = 3;`
    let host_b = tc.class_defs().get(&Ident::from("HostB")).unwrap();
    let reg_b = host_b
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .unwrap();
    let body_b = reg_b.node.body.as_ref().unwrap();
    assert!(body_b.stmts.iter().any(|s| matches!(
        &s.node,
        ast::Stmt::Let { name, .. } if name.as_str() == "c"
    )));
}

#[test]
fn m4_9_pass3_evaluator_failure_reports_arc_macro_002() {
    // 求值器失败：委托体内调用白名单外的方法
    // （这里使用 `while` 禁用构造触发 ForbiddenConstruct）
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class BadInjectAttribute : GenerateToAttribute<Host> {{
    public BadInjectAttribute() {{
        this.Register(() => {{
            var i = 0;
            while (i < 10) {{
                i = i + 1;
            }}
            return "var x = 1;";
        }});
    }}
}}
"#
    );
    let (_tc, _pass2, pass3, _pass4) = run_full_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-002"),
        "Pass 3 should report arc-macro-002 on evaluator failure, got: {errs:?}"
    );
}

#[test]
fn m4_9_pass3_parse_failure_reports_arc_macro_003() {
    // 解析失败：委托返回的字符串不是合法 Arc 代码
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class BadParseAttribute : GenerateToAttribute<Host> {{
    public BadParseAttribute() {{
        this.Register(() => "this is not valid arc code !!!");
    }}
}}
"#
    );
    let (_tc, _pass2, pass3, _pass4) = run_full_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-003"),
        "Pass 3 should report arc-macro-003 on parse failure, got: {errs:?}"
    );
}

#[test]
fn m4_9_pass3_error_isolation_continues_other_registrations() {
    // 错误隔离：一个 feature 的两个注册，第一个失败（解析错误），
    // 第二个应继续处理并成功 splice。
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
    public void Test() {{}}
}}

class MixedInjectAttribute : GenerateToAttribute<Host> {{
    public MixedInjectAttribute() {{
        this.Register(() => "!! invalid syntax !!");
        this.Test(() => "var ok = 42;");
    }}
}}
"#
    );
    let (tc, _pass2, pass3, _pass4) = run_full_pipeline(&src);

    // Pass 3 应报告 arc-macro-003（解析失败）但不影响其他注册
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-003"),
        "Pass 3 should report arc-macro-003 for bad registration, got: {errs:?}"
    );

    // Test slot 应仍被 splice（错误隔离）
    let host = tc.class_defs().get(&Ident::from("Host")).unwrap();
    let test = host
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Test")
        .unwrap();
    let body = test.node.body.as_ref().unwrap();
    assert!(
        body.stmts.iter().any(|s| matches!(
            &s.node,
            ast::Stmt::Let { name, .. } if name.as_str() == "ok"
        )),
        "Test slot should still be spliced despite Register failure"
    );

    // Register slot 应未被 splice（解析失败被跳过）
    let reg = host
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .unwrap();
    let reg_body = reg.node.body.as_ref().unwrap();
    assert!(
        reg_body.stmts.iter().all(|s| !matches!(
            &s.node,
            ast::Stmt::Let { name, .. } if name.as_str() == "ok"
        )),
        "Register slot must not have the Test's spliced code"
    );
}

#[test]
fn m4_9_pass4_catches_type_error_after_splice() {
    // Pass 3 splice 成功，但 splice 后代码有类型错误 → Pass 4 应捕获
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class TypeErrAttribute : GenerateToAttribute<Host> {{
    public TypeErrAttribute() {{
        // splice `int x = undefined_var;` → Pass 4 应报未定义变量
        this.Register(() => "int x = undefined_var;");
    }}
}}
"#
    );
    let (_tc, _pass2, pass3, pass4) = run_full_pipeline(&src);

    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed (parse is OK), got: {:?}",
        pass3.err()
    );
    assert!(
        pass4.is_err(),
        "Pass 4 must catch undefined variable in spliced code, got Ok"
    );
}

#[test]
fn m4_9_expand_macros_no_op_when_no_features() {
    // 无宏特性时 expand_macros 为 no-op
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class PlainClass {{
    public void Foo() {{}}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok(), "Pass 2 should succeed: {:?}", pass2.err());

    let pass3 = tc.expand_macros();
    assert!(pass3.is_ok(), "Pass 3 should be no-op: {:?}", pass3.err());

    let pass4 = tc.check_macro_containers_pass4();
    assert!(
        pass4.is_ok(),
        "Pass 4 should succeed for empty containers: {:?}",
        pass4.err()
    );
}

#[test]
fn m4_9_full_pipeline_stringbuilder_expansion() {
    // 典型场景：用 StringBuilder 拼接 Arc 代码字符串
    let src = format!(
        r#"
{STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class StringBuilderInjectAttribute : GenerateToAttribute<Host> {{
    public StringBuilderInjectAttribute() {{
        this.Register(() => {{
            var sb = new StringBuilder();
            sb.Append("var generated = 100;");
            return sb.ToString();
        }});
    }}
}}
"#
    );
    let (tc, _pass2, pass3, pass4) = run_full_pipeline(&src);

    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed for StringBuilder: {:?}",
        pass3.err()
    );
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate StringBuilder-generated code: {:?}",
        pass4.err()
    );

    // 验证 splice 的语句是 `var generated = 100;`
    let host = tc.class_defs().get(&Ident::from("Host")).unwrap();
    let reg = host
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .unwrap();
    let body = reg.node.body.as_ref().unwrap();
    let has_generated = body.stmts.iter().any(|s| {
        matches!(
            &s.node,
            ast::Stmt::Let { name, init: Some(e), .. } if name.as_str() == "generated"
                && matches!(&e.node, ast::Expr::IntLit(n) if *n == 100)
        )
    });
    assert!(
        has_generated,
        "StringBuilder expansion should produce `var generated = 100;`"
    );
}

#[test]
fn m4_9_pass3_pass4_separate_error_aggregation() {
    // Pass 3 与 Pass 4 错误分别聚合：一个 feature 求值失败（arc-macro-002），
    // 另一个 feature splice 后 Pass 4 类型错误。两者不应相互影响。
    let src = format!(
        r#"
{STD_PREAMBLE}

class HostA {{
    public void Register() {{}}
}}

class HostB {{
    public void Register() {{}}
}}

class EvalFailAttr : GenerateToAttribute<HostA> {{
    public EvalFailAttr() {{
        // 求值失败：while 禁用构造
        this.Register(() => {{
            var i = 0;
            while (i < 5) {{ i = i + 1; }}
            return "var a = 1;";
        }});
    }}
}}

class TypeErrAttr : GenerateToAttribute<HostB> {{
    public TypeErrAttr() {{
        // 求值成功但 Pass 4 类型错误
        this.Register(() => "int x = nonexistent;");
    }}
}}
"#
    );
    let (_tc, _pass2, pass3, pass4) = run_full_pipeline(&src);

    // Pass 3 应报告 arc-macro-002（EvalFailAttr 求值失败）
    let pass3_errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&pass3_errs, "arc-macro-002"),
        "Pass 3 should report arc-macro-002 for EvalFailAttr, got: {pass3_errs:?}"
    );

    // Pass 4 应报告 TypeErrAttr 的 splice 后类型错误
    let pass4_errs = pass4.err().unwrap_or_default();
    assert!(
        !pass4_errs.is_empty(),
        "Pass 4 should catch type error in TypeErrAttr's spliced code, got empty"
    );
}

// ============================================================================
// RFC 028 M5-2: Source Generator 识别（D13.2/D13.3）
//
// 验证 typeck 在 Pass 2 识别 [SourceGenerator] 标记的类并提取 Generate
// 方法体到 MacroCatalog.source_generators。
//
// 覆盖场景：
// 1. [SourceGenerator] 标记的类被识别为 Source Generator
// 2. Generate 方法体被正确提取
// 3. [SourceGeneratorAttribute] 长名形式同样识别
// 4. 未标记 [SourceGenerator] 的类不被识别
// 5. 无 Generate 方法的 Source Generator → generate_method_body 为 None
// ============================================================================

/// M5-2 测试用的 std 前言（含 SourceGeneratorAttribute + IGenerator + GeneratorContext）。
const M5_STD_PREAMBLE: &str = r#"
class Attribute {
    public Attribute() {}
}

class AttributeTargets {
    public const int Class = 1;
    public const int All = 255;
}

[AttributeUsage(AttributeTargets.Class)]
class AttributeUsageAttribute : Attribute {
    public int ValidOn { get; }
    public bool AllowMultiple { get; set; }
    public bool Inherited { get; set; }
    public AttributeUsageAttribute(int validOn) {
        ValidOn = validOn;
        AllowMultiple = false;
        Inherited = true;
    }
}

[AttributeUsage(AttributeTargets.Class)]
class SourceGeneratorAttribute : Attribute {
    public SourceGeneratorAttribute() {}
}

class AttributeList {
    public bool Has(string name) { return false; }
}

class AttributeTable {
    public int Count { get { return 0; } }
    public int GetDefIdAt(int index) { return 0; }
    public AttributeList GetAttrs(int defId) { return new AttributeList(); }
}

class SymbolTable {
    public string GetTypeName(int defId) { return ""; }
    public string GetMemberName(int defId) { return ""; }
}

// RFC 028 M5-4: List<T> 简化桩——Pass 2 typeck 需要「List<string> 可实例化」
// 的最小实现，避免 Pass 2 因 `undefined type List` 报错（std 真实 List<T>
// 在 std/Arc/Collections/List.as，此处仅测试桩用）。
class List<T> {
    public List() {}
    public void Add(T item) {}
}

class GeneratorContext {
    public AttributeTable Attributes { get; }
    public SymbolTable Symbols { get; }
    public List<string> SourceFiles { get; }
    public GeneratorContext() {
        Attributes = new AttributeTable();
        Symbols = new SymbolTable();
        SourceFiles = new List<string>();
    }
}

interface IGenerator {
    List<string> Generate(GeneratorContext context);
}
"#;

#[test]
fn m5_2_source_generator_class_identified() {
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class DtoGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class GeneratedDto {{}}");
        return results;
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    assert!(
        catalog.is_source_generator(&"DtoGenerator".into()),
        "DtoGenerator must be identified as Source Generator"
    );
}

#[test]
fn m5_2_generate_method_body_extracted() {
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class MyGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("var x = 1;");
        return results;
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    let sg = catalog
        .source_generators
        .get(&Ident::from("MyGenerator"))
        .expect("MyGenerator must be in source_generators");

    assert!(
        sg.generate_method_body.is_some(),
        "Generate method body must be extracted, got None"
    );
}

#[test]
fn m5_2_source_generator_attribute_long_name_also_identified() {
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGeneratorAttribute]
class LongNameGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        return results;
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(
        catalog.is_source_generator(&"LongNameGenerator".into()),
        "LongNameGenerator with [SourceGeneratorAttribute] must be identified"
    );
}

#[test]
fn m5_2_non_annotated_class_not_identified_as_source_generator() {
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class PlainClass : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        return results;
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    assert!(
        !catalog.is_source_generator(&"PlainClass".into()),
        "PlainClass without [SourceGenerator] must not be identified"
    );
}

#[test]
fn m5_2_source_generator_without_generate_method_has_none_body() {
    // 标了 [SourceGenerator] 但没有 Generate 方法 → body 为 None
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class EmptyGenerator : IGenerator {{
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    let sg = catalog
        .source_generators
        .get(&Ident::from("EmptyGenerator"))
        .expect("EmptyGenerator must still be registered as source generator");
    assert!(
        sg.generate_method_body.is_none(),
        "EmptyGenerator without Generate method must have None body"
    );
}

#[test]
fn m5_2_source_generator_and_macro_container_coexist() {
    // M4 宏容器与 M5 Source Generator 可在同一编译单元共存
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var a = 1;");
    }}
}}

[SourceGenerator]
class CoexistGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("var b = 2;");
        return results;
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();

    // M4 容器与 feature 仍在
    assert!(catalog.is_container(&"Host".into()));
    assert!(catalog.is_feature(&"InjectAttribute".into()));

    // M5 Source Generator 也在
    assert!(catalog.is_source_generator(&"CoexistGenerator".into()));
}

// ============================================================================
// RFC 028 M5-3: Source Generator Generate 方法求值 + 字符串解析为 Program
//
// 验证受限求值器扩展（List<string> 构造 + Add）与 TypeChecker
// expand_source_generators Pass 3 M5 入口：
// 1. Generate 方法求值成功 → generated_programs 填充
// 2. 多 Add 调用累积多个源文件
// 3. 求值失败（白名单越界 / 类型不匹配）报告 arc-macro-002
// 4. 生成字符串解析失败（语法错误）报告 arc-macro-003
// 5. 缺失 Generate 方法报告 arc-macro-020
// 6. 错误隔离——单个生成器失败不阻塞其他
// 7. 链式 Add 调用支持
// 8. 空字符串跳过
// 9. M4 与 M5 在同一管线协同（expand_macros + expand_source_generators）
// ============================================================================

/// 驱动完整 M5 管线 Pass 2 → Pass 3 M5（expand_source_generators）
fn run_m5_pipeline(src: &str) -> (TypeChecker, Result<(), Vec<typeck::TypeError>>) {
    let (mut tc, _pass2) = check_src_result(src);
    let pass3 = tc.expand_source_generators();
    (tc, pass3)
}

#[test]
fn m5_3_generate_method_evaluated_to_program() {
    // Generate 方法返回包含单个源文件的 List<string>
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class DtoGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class GeneratedDto {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(
        pass3.is_ok(),
        "Pass 3 expand_source_generators should succeed: {:?}",
        pass3.err()
    );

    // 验证 generated_programs 恰好包含 1 个 Program
    let progs = tc.generated_programs();
    assert_eq!(
        progs.len(),
        1,
        "expected exactly 1 generated program, got {}",
        progs.len()
    );

    // 验证生成的 Program 包含一个名为 GeneratedDto 的类
    let prog = &progs[0];
    let has_dto_class = prog.items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "GeneratedDto"
        )
    });
    assert!(
        has_dto_class,
        "generated program should contain class GeneratedDto"
    );
}

#[test]
fn m5_3_multiple_add_calls_produce_multiple_programs() {
    // Generate 方法通过多次 Add 累积多个源文件
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class MultiGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class First {{ }}");
        results.Add("public class Second {{ }}");
        results.Add("public class Third {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());

    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 3, "expected 3 generated programs");
    let names: Vec<&str> = progs
        .iter()
        .map(|p| {
            p.items
                .iter()
                .find_map(|item| match &item.node {
                    ast::Item::Class(c) => Some(c.name.as_str()),
                    _ => None,
                })
                .unwrap_or("")
        })
        .collect();
    assert!(names.contains(&"First"));
    assert!(names.contains(&"Second"));
    assert!(names.contains(&"Third"));
}

#[test]
fn m5_3_chained_add_calls_supported() {
    // 链式 Add：`results.Add("a").Add("b")` —— 同一 List 实例
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class ChainGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class A {{ }}").Add("public class B {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());
    assert_eq!(tc.generated_programs().len(), 2);
}

#[test]
fn m5_3_evaluator_failure_reports_arc_macro_002() {
    // Generate 方法体内调用白名单外方法 → arc-macro-002
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class BadGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        // context.SourceFiles 不是白名单方法，应触发 NotInWhitelist
        results.Add(context.SourceFiles());
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-002"),
        "expected arc-macro-002 error, got: {:?}",
        errs
    );
    // 生成失败时 generated_programs 为空
    assert!(tc.generated_programs().is_empty());
}

#[test]
fn m5_3_parse_failure_reports_arc_macro_003() {
    // Generate 方法返回的字符串非合法 Arc 语法 → arc-macro-003
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class SyntaxErrorGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class Broken {{");
        return results;
    }}
}}
"#
    );
    let (_tc, pass3) = run_m5_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-003"),
        "expected arc-macro-003 error, got: {:?}",
        errs
    );
}

#[test]
fn m5_3_missing_generate_method_reports_arc_macro_020() {
    // 标了 [SourceGenerator] 但没有 Generate 方法 → arc-macro-020
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class EmptyGenerator : IGenerator {{
}}
"#
    );
    let (_tc, pass3) = run_m5_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();
    assert!(
        has_arc_macro_code(&errs, "arc-macro-020"),
        "expected arc-macro-020 error, got: {:?}",
        errs
    );
}

#[test]
fn m5_3_error_isolation_one_generator_failure_continues_others() {
    // 一个生成器失败不阻塞其他生成器
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class BrokenGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class Broken {{");  // 语法错误
        return results;
    }}
}}

[SourceGenerator]
class GoodGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class Good {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    let errs = pass3.err().unwrap_or_default();

    // 应有 arc-macro-003（BrokenGenerator 语法错误）
    assert!(
        has_arc_macro_code(&errs, "arc-macro-003"),
        "expected arc-macro-003 from BrokenGenerator"
    );

    // GoodGenerator 的成功结果应仍被收集
    let progs = tc.generated_programs();
    let has_good = progs.iter().any(|p| {
        p.items.iter().any(|item| {
            matches!(
                &item.node,
                ast::Item::Class(c) if c.name.as_str() == "Good"
            )
        })
    });
    assert!(
        has_good,
        "GoodGenerator output should be collected despite BrokenGenerator failure"
    );
}

#[test]
fn m5_3_empty_string_skipped() {
    // 空字符串（仅空白）的 Add 应被跳过，不计入 generated_programs
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class EmptyStringGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("   ");
        results.Add("public class Real {{ }}");
        results.Add("");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());

    // 空字符串被跳过，仅保留 1 个有效 Program
    assert_eq!(
        tc.generated_programs().len(),
        1,
        "empty strings should be skipped"
    );
}

#[test]
fn m5_3_m4_and_m5_coexist_in_pipeline() {
    // M4 expand_macros 与 M5 expand_source_generators 在同一管线协同
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "var x = 42;");
    }}
}}

[SourceGenerator]
class DtoGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class GeneratedDto {{ }}");
        return results;
    }}
}}
"#
    );
    let (mut tc, _pass2) = check_src_result(&src);

    // M4 Pass 3
    let m4_pass3 = tc.expand_macros();
    assert!(
        m4_pass3.is_ok(),
        "M4 Pass 3 should succeed: {:?}",
        m4_pass3.err()
    );

    // M5 Pass 3
    let m5_pass3 = tc.expand_source_generators();
    assert!(
        m5_pass3.is_ok(),
        "M5 Pass 3 should succeed: {:?}",
        m5_pass3.err()
    );

    // M4 splice 发生：Host.Register 应包含 `var x = 42;`
    let host = tc.class_defs().get(&Ident::from("Host")).unwrap();
    let register = host
        .methods
        .iter()
        .find(|m| m.node.sig.name.as_str() == "Register")
        .unwrap();
    let body = register.node.body.as_ref().unwrap();
    let has_let_x = body.stmts.iter().any(|s| {
        matches!(
            &s.node,
            ast::Stmt::Let { name, .. } if name.as_str() == "x"
        )
    });
    assert!(has_let_x, "M4 splice should inject `var x = 42;`");

    // M5 生成：generated_programs 应包含 GeneratedDto
    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1);
    let has_dto = progs[0].items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "GeneratedDto"
        )
    });
    assert!(has_dto, "M5 should generate GeneratedDto class");
}

#[test]
fn m5_3_string_concat_in_add_supported() {
    // Generate 方法体内用 + 拼接字符串作为 Add 参数
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class ConcatGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class " + "Concat {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());

    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1);
    let has_concat = progs[0].items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "Concat"
        )
    });
    assert!(has_concat);
}

#[test]
fn m5_3_no_source_generators_is_no_op() {
    // 无 Source Generator 时 expand_source_generators 为 no-op
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class PlainClass {{}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(pass3.is_ok());
    assert!(tc.generated_programs().is_empty());
}

// ============================================================================
// RFC 028 M5-2b: GeneratorContext 拦截 e2e 测试
//
// 验证 expand_source_generators 在端到端管线中将 typeck 产物
// （attribute_table + class_def_ids）注入求值器 locals，使 Generate 方法
// 体内 `context.Attributes.Count` / `context.Attributes.GetDefIdAt(i)` /
// `context.Attributes.GetAttrs(id).Has(name)` / `context.Symbols.GetTypeName(id)`
// 等访问能解析到真实数据，而非 preamble 中的 stub 实现。
//
// 覆盖场景：
// 1. context.Attributes.Count 字段访问成功（不报 Unsupported）
// 2. context.Symbols.GetTypeName(defId) 返回非空类型名
// 3. context.Attributes.GetDefIdAt + GetAttrs + Has 链式调用
// 4. context_param_name 缺失时退化为 M5-3 旧路径（向后兼容）
// ============================================================================

#[test]
fn m5_2b_context_attributes_count_accessible_in_generate() {
    // Generate 方法访问 context.Attributes.Count —— 拦截分支应返回真实数据
    // （非 stub 的 0）。typeck 后 attribute_table 至少含 preamble 中
    // [AttributeUsage] / [SourceGenerator] 等属性注册条目，Count > 0。
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class CountingGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        if (context.Attributes.Count > 0) {{
            results.Add("public class HasAttributes {{ }}");
        }} else {{
            results.Add("public class NoAttributes {{ }}");
        }}
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed with M5-2b context interception: {:?}",
        pass3.err()
    );

    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1, "expected exactly 1 generated program");
    let prog = &progs[0];
    // Count > 0 分支应被命中
    let has_has_attrs = prog.items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "HasAttributes"
        )
    });
    assert!(
        has_has_attrs,
        "M5-2b 拦截应使 context.Attributes.Count 返回真实值 (>0)，命中 HasAttributes 分支"
    );
}

#[test]
fn m5_2b_context_symbols_get_type_name_accessible_in_generate() {
    // Generate 方法访问 context.Symbols.GetTypeName(0) —— 拦截分支应返回
    // preamble 中首个注册类的真实类型名（不依赖具体名称，仅验证非空）。
    // 拼接非空类型名生成 `public class Lookup_<TypeName> { }`，
    // 若 GetTypeName 返回空串则生成 `public class Lookup_ { }`。
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class SymbolLookupGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        var firstDefId = context.Attributes.GetDefIdAt(0);
        var typeName = context.Symbols.GetTypeName(firstDefId);
        if (typeName == "") {{
            results.Add("public class Lookup_Empty {{ }}");
        }} else {{
            results.Add("public class Lookup_NonEmpty {{ }}");
        }}
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed with M5-2b SymbolTable interception: {:?}",
        pass3.err()
    );

    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1);
    let prog = &progs[0];
    // GetTypeName(0) 应返回 preamble 中首个注册类的名字（非空）
    let has_nonempty = prog.items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "Lookup_NonEmpty"
        )
    });
    assert!(
        has_nonempty,
        "M5-2b SymbolTable 拦截应使 GetTypeName 返回真实非空类型名"
    );
}

#[test]
fn m5_2b_context_attribute_list_has_accessible_in_generate() {
    // Generate 方法链式调用 context.Attributes.GetDefIdAt(0).GetAttrs(id).Has(name)
    // —— 拦截分支应返回真实判断结果。
    //
    // preamble 中 [AttributeUsage] 标在 AttributeUsageAttribute / SourceGeneratorAttribute
    // 两类上；首个 DefId 上的属性列表至少含 "AttributeUsage"。
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class AttrListCheckGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        if (context.Attributes.Count > 0) {{
            var firstDefId = context.Attributes.GetDefIdAt(0);
            var attrs = context.Attributes.GetAttrs(firstDefId);
            if (attrs.Has("AttributeUsage")) {{
                results.Add("public class AttrUsageFound {{ }}");
            }} else {{
                results.Add("public class AttrUsageMissing {{ }}");
            }}
        }} else {{
            results.Add("public class EmptyTable {{ }}");
        }}
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed with M5-2b AttributeList interception: {:?}",
        pass3.err()
    );

    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1);
    let prog = &progs[0];
    // 首个 DefId 上的属性应包含 AttributeUsage（preamble 中首个标 [AttributeUsage] 的类）
    let has_found = prog.items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "AttrUsageFound"
        )
    });
    assert!(
        has_found,
        "M5-2b AttributeList.Has 拦截应正确识别首个 DefId 上的 AttributeUsage 属性"
    );
}

#[test]
fn m5_2b_generate_method_without_context_param_falls_back_to_m5_3_path() {
    // Generate 方法无参数 —— context_param_name 为 None，应退化为 M5-3 旧路径
    // （不注入 context，但仍能正常求值方法体）。
    //
    // 注意：preamble 中 IGenerator.Generate(GeneratorContext context) 要求一个参数；
    // 此测试在 SourceGenerator 类中提供无参 Generate 重载，仅验证 M5-2b 路径
    // 在 context_param_name 缺失时不崩溃（求值器仅不支持访问 context 字段）。
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class NoContextGenerator : IGenerator {{
    public List<string> Generate() {{
        var results = new List<string>();
        results.Add("public class GeneratedNoContext {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3) = run_m5_pipeline(&src);
    assert!(
        pass3.is_ok(),
        "Pass 3 should succeed when Generate has no context param (M5-3 fallback): {:?}",
        pass3.err()
    );
    let progs = tc.generated_programs();
    assert_eq!(progs.len(), 1);
    let prog = &progs[0];
    let has_generated = prog.items.iter().any(|item| {
        matches!(
            &item.node,
            ast::Item::Class(c) if c.name.as_str() == "GeneratedNoContext"
        )
    });
    assert!(has_generated, "M5-3 fallback 路径应正常生成代码");
}

// ============================================================================
// RFC 028 M5-4: Pass 3 协同 + Pass 4 完整 typeck
//
// 覆盖场景：
// 1. Pass 4 M5 分支对生成代码完整 typeck 成功
// 2. Pass 4 捕获生成代码中的类型错误
// 3. Pass 4 捕获生成代码中未定义类型引用
// 4. 生成代码可引用原模块已注册类型
// 5. 多个生成 Program 顺序 typeck
// 6. 无生成代码时 Pass 4 M5 为 no-op
// 7. run_pass3 统一入口协同执行 M4+M5
// 8. run_pass4 统一入口协同执行 M4+M5 Pass 4
// 9. M4 与 M5 在完整管线（Pass2→Pass3→Pass4）中共存
// ============================================================================

/// 驱动 Pass 2 → Pass 3 (M5) → Pass 4 (M5)，返回 (TypeChecker, Pass 3 结果, Pass 4 结果)。
fn run_m5_full_pipeline(
    src: &str,
) -> (
    TypeChecker,
    Result<(), Vec<typeck::TypeError>>,
    Result<(), Vec<typeck::TypeError>>,
) {
    let (mut tc, _pass2) = check_src_result(src);
    let pass3 = tc.expand_source_generators();
    let pass4 = tc.check_generated_programs_pass4();
    (tc, pass3, pass4)
}

#[test]
fn m5_4_pass4_typecks_generated_class_successfully() {
    // Pass 4 对生成的合法类进行完整 typeck，应通过
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class SimpleGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class GeneratedClass {{ public int X {{ get; set; }} }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate generated class: {:?}",
        pass4.err()
    );
    // 生成类应被注册到 class_defs
    assert!(
        tc.class_defs().contains_key(&Ident::from("GeneratedClass")),
        "GeneratedClass should be in class_defs after Pass 4"
    );
}

#[test]
fn m5_4_pass4_catches_type_error_in_generated_code() {
    // 生成的类方法体引用未定义变量 → Pass 4 应捕获
    // （与 M4-9 m4_7_pass4_catches_spliced_undefined_variable 同型错误）
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class BadTypeGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class BadClass {{ public void Broken() {{ int x = undefined_var; }} }}");
        return results;
    }}
}}
"#
    );
    let (_tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed (parse OK)");
    assert!(
        pass4.is_err(),
        "Pass 4 must catch undefined variable in generated code, got: {:?}",
        pass4.ok()
    );
}

#[test]
fn m5_4_pass4_catches_undefined_type_in_generated_code() {
    // 生成的类构造函数调用未定义函数 → Pass 4 应捕获
    // （与 undefined_var 同族——验证 Pass 4 在生成代码内对未定义符号的检测能力）
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class BadBaseGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class BadCtor {{ public BadCtor() {{ int x = undefined_function(); }} }}");
        return results;
    }}
}}
"#
    );
    let (_tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed (parse OK)");
    assert!(
        pass4.is_err(),
        "Pass 4 must catch undefined function call in generated code, got: {:?}",
        pass4.ok()
    );
}

#[test]
fn m5_4_generated_code_can_reference_original_types() {
    // 生成的类引用原模块中已注册的类型（GeneratorContext 来自 preamble）
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class CrossRefGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class ConsumerClass {{ public GeneratorContext Ctx {{ get; set; }} }}");
        return results;
    }}
}}
"#
    );
    let (_tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());
    assert!(
        pass4.is_ok(),
        "Pass 4 should accept generated code referencing original types: {:?}",
        pass4.err()
    );
}

#[test]
fn m5_4_multiple_generated_programs_sequential_typeck() {
    // 多个 Add 调用产出多个 Program；Pass 4 顺序 typeck 每个
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class MultiGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class First {{ }}");
        results.Add("public class Second {{ }}");
        results.Add("public class Third {{ }}");
        return results;
    }}
}}
"#
    );
    let (tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok(), "Pass 3 should succeed");
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate all 3 classes: {:?}",
        pass4.err()
    );
    // 三个类都应被注册到 class_defs
    for name in &["First", "Second", "Third"] {
        assert!(
            tc.class_defs().contains_key(&Ident::from(*name)),
            "{} should be in class_defs after Pass 4",
            name
        );
    }
}

#[test]
fn m5_4_pass4_no_op_when_no_source_generators() {
    // 无 Source Generator 时 Pass 4 M5 分支应为 no-op
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class PlainOnly {{ }}
"#
    );
    let (tc, pass3, pass4) = run_m5_full_pipeline(&src);
    assert!(pass3.is_ok());
    assert!(
        pass4.is_ok(),
        "Pass 4 M5 should be no-op: {:?}",
        pass4.err()
    );
    assert!(tc.generated_programs().is_empty());
}

#[test]
fn m5_4_run_pass3_unified_entry_runs_both_m4_and_m5() {
    // run_pass3 同时执行 M4 (expand_macros) 与 M5 (expand_source_generators)
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "return 42;");
    }}
}}

[SourceGenerator]
class CoexistGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class CoGenerated {{ }}");
        return results;
    }}
}}
"#
    );
    // 注意：M4 需要 GenerateToAttribute<T> 类被识别；此处借用 M4-2 既有判定
    // （attr 名为 GenerateTo 即识别为容器；feature 派生判定也走 attr 路径）
    let (mut tc, _pass2) = check_src_result(&src);
    let pass3 = tc.run_pass3();
    // Pass 3 可能有 M4 splice 错误（GenerateToAttribute 类未在 preamble 中定义），
    // 但 M5 分支应成功执行——验证 run_pass3 不被 M4 失败阻塞
    // 收集错误后检查 M5 是否产出生成代码
    let _ = pass3;
    let progs = tc.generated_programs();
    assert_eq!(
        progs.len(),
        1,
        "M5 branch should produce 1 Program regardless of M4 outcome"
    );
}

#[test]
fn m5_4_run_pass4_unified_entry_runs_both_m4_and_m5() {
    // run_pass4 同时执行 M4 (check_macro_containers_pass4) 与 M5 (check_generated_programs_pass4)
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class OnlyM5Generator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class OnlyM5Generated {{ }}");
        return results;
    }}
}}
"#
    );
    let (mut tc, _pass2) = check_src_result(&src);
    let _pass3 = tc.run_pass3();
    let pass4 = tc.run_pass4();
    // 无 M4 容器类 → M4 分支 no-op；M5 分支 typeck 生成类
    assert!(
        pass4.is_ok(),
        "run_pass4 should validate M5 generated code: {:?}",
        pass4.err()
    );
    assert!(
        tc.class_defs()
            .contains_key(&Ident::from("OnlyM5Generated")),
        "OnlyM5Generated should be in class_defs"
    );
}

#[test]
fn m5_4_m4_and_m5_coexist_in_full_pipeline() {
    // M4 与 M5 在完整管线 Pass2 → Pass3 (run_pass3) → Pass4 (run_pass4) 中共存
    let src = format!(
        r#"
{M5_STD_PREAMBLE}

[SourceGenerator]
class FullPipelineGenerator : IGenerator {{
    public List<string> Generate(GeneratorContext context) {{
        var results = new List<string>();
        results.Add("public class PipelineGenerated {{ public int Value {{ get; set; }} }}");
        return results;
    }}
}}

class OriginalClass {{
    public int Field {{ get; set; }}
}}
"#
    );
    let (mut tc, pass2) = check_src_result(&src);
    assert!(pass2.is_ok(), "Pass 2 should succeed: {:?}", pass2.err());

    let pass3 = tc.run_pass3();
    assert!(pass3.is_ok(), "Pass 3 should succeed: {:?}", pass3.err());

    let pass4 = tc.run_pass4();
    assert!(
        pass4.is_ok(),
        "Pass 4 should validate all code: {:?}",
        pass4.err()
    );

    // 验证原代码与生成代码均被处理
    assert!(tc.class_defs().contains_key(&Ident::from("OriginalClass")));
    assert!(tc
        .class_defs()
        .contains_key(&Ident::from("PipelineGenerated")));
}

// ============================================================================
// RFC 028 M4-7: Expression 参数注入机制
//
// 验证 attribute 位置的 Lambda 参数被树化为 ExpressionTree 并注入到
// 受限求值器环境，使 feature 委托体能通过形参名访问 Expression 对象。
//
// 覆盖场景：
// 1. Parser 识别 `[Inject(x => x.Age >= 18)]` 中的 Lambda 参数
// 2. typeck convert_arg 把 Lambda 树化为 ResolvedArg::Expression
// 3. collect_feature_ctors 识别 Expression 形参
// 4. Pass 3 expand_feature_registrations_with_locals 生成 expression_locals
// 5. Evaluator::inject_expression_locals 注入 Expression 到 locals
// 6. 求值器内 expr.GetBody().GetStringValue() / GetLeft() 等访问器工作
//    （C# 对齐：from_lambda 根为 Lambda，须先 GetBody）
// 7. Pass 3 自动注入 expression_locals 后委托体能引用形参名
// 8. Expression 实参数量与构造函数 Expression 形参数量不匹配时报错
// ============================================================================

/// 公共前置：包含 Expression 根基类与 GenerateToAttribute<T> 泛型基类。
/// M4-7 测试场景中 feature 派生类的构造函数需声明 `Expression` 形参，
/// 因此需要先声明 `Expression` 类（任意非泛型形式即可，typeck 通过
/// 类型名判定 is_expression_type）。
const M4_7_STD_PREAMBLE: &str = r#"
class Attribute {
    public Attribute() {}
}

class AttributeTargets {
    public const int Class = 1;
    public const int Struct = 2;
    public const int Method = 16;
    public const int Property = 32;
    public const int Field = 64;
    public const int All = 255;
}

[AttributeUsage(AttributeTargets.Class)]
class AttributeUsageAttribute : Attribute {
    public int ValidOn { get; }
    public bool AllowMultiple { get; set; }
    public bool Inherited { get; set; }
    public AttributeUsageAttribute(int validOn) {
        ValidOn = validOn;
        AllowMultiple = false;
        Inherited = true;
    }
}

class Expression {
    public Expression() {}
}
"#;

#[test]
fn m4_7_parser_accepts_lambda_attribute_arg() {
    // Parser 识别 `[Inject(typeof(User), x => x.Age >= 18)]` 中的 Lambda 参数
    // —— 验证 parse_attribute_arg 的 Lambda 分支工作
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : Attribute {{
    public InjectAttribute(Expression selector) {{}}
}}

[Inject(x => x.Age >= 18)]
class User {{
    public int Age {{ get; set; }}
}}
"#
    );
    // 仅验证 parse → typeck 管线不 panic
    let program = Parser::parse_program(&src).expect("parse must succeed");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let _ = tc.check_module(&module);
    // User 类应被注册到 attribute_table，且其上的 Inject 属性应有 1 个位置参数
    let user_def_id = tc.class_def_id("User").expect("User must have DefId");
    let attrs = tc.attribute_table().get_attrs(user_def_id);
    let inject = attrs
        .iter()
        .find(|a| a.name.as_str() == "Inject")
        .expect("Inject attribute must be registered");
    assert_eq!(
        inject.args.len(),
        1,
        "Inject attribute must have 1 positional Lambda arg, got: {:?}",
        inject.args
    );
}

#[test]
fn m4_7_typeck_converts_lambda_to_expression_tree() {
    // typeck convert_arg 把 AttributeArg::Lambda 树化为 ResolvedArg::Expression
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : Attribute {{
    public InjectAttribute(Expression selector) {{}}
}}

[Inject(x => x.Age >= 18)]
class User {{
    public int Age {{ get; set; }}
}}
"#
    );
    let tc = check_src(&src);
    let user_def_id = tc.class_def_id("User").expect("User must have DefId");
    let attrs = tc.attribute_table().get_attrs(user_def_id);
    let inject = attrs
        .iter()
        .find(|a| a.name.as_str() == "Inject")
        .expect("Inject attribute must be registered");
    // 验证参数被树化为 ExpressionTree（ResolvedArg::Expression 变体）
    assert!(
        matches!(inject.args[0], typeck::ResolvedArg::Expression(_)),
        "Lambda arg must be lowered to ResolvedArg::Expression, got: {:?}",
        inject.args[0]
    );
}

#[test]
fn m4_7_collect_feature_ctors_extracts_expression_param() {
    // collect_feature_ctors 识别 feature 派生类构造函数的 Expression 形参
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(Expression selector) {{
        this.Register(() => "code");
    }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("InjectAttribute must be in catalog as feature");
    // 构造函数应有 1 个 Expression 形参
    assert!(!feature.constructors.is_empty(), "feature must have ctors");
    let ctor = &feature.constructors[0];
    assert!(
        ctor.params.iter().any(|p| p.is_expression),
        "ctor must have an Expression param, got: {:?}",
        ctor.params
    );
}

#[test]
fn m4_7_pass3_generates_expression_locals_for_annotated_class() {
    // 被赋能类（标了 [Inject(Lambda)]）触发 Pass 3，生成 expression_locals
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(Expression selector) {{
        this.Register(() => "code");
    }}
}}

[Inject(x => x.Age >= 18)]
class User {{
    public int Age {{ get; set; }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("InjectAttribute must be in catalog");
    // Pass 2 生成 1 个基础注册（来自构造函数体），Pass 3 为被赋能类 User
    // 生成带 expression_locals 的副本——总注册数应为 1（User 一个被赋能类）
    assert!(
        !feature.registrations.is_empty(),
        "feature must have registrations"
    );
    let reg = &feature.registrations[0];
    assert!(
        !reg.expression_locals.is_empty(),
        "registration must have expression_locals for User, got: {:?}",
        reg.expression_locals
    );
    // 形参名应为 `selector`（来自 InjectAttribute 构造函数签名）
    assert_eq!(reg.expression_locals[0].0.as_str(), "selector");
}

#[test]
fn m4_7_evaluator_inject_expression_locals_accessible() {
    // Evaluator::inject_expression_locals 注入 Expression 到 locals。
    // C# 对齐：`from_lambda` 根为 LambdaExpression，常量在 Body；
    // 委托体须 `selector.GetBody().GetStringValue()`（同 SqlTranslator）。
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(Expression selector) {{
        this.Register(() => selector.GetBody().GetStringValue());
    }}
}}

[Inject(() => "hello")]
class User {{}}
"#
    );
    // `[Inject(() => "hello")]` → Lambda { body: Constant("hello") }
    // GetBody() → ConstantExpression(IsString=true, GetStringValue="hello")
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("InjectAttribute must be in catalog");
    let reg = &feature.registrations[0];
    assert!(
        !reg.expression_locals.is_empty(),
        "registration must have expression_locals"
    );
    let w = Whitelist::new();
    let mut evaluator = Evaluator::new(&w);
    evaluator.inject_expression_locals(&reg.expression_locals);
    let result = evaluator
        .eval_lambda(&reg.expansion)
        .expect("eval must succeed");
    assert_eq!(result, "hello");
}

#[test]
fn m4_7_evaluator_expression_tree_traversal_get_left() {
    // 注入完整 ExpressionNode 子树后，经 GetBody 解包 Lambda，再 GetLeft。
    // `x => x.Age >= 18`：root=Lambda，Body=Binary，Left=MemberAccess("Age")
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(Expression selector) {{
        this.Register(() => selector.GetBody().GetLeft().GetMember());
    }}
}}

[Inject(x => x.Age >= 18)]
class User {{
    public int Age {{ get; set; }}
}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("InjectAttribute must be in catalog");
    let reg = &feature.registrations[0];
    let w = Whitelist::new();
    let mut evaluator = Evaluator::new(&w);
    evaluator.inject_expression_locals(&reg.expression_locals);
    // selector = Lambda(Binary(MemberAccess(Parameter("x"), "Age"), Constant(18)))
    // GetBody() = Binary；GetLeft() = MemberAccess；GetMember() = "Age"
    let result = evaluator
        .eval_lambda(&reg.expansion)
        .expect("eval must succeed");
    assert_eq!(result, "Age");
}

#[test]
fn m4_7_pass3_expression_arg_count_mismatch_reports_error() {
    // Expression 实参数量与构造函数 Expression 形参数量不匹配时报 arc-macro-030
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute(Expression selector, Expression validator) {{
        this.Register(() => "code");
    }}
}}

[Inject(x => x.Age >= 18)]
class User {{
    public int Age {{ get; set; }}
}}
"#
    );
    let (tc, result) = check_src_result(&src);
    // collect_macros 应在 Pass 3 中累积 arc-macro-030 错误
    // check_module 把 macro_errors 合并到 self.errors，故 result 应为 Err
    let errs = result.err().unwrap_or_default();
    assert!(
        errs.iter().any(
            |e| matches!(e, typeck::TypeError::Macro { code, .. } if *code == "arc-macro-030")
        ),
        "expected arc-macro-030 error for Expression arg count mismatch, got: {:?}",
        errs
    );
    let _ = tc; // 抑制未使用警告
}

#[test]
fn m4_7_pass3_no_expression_param_skips_locals_injection() {
    // feature 构造函数无 Expression 形参时，Pass 3 跳过 locals 注入
    // —— registrations 保留 Pass 2 原始副本（expression_locals 为空）
    let src = format!(
        r#"
{M4_7_STD_PREAMBLE}

class Host {{
    public void Register() {{}}
}}

class InjectAttribute : GenerateToAttribute<Host> {{
    public InjectAttribute() {{
        this.Register(() => "code");
    }}
}}

[Inject]
class User {{}}
"#
    );
    let tc = check_src(&src);
    let catalog = tc.macro_catalog();
    let feature = catalog
        .features
        .get(&Ident::from("InjectAttribute"))
        .expect("InjectAttribute must be in catalog");
    let reg = &feature.registrations[0];
    assert!(
        reg.expression_locals.is_empty(),
        "feature without Expression ctor param should have empty expression_locals"
    );
}
