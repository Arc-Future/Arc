use ast::{FileId, Span};
use logos::Logos;

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/")]
pub enum Token {
    // Keywords
    #[token("namespace")]
    Namespace,
    #[token("using")]
    Using,
    #[token("global")] // RFC 003：`global using`
    Global,
    #[token("struct")]
    Struct,
    #[token("class")]
    Class,
    /// RFC 006：`record` / `record struct` 引用类型声明（`record class` 已硬拒，RFC 002）。
    #[token("record")]
    Record,
    /// RFC 006 M2：`expr with { … }` 后缀关键字。
    #[token("with")]
    With,
    #[token("interface")]
    Interface,
    #[token("enum")]
    Enum,
    /// RFC 004 M1：`variant` 关键字——标签联合类型声明。
    #[token("variant")]
    Variant,
    #[token("async")]
    Async,
    #[token("await")]
    Await,
    #[token("from")]
    From,
    #[token("where")]
    Where,
    #[token("select")]
    Select,
    #[token("orderby")]
    OrderBy,
    #[token("join")]
    Join,
    #[token("on")]
    On,
    #[token("group")]
    Group,
    #[token("by")]
    By,
    #[token("into")]
    Into,
    #[token("let")]
    Let,
    #[token("var")]
    Var,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("for")]
    For,
    #[token("foreach")]
    Foreach,
    #[token("in")]
    In,
    #[token("return")]
    Return,
    #[token("switch")]
    Switch,
    #[token("case")]
    Case,
    #[token("default")]
    Default,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("throw")]
    Throw,
    #[token("try")]
    Try,
    #[token("catch")]
    Catch,
    #[token("finally")]
    Finally,
    /// RFC 009 §7.3：`lock (expr) { }` 语句糖关键字。
    #[token("lock")]
    Lock,
    #[token("public")]
    Public,
    #[token("private")]
    Private,
    #[token("internal")]
    Internal,
    #[token("protected")]
    Protected,
    #[token("void")]
    Void,
    #[token("float")]
    Float,
    #[token("double")]
    Double,
    #[token("long")]
    Long,
    #[token("short")]
    Short,
    #[token("byte")]
    Byte,
    #[token("char")]
    Char,
    #[token("uint")]
    UInt,
    #[token("ulong")]
    ULong,
    #[token("ushort")]
    UShort,
    #[token("sbyte")]
    SByte,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("new")]
    New,
    // RFC 036 D9.1: `as` 关键字已剔除——强制转换统一使用 `(T)x` 语法。
    // 原 `Token::As` 已移除；现有代码中的 `x as T` 需改为 `(T)x`。
    #[token("virtual")]
    Virtual,
    #[token("override")]
    Override,
    #[token("abstract")]
    Abstract,
    #[token("static")]
    Static,
    /// RFC 003：用户运算符重载声明关键字 `operator +` / `operator ==` 等。
    #[token("operator")]
    Operator,
    #[token("const")]
    Const,
    /// RFC 012：comptime 有限子集——编译期常量求值关键字（仅声明/表达式前缀）。
    #[token("comptime")]
    Comptime,
    #[token("readonly")]
    Readonly,
    #[token("ref")]
    Ref,
    #[token("out")]
    Out,
    /// RFC 005：`params ReadOnlySpan<T>` / `params Span<T>`。
    #[token("params")]
    Params,
    #[token("this")]
    This,
    #[token("base")]
    Base,
    #[token("descending")]
    Descending,
    #[token("null")]
    Null,
    #[token("typeof")]
    TypeOf,
    /// RFC 037 M1.1: `nameof` 内置运算符——编译期解析符号名为字符串。
    /// Parser desugar 为 `Expr::StringLit(name)`，避免新增 AST 节点 /
    /// typeck / MIR / codegen 改动。语义：`nameof(Title)` → "Title"。
    #[token("nameof")]
    NameOf,
    #[token("is")] // RFC 036 M1: `is` 表达式（类型测试 + 模式匹配）
    Is,
    #[token("when")] // RFC 036 M2: `case pat when cond:` / switch 表达式守卫
    When,
    #[token("delegate")]
    Delegate,

    /// C# `///` 文档注释内容（RFC 017）。不含前导 `///` 与首尾空白。
    /// 必须在 LineComment 之前声明——logos 最长匹配相同时按声明顺序，
    /// `///` 同时匹配两条规则（长度相同），DocComment 先声明优先。
    #[regex(r"///[^\n]*", |lex| lex.slice()[3..].trim().to_string())]
    DocComment(String),

    /// 普通单行注释——由 `lex` 过滤丢弃（logos 0.15 的 `None` callback 不会
    /// 跳过整段匹配，故改为产出 token 后在 `lex` 中过滤）。
    #[regex(r"//[^\n]*")]
    LineComment,

    // Types / literals
    // 支持 C# 风格十六进制（`0x0800`）/ 二进制（`0b1010`）前缀字面量。
    #[regex(r"0[xX][0-9a-fA-F]+|0[bB][01]+|[0-9]+", parse_int_lit)]
    IntLit(i64),
    #[regex(r"[0-9]+\.[0-9]+[fFdD]?", parse_float_lit)]
    FloatLit(ast::FloatLitValue),
    #[regex(r#""([^"\\]|\\.)*""#, parse_string)]
    StringLit(String),
    /// RFC 007 M2h：`$@"..."/`@$"..."` verbatim 插值（须先于 `$"` / `@"`, logos 最长匹配）。
    #[regex(r#"\$@""#, parse_verbatim_interpolated_string)]
    #[regex(r#"@\$""#, parse_verbatim_interpolated_string)]
    VerbatimInterpolatedString(String),
    /// RFC 007 M2i：非插值 `@"..."` verbatim（`""` → `"`；`\` 字面；可多行）。
    #[regex(r#"@""#, parse_verbatim_string)]
    VerbatimString(String),
    /// RFC 007：`$"..."` — 回调按花括号深度扫描，洞内可含 `"..."` / 嵌套 `$"..."`。
    #[regex(r#"\$""#, parse_interpolated_string)]
    InterpolatedString(String),
    #[regex(r"'([^'\\]|\\.)'", parse_char)]
    CharLit(char),
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Ident(String),

    // Delimiters
    #[token(";")]
    Semi,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    /// RFC 017 #8：集合表达式 spread `..x`（logos 最长匹配优先于单个 `.`）。
    #[token("..")]
    DotDot,
    #[token(":")]
    Colon,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("<<")]
    Shl,
    /// `>>` 必须在 `>` 之前：logos 最长匹配优先
    #[token(">>")]
    Shr,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("=>")]
    FatArrow,
    #[token("->")]
    Arrow,
    #[token("=")]
    Eq,
    /// `++` postfix/prefix increment (logos longest-match: `++` before `+`).
    #[token("++")]
    PlusPlus,
    /// `--` postfix/prefix decrement (logos longest-match: `--` before `-`).
    #[token("--")]
    MinusMinus,
    /// RFC 003：内置复合赋值（logos 最长匹配优先于 `+`/`=`）。
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("!")]
    Bang,
    /// `|=` 位或复合赋值（枚举 Flags 组合，RFC 004 枚举能力增强）。
    /// 必须声明在 `BitOr`（`|`）之前——logos 最长匹配优先。
    #[token("|=")]
    BitOrEq,
    /// RFC 012 M3：位运算 OR `|`（用于属性参数常量折叠，如
    /// `AttributeTargets.Class | AttributeTargets.Struct`）。
    /// 必须声明在 `OrOr`（`||`）之前——logos 最长匹配优先，`||` 不会被
    /// 误识别为两个 `|`。
    #[token("|")]
    BitOr,
    /// `&` 位与，必须在 `&&` 之前以保证 logos 最长匹配
    #[token("&")]
    BitAnd,
    /// `&=` 位与复合赋值（枚举 Flags 组合，RFC 004 枚举能力增强）。
    /// 必须声明在 `BitAnd`（`&`）之前——logos 最长匹配优先。
    #[token("&=")]
    BitAndEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    /// `^` 位异或，必须在 `^=` 之前
    #[token("^=")]
    BitXorEq,
    #[token("^")]
    BitXor,
    /// `~` — 位取反（单目）。
    #[token("~")]
    Tilde,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Le,
    #[token(">=")]
    Ge,
    #[token("??=")]
    NullCoalesceEq,
    #[token("??")]
    NullCoalesce,
    #[token("?.")]
    QuestionDot,
    #[token("!.")]
    BangDot,
    #[token("?")]
    Question,
}

