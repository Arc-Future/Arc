//! LSP 语义查询层（RFC 038 M1）。
//!
//! ## 职责
//!
//! 桥接 `.arcgr` 语义索引与 LSP 语义 provider（definition / hover / references /
//! documentSymbol）。纯消费 `arcgr` crate 现有数据，不触碰编译器核心。
//!
//! ## 关键设计
//!
//! - [`SemanticIndex`] 持有 `ArcgrFile` + 惰性源码缓存，负责文件路径→URI、行/列↔字节
//!   偏移的转换（LSP 位置模型 0-based；`.arcgr` span 为字节偏移）
//! - [`LineMap`]：源码字节偏移 ↔ (行, 列) 双向映射（按 `\n` 切行）
//! - **列口径**：M1 将 LSP `character` 视为行内 UTF-8 字节偏移（ASCII 主导源码下与
//!   字符计数一致；UTF-16 精确列映射留待 M2+）
//!
//! ## 符号解析
//!
//! [`SemanticIndex::symbol_at_offset`] 两步解析光标处符号：
//! 1. 先匹配 SymbolTable 中**定义 span** 覆盖光标的符号（光标落在定义上）
//! 2. 再匹配 ReferenceTable 中**引用 span** 覆盖光标的条目（光标落在使用处）→ 反查目标符号

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lines::LineIndex;
use arcgr::{
    ArcgrFile, FileTable, ReferenceTable, SymbolEntry, SymbolKind, SymbolTable, TypeSig, Visibility,
};

// ============================================================================
// LSP 位置模型（0-based）
// ============================================================================

/// LSP 位置（0-based 行/列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// LSP 区间。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// LSP 位置（文档 URI + 区间）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// LSP hover 内容（markdown 字符串 + 可选用例范围）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hover {
    pub contents: HoverContents,
}

/// hover 内容——markdown 标记字符串。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HoverContents {
    pub kind: String,
    pub value: String,
}

/// LSP documentSymbol 条目。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DocumentSymbol {
    pub name: String,
    /// LSP SymbolKind 枚举值（见 [`DocumentSymbol::kind_of`]）。
    pub kind: i32,
    pub range: Range,
    /// LSP 键为 `selectionRange`（camelCase，区别于 rust 字段的 snake_case）。
    #[serde(rename = "selectionRange")]
    pub selection_range: Range,
}

/// LSP `workspace/symbol` 条目（`SymbolInformation` 结构，3.17 合法返回类型）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SymbolInformation {
    pub name: String,
    /// LSP SymbolKind 枚举值。
    pub kind: i32,
    pub location: Location,
}

// ============================================================================
// 语义索引
// ============================================================================

/// 一份源码文档——源码文本 + 共享的 UTF-16 行索引。
#[derive(Debug, Clone)]
struct SourceDoc {
    text: String,
    index: LineIndex,
}

/// LSP 语义索引——持有 `ArcgrFile` 与惰性源码缓存。
#[derive(Debug, Default, Clone)]
pub struct SemanticIndex {
    arcgr: ArcgrFile,
    base_dir: PathBuf,
    /// file_id → 文档缓存（`None` 表示读取失败，避免重复 IO）。
    sources: HashMap<u32, Option<SourceDoc>>,
    /// 规范化路径 → file_id（URI 解析用，加载时构建）。
    path_index: HashMap<String, u32>,
}

impl SemanticIndex {
    /// 从磁盘加载 `.arcgr`。
    pub fn load(path: &Path, base_dir: PathBuf) -> Result<Self, String> {
        let bytes =
            std::fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
        let arcgr = arcgr::read_arcgr(&bytes).map_err(|e| format!("parse .arcgr failed: {e}"))?;
        Ok(Self::from_arcgr(arcgr, base_dir))
    }

    /// 从内存 `ArcgrFile` 构建（测试友好）。
    pub fn from_arcgr(arcgr: ArcgrFile, base_dir: PathBuf) -> Self {
        let path_index = build_path_index(&arcgr.file_table);
        Self {
            arcgr,
            base_dir,
            sources: HashMap::new(),
            path_index,
        }
    }

    /// 底层 `.arcgr` 引用。
    pub fn arcgr(&self) -> &ArcgrFile {
        &self.arcgr
    }

    /// 直接注入 file_id 的源码文档（测试与增量更新用，避免依赖磁盘）。
    pub fn inject_source(&mut self, file_id: u32, text: impl Into<String>) {
        let text = text.into();
        self.sources.insert(
            file_id,
            Some(SourceDoc {
                index: LineIndex::new(&text),
                text,
            }),
        );
    }

