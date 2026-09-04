//! `arc ui codegen` —— 将 `.arml` AST 转换为 Arc 代码（RFC 037 M2 ARML code-behind）。
//!
//! 对标 WPF XAML 项目结构与编码模型：
//!
//! ```text
//! examples/ArmlDemo/
//! ├── App.arml              # 应用入口 ARML（<Application x:Class="Ns.App" StartupUri="MainWindow.arml"/>）
//! ├── App.arml.as           # 应用 code-behind（partial class App : Application）
//! ├── MainWindow.arml       # 主窗口 ARML 声明
//! ├── MainWindow.arml.as   # 主窗口 code-behind（partial class MainWindow : Window）
//! ├── Program.as            # 入口文件（var app = new App(); app.Run();）
//! ├── arc.toml              # 项目描述（含 [ui] 节）
//! ├── obj/
//! │   └── Debug/
//! │       ├── App.g.as          # codegen 生成 App partial class : Application
//! │       ├── MainWindow.g.as   # codegen 生成 MainWindow partial class : Window
//! │       └── Program.as         # 合并所有 .g.as + .arml.as + Program.as 的编译单元
//! └── bin/
//!     └── Debug/
//!         └── ArmlDemo.exe
//! ```
//!
//! ## 根元素支持（WPF-aligned 继承）
//!
//! - `<Window>` —— 主窗口声明。生成 `partial class MainWindow : Window`，
//!   override `InitializeComponent()` 设置 `this.Title/Width/Height/Text` 属性
//! - `<Application>` —— 应用入口声明。生成 `partial class App : Application`，
//!   override `InitializeComponent()` 设置 `this.MainWindow = new MainWindow()`
//!   并调用 `this.MainWindow.InitializeComponent()`
//!   通过 `StartupUri="MainWindow.arml"` 推导启动窗口类名（去 `.arml` 后缀）
//!
//! ## 框架源自动合并
//!
//! `CodegenOptions.framework_sources` 列出 Arc.UI 框架源文件（Element.as /
//! Window.as / Application.as / WindowHost.as 等），会被自动 strip namespace
//! 后合并到 `Program.as` 末尾，确保所有类型在同一命名空间可见。这避免用户项目
//! 显式 `using Arc.UI.Components`——所有类型直接在项目命名空间下可用。

use crate::ast::*;
use crate::Parser;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// codegen 选项（对标 WPF 项目模型）。
#[derive(Debug, Clone)]
pub struct CodegenOptions {
    /// 生成代码的命名空间（如 `ArmlDemo`）。
    pub namespace: String,
    /// 用户 partial class 源文件列表（如 `App.arml.as`、`MainWindow.arml.as`）。
    ///
    /// 在 `generate_project` 中被合并到 `Program.as` 末尾——剥离 namespace/using
    /// 行后追加。`generate` 单文档模式不使用此字段。
    pub user_sources: Vec<PathBuf>,
    /// 程序入口文件路径（如 `Program.as`），含 `Main()` 函数。
    ///
    /// 所有 Arc 项目统一此标准——对标 WPF App.g.cs 自动生成的 Main 入口，
    /// 但 Arc 让用户显式控制入口文件。在 `generate_project` 中合并到 `Program.as`
    /// 最末尾（确保 partial class 定义先于 Main 函数）。剥离 namespace/using 行后追加。
    pub program: Option<PathBuf>,
    /// Arc.UI 框架源文件列表（如 `Element.as`、`Window.as`、`Application.as`）。
    ///
    /// 在 `generate_project` 中被自动合并到 `Program.as` 末尾——剥离
    /// namespace/using 行后追加，使所有框架类型在项目命名空间下可见。
    /// 通常由 `arc build` 项目模式自动填充（从 `std/UI/Core/Components/` 等目录）。
    pub framework_sources: Vec<PathBuf>,
    /// 独立 `.g.as` 文件输出目录（如 `obj`）。生成代码落入
    /// `obj/code/<relative_path>/<stem>.g.as`，其中 `<relative_path>` 为
    /// 源文件到项目根目录的相对路径（对标 .NET `<obj>/<Config>/<TFM>/`）。
    pub obj_dir: Option<PathBuf>,
    /// 项目根目录——用于将源文件路径映射为 `obj/code/` 下的相对路径。
    /// 设置时与 `obj_dir` 组合：`<obj_dir>/code/<rel>/<stem>.g.as`。
    pub project_root: Option<PathBuf>,
    /// 构建配置（`Debug` 或 `Release`），影响 bin 输出子目录。
    pub config: String,
}

impl Default for CodegenOptions {
    fn default() -> Self {
        Self {
            namespace: "Arc.UI.Generated".into(),
            user_sources: Vec::new(),
            program: None,
            framework_sources: Vec::new(),
            obj_dir: None,
            project_root: None,
            config: "Debug".into(),
        }
    }
}

/// 一个由 codegen 生成的独立 `.g.as` 文件信息。
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    /// 写入磁盘的完整路径（如 `obj/Debug/MainWindow.g.as`）。
    pub path: PathBuf,
    /// 提取出的 partial class 名（如 `MainWindow`）。
    pub class_name: String,
}

/// `generate_project` 输出。
#[derive(Debug, Clone)]
pub struct ProjectOutput {
    /// 合并所有 `.g.as` + `.arml.as` 后的 `Program.as` 完整内容。
    pub program: String,
    /// 写入磁盘的独立 `.g.as` 文件列表（仅当 `obj_dir` 设置时非空）。
    pub generated_files: Vec<GeneratedFile>,
}

/// 主入口：处理多个 `.arml` 文件 + 多个用户 `.arml.as` 源文件。
///
/// 对每个 `.arml`：
/// 1. 解析 → 提取 Class 名称 → 生成 partial class 体
/// 2. 写独立 `.g.as` 到 `obj/code/<relative_path>/<stem>.g.as`（对标 .NET）
/// 3. 累积类体到 `Program.as`
///
/// 然后对所有 `user_sources` 剥离 namespace/using 行后追加到 `Program.as`。
///
/// # 错误
/// - ARML 文件读取/解析失败
/// - 根元素非 `Window`/`Application`
/// - 缺少 `Class`/`x:Class` 属性
/// - 写 `.g.as` 文件失败
pub fn generate_project(
    arml_files: &[PathBuf],
    opts: &CodegenOptions,
) -> Result<ProjectOutput, String> {
    let mut generated_files = Vec::new();
    let mut program = String::new();

    // 预扫描所有源文件，收集去重后的 `using X;` 行。
    // Arc file-scoped namespace 限制：namespace 必须为文件唯一顶层声明，
    // 但 using 可在 namespace 后多次出现。将各源文件的 using 收集到
    // 顶部统一声明，避免框架源（如 Element.as 的 `using Arc.Collections;`）
    // 被剥离后导致 List<T> 等类型在合并作用域内不可见。
    let mut collected_usings: Vec<String> = Vec::new();
    let collect_from = |path: &Path, usings: &mut Vec<String>| {
        if let Ok(src) = std::fs::read_to_string(path) {
            for line in src.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("using ") && trimmed.ends_with(';') {
                    let s = trimmed.to_string();
                    if !usings.iter().any(|x| x == &s) {
                        usings.push(s);
                    }
                }
            }
        }
    };
    for p in &opts.user_sources {
        collect_from(p, &mut collected_usings);
    }
    for p in &opts.framework_sources {
        collect_from(p, &mut collected_usings);
    }
    if let Some(p) = &opts.program {
        collect_from(p, &mut collected_usings);
    }

    if framework_inlines_internal_sources(&opts.framework_sources) {
        collected_usings.retain(|u| u != "using Arc.UI.Internal;");
    }

    // 头部：namespace + using 仅声明一次（Arc file-scoped namespace 限制）
    program.push_str("// <auto-generated>\n");
    program.push_str("// 由 `arc ui codegen` 生成。RFC 026 ARML code-behind (WPF-style)。\n");
    program.push_str("// 不要手动编辑此文件——修改 .arml/.arml.as 源文件后重新生成。\n");
    program.push_str("// </auto-generated>\n\n");
    program.push_str(&format!("namespace {};\n\n", opts.namespace));
    // `using Arc;` 始终声明，并跳过 collected_usings 中可能重复的 `using Arc;`
    program.push_str("using Arc;\n");
    for u in &collected_usings {
        if u == "using Arc;" {
            continue;
        }
        program.push_str(u);
        program.push('\n');
    }
    program.push('\n');

    for arml_path in arml_files {
        let src = std::fs::read_to_string(arml_path)
            .map_err(|e| format!("read {}: {e}", arml_path.display()))?;
        let doc = Parser::parse(&src).map_err(|e| format!("parse {}: {e}", arml_path.display()))?;

        let (class_name, body) = generate_partial_class_body(&doc)?;

        // 写独立 .g.as 文件到 obj/<config>/code/<relative_path>/<stem>.g.as（对标 .NET）
        if let Some(obj_dir) = &opts.obj_dir {
            let g_path = {
                let rel = if let Some(root) = &opts.project_root {
                    arml_path.strip_prefix(root).unwrap_or(arml_path)
                } else {
                    arml_path
                };
                let parent = rel.parent().unwrap_or_else(|| Path::new("."));
                let stem = rel
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                obj_dir
                    .join(&opts.config)
                    .join("code")
                    .join(parent)
                    .join(format!("{stem}.g.as"))
            };
            if let Some(parent) = g_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create obj dir failed: {e}"))?;
            }
            let g_content = format_g_as_file(&class_name, &body, arml_path, opts);
            std::fs::write(&g_path, &g_content)
                .map_err(|e| format!("write {}: {e}", g_path.display()))?;
            generated_files.push(GeneratedFile {
                path: g_path,
                class_name: class_name.clone(),
            });
        }

        // 累积到 Program.as（带来源注释）
        let arml_name = arml_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        program.push_str(&format!("// from {} ({}.g.as)\n", arml_name, class_name));
        program.push_str(&body);
        program.push('\n');
    }

    // 合并用户源文件（.arml.as partial class 业务实现）
    for user_path in &opts.user_sources {
        let user_code = std::fs::read_to_string(user_path)
            .map_err(|e| format!("read {}: {e}", user_path.display()))?;
        let stripped = strip_namespace_and_using(&user_code);
        let user_name = user_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        program.push_str(&format!("// ---- merged from {} ----\n", user_name));
        program.push_str(&stripped);
        program.push('\n');
    }

    // 合并 Arc.UI 框架源文件（Element/Window/Application/WindowHost 等）
    // —— 在用户源之后、入口文件之前，确保框架类型在 partial class 与 Main
    // 之前可见。剥离 namespace/using 后追加到当前命名空间。
    for fw_path in &opts.framework_sources {
        let fw_code = std::fs::read_to_string(fw_path)
            .map_err(|e| format!("read {}: {e}", fw_path.display()))?;
        let stripped = strip_namespace_and_using(&fw_code);
        let fw_name = fw_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        program.push_str(&format!("// ---- framework: {} ----\n", fw_name));
        program.push_str(&stripped);
        program.push('\n');
    }

    // 合并程序入口文件（Program.as，含 Main 函数）——放在最末尾确保
    // partial class 定义先于 Main 函数被解析器看到
    if let Some(program_path) = &opts.program {
        let program_code = std::fs::read_to_string(program_path)
            .map_err(|e| format!("read {}: {e}", program_path.display()))?;
        let stripped = strip_namespace_and_using(&program_code);
        let program_name = program_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unknown>");
        program.push_str(&format!(
            "// ---- merged from {} (entry) ----\n",
            program_name
        ));
        program.push_str(&stripped);
        program.push('\n');
    }

    Ok(ProjectOutput {
        program,
        generated_files,
    })
}

/// 格式化独立 `.g.as` 文件内容（含头部 namespace/using + partial class 体）。
fn format_g_as_file(
    class_name: &str,
    body: &str,
    source_arml: &Path,
    opts: &CodegenOptions,
) -> String {
    let _ = class_name; // 仅用于日志，不嵌入内容
    let arml_name = source_arml
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>");
    let mut out = String::new();
    out.push_str("// <auto-generated>\n");
    out.push_str(&format!(
        "// 由 `arc ui codegen` 从 {} 生成。RFC 026 ARML code-behind。\n",
        arml_name
    ));
    out.push_str("// 不要手动编辑此文件——修改 .arml 源文件后重新生成。\n");
    out.push_str("// </auto-generated>\n\n");
    out.push_str(&format!("namespace {};\n\n", opts.namespace));
    out.push_str("using Arc;\n\n");
    out.push_str(body);
    out
}

