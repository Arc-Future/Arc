//! RFC 016 §11 多平台 + 多分辨率自适应：文法形式化（M-U0）与编译期检查（M-U1）。
//!
//! M-U0 —— 设计签收：§11 类型化值元素 / `<Match>` / `<Adaptive>` / `Tiers` /
//! `<Application.Media>` 文法形式化落地，doc-test 全部解析成功（见下方示例）。
//!
//! M-U1 —— 语法 + 编译期检查：值类型元素、`Match`、`Tiers`、单位校验；
//! §11.3 书写规则表全部 error/warning 判定；`.arml.as` 污染检查（P1 红线，
//! 双文件配对扫描）。
//!
//! **范围边界**：本模块只到「解析 + 编译期检查」。运行期求值/投影表/容器查询
//! （§11.5）属 M-U2；`{Token}` 引用仅做 AST 记号收集（M-U2 才求值）；
//! **引用未定义 Token = error**（verify 与 codegen 双层一致，§11.3 书写规则表）。
//!
//! # M-U0 文法 doc-tests（§11 逐节）
//!
//! ## §11.2 常量值（无谓词 Token）
//!
//! ```rust
//! use arc_ui::Parser;
//!
//! let src = r##"<Application x:Class="Ns.App">
//!     <Application.Resources>
//!         <Double x:Key="Spacing.Gap" Value="4" />
//!         <Color  x:Key="Color.Surface" Value="#FAFAFA" />
//!         <TrackList x:Key="Grid.MasterDetail" Value="1,2" />
//!         <Thickness x:Key="Border.Pad" Value="4" />
//!         <String x:Key="Greeting" Value="Hello" />
//!     </Application.Resources>
//! </Application>"##;
//! let doc = Parser::parse(src).expect("parse constants");
//! assert_eq!(doc.root.name.as_str(), "Application");
//! let resources = doc.root.child_elements().next().expect("Resources");
//! assert_eq!(resources.name.as_str(), "Resources");
//! assert_eq!(resources.child_elements().count(), 5);
//! let gap = resources
//!     .child_elements()
//!     .find(|e| {
//!         e.attr_with_prefix("x", "Key").and_then(|a| a.value.as_literal()) == Some("Spacing.Gap")
//!     })
//!     .expect("Spacing.Gap token");
//! assert_eq!(gap.name.as_str(), "Double");
//! ```
//!
//! ## §11.2 引用处（`{Token}` 标记扩展，编译期符号解析，非字符串键）
//!
//! ```rust
//! use arc_ui::Parser;
//!
//! let src = r##"<Window x:Class="Ns.W">
//!     <Window.Resources>
//!         <Thickness x:Key="Border.Pad" Value="8" />
//!         <String x:Key="Greeting" Value="Hello" />
//!     </Window.Resources>
//!     <TextBlock Margin="{Token Border.Pad}" Text="{Token Greeting}" />
//! </Window>"##;
//! let doc = Parser::parse(src).expect("parse token references");
//! assert_eq!(doc.root.name.as_str(), "Window");
//! let text = doc
//!     .root
//!     .child_elements()
//!     .find(|e| e.name.as_str() == "TextBlock")
//!     .expect("TextBlock element");
//! let margin = text
//!     .attr("Margin")
//!     .expect("Margin attribute")
//!     .value
//!     .as_markup()
//!     .expect("Margin is a `{Token}` markup extension");
//! assert_eq!(margin.kind.as_str(), "Token");
//! assert_eq!(margin.args.first().map(|s| s.as_str()), Some("Border.Pad"));
//! ```
//!
//! ## §11.3 条件子元素 `<Match>`（结构化 · 非字符串谓词）
//!
//! ```rust
//! use arc_ui::Parser;
//!
//! let src = r##"<Application x:Class="Ns.App">
//!     <Application.Resources>
//!         <Double x:Key="Spacing.Page">
//!             <Match Tier="sm" Value="8" />
//!             <Match Tier="md" Value="16" />
//!             <Match Tier="lg" Value="24" />
//!             <Match Value="16" />
//!         </Double>
//!         <Color x:Key="Color.Primary">
//!             <Match Media="dark" Value="#8FB8FF" />
//!             <Match Media="high-contrast" Value="#FFFFFF" />
//!             <Match Value="#2D6CDF" />
//!         </Color>
//!         <TrackList x:Key="Panel.MasterDetail">
//!             <Match Tier="sm" Value="1" />
//!             <Match Tier="md" Value="1,2" />
//!             <Match Tier="lg" Value="1,2,1" />
//!         </TrackList>
//!     </Application.Resources>
//! </Application>"##;
//! let doc = Parser::parse(src).expect("parse Match children");
//! assert_eq!(doc.root.name.as_str(), "Application");
//! let resources = doc.root.child_elements().next().expect("Resources element");
//! let spacing = resources
//!     .child_elements()
//!     .find(|e| {
//!         e.attr_with_prefix("x", "Key").and_then(|a| a.value.as_literal()) == Some("Spacing.Page")
//!     })
//!     .expect("Spacing.Page token");
//! assert_eq!(spacing.name.as_str(), "Double");
//! let matches = spacing.child_elements().collect::<Vec<_>>();
//! assert_eq!(matches.len(), 4);
//! assert!(matches.iter().all(|m| m.name.as_str() == "Match"));
//! ```
//!
//! ## §11.3 档位声明（全局默认 / 局部覆盖）与参数化媒体坐标声明
//!
//! ```rust
//! use arc_ui::Parser;
//!
//! let src = r##"<Application x:Class="Ns.App">
//!     <Application.Tiers Default="sm:600 md:960 lg:1280" />
//!     <Application.Media>
//!         <Media Name="safe-area-inset-top" Type="Length(vp)" />
//!         <Media Name="font-scale" Type="Ratio" />
//!     </Application.Media>
//! </Application>"##;
//! let doc = Parser::parse(src).expect("parse Tiers/Media declarations");
//! assert_eq!(doc.root.name.as_str(), "Application");
//! let child_names: Vec<&str> = doc.root.child_elements().map(|e| e.name.as_str()).collect();
//! assert!(child_names.contains(&"Tiers"));
//! assert!(child_names.contains(&"Media"));
//!
//! let win = r##"<Window x:Class="Ns.W">
//!     <Window.Tiers Default="sm:520 md:880 lg:1280" />
//! </Window>"##;
//! let doc = Parser::parse(win).expect("parse Window.Tiers local override");
//! assert_eq!(doc.root.name.as_str(), "Window");
//! let win_names: Vec<&str> = doc.root.child_elements().map(|e| e.name.as_str()).collect();
//! assert!(win_names.contains(&"Tiers"));
//! ```
//!
//! ## §11.4 布局级自适应：属性元素内联轨道 + 条件子树 `<Adaptive>`
//!
//! ```rust
//! use arc_ui::Parser;
//!
//! let src = r##"<Window x:Class="Ns.W">
//!     <Grid Rows="Auto,*">
//!         <Grid.Columns>
//!             <TrackList Value="1">
//!                 <Match Tier="md" Value="1,2" />
//!                 <Match Tier="lg" Value="1,2,1" />
//!             </TrackList>
//!         </Grid.Columns>
//!         <StackPanel>
//!             <Adaptive MinWidth="600">
//!                 <Grid Columns="1,2"><TextBlock Text="List" /><TextBlock Text="Detail" /></Grid>
//!             </Adaptive>
//!             <Adaptive MaxWidth="599">
//!                 <TextBlock Text="Tap item to open detail" />
//!             </Adaptive>
//!         </StackPanel>
//!     </Grid>
//! </Window>"##;
//! let doc = Parser::parse(src).expect("parse Adaptive subtrees");
//! assert_eq!(doc.root.name.as_str(), "Window");
//! let grid = doc.root.child_elements().next().expect("Grid element");
//! assert_eq!(grid.name.as_str(), "Grid");
//! let stack = grid
//!     .child_elements()
//!     .find(|e| e.name.as_str() == "StackPanel")
//!     .expect("StackPanel element");
//! let adaptives = stack.child_elements().collect::<Vec<_>>();
//! assert_eq!(adaptives.len(), 2);
//! assert!(adaptives.iter().all(|a| a.name.as_str() == "Adaptive"));
//! ```