    /// 以开放文档的**当前文本**覆盖 file_id 的源码（Document 统一体系）。
    ///
    /// 开放文档（didOpen/didChange 后的缓冲）作为文件真源，覆盖磁盘上的旧内容，
    /// 使语义 provider（definition/hover/references/documentSymbol）的位置换算
    /// 与用户所见一致（含未保存编辑）。
    pub fn set_source_text(&mut self, file_id: u32, text: impl Into<String>) {
        let text = text.into();
        self.sources.insert(
            file_id,
            Some(SourceDoc {
                index: LineIndex::new(&text),
                text,
            }),
        );
    }

    /// 失效 file_id 的源码覆盖（didClose）——移除缓存，下次访问回落到磁盘。
    pub fn invalidate_source(&mut self, file_id: u32) {
        self.sources.remove(&file_id);
    }

    // ─── 文件解析 ───

    /// 将 LSP `uri`（`file:///abs/path`）解析为 `file_id`。
    ///
    /// 匹配口径：解码 `file://` 前缀后，先精确匹配 FileTable.path，再按规范化
    /// 绝对路径匹配（容忍客户端/索引端路径写法差异）。
    pub fn file_id_for_uri(&self, uri: &str) -> Option<u32> {
        let path = uri_to_path(uri);
        self.path_index.get(&path).copied().or_else(|| {
            // 兜底：规范化后匹配
            let canon = normalize_path(&path);
            self.arcgr
                .file_table
                .entries
                .iter()
                .find(|e| normalize_path(&e.path) == canon)
                .map(|e| e.file_id)
        })
    }

    /// 从文件系统读取（或缓存）file_id 对应的源码文档。
    fn source_for(&mut self, file_id: u32) -> Option<&SourceDoc> {
        if !self.sources.contains_key(&file_id) {
            let doc = self.load_source(file_id);
            self.sources.insert(file_id, doc);
        }
        self.sources.get(&file_id).and_then(|d| d.as_ref())
    }

    /// 将 LSP 位置（0-based 行/列，UTF-16 口径）转为字节偏移。
    /// 行越界或文件不可读返回 `None`。
    pub fn position_to_offset(&mut self, file_id: u32, pos: Position) -> Option<usize> {
        let doc = self.source_for(file_id)?;
        doc.index.offset_of(&doc.text, pos.line, pos.character)
    }

    fn load_source(&self, file_id: u32) -> Option<SourceDoc> {
        let path = self.arcgr.file_table.find(file_id)?.path.clone();
        let resolved = resolve_path(&self.base_dir, &path);
        let text = std::fs::read_to_string(&resolved).ok()?;
        Some(SourceDoc {
            index: LineIndex::new(&text),
            text,
        })
    }

    // ─── 符号解析 ───

    /// 解析光标处符号：先定义 span，后引用 span。
    pub fn symbol_at_offset(&self, file_id: u32, offset: usize) -> Option<SymbolEntry> {
        // 1. 定义 span 匹配（光标落在某个符号的定义区间内）
        if let Some(sym) = symbol_table_at_offset(&self.arcgr.symbol_table, file_id, offset) {
            return Some(sym);
        }
        // 2. 引用 span 匹配 → 反查目标符号
        reference_target_at_offset(
            &self.arcgr.reference_table,
            &self.arcgr.symbol_table,
            file_id,
            offset,
        )
        .cloned()
    }

    /// 定义定位：返回 symbol_id 的定义位置（URI + 区间）。
    pub fn definition(&mut self, symbol_id: u32) -> Option<Location> {
        let sym = self.arcgr.symbol_table.find(symbol_id)?.clone();
        let doc = self.source_for(sym.file_id)?;
        let (sl, sc) = doc.index.position_of(&doc.text, sym.span_start as usize);
        let (el, ec) = doc.index.position_of(&doc.text, sym.span_end as usize);
        Some(Location {
            uri: path_to_uri(&self.arcgr.file_table, sym.file_id),
            range: Range {
                start: Position {
                    line: sl,
                    character: sc,
                },
                end: Position {
                    line: el,
                    character: ec,
                },
            },
        })
    }