/// 单文档 partial class 体生成（不含 namespace/using 头）。
///
/// 返回 `(class_name, body)`。`body` 是 `public partial class <Name> { ... }` 块。
/// 支持根元素 `Window` 与 `Application`。
fn generate_partial_class_body(doc: &ArmlDocument) -> Result<(String, String), String> {
    let root_name = &doc.root.name;
    match root_name.as_str() {
        "Window" => generate_window_partial(doc),
        "Application" => generate_application_partial(doc),
        _ => Err(format!(
            "unsupported root element: `{}` (expected `Window` or `Application`)",
            root_name
        )),
    }
}

/// 从根元素提取 Class 名称（支持 `Class="Ns.Name"` 与 `x:Class="Ns.Name"` 两种形式）。
///
/// 返回末段类名（如 `Ns.MainWindow` → `MainWindow`）。
fn extract_class_name(doc: &ArmlDocument) -> Result<String, String> {
    doc.root
        .attr("Class")
        .or_else(|| doc.root.attr_with_prefix("x", "Class"))
        .and_then(|a| a.value.as_literal())
        .and_then(|s| {
            s.rsplit('.')
                .next()
                .filter(|seg| !seg.is_empty())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| {
            format!(
                "root element `<{}>` missing `Class` or `x:Class` attribute",
                doc.root.name
            )
        })
}

/// `<Window>` 根元素 → `partial class MainWindow : Window`，override
/// `InitializeComponent()` 设置从 ARML 解析的属性 + 递归生成子元素树。
///
/// 生成体（对标 WPF MainWindow.g.cs，M3 元素树扩展）：
/// ```arc
/// public partial class MainWindow : Window {
///     public override void InitializeComponent() {
///         this.Title = "...";
///         this.Width = 640;
///         this.Height = 480;
///         this.Text = "...";
///         // M3：子元素树（每行由 emit_child_elements 生成）
///         var child_0 = new StackPanel();
///         child_0.Orientation = "Vertical";
///         var child_1 = new TextBlock();
///         child_1.Text = "Hello";
///         child_0.AddChild(child_1);
///         this.AddChild(child_0);
///     }
/// }
/// ```
///
/// `Window` 基类提供 `Show()`/`Close()`/`OnLoaded()`/`OnClosed()` 等实例方法
/// 与生命周期钩子；`InitializeComponent()` 仅设置属性 + 构建元素树，
/// 不进入事件循环（事件循环由 `Application.Run()` 调用 `MainWindow.Show()` 触发）。
fn generate_window_partial(doc: &ArmlDocument) -> Result<(String, String), String> {
    let class_name = extract_class_name(doc)?;
    let (title, width, height) = extract_root_window(&doc.root);

    // M-U2：编译期投影规格（§11.5）。窗口含 Token 引用/`<Adaptive>` 时才发射求值器。
    let spec = crate::projection::build_projection_spec(doc).map_err(|errs| {
        let joined = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ");
        format!("adaptive projection spec error: {joined}")
    })?;
    let has_tokens = spec.uses_tokens || !spec.tokens.is_empty() || spec.has_adaptives;
    let token_ids: std::collections::BTreeMap<String, usize> = spec
        .tokens
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.to_string(), i))
        .collect();

    let mut body = String::new();
    body.push_str(&format!(
        "public partial class {} : Window {{\n",
        class_name
    ));
    body.push_str("    public override void InitializeComponent() {\n");
    emit_window_property_assignments(&mut body, &title, width, height, /*indent=*/ 8);

    // M3 样式系统：TypeName 供 StyleManager 隐式匹配
    body.push_str("        this.TypeName = \"Window\";\n");

    // RFC 037：窗口局部资源字典（`<Window.Resources>` / `<Styles>`）——
    // 条目 + 声明式 Style 注册（样式解析 primary 域；Application.ApplyStyleTree
    // 以 `MainWindow.Resources is ResourceDictionary` 接管，App 全局作 fallback）。
    // 返回 样式键 → `_style_N` 定型映射，供元素端 Style 多资源绑定对象定型。
    let style_keys = emit_window_resources(&mut body, doc, /*indent=*/ 8)?;

    // M-U2：求值器/宿主创建（在子元素绑定之前，绑定引用 `this._adaptiveHost`）。
    if has_tokens {
        body.push_str("        // ---- M-U2 自适应投影求值器（RFC 027 §11.5；编译期投影表）----\n");
        body.push_str(&crate::projection_arc::render_spec_arc(&spec));
        body.push_str("        this._adaptiveHost = new AdaptiveHost(_adaptiveSpec);\n");
    }

    // M3：递归生成子元素树实例化代码（仅 Arc 逻辑树）。
    // 每个 .arml 子元素 → `var child_N = new ElementType(); ...; parent.AddChild(child_N);`
    // 平台镜像由 Window.Show() → PlatformTreeSync.BuildFromArc 一次性同步（M3.6）。
    let mut counter = 0usize;
    let mut bind_counter = 0usize;
    let named_fields = emit_child_elements(
        &mut body,
        &doc.root,
        "this",
        &mut counter,
        &mut bind_counter,
        /*indent=*/ 8,
        if has_tokens { Some(&token_ids) } else { None },
        &style_keys,
    )?;

    body.push_str("    }\n");
    // M4：`x:Name` 命名元素 → 同名私有字段（InitializeComponent 后 code-behind
    // partial class 内可直接引用；WPF MainWindow.g.cs 同构）。
    for (name, ty) in &named_fields {
        body.push_str(&format!("    private {} {};\n", ty, name));
    }
    if has_tokens {
        body.push_str("    private AdaptiveHost _adaptiveHost;\n");
    }
    body.push_str("}\n");

    Ok((class_name, body))
}

/// 递归生成元素树实例化代码（Arc 逻辑树）。
///
/// 对 `parent.children` 中每个 Element 子节点：
///   1. 生成 `var child_N = new ElementType();`
///   2. 对每个无前缀字面量属性生成 `child_N.AttrName = value;`
///   3. 递归处理该子元素的子元素
///   4. 生成 `parent_var.AddChild(child_N);`
///
/// 平台 RtUiElement 镜像不在 codegen 双写——由 `Window.Show()` 调用
/// `PlatformTreeSync.BuildFromArc` 从逻辑树一次性同步（RFC 037 M3.6）。
///
/// `x:Name="..."`（M4）：生成同名私有字段引用 `this.<name> = child_N;`，
/// 并在返回的命名列表携带 `(name, element_type)` 供调用方在类级声明字段
/// （`InitializeComponent` 后 code-behind 可引用）。
///
/// 变量名计数器 `counter` 在递归中持续递增，保证整个 InitializeComponent
/// 内变量名唯一；`bind_counter` 为 `x:Bind` 订阅退订 token（`xbind_N`）独立
/// 递增。
///
/// `tokens`：Token 名 → 投影表索引（窗口含自适应规格时 `Some`）。`{Token X}`
/// 标记扩展在此展开为宿主绑定注册（M-U2：编译期展开为静态投影表数据）。
///
/// `style_keys`：窗口资源字典 样式键 → `_style_N` 变量名 定型映射（RFC 037）。
/// `Style={StaticResource K1, K2}` 多资源绑定据此定型：全键命中 → 直接引用
/// 注册的 Style 对象（单键直赋 / 多键 `List<Style>`，运行时零字符串查找）；
/// 任一键不可解析（App 全局/主题域）→ 逗号分隔键字符串，应用期解析链兜底。
fn emit_child_elements(
    out: &mut String,
    parent: &Element,
    parent_var: &str,
    counter: &mut usize,
    bind_counter: &mut usize,
    indent: usize,
    tokens: Option<&std::collections::BTreeMap<String, usize>>,
    style_keys: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, String> {
    let pad = " ".repeat(indent);
    let mut named_fields: Vec<(String, String)> = Vec::new();

    for child in &parent.children {
        let Some(elem) = child.as_element() else {
            continue;
        };

        // Grid 列/行定义：WPF 属性元素 <Grid.ColumnDefinitions>（RFC 037 D5.2）
        if parent.name == "Grid" && elem.name == "ColumnDefinitions" {
            emit_grid_definitions(
                out,
                parent_var,
                elem,
                "ColumnDefinitions",
                "ColumnDefinition",
                "Width",
                counter,
                indent,
            );
            continue;
        }
        if parent.name == "Grid" && elem.name == "RowDefinitions" {
            emit_grid_definitions(
                out,
                parent_var,
                elem,
                "RowDefinitions",
                "RowDefinition",
                "Height",
                counter,
                indent,
            );
            continue;
        }

        // RFC 037：属性元素资源容器（`<Window.Resources>` / `<Grid.Styles>` 等，
        // parser 将属性元素解析为 name=属性名的 Element）由 emit_window_resources
        // 递归接管——此处跳过整棵子树，避免发射 `new Resources()` 垃圾实例。
        if ResourceDictionaryDef::from_element(elem).is_some() {
            continue;
        }

        let var = format!("child_{}", *counter);
        let var_p = format!("{}_p", var);
        *counter += 1;

        // `child_N_p` 是平台镜像句柄（long），仅在元素需要平台回写时引用：
        //   - `<TextBlock Text="{x:Bind ...}"/>` 的 M4 文本同步
        // 平台树由 Window.Show() → PlatformTreeSync.BuildFromArc 才构建（RFC 037 M3.6），
        // InitializeComponent 阶段无句柄可取；rt_ui_element_set_string 对 null 句柄
        // 为安全 no-op，故初值 `0`——Arc 逻辑树侧赋值始终生效，Show 时全量同步。
        // Image.Source 不走 codegen 直写（_p 初值 0 下为 no-op 死代码）——由
        // PlatformTreeSync.Image 分支在 Show 阶段统一同步（RFC 037 M3.5/M3.6）。
        // 仅 TextBlock.Text 的 x:Bind 需要 _p（SyncText 回写平台镜像）；TextBox.Text
        // 经 BindTextBoxText 载体（自身 SyncMirrorText 路径），不声明 _p。
        let needs_p = elem.attributes.iter().any(|a| {
            a.value.as_markup().is_some_and(|m| {
                m.kind == MarkupKind::XBind && elem.name == "TextBlock" && a.name == "Text"
            })
        });

        let ctor_line = format!("{}var {} = new {}();\n", pad, var, elem.name);
        out.push_str(&ctor_line);
        // M3 样式系统：TypeName 供 StyleManager 隐式匹配
        out.push_str(&format!("{}{}.TypeName = \"{}\";\n", pad, var, elem.name));
        if needs_p {
            out.push_str(&format!("{}long {} = 0;\n", pad, var_p));
        }

        // M4：`x:Name="<name>"` → 同名私有字段引用（InitializeComponent 后
        // code-behind partial class 内可直接访问）。命名重复 → 编译期错误。
        if let Some(name_attr) = elem.attr_with_prefix("x", "Name") {
            if let Some(name) = name_attr.value.as_literal() {
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err(format!(
                        "invalid x:Name `{name}` on `<{}>` (must be a valid identifier)",
                        elem.name
                    ));
                }
                if named_fields.iter().any(|(n, _)| n.as_str() == name) {
                    return Err(format!("duplicate x:Name `{name}` in window"));
                }
                named_fields.push((name.to_string(), elem.name.to_string()));
                out.push_str(&format!("{}this.{} = {};\n", pad, name, var));
            }
        }

        // 设置属性——无前缀：Arc DP；有前缀/dotted：附加属性
        for attr in &elem.attributes {
            // 标记扩展（如 {x:Bind Title}、{Binding Title}、{Token X}）
            if let Some(markup) = attr.value.as_markup() {
                if markup.kind == MarkupKind::Token {
                    // M-U2：`{Token X}` 编译期展开为投影表数据 + 宿主绑定注册。
                    // 绑定到窗口级宿主（求值器重算 → SetValue → Observe → 局部重绘，
                    // §5.2/§11.5）；元素 DP 槽应用待 Element DP 运行时就绪后接线。
                    let name = markup.args.first().map(|a| a.as_str()).unwrap_or("");
                    match tokens {
                        Some(map) => match map.get(name) {
                            Some(id) => {
                                out.push_str(&format!(
                                    "{0}this._adaptiveHost.BindToken(\"{1}\", {2}, 0);\n",
                                    pad, attr.name, id
                                ));
                            }
                            None => {
                                return Err(format!(
                                    "undefined Token `{name}` on `<{0} {1}>` (declared in Resources)",
                                    elem.name, attr.name
                                ));
                            }
                        },
                        None => {
                            return Err(format!(
                                "`{{Token}}` on `<{0} {1}>` requires an adaptive window spec",
                                elem.name, attr.name
                            ));
                        }
                    }
                    continue;
                }
                // RFC 037：显式样式多资源绑定脱糖——`Style={StaticResource K1, K2}`。
                // 编译定型：全部键均命中窗口资源字典的样式定义 → 直接引用注册的
                // _style_N 对象（单键直赋 / 多键 List<Style>，运行时零字符串查找）；
                // 任一键不可解析（App 全局/主题域）→ 逗号分隔键字符串，应用期由
                // StyleManager 显式趟经解析链逐键解析。必须先于 emit_xbind_attr
                // （其余 markup 报错）。
                if markup.kind == MarkupKind::StaticResource && attr.name == "Style" {
                    if markup.args.is_empty() {
                        return Err(
                            "`{StaticResource}` in `Style` requires a resource key (e.g., `{StaticResource CardStyle}`)"
                                .to_string(),
                        );
                    }
                    let resolved: Option<Vec<&String>> = markup
                        .args
                        .iter()
                        .map(|k| style_keys.get(k.as_str()))
                        .collect();
                    match resolved {
                        Some(vars) if vars.len() == 1 => {
                            out.push_str(&format!("{}{}.Style = {};\n", pad, var, vars[0]));
                        }
                        Some(vars) => {
                            let list_var = format!("_style_refs_{}", *bind_counter);
                            *bind_counter += 1;
                            out.push_str(&format!(
                                "{}var {} = new List<Style>();\n",
                                pad, list_var
                            ));
                            for style_var in vars {
                                out.push_str(&format!(
                                    "{}{}.Add({});\n",
                                    pad, list_var, style_var
                                ));
                            }
                            out.push_str(&format!("{}{}.Style = {};\n", pad, var, list_var));
                        }
                        None => {
                            let joined = markup
                                .args
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(",");
                            out.push_str(&format!(
                                "{}{}.Style = \"{}\";\n",
                                pad,
                                var,
                                escape_arc_string(&joined)
                            ));
                        }
                    }
                    continue;
                }
                emit_xbind_attr(
                    out,
                    &elem.name,
                    &attr.name,
                    &var,
                    &var_p,
                    markup,
                    bind_counter,
                    &pad,
                )?;
                continue;
            }
            let Some(val) = attr.value.as_literal() else {
                continue;
            };
            if let Some(prefix) = &attr.prefix {
                if prefix.as_str() == "x" {
                    continue;
                }
                let attached_key = format!("{}.{}", prefix, attr.name);
                emit_attached_property(out, &var, &attached_key, &attr.name, val, &pad);
                continue;
            }
            if attr.name.contains('.') {
                emit_attached_property(out, &var, attr.name.as_str(), &attr.name, val, &pad);
                continue;
            }
            // RFC 037 D2.4 / D10.4：ARML Click="Method" → code-behind 实例方法组绑定。
            if elem.name == "Button" && attr.name == "Click" {
                out.push_str(&format!("{}{}.OnClick(_ => this.{}());\n", pad, var, val));
                continue;
            }
            let formatted = format_attr_value(&attr.name, val);
            out.push_str(&format!("{}{}.{} = {};\n", pad, var, attr.name, formatted));
        }

        // 递归处理子元素
        let nested = emit_child_elements(
            out,
            elem,
            &var,
            counter,
            bind_counter,
            indent,
            tokens,
            style_keys,
        )?;
        named_fields.extend(nested);

        // 添加到父元素（Element.AddChild 设置 child.Parent 并追加到 Children）
        out.push_str(&format!("{}{}.AddChild({});\n", pad, parent_var, var));
    }
    Ok(named_fields)
}

