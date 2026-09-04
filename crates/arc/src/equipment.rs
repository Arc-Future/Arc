//! 管线装备架构（RFC 013 §管线装备架构）。
//!
//! 编译主流程（`pipeline`）只编排「段轴」；本模块承载被视为**正交横切片**的
//! 能力装备，以窄 trait（SPI）注入主流程，避免能力硬编码耦合于巨型单体。
//! 装配点（composition root）经 [`Equipments`] 默认构造注入具体实现；测试
//! 用例可注入替身装备，隔离验证主流程段序。
//!
//! 本切片实现 RFC 013 装备清单的全套六件装备：**P1 项目管理**、**P2 依赖解析**、
//! **P3 包引用上下文**、**P4 编译调度**、**P5 产物发射**、**P6 测试宿主**。默认实现
//! 均为对既有逻辑的薄委托 / 串行调度（行为不变）；并行调度为可见、可注入的独立
//! 策略。装备化让横切能力脱离 `pipeline` 巨型单体、可替换、可独立测试。
//!
//! 红线（RFC 013）：新增能力必须走装备接口，`pipeline` 不得再出现硬编码
//! if/else 分支式的多路径；装备只能承载一个正交责任，不做无关动态分派。

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

use crate::loader::{load_compile_unit, CompileUnit};
use crate::workspace::Workspace;

/// 项目管理装备（P1）：从任意起点枚举/探测工程布局，定位目标成员。
///
/// 默认实现为 [`Workspace::discover`] / [`Workspace::member_index_of`] 的薄委托。
pub trait ProjectManager {
    /// 从起点（文件或目录）向上解析最近 workspace；无聚合 → `Ok(None)`（纯单项目）。
    fn discover(&self, start: &Path) -> Result<Option<Workspace>, String>;

    /// 定位起点路径命中的 workspace 成员下标（`None` = 起点即 workspace 根）。
    fn locate_member(&self, ws: &Workspace, path: &Path) -> Option<usize>;
}

/// 依赖解析装备（P2）：由依赖图产出拓扑构建顺序。
///
/// `target` 命中成员时交回该成员的 ProjectReference 闭包序（含自身）；`None`
/// 表示全量 workspace 拓扑序。默认实现为 [`Workspace::build_order`] 与
/// [`Workspace::closure_order`] 的薄委托。
pub trait DependencyResolver {
    /// 依依赖图返回成员构建顺序（被依赖者先出）。
    fn order(&self, ws: &Workspace, target: Option<usize>) -> Result<Vec<usize>, String>;
}

/// 默认项目管理装备：委托 `Workspace` 既有能力。
pub struct WorkspaceProjectManager;

impl ProjectManager for WorkspaceProjectManager {
    fn discover(&self, start: &Path) -> Result<Option<Workspace>, String> {
        Workspace::discover(start)
    }

    fn locate_member(&self, ws: &Workspace, path: &Path) -> Option<usize> {
        ws.member_index_of(path)
    }
}

/// 默认依赖解析装备：委托 `Workspace` 既有依赖图。
pub struct WorkspaceDependencyResolver;

impl DependencyResolver for WorkspaceDependencyResolver {
    fn order(&self, ws: &Workspace, target: Option<usize>) -> Result<Vec<usize>, String> {
        match target {
            Some(i) => ws.closure_order(i),
            None => ws.build_order(),
        }
    }
}

/// 编译调度装备（P4）：将若干编译单元按依赖/拓扑分派到串行或并行执行。
///
/// `order` 为 P2 产出的构建序（被依赖者先出）；`dep_on` 为成员→直接依赖成员
/// 的下标表（`dep_on[i]` 含 i 依赖的所有成员 j，j=0..n-1）。`step(i)` 执行单个
/// 编译单元。默认串行保证确定性（RFC 013 确定性要求）；并行策略仅令**无依赖边**
/// 的互不依赖单元并发，依赖序始终是一致性边界，并行不得改变构建序契约。
pub trait CompileScheduler {
    /// 依拓扑序与依赖表调度执行；任一单元失败 → 首个错误（并行等待在途任务收敛）。
    ///
    /// `step` 捕获须 `Send + Sync`，以便并行策略跨 worker 共享只读调用。
    fn run(
        &self,
        order: &[usize],
        dep_on: &[Vec<usize>],
        step: &(dyn Fn(usize) -> Result<(), String> + Send + Sync),
    ) -> Result<(), String>;
}

