//! Dump Expression subclass field offsets (with vs without Expression.Type).
use hir::HirBuilder;
use parse::Parser;
use typeck::{layouts_from_registry, TypeRegistry};

fn layouts_for(src: &str) -> typeck::ProgramLayouts {
    let program = Parser::parse_program(src).expect("parse");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).expect("hir");
    let reg = TypeRegistry::from_module(&module);
    layouts_from_registry(&reg)
}

fn dump(label: &str, src: &str) {
    let layouts = layouts_for(src);
    println!("\n======== {label} ========");
    for name in [
        "Expression",
        "ConstantExpression",
        "MemberExpression",
        "BinaryExpression",
        "UnaryExpression",
        "CastExpression",
        "LambdaExpression",
        "MethodCallExpression",
        "NewExpression",
    ] {
        let Some(layout) = layouts.get(name) else {
            println!("{name}: MISSING from layouts");
            continue;
        };
        println!("=== {name} ===");
        for f in &layout.fields {
            println!("  {:>4}  {:<16}  {}", f.offset, f.name, f.ty);
        }
        // Probe field_info-style lookup for critical fields
        for probe in [
            "Type",
            "TypeName",
            "MemberName",
            "Left",
            "Right",
            "Operand",
            "TargetType",
            "Expr",
            "Body",
            "Arg0",
            "NodeType",
        ] {
            let found = layout.fields.iter().find(|f| f.name == probe);
            if let Some(f) = found {
                println!("  lookup {probe} -> offset {}", f.offset);
            }
        }
    }
}

#[test]
fn dump_expression_field_offsets() {
    let with_type = r#"
namespace Arc.Reflection {
    public abstract class MemberInfo {}
    public abstract class Type : MemberInfo {
        public abstract int TypeId { get; }
        public abstract string FullName { get; }
    }
}
namespace Arc.Linq.Expressions {
    using Arc.Reflection;
    public enum ExpressionType {
        Constant, Parameter, Capture, Member, Index, Binary, Unary,
        Conditional, Call, New, Lambda, Cast
    }
    public class Expression {
        public ExpressionType NodeType { get; set; }
        public Type Type { get; set; }
        public string TypeName { get; set; }
    }
    public class ConstantExpression : Expression {
        public int IntValue;
        public double FloatValue;
        public bool BoolValue;
        public string StringValue;
        public bool IsString;
    }
    public class MemberExpression : Expression {
        public Expression Object { get; set; }
        public string MemberName { get; set; }
    }
    public class BinaryExpression : Expression {
        public Expression Left { get; set; }
        public Expression Right { get; set; }
    }
    public class UnaryExpression : Expression {
        public Expression Operand { get; set; }
    }
    public class CastExpression : Expression {
        public Expression Expr { get; set; }
        public string TargetType { get; set; }
    }
    public class LambdaExpression : Expression {
        public Expression Body { get; set; }
    }
    public class MethodCallExpression : Expression {
        public string MethodName { get; set; }
        public Expression Target { get; set; }
        public Expression Arg0 { get; set; }
    }
    public class NewExpression : Expression {
        public string TypeName { get; set; }
        public Expression ArgValues { get; set; }
    }
}
"#;

    let without_type = r#"
namespace Arc.Reflection {
    public abstract class MemberInfo {}
    public abstract class Type : MemberInfo {
        public abstract int TypeId { get; }
        public abstract string FullName { get; }
    }
}
namespace Arc.Linq.Expressions {
    using Arc.Reflection;
    public enum ExpressionType {
        Constant, Parameter, Capture, Member, Index, Binary, Unary,
        Conditional, Call, New, Lambda, Cast
    }
    public class Expression {
        public ExpressionType NodeType { get; set; }
        public string TypeName { get; set; }
    }
    public class ConstantExpression : Expression {
        public int IntValue;
        public double FloatValue;
        public bool BoolValue;
        public string StringValue;
        public bool IsString;
    }
    public class MemberExpression : Expression {
        public Expression Object { get; set; }
        public string MemberName { get; set; }
    }
    public class BinaryExpression : Expression {
        public Expression Left { get; set; }
        public Expression Right { get; set; }
    }
    public class UnaryExpression : Expression {
        public Expression Operand { get; set; }
    }
    public class CastExpression : Expression {
        public Expression Expr { get; set; }
        public string TargetType { get; set; }
    }
    public class LambdaExpression : Expression {
        public Expression Body { get; set; }
    }
    public class MethodCallExpression : Expression {
        public string MethodName { get; set; }
        public Expression Target { get; set; }
        public Expression Arg0 { get; set; }
    }
    public class NewExpression : Expression {
        public string TypeName { get; set; }
        public Expression ArgValues { get; set; }
    }
}
"#;

    dump("WITH Expression.Type", with_type);
    dump("WITHOUT Expression.Type", without_type);

    // Assert Type is present and shifts TypeName by 8
    let layouts = layouts_for(with_type);
    let expr = layouts.get("Expression").expect("Expression layout");
    let type_field = expr.fields.iter().find(|f| f.name == "Type");
    let type_name = expr.fields.iter().find(|f| f.name == "TypeName");
    assert!(type_field.is_some(), "Expression.Type must be in layout");
    assert_eq!(type_field.unwrap().offset, 24);
    assert_eq!(type_name.unwrap().offset, 32);

    let layouts_old = layouts_for(without_type);
    let expr_old = layouts_old.get("Expression").unwrap();
    let tn_old = expr_old
        .fields
        .iter()
        .find(|f| f.name == "TypeName")
        .unwrap();
    assert_eq!(tn_old.offset, 24, "without Type, TypeName should be at 24");

    // NewExpression: derived TypeName must not create a second slot
    let new_expr = layouts.get("NewExpression").unwrap();
    let tn_count = new_expr
        .fields
        .iter()
        .filter(|f| f.name == "TypeName")
        .count();
    assert_eq!(
        tn_count, 1,
        "NewExpression must not duplicate TypeName field"
    );
}
