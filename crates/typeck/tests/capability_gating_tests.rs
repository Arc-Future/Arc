//! RFC 027 M3 §3.4 能力 gating Phase 1+ 集成测试（[4.4 能力系统]）。
//!
//! 验证 namespace capability 声明与 native module capability 标签的协同：
//! - 声明对应能力的 namespace 可调用带 capability 的 native 方法
//! - 未声明对应能力的 namespace 调用应被 typeck 拒绝
//! - 子 namespace 继承父 namespace 的能力声明
//! - 无 capability 标签的 native module 兼容所有 namespace（Phase 0 行为）

use ast::*;
use hir::HirBuilder;
use parse::Parser;
use typeck::TypeChecker;

/// 构造带 `capability io` 标签的 native module `libcaptest`，仅含一个 `echo` 函数。
fn make_captest_native_module() -> NativeModule {
    NativeModule {
        name: "libcaptest".into(),
        functions: vec![NativeFn {
            name: "echo".into(),
            symbol: None,
            params: vec![NativeParam {
                name: "s".into(),
                ty: Spanned::new(
                    Type::Named {
                        path: vec!["string".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                ),
                direction: ParamDirection::default(),
            }],
            ret: Some(Spanned::new(
                Type::Named {
                    path: vec!["int".into()],
                    generics: vec![],
                },
                Span::DUMMY,
            )),
            calling_conv: CallingConv::default(),
        }],
        types: vec![],
        capability: Some("io".into()),
        library: None,
        library_env_var: None,
        source: None,
        load: LoadStrategy::Static,
        callbacks: vec![],
    }
}

#[test]
fn capability_gating_allows_call_when_namespace_declares_cap() {
    // namespace 带 `capability io` 声明 → 调用带 `capability io` 的 native 应通过
    let src = r#"
namespace myapp capability io {
    public class Driver {
        public void Run() {
            libcaptest.echo("hi");
        }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();

    let mut tc = TypeChecker::new();
    tc.register_native_modules(&[make_captest_native_module()]);
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "expected capability check to pass, got errors: {:?}",
        result.err()
    );
}

#[test]
fn capability_gating_rejects_call_when_namespace_missing_cap() {
    // namespace 无 capability 声明 → 调用带 `capability io` 的 native 应被拒绝
    let src = r#"
namespace myapp {
    public class Driver {
        public void Run() {
            libcaptest.echo("hi");
        }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();

    let mut tc = TypeChecker::new();
    tc.register_native_modules(&[make_captest_native_module()]);
    let result = tc.check_module(&module);
    assert!(
        result.is_err(),
        "expected capability check to fail, but check_module succeeded"
    );
    let errs = result.unwrap_err();
    let combined: String = errs.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        combined.contains("io") && combined.contains("libcaptest"),
        "expected error mentioning capability `io` and module `libcaptest`, got: {combined}"
    );
}

#[test]
fn capability_gating_inherits_parent_namespace_cap() {
    // 父 namespace 声明 io，子 namespace（无声明）应继承 → 调用通过
    let src = r#"
namespace myapp capability io {
    namespace inner {
        public class Driver {
            public void Run() {
                libcaptest.echo("hi");
            }
        }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();

    let mut tc = TypeChecker::new();
    tc.register_native_modules(&[make_captest_native_module()]);
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "expected inherited capability to pass, got errors: {:?}",
        result.err()
    );
}

#[test]
fn capability_gating_phase0_compat_no_native_cap() {
    // 无 capability 标签的 native module 兼容所有 namespace（Phase 0 行为）
    let native = NativeModule {
        name: "libplain".into(),
        functions: vec![NativeFn {
            name: "echo".into(),
            symbol: None,
            params: vec![NativeParam {
                name: "s".into(),
                ty: Spanned::new(
                    Type::Named {
                        path: vec!["string".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                ),
                direction: ParamDirection::default(),
            }],
            ret: Some(Spanned::new(
                Type::Named {
                    path: vec!["int".into()],
                    generics: vec![],
                },
                Span::DUMMY,
            )),
            calling_conv: CallingConv::default(),
        }],
        types: vec![],
        capability: None,
        library: None,
        library_env_var: None,
        source: None,
        load: LoadStrategy::Static,
        callbacks: vec![],
    };
    let src = r#"
namespace plain {
    public class Driver {
        public void Run() {
            libplain.echo("hi");
        }
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();

    let mut tc = TypeChecker::new();
    tc.register_native_modules(&[native]);
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "Phase 0 compat: no-capability native should pass any namespace, got: {:?}",
        result.err()
    );
}

#[test]
fn capability_gating_file_scoped_namespace_with_cap() {
    // file-scoped namespace + capability 声明
    let src = r#"
namespace myapp.io capability io;

public class Driver {
    public void Run() {
        libcaptest.echo("hi");
    }
}
"#;
    let program = Parser::parse_program(src).unwrap();
    let mut hir = HirBuilder::new();
    let module = hir.lower_program(&program).unwrap();

    let mut tc = TypeChecker::new();
    tc.register_native_modules(&[make_captest_native_module()]);
    let result = tc.check_module(&module);
    assert!(
        result.is_ok(),
        "file-scoped namespace with capability should pass, got: {:?}",
        result.err()
    );
}
