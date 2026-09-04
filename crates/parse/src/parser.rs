use crate::lexer::{SpannedToken, Token};
use ast::*;

use crate::error::ParseError;

pub struct Parser {
    pub(crate) tokens: Vec<SpannedToken>,
    pub(crate) pos: usize,
    /// 当前打开的 catch 子句绑定名栈（内层在后）。合成名（`__catch_all` /
    /// `__catch_unnamed`）与实名一视同仁——裸重抛 `throw;` 脱糖为
    /// `throw <栈顶绑定名>`，typeck/MIR 零改动（绑定已由 catch 作用域承载）。
    pub(crate) catch_bindings: Vec<Ident>,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            pos: 0,
            catch_bindings: Vec::new(),
        }
    }

    /// 解析源码（单文件便利入口，file_id=0）。
    /// 适用于测试、CLI 单文件编译等无需跨文件区分的场景。
    pub fn parse_program(source: &str) -> Result<Program, ParseError> {
        Self::parse_program_in_file(source, 0)
    }

    /// 解析源码，指定 file_id（多文件项目场景）。
    /// file_id 由 loader 分配，索引到 FileRegistry 映射文件路径。
    pub fn parse_program_in_file(source: &str, file_id: FileId) -> Result<Program, ParseError> {
        // UTF-8 BOM 容忍：外部编辑器产出的源文件可能携带 EF BB BF 前缀，
        // 剥离后再进 lexer（BOM 属编码元数据，非源码 token）。
        let source = source.strip_prefix('\u{feff}').unwrap_or(source);
        let tokens = crate::lexer::lex(source, file_id).map_err(|e| ParseError::Unexpected {
            span: e.span,
            expected: "valid token".into(),
            found: "invalid character".into(),
        })?;
        Self::new(tokens).parse_program_items()
    }

    /// RFC 009 M4-6: 解析语句序列（无大括号包裹），直到 EOF。
    ///
    /// 用于宏展开字符串解析：将受限求值器（M4-4）输出的字符串解析为
    /// AST 语句列表，后续由 `typeck::macro_eval::splice` 模块注入到
    /// 宏容器方法体。
    ///
    /// 与 `parse_program_in_file` 的区别：本方法解析的是「语句序列」
    /// （`Stmt` 列表）而非「顶层 `Item` 列表」——展开代码片段不包含
    /// namespace / class / fn 等顶层声明，仅是方法体内的语句。
    ///
    /// `file_id` 通常为合成文件 ID（如基于委托位置生成），用于 lex 阶段
    /// 的 token span；splice 模块会在解析后重写所有 span 指向委托位置
    /// （RFC 009 D10.4 诊断锚点）。
    pub fn parse_stmts_from_str(
        source: &str,
        file_id: FileId,
    ) -> Result<Vec<Spanned<Stmt>>, ParseError> {
        let tokens = crate::lexer::lex(source, file_id).map_err(|e| ParseError::Unexpected {
            span: e.span,
            expected: "valid token".into(),
            found: "invalid character".into(),
        })?;
        let mut parser = Self::new(tokens);
        let mut stmts = Vec::new();
        while !parser.is_at_end() {
            stmts.push(parser.parse_stmt()?);
        }
        Ok(stmts)
    }
}

impl Parser {
    pub(crate) fn parse_ident(&mut self) -> Result<Ident, ParseError> {
        match self.advance().token.clone() {
            Token::Ident(s) => Ok(s.into()),
            _ => Err(self.error("identifier", self.describe_prev())),
        }
    }

