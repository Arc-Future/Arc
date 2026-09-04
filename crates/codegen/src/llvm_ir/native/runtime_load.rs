//! RFC 016 unified runtime library loading model: codegen-emitted lazy resolver +
//! per-module function tables.
//!
//! Native modules with `load = "runtime"` (or `auto` degraded to runtime) skip
//! compile-time symbol verification and static linking; this module emits LLVM IR
//! that resolves the library at runtime:
//!
//! - per-module function pointer table (`@__arc_ani_<mod>_ftable`) and load-state
//!   globals (`_loaded` attempted / `_avail` loaded successfully).
//! - `@__arc_ani_ensure_<mod>()` lazy resolver: tries runtime candidates in order
//!   (`library` dir -- literal or Environment expression -- then system path) via
//!   `rt_library_load`, then fills the function table symbol by symbol via
//!   `rt_library_sym`.
//! - `@__arc_ani_try_<mod>(ptr %h)`: resolves each symbol against an already-loaded
//!   handle and fills the table, returning `i8` 1=all symbols ready 0=any missing
//!   (the caller unloads the handle and tries the next candidate).
//!
//! Call sites (`emit_call.rs`) emit an "ensure -> availability latch -> throw
//! `NativeLibraryNotFoundException` on failure -> indirect call via function table"
//! sequence for runtime modules.
//!
//! Reuses existing runtime ABI (`rt_library_load`/`rt_library_sym`/`rt_library_unload`/
//! `rt_list_buffer_and_size`/`rt_env_get_var`/`rt_str_concat`/`rt_str_length`);
//! no new runtime surface.
//!
//! `library` Environment form (`Environment.GetEnvironmentVariable("NAME")`) is
//! evaluated at runtime inside the lazy resolver via `rt_env_get_var` to obtain the
//! library **directory** (same semantics as the literal form); empty/unset -> candidate
//! missing, fall through. The evaluated runtime string is allocated once (the lazy
//! resolver runs at most once per module and lives for the process).
//!
//! Candidate priority (RFC 016, user ruling simplified 2026-08-03: `library` two
//! forms are the sole path mechanism, no multi-layer fallback): `library` (literal XOR
//! Environment expression) -> system path.

use ast::{LoadStrategy, NativeModule};
use std::collections::HashMap;
use std::path::Path;

use crate::llvm_ir::string_pool::escape_llvm_string;

/// Per-runtime-loaded-module info (one entry per module whose effective strategy
/// is `Runtime`).
pub(crate) struct RuntimeModuleInfo {
    /// Module name (LLVM global symbol prefix, e.g. `gpu`).
    pub name: String,
    /// Contract function C symbol names (declaration order == function table slots).
    pub symbols: Vec<String>,
    /// Arc-side function name -> function table slot index.
    pub fn_index: HashMap<String, usize>,
    /// Full candidate path = contract `library` dir + platform lib name; `None` if
    /// not declared.
    ///
    /// User ruling simplified (2026-08-03): relative path base = **executable root**
    /// (`-o` output executable directory), baked to absolute at compile time.
    pub library_candidate: Option<String>,
    /// Environment variable name for the `library` Environment form
    /// (`Environment.GetEnvironmentVariable`); `None` if not declared. Evaluated at
    /// runtime via `rt_env_get_var` to get the library directory, then the platform
    /// lib name is appended.
    pub library_env_var: Option<String>,
    /// Base directory (executable root, baked at compile time) used when the env-var
    /// form evaluates to a **relative** path; present only when the env form is
    /// declared and the exe dir is non-empty. Omitted for absolute paths / empty exe dir.
    pub exe_dir_string: Option<String>,
    /// System-path candidate (bare platform lib name, e.g. `libgpu.so` / `gpu.dll`).
    pub system_candidate: String,
}

/// Module name -> runtime load info.
pub(crate) type RuntimeModuleInfos = HashMap<String, RuntimeModuleInfo>;

/// Platform dynamic library naming (consistent with `resolve_native_lib` candidates).
///
/// - Windows: `<name>.dll`
/// - macOS: `lib<name>.dylib`
/// - other (Linux/OHos): `lib<name>.so`
fn platform_libname(module: &str, is_windows: bool, is_macos: bool) -> String {
    if is_windows {
        format!("{module}.dll")
    } else if is_macos {
        format!("lib{module}.dylib")
    } else {
        format!("lib{module}.so")
    }
}