    /// 查找引用：返回目标符号的全部引用位置（含定义本身）。
    pub fn references(&mut self, symbol_id: u32) -> Vec<Location> {
        let mut out = Vec::new();
        // 定义位置本身
        if let Some(loc) = self.definition(symbol_id) {
            out.push(loc);
        }
        // ReferenceTable 中的使用点（先收集再遍历，避免借用冲突）
        let entries: Vec<_> = self
            .arcgr
            .reference_table
            .find_by_symbol(symbol_id)
            .into_iter()
            .cloned()
            .collect();
        for entry in entries {
            if let Some(doc) = self.source_for(entry.file_id) {
                let (sl, sc) = doc.index.position_of(&doc.text, entry.span_start as usize);
                let (el, ec) = doc.index.position_of(&doc.text, entry.span_end as usize);
                out.push(Location {
                    uri: path_to_uri(&self.arcgr.file_table, entry.file_id),
                    range: Range {
                        start: Position {
                            line: sl,
                            character: sc,
                        },
                        end: Position {
                            line: el,
                            character: ec,
                        },
                    },
                });
            }
        }
        out
    }

    /// hover：符号签名 + 文档摘要（markdown）。
    pub fn hover(&mut self, symbol_id: u32) -> Option<Hover> {
        let sym = self.arcgr.symbol_table.find(symbol_id)?;
        let mut value = format!(
            "```arc\n{} · {}\n```",
            sym.name,
            format_type_sig(&sym.type_sig)
        );
        if let Some(doc) = &sym.doc_summary {
            value.push_str("\n\n");
            value.push_str(doc);
        }
        Some(Hover {
            contents: HoverContents {
                kind: "markdown".into(),
                value,
            },
        })
    }

    /// documentSymbol：列出指定文件内的全部符号（含 selection_range）。
    pub fn document_symbols(&mut self, file_id: u32) -> Vec<DocumentSymbol> {
        let mut out = Vec::new();
        let Some(doc) = self.source_for(file_id).cloned() else {
            return out;
        };
        for sym in &self.arcgr.symbol_table.entries {
            if sym.file_id != file_id {
                continue;
            }
            let (sl, sc) = doc.index.position_of(&doc.text, sym.span_start as usize);
            let (el, ec) = doc.index.position_of(&doc.text, sym.span_end as usize);
            let start = Position {
                line: sl,
                character: sc,
            };
            let end = Position {
                line: el,
                character: ec,
            };
            out.push(DocumentSymbol {
                name: sym.name.clone(),
                kind: symbol_kind_lsp(sym.kind),
                range: Range { start, end },
                selection_range: Range { start, end },
            });
        }
        out
    }

    /// 跨包符号查询（M3）：返回本包内匹配的**公共**符号（LSP `workspace/symbol`）。
    ///
    /// `query` 为大小写不敏感子串匹配；`None`/空查询返回全部公共符号。
    /// 仅暴露 `Public` 符号——非公共符号不可被其他包引用/查询。
    pub fn workspace_symbols(&mut self, query: Option<&str>) -> Vec<SymbolInformation> {
        let mut out = Vec::new();
        let q = query.unwrap_or("").to_lowercase();
        // 先收集匹配的符号信息（避免借用冲突：读取不可变、后续 source_for 需可变）
        let matches: Vec<(u32, u32, u32, String, i32)> = self
            .arcgr
            .symbol_table
            .entries
            .iter()
            .filter(|sym| {
                if sym.visibility != Visibility::Public {
                    return false;
                }
                q.is_empty() || sym.name.to_lowercase().contains(&q)
            })
            .map(|sym| {
                (
                    sym.file_id,
                    sym.span_start,
                    sym.span_end,
                    sym.name.clone(),
                    symbol_kind_lsp(sym.kind),
                )
            })
            .collect();
        for (file_id, start, end, name, kind) in matches {
            let Some(doc) = self.source_for(file_id) else {
                continue;
            };
            let (sl, sc) = doc.index.position_of(&doc.text, start as usize);
            let (el, ec) = doc.index.position_of(&doc.text, end as usize);
            out.push(SymbolInformation {
                name,
                kind,
                location: Location {
                    uri: path_to_uri(&self.arcgr.file_table, file_id),
                    range: Range {
                        start: Position {
                            line: sl,
                            character: sc,
                        },
                        end: Position {
                            line: el,
                            character: ec,
                        },
                    },
                },
            });
        }
        out
    }