/// 串行调度（默认）：严格按拓扑序逐个执行，fail-fast，确定性优先。
pub struct SerialScheduler;

impl CompileScheduler for SerialScheduler {
    fn run(
        &self,
        order: &[usize],
        _dep_on: &[Vec<usize>],
        step: &(dyn Fn(usize) -> Result<(), String> + Send + Sync),
    ) -> Result<(), String> {
        for &i in order {
            step(i)?;
        }
        Ok(())
    }
}

/// 并行调度：依赖感知的并行执行，仅并发**互不依赖**的编译单元。
///
/// 保守策略即 RFC 语义：某单元就绪（其全部直接依赖已完成）方可派发；`jobs`
/// 为最大 worker 数。拓扑有效前提下不会死锁（空转时让步）。
pub struct ParallelScheduler {
    /// 最大并发 worker 数。
    pub jobs: usize,
}

impl ParallelScheduler {
    /// 构造并行调度器；`jobs < 1` 视为 1。
    pub fn with_jobs(jobs: usize) -> Self {
        Self { jobs: jobs.max(1) }
    }
}

impl CompileScheduler for ParallelScheduler {
    fn run(
        &self,
        order: &[usize],
        dep_on: &[Vec<usize>],
        step: &(dyn Fn(usize) -> Result<(), String> + Send + Sync),
    ) -> Result<(), String> {
        let m = order.len();
        if m == 0 {
            return Ok(());
        }
        // 成员索引 → order 位置，用于就绪计数（仅计同时入序的成员；跨序依赖视为已满足）。
        let pos: BTreeMap<usize, usize> = order
            .iter()
            .copied()
            .enumerate()
            .map(|(p, i)| (i, p))
            .collect();
        let mut prereq = vec![0usize; m];
        let mut dependents = vec![Vec::<usize>::new(); m];
        for (p, &i) in order.iter().enumerate() {
            for &j in &dep_on[i] {
                if let Some(&pj) = pos.get(&j) {
                    prereq[p] += 1;
                    dependents[pj].push(p);
                }
            }
        }

        // 共享调度状态：就绪队列 + 完成计数 + 首错；worker 经 Mutex 互斥更新。
        struct State {
            ready: VecDeque<usize>,
            prereq: Vec<usize>,
            dependents: Vec<Vec<usize>>,
            done: usize,
            err: Option<String>,
        }
        let state = Mutex::new(State {
            ready: prereq
                .iter()
                .enumerate()
                .filter(|(_, &c)| c == 0)
                .map(|(p, _)| p)
                .collect(),
            prereq,
            dependents,
            done: 0,
            err: None,
        });
        let workers = self.jobs.min(m).max(1);

        // 编译在主线程以 64MB 栈执行（见 main.rs `main()`：typeck 的
        // `check_expr_inner_impl` 大型 match 在 debug 下栈帧较大，默认线程栈
        // 在深度嵌套下会溢出）。worker 线程并行执行同一编译路径，必须沿用
        // 同等栈，否则跨 worker 并发编译大型成员（如 `using Arc` 拉入 std）
        // 会栈溢出。
        thread::scope(|scope| {
            for _ in 0..workers {
                thread::Builder::new()
                    .stack_size(64 * 1024 * 1024)
                    .spawn_scoped(scope, || loop {
                        let job = {
                            let mut g = state.lock().unwrap();
                            if g.err.is_some() || g.done == m {
                                return;
                            }
                            g.ready.pop_front()
                        };
                        match job {
                            Some(p) => {
                                let r = step(order[p]);
                                let mut g = state.lock().unwrap();
                                match r {
                                    Ok(()) => {
                                        g.done += 1;
                                        // 取出依赖关系表，令下方循环不再借用 guard（Deref 不透明，无法字段级拆分借用）。
                                        let deps = g.dependents[p].clone();
                                        for &d in &deps {
                                            g.prereq[d] -= 1;
                                            if g.prereq[d] == 0 {
                                                g.ready.push_back(d);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        if g.err.is_none() {
                                            g.err = Some(e);
                                        }
                                    }
                                }
                            }
                            None => thread::yield_now(),
                        }
                    })
                    .expect("failed to spawn parallel worker");
            }
        });

        match state.into_inner().unwrap().err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// 包引用上下文装备（P3）：为每个编译单位装配完整引用上下文（`CompileUnit`）。
///
/// 涵盖文件↔包映射、`internals_visible_to`、global usings、native 契约与外部符号
/// 的合成；是 typeck/codegen 的引用上下文来源。默认实现为
/// [`load_compile_unit`] 的薄委托（同时支撑 `arc.toml` 项目目录分支）。
pub trait PackageContext {
    /// 从起点装配编译单元；错误以字符串呈现（对上层统一无需 `LoadError` 特判）。
    fn load(&self, path: &Path) -> Result<CompileUnit, String>;
}

/// 默认包引用上下文装备：委托 `loader::load_compile_unit`（含项目目录分支）。
pub struct LoaderPackageContext;

impl PackageContext for LoaderPackageContext {
    fn load(&self, path: &Path) -> Result<CompileUnit, String> {
        load_compile_unit(path).map_err(|e| format!("load error: {e}"))
    }
}

/// 产物发射角色（RFC 013 P5）：决定发射路径——可执行主程序 或 动态库。
///
/// 对应 [`codegen::emit_role::EmitRole`] 的 MainObject / DynamicLibrary 两形态；
/// `LibraryObject`（`.ao` 发布路径）属阶段 4 产物收口删撤内容，不进本装备契约。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitRole {
    /// 可执行主对象：`codegen::compile_module`（`main` 入口按 `ProjectKind` 判定豁免）。
    MainObject,
    /// 动态库：`codegen::compile_module_to_dynamic_library`（`-shared` + `-fPIC`，无入口点）。
    DynamicLibrary,
}

/// 产物发射请求：发射装备（P5）消费的完整入参——对 `codegen::compile_module*`
/// 收发，平铺携带跨装备共享的编译上下文（主流程装配后经此传递，不建立装备间依赖）。
pub struct ArtifactRequest<'a> {
    /// MIR 函数体（名称 → CFG）。
    pub fns: &'a [(String, mir::MirCfgBody)],
    /// 类型布局。
    pub layouts: &'a typeck::ProgramLayouts,
    /// 输出二进制/库路径。
    pub output: &'a Path,
    /// 中间产物目录。
    pub obj_dir: Option<&'a Path>,
    /// 目标三元组。
    pub target: Option<&'a str>,
    /// 发布（优化）开关。
    pub release: bool,
    /// 源文件路径标示。
    pub file_path: &'a str,
    /// 源码文本（供调试符号）。
    pub source: &'a str,
    /// 调试信息开关。
    pub debug_info: bool,
    /// 函数源码散布表。
    pub fn_spans: &'a HashMap<String, ast::Span>,
    /// native 契约模块。
    pub native_modules: &'a [ast::NativeModule],
    /// native 库搜索路径。
    pub native_lib_paths: &'a [PathBuf],
    /// 跨包外部符号。
    pub external_symbols: &'a [typeck::ExternalSymbolEntry],
    /// 发射角色（决定 MainObject / DynamicLibrary 路径）。
    pub role: EmitRole,
    /// 项目类型（MainObject 路径：`main` 豁免判定）。
    pub project_kind: codegen::ProjectKind,
    /// 动态库导出符号列表（DynamicLibrary 路径）。
    pub export_symbols: &'a [String],
    /// 动态库包元数据（DynamicLibrary 路径）。
    pub package_meta: Option<codegen::PackageMeta>,
    /// RFC 017 产物域：clang 成功后保留文本 IR（`--emit-llvm`），默认焚毁。
    pub keep_ir: bool,
}

/// 产物发射装备（P5）：依 [`EmitRole`] 发射 `.ll` → 目标文件 → 链接。
///
/// 默认实现 [`CodegenArtifactEmitter`] 委托 `codegen::compile_module*`（内部完成
/// LLVM IR 文本 → clang 目标文件 → 链接）。发射是代码生成的收口装备——主流程仅
/// 编排段序与渲染静态初始化诊断，不持有发射细节。
pub trait ArtifactEmitter {
    /// 递交一次产物发射；成功返回静态初始化诊断（由上层渲染），失败以字符串呈现。
    fn emit(&self, request: ArtifactRequest) -> Result<Vec<codegen::StaticInitDiagnostic>, String>;
}

/// 默认产物发射装备：委托 `codegen` 的既有主程序/动态库编译路径。
pub struct CodegenArtifactEmitter;

impl ArtifactEmitter for CodegenArtifactEmitter {
    fn emit(&self, request: ArtifactRequest) -> Result<Vec<codegen::StaticInitDiagnostic>, String> {
        let diags = match request.role {
            EmitRole::DynamicLibrary => codegen::compile_module_to_dynamic_library(
                request.fns,
                request.layouts,
                request.output,
                request.obj_dir,
                request.target,
                request.release,
                request.file_path,
                request.source,
                request.debug_info,
                request.fn_spans,
                request.native_modules,
                request.native_lib_paths,
                &Default::default(),
                request.external_symbols,
                request.export_symbols,
                request.package_meta,
                request.keep_ir,
            ),
            EmitRole::MainObject => codegen::compile_module(
                request.fns,
                request.layouts,
                request.output,
                request.obj_dir,
                request.target,
                request.release,
                request.file_path,
                request.source,
                request.debug_info,
                request.fn_spans,
                request.native_modules,
                request.native_lib_paths,
                &Default::default(),
                request.external_symbols,
                request.project_kind,
                request.keep_ir,
            ),
        }
        .map_err(|e| format!("codegen error: {e}"))?;
        Ok(diags)
    }
}

/// 测试宿主装备（P6）：合成 `__QifTestHost::Main`——Fact/Theory 收集、Order/filter、
/// fixture、并行/串行调度（RFC 013 P6 契约）。
///
/// 默认实现 [`PipelineTestHost`] 委托 `pipeline::generate_qif_test_main`（薄委托
/// 零行为变更）；装备化让测试模式宿主合成可独立替换/测试，脱离主流程硬编码。
pub trait TestHost {
    /// 由已收集的 QIF 测试方法合成宿主源码字符串。
    fn generate(
        &self,
        methods: &[crate::pipeline::QifTestMethod],
        opts: &crate::pipeline::QifCompileOptions,
    ) -> String;
}

/// 默认测试宿主装备：委托既有 `pipeline::generate_qif_test_main`。
pub struct PipelineTestHost;

impl TestHost for PipelineTestHost {
    fn generate(
        &self,
        methods: &[crate::pipeline::QifTestMethod],
        opts: &crate::pipeline::QifCompileOptions,
    ) -> String {
        crate::pipeline::generate_qif_test_main(methods, opts)
    }
}

/// 装备束：管线装配点的默认构造（composition root 由此注入）。
///
/// 字段为装备 trait 对象，可行替换；`Default` 装配既有实现，对外行为不变。
pub struct Equipments {
    /// 项目管理装备（P1）。
    pub project: Box<dyn ProjectManager>,
    /// 依赖解析装备（P2）。
    pub resolve: Box<dyn DependencyResolver>,
    /// 包引用上下文装备（P3）。
    pub context: Box<dyn PackageContext>,
    /// 编译调度装备（P4）：默认串行，确定性优先；并行经 bundle 替换注入。
    pub schedule: Box<dyn CompileScheduler>,
    /// 产物发射装备（P5）：默认委托 `codegen` 既主程序/动态库路径。
    pub emitter: Box<dyn ArtifactEmitter>,
    /// 测试宿主装备（P6）：合成 `__QifTestHost::Main`。
    pub host: Box<dyn TestHost>,
}

impl Default for Equipments {
    fn default() -> Self {
        Self {
            project: Box::new(WorkspaceProjectManager),
            resolve: Box::new(WorkspaceDependencyResolver),
            context: Box::new(LoaderPackageContext),
            schedule: Box::new(SerialScheduler),
            emitter: Box::new(CodegenArtifactEmitter),
            host: Box::new(PipelineTestHost),
        }
    }
}

impl Equipments {
    /// 装配默认装备束（P1 项目管理 · P2 依赖解析 · P3 打包上下文 · P4 串行调度 · P5 产物发射 · P6 测试宿主）。
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 替身依赖解析装备：验证装备可替换性（主流程只依赖 trait 不依赖实现）。
    struct StubDependencyResolver;

    impl DependencyResolver for StubDependencyResolver {
        fn order(&self, _ws: &Workspace, target: Option<usize>) -> Result<Vec<usize>, String> {
            Ok(match target {
                Some(i) => vec![i],
                None => vec![],
            })
        }
    }

    #[test]
    fn default_bundle_assembles_workspace_equipments() {
        let equipments = Equipments::new();
        // 默认装备确保类型正确装配；具体解析行为由 Workspace 测试覆盖。
        assert!(equipments
            .project
            .discover(Path::new("/non/existent"))
            .is_ok());
    }

    #[test]
    fn resolver_is_replaceable_via_bundle() {
        let equipments = Equipments {
            project: Box::new(WorkspaceProjectManager),
            resolve: Box::new(StubDependencyResolver),
            context: Box::new(LoaderPackageContext),
            schedule: Box::new(SerialScheduler),
            emitter: Box::new(CodegenArtifactEmitter),
            host: Box::new(PipelineTestHost),
        };
        // 空 workspace：替身装备按 target 直接交回（`None`→空序）。
        let ws = Workspace {
            root: PathBuf::from("."),
            members: Vec::new(),
        };
        let order = equipments.resolve.order(&ws, None).unwrap();
        assert!(order.is_empty());
        let order = equipments.resolve.order(&ws, Some(0)).unwrap();
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn serial_runs_in_order_and_fails_fast() {
        let executed = std::sync::Mutex::new(Vec::new());
        let result = SerialScheduler.run(&[2, 0, 1], &[vec![], vec![], vec![]], &|i| {
            executed.lock().unwrap().push(i);
            Ok(())
        });
        assert!(result.is_ok());
        assert_eq!(*executed.lock().unwrap(), vec![2, 0, 1]);

        // 失败即止：后续下标不再执行（fail-fast）。
        let executed = std::sync::Mutex::new(Vec::new());
        let result = SerialScheduler.run(&[0, 1, 2], &[vec![], vec![], vec![]], &|i| {
            executed.lock().unwrap().push(i);
            if i == 1 {
                Err("boom".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "boom");
        assert_eq!(*executed.lock().unwrap(), vec![0, 1]);
    }

    #[test]
    fn parallel_respects_dependencies_and_runs_all() {
        // 图：0/1 无依赖；2 依赖 0 与 1。order 为被依赖者先出的拓扑序。
        let order = vec![0, 1, 2];
        let dep_on = vec![vec![], vec![], vec![0, 1]];
        let done = std::sync::Mutex::new(Vec::new());
        let scheduler = ParallelScheduler::with_jobs(4);
        let result = scheduler.run(&order, &dep_on, &|i| {
            std::thread::sleep(std::time::Duration::from_millis(5));
            done.lock().unwrap().push(i);
            Ok(())
        });
        assert!(result.is_ok());
        // 依赖约束：2 必须发生在 0 与 1 之后（0、1 间次序不定）。
        let done = done.into_inner().unwrap();
        assert_eq!(done.len(), 3);
        let p2 = done.iter().position(|&x| x == 2).unwrap();
        let p0 = done.iter().position(|&x| x == 0).unwrap();
        let p1 = done.iter().position(|&x| x == 1).unwrap();
        assert!(
            p0 < p2 && p1 < p2,
            "dependent 2 ran before its deps: {done:?}"
        );
    }

    #[test]
    fn parallel_propagates_first_error_and_runs_at_most_ready_tasks() {
        let order = vec![0, 1];
        let done = std::sync::Mutex::new(Vec::new());
        let scheduler = ParallelScheduler::with_jobs(2);
        let result = scheduler.run(&order, &[vec![], vec![]], &|i| {
            done.lock().unwrap().push(i);
            if i == 1 {
                Err("fail1".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "fail1");
        assert_eq!(done.into_inner().unwrap().len(), 2);
    }

    #[test]
    fn package_context_is_replaceable_via_bundle() {
        struct StubContext;
        impl PackageContext for StubContext {
            fn load(&self, _path: &Path) -> Result<CompileUnit, String> {
                Err("stub load".to_string())
            }
        }
        let equipments = Equipments {
            project: Box::new(WorkspaceProjectManager),
            resolve: Box::new(WorkspaceDependencyResolver),
            context: Box::new(StubContext),
            schedule: Box::new(SerialScheduler),
            emitter: Box::new(CodegenArtifactEmitter),
            host: Box::new(PipelineTestHost),
        };
        // 验证 bundle 的 `context` 路由到注入的替身装备（而非默认 loader）。
        let err = equipments
            .context
            .load(Path::new("/irrelevant"))
            .unwrap_err();
        assert_eq!(err, "stub load");
    }

    #[test]
    fn emitter_is_replaceable_via_bundle() {
        struct StubEmitter;
        impl ArtifactEmitter for StubEmitter {
            fn emit(
                &self,
                _request: ArtifactRequest,
            ) -> Result<Vec<codegen::StaticInitDiagnostic>, String> {
                Err("stub emit".to_string())
            }
        }
        let equipments = Equipments {
            project: Box::new(WorkspaceProjectManager),
            resolve: Box::new(WorkspaceDependencyResolver),
            context: Box::new(LoaderPackageContext),
            schedule: Box::new(SerialScheduler),
            emitter: Box::new(StubEmitter),
            host: Box::new(PipelineTestHost),
        };
        // 验证 bundle 的 `emitter` 路由到注入的替身装备（而非默认 codegen）。
        let request = ArtifactRequest {
            fns: &[],
            layouts: &typeck::ProgramLayouts::default(),
            output: Path::new("/irrelevant-out"),
            obj_dir: None,
            target: None,
            release: false,
            file_path: "",
            source: "",
            debug_info: false,
            fn_spans: &HashMap::new(),
            native_modules: &[],
            native_lib_paths: &[],
            external_symbols: &[],
            role: EmitRole::MainObject,
            project_kind: codegen::ProjectKind::Executable,
            export_symbols: &[],
            package_meta: None,
            keep_ir: false,
        };
        let err = equipments.emitter.emit(request).unwrap_err();
        assert_eq!(err, "stub emit");
    }

    #[test]
    fn test_host_is_replaceable_via_bundle() {
        struct StubHost;
        impl TestHost for StubHost {
            fn generate(
                &self,
                _: &[crate::pipeline::QifTestMethod],
                _: &crate::pipeline::QifCompileOptions,
            ) -> String {
                "stub host".to_string()
            }
        }
        let equipments = Equipments {
            project: Box::new(WorkspaceProjectManager),
            resolve: Box::new(WorkspaceDependencyResolver),
            context: Box::new(LoaderPackageContext),
            schedule: Box::new(SerialScheduler),
            emitter: Box::new(CodegenArtifactEmitter),
            host: Box::new(StubHost),
        };
        // 验证 bundle 的 `host` 路由到注入的替身装备（而非默认 pipeline 合成）。
        assert_eq!(
            equipments.host.generate(&[], &Default::default()),
            "stub host"
        );
    }
}