/// Build the runtime-loaded module info table from effective strategy classification.
///
/// `exe_dir`: executable root directory (`-o` output executable directory, user
/// ruling simplified 2026-08-03). Relative `library` literals are baked to absolute
/// paths from here; relative env-var form results are likewise prefixed at runtime.
pub(crate) fn build_runtime_infos(
    modules: &[NativeModule],
    strategies: &HashMap<String, LoadStrategy>,
    is_windows: bool,
    is_macos: bool,
    exe_dir: &Path,
) -> RuntimeModuleInfos {
    let mut out = RuntimeModuleInfos::new();
    let exe_dir_str = {
        let s = exe_dir.display().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };
    for module in modules {
        let name = module.name.to_string();
        if strategies.get(&name) != Some(&LoadStrategy::Runtime) {
            continue;
        }
        let platform = platform_libname(&name, is_windows, is_macos);
        let library_candidate = module.library.as_ref().map(|dir| {
            let mut p = if dir.is_absolute() {
                dir.clone()
            } else {
                exe_dir.join(dir)
            };
            p.push(&platform);
            p.display().to_string()
        });
        let library_env_var = module.library_env_var.clone();
        let exe_dir_string = if library_env_var.is_some() {
            exe_dir_str.clone()
        } else {
            None
        };
        let mut fn_index = HashMap::new();
        for (i, f) in module.functions.iter().enumerate() {
            fn_index.insert(f.name.to_string(), i);
        }
        let symbols: Vec<String> = module
            .functions
            .iter()
            .map(|f| {
                f.symbol
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| f.name.to_string())
            })
            .collect();
        out.insert(
            name.clone(),
            RuntimeModuleInfo {
                name,
                symbols,
                fn_index,
                library_candidate,
                library_env_var,
                exe_dir_string,
                system_candidate: platform,
            },
        );
    }
    out
}

/// Emit globals + lazy resolver functions for all runtime-loaded modules.
pub(crate) fn emit_runtime_load_support(infos: &RuntimeModuleInfos) -> String {
    if infos.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push_str("; ---- RFC 016: runtime-loaded native modules (lazy resolvers) ----\n");
    let mut names: Vec<&String> = infos.keys().collect();
    names.sort();
    for name in names {
        out.push_str(&emit_module_support(&infos[name]));
        out.push('\n');
    }
    out
}

/// LLVM IR label generator (unique within a function).
struct LabelGen {
    counter: u32,
}

impl LabelGen {
    fn new() -> Self {
        Self { counter: 0 }
    }

    fn fresh(&mut self, base: &str) -> String {
        let l = format!("{base}{}", self.counter);
        self.counter += 1;
        l
    }
}