fn parse_string(lex: &mut logos::Lexer<'_, Token>) -> Option<String> {
    let s = lex.slice();
    Some(unescape_string(&s[1..s.len() - 1]))
}

fn parse_int_lit(lex: &mut logos::Lexer<'_, Token>) -> Option<i64> {
    let s = lex.slice();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        i64::from_str_radix(bin, 2).ok()
    } else {
        s.parse().ok()
    }
}

fn parse_float_lit(lex: &mut logos::Lexer<'_, Token>) -> Option<ast::FloatLitValue> {
    let s = lex.slice();
    let last = s.chars().last()?;
    match last {
        'f' | 'F' => {
            let num = &s[..s.len() - 1];
            num.parse::<f32>().ok().map(ast::FloatLitValue::Float)
        }
        'd' | 'D' => {
            let num = &s[..s.len() - 1];
            num.parse::<f64>().ok().map(ast::FloatLitValue::Double)
        }
        _ => s.parse::<f64>().ok().map(ast::FloatLitValue::Double),
    }
}

/// RFC 007：匹配 `$"` 后，按花括号深度消费至闭合 `"`，返回 **未 unescape** 的 interior
///（字面段转义与 `{{`/`}}` 由 parser 处理；洞内容保持源码原样以便二次解析）。
fn parse_interpolated_string(lex: &mut logos::Lexer<'_, Token>) -> Option<String> {
    scan_interpolated_remainder(lex, false)
}