/// 发射 Grid.ColumnDefinitions / Grid.RowDefinitions 属性元素赋值。
fn emit_grid_definitions(
    out: &mut String,
    grid_var: &str,
    prop_elem: &Element,
    field_name: &str,
    def_type: &str,
    size_attr: &str,
    counter: &mut usize,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    let list_var = format!("grid_def_{}", *counter);
    *counter += 1;
    out.push_str(&format!("{}var {} = new List<object>();\n", pad, list_var));
    for child in prop_elem.child_elements() {
        if child.name != def_type {
            continue;
        }
        let def_var = format!("child_{}", *counter);
        *counter += 1;
        out.push_str(&format!("{}var {} = new {}();\n", pad, def_var, def_type));
        if let Some(val) = child.attr(size_attr).and_then(|a| a.value.as_literal()) {
            out.push_str(&format!(
                "{}{}.{} = GridLength.Parse(\"{}\");\n",
                pad,
                def_var,
                size_attr,
                escape_arc_string(val)
            ));
        }
        out.push_str(&format!("{}{}.Add({});\n", pad, list_var, def_var));
    }
    out.push_str(&format!(
        "{}{}.{} = {};\n",
        pad, grid_var, field_name, list_var
    ));
}

/// 发射附加属性赋值（Arc 逻辑树 SetAttachedNumber/SetAttachedString；Grid.Row/Column 例外）。
///
/// RFC 019：`Grid.Row` / `Grid.Column` 为 typed `DependencyProperty<int>`，发射宿主
/// 静态访问器 `Grid.SetRow(child_N, 2)` / `Grid.SetColumn(child_N, 0)`，替代
/// `SetAttachedNumber` string 键路径。仅以**完整附加键**（attached_key）判定宿主，
/// `Foo.Row` 等未知宿主不落入 typed 分支。非整数字面量由 typeck 层拒绝
/// （RFC 019 §1.2）；此处整数 parse 失败时走既有数值/字符串路径兜底（防御）。
fn emit_attached_property(
    out: &mut String,
    var: &str,
    attached_key: &str,
    local_name: &str,
    val: &str,
    pad: &str,
) {
    let local = if local_name.contains('.') {
        local_name.rsplit('.').next().unwrap_or(local_name)
    } else {
        local_name
    };
    if attached_key == "Grid.Row" || attached_key == "Grid.Column" {
        if let Ok(n) = val.parse::<i64>() {
            let setter = if attached_key == "Grid.Row" {
                "SetRow"
            } else {
                "SetColumn"
            };
            out.push_str(&format!("{}Grid.{}({}, {});\n", pad, setter, var, n));
            return;
        }
    }
    if val.parse::<f64>().is_ok()
        && (is_numeric_attr(local)
            || local == "Row"
            || local == "Column"
            || local == "RowSpan"
            || local == "ColumnSpan")
    {
        let formatted = format_attached_number(val);
        out.push_str(&format!(
            "{}{}.SetAttachedNumber(\"{}\", {});\n",
            pad, var, attached_key, formatted
        ));
    } else {
        out.push_str(&format!(
            "{}{}.SetAttachedString(\"{}\", \"{}\");\n",
            pad,
            var,
            attached_key,
            escape_arc_string(val)
        ));
    }
}

/// `x:Bind` Mode 参数（RFC 037 D4.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XBindMode {
    OneTime,
    OneWay,
    TwoWay,
}

fn xbind_mode(ext: &MarkupExtension) -> Result<XBindMode, String> {
    for (key, val) in &ext.properties {
        if key == "Mode" {
            return match val.as_str() {
                "OneTime" => Ok(XBindMode::OneTime),
                "OneWay" => Ok(XBindMode::OneWay),
                "TwoWay" => Ok(XBindMode::TwoWay),
                "OneWayToSource" => Err(format!(
                    "x:Bind Mode={val} is not supported in RFC 026 M4 slice (OneTime/OneWay/TwoWay only)"
                )),
                other => Err(format!(
                    "invalid x:Bind Mode `{other}`, expected OneTime/OneWay/TwoWay"
                )),
            };
        }
    }
    Ok(XBindMode::OneWay)
}

/// 将 `{x:Bind path}` 脱糖为强类型 code-behind 属性访问（RFC 037 M4）。
///
/// M4 垂直切片目标（对齐 027 §13 / 042 §2.3）：
///   - `<TextBlock Text="{x:Bind Prop}"/>`：`SyncText` 切片（逻辑树 + 平台镜像），
///     OneTime 仅初值；OneWay/TwoWay 追加 `ObserveProperty` 订阅 + G2 卸载退订；
///     TextBlock 无输入通道，TwoWay 与 OneWay 等价（仅源→目标）。
///   - `<TextBox Text="{x:Bind Prop}"/>`：VM→UI 经运行时载体
///     `BindingOperations.BindTextBoxText`（初始值 + 订阅 + G2 退订，经 Text
///     setter 路由编辑内核）；TwoWay 追加内联 `OnTextChanged` 写回 VM setter
///     （相等性守卫防回环）。
///
/// 绑定源 = code-behind `[Observable]` 属性（`this.ObserveProperty("Prop")`
/// 静态定址，编译器管理生命周期，G2 退订）。
fn emit_xbind_attr(
    out: &mut String,
    elem_type: &str,
    attr_name: &str,
    target_var: &str,
    target_p_var: &str,
    ext: &MarkupExtension,
    bind_counter: &mut usize,
    pad: &str,
) -> Result<(), String> {
    match ext.kind {
        MarkupKind::XBind => {}
        MarkupKind::Binding => {
            return Err(
                "`{Binding}` runtime binding is not supported; use `{x:Bind}` (RFC 037 M4 compile-time binding)".into(),
            );
        }
        other => {
            return Err(format!(
                "markup extension `{}` is not supported on `<{elem_type} {attr_name}=...>`",
                other.as_str()
            ));
        }
    }

    if ext.args.is_empty() {
        return Err("`x:Bind` requires a binding path (e.g., `{x:Bind Title}`)".into());
    }
    let path = ext.args[0].as_str();
    if path.contains('.') {
        return Err(format!(
            "nested x:Bind path `{path}` is not supported in RFC 026 M4 slice (single property only)"
        ));
    }
    if !path.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("invalid x:Bind path `{path}`"));
    }

    let mode = xbind_mode(ext)?;
    // 绑定源：code-behind `[Observable]` 属性（编译器静态定址通道，零运行期字符串解析）。
    let prop = format!("this.{path}");
    let src = format!("this.ObserveProperty(\"{}\")", path);

    match (elem_type, attr_name) {
        ("TextBlock", "Text") => {
            emit_xbind_text(out, target_var, target_p_var, &src, mode, bind_counter, pad);
            Ok(())
        }
        ("TextBox", "Text") => {
            emit_xbind_textbox(out, target_var, &src, &prop, mode, bind_counter, pad);
            Ok(())
        }
        _ => Err(format!(
            "x:Bind on `<{elem_type} {attr_name}=...>` is not supported in RFC 026 M4 slice (only `<TextBlock Text={{x:Bind ...}}/>` and `<TextBox Text={{x:Bind ...}}/>`)"
        )),
    }
}

/// `Text.Text="{x:Bind Prop}"` 脱糖形态（RFC 037 M4，运行时载体扩展）：
///
/// ```arc
/// long child_0_p = 0;  // 平台镜像句柄占位（long；Show() 后由镜像树持有，0 = 未构建）
/// BindingOperations.SyncText(child_0, child_0_p, this.ObserveProperty("Prop").Value.ToString());
/// int xbind_0 = BindingOperations.BindText(child_0, child_0_p, this.ObserveProperty("Prop"));
/// ```
///
/// `child_0_p` 声明由 `emit_child_elements` 在元素需要平台回写时生成（初值 `0`，
/// 句柄在 Window.Show() → PlatformTreeSync.BuildFromArc 阶段才创建；运行时对
/// null 句柄为安全 no-op）。OneTime 仅初值行（SyncText）；OneWay/TwoWay 经
/// `BindingOperations.BindText` 运行时载体（初始 SyncText + 源订阅 + G2 卸载
/// 退订登记，一次调用内完成）。订阅/退订回调只捕获绑定 id（int），不捕获
/// 类引用——规避逃逸闭包 ByRef 捕获悬垂（RFC 006 M4 报告；与手动
/// `Subscribe(v => SyncText(child_0, ...))` 形态相对，后者跨函数逃逸后 AV）。
/// `xbind_N` 为 `InitializeComponent` 内载体返回的退订 token（BindText 已
/// 内部登记 G2 退订，token 变量仅保序占位）。
fn emit_xbind_text(
    out: &mut String,
    target_var: &str,
    target_p_var: &str,
    src: &str,
    mode: XBindMode,
    bind_counter: &mut usize,
    pad: &str,
) {
    out.push_str(&format!(
        "{}BindingOperations.SyncText({}, {}, {}.Value.ToString());\n",
        pad, target_var, target_p_var, src
    ));

    if mode != XBindMode::OneTime {
        let token = format!("xbind_{}", *bind_counter);
        *bind_counter += 1;
        out.push_str(&format!(
            "{}int {} = BindingOperations.BindText({}, {}, {});\n",
            pad, token, target_var, target_p_var, src
        ));
    }
}

