//! Static overload resolution for Assert-like facades.

#[cfg(test)]
mod tests {
    use ast::Ident;
    use hir::HirBuilder;
    use parse::Parser;

    use crate::oop_types::AccessContext;
    use crate::TypeRegistry;

    #[test]
    fn resolve_equal_string_not_int() {
        let src = r#"
public static class Assert {
    public static void Equal(int expected, int actual) { }
    public static void Equal(long expected, long actual) { }
    public static void Equal(string expected, string actual) { }
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let reg = TypeRegistry::from_module(&module);
        let assert_ty: Ident = "Assert".into();
        let equal: Ident = "Equal".into();
        let nom = reg.types.get(&assert_ty).expect("Assert registered");
        let sigs = nom.methods.get(&equal).expect("Equal overloads");
        assert_eq!(sigs.len(), 3, "expected 3 Equal overloads, got {sigs:?}");

        let ctx = AccessContext::none();
        let (decl, sig) = reg
            .resolve_method_overload(
                &assert_ty,
                &equal,
                &["string".into(), "string".into()],
                &ctx,
            )
            .expect("string Equal should resolve");
        assert_eq!(decl.as_str(), "Assert");
        assert_eq!(sig.params[0].ty.as_str(), "string");
        assert_eq!(sig.params[1].ty.as_str(), "string");

        let (decl, sig) = reg
            .resolve_method_overload(&assert_ty, &equal, &["int".into(), "int".into()], &ctx)
            .expect("int Equal should resolve");
        assert_eq!(decl.as_str(), "Assert");
        assert_eq!(sig.params[0].ty.as_str(), "int");
    }

    #[test]
    fn typeck_assert_equal_string_call() {
        let src = r#"
public static class Assert {
    public static void Equal(int expected, int actual) { }
    public static void Equal(string expected, string actual) { }
}
void Main() {
    Assert.Equal("hi", "hi");
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let mut tc = crate::TypeChecker::new();
        let err = tc.check_module(&module);
        assert!(err.is_ok(), "typeck failed: {err:?}");
    }

    #[test]
    fn typeck_assert_equal_string_with_delta_overload() {
        // Equal(double,double,double) 更高 arity 曾触发 RFC 007 try_bind；
        // 与 string 双参重载并存时仍须正确解析。
        let src = r#"
public static class Assert {
    public static void Equal(int expected, int actual) { }
    public static void Equal(string expected, string actual) { }
    public static void Equal(double expected, double actual, double delta) { }
}
void Main() {
    Assert.Equal("hi", "hi");
    Assert.Equal(1, 1);
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let mut tc = crate::TypeChecker::new();
        let err = tc.check_module(&module);
        assert!(err.is_ok(), "typeck failed: {err:?}");
    }

    #[test]
    fn resolve_throws_three_arg_with_func_infer_lambda() {
        // 无目标类型 lambda → Func_Infer；须匹配 Action≡Func_void 的三参重载。
        let src = r#"
public static class Assert {
    public static void Throws(string actionName, Action action) { }
    public static void Throws(string errorCode, string actionName, Action action) { }
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let reg = TypeRegistry::from_module(&module);
        let assert_ty: Ident = "Assert".into();
        let throws: Ident = "Throws".into();
        let ctx = AccessContext::none();
        let (decl, sig) = reg
            .resolve_method_overload_lambda_soft(
                &assert_ty,
                &throws,
                &["string".into(), "string".into(), "Func_Infer".into()],
                &ctx,
            )
            .expect("3-arg Throws with Func_Infer should soft-resolve");
        assert_eq!(decl.as_str(), "Assert");
        assert_eq!(
            sig.params.len(),
            3,
            "expected 3-param overload, got {sig:?}"
        );
    }

    #[test]
    fn typeck_assert_throws_three_arg_lambda() {
        let src = r#"
public class Exception {
    public string Message;
    public Exception(string m) { Message = m; }
}
public static class Assert {
    public static void Throws(string actionName, Action action) { }
    public static void Throws(string errorCode, string actionName, Action action) { }
}
void Main() {
    Assert.Throws("x", "boom", () => { throw new Exception("x"); });
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let mut tc = crate::TypeChecker::new();
        let err = tc.check_module(&module);
        assert!(err.is_ok(), "typeck failed: {err:?}");
    }

    #[test]
    fn static_class_generic_method_pushes_typed_fn_template() {
        // `static class` 泛型方法走 push_typed_fn（非入 extension_fn_templates），
        // 否则 MIR 无法从 `Assert::Empty` 克隆 `Assert::Empty__int`。
        let src = r#"
public class List<T> {
    public int Count;
}
public static class Assert {
    public static void Empty<T>(List<T> collection) { }
}
void Main() {
    List<int> xs = new List<int>();
    Assert.Empty<int>(xs);
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let mut tc = crate::TypeChecker::new();
        let err = tc.check_module(&module);
        assert!(err.is_ok(), "typeck failed: {err:?}");
        assert!(
            tc.typed_fns()
                .iter()
                .any(|f| f.name.as_str() == "Assert::Empty"),
            "expected Assert::Empty typed_fn template, got names: {:?}",
            tc.typed_fns()
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            !tc.extension_fn_templates
                .keys()
                .any(|k| k.as_str().contains("Empty")),
            "non-extension generic static method must not go to extension_fn_templates"
        );
    }

    #[test]
    fn static_generic_method_infers_type_args_from_list_arg() {
        // `Assert.Empty(xs)` 无显式 type arg：从 `List_int` 推断 `T=int`，
        // 并回写 MethodCall.type_args 供 MIR mono。
        let src = r#"
public class List<T> {
    public int Count;
}
public static class Assert {
    public static void Empty<T>(List<T> collection) { }
}
void Main() {
    List<int> xs = new List<int>();
    Assert.Empty(xs);
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let mut tc = crate::TypeChecker::new();
        let err = tc.check_module(&module);
        assert!(err.is_ok(), "typeck failed: {err:?}");
    }

    #[test]
    fn static_generic_all_infers_despite_func_infer_lambda() {
        // 第二参 Func_Infer_bool：Infer 段不绑定，从 List_int 定 T。
        let src = r#"
public class List<T> {
    public int Count;
}
public static class Assert {
    public static void All<T>(List<T> collection, Func_T_bool predicate) { }
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let reg = TypeRegistry::from_module(&module);
        let ctx = AccessContext::none();
        let result = reg.resolve_method_infer_type_args(
            &"Assert".into(),
            &"All".into(),
            &["List_int".into(), "Func_Infer_bool".into()],
            &ctx,
        );
        match result {
            Ok((_, sig, targs)) => {
                assert_eq!(targs, vec![Ident::from("int")]);
                assert_eq!(sig.params[0].ty.as_str(), "List_int");
                assert_eq!(sig.params[1].ty.as_str(), "Func_int_bool");
            }
            Err(e) => {
                let nom = reg.types.get(&Ident::from("Assert")).unwrap();
                let sigs = nom.methods.get(&Ident::from("All")).unwrap();
                panic!(
                    "infer failed: {e:?}; registered All={:?}",
                    sigs.iter()
                        .map(|s| {
                            (
                                s.generics.clone(),
                                s.params.iter().map(|p| p.ty.clone()).collect::<Vec<_>>(),
                            )
                        })
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}
