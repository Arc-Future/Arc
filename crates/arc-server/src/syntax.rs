//! 语法服务层（Roslyn 级 SyntaxTree 基础）。
//!
//! ## 定位
//!
//! 与 [`super::semantic`]（消费 `.arcgr` 语义索引）互补，本模块**直接解析源码文本**，
//! 提供不依赖语义分析的纯语法能力：
//!
//! - [`SyntaxTree`]：源码 + UTF-16 行索引 + token 流 + 首个语法诊断
//! - [`SyntaxTree::folding_ranges`]：基于花括号配对的折叠区间
//! - [`SyntaxTree::semantic_tokens`]：token → 语义高亮 token（LSP semanticTokens/full）
//! - [`SyntaxTree::diagnostic`]：词法/语法首个错误 → LSP Diagnostic
//! - [`TextDocument`]：打开文档的有状态封装，支持 didOpen/didChange/didClose
//!
//! ## 设计要点
//!
//! - **列口径**：统一采用 **UTF-16 code unit**（LSP 规范口径）。与 [`super::semantic`] 的
//!   字节列（面向 `.arcgr` 字节 span、ASCII 主导假设）不同——语法服务处理开放文档的
//!   任意编辑，必须按 LSP 口径精确换算位置。
//! - **复用编译器 `parse` crate**：token 流来自 `parse::lex`，语法诊断来自 `parse::Parser`；
//!   不重写词法/语法，不拖入编译器核心（parse 仅依赖 ast+logos）。
//! - **已知边界**：`parse` 为 fail-fast（只报首个错误，非 Roslyn 式错误恢复多诊断）；
//!   全量重解析（无增量 reparse）。这些在后续里程碑演进。
//!
//! ## 当前局限
//!
//! lexer 丢弃空白与普通注释（trivia 不保留），故折叠仅覆盖花括号块、高亮不含注释间隙。

use serde::{Deserialize, Serialize};

use crate::lines::LineIndex;
use crate::semantic::{Position, Range};

use parse::{lex, Parser, SpannedToken, Token};

// ============================================================================
// LSP 结果类型
// ============================================================================

/// LSP 折叠区间（`startLine`/`startCharacter`/`endLine`/`endCharacter`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldingRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// LSP 语义 token（未编码——由 [`encode_semantic_tokens`] 转为 delta 格式 data）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: u32,
    pub start_character: u32,
    pub length: u32,
    pub type_index: u32,
}

/// LSP 诊断条目（`textDocument/diagnostic` 返回）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    pub range: Range,
    /// LSP 严重度：1=Error。
    pub severity: u8,
    pub source: String,
    pub message: String,
}

/// 语义 token 图例（`semanticTokensProvider.legend.tokenTypes`）。
///
/// 下标与 [`token_type_index`] 的返回值一一对应，声明顺序即索引。
pub const SEMANTIC_TOKEN_TYPES: [&str; 7] = [
    "keyword",
    "identifier",
    "number",
    "string",
    "comment",
    "operator",
    "punctuation",
];

// ============================================================================
// SyntaxTree
// ============================================================================

/// 内部语法诊断——持字节区间，查询时经 [`LineIndex`] 转 LSP `Range`。
#[derive(Debug, Clone)]
struct SyntaxDiagnostic {
    message: String,
    start: u32,
    end: u32,
}

/// 语法树（Roslyn 式 SyntaxTree 基础）。
///
/// 一次性解析：源码 + token 流 + 首个语法诊断。纯语法、无语义分析。
#[derive(Debug, Clone)]
pub struct SyntaxTree {
    text: String,
    index: LineIndex,
    tokens: Vec<SpannedToken>,
    diagnostic: Option<SyntaxDiagnostic>,
}

impl SyntaxTree {
    /// 解析源码文本，产出 token 流与首个语法诊断。
    pub fn parse(text: &str) -> Self {
        let index = LineIndex::new(text);
        // token 流用于折叠/高亮；lex 失败（非法字符）时为空。
        let tokens = lex(text, 0).unwrap_or_default();
        // 语法诊断：`parse_program_in_file` 内部重新 lex，报首个词法/语法错误。
        let diagnostic = match Parser::parse_program_in_file(text, 0) {
            Ok(_) => None,
            Err(e) => Some(SyntaxDiagnostic::from_parse_error(&e, text.len())),
        };
        Self {
            text: text.to_string(),
            index,
            tokens,
            diagnostic,
        }
    }