/// `TextBox.Text="{x:Bind Prop}"` 脱糖形态（RFC 037 M4 / §8 修订 text-editing.md）：
///
/// ```arc
/// int xbind_0 = BindingOperations.BindTextBoxText(child_0, this.ObserveProperty("Prop"));  // OneWay / VM→UI 半边
/// child_0.OnTextChanged((x: string) => { if (this.Prop != x) { this.Prop = x; } });        // TwoWay UI→VM 写回
/// child_0.Text = this.Prop;                                                                // OneTime
/// ```
///
/// **VM→UI 半边一律经 `BindTextBoxText` 载体**——`TextBox.Text` setter 路由编辑
/// 内核（TextBoxModel 唯一真相：初始值/订阅回写/撤销快照/回声同值早退全链），
/// **不得**退回裸 `SetBinding` + `SetValue(DP)`（绕过内核 → 状态失同步：DP 与
/// model 文本分叉、撤销栈污染、TextChanged 丢失）。载体回调只捕获绑定 id（int），
/// G2 卸载退订登记在 target 生命周期。
///
/// **TwoWay = OneWay 载体 + 独立的 UI→VM 写回表面**：UI→VM 写回经内联
/// `OnTextChanged` **捕获 `this`（堆对象，跨函数逃逸安全）调编译器合成
/// setter**——RFC 037 §5.3：「codegen 写回 VM setter → 合成通知闭环」，与
/// `data_driven_twoway_e2e` 的 `vm.Name = x` 同一路径。相等性守卫
/// `if (this.Prop != x)` 防回环（`TextBox.Text` setter 无条件触发
/// `TextChanged`、string 属性无相等性短路；VM 回声 → 载体 Apply 同值 →
/// 内核 SetText 同值早退，双重收敛）。
fn emit_xbind_textbox(
    out: &mut String,
    target_var: &str,
    src: &str,
    prop: &str,
    mode: XBindMode,
    bind_counter: &mut usize,
    pad: &str,
) {
    match mode {
        XBindMode::OneTime => {
            out.push_str(&format!("{}{}.Text = {};\n", pad, target_var, prop));
        }
        XBindMode::OneWay => {
            let token = format!("xbind_{}", *bind_counter);
            *bind_counter += 1;
            out.push_str(&format!(
                "{}int {} = BindingOperations.BindTextBoxText({}, {});\n",
                pad, token, target_var, src
            ));
        }
        XBindMode::TwoWay => {
            // VM→UI：BindTextBoxText 载体（初始值 + 订阅 + G2 退订；经 Text setter 路由内核）。
            let token = format!("xbind_{}", *bind_counter);
            *bind_counter += 1;
            out.push_str(&format!(
                "{}int {} = BindingOperations.BindTextBoxText({}, {});\n",
                pad, token, target_var, src
            ));
            // UI→VM：内联 OnTextChanged 写回 VM setter（捕获 this = 堆对象，跨函数安全；
            // 相等性守卫防回环）。
            out.push_str(&format!(
                "{}{}.OnTextChanged((x: string) => {{\n",
                pad, target_var
            ));
            out.push_str(&format!("{}    if ({} != x) {{\n", pad, prop));
            out.push_str(&format!("{}        {} = x;\n", pad, prop));
            out.push_str(&format!("{}    }}\n", pad));
            out.push_str(&format!("{}}});\n", pad));
        }
    }
}

/// 格式化附加数值属性为 double 字面量（SetAttachedNumber 第二参数须为 double）。
fn format_attached_number(val: &str) -> String {
    if let Ok(n) = val.parse::<f64>() {
        if n.fract() == 0.0 {
            return format!("{:.1}", n);
        }
        return val.to_string();
    }
    "0.0".to_string()
}

/// 判定属性是否为数值类型（与 format_attr_value 数字属性列表一致）。
fn is_numeric_attr(name: &str) -> bool {
    matches!(
        name,
        "Width"
            | "Height"
            | "FontSize"
            | "Spacing"
            | "ColumnSpacing"
            | "RowSpacing"
            | "Left"
            | "Top"
            | "RadiusX"
            | "RadiusY"
            | "StrokeThickness"
            | "MinWidth"
            | "MaxWidth"
            | "MinHeight"
            | "MaxHeight"
            | "HorizontalOffset"
            | "VerticalOffset"
            | "Minimum"
            | "Maximum"
            | "Value"
            | "MaxLength"
            | "Row"
            | "Column"
            | "RowSpan"
            | "ColumnSpan"
    )
}

/// 判定属性是否为布尔类型（与 format_attr_value bool 属性列表一致）。
fn is_bool_attr(name: &str) -> bool {
    matches!(
        name,
        "IsEnabled"
            | "IsChecked"
            | "IsVisible"
            | "IsThreeState"
            | "IsReadOnly"
            | "IsDefault"
            | "IsCancel"
            | "Focusable"
            | "IsTabStop"
    )
}

/// 返回强类型枚举属性所属的枚举类型名；非枚举属性返回 None。
/// 对标 WPF 强类型枚举（Orientation/HorizontalAlignment/VerticalAlignment/
/// Stretch/ScrollBarVisibility），codegen 据此发出 `EnumType.Member` 而非字符串。
fn enum_type_for(name: &str) -> Option<&'static str> {
    match name {
        "Orientation" => Some("Orientation"),
        "HorizontalAlignment" | "HorizontalContentAlignment" => Some("HorizontalAlignment"),
        "VerticalAlignment" | "VerticalContentAlignment" => Some("VerticalAlignment"),
        "Stretch" => Some("Stretch"),
        "HorizontalScrollBarVisibility" | "VerticalScrollBarVisibility" => {
            Some("ScrollBarVisibility")
        }
        _ => None,
    }
}

/// 将枚举成员值格式化为 `EnumType.Member`；值非法则返回 None（回退字符串字面量，
/// 交由 Arc 侧对强类型 DP 赋字符串触发清晰编译错误）。
fn format_enum_value(enum_type: &str, val: &str) -> Option<String> {
    let valid = |members: &[&str]| members.contains(&val);
    match enum_type {
        "Orientation" => {
            if valid(&["Horizontal", "Vertical"]) {
                return Some(format!("Orientation.{val}"));
            }
        }
        "HorizontalAlignment" => {
            if valid(&["Left", "Center", "Right", "Stretch"]) {
                return Some(format!("HorizontalAlignment.{val}"));
            }
        }
        "VerticalAlignment" => {
            if valid(&["Top", "Center", "Bottom", "Stretch"]) {
                return Some(format!("VerticalAlignment.{val}"));
            }
        }
        "Stretch" => {
            if valid(&["None", "Fill", "Uniform", "UniformToFill"]) {
                return Some(format!("Stretch.{val}"));
            }
        }
        "ScrollBarVisibility" if valid(&["Disabled", "Auto", "Hidden", "Visible"]) => {
            return Some(format!("ScrollBarVisibility.{val}"));
        }
        _ => {}
    }
    None
}

/// 格式化 ARML 属性值为 Arc 字面量。
///
/// - 数字属性（Width/Height/FontSize/Spacing/Left/Top）→ 数字字面量
/// - bool 属性（IsEnabled/IsChecked/IsVisible）→ true/false
/// - 强类型枚举属性（Orientation/Alignment/Stretch/ScrollBarVisibility）→ `EnumType.Member`
/// - 其他 → 字符串字面量（带转义）
fn format_attr_value(name: &str, val: &str) -> String {
    // 数字属性（double 字段）
    if is_numeric_attr(name) {
        // 验证为数字，失败则当作字符串
        if val.parse::<f64>().is_ok() {
            // 整数则去尾 .0，浮点保留原样
            if let Ok(n) = val.parse::<f64>() {
                if n.fract() == 0.0 {
                    return format!("{}", n as i64);
                }
                return val.to_string();
            }
        }
    }
    // bool 属性
    if is_bool_attr(name) && (val == "true" || val == "false") {
        return val.to_string();
    }
    // 强类型枚举属性 → EnumType.Member
    if let Some(enum_type) = enum_type_for(name) {
        if let Some(formatted) = format_enum_value(enum_type, val) {
            return formatted;
        }
    }
    // 默认：字符串字面量
    format!("\"{}\"", escape_arc_string(val))
}