    pub(crate) fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.match_token(&expected) {
            Ok(())
        } else {
            Err(self.error(&format!("{expected:?}"), self.describe_current()))
        }
    }

    /// 消费泛型实参列表的关闭 `>`。
    ///
    /// 词法器将 `>>`（嵌套泛型如 `List<List<int>>`）合并为单个 `Shr` token。
    /// 此处将其拆分为两个 `Gt`：本层消费一个，剩余一个以合成 token 插回流中，
    /// 供外层泛型关闭使用（与 C#/Java 编译器语义一致）。
    pub(crate) fn expect_gt_close(&mut self) -> Result<(), ParseError> {
        if self.match_token(&Token::Gt) {
            return Ok(());
        }
        if self.check(&Token::Shr) {
            let span = self.peek().span;
            self.advance();
            self.tokens.insert(
                self.pos,
                SpannedToken {
                    token: Token::Gt,
                    span,
                },
            );
            return Ok(());
        }
        Err(self.error("`>`", self.describe_current()))
    }

    pub(crate) fn match_token(&mut self, token: &Token) -> bool {
        if self.check(token) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// RFC 037：识别上下文关键字（contextual keyword）。
    ///
    /// 检查当前 token 是否为指定名称的标识符（`Token::Ident(name)`），
    /// **且**下一 token 在 `follow_set` 中。若匹配则消费当前 token 并返回 `true`，
    /// 否则返回 `false`（不消费）。
    ///
    /// 用于 `partial` 等上下文关键字——它们在某些位置作为关键字，在其他位置
    /// 作为普通标识符（如 `var partial = 1;`）。通过 lookahead 判定上下文，
    /// 不破坏现有代码。
    pub(crate) fn match_ident_keyword(&mut self, name: &str, follow_set: &[Token]) -> bool {
        if let Token::Ident(s) = &self.peek().token {
            if s == name {
                let next = self.tokens.get(self.pos + 1).map(|t| &t.token);
                if let Some(next_tok) = next {
                    if follow_set.iter().any(|t| t == next_tok) {
                        self.advance();
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(crate) fn check_any(&self, tokens: &[Token]) -> bool {
        tokens.iter().any(|t| self.check(t))
    }

    pub(crate) fn advance(&mut self) -> &SpannedToken {
        if !self.is_at_end() {
            self.pos += 1;
        }
        &self.tokens[self.pos - 1]
    }

    pub(crate) fn peek(&self) -> &SpannedToken {
        &self.tokens[self.pos]
    }

    pub(crate) fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    pub(crate) fn check(&self, token: &Token) -> bool {
        !self.is_at_end() && &self.peek().token == token
    }

    pub(crate) fn current_span(&self) -> Span {
        if self.is_at_end() {
            self.prev_span()
        } else {
            self.peek().span
        }
    }

    pub(crate) fn prev_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(Span::DUMMY)
    }

    pub(crate) fn describe_current(&self) -> String {
        if self.is_at_end() {
            "EOF".into()
        } else {
            format!("{:?}", self.peek().token)
        }
    }

    pub(crate) fn describe_prev(&self) -> String {
        format!("{:?}", self.tokens[self.pos - 1].token)
    }

    pub(crate) fn looks_like_leading_type_decl(&self) -> bool {
        if !matches!(
            self.peek().token,
            Token::Void
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
                | Token::LParen
                | Token::Ident(_)
        ) {
            return false;
        }
        let saved = self.pos;
        let mut p = Parser {
            tokens: self.tokens.clone(),
            pos: saved,
            catch_bindings: self.catch_bindings.clone(),
        };
        p.parse_type().is_ok()
            && p.parse_ident().is_ok()
            && (p.check(&Token::Eq) || p.check(&Token::Semi))
    }

    pub(crate) fn error(&self, expected: &str, found: String) -> ParseError {
        ParseError::Unexpected {
            span: self.current_span(),
            expected: expected.to_string(),
            found,
        }
    }

    pub(crate) fn bare_brace_initializer_error(&self) -> ParseError {
        self.error(
            "`[e1, e2, ...]` collection expression or leading-type `T[] x = [...]`",
            "bare `{ ... }` is not valid — use `[...]` collection expression".into(),
        )
    }

    pub(crate) fn struct_without_new_error(&self, type_name: &str) -> ParseError {
        self.error(
            &format!("`new {type_name}() {{ ... }}`"),
            format!("`{type_name} {{ ... }}` without `new` — use `new {type_name}() {{ ... }}`"),
        )
    }
}