/// RFC 007 M2h：匹配 `$@"` / `@$"` 后扫描；字面区 `""` → 引号，无 `\` 转义。
fn parse_verbatim_interpolated_string(lex: &mut logos::Lexer<'_, Token>) -> Option<String> {
    scan_interpolated_remainder(lex, true)
}

/// RFC 007 M2i：匹配 `@"` 后扫描；`""` → `"`；`\` 不转义；可多行。
fn parse_verbatim_string(lex: &mut logos::Lexer<'_, Token>) -> Option<String> {
    let rem = lex.remainder();
    let bytes = rem.as_bytes();
    let mut i = 0usize;
    let mut out = String::new();
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                out.push('"');
                i += 2;
            } else {
                lex.bump(i + 1);
                return Some(out);
            }
        } else {
            // UTF-8 安全：按字节复制非 ASCII 亦可（源码为 UTF-8）
            let ch = rem[i..].chars().next()?;
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    None
}

fn scan_interpolated_remainder(
    lex: &mut logos::Lexer<'_, Token>,
    verbatim: bool,
) -> Option<String> {
    let rem = lex.remainder();
    let end = scan_interp_body(rem.as_bytes(), 0, verbatim)?;
    let interior = rem[..end].to_string();
    lex.bump(end + 1);
    Some(interior)
}

/// 从 `bytes[start]` 扫描至闭合 `"`（不含该引号），返回闭合引号下标；失败 → `None`。
fn scan_interp_body(bytes: &[u8], start: usize, verbatim: bool) -> Option<usize> {
    let mut i = start;
    let mut depth = 0usize;
    while i < bytes.len() {
        if depth == 0 {
            match bytes[i] {
                b'"' => {
                    if verbatim && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                    } else {
                        return Some(i);
                    }
                }
                b'\\' if !verbatim => {
                    i += 2;
                    if i > bytes.len() {
                        return None;
                    }
                }
                b'{' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        i += 2;
                    } else {
                        depth = 1;
                        i += 1;
                    }
                }
                b'}' => {
                    if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                        i += 2;
                    } else {
                        return None;
                    }
                }
                _ => i += 1,
            }
        } else {
            match bytes[i] {
                b'"' => {
                    i = skip_regular_string(bytes, i + 1)?;
                }
                b'\'' => {
                    i = skip_char_or_string(bytes, i + 1, b'\'')?;
                }
                b'@' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                    // 洞内普通 verbatim `@"..."`（M2i 亦为独立 token；洞内仍须跳过）
                    i = skip_verbatim_string(bytes, i + 2)?;
                }
                b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                    i = scan_interp_body(bytes, i + 2, false)? + 1;
                }
                b'$' if i + 2 < bytes.len() && bytes[i + 1] == b'@' && bytes[i + 2] == b'"' => {
                    i = scan_interp_body(bytes, i + 3, true)? + 1;
                }
                b'@' if i + 2 < bytes.len() && bytes[i + 1] == b'$' && bytes[i + 2] == b'"' => {
                    i = scan_interp_body(bytes, i + 3, true)? + 1;
                }
                b'{' => {
                    depth += 1;
                    i += 1;
                }
                b'}' => {
                    depth -= 1;
                    i += 1;
                }
                _ => i += 1,
            }
        }
    }
    None
}