/// `<Application>` 根元素 → `partial class App : Application`，override
/// `InitializeComponent()` 创建 StartupUri 指向的窗口实例并调用其
/// `InitializeComponent()`。
///
/// `StartupUri="MainWindow.arml"` 推导为 `MainWindow` 类名（去 `.arml` 后缀，
/// 取最后一段 `/` 后部分）。生成体：
///
/// ```arc
/// public partial class App : Application {
///     public override void InitializeComponent() {
///         this.MainWindow = new MainWindow();
///         this.MainWindow.InitializeComponent();
///     }
/// }
/// ```
///
/// `<Application.Resources>` / `<Resources>` 容器内的声明式 `<Style>` 会在
/// `InitializeComponent()` 末尾等价生成运行期 `AddStyle` 注册（对标 WPF 将
/// App.xaml 资源在 InitializeComponent 阶段物化；`Application.Run() →
/// OnStartup() + ApplyImplicitStyles` 读取 Resources 应用隐式样式）：
///
/// ```arc
/// Style _style_0 = new Style();
/// _style_0.TargetType = "Button";
/// _style_0.Key = "";                    // 无 x:Key → 隐式样式
/// Setter _setter_0_0 = new Setter();
/// _setter_0_0.Property = "Background";
/// _setter_0_0.Value = SetterValue.String("#FFCC2222");
/// _style_0.Setters.Add(_setter_0_0);
/// this.Resources.AddStyle(_style_0);
/// ```
///
/// `Application` 基类提供 `Run()` 方法编排完整生命周期：
/// `InitializeComponent() → OnStartup() → MainWindow.Show() → OnExit()`。
fn generate_application_partial(doc: &ArmlDocument) -> Result<(String, String), String> {
    let class_name = extract_class_name(doc)?;

    let startup_class = doc
        .root
        .attr("StartupUri")
        .and_then(|a| a.value.as_literal())
        .map(|s| {
            // "MainWindow.arml" → "MainWindow"；"Sub/MainWindow.arml" → "MainWindow"
            let stem = s.rsplit('/').next().unwrap_or(s);
            stem.trim_end_matches(".arml").to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "MainWindow".to_string());

    let mut body = String::new();
    body.push_str(&format!(
        "public partial class {} : Application {{\n",
        class_name
    ));
    body.push_str("    public override void InitializeComponent() {\n");
    body.push_str(&format!(
        "        this.MainWindow = new {}();\n",
        startup_class
    ));
    body.push_str("        this.MainWindow.InitializeComponent();\n");
    emit_application_styles(&mut body, doc, /*indent=*/ 8)?;
    emit_application_themes(&mut body, doc, /*indent=*/ 8);
    body.push_str("    }\n");
    body.push_str("}\n");

    Ok((class_name, body))
}

/// 发射窗口局部资源容器（`<Window.Resources>` / `<Styles>`，对照
/// `ResourceDictionaryDef::from_element`）为运行期 `ResourceDictionary` 构造
/// 代码：类型化条目 `Add` + merged 子字典 + 声明式 `Style` 注册。
///
/// 容器收集为**递归**语义：顶层属性元素（`<Window.Resources>`）与嵌套属性
/// 元素（`<Grid.Styles>`）一并上浮注册进窗口级字典——运行时 StyleManager
/// 以 `MainWindow.Resources` 为唯一 primary 解析域（无子树样式域），嵌套
/// 容器静默降级会导致样式定义丢失（禁止静默吞掉）。
///
/// `FrameworkElement.Resources` 为 object DP——条目/样式发射必须经**强类型
/// 局部变量**（`var _resources`）静态解析 `Add`/`AddStyle`，随后一次赋值。
/// 自适应投影条目（`<Double x:Key=...><Match .../></Double>`，value 为 `None`）
/// 不走运行时字典，由 Token 投影机制接管（M-U2）。窗口域无 ThemeDictionaries
/// 注册通道——keyed 子字典在此报错（仅 Application 域支持）。
fn emit_window_resources(
    out: &mut String,
    doc: &ArmlDocument,
    indent: usize,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut containers = Vec::new();
    collect_resource_containers(&doc.root, &mut containers);
    let mut style_keys = std::collections::BTreeMap::new();
    if containers.is_empty() {
        return Ok(style_keys);
    }
    let pad = " ".repeat(indent);
    // 定型映射：与下方发射遍历同序（merged 先于同级 styles）独立推进编号，
    // 得到 样式键 → `_style_N` 变量名 的完全一致对应，供元素端
    // `Style={StaticResource ...}` 编译定型为对象引用。
    let mut map_index = 0usize;
    for dict in &containers {
        collect_style_key_vars(dict, &mut style_keys, &mut map_index);
    }
    let mut style_index = 0usize;
    let mut merged_index = 0usize;
    out.push_str(&format!(
        "{0}// ---- 窗口局部资源字典（RFC 037：样式解析 primary 域）----\n",
        pad
    ));
    out.push_str(&format!(
        "{0}var _resources = new ResourceDictionary();\n",
        pad
    ));
    out.push_str(&format!("{0}this.Resources = _resources;\n", pad));
    for dict in &containers {
        emit_resource_dictionary(
            out,
            dict,
            "_resources",
            &mut style_index,
            &mut merged_index,
            indent,
        )?;
    }
    Ok(style_keys)
}

/// 预扫描字典树，收集 样式键 → `_style_N` 变量名 映射（编号推进顺序与
/// `emit_resource_dictionary` 的发射顺序一致：merged 子字典先于同级 styles）。
/// 隐式样式（无 x:Key）不参与键引用，不进映射。
fn collect_style_key_vars(
    dict: &ResourceDictionaryDef,
    map: &mut std::collections::BTreeMap<String, String>,
    style_index: &mut usize,
) {
    for merged in &dict.merged {
        collect_style_key_vars(merged, map, style_index);
    }
    for style in &dict.styles {
        if let Some(key) = &style.key {
            map.insert(key.to_string(), format!("_style_{}", *style_index));
        }
        *style_index += 1;
    }
}

/// 递归收集元素树中的资源容器（裸 `<ResourceDictionary>`、属性元素
/// `<X.Resources>` / `<X.Styles>`，对照 `ResourceDictionaryDef::from_element`）。
/// 命中容器后不再深入——容器内部条目已由 `parse_scope_container` /
/// `parse_resource_dictionary` 完整解析（嵌套子字典为 merged），避免重复收集。
fn collect_resource_containers(elem: &Element, containers: &mut Vec<ResourceDictionaryDef>) {
    for child in elem.child_elements() {
        if let Some(dict) = ResourceDictionaryDef::from_element(child) {
            containers.push(dict);
        } else {
            collect_resource_containers(child, containers);
        }
    }
}

/// 递归发射字典内容（条目 → merged 子字典 → 声明式 Style）到 `dict_target`。
/// 外部文件引用（`<ResourceDictionary Source=...>`）尚无解析机制，明确报错
/// （禁止静默吞掉导致引用悬空）。
fn emit_resource_dictionary(
    out: &mut String,
    dict: &ResourceDictionaryDef,
    dict_target: &str,
    style_index: &mut usize,
    merged_index: &mut usize,
    indent: usize,
) -> Result<(), String> {
    let pad = " ".repeat(indent);
    for entry in &dict.entries {
        let Some(v) = &entry.value else {
            continue; // 自适应投影条目（`<Match>` 子元素），由 Token 机制接管
        };
        let val = format_resource_value(entry.type_name.as_str(), v);
        out.push_str(&format!(
            "{}{}.Add(\"{}\", {});\n",
            pad,
            dict_target,
            escape_arc_string(entry.key.as_str()),
            val
        ));
    }
    for merged in &dict.merged {
        if let Some(source) = &merged.source {
            return Err(format!(
                "external ResourceDictionary `Source=\"{}\"` is not supported in window resources",
                source.as_str()
            ));
        }
        let sub_var = format!("_merged_{}", *merged_index);
        *merged_index += 1;
        out.push_str(&format!(
            "{}var {} = new ResourceDictionary();\n",
            pad, sub_var
        ));
        emit_resource_dictionary(out, merged, &sub_var, style_index, merged_index, indent)?;
        out.push_str(&format!(
            "{}{}.MergedDictionaries.Add({});\n",
            pad, dict_target, sub_var
        ));
    }
    if !dict.theme_entries.is_empty() {
        return Err(
            "keyed `<ResourceDictionary>` / `<ThemeDictionaries>` are only supported at Application scope"
                .to_string(),
        );
    }
    for style in &dict.styles {
        emit_style_registration(out, style, style_index, &pad, dict_target)?;
    }
    Ok(())
}

/// 解析 `<Application>` 根下资源容器（`<Application.Resources>` / `<Resources>` /
/// `<ResourceDictionary>`，对照 `ResourceDictionaryDef::from_element`）中的声明式
/// `<Style>`，生成等价运行期 `AddStyle` 注册代码（Part A：declarative Style）。
///
/// 每个 `<Style>` → `Style` 实例 + `Setter` 实例 + `this.Resources.AddStyle(...)`，
/// 变量名风格对齐现有 codegen（`child_N` / `_adaptiveHost`）：`_style_N` / `_setter_N_M`。
fn emit_application_styles(
    out: &mut String,
    doc: &ArmlDocument,
    indent: usize,
) -> Result<(), String> {
    let pad = " ".repeat(indent);
    let mut style_index = 0usize;
    for container in doc.root.child_elements() {
        let Some(dict) = ResourceDictionaryDef::from_element(container) else {
            continue;
        };
        for style in dict.all_styles() {
            emit_style_registration(out, style, &mut style_index, &pad, "this.Resources")?;
        }
    }
    Ok(())
}

/// 发射单个 `<Style>` 的等价运行期注册代码（`Style` + `Setter` 实例化 →
/// `dict_target.AddStyle`）。`dict_target` 为承载字典的表达式：Application 域
/// `this.Resources`（强类型属性）或窗口域 `_resources` 局部变量（object DP 需
/// 局部强类型承载才能静态解析方法调用）。Setter 值为 markup 扩展时仅
/// `{StaticResource key}` 合法（发射 `SetterValue.StaticResource`，应用期按
/// 活动主题解析），其余报错。
fn emit_style_registration(
    out: &mut String,
    style: &StyleDef,
    style_index: &mut usize,
    pad: &str,
    dict_target: &str,
) -> Result<(), String> {
    let style_var = format!("_style_{}", *style_index);
    out.push_str(&format!("{}Style {} = new Style();\n", pad, style_var));
    if let Some(target) = &style.target_type {
        out.push_str(&format!(
            "{}{}.TargetType = \"{}\";\n",
            pad,
            style_var,
            escape_arc_string(target.as_str())
        ));
    }
    // RFC 037：BasedOn 继承——`{StaticResource ParentKey}` 或字面量 `ParentKey`
    // 均发射父样式键；应用期 StyleManager 经解析链 LookupStyle 父先子后
    // （提取逻辑与 verify::style_based_on_key 同构；环检测已前置）。
    if let Some(based_on) = &style.based_on {
        let parent_key = match based_on {
            AttributeValue::Literal(s) => Some(s.as_str()),
            AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::StaticResource => {
                ext.args.first().map(|s| s.as_str())
            }
            _ => None,
        };
        let Some(parent_key) = parent_key else {
            return Err(
                "`BasedOn` on `<Style>` requires `{StaticResource ParentKey}` or a literal key"
                    .to_string(),
            );
        };
        out.push_str(&format!(
            "{}{}.BasedOn = \"{}\";\n",
            pad,
            style_var,
            escape_arc_string(parent_key)
        ));
    }
    // 无 x:Key → 隐式样式（Key = ""）；有 x:Key → 具名样式。
    let key = style.key.as_deref().unwrap_or("");
    out.push_str(&format!(
        "{}{}.Key = \"{}\";\n",
        pad,
        style_var,
        escape_arc_string(key)
    ));
    for (setter_idx, setter) in style.setters.iter().enumerate() {
        let setter_var = format!("_setter_{}_{}", *style_index, setter_idx);
        out.push_str(&format!("{}Setter {} = new Setter();\n", pad, setter_var));
        out.push_str(&format!(
            "{}{}.Property = \"{}\";\n",
            pad,
            setter_var,
            escape_arc_string(setter.property.as_str())
        ));
        match &setter.value {
            AttributeValue::Literal(val) => {
                out.push_str(&format!(
                    "{}{}.Value = {};\n",
                    pad,
                    setter_var,
                    format_setter_value(val)
                ));
            }
            AttributeValue::MarkupExtension(ext) => match ext.kind {
                MarkupKind::StaticResource => {
                    let Some(key) = ext.args.first() else {
                        return Err(format!(
                            "`{{{}}}` in `<Setter Value=...>` requires a resource key (e.g., `{{{}}}`)",
                            ext.kind.as_str(),
                            ext.kind.as_str()
                        ));
                    };
                    out.push_str(&format!(
                        "{}{}.Value = SetterValue.{}(\"{}\");\n",
                        pad,
                        setter_var,
                        ext.kind.as_str(),
                        escape_arc_string(key.as_str())
                    ));
                }
                other => {
                    return Err(format!(
                        "markup extension `{}` is not supported in `<Setter Value=...>` (only `{{StaticResource}}`)",
                        other.as_str()
                    ));
                }
            },
        }
        out.push_str(&format!(
            "{}{}.Setters.Add({});\n",
            pad, style_var, setter_var
        ));
    }
    out.push_str(&format!("{}{}.AddStyle({});\n", pad, dict_target, style_var));
    *style_index += 1;
    Ok(())
}

/// 将 `<Setter Value="..."/>` 字面量格式化为 `SetterValue` 工厂调用
/// （对照 `std/UI/Core/Styling/SetterValue.as` 的 variant 构造器）。
///
/// - 数字字面量 → `SetterValue.Number(double)`（整数补 `.0`）
/// - `true`/`false` → `SetterValue.Boolean(bool)`
/// - 其他（颜色、字体名等字符串）→ `SetterValue.String("...")`
fn format_setter_value(val: &str) -> String {
    if val == "true" || val == "false" {
        return format!("SetterValue.Boolean({val})");
    }
    if let Ok(n) = val.parse::<f64>() {
        if n.fract() == 0.0 {
            return format!("SetterValue.Number({}.0)", n as i64);
        }
        return format!("SetterValue.Number({val})");
    }
    format!("SetterValue.String(\"{}\")", escape_arc_string(val))
}

/// 解析 `<Application.Themes>/<Theme ...>` 声明，在**编译期**确定每个主题的聚合结果。
///
/// 基于 WPF「覆盖键即定制」语义，但避免运行期多层层叠 Merged（第三方库多封装覆盖时
/// O(层×条目) 开销）。编译器将 `BasedOn` 继承链扁平化：每个 `<Theme>` 发射一个**平坦**
/// `ResourceDictionary`（基底工厂 + 沿链全量覆盖 `Add`，后声明覆盖同名键），
/// `RegisterTheme` 只做纯存储（无合并）。运行时切主题即 O(1) 换引用，零覆盖赋值。
fn emit_application_themes(out: &mut String, doc: &ArmlDocument, indent: usize) {
    let pad = " ".repeat(indent);
    let themes = doc.collect_themes();
    let mut index_of: HashMap<&str, usize> = HashMap::new();
    for (i, t) in themes.iter().enumerate() {
        index_of.insert(t.key.as_str(), i);
    }
    for (theme_index, theme) in themes.iter().enumerate() {
        let theme_var = format!("_theme_{}", theme_index);
        let (base_expr, chain) = resolve_theme_chain(theme, &themes, &index_of);
        out.push_str(&format!(
            "{}ResourceDictionary {} = {};\n",
            pad, theme_var, base_expr
        ));
        for link in &chain {
            for dict in &link.dictionaries {
                for entry in &dict.entries {
                    if let Some(v) = &entry.value {
                        let val = format_resource_value(entry.type_name.as_str(), v);
                        out.push_str(&format!(
                            "{}{}.Add(\"{}\", {});\n",
                            pad,
                            theme_var,
                            escape_arc_string(entry.key.as_str()),
                            val
                        ));
                    }
                }
            }
        }
        out.push_str(&format!(
            "{}this.ThemeDictionaries.RegisterTheme(\"{}\", {});\n",
            pad,
            escape_arc_string(theme.key.as_str()),
            theme_var
        ));
    }
}

/// 编译期解析主题的 `BasedOn` 继承链，返回 `(基底构造表达式, root-first 覆盖链)`。
///
/// 基底：内置 Light/Dark（`BuiltInTheme.CreateLight/CreateDark` 工厂）或空字典（独立主题）。
/// 覆盖链按 root-first 展开（基底最近的覆盖先 Add，后声明覆盖同名键），把多层继承
/// 折叠为一次平坦构造，消除运行期逐层的 `ResourceDictionary.Merged`。
fn resolve_theme_chain<'a>(
    theme: &'a ThemeDef,
    themes: &'a [ThemeDef],
    index_of: &HashMap<&str, usize>,
) -> (String, Vec<&'a ThemeDef>) {
    let mut chain: Vec<&ThemeDef> = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut base: Option<&str> = None;
    let mut current: Option<&ThemeDef> = Some(theme);
    while let Some(cur) = current {
        if !visited.insert(cur.key.as_str()) {
            break; // 环保护（不应出现）
        }
        chain.push(cur);
        match &cur.based_on {
            Some(b) if b == "Light" || b == "Dark" => {
                base = Some(b.as_str());
                current = index_of.get(b.as_str()).map(|&i| &themes[i]);
            }
            Some(b) => {
                current = index_of.get(b.as_str()).map(|&i| &themes[i]);
            }
            None => {
                if cur.key.as_str() == "Light" || cur.key.as_str() == "Dark" {
                    base = Some(cur.key.as_str());
                }
                current = None;
            }
        }
    }
    chain.reverse(); // root-first
    let base_expr = match base {
        Some("Light") => "BuiltInTheme.CreateLight()".to_string(),
        Some("Dark") => "BuiltInTheme.CreateDark()".to_string(),
        _ => "new ResourceDictionary()".to_string(),
    };
    (base_expr, chain)
}

