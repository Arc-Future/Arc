//! L1 项目管理体系测试：arc.toml / namespace / global_usings / 包元数据。
//!
//! 注意：编译器当前用包名校验源码 namespace（对标 C# 项目默认 namespace）。
//! manifest 的 `namespace` 字段已解析但未用于校验——这是一个 GAP。

use arc_tests::assert_compiles_project;

fn default_manifest(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n")
}

#[test]
fn test_arc_toml_default() {
    let manifest = default_manifest("proj_mgmt_default");
    assert_compiles_project(
        "proj_mgmt_default",
        &[("Program.as", "void Main() {}")],
        &manifest,
    );
}

#[test]
fn test_package_name_version() {
    let manifest = "\
[package]
name = \"proj-mgmt-ver\"
version = \"1.2.3\"
edition = \"1\"

[dependencies]
";
    assert_compiles_project(
        "proj_mgmt_ver",
        &[("Program.as", "void Main() {}")],
        manifest,
    );
}

#[test]
fn test_package_namespace_explicit() {
    // manifest namespace 字段解析（已验证解析，是否生效待 GAP 修复）
    let manifest = "\
[package]
name = \"proj-mgmt-ns\"
namespace = \"MyApp.Core\"
edition = \"1\"

[dependencies]
";
    assert_compiles_project(
        "proj_mgmt_ns",
        &[("Program.as", "void Main() {}")],
        manifest,
    );
}

#[test]
#[ignore = "GAP: library kind linker still expects main entry on Windows"]
fn test_package_kind_library() {
    let manifest = "\
[package]
name = \"proj-mgmt-kind\"
kind = \"library\"
edition = \"1\"

[dependencies]
";
    assert_compiles_project("proj_mgmt_kind", &[("Program.as", "")], manifest);
}

#[test]
fn test_global_usings_single() {
    let manifest = "\
[package]
name = \"proj-mgmt-gu1\"
edition = \"1\"
global_usings = [\"Arc\"]

[dependencies]
";
    assert_compiles_project(
        "proj_mgmt_gu1",
        &[("Program.as", "void Main() { }")],
        manifest,
    );
}

#[test]
fn test_global_usings_multiple() {
    let manifest = "\
[package]
name = \"proj-mgmt-gu2\"
edition = \"1\"
global_usings = [\"Arc\", \"Arc.Collections\"]

[dependencies]
";
    assert_compiles_project(
        "proj_mgmt_gu2",
        &[("Program.as", "void Main() { }")],
        manifest,
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes class constructors in multi-file"]
fn test_multi_file_simple() {
    let manifest = default_manifest("proj_mgmt_multi");
    assert_compiles_project(
        "proj_mgmt_multi",
        &[
            (
                "Program.as",
                "void Main() { var m = new MathOps(); var r = m.Add(3, 4); }",
            ),
            (
                "MathOps.as",
                "public class MathOps { public int Add(int a, int b) => a + b; }",
            ),
        ],
        &manifest,
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes class constructors in multi-file"]
fn test_multi_file_separate_names() {
    let manifest = default_manifest("proj_mgmt_sep");
    assert_compiles_project(
        "proj_mgmt_sep",
        &[
            (
                "Program.as",
                "void Main() { var h = new Helper(); h.Work(); }",
            ),
            ("Helper.as", "public class Helper { public void Work() {} }"),
            (
                "Utils.as",
                "public static class Utils { public static string Greet() => \"hi\"; }",
            ),
        ],
        &manifest,
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes cross-file method refs"]
fn test_fully_qualified_name() {
    // 跨文件：兄弟文件声明与包名一致的 namespace
    let manifest = default_manifest("proj_mgmt_fqn");
    assert_compiles_project(
        "proj_mgmt_fqn",
        &[
            ("Program.as", "using proj_mgmt_fqn; void Main() { var x = Math.Square(5); }"),
            ("Settings.as", "namespace proj_mgmt_fqn; public static class Math { public static int Square(int x) => x * x; }"),
        ],
        &manifest,
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes cross-file method refs"]
fn test_multi_file_struct() {
    let manifest = default_manifest("proj_mgmt_struct");
    assert_compiles_project(
        "proj_mgmt_struct",
        &[
            ("Program.as", "using proj_mgmt_struct; void Main() { var p = new Point(); var s = ShapeInfo.Describe(p); }"),
            ("Geometry.as", "namespace proj_mgmt_struct; public struct Point { public int X; public int Y; } public static class ShapeInfo { public static string Describe(Point p) => $\"({p.X}, {p.Y})\"; }"),
        ],
        &manifest,
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes cross-file method refs"]
fn test_multi_file_enum() {
    let manifest = default_manifest("proj_mgmt_enum");
    assert_compiles_project(
        "proj_mgmt_enum",
        &[
            ("Program.as", "using proj_mgmt_enum; void Main() { var d = Direction.North; var n = Directions.Count(); }"),
            ("Enums.as", "namespace proj_mgmt_enum; public enum Direction { North, South, East, West } public static class Directions { public static int Count() => 4; }"),
        ],
        &manifest,
    );
}

#[test]
fn test_package_meta_all_fields() {
    let manifest = "\
[package]
name = \"proj-mgmt-full\"
version = \"0.1.0\"
namespace = \"Demo.App\"
edition = \"1\"
global_usings = [\"Arc\"]

[dependencies]
";
    assert_compiles_project(
        "proj_mgmt_full",
        &[("Program.as", "void Main() {}")],
        manifest,
    );
}
