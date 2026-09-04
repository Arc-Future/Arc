//! 原生链接封装（RFC 017 源码打包：单 TU 全静态链接）。
//!
//! 用户 `.o` + runtime + native → 可执行二进制；`arc test` harness 链接同源。
//! 不感知 QIF / 测试 / 项目语义——通用链接基础设施。

use std::path::{Path, PathBuf};

use thiserror::Error;

/// 链接错误类型。
#[derive(Debug, Error)]
pub enum LinkError {
    #[error("link failed: {0}")]
    Link(String),
}

/// 链接单对象为可执行二进制：用户 `.o` → exe（含 runtime + native）。
pub fn link_executable(
    user_obj: &Path,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    release: bool,
    debug_info: bool,
    native_modules: &[ast::NativeModule],
    native_lib_paths: &[PathBuf],
) -> Result<(), LinkError> {
    let objs = [user_obj.to_path_buf()];
    let target_str = target.map(|t| t.as_str());
    codegen::link_objects_to_executable(
        &objs,
        output,
        obj_dir,
        target_str,
        release,
        debug_info,
        native_modules,
        native_lib_paths,
    )
    .map_err(|e| LinkError::Link(e.to_string()))?;
    Ok(())
}

/// 链接测试 harness 二进制（RFC 032 测试库 + harness 二进制分离模型）。
///
/// 输入：
/// - `test_obj`：测试源码编译产出的 `.o`
/// - `output`：输出可执行二进制路径
/// - `obj_dir`：中间产物目录（runtime `.o` 缓存）
/// - `target` / `release` / `debug_info`：与 `arc build` 一致
/// - `native_lib_paths`：native 库搜索路径（与 `arc build` 一致）
///
/// 委托 [`link_executable`]（native_modules 为空：harness 链接阶段不重新解析
/// native 契约，库搜索路径 + 平台默认标志已足够）。
pub fn link_test_harness(
    test_obj: &Path,
    output: &Path,
    obj_dir: Option<&Path>,
    target: Option<&crate::target::TargetTriple>,
    release: bool,
    debug_info: bool,
    native_lib_paths: &[PathBuf],
) -> Result<(), LinkError> {
    link_executable(
        test_obj,
        output,
        obj_dir,
        target,
        release,
        debug_info,
        &[],
        native_lib_paths,
    )
}
