//! `arc ui verify` —— 编译期验证报告（RFC 037 M1 D11 + RFC 037 M-U0/M-U1）。
//!
//! 输出：属性约束验证、绑定路径验证、Style/ResourceDictionary 校验、
//! A11y 检查、布局溢出检查、自适应编译期检查（类型化值元素/`Match`/`Tiers`/
//! 单位校验，RFC 037 §11）、`.arml.as` 污染检查（P1 红线，双文件配对扫描）。

use crate::adaptive::{check_adaptive, check_codebehind_pollution};
use crate::ast::*;
use crate::error::ArmlError;
use crate::typeck::{TypeCheckReport, TypeChecker};
use indexmap::IndexMap;

/// 验证报告。
#[derive(Debug, Clone, Default)]
pub struct VerificationReport {
    /// 类型检查报告（组件/属性/绑定/Style 语法）。
    pub type_check: TypeCheckReport,
    /// Style / ResourceDictionary 语义问题。
    pub style_issues: Vec<ArmlError>,
    /// A11y 检查发现的问题。
    pub a11y_issues: Vec<ArmlError>,
    /// 布局溢出检查发现的问题。
    pub layout_issues: Vec<ArmlError>,
    /// 自适应编译期错误（RFC 016 §11；含 strict 升级的严格 warning）。
    pub adaptive_issues: Vec<ArmlError>,
    /// 自适应非严格 warning（如未使用 Token）。
    pub adaptive_warnings: Vec<ArmlError>,
    /// `.arml.as` 污染检查发现的问题（P1 红线）。
    pub codebehind_issues: Vec<ArmlError>,
}

impl VerificationReport {
    pub fn is_ok(&self) -> bool {
        self.type_check.is_ok()
            && self.style_issues.is_empty()
            && self.a11y_issues.is_empty()
            && self.layout_issues.is_empty()
            && self.adaptive_issues.is_empty()
            && self.codebehind_issues.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.type_check.errors.len()
            + self.style_issues.len()
            + self.a11y_issues.len()
            + self.layout_issues.len()
            + self.adaptive_issues.len()
            + self.codebehind_issues.len()
    }

    pub fn warning_count(&self) -> usize {
        self.type_check.warnings.len() + self.adaptive_warnings.len()
    }
}

/// 执行完整验证：类型检查 + Style + A11y + 布局（非 strict）。
pub fn verify_report(doc: &ArmlDocument, checker: &TypeChecker) -> VerificationReport {
    verify_report_with_strict(doc, checker, false)
}

/// 执行完整验证（含自适应检查）；`strict` 时严格 warning 升级为 error。
pub fn verify_report_with_strict(
    doc: &ArmlDocument,
    checker: &TypeChecker,
    strict: bool,
) -> VerificationReport {
    let mut report = VerificationReport {
        type_check: checker.check(doc),
        ..Default::default()
    };
    check_styles(doc, &mut report);
    check_a11y(&doc.root, &mut report);
    check_layout(&doc.root, &mut report);
    let adaptive = check_adaptive(doc, strict);
    report.adaptive_issues = adaptive.errors;
    report.adaptive_warnings = adaptive.warnings;
    report
}

/// 执行 `.arml.as` 污染检查（P1 红线，双文件配对扫描），并入报告。
pub fn check_codebehind_report(arml_path: &std::path::Path, report: &mut VerificationReport) {
    report.codebehind_issues = check_codebehind_pollution(arml_path);
}

/// Style / ResourceDictionary 校验：重复 `x:Key`、BasedOn 未知引用与继承环检测。
fn check_styles(doc: &ArmlDocument, report: &mut VerificationReport) {
    let mut dictionaries = Vec::new();
    for dict in doc.collect_resource_dictionaries() {
        check_dictionary_keys(&dict, &mut report.style_issues);
        dictionaries.push(dict);
    }
    check_based_on(&dictionaries, &mut report.style_issues);
}

fn check_dictionary_keys(dict: &ResourceDictionaryDef, issues: &mut Vec<ArmlError>) {
    let mut keys: IndexMap<&str, Span> = IndexMap::new();
    for style in &dict.styles {
        if let Some(key) = style.key.as_deref() {
            if let Some(prev) = keys.get(key) {
                issues.push(ArmlError::type_error(
                    style.span,
                    format!("duplicate Style x:Key `{key}` (first at {prev:?})"),
                ));
            } else {
                keys.insert(key, style.span);
            }
        }
    }
    for merged in &dict.merged {
        check_dictionary_keys(merged, issues);
    }
    for (_, theme) in &dict.theme_entries {
        check_dictionary_keys(theme, issues);
    }
}