use crate::adaptive_lit::{check_value_literal, parse_breakpoint, split_length, ValueType};
use crate::ast::*;
use crate::error::ArmlError;
use smol_str::SmolStr;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 自适应编译期检查结果。
#[derive(Debug, Default)]
pub struct AdaptiveCheck {
    /// 编译错误（含 strict 模式下被升级的严格 warning）。
    pub errors: Vec<ArmlError>,
    /// 非严格 warning（`未使用 Token` 等）。
    pub warnings: Vec<ArmlError>,
}

/// 内置 Tier 档位：`sm`/`md`/`lg`（RFC 016 §11.3「全项目可用，无需声明」）。
const BUILTIN_TIERS: &[(&str, f64)] = &[("sm", 600.0), ("md", 960.0), ("lg", 1280.0)];

/// 内置 Media 坐标（RFC 016 §11.3：坐标集 = 内置 ∪ `<Application.Media>` 声明）。
const BUILTIN_MEDIA: &[&str] = &["dark", "light", "high-contrast", "reduced-motion"];

/// 媒体参数类型（`<Media Type>`，§11.3）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum MediaValueSpec {
    /// `Length(vp|px|%|lpx)`：长度
    Length(&'static str),
    /// `Ratio`：比例（纯数字）
    Ratio,
    /// `string`：字符串
    String,
    /// 其他（枚举名等）：枚举成员无法在切片内进一步校验
    Enum(String),
}

/// Tier 引用解析结果。
enum TierResolve {
    Known,
    /// 隐式依赖全局默认（子作用域未声明，触发档位阈值漂移 warning）
    Drift,
    Unknown,
}

/// 作用域：子作用域（`<Window.Tiers>` 局部覆盖）档位表。
#[derive(Default, Clone)]
struct Scope {
    local_tiers: Option<BTreeMap<String, f64>>,
}

impl Scope {
    fn global() -> Self {
        Self { local_tiers: None }
    }
}

