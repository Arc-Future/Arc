//! RFC 102 里程碑④：编译期声明级字段环检测（`arc-cycle-001` warning 通道）单元测试。
//!
//! 通过 parse → hir → typeck 完整管线驱动检测器，用合成小 class 集覆盖：
//! - 合成互环（A→B→A）被标记
//! - 自环（A.A）被标记
//! - `Weak<T>` 字段断环不标记（弱引用不断强环——硬要求）
//! - 门面 / 基元字段不标记
//! - 基类继承边成环被标记
//! - 无环不误报
//!
//! 测试用合成夹具而非 std 的 `Element.Parent`（后者可能被并发修改）。

use hir::HirBuilder;
use parse::Parser;
use typeck::{TypeChecker, TypeWarning};

/// 驱动 parse → hir → typeck 管线，返回 `TypeChecker`（含 warnings）。
fn check_src(src: &str) -> TypeChecker {
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();
    let mut tc = TypeChecker::new();
    let _ = tc.check_module(&module);
    tc
}

fn warnings_of(tc: &TypeChecker) -> Vec<&TypeWarning> {
    tc.warnings()
        .iter()
        .filter(|w| w.code == "arc-cycle-001")
        .collect()
}

/// 全部 `arc-cycle-001` 消息拼接，供断言。
fn warning_texts(tc: &TypeChecker) -> Vec<String> {
    warnings_of(tc).iter().map(|w| w.message.clone()).collect()
}

#[test]
fn mutual_cycle_a_b_flagged() {
    let tc = check_src(
        r#"
class A {
    public B B;
}
class B {
    public A A;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        !texts.is_empty(),
        "A→B→A 声明级互环必须被标记，实际无 warning: {:?}",
        texts
    );
    assert!(
        texts.iter().any(|m| m.contains("A.B → B.A → A")),
        "消息应体现 A.B → B.A → A，实际: {:?}",
        texts
    );
}

#[test]
fn self_cycle_flagged() {
    let tc = check_src(
        r#"
class A {
    public A Next;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts.iter().any(|m| m.contains("A.Next")),
        "自环 A.Next → A 必须被标记，实际: {:?}",
        texts
    );
}

#[test]
fn weak_field_does_not_form_cycle() {
    // Weak<T> 字段是弱引用，不断强环——仅 Weak 字段的类不得被标记。
    let tc = check_src(
        r#"
class Weak<T> {
    public int _target;
}
class Node {
    public Weak<Node> Prev;
    public Weak<Node> Next;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts.is_empty(),
        "仅 Weak<T> 字段不应构成环，实际: {:?}",
        texts
    );
}

#[test]
fn weak_field_does_not_hide_real_cycle() {
    // Weak 字段跳过，但强引用环 A↔B 仍须被标记。
    let tc = check_src(
        r#"
class Weak<T> {
    public int _target;
}
class A {
    public Weak<B> WeakRef;
    public B B;
}
class B {
    public A A;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts.iter().any(|m| m.contains("A.B → B.A")),
        "Weak 字段不得掩盖真实强引用环，实际: {:?}",
        texts
    );
}

#[test]
fn facade_and_primitive_not_flagged() {
    let tc = check_src(
        r#"
class Facade {
    public int _handle;
}
class P {
    public int X;
    public string S;
    public object O;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts.is_empty(),
        "门面 / 基元 / object 字段不构成 class 环，实际: {:?}",
        texts
    );
}

#[test]
fn inheritance_edge_cycle_flagged() {
    // 基类继承边（Base → Derived 经字段，Derived → Base 经继承）成环。
    let tc = check_src(
        r#"
class Base {
    public Derived D;
}
class Derived : Base {
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts
            .iter()
            .any(|m| m.contains("Base.D") && m.contains("Derived（基类）")),
        "基类继承边成环必须被标记，实际: {:?}",
        texts
    );
}

#[test]
fn static_members_not_flagged() {
    // 静态字段 / 静态自动属性是类级根，不构成实例级强引用环（registry 将静态
    // 自动属性注册为 is_static=false，检测器须经 AST 补判排除）。
    let tc = check_src(
        r#"
class Holder {
    public static Holder Instance { get; }
    public static Holder Cache;
    public int Value;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(
        texts.is_empty(),
        "静态字段/静态自动属性不得被标记为实例级字段环，实际: {:?}",
        texts
    );
}

#[test]
fn acyclic_no_false_positive() {
    let tc = check_src(
        r#"
class A {
    public B B;
}
class B {
    public int X;
}
class C {
    public string S;
}
"#,
    );
    let texts = warning_texts(&tc);
    assert!(texts.is_empty(), "无环不应误报，实际: {:?}", texts);
}