/// 提取 Style 的 `BasedOn` 父键（`{StaticResource key}` 或字面量）。
/// 返回借用而非 owned `SmolStr`：调用方将键存入以 `'a` 为界的继承边表，
/// 底层数据活在 AST 内，借用即可达（同 `check_dictionary_keys` 的键借用模式）。
fn style_based_on_key(style: &StyleDef) -> Option<&str> {
    match style.based_on.as_ref()? {
        AttributeValue::Literal(s) => Some(s.as_str()),
        AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::StaticResource => {
            ext.args.first().map(|s| s.as_str())
        }
        _ => None,
    }
}

/// 收集全部 keyed Style 的 `key -> (based_on_key, span)` 继承边（含 merged/theme 字典）。
fn collect_based_on_edges<'a>(
    dict: &'a ResourceDictionaryDef,
    edges: &mut IndexMap<&'a str, (&'a str, Span)>,
) {
    for style in &dict.styles {
        if let Some(key) = style.key.as_deref() {
            if let Some(parent) = style_based_on_key(style) {
                edges.entry(key).or_insert((parent, style.span));
            }
        }
    }
    for merged in &dict.merged {
        collect_based_on_edges(merged, edges);
    }
    for (_, theme) in &dict.theme_entries {
        collect_based_on_edges(theme, edges);
    }
}

/// BasedOn 校验：父键必须存在；继承链禁止成环（运行时 `ApplyWithBasedOn` 会无限递归）。
fn check_based_on(dictionaries: &[ResourceDictionaryDef], issues: &mut Vec<ArmlError>) {
    let mut edges: IndexMap<&str, (&str, Span)> = IndexMap::new();
    for dict in dictionaries {
        collect_based_on_edges(dict, &mut edges);
    }
    for (key, (parent, span)) in &edges {
        if !edges.contains_key(*parent) {
            issues.push(ArmlError::type_error(
                *span,
                format!("Style `{key}` BasedOn unknown key `{parent}`"),
            ));
        }
    }
    let mut state: IndexMap<&str, u8> = IndexMap::new();
    for start in edges.keys().copied().collect::<Vec<_>>() {
        if state.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut path: Vec<&str> = Vec::new();
        let mut cur = Some(start);
        while let Some(key) = cur {
            match state.get(key).copied().unwrap_or(0) {
                1 => {
                    let start_idx = path.iter().position(|p| *p == key).unwrap_or(0);
                    let cycle: Vec<&str> = path[start_idx..].to_vec();
                    issues.push(ArmlError::type_error(
                        edges.get(key).map(|(_, span)| *span).unwrap_or_default(),
                        format!("Style BasedOn cycle: `{}`", cycle.join(" -> ")),
                    ));
                    break;
                }
                2 => break,
                _ => {
                    state.insert(key, 1);
                    path.push(key);
                    cur = edges
                        .get(key)
                        .map(|(parent, _)| *parent)
                        .filter(|parent| edges.contains_key(*parent));
                }
            }
        }
        for key in &path {
            state.insert(key, 2);
        }
    }
}

/// A11y 检查：验证可交互控件有可访问标签。
fn check_a11y(element: &Element, report: &mut VerificationReport) {
    // Button/Input/CheckBox 等交互控件应有 Content/Text/AutomationProperties.Name
    let interactive = matches!(
        element.name.as_str(),
        "Button" | "TextBox" | "CheckBox" | "Slider"
    );
    if interactive {
        let has_label = element.attr("Content").is_some()
            || element.attr("Text").is_some()
            || element
                .attr_with_prefix("AutomationProperties", "Name")
                .is_some();
        if !has_label {
            report.a11y_issues.push(ArmlError::type_error(
                element.span,
                format!("interactive `<{}>` lacks accessible label (Content/Text/AutomationProperties.Name)", element.name),
            ));
        }
    }
    for child in &element.children {
        if let ElementChild::Element(e) = child {
            check_a11y(e, report);
        }
    }
}

/// 布局检查：检测固定尺寸容器内子元素溢出风险。
fn check_layout(element: &Element, report: &mut VerificationReport) {
    // Window 有 Width/Height 时检查子元素是否可能溢出（M1 简化版）
    if element.name == "Window" {
        if let (Some(w), Some(h)) = (element.attr("Width"), element.attr("Height")) {
            // 简化检查：只看是否存在显式尺寸
            let _ = (w, h);
        }
    }
    // StackPanel 子元素超过 ~10 个时给出建议
    if element.name == "StackPanel" {
        let child_count = element.child_elements().count();
        if child_count > 10 {
            report.layout_issues.push(ArmlError::type_error(
                element.span,
                format!("`<StackPanel>` has {child_count} children; consider `<ScrollView>` for scrolling"),
            ));
        }
    }
    for child in &element.children {
        if let ElementChild::Element(e) = child {
            check_layout(e, report);
        }
    }
}
