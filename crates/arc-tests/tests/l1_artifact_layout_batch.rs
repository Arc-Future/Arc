//! L1 编译产物布局测试：obj/Debug 结构、产物路径、中间文件。
//!
//! 注意：涉及 `new Class()` 构造函数的测试可能触发可达性裁剪 bug（GAP #11），
//! 多文件类构造函数相关测试标记为 `#[ignore]`。

use std::path::Path;

use arc_tests::{assert_compiles_project, workspace_root};

#[test]
fn test_artifact_obj_debug_created() {
    let name = "artifact_obj_debug";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    assert_compiles_project(name, &[("Program.as", "void Main() {}")], &manifest);
    let dir = workspace_root().join("target/arc-tests").join(name);
    let obj_dir = dir.join("obj/Debug");
    assert!(
        obj_dir.exists(),
        "{name}: obj/Debug directory should exist after compilation"
    );
}

#[test]
fn test_artifact_binary_created() {
    let name = "artifact_binary";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    assert_compiles_project(name, &[("Program.as", "void Main() {}")], &manifest);
    let dir = workspace_root().join("target/arc-tests").join(name);
    let binary_name = if cfg!(windows) {
        "proj_main.exe"
    } else {
        "proj_main"
    };
    let binary = dir.join(binary_name);
    assert!(
        binary.exists(),
        "{name}: binary should exist after compilation"
    );
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes class constructors in multi-file"]
fn test_artifact_multi_file_layout() {
    let name = "artifact_multi";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    assert_compiles_project(
        name,
        &[
            ("Program.as", "void Main() { var h = new Helper(); }"),
            ("Helper.as", "public class Helper { public void Work() {} }"),
        ],
        &manifest,
    );
    let dir = workspace_root().join("target/arc-tests").join(name);
    let obj_dir = dir.join("obj/Debug");
    assert!(obj_dir.exists(), "obj/Debug should exist");
}

#[test]
fn test_artifact_clean_build() {
    let name = "artifact_clean";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    let dir = workspace_root().join("target/arc-tests").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    assert_compiles_project(name, &[("Program.as", "void Main() {}")], &manifest);
    assert!(dir.exists(), "project directory should exist");
    let obj_dir = dir.join("obj/Debug");
    assert!(obj_dir.exists(), "obj/Debug should exist");
}

#[test]
fn test_artifact_rebuild_idempotent() {
    let name = "artifact_rebuild";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    assert_compiles_project(name, &[("Program.as", "void Main() {}")], &manifest);
    assert_compiles_project(name, &[("Program.as", "void Main() {}")], &manifest);
    let dir = workspace_root().join("target/arc-tests").join(name);
    let binary_name = if cfg!(windows) {
        "proj_main.exe"
    } else {
        "proj_main"
    };
    assert!(Path::new(&dir).join(binary_name).exists());
}

#[test]
#[ignore = "GAP #11: reachability prune incorrectly removes cross-file method refs"]
fn test_artifact_namespaced_project() {
    let name = "artifact_ns";
    let manifest = format!("[package]\nname = \"{name}\"\nedition = \"1\"\n\n[dependencies]\n");
    assert_compiles_project(
        name,
        &[("Program.as", "using artifact_ns; void Main() { var x = Math.Add(1, 2); }"),
          ("Math.as", "namespace artifact_ns; public static class Math { public static int Add(int a, int b) => a + b; }")],
        &manifest,
    );
    let dir = workspace_root().join("target/arc-tests").join(name);
    assert!(dir.exists());
    let obj_dir = dir.join("obj/Debug");
    assert!(obj_dir.exists());
}
