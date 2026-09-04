//! RFC 007：插值字符串 interior 拆分与洞表达式解析。
//! M2a：支持 `{expr,align}` / `{expr:format}` / `{expr,align:format}`；
//! M2h：`$@"..."/`@$"..."` verbatim（`""` → `"`，无 `\` 转义）；多行 `$"..."` 同源。
//! 不支持的自定义格式串在 typeck 硬错误（本文件只拆分语法）。

use crate::error::ParseError;
use crate::lexer;
use crate::parser::Parser;
use ast::*;

impl Parser {
    /// 将 lexer 产出的 `$"..."` interior（未 unescape）解析为 `Expr::InterpolatedString`。
    pub(crate) fn parse_interpolated_from_interior(
        interior: &str,
        span: Span,
    ) -> Result<Expr, ParseError> {
        Self::parse_interpolated_from_interior_ex(interior, span, false)
    }

    /// RFC 007 M2h：verbatim 插值 interior。
    pub(crate) fn parse_verbatim_interpolated_from_interior(
        interior: &str,
        span: Span,
    ) -> Result<Expr, ParseError> {
        Self::parse_interpolated_from_interior_ex(interior, span, true)
    }

    fn parse_interpolated_from_interior_ex(
        interior: &str,
        span: Span,
        verbatim: bool,
    ) -> Result<Expr, ParseError> {
        let parts = split_interp_parts(interior, span, verbatim)?;
        Ok(Expr::InterpolatedString { parts })
    }
}

fn split_interp_parts(
    interior: &str,
    span: Span,
    verbatim: bool,
) -> Result<Vec<InterpPart>, ParseError> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let bytes = interior.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                if !lit.is_empty() {
                    parts.push(InterpPart::Lit(std::mem::take(&mut lit)));
                }
                i += 1;
                let hole_start = i;
                let mut depth = 1usize;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'"' => {
                            i = skip_escaped_quoted(bytes, i + 1, b'"')?;
                        }
                        b'\'' => {
                            i = skip_escaped_quoted(bytes, i + 1, b'\'')?;
                        }
                        b'@' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                            i = skip_verbatim_quoted(bytes, i + 2)?;
                        }
                        b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                            i = skip_nested_interp(bytes, i + 2, false)?;
                        }
                        b'$' if i + 2 < bytes.len()
                            && bytes[i + 1] == b'@'
                            && bytes[i + 2] == b'"' =>
                        {
                            i = skip_nested_interp(bytes, i + 3, true)?;
                        }
                        b'@' if i + 2 < bytes.len()
                            && bytes[i + 1] == b'$'
                            && bytes[i + 2] == b'"' =>
                        {
                            i = skip_nested_interp(bytes, i + 3, true)?;
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
                if depth != 0 {
                    return Err(ParseError::Unexpected {
                        span,
                        expected: "closing '}' for interpolation hole".into(),
                        found: "end of interpolated string".into(),
                    });
                }
                let hole_end = i - 1;
                let hole = &interior[hole_start..hole_end];
                parts.push(InterpPart::Expr(parse_hole(hole, span)?));
            }
            b'}' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'}' {
                    lit.push('}');
                    i += 2;
                } else {
                    return Err(ParseError::Unexpected {
                        span,
                        expected: "'}}' escape or end of string".into(),
                        found: "lone '}'".into(),
                    });
                }
            }
            b'"' if verbatim && i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                lit.push('"');
                i += 2;
            }
            b'\\' if !verbatim => {
                let rest = &interior[i..];
                let (ch, consumed) = unescape_one(rest)?;
                lit.push(ch);
                i += consumed;
            }
            _ => {
                let ch = interior[i..].chars().next().unwrap();
                lit.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    if !lit.is_empty() || parts.is_empty() {
        parts.push(InterpPart::Lit(lit));
    }
    Ok(parts)
}

