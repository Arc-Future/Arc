//! L1 依赖管理测试：包依赖 / 传递依赖 / 包内可见性 / InternalsVisibleTo。
//!
//! 注意：编译器当前用包名校验源码 namespace。包名用 `_` 命名以匹配源码标识符。

use arc_tests::{assert_compiles_multipackage, assert_compiles_project};

#[test]
fn test_single_package_no_deps() {
    assert_compiles_multipackage(
        "dep_single",
        &[(
            "main",
            "main",
            "[package]\nname = \"dep_single\"\nedition = \"1\"\n\n[dependencies]\n",
            vec![("Program.as", "void Main() {}")],
        )],
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes cross-package method refs"]
fn test_two_packages_static_dep() {
    assert_compiles_multipackage(
        "dep_two_static",
        &[
            (
                "main",
                "main",
                "[package]\nname = \"dep_two_main\"\nedition = \"1\"\n\n[dependencies]\n\"dep_two_lib\" = { path = \"../lib\" }\n",
                vec![("Program.as", "using dep_two_lib; void Main() { var v = Math.Double(21); }")],
            ),
            (
                "lib",
                "lib",
                "[package]\nname = \"dep_two_lib\"\nedition = \"1\"\n\n[dependencies]\n",
                vec![("Math.as", "namespace dep_two_lib; public static class Math { public static int Double(int x) => x * 2; }")],
            ),
        ],
    );
}

#[test]
#[ignore = "GAP #11: transitive dep import resolution incomplete + reachability prune"]
fn test_three_packages_transitive() {
    assert_compiles_multipackage(
        "dep_transitive",
        &[
            (
                "main",
                "main",
                "[package]\nname = \"dep_tr_main\"\nedition = \"1\"\n\n[dependencies]\n\"dep_tr_lib\" = { path = \"../lib\" }\n",
                vec![("Program.as", "using dep_tr_lib; void Main() { var r = Calculator.Compute(6); }")],
            ),
            (
                "lib",
                "lib",
                "[package]\nname = \"dep_tr_lib\"\nedition = \"1\"\n\n[dependencies]\n\"dep_tr_util\" = { path = \"../util\" }\n",
                vec![("Calculator.as", "namespace dep_tr_lib; using dep_tr_util; public static class Calculator { public static int Compute(int x) => Math.Square(x); }")],
            ),
            (
                "util",
                "util",
                "[package]\nname = \"dep_tr_util\"\nedition = \"1\"\n\n[dependencies]\n",
                vec![("Math.as", "namespace dep_tr_util; public static class Math { public static int Square(int x) => x * x; }")],
            ),
        ],
    );
}

#[test]
fn test_package_internal_visibility() {
    let manifest = "[package]\nname = \"dep_internal\"\nedition = \"1\"\n\n[dependencies]\n";
    assert_compiles_multipackage(
        "dep_internal",
        &[(
            "main",
            "main",
            manifest,
            vec![
                ("Program.as", "using dep_internal; void Main() { var pub = PublicType.Name(); var priv = InternalType.Secret(); }"),
                ("Types.as", "namespace dep_internal; public static class PublicType { public static string Name() => \"pub\"; } internal static class InternalType { internal static string Secret() => \"internal\"; }"),
            ],
        )],
    );
}

#[test]
fn test_internals_visible_to() {
    let manifest = "\
[package]
name = \"dep_ivt\"
edition = \"1\"
internals_visible_to = [\"dep_ivt_test\"]

[dependencies]
";
    assert_compiles_project(
        "dep_ivt",
        &[
            ("Program.as", "using dep_ivt; void Main() { var s = InternalData.Get(); }"),
            ("InternalData.as", "namespace dep_ivt; internal static class InternalData { internal static string Get() => \"visible\"; }"),
        ],
        manifest,
    );
}

#[test]
#[ignore = "GAP: sibling namespace must match package name (C# allows any namespace in project files)"]
fn test_package_namespace_isolation() {
    assert_compiles_multipackage(
        "dep_ns_iso",
        &[(
            "main",
            "main",
            "[package]\nname = \"dep_ns_iso\"\nedition = \"1\"\n\n[dependencies]\n",
            vec![
                ("Program.as", "using Models; using Data; void Main() { var m = User.GetName(); var d = User.GetId(); }"),
                ("Models.as", "namespace Models; public static class User { public static string GetName() => \"model\"; }"),
                ("Data.as", "namespace Data; public static class User { public static int GetId() => 42; }"),
            ],
        )],
    );
}
