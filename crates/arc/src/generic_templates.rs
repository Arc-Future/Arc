//! RFC 038 M2-G1：泛型模板区收集器（publish 侧）。
//!
//! 从已解析 AST `Program` 收集**每包的泛型可达闭包源码文本**，产出
//! `声明名 → 定义体源码文本` 映射，供 `arc publish` 写入 `.aopkg`
//! metadata 的 `generic_templates`（泛型模板区）。
//!
//! ## 收集边界（RFC 038 §4.5.4）
//!
//! - **public 泛型声明**：泛型 class / struct / interface / variant /
//!   泛型方法（含扩展方法）/ 泛型顶层函数。
//! - 泛型类型捕获其**整段声明**（含体内私有辅助成员），保证模板自洽——
//!   与 `@__dict_eq_List_int` 教训同类：仅收集声明本身会导致单态化撞未定义符号。
//! - **私有辅助闭包**（public 泛型方法引用的包内 private helper / 非泛型私有
//!   类型）：本实现先收集**泛型声明级**源码；private-helper 可达闭包收敛为
//!   后续增量（M2-G1b），随 M2-G2 消费端注入一并验证。
//!
//! ## 载体
//!
//! 源码文本逐字保存（含注释/空白），消费端 load 后注入 typeck 复用单态化
//! 管线（A 载体，务实中间态；A→C 演进见 RFC 038 §4.5.2）。
//!
//! ## D4：aopkg 模板区统一优先
//!
//! 语义上本区是跨库泛型**统一权威通道**（单一惯用法，避免 std 源码双轨）；
//! 消费端与本地磁盘 `std/` 源码注入并列时以本区为准，禁止双写重复定义。

use ast::{FileId, Item, MethodDef, Program, Span, Spanned, Visibility};
use std::collections::HashMap;

use crate::loader::FileRegistry;

/// 收集每包泛型模板：`声明名 → 定义体源码文本`。
pub fn collect_generic_templates(program: &Program, files: &FileRegistry) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // 声明名 → 占位，去重（同一泛型声明只收一次）。
    let mut seen: Vec<String> = Vec::new();
    let mut src_cache: HashMap<FileId, Option<String>> = HashMap::new();
    walk_items(&program.items, files, &mut src_cache, &mut seen, &mut out);
    out
}

/// 递归遍历 items（含 namespace 嵌套），收集 public 泛型声明。
fn walk_items(
    items: &[Spanned<Item>],
    files: &FileRegistry,
    src_cache: &mut HashMap<FileId, Option<String>>,
    seen: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) {
    for it in items {
        match &it.node {
            Item::Namespace(ns) => {
                walk_items(&ns.items, files, src_cache, seen, out);
            }
            Item::Class(c) => collect_type(
                &it.span,
                c.vis,
                c.is_static,
                &c.name,
                &c.generics,
                c.methods.iter(),
                files,
                src_cache,
                seen,
                out,
                "Class",
            ),
            Item::Struct(s) => collect_type(
                &it.span,
                s.vis,
                false,
                &s.name,
                &s.generics,
                s.methods.iter(),
                files,
                src_cache,
                seen,
                out,
                "Struct",
            ),
            Item::Interface(i) => {
                // 接口无方法体，其泛型模板为签名面；捕获整段声明（含方法签名）。
                collect_type(
                    &it.span,
                    i.vis,
                    false,
                    &i.name,
                    &i.generics,
                    std::iter::empty(),
                    files,
                    src_cache,
                    seen,
                    out,
                    "Interface",
                );
            }
            Item::Variant(v) => {
                let is_pub = v.vis == Visibility::Public;
                let is_generic = !v.generics.is_empty();
                if is_pub && is_generic {
                    push_span(&it.span, &v.name, files, src_cache, seen, out);
                }
            }
            Item::Fn(f) => {
                let is_pub = f.vis == Visibility::Public;
                let is_generic = !f.generics.is_empty();
                if is_pub && is_generic {
                    push_span(&it.span, &f.name, files, src_cache, seen, out);
                }
            }
            _ => {}
        }
    }
}

/// 泛型类型：捕获整段声明（含体内私有辅助成员）。非泛型类型仅收集其泛型方法。
#[allow(clippy::too_many_arguments)]
fn collect_type<'a, I>(
    type_span: &Span,
    vis: Visibility,
    is_static: bool,
    name: &ast::Ident,
    generics: &[ast::GenericParam],
    methods: I,
    files: &FileRegistry,
    src_cache: &mut HashMap<FileId, Option<String>>,
    seen: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
    _kind: &str,
) where
    I: Iterator<Item = &'a Spanned<MethodDef>>,
{
    let is_pub = vis == Visibility::Public;
    let is_generic_type = !generics.is_empty();
    if is_pub && is_generic_type {
        // 泛型类型：整段声明即模板（覆盖其所有方法，含 private helper）。
        push_span(type_span, name, files, src_cache, seen, out);
        return;
    }
    if !is_pub {
        return;
    }
    let ms: Vec<&Spanned<MethodDef>> = methods.collect();
    // M2-G1b：非泛型 `public static class` 含泛型方法 → 整段捕获为模板。
    //
    // 泛型方法（如 DI `ServiceCollectionExtensions.AddTransient<T>`）的裸方法体
    // 无法独立重解析（消费端 `parse_program_in_file` 只接受顶层 item，方法体
    // 引用 enclosing 成员），故把整个静态类作为自洽模板单元捕获——与「泛型类型
    // 整段声明即模板」同构：类内 private helper / 非泛型成员一并包含，保证
    // 消费端注入后可重解析、可单态化。
    if is_static && ms.iter().any(|m| !m.node.sig.generics.is_empty()) {
        push_span(type_span, name, files, src_cache, seen, out);
        return;
    }
    // 其余非泛型类型：收集其中泛型方法（裸方法体，消费端 M2-G1b 边界内仍不可
    // 独立重解析——非 static 类泛型方法的 enclosing 闭包收敛属后续增量）。
    for m in ms {
        if !m.node.sig.generics.is_empty() {
            let key = format!("{}.{}", name, m.node.sig.name);
            push_span(&m.span, &ast::Ident::new(key), files, src_cache, seen, out);
        }
    }
}

/// 从 span 切片源文件字节 → UTF-8（lossy），按声明名 push，去重。
fn push_span(
    span: &Span,
    name: &ast::Ident,
    files: &FileRegistry,
    src_cache: &mut HashMap<FileId, Option<String>>,
    seen: &mut Vec<String>,
    out: &mut Vec<(String, String)>,
) {
    let key = name.to_string();
    if seen.iter().any(|s| s == &key) {
        return;
    }
    let Some(src) = source_for(files, src_cache, span) else {
        return;
    };
    seen.push(key.clone());
    out.push((key, src));
}

/// 读取 FileId 对应源文件，按 span 字节区间切片（缓存整文件）。
fn source_for(
    files: &FileRegistry,
    cache: &mut HashMap<FileId, Option<String>>,
    span: &Span,
) -> Option<String> {
    if span.file_id == 0 {
        return None;
    }
    let content = cache.entry(span.file_id).or_insert_with(|| {
        files
            .path_of(span.file_id)
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    });
    let content = content.as_ref()?;
    let start = span.start as usize;
    let end = (span.end as usize).min(content.len());
    if start >= end {
        return None;
    }
    Some(content[start..end].to_string())
}
