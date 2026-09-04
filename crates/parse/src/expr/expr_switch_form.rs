//! RFC 036 M4：C# 8 switch 表达式 `e switch { pat => expr, ... }`。

use crate::error::ParseError;
use crate::lexer::Token;
use crate::parser::Parser;
use ast::*;

impl Parser {
    /// 解析 postfix switch 表达式；`scrutinee` 已解析完毕。
    pub(crate) fn parse_switch_expr_form(
        &mut self,
        scrutinee: Spanned<Expr>,
    ) -> Result<Spanned<Expr>, ParseError> {
        let start = scrutinee.span;
        self.expect(Token::Switch)?;
        self.expect(Token::LBrace)?;
        let mut arms = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let pattern = self.parse_pattern()?;
            let when = if self.match_token(&Token::When) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::FatArrow)?;
            let body = self.parse_expr()?;
            arms.push(SwitchExprArm {
                pattern,
                when,
                body,
            });
            if self.match_token(&Token::Comma) {
                continue;
            }
            // 允许省略末尾逗号：下一 token 为 `}`
            if self.check(&Token::RBrace) {
                break;
            }
            return Err(self.error("`,` or `}`", self.describe_current()));
        }
        self.expect(Token::RBrace)?;
        Ok(Spanned::new(
            Expr::SwitchForm(SwitchExprForm {
                scrutinee: Box::new(scrutinee),
                arms,
            }),
            start.merge(self.prev_span()),
        ))
    }
}