/// 拆分洞为 `expr` / 可选 `alignment` / 可选 `format`（顶层 `,` / `:`）。
fn parse_hole(hole: &str, span: Span) -> Result<InterpHole, ParseError> {
    let (expr_src, alignment, format) = split_hole_components(hole, span)?;
    if expr_src.trim().is_empty() {
        return Err(ParseError::Unexpected {
            span,
            expected: "expression inside interpolation hole".into(),
            found: "empty hole".into(),
        });
    }
    let expr = parse_hole_expr(expr_src, span)?;
    Ok(InterpHole {
        expr,
        alignment,
        format,
    })
}

fn split_hole_components(
    hole: &str,
    span: Span,
) -> Result<(&str, Option<i32>, Option<String>), ParseError> {
    let bytes = hole.as_bytes();
    let mut i = 0;
    let mut depth = 0usize;
    let mut align_at: Option<usize> = None;
    let mut format_at: Option<usize> = None;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'\'' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'\'' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                i += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b',' if depth == 0 && align_at.is_none() && format_at.is_none() => {
                align_at = Some(i);
                i += 1;
            }
            b':' if depth == 0 && format_at.is_none() => {
                format_at = Some(i);
                i += 1;
            }
            _ => i += 1,
        }
    }

    let (expr_end, alignment, format) = match (align_at, format_at) {
        (None, None) => (hole.len(), None, None),
        (Some(a), None) => {
            let align = parse_alignment(&hole[a + 1..], span)?;
            (a, Some(align), None)
        }
        (None, Some(f)) => {
            let fmt = hole[f + 1..].to_string();
            if fmt.is_empty() {
                return Err(ParseError::Unexpected {
                    span,
                    expected: "format specifier after ':'".into(),
                    found: "empty format".into(),
                });
            }
            (f, None, Some(fmt))
        }
        (Some(a), Some(f)) => {
            if f < a {
                return Err(ParseError::Unexpected {
                    span,
                    expected: "{expr,alignment:format} with alignment before format".into(),
                    found: "':' before ','".into(),
                });
            }
            let align = parse_alignment(&hole[a + 1..f], span)?;
            let fmt = hole[f + 1..].to_string();
            if fmt.is_empty() {
                return Err(ParseError::Unexpected {
                    span,
                    expected: "format specifier after ':'".into(),
                    found: "empty format".into(),
                });
            }
            (a, Some(align), Some(fmt))
        }
    };
    Ok((&hole[..expr_end], alignment, format))
}

fn parse_alignment(src: &str, span: Span) -> Result<i32, ParseError> {
    let s = src.trim();
    if s.is_empty() {
        return Err(ParseError::Unexpected {
            span,
            expected: "integer alignment after ','".into(),
            found: "empty alignment".into(),
        });
    }
    s.parse::<i32>().map_err(|_| ParseError::Unexpected {
        span,
        expected: "integer literal alignment (RFC 007 M2a)".into(),
        found: format!("'{s}'"),
    })
}

fn skip_escaped_quoted(bytes: &[u8], mut i: usize, quote: u8) -> Result<usize, ParseError> {
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return Ok(i + 1);
        }
        i += 1;
    }
    Err(ParseError::Eof)
}

fn skip_verbatim_quoted(bytes: &[u8], mut i: usize) -> Result<usize, ParseError> {
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                i += 2;
            } else {
                return Ok(i + 1);
            }
        } else {
            i += 1;
        }
    }
    Err(ParseError::Eof)
}