/// 将主题资源条目格式化（对照 `ResourceValue` variant，见 `std/UI/Core/Styling/ResourceValue.as`）。
///
/// - 数值类型（`Double`/`Integer`）→ `ResourceValue.Number(double)`
/// - 颜色画刷类型（`Color`/`Brush`/`SolidColorBrush`）→ `ResourceValue.Brush(Brushes.Parse(...))`
///   （**类型化 IBrush**，非字符串；命名色/hex 由 Brushes 单一来源解析）
/// - 其余（`String`/`Boolean` 等）→ `ResourceValue.String("...")`
fn format_resource_value(type_name: &str, value: &str) -> String {
    match type_name {
        "Double" | "Integer" => {
            if let Ok(n) = value.parse::<f64>() {
                if n.fract() == 0.0 {
                    return format!("ResourceValue.Number({}.0)", n as i64);
                }
                return format!("ResourceValue.Number({value})");
            }
            "ResourceValue.Number(0.0)".to_string()
        }
        "Color" | "Brush" | "SolidColorBrush" => {
            format!(
                "ResourceValue.Brush(Brushes.Parse(\"{}\"))",
                escape_arc_string(value)
            )
        }
        _ => format!("ResourceValue.String(\"{}\")", escape_arc_string(value)),
    }
}

/// 单文档生成完整 `.as` 内容（含头部 namespace/using + 单 partial class）。
///
/// 用于单 ARML 输入场景（如单元测试）。多 ARML 场景请使用 [`generate_project`]。
///
/// 根元素必须为 `Window`/`Application` 且含 `Class`/`x:Class` 属性——否则返回
/// `Err` 描述错误。早期 demo 的"无 Class 属性退化为顶层 Main 函数"模式已
/// 随 Window.Text 字段废弃而删除（M3 元素树渲染完备后不再需要纯文本 fallback）。
pub fn generate(doc: &ArmlDocument, opts: &CodegenOptions) -> Result<String, String> {
    let (_class_name, body) = generate_partial_class_body(doc)?;
    let mut out = String::new();
    out.push_str("// <auto-generated>\n");
    out.push_str("// 由 `arc ui codegen` 从 .arml 文档生成。RFC 026 M2 ARML code-behind。\n");
    out.push_str("// </auto-generated>\n\n");
    out.push_str(&format!("namespace {};\n\n", opts.namespace));
    out.push_str("using Arc;\n\n");
    out.push_str(&body);
    Ok(out)
}

fn framework_inlines_internal_sources(sources: &[PathBuf]) -> bool {
    sources.iter().any(|p| {
        p.components().any(|c| c.as_os_str() == "Internal")
            && p.components().any(|c| c.as_os_str() == "UI")
    })
}