/// Emit a single runtime module's globals + ensure + try functions.
fn emit_module_support(info: &RuntimeModuleInfo) -> String {
    let n = info.symbols.len();
    let p = format!("__arc_ani_{}", info.name);
    let ftable = format!("@{p}_ftable");
    let loaded = format!("@{p}_loaded");
    let avail = format!("@{p}_avail");
    let mname = format!("@{p}_mname");

    let mut out = String::new();
    // ---- globals ----
    out.push_str(&format!(
        "; RFC 016 module `{}` (load = runtime)\n",
        info.name
    ));
    out.push_str(&format!(
        "{ftable} = internal global [{n} x ptr] zeroinitializer\n"
    ));
    out.push_str(&format!("{loaded} = internal global i8 0\n"));
    out.push_str(&format!("{avail} = internal global i8 0\n"));
    out.push_str(&format!(
        "{mname} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
        info.name.len() + 1,
        escape_llvm_string(info.name.as_bytes())
    ));
    for (k, sym) in info.symbols.iter().enumerate() {
        out.push_str(&format!(
            "@{p}_sym_{k} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
            sym.len() + 1,
            escape_llvm_string(sym.as_bytes())
        ));
    }
    let sys = &info.system_candidate;
    if let Some(cand) = &info.library_candidate {
        out.push_str(&format!(
            "@{p}_cand_lib = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
            cand.len() + 1,
            escape_llvm_string(cand.as_bytes())
        ));
    }
    if let Some(envvar) = &info.library_env_var {
        out.push_str(&format!(
            "@{p}_cand_envname = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
            envvar.len() + 1,
            escape_llvm_string(envvar.as_bytes())
        ));
        out.push_str(&format!(
            "@{p}_cand_sep = private unnamed_addr constant [2 x i8] c\"/\\00\"\n"
        ));
        out.push_str(&format!(
            "@{p}_cand_libname = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
            sys.len() + 1,
            escape_llvm_string(sys.as_bytes())
        ));
        // Executable root (user ruling simplified 2026-08-03): when the env-var value
        // is a relative path, prefix it with the exe dir; absolute paths are used as-is.
        if let Some(exedir) = &info.exe_dir_string {
            out.push_str(&format!(
                "@{p}_cand_exedir = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
                exedir.len() + 1,
                escape_llvm_string(exedir.as_bytes())
            ));
        }
    }
    out.push_str(&format!(
        "@{p}_cand_sys = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
        sys.len() + 1,
        escape_llvm_string(sys.as_bytes())
    ));

    // ---- try helper: resolve symbols against an already-loaded handle + fill table ----
    out.push_str(&format!("define internal i8 @{p}_try(ptr %h) {{\n"));
    if n == 0 {
        out.push_str("entry:\n");
        out.push_str("  ret i8 1\n");
    } else {
        let mut fill_stores = String::new();
        for k in 0..n {
            let label = if k == 0 {
                "entry".to_string()
            } else {
                format!("s{k}")
            };
            out.push_str(&format!("{label}:\n"));
            let f = format!("%f{k}");
            out.push_str(&format!(
                "{f} = call ptr @rt_library_sym(ptr %h, ptr @{p}_sym_{k})\n"
            ));
            out.push_str(&format!("%ok{k} = icmp ne ptr {f}, null\n"));
            if k + 1 < n {
                out.push_str(&format!("br i1 %ok{k}, label %s{}, label %fail\n", k + 1));
            } else {
                out.push_str(&format!("br i1 %ok{k}, label %fill, label %fail\n"));
            }
            fill_stores.push_str(&format!(
                "  store ptr {f}, ptr getelementptr inbounds ([{n} x ptr], ptr {ftable}, i32 0, i32 {k})\n"
            ));
        }
        out.push_str("fill:\n");
        out.push_str(&fill_stores);
        out.push_str(&format!("  store i8 1, ptr {avail}\n"));
        out.push_str("  ret i8 1\n");
        out.push_str("fail:\n");
        out.push_str("  ret i8 0\n");
    }
    out.push_str("}\n");

    // ---- ensure lazy resolver ----
    out.push_str(&format!("define internal void @{p}_ensure() {{\n"));
    out.push_str("entry:\n");
    out.push_str(&format!("  %ld = load i8, ptr {loaded}\n"));
    out.push_str("  %un = icmp eq i8 %ld, 0\n");
    out.push_str("  br i1 %un, label %resolve, label %done\n");
    out.push_str("resolve:\n");
    out.push_str(&format!("  store i8 1, ptr {loaded}\n"));

    let mut labels = LabelGen::new();
    let mut next = labels.fresh("cand");
    out.push_str(&format!("  br label %{next}\n"));

    // Candidate 1: contract `library` -- literal dir (full path constant baked at
    // compile time) or Environment expression (runtime `rt_env_get_var` evaluation of
    // the dir, appending the platform lib name). Mutually exclusive (single `library`
    // declaration); empty/unset -> candidate missing, fall through.
    if info.library_candidate.is_some() || info.library_env_var.is_some() {
        let lib = next.clone();
        next = labels.fresh("cand");
        out.push_str(&format!("{lib}:\n"));
        if info.library_env_var.is_some() {
            // Environment form (self-contained sequence):
            //   rt_env_get_var -> empty-string guard -> absolute/relative detection ->
            //   relative: prefix exe dir (exe_dir + "/" + envdir) -> append platform lib name.
            let abs_l = labels.fresh("abs");
            let prep_l = labels.fresh("prep");
            let mk_l = labels.fresh("mk");
            let try_p = labels.fresh("tryp");
            let fail_p = labels.fresh("failp");
            let try_m = labels.fresh("trym");
            let fail_m = labels.fresh("failm");
            out.push_str(&format!(
                "  %envdir = call ptr @rt_env_get_var(ptr @{p}_cand_envname)\n"
            ));
            out.push_str("  %envlen = call i32 @rt_str_length(ptr %envdir)\n");
            out.push_str("  %envok = icmp ne i32 %envlen, 0\n");
            if info.exe_dir_string.is_some() {
                out.push_str(&format!("  br i1 %envok, label %{abs_l}, label %{next}\n"));
                out.push_str(&format!("{abs_l}:\n"));
                // Absolute-path detection (cross-platform): first char '/' or '\\'
                // (POSIX root / Windows rooted/UNC), or second char ':' (Windows drive).
                out.push_str("  %c0 = call i32 @rt_str_char_at(ptr %envdir, i32 0)\n");
                out.push_str("  %isabs0 = icmp eq i32 %c0, 47\n");
                out.push_str("  %isabs1 = icmp eq i32 %c0, 92\n");
                out.push_str("  %c1 = call i32 @rt_str_char_at(ptr %envdir, i32 1)\n");
                out.push_str("  %isdrv = icmp eq i32 %c1, 58\n");
                out.push_str("  %or01 = or i1 %isabs0, %isabs1\n");
                out.push_str("  %isabs = or i1 %or01, %isdrv\n");
                out.push_str(&format!("  br i1 %isabs, label %{mk_l}, label %{prep_l}\n"));
                // Relative path: exe_dir + "/" + envdir + "/" + libname
                out.push_str(&format!("{prep_l}:\n"));
                out.push_str(&format!(
                    "  %p1 = call ptr @rt_str_concat(ptr @{p}_cand_exedir, ptr @{p}_cand_sep)\n"
                ));
                out.push_str("  %p2 = call ptr @rt_str_concat(ptr %p1, ptr %envdir)\n");
                out.push_str(&format!(
                    "  %envsepp = call ptr @rt_str_concat(ptr %p2, ptr @{p}_cand_sep)\n"
                ));
                out.push_str(&format!(
                    "  %envfullp = call ptr @rt_str_concat(ptr %envsepp, ptr @{p}_cand_libname)\n"
                ));
                out.push_str("  %h = call ptr @rt_library_load(ptr %envfullp)\n");
                out.push_str("  %ok = icmp ne ptr %h, null\n");
                out.push_str(&format!("  br i1 %ok, label %{try_p}, label %{next}\n"));
                out.push_str(&format!("{try_p}:\n"));
                out.push_str(&format!("  %r = call i8 @{p}_try(ptr %h)\n"));
                out.push_str("  %rok = icmp ne i8 %r, 0\n");
                out.push_str(&format!("  br i1 %rok, label %done, label %{fail_p}\n"));
                out.push_str(&format!("{fail_p}:\n"));
                out.push_str("  call void @rt_library_unload(ptr %h)\n");
                out.push_str(&format!("  br label %{next}\n"));
                // Absolute path: envdir + "/" + libname
                out.push_str(&format!("{mk_l}:\n"));
            } else {
                // No baked exe dir (published .o without a final executable location)
                // -> use the env-var value as-is.
                out.push_str(&format!("  br i1 %envok, label %{mk_l}, label %{next}\n"));
                out.push_str(&format!("{mk_l}:\n"));
            }
            out.push_str(&format!(
                "  %envsepm = call ptr @rt_str_concat(ptr %envdir, ptr @{p}_cand_sep)\n"
            ));
            out.push_str(&format!(
                "  %envfullm = call ptr @rt_str_concat(ptr %envsepm, ptr @{p}_cand_libname)\n"
            ));
            out.push_str("  %habs = call ptr @rt_library_load(ptr %envfullm)\n");
            out.push_str("  %okabs = icmp ne ptr %habs, null\n");
            out.push_str(&format!("  br i1 %okabs, label %{try_m}, label %{next}\n"));
            out.push_str(&format!("{try_m}:\n"));
            out.push_str(&format!("  %rabs = call i8 @{p}_try(ptr %habs)\n"));
            out.push_str("  %rokabs = icmp ne i8 %rabs, 0\n");
            out.push_str(&format!("  br i1 %rokabs, label %done, label %{fail_m}\n"));
            out.push_str(&format!("{fail_m}:\n"));
            out.push_str("  call void @rt_library_unload(ptr %habs)\n");
            out.push_str(&format!("  br label %{next}\n"));
        } else {
            // Literal form (shared tail sequence).
            let try_l = labels.fresh("try");
            let fail_l = labels.fresh("failc");
            out.push_str(&format!(
                "  %h = call ptr @rt_library_load(ptr @{p}_cand_lib)\n"
            ));
            out.push_str("  %ok = icmp ne ptr %h, null\n");
            out.push_str(&format!("  br i1 %ok, label %{try_l}, label %{next}\n"));
            out.push_str(&format!("{try_l}:\n"));
            out.push_str(&format!("  %r = call i8 @{p}_try(ptr %h)\n"));
            out.push_str("  %rok = icmp ne i8 %r, 0\n");
            out.push_str(&format!("  br i1 %rok, label %done, label %{fail_l}\n"));
            out.push_str(&format!("{fail_l}:\n"));
            out.push_str("  call void @rt_library_unload(ptr %h)\n");
            out.push_str(&format!("  br label %{next}\n"));
        }
    }

    // Candidate 2: system path (bare platform lib name; OS dynamic loader searches
    // the system path).
    let sys_l = next.clone();
    let try_s = labels.fresh("try");
    let fail_s = labels.fresh("fails");
    out.push_str(&format!("{sys_l}:\n"));
    out.push_str(&format!(
        "  %h2 = call ptr @rt_library_load(ptr @{p}_cand_sys)\n"
    ));
    out.push_str("  %ok2 = icmp ne ptr %h2, null\n");
    out.push_str(&format!("  br i1 %ok2, label %{try_s}, label %done\n"));
    out.push_str(&format!("{try_s}:\n"));
    out.push_str(&format!("  %r2 = call i8 @{p}_try(ptr %h2)\n"));
    out.push_str("  %rok2 = icmp ne i8 %r2, 0\n");
    out.push_str(&format!("  br i1 %rok2, label %done, label %{fail_s}\n"));
    out.push_str(&format!("{fail_s}:\n"));
    out.push_str("  call void @rt_library_unload(ptr %h2)\n");
    out.push_str("  br label %done\n");
    out.push_str("done:\n");
    out.push_str("  ret void\n");
    out.push_str("}\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{CallingConv, NativeFn, Spanned, Type};
    use std::path::{Path, PathBuf};

    fn make_module(
        name: &str,
        library_env_var: Option<String>,
        library: Option<PathBuf>,
    ) -> NativeModule {
        NativeModule {
            name: name.into(),
            functions: vec![NativeFn {
                name: "init".into(),
                symbol: None,
                params: vec![],
                ret: Some(Spanned::new(
                    Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    ast::Span::DUMMY,
                )),
                calling_conv: CallingConv::default(),
            }],
            types: vec![],
            capability: None,
            library,
            library_env_var,
            source: None,
            load: LoadStrategy::Runtime,
            callbacks: vec![],
        }
    }

    fn strategies(modules: &[NativeModule]) -> HashMap<String, LoadStrategy> {
        let mut out = HashMap::new();
        for m in modules {
            out.insert(m.name.to_string(), m.load);
        }
        out
    }

    /// RFC 016: the Environment-expression `library` lazy resolver emits
    /// `rt_env_get_var` + platform-lib-name concat; literal `library` does not emit
    /// the env-evaluation sequence.
    #[test]
    fn emit_env_var_library_candidate_ir() {
        let env_mod = make_module("gpu_env", Some("ARC_GPU_LIB".into()), None);
        let lit_mod = make_module("gpu_lit", None, Some(PathBuf::from("/opt/gpu/lib")));
        let modules = vec![env_mod.clone(), lit_mod.clone()];
        let exe_dir = Path::new("/opt/app/bin");
        let infos = build_runtime_infos(&modules, &strategies(&modules), false, false, exe_dir);
        assert!(infos.contains_key("gpu_env"));
        assert!(infos.contains_key("gpu_lit"));
        assert_eq!(
            infos["gpu_env"].exe_dir_string.as_deref(),
            Some("/opt/app/bin")
        );
        assert_eq!(infos["gpu_lit"].exe_dir_string, None);

        let env_ir = emit_module_support(&infos["gpu_env"]);
        assert!(
            env_ir.contains("@__arc_ani_gpu_env_cand_envname"),
            "env name const missing:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@__arc_ani_gpu_env_cand_libname"),
            "libname const missing:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@rt_env_get_var"),
            "env evaluation missing:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@rt_str_length"),
            "empty-string guard missing:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@rt_str_concat"),
            "dir+libname concat missing:\n{env_ir}"
        );
        assert!(
            env_ir.contains("libgpu_env.so"),
            "platform libname must be appended:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@__arc_ani_gpu_env_cand_exedir"),
            "exe-dir const missing for relative env values:\n{env_ir}"
        );
        assert!(
            env_ir.contains("@rt_str_char_at"),
            "absolute-path detection missing:\n{env_ir}"
        );
        assert!(
            !env_ir.contains("@__arc_ani_gpu_env_cand_lib = "),
            "env form must not bake literal path:\n{env_ir}"
        );

        let lit_ir = emit_module_support(&infos["gpu_lit"]);
        assert!(
            lit_ir.contains("@__arc_ani_gpu_lit_cand_lib = "),
            "literal candidate const missing:\n{lit_ir}"
        );
        let mut expected = PathBuf::from("/opt/gpu/lib");
        expected.push("libgpu_lit.so");
        let escaped = escape_llvm_string(expected.display().to_string().as_bytes());
        assert!(
            lit_ir.contains(&escaped),
            "literal path must bake full candidate {}, got:\n{lit_ir}",
            expected.display()
        );
        assert!(
            !lit_ir.contains("@rt_env_get_var"),
            "literal form must not evaluate env:\n{lit_ir}"
        );
    }

    /// User ruling simplified (2026-08-03): relative `library` literals are baked to
    /// absolute paths against the executable root (`-o` output dir).
    #[test]
    fn emit_relative_literal_resolved_against_exe_dir() {
        let lit_mod = make_module("gpu_rel", None, Some(PathBuf::from("vendor/gpu/lib")));
        let strat = strategies(std::slice::from_ref(&lit_mod));
        let infos = build_runtime_infos(
            std::slice::from_ref(&lit_mod),
            &strat,
            false,
            false,
            Path::new("/opt/app/bin"),
        );
        let ir = emit_module_support(&infos["gpu_rel"]);
        // Expected path follows the same construction order as the code
        // (exe_dir.join(dir) then push platform lib name), so Windows separator
        // normalization is consistent.
        let mut expected = Path::new("/opt/app/bin").join("vendor/gpu/lib");
        expected.push("libgpu_rel.so");
        let escaped = escape_llvm_string(expected.display().to_string().as_bytes());
        assert!(
            ir.contains(&escaped),
            "relative literal must resolve against exe dir, expected {}, got:\n{ir}",
            expected.display()
        );
    }

    /// RFC 016: unset env var (empty string) -> env candidate missing, the IR has a
    /// branch skipping to the next candidate.
    #[test]
    fn emit_env_var_library_skips_on_empty() {
        let env_mod = make_module("gpu_env2", Some("ARC_GPU_LIB".into()), None);
        let strat = strategies(std::slice::from_ref(&env_mod));
        let infos = build_runtime_infos(
            std::slice::from_ref(&env_mod),
            &strat,
            false,
            false,
            Path::new("/opt/app/bin"),
        );
        let ir = emit_module_support(&infos["gpu_env2"]);
        // envok branch: non-empty -> concat + load; empty -> jump to next candidate.
        assert!(
            ir.contains("%envok = icmp ne i32 %envlen, 0"),
            "empty guard compare missing:\n{ir}"
        );
        assert!(
            ir.contains("br i1 %envok"),
            "empty guard branch missing:\n{ir}"
        );
    }
}
