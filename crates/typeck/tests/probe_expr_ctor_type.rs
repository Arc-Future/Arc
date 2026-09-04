//! Probe: does Expression ctor with `Type Type` + `Type = null` land in typed_fns?
use hir::HirBuilder;
use parse::Parser;
use typeck::TypeChecker;

const SRC: &str = r#"
namespace Arc.Reflection {
    public abstract class MemberInfo {}
    public abstract class Type : MemberInfo {
        public abstract int TypeId { get; }
        public abstract string FullName { get; }
    }
}
namespace Arc.Linq.Expressions {
    using Arc.Reflection;
    public enum ExpressionType { Constant, Member, Call, Cast }
    public class Expression {
        public ExpressionType NodeType { get; set; }
        public Type Type { get; set; }
        public string TypeName { get; set; }
        public Expression() {
            NodeType = ExpressionType.Constant;
            Type = null;
            TypeName = "";
        }
        public virtual string GetMember() { return ""; }
    }
    public class MemberExpression : Expression {
        public Expression Object { get; set; }
        public string Member { get; set; }
        public MemberExpression() {
            NodeType = ExpressionType.Member;
            Member = "";
        }
    }
    public class MethodCallExpression : Expression {
        public string Method { get; set; }
        public MethodCallExpression() {
            NodeType = ExpressionType.Call;
            Method = "";
        }
    }
    public class CastExpression : Expression {
        public Expression Expr { get; set; }
        public string TargetType { get; set; }
        public CastExpression() {
            NodeType = ExpressionType.Cast;
            TargetType = "";
        }
    }
}
"#;

#[test]
fn probe_expression_ctors_in_typed_fns() {
    let program = Parser::parse_program(SRC).expect("parse");
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).expect("hir");
    let mut checker = TypeChecker::new();
    let typed = checker.check_module(&module).unwrap_or_else(|e| {
        panic!("typeck failed: {e:?}");
    });
    let names: Vec<String> = typed.iter().map(|f| f.name.to_string()).collect();
    println!("typed_fns ({}):", names.len());
    for n in &names {
        println!("  {n}");
        if n.contains("__ctor") {
            let f = typed.iter().find(|tf| tf.name.as_str() == n).unwrap();
            println!(
                "    body={:?} typed_body={} class_fields={:?}",
                f.body.as_ref().map(|b| b.stmts.len()),
                f.typed_body.is_some(),
                f.class_fields
            );
        }
    }
    assert!(
        names.iter().any(|n| n == "__ctor::Expression"),
        "missing __ctor::Expression; have: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "__ctor::MemberExpression"),
        "missing __ctor::MemberExpression; have: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "__ctor::MethodCallExpression"),
        "missing __ctor::MethodCallExpression; have: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "__ctor::CastExpression"),
        "missing __ctor::CastExpression; have: {names:?}"
    );
}
