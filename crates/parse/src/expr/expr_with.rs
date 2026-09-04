//! RFC 006 M2：`expr with { Member = value, … }` 后缀解析。

use crate::error::ParseError;
use crate::lexer::Token;
use crate::parser::Parser;
use ast::*;

impl Parser {
    /// 解析 `with { Ident = Expr, … }`，接收者已解析为 `left`。
    pub(crate) fn parse_with_expr(
        &mut self,
        left: Spanned<Expr>,
    ) -> Result<Spanned<Expr>, ParseError> {
        let start = left.span;
        self.expect(Token::With)?;
        self.expect(Token::LBrace)?;
        let inits = self.parse_object_initializer_fields()?;
        Ok(Spanned::new(
            Expr::With {
                receiver: Box::new(left),
                inits,
            },
            start.merge(self.prev_span()),
        ))
    }
}