    /// 检测光标处是否为**外部引用**（M3 跨包跳转辅助）。
    ///
    /// 命中引用 span、但目标 `symbol_id` 在本包不可解析（即指向其他包定义）时，
    /// 返回引用 span 覆盖的源码文本（= 被引用标识符名），供跨包按名解析。
    /// 本地可解析的引用返回 `None`（本地路径已能处理）。
    pub fn external_ref_name_at_offset(&mut self, file_id: u32, offset: usize) -> Option<String> {
        // 先复制引用条目字段（避免借用冲突：随后 source_for 需可变借用）
        let (symbol_id, start, end) = self
            .arcgr
            .reference_table
            .entries
            .iter()
            .find(|e| {
                e.file_id == file_id
                    && e.span_start as usize <= offset
                    && offset <= e.span_end as usize
            })
            .map(|e| (e.symbol_id, e.span_start as usize, e.span_end as usize))?;
        // 目标本地可解析 → 非外部引用（本地路径已能处理）
        if self.arcgr.symbol_table.find(symbol_id).is_some() {
            return None;
        }
        let doc = self.source_for(file_id)?;
        if start >= end || end > doc.text.len() {
            return None;
        }
        Some(doc.text[start..end].to_string())
    }
}

/// 一个包——`package_id` + 语义索引（对应一个 `.arcgr`）。
///
/// M3 跨包查询：一个 workspace 可持有主包 + 若干依赖包，`workspace/symbol`
/// 聚合所有包的公共符号，实现跨包定位（含依赖库导出的符号）。
#[derive(Debug, Clone)]
pub struct PackageIndex {
    /// 包标识（包名或依赖路径）。
    pub id: String,
    /// 该包的语义索引。
    pub semantic: SemanticIndex,
}

impl PackageIndex {
    pub fn new(id: impl Into<String>, semantic: SemanticIndex) -> Self {
        Self {
            id: id.into(),
            semantic,
        }
    }
}

/// 全局符号引用——`package_id + symbol_id`（跨包身份）。
///
/// `.arcgr` 的 `symbol_id` 是包内局部 u32，跨包跳转需以「包 + 符号」二元组唯一定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalSymbolRef {
    pub package_id: String,
    pub symbol_id: u32,
}

impl GlobalSymbolRef {
    pub fn new(package_id: impl Into<String>, symbol_id: u32) -> Self {
        Self {
            package_id: package_id.into(),
            symbol_id,
        }
    }
}

// ============================================================================
// 纯查询辅助（无 self——可独立测试）
// ============================================================================

/// 在 SymbolTable 中查找定义 span 覆盖 (file_id, offset) 的符号。
pub fn symbol_table_at_offset(
    table: &SymbolTable,
    file_id: u32,
    offset: usize,
) -> Option<SymbolEntry> {
    table
        .entries
        .iter()
        .find(|e| {
            e.file_id == file_id && e.span_start as usize <= offset && offset <= e.span_end as usize
        })
        .cloned()
}

/// 在 ReferenceTable 中查找引用 span 覆盖 (file_id, offset) 的条目，反查目标符号。
pub fn reference_target_at_offset<'a>(
    table: &'a ReferenceTable,
    symbols: &'a SymbolTable,
    file_id: u32,
    offset: usize,
) -> Option<&'a SymbolEntry> {
    let entry = table.entries.iter().find(|e| {
        e.file_id == file_id && e.span_start as usize <= offset && offset <= e.span_end as usize
    })?;
    symbols.find(entry.symbol_id)
}

// ============================================================================
// 路径 / URI 转换
// ============================================================================

/// 构建「规范化路径 → file_id」索引。
fn build_path_index(table: &FileTable) -> HashMap<String, u32> {
    table
        .entries
        .iter()
        .map(|e| (normalize_path(&e.path), e.file_id))
        .collect()
}

/// 去掉 `file://` 前缀并保留绝对路径（Windows 盘符兼容）。
fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://").unwrap_or(uri).to_string()
}

/// 规范化路径——统一分隔符并去尾部分隔符，便于跨客户端/索引端比较。
fn normalize_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized.trim_end_matches('/').to_string()
}

/// 将 file_id 的路径解析为绝对路径（相对 base_dir 时拼接）。
fn resolve_path(base_dir: &Path, path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        base_dir.join(p)
    }
}