/// 自适应检查上下文。
struct Ctx {
    strict: bool,
    errors: Vec<ArmlError>,
    warnings: Vec<ArmlError>,
    /// 全局档位（内置 sm/md/lg + `<Application.Tiers>` 声明）
    global_tiers: BTreeMap<String, f64>,
    /// `<Application.Media>` 参数化坐标声明
    app_media: BTreeMap<String, MediaValueSpec>,
    /// Token 定义（`x:Key`）与引用（`{Token}`/`{StaticResource}`）
    token_defs: BTreeMap<String, Span>,
    token_refs: BTreeSet<String>,
    /// `{Token}` 专用引用表（首次引用位置；引用未定义 Token 检查用，
    /// `{StaticResource}` 不入表——可引用 Style/模板等非类型化值资源键）
    token_refs_token: BTreeMap<String, Span>,
    /// `<Application.Tiers>` 全局档位声明次数（唯一于作用域）
    app_tiers_decl_count: usize,
}

impl Default for Ctx {
    fn default() -> Self {
        Self {
            strict: false,
            errors: Vec::new(),
            warnings: Vec::new(),
            global_tiers: BUILTIN_TIERS
                .iter()
                .map(|(n, v)| (n.to_string(), *v))
                .collect(),
            app_media: BTreeMap::new(),
            token_defs: BTreeMap::new(),
            token_refs: BTreeSet::new(),
            token_refs_token: BTreeMap::new(),
            app_tiers_decl_count: 0,
        }
    }
}

/// 执行自适应编译期检查（§11.2/§11.3/§11.4 书写规则表）。
///
/// `strict` 为真时，标记 `strict = error` 的 warning（区间未全覆盖 / 档位阈值漂移 /
/// 死分支 / 同权重歧义）升级为 error。
pub fn check_adaptive(doc: &ArmlDocument, strict: bool) -> AdaptiveCheck {
    let mut ctx = Ctx {
        strict,
        ..Ctx::default()
    };
    ctx.walk(&doc.root, None, &Scope::global());
    // 未使用 Token（死符号）——warning（§11.3 书写规则表）
    let unused: Vec<(String, Span)> = ctx
        .token_defs
        .iter()
        .filter(|(key, _)| !ctx.token_refs.contains(*key))
        .map(|(key, span)| (key.clone(), *span))
        .collect();
    for (key, span) in unused {
        ctx.warning(
            span,
            format!("unused Token `{key}` (declared but never referenced)"),
            false,
        );
    }
    // 引用未定义 Token → error（§11.3 书写规则表；verify 与 codegen 双层一致）
    let undefined: Vec<(String, Span)> = ctx
        .token_refs_token
        .iter()
        .filter(|(key, _)| !ctx.token_defs.contains_key(*key))
        .map(|(key, span)| (key.clone(), *span))
        .collect();
    for (key, span) in undefined {
        ctx.error(span, format!("undefined Token `{key}` (no definition)"));
    }
    AdaptiveCheck {
        errors: ctx.errors,
        warnings: ctx.warnings,
    }
}

/// `.arml.as` 污染检查（RFC 037 §4.3 P1 红线）：`.arml` ↔ `.arml.as` 双文件配对扫描。
///
/// `.arml.as` 出现**任何**「端侧/尺寸」字样（`ActualWidth`、`Idiom`、`IsLandscape`…）
/// 即违反 P1（自适应是声明数据，不是 code-behind 控制流）。
pub fn check_codebehind_pollution(arml_path: &Path) -> Vec<ArmlError> {
    const POLLUTION_WORDS: &[&str] = &[
        "ActualWidth",
        "ActualHeight",
        "MinWidth",
        "MaxWidth",
        "MinHeight",
        "MaxHeight",
        "IsLandscape",
        "IsPortrait",
        "Idiom",
        "DeviceType",
    ];
    let Some(stem) = arml_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let codebehind = arml_path.with_file_name(format!("{stem}.arml.as"));
    if !codebehind.is_file() {
        return Vec::new();
    }
    let Ok(src) = std::fs::read_to_string(&codebehind) else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        for w in POLLUTION_WORDS {
            if contains_word(line, w) {
                issues.push(ArmlError::type_error(
                    Span::dummy(),
                    format!(
                        "`{}` line {}: `.arml.as` pollution — `{}` is size/platform wording \
                         (P1 red line, RFC 037 §4.3; adaptive belongs in `.arml`)",
                        codebehind.display(),
                        idx + 1,
                        w
                    ),
                ));
                break;
            }
        }
    }
    issues
}

fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i + word.len() <= bytes.len() {
        if &bytes[i..i + word.len()] == word.as_bytes() {
            let before = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            let after = i + word.len() == bytes.len()
                || !(bytes[i + word.len()].is_ascii_alphanumeric()
                    || bytes[i + word.len()] == b'_');
            if before && after {
                return true;
            }
        }
        i += 1;
    }
    false
}