    /// 折叠区间——基于 `{`/`}` 配对，跨行才折叠。
    pub fn folding_ranges(&self) -> Vec<FoldingRange> {
        let mut stack: Vec<u32> = Vec::new();
        let mut out = Vec::new();
        for t in &self.tokens {
            match t.token {
                Token::LBrace => stack.push(t.span.start),
                Token::RBrace => {
                    if let Some(start) = stack.pop() {
                        let (sl, _) = self.index.position_of(&self.text, start as usize);
                        let (el, _) = self.index.position_of(&self.text, t.span.end as usize);
                        if el > sl {
                            out.push(FoldingRange {
                                start_line: sl,
                                start_character: 0,
                                end_line: el,
                                end_character: 0,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// 语义 token 流（未编码）。token 按源码顺序给出 UTF-16 位置与长度。
    pub fn semantic_tokens(&self) -> Vec<SemanticToken> {
        self.tokens
            .iter()
            .map(|t| {
                let start = t.span.start as usize;
                let end = t.span.end as usize;
                let (line, ch) = self.index.position_of(&self.text, start);
                let length = self.index.utf16_len(&self.text, start, end);
                SemanticToken {
                    line,
                    start_character: ch,
                    length,
                    type_index: token_type_index(&t.token),
                }
            })
            .collect()
    }

    /// 首个语法诊断（词法/语法错误）；无错误返回 `None`。
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        let d = self.diagnostic.as_ref()?;
        let (sl, sc) = self.index.position_of(&self.text, d.start as usize);
        let (el, ec) = self.index.position_of(&self.text, d.end as usize);
        Some(Diagnostic {
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
            severity: 1,
            source: "arc".into(),
            message: d.message.clone(),
        })
    }
}

impl SyntaxDiagnostic {
    fn from_parse_error(e: &parse::ParseError, text_len: usize) -> Self {
        match e {
            parse::ParseError::Unexpected {
                span,
                expected,
                found,
            } => Self {
                message: format!("unexpected `{found}`, expected {expected}"),
                start: span.start,
                end: span.end.max(span.start),
            },
            parse::ParseError::Eof => Self {
                message: "unexpected end of file".into(),
                start: text_len as u32,
                end: text_len as u32,
            },
        }
    }
}

/// token → 语义 token 类型下标（见 [`SEMANTIC_TOKEN_TYPES`]）。
fn token_type_index(t: &Token) -> u32 {
    match t {
        // 关键字
        Token::Namespace
        | Token::Using
        | Token::Global
        | Token::Struct
        | Token::Class
        | Token::Record
        | Token::With
        | Token::Interface
        | Token::Enum
        | Token::Variant
        | Token::Async
        | Token::Await
        | Token::From
        | Token::Where
        | Token::Select
        | Token::OrderBy
        | Token::Join
        | Token::On
        | Token::Group
        | Token::By
        | Token::Into
        | Token::Let
        | Token::Var
        | Token::If
        | Token::Else
        | Token::While
        | Token::For
        | Token::Foreach
        | Token::In
        | Token::Return
        | Token::Switch
        | Token::Case
        | Token::Default
        | Token::Break
        | Token::Continue
        | Token::Throw
        | Token::Try
        | Token::Catch
        | Token::Finally
        | Token::Lock
        | Token::Public
        | Token::Private
        | Token::Internal
        | Token::Protected
        | Token::Void
        | Token::Float
        | Token::Double
        | Token::Long
        | Token::Short
        | Token::Byte
        | Token::Char
        | Token::UInt
        | Token::ULong
        | Token::UShort
        | Token::SByte
        | Token::True
        | Token::False
        | Token::New
        | Token::Virtual
        | Token::Override
        | Token::Abstract
        | Token::Static
        | Token::Operator
        | Token::Const
        | Token::Readonly
        | Token::Ref
        | Token::Out
        | Token::Params
        | Token::This
        | Token::Base
        | Token::Descending
        | Token::Null
        | Token::TypeOf
        | Token::NameOf
        | Token::Is
        | Token::Comptime
        | Token::When
        | Token::Delegate => 0,

        Token::Ident(_) => 1,
        Token::IntLit(_) | Token::FloatLit(_) => 2,
        Token::StringLit(_)
        | Token::VerbatimString(_)
        | Token::InterpolatedString(_)
        | Token::VerbatimInterpolatedString(_)
        | Token::CharLit(_) => 3,
        Token::DocComment(_) => 4,

        // 运算符
        Token::Shl
        | Token::Shr
        | Token::Lt
        | Token::Gt
        | Token::FatArrow
        | Token::Arrow
        | Token::Eq
        | Token::PlusPlus
        | Token::MinusMinus
        | Token::PlusEq
        | Token::MinusEq
        | Token::StarEq
        | Token::SlashEq
        | Token::BitOrEq
        | Token::BitAndEq
        | Token::BitXorEq
        | Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::Bang
        | Token::BitOr
        | Token::BitAnd
        | Token::AndAnd
        | Token::OrOr
        | Token::BitXor
        | Token::Tilde
        | Token::EqEq
        | Token::NotEq
        | Token::Le
        | Token::Ge
        | Token::NullCoalesceEq
        | Token::NullCoalesce
        | Token::QuestionDot
        | Token::BangDot
        | Token::Question => 5,

        // 标点/分隔符
        Token::Semi
        | Token::Comma
        | Token::Dot
        | Token::DotDot
        | Token::Colon
        | Token::LParen
        | Token::RParen
        | Token::LBrace
        | Token::RBrace
        | Token::LBracket
        | Token::RBracket => 6,

        // LineComment 已被 lexer 过滤，不应出现在流中
        Token::LineComment => 6,
    }
}

/// 将语义 token 流编码为 LSP `SemanticTokens.data`（相对 delta 格式）。
pub fn encode_semantic_tokens(tokens: &[SemanticToken]) -> Vec<u32> {
    let mut out = Vec::with_capacity(tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_char = 0u32;
    for t in tokens {
        if t.line == prev_line {
            out.push(0);
            out.push(t.start_character.saturating_sub(prev_char));
        } else {
            out.push(t.line - prev_line);
            out.push(t.start_character);
        }
        out.push(t.length);
        out.push(t.type_index);
        out.push(0); // tokenModifiers
        prev_line = t.line;
        prev_char = t.start_character;
    }
    out
}

// ============================================================================
// 文本文档（didOpen/didChange/didClose）
// ============================================================================

/// LSP `textDocument/didChange` 的单条内容变更。
#[derive(Debug, Clone, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    /// 变更区间；`None` 表示整文档替换（full sync）。
    pub range: Option<Range>,
    /// 替换文本。
    pub text: String,
}

/// 打开的文本文档——维护源码缓冲与**惰性**语法树。
///
/// ## 高效 Change 处理（惰性投机式重解析）
///
/// `didChange` 只更新源码缓冲并**标记脏**（`tree = None`），不立即解析；[`TextDocument::tree`]
/// 在首次 provider 查询时才按需重解析一次。效果：
///
/// - 快速连续键入：多次 `didChange` 合并为**单次**重解析；
/// - 变更后无查询：完全**跳过**重解析（省去无谓 CPU）。
///
/// 行索引在每次变更批处理后重建一次（O(文本长)），同样按需延后到查询。
#[derive(Debug, Clone)]
pub struct TextDocument {
    pub uri: String,
    pub language_id: String,
    version: i32,
    text: String,
    index: LineIndex,
    /// 惰性语法树：`None` 表示自上次构建以来已发生变更（脏）。
    tree: Option<SyntaxTree>,
}

impl TextDocument {
    /// 打开文档（didOpen）——构建行索引，语法树留待首次查询。
    pub fn open(
        uri: impl Into<String>,
        language_id: impl Into<String>,
        version: i32,
        text: &str,
    ) -> Self {
        Self {
            uri: uri.into(),
            language_id: language_id.into(),
            version,
            text: text.to_string(),
            index: LineIndex::new(text),
            tree: None,
        }
    }

    /// 当前版本号。
    pub fn version(&self) -> i32 {
        self.version
    }

    /// 当前源码文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 应用一组内容变更（按序），并**标记脏**——不在此处解析。
    ///
    /// 惰性投机式：仅更新文本缓冲 + 重建行索引，语法树留待 [`Self::tree`] 按需重解析。
    pub fn apply_changes(&mut self, changes: &[TextDocumentContentChangeEvent], version: i32) {
        for change in changes {
            match &change.range {
                Some(range) => {
                    // 增量：UTF-16 位置 → 字节偏移，替换区间
                    if let (Some(start), Some(end)) = (
                        self.index
                            .offset_of(&self.text, range.start.line, range.start.character),
                        self.index
                            .offset_of(&self.text, range.end.line, range.end.character),
                    ) {
                        if start <= end && end <= self.text.len() {
                            self.text.replace_range(start..end, &change.text);
                        }
                    }
                }
                None => {
                    // 整文档替换
                    self.text = change.text.clone();
                }
            }
        }
        self.version = version;
        // 重建行索引（反映变更后的行列）并置脏——解析延迟到首次查询。
        self.index = LineIndex::new(&self.text);
        self.tree = None;
    }

    /// 获取语法树——脏时按当前文本重解析一次（惰性投机式）。
    pub fn tree(&mut self) -> &SyntaxTree {
        if self.tree.is_none() {
            self.tree = Some(SyntaxTree::parse(&self.text));
        }
        self.tree.as_ref().expect("tree just built")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_tree_parses_tokens() {
        let tree = SyntaxTree::parse("namespace A { void F() { } }");
        assert!(!tree.tokens.is_empty());
        assert!(tree.diagnostic().is_none());
    }

    #[test]
    fn syntax_tree_reports_parse_error() {
        let tree = SyntaxTree::parse("class Foo {");
        let diag = tree.diagnostic().expect("must have diagnostic");
        assert_eq!(diag.source, "arc");
        assert_eq!(diag.severity, 1);
        assert!(!diag.message.is_empty());
    }

    #[test]
    fn folding_ranges_on_braces() {
        let tree = SyntaxTree::parse("class A {\n  void F() {\n  }\n}\n");
        let folds = tree.folding_ranges();
        // 两个跨行折叠：F 的方法体 + A 的类体
        assert_eq!(folds.len(), 2);
        // 类体折叠：从第 0 行到第 3 行
        assert!(folds.iter().any(|f| f.start_line == 0 && f.end_line == 3));
        // 方法体折叠：从第 1 行到第 2 行
        assert!(folds.iter().any(|f| f.start_line == 1 && f.end_line == 2));
    }

    #[test]
    fn semantic_tokens_classify_keyword_and_ident() {
        let tree = SyntaxTree::parse("namespace A;");
        let tokens = tree.semantic_tokens();
        // namespace=keyword(0), A=identifier(1), ;=punctuation(6)
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].type_index, 0);
        assert_eq!(tokens[1].type_index, 1);
        assert_eq!(tokens[2].type_index, 6);
    }

    #[test]
    fn encode_semantic_tokens_delta() {
        // 同 0 行两个 token：(0,0,len2,type0) 与 (0,2,len1,type1)
        let tokens = vec![
            SemanticToken {
                line: 0,
                start_character: 0,
                length: 2,
                type_index: 0,
            },
            SemanticToken {
                line: 0,
                start_character: 2,
                length: 1,
                type_index: 1,
            },
        ];
        let data = encode_semantic_tokens(&tokens);
        assert_eq!(data, vec![0, 0, 2, 0, 0, 0, 2, 1, 1, 0]);
    }

    #[test]
    fn text_document_incremental_change() {
        let mut doc = TextDocument::open("uri", "arc", 1, "class A {}\n");
        // 在 "class A" 后插入 "B"（第 0 行第 7 列）→ "class AB {}"
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 7,
                },
                end: Position {
                    line: 0,
                    character: 7,
                },
            }),
            text: "B".into(),
        };
        doc.apply_changes(&[change], 2);
        assert_eq!(doc.text(), "class AB {}\n");
        assert_eq!(doc.version(), 2);
        // 惰性：变更后未查询，树为空；首次 tree() 触发重解析
        assert!(doc.tree.is_none(), "tree must remain dirty after change");
        assert_eq!(doc.tree().folding_ranges().len(), 0);
    }

    #[test]
    fn text_document_full_replace() {
        let mut doc = TextDocument::open("uri", "arc", 1, "old");
        let change = TextDocumentContentChangeEvent {
            range: None,
            text: "new".into(),
        };
        doc.apply_changes(&[change], 3);
        assert_eq!(doc.text(), "new");
        assert_eq!(doc.version(), 3);
    }

    #[test]
    fn text_document_lazy_reparse_on_demand() {
        let mut doc = TextDocument::open("uri", "arc", 1, "class A {\n  void F() {\n  }\n}\n");
        // 打开时未解析：树为脏（惰性）
        assert!(doc.tree.is_none(), "open must be lazy (no parse)");
        // 首次查询触发一次解析
        assert_eq!(doc.tree().folding_ranges().len(), 2);
        // 同一版本内复用：树已构建，不重复解析
        assert!(doc.tree().folding_ranges().len() == 2);
        // 变更后置脏；再查询用新文本重解析
        let change = TextDocumentContentChangeEvent {
            range: Some(Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 3,
                    character: 0,
                },
            }),
            text: "class B { }\n".into(),
        };
        doc.apply_changes(&[change], 2);
        assert!(doc.tree.is_none(), "tree must be dirty after change");
        assert_eq!(doc.tree().folding_ranges().len(), 0);
    }
}