/// 从用户 partial class 文件内容中去除 `namespace X;` 与 `using X;` 行，
/// 保留 partial class 体本身。这允许将用户 partial class 与生成 partial class
/// 合并为单一 Arc 文件（Arc file-scoped namespace 限制：必须为文件唯一顶层声明）。
///
/// 保留所有 `partial class { ... }` 块、注释、空行。移除 file-scoped namespace/using，
/// 并展开 block `namespace X { ... }` 包装（如 `std/Arc/Tasks/Task.as`），使合并进
/// Program.as 后类型与同批 framework 源处于同一命名空间。
pub(crate) fn strip_namespace_and_using(src: &str) -> String {
    let mut out = Vec::new();
    let mut block_ns_depth: i32 = 0;
    for line in src.lines() {
        let trimmed = line.trim_start();
        if block_ns_depth == 0 {
            if trimmed.starts_with("namespace ") && trimmed.ends_with(';') {
                continue;
            }
            if trimmed.starts_with("using ") && trimmed.ends_with(';') {
                continue;
            }
            if trimmed.starts_with("namespace ") && trimmed.ends_with('{') {
                block_ns_depth = brace_delta(line);
                if block_ns_depth <= 0 {
                    block_ns_depth = 1;
                }
                continue;
            }
            out.push(line);
            continue;
        }
        block_ns_depth += brace_delta(line);
        if block_ns_depth <= 0 {
            block_ns_depth = 0;
            continue;
        }
        out.push(line);
    }
    out.join("\n")
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for ch in line.chars() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// 发射 `this.Title = "..."; this.Width = ...; this.Height = ...;`
/// 属性赋值代码（WPF-aligned InitializeComponent 模式）。
///
/// 这些赋值通过 Signal<T> 后端的属性 set 触发订阅者通知——Window 基类的
/// Title/Width/Height 是响应式属性，赋值会自动通知渲染层局部刷新。
/// 后续 `Window.Show()` 读取这些属性调用
/// `WindowHost.RunWithRoot(this.Title, w, h, _platformRoot)`。
fn emit_window_property_assignments(
    out: &mut String,
    title: &str,
    width: u32,
    height: u32,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    out.push_str(&format!(
        "{}this.Title = \"{}\";\n",
        pad,
        escape_arc_string(title)
    ));
    out.push_str(&format!("{}this.Width = {};\n", pad, width));
    out.push_str(&format!("{}this.Height = {};\n", pad, height));
}

/// 从根元素提取 `(title, width, height)`。
///
/// - `title` 来自根元素 `Title` 属性，缺省 `"Arc"`。
/// - `width`/`height` 来自 `Width`/`Height` 属性，缺省 `640`/`480`。
///
/// 不再提取 `Text` 属性——Window.Text 字段已废弃（RFC 026 D3.2 依赖属性
/// 重构），内容由 Content 属性或子元素树承载。
fn extract_root_window(root: &Element) -> (String, u32, u32) {
    let title = root
        .attr("Title")
        .and_then(|a| a.value.as_literal())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Arc".into());
    let width = root
        .attr("Width")
        .and_then(|a| a.value.as_literal())
        .and_then(|s| s.parse().ok())
        .unwrap_or(640);
    let height = root
        .attr("Height")
        .and_then(|a| a.value.as_literal())
        .and_then(|s| s.parse().ok())
        .unwrap_or(480);
    (title, width, height)
}

/// 转义 Arc 字符串字面量中的特殊字符（`\`、`"`、换行等）。
fn escape_arc_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    // ===== 单文档 generate() 测试 =====

    #[test]
    fn codegen_xbind_text_oneway_uses_bindtext_carrier() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock Text="{x:Bind Title, Mode=OneWay}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("long child_0_p = 0;"));
        assert!(code.contains("BindingOperations.SyncText(child_0, child_0_p, this.ObserveProperty(\"Title\").Value.ToString());"));
        // 订阅经运行时载体 BindText（初始 SyncText + 订阅 + G2 退订，一次调用）；
        // 不产生捕获类引用的逃逸闭包手动 Subscribe 形态。
        assert!(code.contains(
            "int xbind_0 = BindingOperations.BindText(child_0, child_0_p, this.ObserveProperty(\"Title\"));"
        ));
        assert!(!code.contains(".Subscribe(v =>"));
        assert!(!code.contains("RegisterDetach(() =>"));
    }

    #[test]
    fn codegen_xbind_text_onetime_skips_subscribe() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock Text="{x:Bind Title, Mode=OneTime}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("long child_0_p = 0;"));
        assert!(code.contains("BindingOperations.SyncText(child_0, child_0_p, this.ObserveProperty(\"Title\").Value.ToString());"));
        assert!(!code.contains(".Subscribe("));
        assert!(!code.contains("RegisterDetach"));
    }

    #[test]
    fn codegen_xbind_text_twoway_no_input_channel_equals_oneway() {
        // TextBlock 无输入通道：TwoWay 与 OneWay 等价（仅源→目标；运行时载体 BindText）。
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock Text="{x:Bind Title, Mode=TwoWay}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains(
            "int xbind_0 = BindingOperations.BindText(child_0, child_0_p, this.ObserveProperty(\"Title\"));"
        ));
        // TextBlock 无 OnTextChanged 写回表面
        assert!(!code.contains("OnTextChanged"));
    }

    #[test]
    fn codegen_xbind_textbox_oneway_uses_bindtextboxtext_carrier() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBox Text="{x:Bind Name, Mode=OneWay}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        // VM→UI 半边经 BindTextBoxText 载体（初始值 + 订阅 + G2 退订；经 Text
        // setter 路由编辑内核 TextBoxModel，不绕过内核裸 SetValue）。
        assert!(code.contains(
            "int xbind_0 = BindingOperations.BindTextBoxText(child_0, this.ObserveProperty(\"Name\"));"
        ));
        // TextBox 经载体路径同步平台镜像，不声明 _p 占位
        assert!(!code.contains("long child_0_p = 0;"));
        assert!(!code.contains("SetBinding"));
    }

    #[test]
    fn codegen_xbind_textbox_onetime_initial_write_only() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBox Text="{x:Bind Name, Mode=OneTime}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        // OneTime：仅初值直写（TextBox.Text setter 自身同步平台镜像；无订阅）
        assert!(code.contains("child_0.Text = this.Name;"));
        assert!(!code.contains("BindTextBoxText"));
        assert!(!code.contains("SetBinding"));
    }

    #[test]
    fn codegen_xbind_textbox_twoway_carrier_plus_inline_writeback() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBox Text="{x:Bind Name, Mode=TwoWay}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        // TwoWay = OneWay VM→UI 载体（BindTextBoxText）+ 内联 OnTextChanged 写回
        // VM setter（RFC 037 §5.3 场景 3：codegen 写回 VM setter → 合成通知闭环；
        // 相等性守卫防回环 + 内核 SetText 同值早退双重收敛）。
        assert!(code.contains(
            "int xbind_0 = BindingOperations.BindTextBoxText(child_0, this.ObserveProperty(\"Name\"));"
        ));
        assert!(code.contains("child_0.OnTextChanged((x: string) => {"));
        assert!(code.contains("if (this.Name != x) {"));
        assert!(code.contains("this.Name = x;"));
        // 不得用 SetBinding/SetTwoWay 裸载体（绕过编辑内核）
        assert!(!code.contains("SetBinding"));
        assert!(!code.contains("SetTwoWay"));
    }

    #[test]
    fn codegen_xbind_binding_runtime_rejected() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock Text="{Binding Title}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate(&doc, &CodegenOptions::default()).unwrap_err();
        assert!(err.contains("Binding"));
        assert!(err.contains("x:Bind"));
    }

    #[test]
    fn codegen_xbind_non_text_target_rejected() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Button Content="{x:Bind Title}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate(&doc, &CodegenOptions::default()).unwrap_err();
        assert!(err.contains("not supported"));
        assert!(err.contains("TextBlock Text"));
    }

    #[test]
    fn codegen_xname_generates_private_field() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <StackPanel>
        <TextBlock x:Name="Greeting" Text="hi"/>
        <Button x:Name="GoButton" Content="Go"/>
    </StackPanel>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        // InitializeComponent 内字段赋值（child_0=StackPanel、child_1=TextBlock、child_2=Button）
        assert!(code.contains("this.Greeting = child_1;"));
        assert!(code.contains("this.GoButton = child_2;"));
        // 类级私有字段声明（WPF MainWindow.g.cs 同构）
        assert!(code.contains("private TextBlock Greeting;"));
        assert!(code.contains("private Button GoButton;"));
    }

    #[test]
    fn codegen_xname_duplicate_errors() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <TextBlock x:Name="Dup" Text="a"/>
    <TextBlock x:Name="Dup" Text="b"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate(&doc, &CodegenOptions::default()).unwrap_err();
        assert!(err.contains("duplicate x:Name"));
        assert!(err.contains("Dup"));
    }

    #[test]
    fn codegen_button_click_wires_onclick() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Button Content="Go" Click="OnGo"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("child_0.OnClick(_ => this.OnGo());"));
    }

    #[test]
    fn codegen_class_mode_generates_partial_class() {
        let src = r#"<?xml version="1.0"?>
<Window Title="Hello" Width="320" Height="200" Class="ArmlHello.MainWindow">
    <TextBlock Text="Hello, ARML!"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "ArmlHello".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        // WPF-aligned: partial class MainWindow : Window + override InitializeComponent
        assert!(code.contains("public partial class MainWindow : Window {"));
        assert!(code.contains("public override void InitializeComponent() {"));
        // 属性赋值（this.Title/Width/Height）——Signal<T> 后端的属性 set
        assert!(code.contains("this.Title = \"Hello\";"));
        assert!(code.contains("this.Width = 320;"));
        assert!(code.contains("this.Height = 200;"));
        // RFC 037 D3.2：Window.Text 字段已废弃，codegen 不再生成 this.Text 赋值
        assert!(!code.contains("this.Text ="));
        // M3：子元素作为 Children 节点由 codegen 实例化并通过 AddChild 挂载到逻辑树
        assert!(code.contains("var child_0 = new TextBlock();"));
        assert!(code.contains("child_0.Text = \"Hello, ARML!\";"));
        assert!(code.contains("this.AddChild(child_0);"));
        // M3.6：平台镜像不在 codegen 双写——由 Show() → PlatformTreeSync 同步
        assert!(!code.contains("WindowHost.ElementCreate"));
        assert!(!code.contains("WindowHost.ElementSetString"));
        assert!(!code.contains("WindowHost.ElementAddChild"));
        assert!(!code.contains("_platformRoot"));
        // M3 样式系统：TypeName 赋值为 StyleManager 隐式匹配
        assert!(code.contains("this.TypeName = \"Window\";"));
        assert!(code.contains("child_0.TypeName = \"TextBlock\";"));
        // 不应生成顶层 Main 函数
        assert!(!code.contains("public void Main()"));
    }

    #[test]
    fn codegen_button_click_binds_code_behind() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Button Content="Go" Click="OnGo"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("child_0.OnClick(_ => this.OnGo());"));
        assert!(!code.contains("child_0.Click ="));
    }

    #[test]
    fn codegen_type_name_deep_nesting() {
        // 验证 TypeName 在 3 层嵌套中正确传播。
        // Window → StackPanel → Button
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <StackPanel>
        <Button Content="OK"/>
    </StackPanel>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");

        // 层级0：Window 根
        assert!(code.contains("this.TypeName = \"Window\";"));

        // 层级1：StackPanel
        assert!(code.contains("child_0.TypeName = \"StackPanel\";"));

        // 层级2：Button
        assert!(code.contains("child_1.TypeName = \"Button\";"));

        // 变量计数器正确递增——每层独立变量名
        assert!(code.contains("var child_0 = new StackPanel();"));
        assert!(code.contains("var child_1 = new Button();"));

        // AddChild 正确路由
        assert!(code.contains("child_0.AddChild(child_1);")); // StackPanel → Button
        assert!(code.contains("this.AddChild(child_0);")); // Window → StackPanel
    }

    #[test]
    fn codegen_m3_no_platform_mirror_in_initialize_component() {
        // M3.6：属性类型由 Arc DP 承载；平台镜像改由 PlatformTreeSync 在 Show 同步。
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Button Content="Click" IsEnabled="true" FontSize="14"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("child_0.Content = \"Click\";"));
        assert!(!code.contains("child_0.MirrorContent ="));
        assert!(code.contains("child_0.IsEnabled = true;"));
        assert!(code.contains("child_0.FontSize = 14;"));
        assert!(!code.contains("WindowHost.ElementSet"));
        assert!(!code.contains("_p = WindowHost"));
        // `_p` 仅在有平台回写需求（x:Bind）时声明，普通元素不生成
        assert!(!code.contains("child_0_p"));
    }

    #[test]
    fn codegen_m35_image_source_goes_via_platform_tree_sync() {
        // Image.Source 不再 codegen 直写镜像 handle（_p=0 时为 no-op 死代码）；
        // 改由 Window.Show() → PlatformTreeSync.Image 分支统一同步（RFC 037 M3.5/M3.6）。
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Image Source="logo.png" Width="128" Height="128" Stretch="Uniform"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("var child_0 = new Image();"));
        // Source/Stretch 经 Arc DP 赋值承载；不生成死 ElementSetString。
        assert!(code.contains("child_0.Source = \"logo.png\";"));
        assert!(code.contains("child_0.Stretch = Stretch.Uniform;"));
        assert!(!code.contains("child_0_p"));
        assert!(!code.contains("WindowHost.ElementSetString"));
    }

    #[test]
    fn codegen_x_class_prefix_form() {
        // x:Class="Ns.Name" XAML 标准前缀形式
        let src = r#"<?xml version="1.0"?>
<Window Title="Hi" Width="10" Height="10" xmlns:x="http://x" x:Class="ArmlHello.MainWindow">
    <TextBlock Text="yo"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "ArmlHello".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("public partial class MainWindow : Window {"));
        assert!(code.contains("public override void InitializeComponent() {"));
    }

    #[test]
    fn codegen_no_class_attribute_errors() {
        // RFC 026 D3.2：Window.Text 字段废弃后，旧 demo 的"无 Class 属性退化为
        // 顶层 Main 函数"模式已删除。无 Class 属性现在应返回错误，而非生成
        // 调用 Window.RunWithText 的旧 demo 代码。
        let src = r#"<Window Title="X" Width="1" Height="1">
    <TextBlock Text="hi"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate(&doc, &CodegenOptions::default()).unwrap_err();
        assert!(err.contains("missing"));
        assert!(err.contains("Class"));
    }

    #[test]
    fn codegen_defaults_when_window_props_missing() {
        // RFC 037 D3.2：extract_root_window 不再返回 text——Window.Text 已废弃。
        // 缺省值：title="Arc"，width=640，height=480。
        let src = "<Window><TextBlock Text=\"hi\"/></Window>";
        let doc = Parser::parse(src).expect("parse");
        let (title, width, height) = extract_root_window(&doc.root);
        assert_eq!(title, "Arc");
        assert_eq!(width, 640);
        assert_eq!(height, 480);
    }

    #[test]
    fn codegen_escapes_special_chars_in_title() {
        // RFC 037 D3.2：Window.Text 已废弃，转义逻辑现由 this.Title 承载。
        // XML 不解析反斜杠转义，输入 `back\slash`（单反斜杠）经 Arc 转义为 `back\\slash`。
        let src = r#"<Window Title="back\slash" Width="1" Height="1" Class="Ns.W">
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(&doc, &CodegenOptions::default()).expect("generate");
        // 单反斜杠转义为 `\\`（赋值到 this.Title）
        assert!(code.contains(r#"this.Title = "back\\slash";"#));
    }

    // ===== strip_namespace_and_using 辅助函数测试 =====

    #[test]
    fn codegen_strip_namespace_block_wrapper() {
        let input = "namespace Arc {\npublic class Task<T> {\n    public static Task Delay(int ms) { return null; }\n}\n}\n";
        let stripped = strip_namespace_and_using(input);
        assert!(!stripped.contains("namespace Arc"));
        assert!(stripped.contains("public class Task<T>"));
        assert!(stripped.contains("Delay"));
    }

    #[test]
    fn codegen_strip_namespace_and_using_helper() {
        let input = "namespace Foo;\nusing Bar;\nusing Baz;\n\npublic partial class C {\n    void M() {}\n}\n";
        let stripped = strip_namespace_and_using(input);
        assert!(!stripped.contains("namespace Foo;"));
        assert!(!stripped.contains("using Bar;"));
        assert!(!stripped.contains("using Baz;"));
        assert!(stripped.contains("public partial class C"));
        assert!(stripped.contains("void M()"));
    }

    // ===== Application 根元素测试 =====

    #[test]
    fn codegen_application_partial_generates_app_class() {
        let src = r#"<?xml version="1.0"?>
<Application xmlns="http://schemas.arc.dev/winfx/2026"
             xmlns:x="http://schemas.arc.dev/xaml"
             x:Class="ArmlHello.App"
             StartupUri="MainWindow.arml"/>"#;
        let doc = Parser::parse(src).expect("parse");
        let (class_name, body) = generate_partial_class_body(&doc).expect("body");
        assert_eq!(class_name, "App");
        // WPF-aligned: partial class App : Application + override InitializeComponent
        assert!(body.contains("public partial class App : Application {"));
        assert!(body.contains("public override void InitializeComponent() {"));
        // 设置 this.MainWindow = new MainWindow() + this.MainWindow.InitializeComponent()
        assert!(body.contains("this.MainWindow = new MainWindow();"));
        assert!(body.contains("this.MainWindow.InitializeComponent();"));
    }

    #[test]
    fn codegen_application_partial_default_startup_when_missing() {
        // 无 StartupUri → 默认 MainWindow
        let src = r#"<Application x:Class="Ns.App"/>"#;
        let doc = Parser::parse(src).expect("parse");
        let (_, body) = generate_partial_class_body(&doc).expect("body");
        assert!(body.contains("this.MainWindow = new MainWindow();"));
    }

    #[test]
    fn codegen_application_resources_style_emits_addstyle() {
        // Part A：声明式 `<Application.Resources><Style>` → 运行期 AddStyle。
        // 无 x:Key → 隐式样式（Key = ""）；Setter Value 按字面量类型选 SetterValue 工厂。
        let src = r##"<?xml version="1.0"?>
<Application xmlns="http://schemas.arc.dev/winfx/2026"
             xmlns:x="http://schemas.arc.dev/xaml"
             x:Class="ArmlDemo.App"
             StartupUri="MainWindow.arml">
    <Application.Resources>
        <Style TargetType="Button">
            <Setter Property="Background" Value="#FFCC2222"/>
        </Style>
    </Application.Resources>
</Application>"##;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "ArmlDemo".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");
        assert!(code.contains("this.Resources.AddStyle(_style_0);"));
        assert!(code.contains("_style_0.TargetType = \"Button\";"));
        assert!(code.contains("_style_0.Key = \"\";"));
        assert!(code.contains("_setter_0_0.Property = \"Background\";"));
        assert!(code.contains("_setter_0_0.Value = SetterValue.String(\"#FFCC2222\");"));
        assert!(code.contains("_style_0.Setters.Add(_setter_0_0);"));
    }

    #[test]
    fn codegen_application_themes_emits_register_aggregated() {
        // 声明式 `<Application.Themes>`：覆盖内置键 + 聚合多个 ResourceDictionary +
        // BasedOn 继承。编译器在**编译期**扁平化继承链 → 平坦 ResourceDictionary +
        // 纯存储 RegisterTheme（无运行期 Merged）。
        let src = r##"<?xml version="1.0"?>
<Application xmlns="http://schemas.arc.dev/winfx/2026"
             xmlns:x="http://schemas.arc.dev/xaml"
             x:Class="ArmlDemo.App"
             StartupUri="MainWindow.arml">
    <Application.Themes>
        <Theme x:Key="Light">
            <Color  x:Key="Color.Primary"  Value="#FF1677FF"/>
            <Double x:Key="Radius.Control" Value="8"/>
        </Theme>
        <Theme x:Key="HighContrast" BasedOn="Light">
            <ResourceDictionary>
                <Color x:Key="Color.Background" Value="#FF000000"/>
            </ResourceDictionary>
            <ResourceDictionary>
                <Color x:Key="Color.Text.Primary" Value="#FFFFFFFF"/>
            </ResourceDictionary>
        </Theme>
    </Application.Themes>
</Application>"##;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "ArmlDemo".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate");

        // Light：覆盖内置 → 基底 BuiltInTheme.CreateLight() + 类型化 IBrush 工厂 + 纯存储注册
        assert!(code.contains("ResourceDictionary _theme_0 = BuiltInTheme.CreateLight();"));
        assert!(code.contains(
            "_theme_0.Add(\"Color.Primary\", ResourceValue.Brush(Brushes.Parse(\"#FF1677FF\")));"
        ));
        assert!(code.contains("_theme_0.Add(\"Radius.Control\", ResourceValue.Number(8.0));"));
        assert!(code.contains("this.ThemeDictionaries.RegisterTheme(\"Light\", _theme_0);"));

        // HighContrast BasedOn Light：编译器继承 Light 的覆盖（Color.Primary）+ 自身聚合
        assert!(code.contains("ResourceDictionary _theme_1 = BuiltInTheme.CreateLight();"));
        assert!(code.contains(
            "_theme_1.Add(\"Color.Primary\", ResourceValue.Brush(Brushes.Parse(\"#FF1677FF\")));"
        ));
        assert!(code.contains(
            "_theme_1.Add(\"Color.Background\", ResourceValue.Brush(Brushes.Parse(\"#FF000000\")));"
        ));
        assert!(code.contains(
            "_theme_1.Add(\"Color.Text.Primary\", ResourceValue.Brush(Brushes.Parse(\"#FFFFFFFF\")));"
        ));
        assert!(code.contains("this.ThemeDictionaries.RegisterTheme(\"HighContrast\", _theme_1);"));
    }

    #[test]
    fn codegen_application_style_with_key_emits_named_key() {
        // 有 x:Key → 具名样式（Key = "Foo"），不再生成 Key = ""。
        let src = r##"<Application x:Class="Ns.App" StartupUri="MainWindow.arml">
    <Application.Resources>
        <Style TargetType="Button" x:Key="Foo">
            <Setter Property="Background" Value="#FFCC2222"/>
        </Style>
    </Application.Resources>
</Application>"##;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(&doc, &CodegenOptions::default()).expect("generate");
        assert!(code.contains("_style_0.Key = \"Foo\";"));
        assert!(!code.contains("_style_0.Key = \"\";"));
        assert!(code.contains("this.Resources.AddStyle(_style_0);"));
    }

    #[test]
    fn codegen_application_style_setter_typed_factories() {
        // SetterValue 真实工厂：数字 → Number(double)，bool → Boolean(bool)，
        // 字符串（颜色/字体）→ String。
        let src = r#"<Application x:Class="Ns.App" StartupUri="MainWindow.arml">
    <Application.Resources>
        <Style TargetType="Button" x:Key="Big">
            <Setter Property="FontSize" Value="14"/>
            <Setter Property="IsEnabled" Value="true"/>
            <Setter Property="Foreground" Value="White"/>
        </Style>
    </Application.Resources>
</Application>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(&doc, &CodegenOptions::default()).expect("generate");
        assert!(code.contains("_setter_0_0.Value = SetterValue.Number(14.0);"));
        assert!(code.contains("_setter_0_1.Value = SetterValue.Boolean(true);"));
        assert!(code.contains("_setter_0_2.Value = SetterValue.String(\"White\");"));
        assert!(code.contains("_style_0.Key = \"Big\";"));
    }

    #[test]
    fn codegen_unsupported_root_element_errors() {
        let src = r#"<Button x:Class="Ns.Btn"/>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate_partial_class_body(&doc).unwrap_err();
        assert!(err.contains("unsupported root element"));
        assert!(err.contains("Button"));
    }

    #[test]
    fn codegen_missing_class_attribute_errors() {
        let src = r#"<Window Title="x" Width="1" Height="1"/>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate_partial_class_body(&doc).unwrap_err();
        assert!(err.contains("missing"));
        assert!(err.contains("Class"));
    }

    // ===== generate_project 多 ARML + 用户源合并测试 =====

    #[test]
    fn codegen_generate_project_multi_arml_and_user_sources() {
        let tmpdir = std::env::temp_dir().join("arc_ui_codegen_project_test");
        std::fs::create_dir_all(&tmpdir).unwrap();

        // App.arml
        let app_arml = tmpdir.join("App.arml");
        std::fs::write(
            &app_arml,
            r#"<?xml version="1.0"?>
<Application x:Class="ArmlHello.App" StartupUri="MainWindow.arml"/>"#,
        )
        .unwrap();

        // MainWindow.arml
        let mw_arml = tmpdir.join("MainWindow.arml");
        std::fs::write(
            &mw_arml,
            r#"<?xml version="1.0"?>
<Window Title="Hi" Width="100" Height="100" x:Class="ArmlHello.MainWindow">
    <TextBlock Text="yo"/>
</Window>"#,
        )
        .unwrap();

        // App.arml.as —— 用户 partial class : Application（对标 WPF App.xaml.cs）
        // 不含 Run()——继承自 Application 基类；可 override OnStartup/OnExit
        let app_as = tmpdir.join("App.arml.as");
        std::fs::write(
            &app_as,
            r#"namespace ArmlHello;
using Arc;

public partial class App : Application {
    // 用户可选 override OnStartup/OnExit 等生命周期钩子
}
"#,
        )
        .unwrap();

        // MainWindow.arml.as —— 用户 partial class : Window
        let mw_as = tmpdir.join("MainWindow.arml.as");
        std::fs::write(
            &mw_as,
            r#"namespace ArmlHello;
using Arc;

public partial class MainWindow : Window {
    // 用户可选 override OnLoaded/OnClosed 等生命周期钩子
}
"#,
        )
        .unwrap();

        // Program.as —— 统一入口文件（所有 Arc 项目标准）
        let program_as = tmpdir.join("Program.as");
        std::fs::write(
            &program_as,
            r#"namespace ArmlHello;
using Arc;

public void Main() {
    var app = new App();
    app.Run();
}
"#,
        )
        .unwrap();

        // obj_dir
        let obj_dir = tmpdir.join("obj");

        let opts = CodegenOptions {
            namespace: "ArmlHello".into(),
            user_sources: vec![app_as.clone(), mw_as.clone()],
            program: Some(program_as.clone()),
            obj_dir: Some(obj_dir.clone()),
            project_root: Some(tmpdir.clone()),
            config: "Debug".into(),
            framework_sources: Vec::new(),
        };

        let result =
            generate_project(&[app_arml.clone(), mw_arml.clone()], &opts).expect("project");

        // 独立 .g.as 文件应生成到 obj/<config>/code/<stem>.g.as（.NET 体系）
        assert_eq!(result.generated_files.len(), 2);
        let app_g = &result.generated_files[0];
        let mw_g = &result.generated_files[1];
        assert_eq!(app_g.class_name, "App");
        assert_eq!(mw_g.class_name, "MainWindow");
        assert!(
            app_g.path.ends_with("obj\\Debug\\code\\App.g.as")
                || app_g.path.ends_with("obj/Debug/code/App.g.as"),
            "expected obj/Debug/code/App.g.as, got {}",
            app_g.path.display()
        );
        assert!(
            mw_g.path.ends_with("obj\\Debug\\code\\MainWindow.g.as")
                || mw_g.path.ends_with("obj/Debug/code/MainWindow.g.as"),
            "expected obj/Debug/code/MainWindow.g.as, got {}",
            mw_g.path.display()
        );

        // 文件确实存在
        assert!(app_g.path.exists());
        assert!(mw_g.path.exists());

        // Program.as 内容验证
        let program = &result.program;
        // namespace/using 仅声明一次
        assert_eq!(program.matches("namespace ArmlHello;").count(), 1);
        assert_eq!(program.matches("using Arc;").count(), 1);

        // WPF-aligned: App/MainWindow 都继承自基类（.g.as + 用户 .arml.as）
        assert_eq!(
            program
                .matches("public partial class App : Application {")
                .count(),
            2
        );
        assert_eq!(
            program
                .matches("public partial class MainWindow : Window {")
                .count(),
            2
        );

        // App.g.as override InitializeComponent 设置 MainWindow
        assert!(program.contains("public override void InitializeComponent() {"));
        assert!(program.contains("this.MainWindow = new MainWindow();"));
        assert!(program.contains("this.MainWindow.InitializeComponent();"));

        // MainWindow.g.as override InitializeComponent 设置 Window 属性
        assert!(program.contains("this.Title = \"Hi\";"));
        assert!(program.contains("this.Width = 100;"));
        assert!(program.contains("this.Height = 100;"));
        // M3：`<TextBlock Text="yo"/>` 子元素作为 Children 节点，不再回填 Window.Text
        assert!(program.contains("var child_0 = new TextBlock();"));
        assert!(program.contains("child_0.Text = \"yo\";"));
        assert!(program.contains("this.AddChild(child_0);"));
        // M3.6：平台镜像不在 codegen 双写
        assert!(!program.contains("WindowHost.ElementCreate"));
        assert!(!program.contains("this._platformRoot"));

        // Program.as 的 Main 入口应保留（合并到末尾）
        assert!(program.contains("public void Main() {"));
        assert!(program.contains("var app = new App();"));
        assert!(program.contains("app.Run();"));

        // 清理
        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn codegen_generate_project_without_obj_dir_skips_g_as_files() {
        // 不设置 obj_dir → 仅生成 Program.as，不写 .g.as 文件
        let tmpdir = std::env::temp_dir().join("arc_ui_codegen_project_no_obj");
        std::fs::create_dir_all(&tmpdir).unwrap();

        let arml = tmpdir.join("MainWindow.arml");
        std::fs::write(
            &arml,
            r#"<Window Title="Hi" Width="100" Height="100" x:Class="ArmlHello.MainWindow">
    <TextBlock Text="yo"/>
</Window>"#,
        )
        .unwrap();

        let opts = CodegenOptions {
            namespace: "ArmlHello".into(),
            user_sources: vec![],
            program: None,
            obj_dir: None,
            project_root: None,
            config: "Debug".into(),
            framework_sources: Vec::new(),
        };

        let result = generate_project(&[arml], &opts).expect("project");
        assert!(result.generated_files.is_empty());
        assert!(result
            .program
            .contains("public partial class MainWindow : Window {"));

        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    #[test]
    fn codegen_inline_internal_omits_arc_ui_internal_using() {
        let tmpdir = std::env::temp_dir().join("arc_ui_codegen_internal_using_test");
        std::fs::create_dir_all(&tmpdir).unwrap();
        let arml = tmpdir.join("MainWindow.arml");
        std::fs::write(
            &arml,
            r#"<Window Title="Hi" Width="100" Height="100" x:Class="ArmlHello.MainWindow">
    <TextBlock Text="yo"/>
</Window>"#,
        )
        .unwrap();
        let fw_window = tmpdir.join("UI/Components/Window.as");
        std::fs::create_dir_all(fw_window.parent().unwrap()).unwrap();
        std::fs::write(
            &fw_window,
            "namespace Arc.UI.Components;\nusing Arc.UI.Internal;\npublic class Window {}\n",
        )
        .unwrap();
        let fw_internal = tmpdir.join("UI/Internal/FocusManager.as");
        std::fs::create_dir_all(fw_internal.parent().unwrap()).unwrap();
        std::fs::write(
            &fw_internal,
            "namespace Arc.UI.Internal;\ninternal class FocusManager {}\n",
        )
        .unwrap();
        let opts = CodegenOptions {
            namespace: "ArmlHello".into(),
            user_sources: vec![],
            program: None,
            obj_dir: None,
            project_root: None,
            config: "Debug".into(),
            framework_sources: vec![fw_window, fw_internal],
        };
        let result = generate_project(&[arml], &opts).expect("project");
        assert!(!result.program.contains("using Arc.UI.Internal;"));
        assert!(result.program.contains("internal class FocusManager"));
        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    // ===== M-U2 自适应投影：`{Token}` 编译期展开（RFC 016 §11.5）=====

    #[test]
    fn codegen_token_reference_expands_to_projection_binding() {
        // M-U0/M-U1 时代 `{Token}` 在 codegen 侧「拒绝求值」；M-U2 展开为
        // 静态投影表数据 + 宿主绑定注册。
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <Window.Resources>
        <Double x:Key="Spacing.Page">
            <Match Tier="sm" Value="8" />
            <Match Tier="md" Value="16" />
            <Match Tier="lg" Value="24" />
            <Match Value="16" />
        </Double>
    </Window.Resources>
    <StackPanel Padding="{Token Spacing.Page}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let code = generate(
            &doc,
            &CodegenOptions {
                namespace: "Ns".into(),
                ..CodegenOptions::default()
            },
        )
        .expect("generate with token reference");
        // 投影规格数据（状态空间：Tier 维度，card 4 = 引用 3 + no-match 槽）
        assert!(code.contains("AdaptiveSpec _adaptiveSpec = new AdaptiveSpec();"));
        assert!(code.contains("_adaptiveSpec.NumStates = 4;"));
        assert!(code.contains("_adaptiveSpec.TierCount = 3;"));
        assert!(code.contains("_adaptiveSpec.TierRef"));
        // 求值器/宿主 + 绑定注册（编译期展开为静态投影表数据）
        assert!(code.contains("this._adaptiveHost = new AdaptiveHost(_adaptiveSpec);"));
        assert!(code.contains("this._adaptiveHost.BindToken(\"Padding\", 0, 0);"));
        assert!(code.contains("private AdaptiveHost _adaptiveHost;"));
        // 投影表值：sm→8, md→16, lg→24, no-match→16
        assert!(code.contains("8.0, 16.0, 24.0, 16.0"));
    }

    #[test]
    fn codegen_undefined_token_reference_errors() {
        let src = r#"<Window Title="T" Width="100" Height="100" Class="Ns.W">
    <StackPanel Padding="{Token NoSuch}"/>
</Window>"#;
        let doc = Parser::parse(src).expect("parse");
        let err = generate(&doc, &CodegenOptions::default()).unwrap_err();
        assert!(err.contains("NoSuch"));
        assert!(err.contains("undefined Token"));
    }
}
