use super::*;

impl Parser {
    pub(crate) fn parse_fn(&mut self) -> Result<FnDef, ParseError> {
        let vis = self.parse_vis();
        let is_async = self.match_token(&Token::Async);
        let ret = if self.starts_with_return_type() {
            Some(self.parse_type()?)
        } else {
            None
        };
        let name = self.parse_ident()?;
        self.parse_fn_tail(vis, name, is_async, ret)
    }

    pub(crate) fn parse_fn_tail(
        &mut self,
        vis: Visibility,
        name: Ident,
        is_async: bool,
        ret: Option<Spanned<Type>>,
    ) -> Result<FnDef, ParseError> {
        let generics = self.parse_generics()?;
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        let where_clause = self.parse_where_clause()?;
        let is_async_suffix = self.match_token(&Token::Async);
        let is_async = is_async || is_async_suffix;
        // 表达式体函数 `Ret F(...) => expr;` 与类方法同一脱糖。
        let body = if self.match_token(&Token::LBrace) {
            Some(self.parse_block_inner()?)
        } else if self.match_token(&Token::FatArrow) {
            Some(self.parse_expr_bodied_method_block(ret.as_ref())?)
        } else {
            self.expect(Token::Semi)?;
            None
        };
        Ok(FnDef {
            vis,
            name,
            generics,
            where_clause,
            params,
            ret,
            body,
            is_async,
            attributes: vec![],
            doc: None,
        })
    }

    pub(crate) fn finish_method_sig(
        &mut self,
        vis: Visibility,
        modifier: MethodModifier,
        is_async: bool,
        name: Ident,
        ret: Option<Spanned<Type>>,
    ) -> Result<MethodSig, ParseError> {
        self.finish_method_sig_ext(vis, modifier, false, is_async, name, ret)
    }

    /// RFC 004 M1：扩展 `finish_method_sig` 支持接口 `static abstract` 成员标记。
    ///
    /// `is_static_abstract=true` 仅在 `parse_interface_member` 检测到
    /// `static abstract` 修饰符组合时传入；其他调用方使用 `finish_method_sig`
    /// 保持 `is_static_abstract=false` 默认行为。
    pub(crate) fn finish_method_sig_ext(
        &mut self,
        vis: Visibility,
        modifier: MethodModifier,
        is_static_abstract: bool,
        is_async: bool,
        name: Ident,
        ret: Option<Spanned<Type>>,
    ) -> Result<MethodSig, ParseError> {
        let generics = self.parse_generics()?;
        let params = self.parse_params_after_name()?;
        let where_clause = self.parse_where_clause()?;
        Ok(MethodSig {
            vis,
            name,
            generics,
            where_clause,
            params,
            ret,
            is_async,
            modifier,
            is_static_abstract,
            attributes: vec![],
            doc: None,
        })
    }

    pub(crate) fn parse_params_after_name(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(Token::LParen)?;
        let params = self.parse_params()?;
        self.expect(Token::RParen)?;
        Ok(params)
    }

    pub(crate) fn parse_method_modifier(&mut self) -> MethodModifier {
        // C# 标准：`override abstract` 组合（派生抽象类重新声明基类方法为抽象）。
        // 接受 `override abstract` 与 `abstract override` 两种顺序。
        // 其他单修饰符走原路径。
        if self.check(&Token::Override) && self.check_at(1, &Token::Abstract) {
            self.advance(); // consume Override
            self.advance(); // consume Abstract
            return MethodModifier::OverrideAbstract;
        }
        if self.check(&Token::Abstract) && self.check_at(1, &Token::Override) {
            self.advance(); // consume Abstract
            self.advance(); // consume Override
            return MethodModifier::OverrideAbstract;
        }
        if self.match_token(&Token::Virtual) {
            MethodModifier::Virtual
        } else if self.match_token(&Token::Override) {
            MethodModifier::Override
        } else if self.match_token(&Token::Abstract) {
            MethodModifier::Abstract
        } else if self.match_token(&Token::Static) {
            MethodModifier::Static
        } else {
            MethodModifier::None
        }
    }

    pub(crate) fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.parse_params_until(|p| p.check(&Token::RParen))
    }

    /// RFC 007：索引器参数 `this[ int index, ... ]`。
    pub(crate) fn parse_indexer_params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(Token::LBracket)?;
        let params = self.parse_params_until(|p| p.check(&Token::RBracket))?;
        self.expect(Token::RBracket)?;
        if params.is_empty() {
            return Err(self.error(
                "indexer parameter",
                "indexer `this[]` requires at least one parameter".into(),
            ));
        }
        Ok(params)
    }

    fn parse_params_until(
        &mut self,
        at_end: impl Fn(&Self) -> bool,
    ) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if at_end(self) {
            return Ok(params);
        }
        loop {
            // 参数级属性（如 `[Description("...")]`，供 `[AITool]` 参数 schema 描述）。
            let attributes = self.parse_attributes()?;
            let is_params = self.match_token(&Token::Params);
            let is_extension_receiver = self.match_token(&Token::This);
            let is_ref = self.match_token(&Token::Ref);
            let is_out = self.match_token(&Token::Out);
            let is_in = self.match_token(&Token::In);
            // C# 规范：`ref`/`out`/`in` 三者互斥；`params` 与它们及 `this` 互斥。
            let modifier_count = (is_ref as u8) + (is_out as u8) + (is_in as u8);
            if modifier_count > 1 {
                return Err(self.error(
                    "parameter modifier `ref` / `out` / `in` (mutually exclusive)",
                    "multiple modifiers combined".into(),
                ));
            }
            if is_params && (modifier_count > 0 || is_extension_receiver) {
                return Err(self.error(
                    "`params` cannot combine with `ref` / `out` / `in` / `this`",
                    "invalid params modifier".into(),
                ));
            }
            if is_extension_receiver && modifier_count > 0 {
                return Err(self.error(
                    "extension receiver (`this`) without `ref` / `out` / `in`",
                    "modifier on extension receiver".into(),
                ));
            }
            let ty = self.parse_type()?;
            let name = self.parse_ident()?;
            let default = if self.match_token(&Token::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            if is_params && default.is_some() {
                return Err(self.error(
                    "`params` parameter cannot have a default value",
                    "params with default".into(),
                ));
            }
            params.push(Param {
                name,
                ty,
                attributes,
                is_extension_receiver,
                is_ref,
                is_out,
                is_in,
                is_params,
                default,
            });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        // `params` 仅允许末位形参。
        if let Some(pos) = params.iter().position(|p| p.is_params) {
            if pos + 1 != params.len() {
                return Err(self.error(
                    "`params` is only allowed on the final parameter",
                    "params not last".into(),
                ));
            }
        }
        Ok(params)
    }
}