fn skip_regular_string(bytes: &[u8], i: usize) -> Option<usize> {
    skip_char_or_string(bytes, i, b'"')
}

fn skip_char_or_string(bytes: &[u8], mut i: usize, quote: u8) -> Option<usize> {
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn skip_verbatim_string(bytes: &[u8], mut i: usize) -> Option<usize> {
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                i += 2;
            } else {
                return Some(i + 1);
            }
        } else {
            i += 1;
        }
    }
    None
}

fn parse_char(lex: &mut logos::Lexer<'_, Token>) -> Option<char> {
    let s = lex.slice();
    let inner = unescape_string(&s[1..s.len() - 1]);
    inner.chars().next()
}

fn unescape_string(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

pub fn lex(source: &str, file_id: FileId) -> Result<Vec<SpannedToken>, LexError> {
    let mut tokens = Vec::new();
    let mut lexer = Token::lexer(source);
    while let Some(result) = lexer.next() {
        let span = Span::new(file_id, lexer.span().start as u32, lexer.span().end as u32);
        let token = result.map_err(|_| LexError { span })?;
        // 普通单行注释（`//`）在 lex 阶段过滤丢弃；文档注释（`///`）保留供 parser 收集。
        if matches!(token, Token::LineComment) {
            continue;
        }
        tokens.push(SpannedToken { token, span });
    }
    Ok(tokens)
}

/// Escape dump payloads so newlines/tabs do not break the one-token-per-line format.
fn dump_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn dump_kind_payload(kind: &str, payload: &str) -> String {
    format!("{kind}\t{}", dump_escape(payload))
}

/// Serialize `lex` output for golden / Arc lexer parity (RFC 011 §D Step 0).
///
/// Format: one token per line, no EOF.
/// - No payload: `KindName` (Rust `Token` variant name)
/// - Payload: `KindName\t` + escaped payload (`\\` `\n` `\t` `\r`)
/// - `FloatLit` uses the source slice (avoids float print drift)
pub fn dump_lex(source: &str) -> Result<String, LexError> {
    let tokens = lex(source, 0)?;
    let mut out = String::new();
    for st in &tokens {
        let line = match &st.token {
            Token::DocComment(s) => dump_kind_payload("DocComment", s),
            Token::Ident(s) => dump_kind_payload("Ident", s),
            Token::StringLit(s) => dump_kind_payload("StringLit", s),
            Token::VerbatimString(s) => dump_kind_payload("VerbatimString", s),
            Token::VerbatimInterpolatedString(s) => {
                dump_kind_payload("VerbatimInterpolatedString", s)
            }
            Token::InterpolatedString(s) => dump_kind_payload("InterpolatedString", s),
            Token::CharLit(c) => {
                let mut buf = String::new();
                buf.push(*c);
                dump_kind_payload("CharLit", &buf)
            }
            Token::IntLit(n) => dump_kind_payload("IntLit", &n.to_string()),
            Token::FloatLit(_) => {
                let start = st.span.start as usize;
                let end = st.span.end as usize;
                let slice = &source[start..end];
                dump_kind_payload("FloatLit", slice)
            }
            // Keywords / operators / delimiters — variant name only
            Token::Namespace => "Namespace".into(),
            Token::Using => "Using".into(),
            Token::Global => "Global".into(),
            Token::Struct => "Struct".into(),
            Token::Class => "Class".into(),
            Token::Record => "Record".into(),
            Token::With => "With".into(),
            Token::Interface => "Interface".into(),
            Token::Enum => "Enum".into(),
            Token::Variant => "Variant".into(),
            Token::Async => "Async".into(),
            Token::Await => "Await".into(),
            Token::From => "From".into(),
            Token::Where => "Where".into(),
            Token::Select => "Select".into(),
            Token::OrderBy => "OrderBy".into(),
            Token::Join => "Join".into(),
            Token::On => "On".into(),
            Token::Group => "Group".into(),
            Token::By => "By".into(),
            Token::Into => "Into".into(),
            Token::Let => "Let".into(),
            Token::Var => "Var".into(),
            Token::If => "If".into(),
            Token::Else => "Else".into(),
            Token::While => "While".into(),
            Token::For => "For".into(),
            Token::Foreach => "Foreach".into(),
            Token::In => "In".into(),
            Token::Return => "Return".into(),
            Token::Switch => "Switch".into(),
            Token::Case => "Case".into(),
            Token::Default => "Default".into(),
            Token::Break => "Break".into(),
            Token::Continue => "Continue".into(),
            Token::Throw => "Throw".into(),
            Token::Try => "Try".into(),
            Token::Catch => "Catch".into(),
            Token::Finally => "Finally".into(),
            Token::Lock => "Lock".into(),
            Token::Public => "Public".into(),
            Token::Private => "Private".into(),
            Token::Internal => "Internal".into(),
            Token::Protected => "Protected".into(),
            Token::Void => "Void".into(),
            Token::Float => "Float".into(),
            Token::Double => "Double".into(),
            Token::Long => "Long".into(),
            Token::Short => "Short".into(),
            Token::Byte => "Byte".into(),
            Token::Char => "Char".into(),
            Token::UInt => "UInt".into(),
            Token::ULong => "ULong".into(),
            Token::UShort => "UShort".into(),
            Token::SByte => "SByte".into(),
            Token::True => "True".into(),
            Token::False => "False".into(),
            Token::New => "New".into(),
            Token::Virtual => "Virtual".into(),
            Token::Override => "Override".into(),
            Token::Abstract => "Abstract".into(),
            Token::Static => "Static".into(),
            Token::Operator => "Operator".into(),
            Token::Const => "Const".into(),
            Token::Comptime => "Comptime".into(),
            Token::Readonly => "Readonly".into(),
            Token::Ref => "Ref".into(),
            Token::Out => "Out".into(),
            Token::Params => "Params".into(),
            Token::This => "This".into(),
            Token::Base => "Base".into(),
            Token::Descending => "Descending".into(),
            Token::Null => "Null".into(),
            Token::TypeOf => "TypeOf".into(),
            Token::NameOf => "NameOf".into(),
            Token::Is => "Is".into(),
            Token::When => "When".into(),
            Token::Delegate => "Delegate".into(),
            Token::LineComment => unreachable!("LineComment filtered by lex"),
            Token::Semi => "Semi".into(),
            Token::Comma => "Comma".into(),
            Token::Dot => "Dot".into(),
            Token::DotDot => "DotDot".into(),
            Token::Colon => "Colon".into(),
            Token::LParen => "LParen".into(),
            Token::RParen => "RParen".into(),
            Token::LBrace => "LBrace".into(),
            Token::RBrace => "RBrace".into(),
            Token::LBracket => "LBracket".into(),
            Token::RBracket => "RBracket".into(),
            Token::Lt => "Lt".into(),
            Token::Gt => "Gt".into(),
            Token::Shl => "Shl".into(),
            Token::Shr => "Shr".into(),
            Token::FatArrow => "FatArrow".into(),
            Token::Arrow => "Arrow".into(),
            Token::Eq => "Eq".into(),
            Token::PlusPlus => "PlusPlus".into(),
            Token::MinusMinus => "MinusMinus".into(),
            Token::PlusEq => "PlusEq".into(),
            Token::MinusEq => "MinusEq".into(),
            Token::StarEq => "StarEq".into(),
            Token::SlashEq => "SlashEq".into(),
            Token::Plus => "Plus".into(),
            Token::Minus => "Minus".into(),
            Token::Star => "Star".into(),
            Token::Slash => "Slash".into(),
            Token::Percent => "Percent".into(),
            Token::Bang => "Bang".into(),
            Token::BitOr => "BitOr".into(),
            Token::BitOrEq => "BitOrEq".into(),
            Token::BitAnd => "BitAnd".into(),
            Token::BitAndEq => "BitAndEq".into(),
            Token::BitXor => "BitXor".into(),
            Token::BitXorEq => "BitXorEq".into(),
            Token::Tilde => "Tilde".into(),
            Token::AndAnd => "AndAnd".into(),
            Token::OrOr => "OrOr".into(),
            Token::EqEq => "EqEq".into(),
            Token::NotEq => "NotEq".into(),
            Token::Le => "Le".into(),
            Token::Ge => "Ge".into(),
            Token::NullCoalesceEq => "NullCoalesceEq".into(),
            Token::NullCoalesce => "NullCoalesce".into(),
            Token::QuestionDot => "QuestionDot".into(),
            Token::BangDot => "BangDot".into(),
            Token::Question => "Question".into(),
        };
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid token at {}..{}", self.span.start, self.span.end)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        // RFC 045：lexer fixtures 随 0514f024 迁入 `crates/parse/fixtures`。
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn read_fixture(name: &str) -> String {
        let path = fixtures_dir().join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn lex_keywords() {
        let tokens = lex("async from select", 0).unwrap();
        assert_eq!(tokens[0].token, Token::Async);
        assert_eq!(tokens[1].token, Token::From);
    }

    #[test]
    fn dump_lex_basic_kinds_and_escape() {
        let dump = dump_lex(
            r#"async foo; // line
/// doc note
"hi\n" 'x' 0xFF 1.5 ++ || ?.
"#,
        )
        .unwrap();
        assert!(dump.contains("Async\n"));
        assert!(dump.contains("Ident\tfoo\n"));
        assert!(dump.contains("Semi\n"));
        assert!(!dump.contains("LineComment"));
        assert!(dump.contains("DocComment\tdoc note\n"));
        assert!(dump.contains("StringLit\thi\\n\n"));
        assert!(dump.contains("CharLit\tx\n"));
        assert!(dump.contains("IntLit\t255\n"));
        assert!(dump.contains("FloatLit\t1.5\n"));
        assert!(dump.contains("PlusPlus\n"));
        assert!(dump.contains("OrOr\n"));
        assert!(dump.contains("QuestionDot\n"));
    }

    #[test]
    fn dump_lex_null_coalesce_assign_maximal_munch() {
        // `??=`（NullCoalesceEq）与 `??`（NullCoalesce）并存：logos 最长
        // 匹配下三字符 lexeme 优先于两字符，`?? x` 不受影响。
        let dump = dump_lex("a ??= b ?? c").unwrap();
        assert!(dump.contains("NullCoalesceEq\n"));
        assert!(dump.contains("NullCoalesce\n"));
    }

    #[test]
    fn dump_lex_simple_interp() {
        let dump = dump_lex(r#"$"x={y}""#).unwrap();
        assert_eq!(dump, "InterpolatedString\tx={y}\n");
    }

    #[test]
    fn dump_lex_char_tab_and_bin_hex() {
        let dump = dump_lex(r#"'\t' 0b1010 0xFF"#).unwrap();
        assert_eq!(dump, "CharLit\t\\t\nIntLit\t10\nIntLit\t255\n");
    }

    #[test]
    fn dump_lex_float_uses_source_slice() {
        // Avoid f64 print drift (e.g. 0.1); payload must be the lexeme.
        let dump = dump_lex("0.1 3.14159").unwrap();
        assert_eq!(dump, "FloatLit\t0.1\nFloatLit\t3.14159\n");
    }

    #[test]
    fn dump_lex_verbatim_and_doc_escape() {
        let dump = dump_lex("/// a\\tb\n@\"say \"\"hi\"\"\\n\"").unwrap();
        assert!(dump.contains("DocComment\ta\\\\tb\n"));
        assert!(dump.contains("VerbatimString\tsay \"hi\"\\\\n\n"));
    }

    #[test]
    fn dump_lex_fixture_basic_golden() {
        let dump = dump_lex(&read_fixture("basic.as")).unwrap();
        let expected = "\
Async
Ident\tfoo
Semi
DocComment\tdoc note
StringLit\thi\\n
CharLit\t\\t
IntLit\t255
IntLit\t10
IntLit\t42
FloatLit\t1.5
PlusPlus
MinusMinus
PlusEq
MinusEq
StarEq
SlashEq
OrOr
BitOr
BitOrEq
BitAndEq
BitXorEq
DotDot
FatArrow
QuestionDot
BangDot
NullCoalesce
AndAnd
EqEq
NotEq
Le
Ge
Arrow
Namespace
Using
Class
Struct
Enum
Var
If
Else
While
For
Return
Public
Void
New
True
False
Null
LBrace
RBrace
LParen
RParen
LBracket
RBracket
Lt
Gt
Comma
Colon
Dot
Question
Bang
Plus
Minus
Star
Slash
Percent
Eq
InterpolatedString\tx={y}
";
        assert_eq!(dump, expected);
        assert!(!dump.contains("LineComment"));
        assert!(!dump.lines().any(|l| l == "Eof"));
    }

    #[test]
    fn dump_lex_fixture_interp_golden() {
        let dump = dump_lex(&read_fixture("interp.as")).unwrap();
        let expected = "\
InterpolatedString\thello {name}
VerbatimInterpolatedString\tverbatim {x}
VerbatimString\tsay \"hi\"\\\\n
VerbatimInterpolatedString\talt {y}
";
        assert_eq!(dump, expected);
    }
}
