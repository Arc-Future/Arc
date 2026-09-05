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

    #[test]
    fn soft_match_nested_func_param_against_unbound_lambda() {
        // Chord OnWaterfall 形态：handler 形参为嵌套 Func
        // `Func<object, Func<object,object>, object>`，实参 λ 未绑定
        // （`Func_Infer_Infer_Infer`）。旧 arity=None 回溯按 count 升序取首解，
        // 把嵌套组误切作 ret（arity 1）→ 软匹配零候选 → 回退首签名错绑
        // （expected 2 / found 3）。须以实参 λ 元数为目标 arity 重解析期望签名。
        let src = r#"
public class ChordContext {
    public void OnWaterfall(string name, object handler) { }
    public void OnWaterfall(string name, Func_object_Func_object_object_object handler) { }
    public void OnWaterfall(string name, Func_object_Func_object_object_object handler, bool prepend) { }
}
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let reg = TypeRegistry::from_module(&module);
        let ctx = AccessContext::none();
        // 直接单测 func_name_infer_compatible 语义（经 soft 解析的 3 参候选）。
        let result = reg.resolve_method_overload_lambda_soft(
            &"ChordContext".into(),
            &"OnWaterfall".into(),
            &[
                "string".into(),
                "Func_Infer_Infer_Infer".into(),
                "bool".into(),
            ],
            &ctx,
        );
        match result {
            Ok((_, sig)) => {
                assert_eq!(
                    sig.params.len(),
                    3,
                    "expected 3-param overload to win, got {sig:?}"
                );
            }
            Err(e) => panic!("soft resolve failed: {e:?}"),
        }
    }

    #[test]
    fn generic_template_link_picks_func_form_for_unbound_lambda() {
        // Chord Provide 形态：`Provide<T>(T)` 与 `Provide<T>(Func<T>)` 双泛型
        // 重载；调用 `app.Provide<Greeter>(() => …)` 实参为未绑定 λ
        //（`Func_Infer`）。模板唯一匹配须按 λ 软兼容命中 Func<T> 形——否则
        // 回退替换后基底（`Provide_Func_Greeter`）+ 后缀与 mono 模板名
        //（`Provide_Func_T__Greeter`）分叉 → arc-prune-001。
        let src = r#"
public class Greeter { }
public class ChordContext {
    public IDisposable Provide<T>(T instance) where T : class { return null; }
    public IDisposable Provide<T>(Func_T factory) where T : class { return null; }
}
public interface IDisposable { }
"#;
        let program = Parser::parse_program(src).unwrap();
        let mut hir = HirBuilder::new();
        let module = hir.lower_program(&program).unwrap();
        let reg = TypeRegistry::from_module(&module);
        let ctx = AccessContext::none();
        let base = reg
            .method_generic_template_link_name(
                &"ChordContext".into(),
                &"Provide".into(),
                &["Func_Infer".into()],
                &["Greeter".into()],
                &ctx,
            )
            .unwrap_or_else(|| {
                reg.method_generic_template_link_name_by_arity(
                    &"ChordContext".into(),
                    &"Provide".into(),
                    1,
                    1,
                    &ctx,
                )
                .unwrap_or_else(|| panic!("no template base found for Provide<Greeter>(lambda)"))
            });
        assert!(
            base.ends_with("Provide_Func_T"),
            "expected placeholder base `..._Provide_Func_T`, got {base}"
        );
    }
}
