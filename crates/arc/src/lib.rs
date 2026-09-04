pub mod arcgr;
pub mod archive;
pub mod clang_version;
pub mod components;
pub mod doctor;
pub mod download;
pub mod env;
pub mod equipment;
pub mod fs_util;
pub mod generic_templates;
pub mod hash;
pub mod incremental;
pub mod inspect;
pub mod linker;
pub mod loader;
pub mod manifest;
pub mod overview;
pub mod package_graph;
pub mod pipeline;
pub mod publish;
pub mod query;
pub mod release;
pub mod release_sign;
pub mod scaffold;
pub mod self_update;
pub mod target;
pub mod toolchain;
pub mod version;
pub mod workspace;

pub use equipment::{
    ArtifactEmitter, ArtifactRequest, CodegenArtifactEmitter, CompileScheduler, DependencyResolver,
    EmitRole, Equipments, LoaderPackageContext, PackageContext, ParallelScheduler,
    PipelineTestHost, ProjectManager, SerialScheduler, TestHost, WorkspaceDependencyResolver,
    WorkspaceProjectManager,
};
pub use incremental::{
    compute_fingerprint, compute_fingerprint_inputs, compute_incremental_report,
    format_incremental_report, is_up_to_date, is_up_to_date_tagged, record_build,
    record_build_tagged, FingerprintInputs, IncrementalReport,
};
pub use loader::load_compile_unit as load;
pub use loader::{find_workspace_root, load_compile_unit, CompileUnit, LoadError};
pub use manifest::{
    find_arc_manifest, require_arc_manifest, resolve_effective_std_root, resolve_std_root,
    ArcManifest, CompilerSection, DependencySpec, ManifestError, QifSection, StdSection, UiSection,
    WorkspaceSection,
};
pub use package_graph::{ClosureError, PackageGraph, PackageNode};
pub use pipeline::{
    compile_file, compile_file_to_dynamic_library, compile_file_with_native, compile_source,
    compile_source_with_native, compile_test_file, compile_test_project, project_has_tests,
    CompileOptions, FieldCyclePolicy, QifCompileOptions,
};
pub use scaffold::{
    detect_project, format_detect_human, scaffold_project, ProjectInfo, ProjectType,
    ScaffoldOptions, ScaffoldReport,
};
pub use workspace::{Workspace, WorkspaceMember};

/// 包元数据——嵌入动态库供运行时版本校验。
pub use codegen::PackageMeta;
/// 项目类型——可执行程序 vs 库（编译期固定规则，对标 C#）
pub use codegen::ProjectKind;

#[cfg(test)]
mod test_mutex;
#[cfg(test)]
pub(crate) use test_mutex::ENV_TEST_MUTEX;