/// 将 file_id 对应文件路径渲染为 LSP `file://` URI。
fn path_to_uri(table: &FileTable, file_id: u32) -> String {
    let path = table.find(file_id).map(|e| e.path.as_str()).unwrap_or("");
    let normalized = normalize_path(path);
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

// ============================================================================
// SymbolKind → LSP SymbolKind
// ============================================================================

/// 映射 `arcgr::SymbolKind` 到 LSP `SymbolKind` 数值（[规范](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#symbolKind)）。
pub fn symbol_kind_lsp(kind: SymbolKind) -> i32 {
    match kind {
        SymbolKind::Module => 2,
        SymbolKind::Class => 5,
        SymbolKind::Method => 6,
        SymbolKind::StaticMethod => 6,
        SymbolKind::Property => 7,
        SymbolKind::Field => 8,
        SymbolKind::Enum => 10,
        SymbolKind::Interface => 11,
        SymbolKind::Function => 12,
        SymbolKind::Constant => 14,
        SymbolKind::Variant => 19,
        SymbolKind::Struct => 23,
    }
}

// ============================================================================
// TypeSig 渲染（hover 用，紧凑版）
// ============================================================================

/// 将 `TypeSig` 渲染为简短可读签名（与 `arc` 查询层口径对齐）。
pub fn format_type_sig(sig: &TypeSig) -> String {
    match sig {
        TypeSig::Int => "int".into(),
        TypeSig::Long => "long".into(),
        TypeSig::Float => "float".into(),
        TypeSig::Double => "double".into(),
        TypeSig::Bool => "bool".into(),
        TypeSig::String => "string".into(),
        TypeSig::Unit => "void".into(),
        TypeSig::Null => "null".into(),
        TypeSig::Object => "object".into(),
        TypeSig::UInt => "uint".into(),
        TypeSig::ULong => "ulong".into(),
        TypeSig::UShort => "ushort".into(),
        TypeSig::SByte => "sbyte".into(),
        TypeSig::Named {
            fully_qualified_name,
            generic_args,
        } => {
            if generic_args.is_empty() {
                fully_qualified_name.clone()
            } else {
                let args: Vec<String> = generic_args.iter().map(format_type_sig).collect();
                format!("{}<{}>", fully_qualified_name, args.join(", "))
            }
        }
        TypeSig::Func { params, ret, .. } => {
            let p: Vec<String> = params.iter().map(format_type_sig).collect();
            format!("Func<{} -> {}>", p.join(", "), format_type_sig(ret))
        }
        TypeSig::Method {
            receiver,
            params,
            ret,
            ..
        } => {
            let p: Vec<String> = params.iter().map(format_type_sig).collect();
            format!(
                "{}::Method<({}) -> {}>",
                format_type_sig(receiver),
                p.join(", "),
                format_type_sig(ret)
            )
        }
        TypeSig::Property { prop_type, .. } => {
            format!("Property<{}>", format_type_sig(prop_type))
        }
        TypeSig::GenericParam { param_index } => format!("T{param_index}"),
        TypeSig::Nullable { inner } => format!("{}?", format_type_sig(inner)),
        TypeSig::List { element_type } => format!("List<{}>", format_type_sig(element_type)),
        TypeSig::Array {
            element_type,
            length,
        } => {
            format!("{}[{length}]", format_type_sig(element_type))
        }
        TypeSig::Tuple { elements } => {
            let e: Vec<String> = elements.iter().map(format_type_sig).collect();
            format!("({})", e.join(", "))
        }
        TypeSig::Closure { fn_sig, env_type } => format!(
            "Closure<fn={}, env={}>",
            format_type_sig(fn_sig),
            format_type_sig(env_type)
        ),
        TypeSig::Variant {
            fully_qualified_name,
            cases,
        } => {
            let c: Vec<String> = cases
                .iter()
                .map(|c| format!("{}: {}", c.case_name, format_type_sig(&c.payload_type)))
                .collect();
            format!("Variant {fully_qualified_name} {{ {} }}", c.join(", "))
        }
        TypeSig::TaskHandle { result_type } => format!("Task<{}>", format_type_sig(result_type)),
        TypeSig::Span { element_type } => format!("Span<{}>", format_type_sig(element_type)),
        TypeSig::Expression { delegate_type } => {
            format!("Expression<{}>", format_type_sig(delegate_type))
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use arcgr::{FileEntry, ReferenceContext, ReferenceEntry, ReferenceGraph, Visibility};

    /// 构造含定义与引用的测试索引。
    ///
    /// 注入源码（每行 6 字节 + `\n`，4 行）：
    /// ```text
    /// aaaaaa   line 0  [0..6]
    /// bbbbbb   line 1  [7..13]
    /// cccccc   line 2  [14..20]
    /// dddddd   line 3  [21..27]
    /// ```
    /// 符号 span（均落在对应行内）：
    /// - IFoo(1)     [0,5]    line0
    /// - FooImpl(2)  [7,12]   line1
    /// - Main(3)     [14,19]  line2
    /// - IFoo.Bar(4) [1,5]    line0
    /// - FooImpl.Bar(5) [8,12] line1
    ///
    /// 引用：Main(3) 在 [30,35]（line3 末尾之外，不覆盖任何定义 span）
    fn sample_index() -> SemanticIndex {
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 4));
        let symbols: [(u32, &str, SymbolKind, TypeSig, u32); 5] = [
            (
                1,
                "IFoo",
                SymbolKind::Interface,
                TypeSig::Named {
                    fully_qualified_name: "IFoo".into(),
                    generic_args: vec![],
                },
                0,
            ),
            (
                2,
                "FooImpl",
                SymbolKind::Class,
                TypeSig::Named {
                    fully_qualified_name: "FooImpl".into(),
                    generic_args: vec![],
                },
                7,
            ),
            (
                3,
                "Main",
                SymbolKind::Function,
                TypeSig::Func {
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    captures: false,
                },
                14,
            ),
            (
                4,
                "IFoo.Bar",
                SymbolKind::Method,
                TypeSig::Method {
                    receiver: Box::new(TypeSig::Named {
                        fully_qualified_name: "IFoo".into(),
                        generic_args: vec![],
                    }),
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    is_virtual: true,
                    vtable_slot: 0,
                },
                1,
            ),
            (
                5,
                "FooImpl.Bar",
                SymbolKind::Method,
                TypeSig::Method {
                    receiver: Box::new(TypeSig::Named {
                        fully_qualified_name: "FooImpl".into(),
                        generic_args: vec![],
                    }),
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    is_virtual: false,
                    vtable_slot: 0,
                },
                8,
            ),
        ];
        for (id, name, kind, sig, start) in symbols {
            file.symbol_table.entries.push(SymbolEntry::new(
                id,
                name,
                kind,
                Visibility::Public,
                1,
                start,
                start + 5,
                sig,
                Some(format!("doc for {name}")),
            ));
        }
        // 引用：Main(3) 在 [30,35]（不覆盖任何定义 span，避免解析歧义）
        file.reference_table.entries.push(ReferenceEntry::new(
            1,
            3,
            1,
            30,
            35,
            ReferenceContext::Call,
        ));
        file.reference_graph = ReferenceGraph::default();
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        idx.inject_source(1, "aaaaaa\nbbbbbb\ncccccc\ndddddd\n");
        idx
    }

    #[test]
    fn line_index_round_trip() {
        let src = "abc\ndef\n\nghi";
        let idx = LineIndex::new(src);
        assert_eq!(idx.line_count(), 4);
        let (l, c) = idx.position_of(src, 0);
        assert_eq!((l, c), (0, 0));
        let (l, c) = idx.position_of(src, 3);
        assert_eq!((l, c), (0, 3));
        let (l, c) = idx.position_of(src, 4);
        assert_eq!((l, c), (1, 0));
        let (l, c) = idx.position_of(src, 5);
        assert_eq!((l, c), (1, 1));
        // offset 越界 → 收敛到最后一行末尾
        let (l, c) = idx.position_of(src, 100);
        assert_eq!((l, c), (3, 3));
        // 反向：UTF-16 列 → 字节偏移
        assert_eq!(idx.offset_of(src, 0, 1), Some(1));
        assert_eq!(idx.offset_of(src, 1, 2), Some(6));
        assert_eq!(idx.offset_of(src, 9, 0), None); // 行越界
    }

    #[test]
    fn position_to_offset_is_utf16_aware() {
        // 非 ASCII 源码：验证 semantic 的 LSP 列换算按 UTF-16（而非字节）口径
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/utf.as".into(), 0, 1));
        file.symbol_table.entries.push(SymbolEntry::new(
            1,
            "Bar",
            SymbolKind::Class,
            Visibility::Public,
            1,
            4, // "😀Ba" 中 'B' 的字节偏移 = 4
            7,
            TypeSig::Named {
                fully_qualified_name: "Bar".into(),
                generic_args: vec![],
            },
            None,
        ));
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        // 第 0 行：😀(UTF-16 列 0-1) B(列 2) ...
        idx.inject_source(1, "😀Bar\n");
        // 'B' 的 UTF-16 列 = 2 → 应映射到字节偏移 4
        let off = idx
            .position_to_offset(
                1,
                Position {
                    line: 0,
                    character: 2,
                },
            )
            .expect("should resolve");
        assert_eq!(off, 4);
        // definition 的 selection_range 起始列应为 UTF-16 列 2
        let loc = idx.definition(1).unwrap();
        assert_eq!(loc.range.start.character, 2);
        assert_eq!(loc.range.end.character, 5);
    }

    #[test]
    fn symbol_at_offset_matches_definition_span() {
        let idx = sample_index();
        // FooImpl 定义 span = [7, 12]
        let sym = idx.symbol_at_offset(1, 9).expect("should resolve");
        assert_eq!(sym.symbol_id, 2);
        assert_eq!(sym.name, "FooImpl");
    }

    #[test]
    fn symbol_at_offset_matches_reference_span() {
        let idx = sample_index();
        // 引用 span [30,35] 指向 Main(3)
        let sym = idx
            .symbol_at_offset(1, 32)
            .expect("should resolve via reference");
        assert_eq!(sym.symbol_id, 3);
        assert_eq!(sym.name, "Main");
    }

    #[test]
    fn symbol_at_offset_none_when_no_match() {
        let idx = sample_index();
        // 偏移 40 无任何覆盖
        assert!(idx.symbol_at_offset(1, 40).is_none());
    }

    #[test]
    fn definition_returns_uri_and_range() {
        let mut idx = sample_index();
        // Main 定义 span [14,19] → 行 2 列 0 到行 2 列 5
        let loc = idx.definition(3).expect("definition");
        assert_eq!(loc.uri, "file:///proj/src/main.as");
        assert_eq!(
            loc.range.start,
            Position {
                line: 2,
                character: 0
            }
        );
        assert_eq!(
            loc.range.end,
            Position {
                line: 2,
                character: 5
            }
        );
    }

    #[test]
    fn references_includes_definition_and_usage() {
        let mut idx = sample_index();
        let refs = idx.references(3);
        // 定义 1 + 引用 1
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|l| l.uri == "file:///proj/src/main.as"));
    }

    #[test]
    fn hover_contains_name_and_signature() {
        let mut idx = sample_index();
        let h = idx.hover(3).expect("hover");
        assert!(h.contents.value.contains("Main"));
        assert!(h.contents.value.contains("void"));
        assert_eq!(h.contents.kind, "markdown");
    }

    #[test]
    fn document_symbols_lists_file_symbols() {
        let mut idx = sample_index();
        let symbols = idx.document_symbols(1);
        assert_eq!(symbols.len(), 5);
        assert!(symbols.iter().any(|s| s.name == "Main"));
        assert!(symbols.iter().any(|s| s.name == "FooImpl"));
    }

    #[test]
    fn workspace_symbols_filters_public_by_query() {
        // sample_index 全部符号为 Public → 空查询返回全部
        let mut idx = sample_index();
        let all = idx.workspace_symbols(None);
        assert_eq!(all.len(), 5);

        // 子串匹配（大小写不敏感）
        let foo = idx.workspace_symbols(Some("foo"));
        assert!(foo.iter().any(|s| s.name == "FooImpl"));
        assert!(foo.iter().any(|s| s.name == "IFoo"));
        assert!(!foo.iter().any(|s| s.name == "Main"));

        // 位置与 URI 正确
        let m = idx.workspace_symbols(Some("Main"));
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].location.uri, "file:///proj/src/main.as");
        assert_eq!(m[0].kind, symbol_kind_lsp(SymbolKind::Function));
    }

    #[test]
    fn workspace_symbols_excludes_non_public() {
        // 一个非 Public 符号不应出现在跨包查询结果中
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/priv.as".into(), 0, 1));
        file.symbol_table.entries.push(SymbolEntry::new(
            1,
            "Visible",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            7,
            TypeSig::Named {
                fully_qualified_name: "Visible".into(),
                generic_args: vec![],
            },
            None,
        ));
        file.symbol_table.entries.push(SymbolEntry::new(
            2,
            "Hidden",
            SymbolKind::Class,
            Visibility::Private,
            1,
            8,
            14,
            TypeSig::Named {
                fully_qualified_name: "Hidden".into(),
                generic_args: vec![],
            },
            None,
        ));
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        idx.inject_source(1, "Visible\nHidden\n");
        let all = idx.workspace_symbols(None);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Visible");
    }

    #[test]
    fn external_ref_name_at_offset_detects_external_reference() {
        // 引用条目指向 symbol_id 99（本包不存在）→ 外部引用 → 返回标识符文本
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 1));
        file.reference_table.entries.push(ReferenceEntry::new(
            1,
            99, // 外部目标（本包无此符号）
            1,
            0,
            12, // "ExternalType" 的 span
            ReferenceContext::TypeAnnotation,
        ));
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        idx.inject_source(1, "ExternalType\n");
        assert_eq!(
            idx.external_ref_name_at_offset(1, 1),
            Some("ExternalType".to_string())
        );
        // 偏移不在引用 span 内 → None
        assert!(idx.external_ref_name_at_offset(1, 40).is_none());
    }

    #[test]
    fn external_ref_name_at_offset_none_for_local_reference() {
        // 本地可解析引用（symbol_id 存在）→ 非外部引用 → None
        let mut file = ArcgrFile::new();
        file.file_table
            .entries
            .push(FileEntry::new(1, "/proj/src/main.as".into(), 0, 1));
        file.symbol_table.entries.push(SymbolEntry::new(
            5,
            "Local",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            5,
            TypeSig::Named {
                fully_qualified_name: "Local".into(),
                generic_args: vec![],
            },
            None,
        ));
        file.reference_table.entries.push(ReferenceEntry::new(
            1,
            5, // 本地目标
            1,
            0,
            5,
            ReferenceContext::TypeAnnotation,
        ));
        let mut idx = SemanticIndex::from_arcgr(file, PathBuf::from("/proj"));
        idx.inject_source(1, "Local\n");
        assert!(idx.external_ref_name_at_offset(1, 1).is_none());
    }

    #[test]
    fn set_source_text_overrides_and_invalidate_restores() {
        let mut idx = sample_index();
        // 注入 1 行文本模拟磁盘旧内容
        idx.inject_source(1, "Old\n");
        assert!(idx
            .position_to_offset(
                1,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_some());
        // 1 行文本 → line 2 越界 → None
        assert!(idx
            .position_to_offset(
                1,
                Position {
                    line: 2,
                    character: 0
                }
            )
            .is_none());

        // 开放文档覆盖为 3 行 → line 2 可解析（offset 4 = "C"）
        idx.set_source_text(1, "A\nB\nC\n");
        assert_eq!(
            idx.position_to_offset(
                1,
                Position {
                    line: 2,
                    character: 0
                }
            ),
            Some(4)
        );

        // 失效覆盖 → 回落到磁盘（此处无真实文件 → 读取失败 → None）
        idx.invalidate_source(1);
        assert!(idx
            .position_to_offset(
                1,
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_none());
    }

    #[test]
    fn file_id_for_uri_matches() {
        let idx = sample_index();
        assert_eq!(idx.file_id_for_uri("file:///proj/src/main.as"), Some(1));
        // 带尾部斜杠/反斜杠差异的归一化匹配
        assert_eq!(idx.file_id_for_uri("file:///proj\\src\\main.as"), Some(1));
        assert_eq!(idx.file_id_for_uri("file:///elsewhere.as"), None);
    }

    #[test]
    fn symbol_kind_mapping_covers_all() {
        assert_eq!(symbol_kind_lsp(SymbolKind::Class), 5);
        assert_eq!(symbol_kind_lsp(SymbolKind::Function), 12);
        assert_eq!(symbol_kind_lsp(SymbolKind::Method), 6);
        assert_eq!(symbol_kind_lsp(SymbolKind::Interface), 11);
        assert_eq!(symbol_kind_lsp(SymbolKind::Struct), 23);
        assert_eq!(symbol_kind_lsp(SymbolKind::Enum), 10);
        assert_eq!(symbol_kind_lsp(SymbolKind::Field), 8);
        assert_eq!(symbol_kind_lsp(SymbolKind::Constant), 14);
    }

    #[test]
    fn format_type_sig_renders_compound() {
        let sig = TypeSig::List {
            element_type: Box::new(TypeSig::Named {
                fully_qualified_name: "Foo".into(),
                generic_args: vec![TypeSig::Int, TypeSig::String],
            }),
        };
        assert_eq!(format_type_sig(&sig), "List<Foo<int, string>>");
    }
}
