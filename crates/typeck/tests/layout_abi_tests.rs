//! Layout ABI SSoT：bool → int32_t；struct 字段 offset 与 C ABI 一致。
use hir::HirBuilder;
use parse::Parser;
use typeck::{abi_size_of, layouts_from_registry, TypeRegistry};

fn layouts_for(src: &str) -> typeck::ProgramLayouts {
    let program = Parser::parse_program(src).expect("parse");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).expect("hir");
    let reg = TypeRegistry::from_module(&module);
    layouts_from_registry(&reg)
}

fn registry_for(src: &str) -> TypeRegistry {
    let program = Parser::parse_program(src).expect("parse");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).expect("hir");
    TypeRegistry::from_module(&module)
}

#[test]
fn struct_bool_then_int_offsets_match_c_abi() {
    let src = r#"
public struct FlagInt {
    public bool Flag;
    public int Value;
}
"#;
    let layouts = layouts_for(src);
    let s = layouts.structs.get("FlagInt").expect("FlagInt");
    let flag = s.fields.iter().find(|f| f.name == "Flag").expect("Flag");
    let value = s.fields.iter().find(|f| f.name == "Value").expect("Value");
    assert_eq!(flag.offset, 0, "bool at offset 0");
    assert_eq!(value.offset, 4, "int after bool(int32_t) at offset 4");
    let reg = registry_for(src);
    assert_eq!(abi_size_of(&reg, "FlagInt"), 8);
}

#[test]
fn class_bool_field_size_is_four() {
    let src = r#"
public class HasBool {
    public bool Ready;
    public int Count;
}
"#;
    let layouts = layouts_for(src);
    let c = layouts.classes.get("HasBool").expect("HasBool");
    let ready = c.fields.iter().find(|f| f.name == "Ready").expect("Ready");
    let count = c.fields.iter().find(|f| f.name == "Count").expect("Count");
    // header 16 + Ready@16 (bool=4) → Count@20
    assert_eq!(ready.offset, 16);
    assert_eq!(count.offset, 20);
}

/// CD-30 批处理扩容（阶段 A）：跨命名空间同名类经 layout 层物化为可区分条目。
///
/// 两个 namespace 各有 `class Shape`：胜者保留短名键 `classes["Shape"]`（主查找
/// 不变），碰撞输家按其 FQN 物化出独立 ClassLayout 且 `name` 即其 FQN——两个
/// 同名类的类型身份层归属可唯一标识（符号/反射寻址由阶段 C 按 FQN 完成）。
#[test]
fn cross_namespace_same_class_distinguishable() {
    let src = r#"
namespace BatchX.Case1 {
    public class Shape {
        public string Who() { return "case1"; }
    }
}
namespace BatchX.Case2 {
    public class Shape {
        public string Who() { return "case2"; }
    }
}
"#;
    let layouts = layouts_for(src);
    // 胜者保留短名键。
    let winner = layouts
        .classes
        .get("Shape")
        .expect("winner keeps short-name key in classes");
    assert_eq!(winner.name, "Shape");
    // 碰撞输家按 FQN 物化（键含命名空间），且自描述其命名空间归属。
    let fqn_keys: Vec<_> = layouts
        .classes
        .keys()
        .filter(|k| k.as_str() != "Shape" && k.as_str().contains('.'))
        .collect();
    assert_eq!(fqn_keys.len(), 1, "exactly one FQN-keyed shadowed class");
    let loser = layouts
        .classes
        .get(fqn_keys[0].as_str())
        .expect("shadowed class materialized under FQN");
    assert_eq!(loser.name.as_str(), fqn_keys[0].as_str());
    // 两个同名类并存且可区分。
    assert_eq!(layouts.classes.len(), 2);
    assert!(layouts.classes.get("Shape").is_some());
    assert!(layouts.classes.get(&winner.name).is_some());
    assert_ne!(winner.name.as_str(), loser.name.as_str());
    // 各自独立声明其方法（互不串用）。
    assert!(winner.declared_methods.iter().any(|m| m.name == "Who"));
    assert!(loser.declared_methods.iter().any(|m| m.name == "Who"));
}