fn skip_nested_interp(bytes: &[u8], mut i: usize, verbatim: bool) -> Result<usize, ParseError> {
    let mut depth = 0usize;
    while i < bytes.len() {
        if depth == 0 {
            match bytes[i] {
                b'"' => {
                    if verbatim && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        i += 2;
                    } else {
                        return Ok(i + 1);
                    }
                }
                b'\\' if !verbatim => i += 2,
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
                        return Err(ParseError::Eof);
                    }
                }
                _ => i += 1,
            }
        } else {
            match bytes[i] {
                b'"' => {
                    i = skip_escaped_quoted(bytes, i + 1, b'"')?;
                }
                b'\'' => {
                    i = skip_escaped_quoted(bytes, i + 1, b'\'')?;
                }
                b'@' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                    i = skip_verbatim_quoted(bytes, i + 2)?;
                }
                b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'"' => {
                    i = skip_nested_interp(bytes, i + 2, false)?;
                }
                b'$' if i + 2 < bytes.len() && bytes[i + 1] == b'@' && bytes[i + 2] == b'"' => {
                    i = skip_nested_interp(bytes, i + 3, true)?;
                }
                b'@' if i + 2 < bytes.len() && bytes[i + 1] == b'$' && bytes[i + 2] == b'"' => {
                    i = skip_nested_interp(bytes, i + 3, true)?;
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
    Err(ParseError::Eof)
}

fn unescape_one(s: &str) -> Result<(char, usize), ParseError> {
    let mut chars = s.chars();
    let first = chars.next().ok_or(ParseError::Eof)?;
    if first != '\\' {
        return Ok((first, first.len_utf8()));
    }
    match chars.next() {
        Some('n') => Ok(('\n', 2)),
        Some('t') => Ok(('\t', 2)),
        Some('r') => Ok(('\r', 2)),
        Some('\\') => Ok(('\\', 2)),
        Some('"') => Ok(('"', 2)),
        Some('\'') => Ok(('\'', 2)),
        Some(other) => Ok((other, 1 + other.len_utf8())),
        None => Ok(('\\', 1)),
    }
}

fn parse_hole_expr(hole: &str, span: Span) -> Result<Spanned<Expr>, ParseError> {
    let tokens = lexer::lex(hole, span.file_id).map_err(|_| ParseError::Unexpected {
        span,
        expected: "valid expression in interpolation hole".into(),
        found: "invalid token".into(),
    })?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    if !parser.is_at_end() {
        return Err(ParseError::Unexpected {
            span,
            expected: "end of interpolation hole".into(),
            found: format!("{:?}", parser.peek().token),
        });
    }
    // 保留外层 span，便于诊断定位到整段 `$"..."`
    Ok(Spanned::new(expr.node, span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{lex, Token};

    fn parse_interp(src: &str) -> Expr {
        let tokens = lex(src, 0).expect("lex");
        assert_eq!(tokens.len(), 1);
        match &tokens[0].token {
            Token::InterpolatedString(interior) => {
                Parser::parse_interpolated_from_interior(interior, tokens[0].span).expect("parse")
            }
            Token::VerbatimInterpolatedString(interior) => {
                Parser::parse_verbatim_interpolated_from_interior(interior, tokens[0].span)
                    .expect("parse")
            }
            other => panic!("expected InterpolatedString, got {other:?}"),
        }
    }

    fn parse_interp_err(src: &str) -> ParseError {
        let tokens = lex(src, 0).expect("lex");
        match &tokens[0].token {
            Token::InterpolatedString(interior) => {
                Parser::parse_interpolated_from_interior(interior, tokens[0].span).unwrap_err()
            }
            Token::VerbatimInterpolatedString(interior) => {
                Parser::parse_verbatim_interpolated_from_interior(interior, tokens[0].span)
                    .unwrap_err()
            }
            other => panic!("expected InterpolatedString, got {other:?}"),
        }
    }

    #[test]
    fn lit_only() {
        let e = parse_interp(r#"$"hello""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts, vec![InterpPart::Lit("hello".into())]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn brace_escape() {
        let e = parse_interp(r#"$"{{x}}""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts, vec![InterpPart::Lit("{x}".into())]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn simple_hole() {
        let e = parse_interp(r#"$"a{x}b""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Lit("a".into()));
                match &parts[1] {
                    InterpPart::Expr(hole) => {
                        assert!(hole.alignment.is_none());
                        assert!(hole.format.is_none());
                        match &hole.expr.node {
                            Expr::Ident(n) => assert_eq!(n.as_str(), "x"),
                            other => panic!("{other:?}"),
                        }
                    }
                    other => panic!("{other:?}"),
                }
                assert_eq!(parts[2], InterpPart::Lit("b".into()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hole_with_string_lit() {
        let e = parse_interp(r#"$"hi {foo("bar")}""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(parts[1], InterpPart::Expr(_)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hole_with_alignment() {
        let e = parse_interp(r#"$"{x,5}""#);
        match e {
            Expr::InterpolatedString { parts } => match &parts[0] {
                InterpPart::Expr(hole) => {
                    assert_eq!(hole.alignment, Some(5));
                    assert!(hole.format.is_none());
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hole_with_neg_alignment() {
        let e = parse_interp(r#"$"{x,-8}""#);
        match e {
            Expr::InterpolatedString { parts } => match &parts[0] {
                InterpPart::Expr(hole) => assert_eq!(hole.alignment, Some(-8)),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hole_with_format() {
        let e = parse_interp(r#"$"{n:D5}""#);
        match e {
            Expr::InterpolatedString { parts } => match &parts[0] {
                InterpPart::Expr(hole) => {
                    assert!(hole.alignment.is_none());
                    assert_eq!(hole.format.as_deref(), Some("D5"));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hole_with_align_and_format() {
        let e = parse_interp(r#"$"{n,10:X}""#);
        match e {
            Expr::InterpolatedString { parts } => match &parts[0] {
                InterpPart::Expr(hole) => {
                    assert_eq!(hole.alignment, Some(10));
                    assert_eq!(hole.format.as_deref(), Some("X"));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn empty_format_errors() {
        let _ = parse_interp_err(r#"$"{x:}""#);
    }

    #[test]
    fn bad_alignment_errors() {
        let _ = parse_interp_err(r#"$"{x,y}""#);
    }

    #[test]
    fn verbatim_dollar_at_quote_escape() {
        let e = parse_interp(r#"$@"say ""hi"" {x}""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0], InterpPart::Lit("say \"hi\" ".into()));
                assert!(matches!(parts[1], InterpPart::Expr(_)));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn verbatim_at_dollar_form() {
        let e = parse_interp(r#"@$"a{x}b""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Lit("a".into()));
                assert_eq!(parts[2], InterpPart::Lit("b".into()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn verbatim_keeps_backslash() {
        let e = parse_interp(r#"$@"\n{x}""#);
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts[0], InterpPart::Lit("\\n".into()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multiline_dollar_interp() {
        let e = parse_interp("$\"a\n{x}\nb\"");
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Lit("a\n".into()));
                assert_eq!(parts[2], InterpPart::Lit("\nb".into()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn multiline_verbatim_interp() {
        let e = parse_interp("$@\"a\n{x}\nb\"");
        match e {
            Expr::InterpolatedString { parts } => {
                assert_eq!(parts.len(), 3);
                assert_eq!(parts[0], InterpPart::Lit("a\n".into()));
                assert_eq!(parts[2], InterpPart::Lit("\nb".into()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn non_interp_verbatim_string() {
        let tokens = lex(r#"@"say ""hi""\n""#, 0).expect("lex");
        assert_eq!(tokens.len(), 1);
        match &tokens[0].token {
            Token::VerbatimString(s) => assert_eq!(s, "say \"hi\"\\n"),
            other => panic!("{other:?}"),
        }
        let program = Parser::parse_program(r#"string Main() { return @"c:\path"; }"#).unwrap();
        match &program.items[0].node {
            Item::Fn(f) => {
                let body = f.body.as_ref().unwrap();
                match &body.stmts[0].node {
                    Stmt::Return(Some(e)) => {
                        assert!(matches!(&e.node, Expr::StringLit(s) if s == r"c:\path"));
                    }
                    other => panic!("{other:?}"),
                }
            }
            other => panic!("{other:?}"),
        }
    }
}