impl Ctx {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(ArmlError::type_error(span, message));
    }

    /// strict 模式下，`strict_escalates` 的 warning 升级为 error。
    fn warning(&mut self, span: Span, message: impl Into<String>, strict_escalates: bool) {
        let msg = message.into();
        if strict_escalates && self.strict {
            self.errors.push(ArmlError::type_error(span, msg));
        } else {
            self.warnings.push(ArmlError::type_error(span, msg));
        }
    }

    /// 树遍历：根元素声明（Tiers/Media）+ 值元素 / Match / Adaptive / 布局容器。
    fn walk(&mut self, el: &Element, parent: Option<&str>, scope: &Scope) {
        match el.name.as_str() {
            "Application" => {
                self.collect_application_declarations(el);
                for child in el.child_elements() {
                    if child.name == "Tiers" || child.name == "Media" {
                        continue;
                    }
                    self.walk(child, Some("Application"), &Scope::global());
                }
                return;
            }
            "Window" | "UserControl" | "Page" => {
                let local = self.collect_local_tiers(el);
                let sub = Scope { local_tiers: local };
                for child in el.child_elements() {
                    if child.name == "Tiers" {
                        continue;
                    }
                    self.walk(child, Some(el.name.as_str()), &sub);
                }
                return;
            }
            _ => {}
        }
        if let Some(vt) = ValueType::from_element_name(&el.name) {
            self.check_value_element(el, vt, scope);
            return;
        }
        if el.name == "Match" {
            if parent.is_none_or(|p| ValueType::from_element_name(p).is_none()) {
                self.error(
                    el.span,
                    "`<Match>` must be a child of a type-valued element (`<Double>`/`<Color>`/`<TrackList>`/`<Thickness>`/`<Boolean>`/`<String>`)",
                );
            }
            return;
        }
        if el.name == "Adaptive" {
            self.check_adaptive_element(el, scope);
            for child in el.child_elements() {
                self.walk(child, Some("Adaptive"), scope);
            }
            return;
        }
        // 收集 `{Token}`/`{StaticResource}` 引用（未使用 Token 检查用）
        for attr in &el.attributes {
            if let Some(ext) = attr.value.as_markup() {
                if matches!(ext.kind, MarkupKind::Token | MarkupKind::StaticResource) {
                    for a in &ext.args {
                        self.token_refs.insert(a.to_string());
                    }
                    if ext.kind == MarkupKind::Token {
                        // 未定义 Token 检查表：记首次引用位置（StaticResource 不入表）
                        for a in &ext.args {
                            self.token_refs_token
                                .entry(a.to_string())
                                .or_insert(ext.span);
                        }
                    }
                }
            }
        }
        // 同父 `<Adaptive>` 兄弟断点区间单调性（§11.3 断点区间重叠 → error）
        self.check_adaptive_sibling_intervals(el);
        for child in el.child_elements() {
            self.walk(child, Some(el.name.as_str()), scope);
        }
    }

    // ===== 声明收集 =====

    /// `<Application>` 根：`<Application.Tiers>`（全局档位）+ `<Application.Media>`（媒体坐标）。
    fn collect_application_declarations(&mut self, el: &Element) {
        for child in el.child_elements() {
            match child.name.as_str() {
                "Tiers" => {
                    if self.app_tiers_decl_count > 0 {
                        self.error(
                            child.span,
                            "duplicate `<Application.Tiers>` global declaration (unique per scope)",
                        );
                    }
                    self.app_tiers_decl_count += 1;
                    let errors =
                        parse_tiers_decl(child, &mut self.global_tiers, "`<Application.Tiers>`");
                    self.errors.extend(errors);
                }
                "Media" => {
                    for m in child.child_elements() {
                        if m.name != "Media" {
                            self.error(
                                m.span,
                                "`<Application.Media>` accepts only `<Media>` children",
                            );
                            continue;
                        }
                        self.parse_media_decl(m);
                    }
                }
                _ => {}
            }
        }
    }

    /// `<Media Name="..." Type="...">` 参数化坐标声明。
    fn parse_media_decl(&mut self, el: &Element) {
        let mut name: Option<SmolStr> = None;
        let mut ty: Option<SmolStr> = None;
        for attr in &el.attributes {
            let qn = attr.qualified_name();
            if qn == "xmlns" || qn.starts_with("xmlns:") || attr.prefix.is_some() {
                continue;
            }
            match attr.name.as_str() {
                "Name" => name = attr.value.as_literal().map(SmolStr::from),
                "Type" => ty = attr.value.as_literal().map(SmolStr::from),
                other => self.error(
                    attr.span,
                    format!("unknown attribute `{other}` on `<Media>`"),
                ),
            }
        }
        let Some(name) = name else {
            self.error(el.span, "`<Media>` requires `Name`");
            return;
        };
        let Some(ty) = ty else {
            self.error(
                el.span,
                format!("`<Media Name=\"{name}\">` requires `Type`"),
            );
            return;
        };
        match parse_media_type(&ty) {
            Ok(spec) => {
                if self.app_media.contains_key(name.as_str()) {
                    self.error(
                        el.span,
                        format!("duplicate `<Media Name=\"{name}\">` declaration"),
                    );
                } else {
                    self.app_media.insert(name.to_string(), spec);
                }
            }
            Err(msg) => self.error(el.span, format!("`<Media Name=\"{name}\">`: {msg}")),
        }
    }

    /// `<Window.Tiers>` 局部覆盖：唯一于作用域，重复覆盖 → error。
    fn collect_local_tiers(&mut self, el: &Element) -> Option<BTreeMap<String, f64>> {
        let mut decls: Vec<&Element> = Vec::new();
        for child in el.child_elements() {
            if child.name == "Tiers" {
                decls.push(child);
            }
        }
        if decls.len() > 1 {
            for d in &decls {
                self.error(
                    d.span,
                    format!(
                        "duplicate `<{}>.Tiers` local override (unique per scope)",
                        el.name
                    ),
                );
            }
            return None;
        }
        decls.first().map(|d| {
            let mut map = BTreeMap::new();
            let errors = parse_tiers_decl(d, &mut map, "`<Window.Tiers>`");
            self.errors.extend(errors);
            map
        })
    }

    // ===== 值元素 / Match / Adaptive 检查 =====

    fn check_value_element(&mut self, el: &Element, vt: ValueType, scope: &Scope) {
        // Token 定义（x:Key / Key）
        if let Some(key) = key_of(el) {
            self.token_defs.insert(key.to_string(), el.span);
        }
        // 未知属性 → error（§11.2：未知属性/类型不匹配 = 编译错误）
        for attr in &el.attributes {
            let qn = attr.qualified_name();
            if qn == "xmlns" || qn.starts_with("xmlns:") || attr.prefix.as_deref() == Some("x") {
                continue;
            }
            if attr.name != "Value" && attr.name != "Key" {
                self.error(
                    attr.span,
                    format!("unknown attribute `{}` on `<{}>`", attr.name, el.name),
                );
            }
        }
        // 子元素只允许 `<Match>`
        for child in el.child_elements() {
            if child.name != "Match" {
                self.error(
                    child.span,
                    format!("`<{}>` accepts only `<Match>` condition children", el.name),
                );
            }
        }
        // 常量值（无谓词 Token）
        let base_value = el.attr("Value").map(|a| a.value.clone());
        match &base_value {
            Some(AttributeValue::Literal(v)) => {
                self.check_literal(vt, v, el.span, el.name.as_str())
            }
            Some(AttributeValue::MarkupExtension(ext)) => {
                if ext.kind == MarkupKind::Token {
                    for a in &ext.args {
                        self.token_refs.insert(a.to_string());
                        self.token_refs_token
                            .entry(a.to_string())
                            .or_insert(ext.span);
                    }
                } else {
                    self.error(
                        el.attr("Value").map(|a| a.span).unwrap_or(el.span),
                        format!(
                            "`<{}> Value` must be a literal or `{{Token}}` reference",
                            el.name
                        ),
                    );
                }
            }
            None => {}
        }
        let matches: Vec<&Element> = el.child_elements().filter(|c| c.name == "Match").collect();
        // 常量值缺失且无 Match → error（`Value` 必填，§11.3）
        if base_value.is_none() && matches.is_empty() {
            self.error(
                el.span,
                format!("`<{}>` requires `Value` or `<Match>` children", el.name),
            );
        }
        for m in &matches {
            self.check_match(m, vt, scope);
        }
        self.check_match_family(el, &matches);
    }

    fn check_match(&mut self, m: &Element, vt: ValueType, scope: &Scope) {
        const MATCH_ATTRS: &[&str] = &[
            "Tier",
            "MinWidth",
            "MaxWidth",
            "Idiom",
            "Media",
            "MediaValue",
            "Density",
            "Value",
        ];
        for attr in &m.attributes {
            let qn = attr.qualified_name();
            if qn == "xmlns" || qn.starts_with("xmlns:") {
                continue;
            }
            if attr.prefix.is_some() {
                self.error(
                    attr.span,
                    format!("unexpected prefixed attribute on `<Match>`: `{qn}`"),
                );
                continue;
            }
            if !MATCH_ATTRS.contains(&attr.name.as_str()) {
                self.error(
                    attr.span,
                    format!("unknown attribute `{}` on `<Match>`", attr.name),
                );
            }
        }
        // `Value` 必填；类型由父元素决定（§11.3）
        match m.attr("Value") {
            Some(a) => match &a.value {
                AttributeValue::Literal(v) => {
                    self.check_literal(vt, v, a.span, "`<Match> Value`");
                }
                AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::Token => {
                    for arg in &ext.args {
                        self.token_refs.insert(arg.to_string());
                        self.token_refs_token
                            .entry(arg.to_string())
                            .or_insert(ext.span);
                    }
                }
                _ => self.error(
                    a.span,
                    "`<Match> Value` must be a literal or `{Token}` reference",
                ),
            },
            None => self.error(m.span, "`<Match>` requires `Value`"),
        }
        self.check_conditions(m, scope, "`<Match>`");
    }

    /// 结构化条件属性校验（`Match` 与 `<Adaptive>` 同一套，§11.3/§11.4）。
    fn check_conditions(&mut self, el: &Element, scope: &Scope, desc: &str) {
        let has_tier = el.attr("Tier").is_some();
        let has_min = el.attr("MinWidth").is_some();
        let has_max = el.attr("MaxWidth").is_some();
        // 同元素混用 Tier 与 MinWidth/MaxWidth → error（语法防混淆）
        if has_tier && (has_min || has_max) {
            self.error(
                el.span,
                format!("{desc} mixes `Tier` with `MinWidth`/`MaxWidth` (RFC 027 §11.3)"),
            );
        }
        // Tier：编译期枚举符号；引用未定义档位 → error；隐式漂移 → warning
        if let Some(a) = el.attr("Tier") {
            let name = a.value.as_literal().unwrap_or_default().to_string();
            match self.resolve_tier(&name, scope) {
                TierResolve::Unknown => self.error(
                    a.span,
                    format!("undefined Tier `{name}` (declare in `<Application.Tiers>` or a window `<Tiers>`)"),
                ),
                TierResolve::Drift => self.warning(
                    a.span,
                    format!("implicit tier threshold drift: `{name}` falls back to the global default, not declared in this window's `<Tiers>` (strict = error)"),
                    true,
                ),
                TierResolve::Known => {}
            }
        }
        // MinWidth/MaxWidth：纯数字，单位固定 vp（写单位符号 = 编译错误）
        for bw in ["MinWidth", "MaxWidth"] {
            if let Some(a) = el.attr(bw) {
                match &a.value {
                    AttributeValue::Literal(v) => {
                        if let Err(msg) = parse_breakpoint(v) {
                            self.error(a.span, format!("{desc} {bw}: {msg}"));
                        }
                    }
                    _ => self.error(
                        a.span,
                        format!("{desc} {bw} must be a literal plain number"),
                    ),
                }
            }
        }
        // Idiom：编译期枚举符号
        if let Some(a) = el.attr("Idiom") {
            let v = a.value.as_literal().unwrap_or_default();
            if !["Desktop", "Mobile", "Tablet", "TV", "Watch"].contains(&v) {
                self.error(
                    a.span,
                    format!("undefined Idiom `{v}` (expected Desktop/Mobile/Tablet/TV/Watch)"),
                );
            }
        }
        // Density：编译期枚举符号
        if let Some(a) = el.attr("Density") {
            let v = a.value.as_literal().unwrap_or_default();
            if !["compact", "comfortable", "cozy"].contains(&v) {
                self.error(
                    a.span,
                    format!("undefined Density `{v}` (expected compact/comfortable/cozy)"),
                );
            }
        }
        // Media / MediaValue 配对（坐标集 = 内置 ∪ `<Application.Media>` 声明）
        let media_attr = el.attr("Media");
        let mval_attr = el.attr("MediaValue");
        if mval_attr.is_some() && media_attr.is_none() {
            self.error(
                mval_attr.map(|a| a.span).unwrap_or(el.span),
                format!("{desc} `MediaValue` requires a paired `Media`"),
            );
        }
        if let Some(a) = media_attr {
            let name = a.value.as_literal().unwrap_or_default().to_string();
            if let Some(spec) = self.app_media.get(&name) {
                // 参数化坐标：声明处定类型，引用处须配 MediaValue
                if let Some(mv) = mval_attr {
                    if let AttributeValue::Literal(v) = &mv.value {
                        if let Err(msg) = self.validate_media_value(spec, v) {
                            self.error(mv.span, format!("{desc} `MediaValue`: {msg}"));
                        }
                    }
                } else {
                    self.error(
                        a.span,
                        format!("parameterized Media `{name}` requires `MediaValue`"),
                    );
                }
            } else if is_builtin_media(&name) {
                // 无参数坐标不得使用 MediaValue
                if let Some(mv) = mval_attr {
                    self.error(
                        mv.span,
                        format!("builtin Media `{name}` is not parameterized; `MediaValue` is not allowed"),
                    );
                }
            } else {
                self.error(
                    a.span,
                    format!(
                        "undefined Media `{name}` (builtin or declared in `<Application.Media>`)"
                    ),
                );
            }
        }
    }

    fn resolve_tier(&self, name: &str, scope: &Scope) -> TierResolve {
        if let Some(local) = &scope.local_tiers {
            if local.contains_key(name) {
                return TierResolve::Known;
            }
            if self.global_tiers.contains_key(name) {
                return TierResolve::Drift;
            }
            return TierResolve::Unknown;
        }
        if self.global_tiers.contains_key(name) {
            TierResolve::Known
        } else {
            TierResolve::Unknown
        }
    }

    fn validate_media_value(&self, spec: &MediaValueSpec, value: &str) -> Result<(), String> {
        match spec {
            MediaValueSpec::Length(unit) => {
                let (_, u) = split_length(value)?;
                match u {
                    None if *unit == "vp" => Ok(()),
                    Some(u) if u == *unit => Ok(()),
                    _ => Err(format!(
                        "`MediaValue` for a `Length({unit})` media must be a plain number (default vp) or carry unit `{unit}`"
                    )),
                }
            }
            MediaValueSpec::Ratio => {
                if value.trim().parse::<f64>().is_ok() {
                    Ok(())
                } else {
                    Err("`MediaValue` for a `Ratio` media must be a plain number".into())
                }
            }
            MediaValueSpec::String => Ok(()),
            MediaValueSpec::Enum(_) => Ok(()),
        }
    }

    fn check_literal(&mut self, vt: ValueType, value: &str, span: Span, desc: &str) {
        if let Err(msg) = check_value_literal(vt, value) {
            self.error(span, format!("invalid {desc} literal: {msg}"));
        }
    }

    /// `<Adaptive>`：与 `Match` 同一套结构化条件属性（§11.4）。
    fn check_adaptive_element(&mut self, el: &Element, scope: &Scope) {
        self.check_conditions(el, scope, "`<Adaptive>`");
    }

    // ===== 跨 Match / 兄弟规则 =====

    fn check_match_family(&mut self, el: &Element, matches: &[&Element]) {
        let has_tier = matches.iter().any(|m| m.attr("Tier").is_some());
        let has_bp = matches
            .iter()
            .any(|m| m.attr("MinWidth").is_some() || m.attr("MaxWidth").is_some());
        // 同一 Token 混用 Tier 与 MinWidth/MaxWidth → error（语法防混淆）
        if has_tier && has_bp {
            self.error(
                el.span,
                format!(
                    "`<{}>` mixes `Tier` and `MinWidth`/`MaxWidth` across `<Match>` children (RFC 027 §11.3)",
                    el.name
                ),
            );
        }
        // 断点区间重叠（不单调）→ error
        let intervals: Vec<(&Element, (f64, f64))> = matches
            .iter()
            .copied()
            .filter_map(|m| breakpoint_interval(m).map(|iv| (m, iv)))
            .collect();
        if intervals.len() >= 2 {
            self.check_interval_overlap(&intervals, "`<Match>` breakpoint intervals");
        }
        // 区间未全覆盖（无兜底）→ warning（strict = error）
        let has_fallback = matches.iter().any(|m| !has_any_condition(m));
        let has_base = el.attr("Value").is_some();
        if !matches.is_empty() && !has_base && !has_fallback {
            self.warning(
                el.span,
                format!(
                    "`<{}>` has no fallback (no `Value` and no unconditional `<Match>`); condition ranges not fully covered (strict = error)",
                    el.name
                ),
                true,
            );
        }
        // 死分支（冗余）→ warning（strict = error）；同权重歧义 → warning（strict = error）
        let conds: Vec<(&Element, BTreeSet<(String, String)>)> = matches
            .iter()
            .filter(|m| has_any_condition(m))
            .map(|m| (*m, match_conditions(m)))
            .collect();
        for (i, (a, ca)) in conds.iter().enumerate() {
            for (b, cb) in conds.iter().skip(i + 1) {
                if ca.is_superset(cb) && ca.len() > cb.len() {
                    self.warning(
                        b.span,
                        format!(
                            "dead `<Match>`: conditions {} are fully covered by {} (strict = error)",
                            fmt_conds(cb),
                            fmt_conds(ca)
                        ),
                        true,
                    );
                } else if cb.is_superset(ca) && cb.len() > ca.len() {
                    self.warning(
                        a.span,
                        format!(
                            "dead `<Match>`: conditions {} are fully covered by {} (strict = error)",
                            fmt_conds(ca),
                            fmt_conds(cb)
                        ),
                        true,
                    );
                } else if !ca.is_empty() && ca == cb {
                    self.warning(
                        b.span,
                        format!(
                            "ambiguous `<Match>`: identical conditions {} (strict = error)",
                            fmt_conds(cb)
                        ),
                        true,
                    );
                }
            }
        }
    }

    fn check_adaptive_sibling_intervals(&mut self, el: &Element) {
        let adaptive: Vec<&Element> = el
            .child_elements()
            .filter(|c| c.name == "Adaptive")
            .collect();
        let intervals: Vec<(&Element, (f64, f64))> = adaptive
            .iter()
            .copied()
            .filter_map(|a| breakpoint_interval(a).map(|iv| (a, iv)))
            .collect();
        if intervals.len() >= 2 {
            self.check_interval_overlap(&intervals, "`<Adaptive>` breakpoint intervals");
        }
    }

    fn check_interval_overlap(&mut self, intervals: &[(&Element, (f64, f64))], desc: &str) {
        let mut sorted: Vec<&(&Element, (f64, f64))> = intervals.iter().collect();
        sorted.sort_by(|a, b| {
            a.1 .0
                .partial_cmp(&b.1 .0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for w in sorted.windows(2) {
            let (ea, ia) = *w[0];
            let (eb, ib) = *w[1];
            if ib.0 < ia.1 {
                self.error(
                    ea.span.merge(eb.span),
                    format!(
                        "{desc} overlap between `{0}` and `{1}` (breakpoint ranges must be monotonic, RFC 027 §11.3)",
                        fmt_interval(ia),
                        fmt_interval(ib)
                    ),
                );
            }
        }
    }
}

fn key_of(el: &Element) -> Option<&str> {
    el.attr_with_prefix("x", "Key")
        .or_else(|| el.attr("Key"))
        .and_then(|a| a.value.as_literal())
}

/// 解析 `<*.Tiers Default="sm:600 md:960 lg:1280">` 声明。
///
/// 返回解析错误；档位表写入 `map`（同一 `Default` 内重名 → error）。
fn parse_tiers_decl(el: &Element, map: &mut BTreeMap<String, f64>, desc: &str) -> Vec<ArmlError> {
    let mut errs = Vec::new();
    let Some(default) = el.attr("Default") else {
        errs.push(ArmlError::type_error(
            el.span,
            format!("{desc} requires `Default=\"name:value ...\"`"),
        ));
        return errs;
    };
    let Some(lit) = default.value.as_literal() else {
        errs.push(ArmlError::type_error(
            default.span,
            format!("{desc} `Default` must be a literal tier table"),
        ));
        return errs;
    };
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for pair in lit.split_whitespace() {
        match pair.split_once(':') {
            Some((name, value)) if !name.is_empty() => match value.parse::<f64>() {
                Ok(v) => {
                    if !seen.insert(name) {
                        errs.push(ArmlError::type_error(
                            default.span,
                            format!("{desc}: tier `{name}` declared more than once"),
                        ));
                        continue;
                    }
                    map.insert(name.to_string(), v);
                }
                Err(_) => errs.push(ArmlError::type_error(
                    default.span,
                    format!("{desc}: threshold for tier `{name}` must be a number, got `{value}`"),
                )),
            },
            _ => errs.push(ArmlError::type_error(
                default.span,
                format!("{desc}: invalid tier table entry `{pair}` (expected `name:number`)"),
            )),
        }
    }
    errs
}

fn has_any_condition(m: &Element) -> bool {
    ["Tier", "MinWidth", "MaxWidth", "Idiom", "Media", "Density"]
        .iter()
        .any(|n| m.attr(n).is_some())
}

/// Match 条件集（MediaValue 折叠进 Media 条件：`name(value)`）。
fn match_conditions(m: &Element) -> BTreeSet<(String, String)> {
    let mut s = BTreeSet::new();
    if let Some(a) = m.attr("Tier") {
        if let Some(v) = a.value.as_literal() {
            s.insert(("Tier".into(), v.to_string()));
        }
    }
    if let Some(a) = m.attr("MinWidth") {
        if let Some(v) = a.value.as_literal() {
            s.insert(("MinWidth".into(), v.to_string()));
        }
    }
    if let Some(a) = m.attr("MaxWidth") {
        if let Some(v) = a.value.as_literal() {
            s.insert(("MaxWidth".into(), v.to_string()));
        }
    }
    if let Some(a) = m.attr("Idiom") {
        if let Some(v) = a.value.as_literal() {
            s.insert(("Idiom".into(), v.to_string()));
        }
    }
    if let Some(a) = m.attr("Density") {
        if let Some(v) = a.value.as_literal() {
            s.insert(("Density".into(), v.to_string()));
        }
    }
    if let Some(a) = m.attr("Media") {
        if let Some(v) = a.value.as_literal() {
            let mv = m.attr("MediaValue").and_then(|x| x.value.as_literal());
            let key = match mv {
                Some(mv) => format!("{v}({mv})"),
                None => v.to_string(),
            };
            s.insert(("Media".into(), key));
        }
    }
    s
}

fn fmt_conds(conds: &BTreeSet<(String, String)>) -> String {
    let parts: Vec<String> = conds
        .iter()
        .map(|(dim, val)| format!("{dim}={val}"))
        .collect();
    parts.join(" & ")
}

fn fmt_interval((lo, hi): (f64, f64)) -> String {
    if hi == f64::MAX {
        format!("≥{lo}")
    } else if lo == 0.0 {
        format!("<{hi}")
    } else {
        format!("[{lo}, {hi})")
    }
}

fn breakpoint_interval(m: &Element) -> Option<(f64, f64)> {
    let min = m
        .attr("MinWidth")
        .and_then(|a| a.value.as_literal())
        .and_then(|v| v.trim().parse::<f64>().ok());
    let max = m
        .attr("MaxWidth")
        .and_then(|a| a.value.as_literal())
        .and_then(|v| v.trim().parse::<f64>().ok());
    match (min, max) {
        (Some(mn), Some(mx)) => Some((mn, mx)),
        (Some(mn), None) => Some((mn, f64::MAX)),
        (None, Some(mx)) => Some((0.0, mx)),
        (None, None) => None,
    }
}

fn is_builtin_media(name: &str) -> bool {
    BUILTIN_MEDIA.contains(&name) || name.starts_with("safe-area-") || name.starts_with("dpi:")
}

/// 解析 `<Media Type>`（§11.3）。
fn parse_media_type(t: &str) -> Result<MediaValueSpec, String> {
    let t = t.trim();
    if let Some(inner) = t.strip_prefix("Length(").and_then(|x| x.strip_suffix(')')) {
        match inner {
            "vp" => Ok(MediaValueSpec::Length("vp")),
            "px" => Ok(MediaValueSpec::Length("px")),
            "%" => Ok(MediaValueSpec::Length("%")),
            "lpx" => Ok(MediaValueSpec::Length("lpx")),
            other => Err(format!(
                "invalid `Media` Type unit `{other}` (expected vp/px/%/lpx)"
            )),
        }
    } else if t == "Ratio" {
        Ok(MediaValueSpec::Ratio)
    } else if t.eq_ignore_ascii_case("string") {
        Ok(MediaValueSpec::String)
    } else if t.is_empty() {
        Err("`<Media>` requires non-empty `Type`".into())
    } else {
        Ok(MediaValueSpec::Enum(t.to_string()))
    }
}
