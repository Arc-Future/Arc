use super::*;

impl Parser {
    pub(crate) fn parse_field_or_property(&mut self) -> Result<FieldOrProperty, ParseError> {
        // RFC 017：收集成员级 `///` 文档注释（P1d）。
        let doc = self.collect_doc_comments();
        let attrs = self.parse_attributes()?;
        let vis = self.parse_vis();
        let (is_const, is_readonly, is_static) = self.parse_field_modifier()?;
        let ty = self.parse_type()?;
        let name = self.parse_ident()?;
        if self.match_token(&Token::LBrace) {
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let accessor = self.parse_ident()?;
                match accessor.as_str() {
                    "get" | "set" | "init" => {}
                    _ => {
                        return Err(
                            self.error("get, set, or init", format!("found accessor `{accessor}`"))
                        );
                    }
                }
                self.match_token(&Token::Semi);
            }
            self.expect(Token::RBrace)?;
            // FieldOrProperty::Property 为标记变体，不携带 PropertyDef；
            // 此处 doc/attrs 随标记丢弃（实际 PropertyDef 由 parse_class_body_member /
            // parse_interface_member 直接构造并设置 doc/attributes）。
            Ok(FieldOrProperty::Property)
        } else {
            let init = if self.match_token(&Token::Eq) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::Semi)?;
            Ok(FieldOrProperty::Field(FieldDef {
                vis,
                name,
                ty,
                is_readonly,
                is_const,
                is_static,
                init,
                attributes: attrs,
                doc,
            }))
        }
    }

    pub(crate) fn parse_class_body_member(
        &mut self,
        class_name: &Ident,
    ) -> Result<ClassBodyMember, ParseError> {
        // RFC 017：收集成员级 `///` 文档注释（P1d）。
        let doc = self.collect_doc_comments();
        let attrs = self.parse_attributes()?;
        let vis = self.parse_vis();
        let (is_const, is_readonly, is_static) = self.parse_field_modifier()?;
        // RFC 061：parse_field_modifier 已消费 `static`，若成员最终是方法，
        // 需根据 is_static 设置 MethodModifier::Static（parse_method_modifier
        // 此时不会看到 static token）。
        let mut modifier = self.parse_method_modifier();
        if is_static && modifier == MethodModifier::None {
            modifier = MethodModifier::Static;
        }
        let is_async = self.match_token(&Token::Async);

        // RFC 006 M3：`required` 修饰符（属性前；字段暂不支持）。
        let is_required = if matches!(&self.peek().token, Token::Ident(n) if n.as_str() == "required")
        {
            self.advance();
            true
        } else {
            false
        };

        // Constructor: public Rectangle(int w, int h) { } — no return type
        if let Token::Ident(name) = &self.peek().token {
            if name == class_name
                && self.check_at(1, &Token::LParen)
                && modifier == MethodModifier::None
                && !is_async
            {
                if is_required {
                    return Err(self.error(
                        "constructor",
                        "`required` cannot modify a constructor (RFC 006 M3)".into(),
                    ));
                }
                let _ = self.parse_ident()?;
                let params = self.parse_params_after_name()?;
                // 构造器初始化器 `: base(args)`（RFC 009 L1）。
                // 仅支持 `: base(...)`；`: this(...)` 暂未实现。
                // `base` 是关键字 token（Token::Base），需用 match_token 消费。
                let base_args = if self.match_token(&Token::Colon) {
                    if !self.match_token(&Token::Base) {
                        return Err(
                            self.error("`base`", format!("found `{:?}`", self.peek().token))
                        );
                    }
                    self.expect(Token::LParen)?;
                    Some(self.parse_call_args()?)
                } else {
                    None
                };
                self.expect(Token::LBrace)?;
                let body = self.parse_block_inner()?;
                return Ok(ClassBodyMember::Constructor(ConstructorDef {
                    vis,
                    params,
                    body,
                    base_args,
                    doc,
                }));
            }
        }

        let ty = self.parse_type()?;

        // RFC 060：`T this[params] { get/set }` 索引器声明；亦支持 `T this[...] => expr;`。
        if self.check(&Token::This) {
            self.advance();
            let index_params = self.parse_indexer_params()?;
            if self.match_token(&Token::FatArrow) {
                let get_body = self.parse_expr_bodied_return_block()?;
                return Ok(ClassBodyMember::Property(PropertyDef {
                    vis,
                    name: "Item".into(),
                    ty,
                    has_get: true,
                    has_set: false,
                    has_init: false,
                    is_required,
                    get_body: Some(get_body),
                    set_body: None,
                    get_vis: None,
                    set_vis: None,
                    modifier,
                    is_static_abstract: false,
                    attributes: attrs,
                    index_params,
                    init: None,
                    doc,
                }));
            }
            self.expect(Token::LBrace)?;
            let (get_vis, set_vis, has_get, has_set, has_init, get_body, set_body) =
                self.parse_property_accessors_inner()?;
            return Ok(ClassBodyMember::Property(PropertyDef {
                vis,
                name: "Item".into(),
                ty,
                has_get,
                has_set,
                has_init,
                is_required,
                get_body,
                set_body,
                get_vis,
                set_vis,
                modifier,
                is_static_abstract: false,
                attributes: attrs,
                index_params,
                init: None,
                doc,
            }));
        }

        // RFC 003：`TRet operator ⊕(…)` → MethodDef(name = op_*)。
        if self.check(&Token::Operator) {
            return self.parse_operator_method(
                vis,
                modifier,
                is_async,
                is_required,
                ty,
                attrs,
                doc,
            );
        }

        let name = self.parse_ident()?;

        if self.check(&Token::LParen) || self.check(&Token::Lt) {
            if is_required {
                return Err(self.error(
                    "method",
                    "`required` cannot modify a method (RFC 006 M3)".into(),
                ));
            }
            let mut sig = self.finish_method_sig(vis, modifier, is_async, name, Some(ty))?;
            sig.attributes = attrs;
            // 方法 doc 优先置于 MethodDef.doc；sig.doc 保持 None（与 docgen 优先取
            // MethodDef.doc 的逻辑一致）。
            // 表达式体方法 `Ret M(...) => expr;` 脱糖为块（void → 语句；非 void → return）。
            let body = if self.match_token(&Token::LBrace) {
                Some(self.parse_block_inner()?)
            } else if self.match_token(&Token::FatArrow) {
                Some(self.parse_expr_bodied_method_block(sig.ret.as_ref())?)
            } else {
                self.expect(Token::Semi)?;
                None
            };
            return Ok(ClassBodyMember::Method(MethodDef { sig, body, doc }));
        }

        // 表达式体只读属性：Type Prop => expr; → { get { return expr; } }
        if self.match_token(&Token::FatArrow) {
            let get_body = self.parse_expr_bodied_return_block()?;
            if is_required {
                return Err(self.error(
                    "required property",
                    "`required` property must have `set` or `init` accessor (RFC 006 M3)".into(),
                ));
            }
            return Ok(ClassBodyMember::Property(PropertyDef {
                vis,
                name,
                ty,
                has_get: true,
                has_set: false,
                has_init: false,
                is_required: false,
                get_body: Some(get_body),
                set_body: None,
                get_vis: None,
                set_vis: None,
                modifier,
                is_static_abstract: false,
                attributes: attrs,
                index_params: vec![],
                init: None,
                doc,
            }));
        }

        if self.match_token(&Token::LBrace) {
            let (get_vis, set_vis, has_get, has_set, has_init, get_body, set_body) =
                self.parse_property_accessors_inner()?;
            if is_required && !has_set && !has_init {
                return Err(self.error(
                    "required property",
                    "`required` property must have `set` or `init` accessor (RFC 006 M3)".into(),
                ));
            }
            // 属性初值（C# `T Prop { get; } = expr;`）：`= expr;` 尾随在访问器块后。
            // 仅 auto-property（无访问器体）允许；语义 = backing field 初值，与
            // 表达式体 `=> expr` 不同（初值构造期执行一次，getter 零成本读字段）。
            // 有访问器体的 custom 属性带初值属文法误用（C# CS8050），由 parser 拒绝。
            let init = if !has_get && !has_set && !has_init {
                None
            } else if self.check(&Token::Eq) {
                if get_body.is_some() || set_body.is_some() {
                    return Err(self.error(
                        "property before initializer",
                        "property initializer `= expr` is only allowed on an auto-property (no accessor bodies)".into(),
                    ));
                }
                self.advance();
                let e = self.parse_expr()?;
                self.expect(Token::Semi)?;
                Some(e)
            } else {
                None
            };
            return Ok(ClassBodyMember::Property(PropertyDef {
                vis,
                name,
                ty,
                has_get,
                has_set,
                has_init,
                is_required,
                get_body,
                set_body,
                get_vis,
                set_vis,
                modifier,
                is_static_abstract: false,
                attributes: attrs,
                index_params: vec![],
                init,
                doc,
            }));
        }

        if is_required {
            return Err(self.error(
                "required field",
                "`required` on fields is not supported in RFC 006 M3; use a property".into(),
            ));
        }
        let first_init = if self.match_token(&Token::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        if self.match_token(&Token::Comma) {
            let mut fields = vec![FieldDef {
                vis,
                name,
                ty: ty.clone(),
                is_readonly,
                is_const,
                is_static,
                init: first_init,
                attributes: attrs.clone(),
                doc: doc.clone(),
            }];
            loop {
                let next_name = self.parse_ident()?;
                let next_init = if self.match_token(&Token::Eq) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                fields.push(FieldDef {
                    vis,
                    name: next_name,
                    ty: ty.clone(),
                    is_readonly,
                    is_const,
                    is_static,
                    init: next_init,
                    attributes: attrs.clone(),
                    doc: doc.clone(),
                });
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
            self.expect(Token::Semi)?;
            Ok(ClassBodyMember::MultiField(fields))
        } else {
            self.expect(Token::Semi)?;
            Ok(ClassBodyMember::Field(FieldDef {
                vis,
                name,
                ty,
                is_readonly,
                is_const,
                is_static,
                init: first_init,
                attributes: attrs,
                doc,
            }))
        }
    }

    /// Parse an accessor body: `{ ... }` / `=> … `（自定义）或 `;`（自动）。
    ///
    /// `as_return`：`get => expr;` 脱糖为 `return expr;`；`set => … ` 脱糖为
    /// 赋值或表达式语句（Arc 赋值是语句，须在此路径接受 `lhs = rhs`）。
    fn parse_accessor_body(&mut self, as_return: bool) -> Result<Option<Block>, ParseError> {
        if self.match_token(&Token::LBrace) {
            let body = self.parse_block_inner()?;
            Ok(Some(body))
        } else if self.match_token(&Token::FatArrow) {
            if as_return {
                Ok(Some(self.parse_expr_bodied_return_block()?))
            } else {
                Ok(Some(self.parse_expr_bodied_stmt_block()?))
            }
        } else {
            self.expect(Token::Semi)?;
            Ok(None)
        }
    }

    /// `=> expr;` → `{ return expr; }`（表达式体属性 / get / 非 void 方法）。
    pub(crate) fn parse_expr_bodied_return_block(&mut self) -> Result<Block, ParseError> {
        let expr = self.parse_expr()?;
        self.expect(Token::Semi)?;
        let span = expr.span;
        Ok(Block {
            stmts: vec![Spanned::new(Stmt::Return(Some(expr)), span)],
            tail: None,
        })
    }

    /// `=> stmt-expr;` → 单语句块。支持 `lhs = rhs`（set 访问器）与普通表达式语句。
    pub(crate) fn parse_expr_bodied_stmt_block(&mut self) -> Result<Block, ParseError> {
        let start = self.current_span();
        let expr = self.parse_expr()?;
        self.expect(Token::Semi)?;
        let stmt = match expr.node {
            Expr::Assign { target, value } => Stmt::Assign {
                target: *target,
                value: *value,
            },
            other => Stmt::Expr(Spanned::new(other, expr.span)),
        };
        let end = self.prev_span();
        Ok(Block {
            stmts: vec![Spanned::new(stmt, start.merge(end))],
            tail: None,
        })
    }

    /// 表达式体方法：非 `void` → `return expr;`；`void` → 语句块（可含赋值）。
    pub(crate) fn parse_expr_bodied_method_block(
        &mut self,
        ret: Option<&Spanned<Type>>,
    ) -> Result<Block, ParseError> {
        let is_void = match ret {
            None => true,
            Some(t) => matches!(
                &t.node,
                Type::Named { path, .. }
                    if path.len() == 1 && path[0].as_str() == "void"
            ),
        };
        if is_void {
            self.parse_expr_bodied_stmt_block()
        } else {
            self.parse_expr_bodied_return_block()
        }
    }

    pub(crate) fn parse_interface_member(&mut self) -> Result<InterfaceBodyMember, ParseError> {
        // RFC 017：收集成员级 `///` 文档注释（P1d）。
        let doc = self.collect_doc_comments();
        let attrs = self.parse_attributes()?;
        let _vis = self.parse_vis();
        // RFC 004 M1：检测 `static abstract` 修饰符组合（仅接口成员合法）。
        // 接受 `static abstract` 与 `abstract static` 两种顺序；其他情形走
        // `parse_method_modifier` 单修饰符路径。`is_static_abstract` 标记
        // 透传到 `MethodSig`/`PropertyDef`，供 typeck 跳过实例校验、
        // codegen 拦截器识别。
        let (modifier, is_static_abstract) = self.parse_interface_member_modifier();
        let is_async = self.match_token(&Token::Async);
        let ty = self.parse_type()?;
        // RFC 060：接口索引器 `T this[params] { get; set; }`
        if self.check(&Token::This) {
            self.advance();
            let index_params = self.parse_indexer_params()?;
            self.expect(Token::LBrace)?;
            let (has_get, has_set, has_init, _, _) =
                self.parse_interface_property_accessors_inner()?;
            return Ok(InterfaceBodyMember::Property(PropertyDef {
                vis: Visibility::Public,
                name: "Item".into(),
                ty,
                has_get,
                has_set,
                has_init,
                is_required: false,
                get_body: None,
                set_body: None,
                get_vis: None,
                set_vis: None,
                modifier: MethodModifier::None,
                is_static_abstract,
                attributes: attrs,
                index_params,
                init: None,
                doc,
            }));
        }
        let name = self.parse_ident()?;
        if self.check(&Token::LParen) || self.check(&Token::Lt) {
            let mut sig = self.finish_method_sig_ext(
                Visibility::Public,
                modifier,
                is_static_abstract,
                is_async,
                name,
                Some(ty),
            )?;
            sig.attributes = attrs;
            // 接口方法只存 MethodSig（无 MethodDef），doc 置于 sig.doc。
            sig.doc = doc;
            return Ok(InterfaceBodyMember::Method(sig));
        }
        if self.match_token(&Token::LBrace) {
            let (has_get, has_set, has_init, _, _) =
                self.parse_interface_property_accessors_inner()?;
            return Ok(InterfaceBodyMember::Property(PropertyDef {
                vis: Visibility::Public,
                name,
                ty,
                has_get,
                has_set,
                has_init,
                is_required: false,
                get_body: None,
                set_body: None,
                get_vis: None,
                set_vis: None,
                modifier: MethodModifier::None,
                is_static_abstract,
                attributes: attrs,
                index_params: vec![],
                init: None,
                doc,
            }));
        }
        Err(self.error(
            "method or property",
            "interface member must be `Type Name();`, `Type Name { get; }`, or `Type this[...] { get; }`".into(),
        ))
    }

    /// 类属性/索引器访问器体（{ 已消费）。RFC 006：识别 init（M1 与 auto ;）；支持 get => expr; / set => lhs = value;。
    /// RFC 006 A1：每个访问器前可选项性地出现可见性关键字（per-accessor 可见性）。
    /// 返回值：`(get_vis, set_vis, has_get, has_set, has_init, get_body, set_body)`；
    /// `get_vis`/`set_vis` 为 None 表示未显式声明，继承属性自身可见性（C# 默认）。
    fn parse_property_accessors_inner(
        &mut self,
    ) -> Result<
        (
            Option<Visibility>,
            Option<Visibility>,
            bool,
            bool,
            bool,
            Option<Block>,
            Option<Block>,
        ),
        ParseError,
    > {
        let mut get_vis = None;
        let mut set_vis = None;
        let mut has_get = false;
        let mut has_set = false;
        let mut has_init = false;
        let mut get_body = None;
        let mut set_body = None;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // RFC 006 A1：访问器级可见性（可选）。仅在显式出现可见性关键字时才记录 Some，
            // 否则 None（继承属性自身可见性）。不能用 parse_vis() 判断——无修饰符时它
            // 返回 Visibility::Private（成员默认），无法区分「未出现」与「显式 private」。
            let accessor_vis = if self.check(&Token::Public)
                || self.check(&Token::Private)
                || self.check(&Token::Protected)
                || self.check(&Token::Internal)
            {
                Some(self.parse_vis())
            } else {
                None
            };
            let accessor = self.parse_ident()?;
            match accessor.as_str() {
                "get" => {
                    has_get = true;
                    if accessor_vis.is_some() {
                        get_vis = accessor_vis;
                    }
                    get_body = self.parse_accessor_body(true)?;
                }
                "set" => {
                    has_set = true;
                    if accessor_vis.is_some() {
                        set_vis = accessor_vis;
                    }
                    set_body = self.parse_accessor_body(false)?;
                }
                "init" => {
                    // RFC 006 M2：允许自定义 `init { … }`（体存入 set_body，与 set 互斥）。
                    let body = self.parse_accessor_body(false)?;
                    has_init = true;
                    if accessor_vis.is_some() {
                        set_vis = accessor_vis;
                    }
                    if body.is_some() {
                        set_body = body;
                    }
                }
                _ => {
                    return Err(
                        self.error("get, set, or init", format!("found accessor `{accessor}`"))
                    );
                }
            }
        }
        self.expect(Token::RBrace)?;
        if has_set && has_init {
            return Err(self.error(
                "set or init",
                "property cannot declare both `set` and `init` accessors".into(),
            ));
        }
        Ok((
            get_vis, set_vis, has_get, has_set, has_init, get_body, set_body,
        ))
    }

    /// 接口属性/索引器访问器（仅 `get;` / `set;`，`{` 已消费）。
    /// RFC 069：接口 `init` 后置（M1 硬拒绝）。
    fn parse_interface_property_accessors_inner(
        &mut self,
    ) -> Result<(bool, bool, bool, Option<Block>, Option<Block>), ParseError> {
        let mut has_get = false;
        let mut has_set = false;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // RFC 006 A1：C# 接口属性不允许访问器级可见性修饰，直接不支持，遇显式修饰报错。
            if self.check(&Token::Public)
                || self.check(&Token::Private)
                || self.check(&Token::Protected)
                || self.check(&Token::Internal)
            {
                return Err(self.error(
                    "get or set",
                    "interface property accessors cannot have per-accessor visibility (RFC 006 A1)"
                        .into(),
                ));
            }
            let accessor = self.parse_ident()?;
            match accessor.as_str() {
                "get" => has_get = true,
                "set" => has_set = true,
                "init" => {
                    return Err(self.error(
                        "get or set",
                        "interface `init` accessors are not supported in RFC 006 M1".into(),
                    ));
                }
                _ => {
                    return Err(self.error("get or set", format!("found accessor `{accessor}`")));
                }
            }
            self.match_token(&Token::Semi);
        }
        self.expect(Token::RBrace)?;
        Ok((has_get, has_set, false, None, None))
    }

    /// RFC 004 M1：解析接口成员修饰符，识别 `static abstract` 组合。
    ///
    /// 返回 `(modifier, is_static_abstract)`：
    /// - `static abstract` 或 `abstract static` → `(Static, true)`
    /// - `static` → `(Static, false)`
    /// - `abstract` → `(Abstract, false)`
    /// - `virtual`/`override` → 对应变体
    /// - 无修饰符 → `(None, false)`
    ///
    /// `static abstract` 仅在接口成员位置合法；类成员位置由
    /// `parse_method_modifier` 处理（不识别组合）。
    fn parse_interface_member_modifier(&mut self) -> (MethodModifier, bool) {
        // `static abstract` 顺序
        if self.check(&Token::Static) && self.check_at(1, &Token::Abstract) {
            self.advance(); // consume Static
            self.advance(); // consume Abstract
            return (MethodModifier::Static, true);
        }
        // `abstract static` 顺序（少见但合法）
        if self.check(&Token::Abstract) && self.check_at(1, &Token::Static) {
            self.advance(); // consume Abstract
            self.advance(); // consume Static
            return (MethodModifier::Static, true);
        }
        (self.parse_method_modifier(), false)
    }

    /// RFC 003：解析 `operator ⊕(params)` 并归一为 `op_*` 静态方法。
    fn parse_operator_method(
        &mut self,
        vis: Visibility,
        modifier: MethodModifier,
        is_async: bool,
        is_required: bool,
        ty: Spanned<Type>,
        attrs: Vec<Attribute>,
        doc: Option<String>,
    ) -> Result<ClassBodyMember, ParseError> {
        self.expect(Token::Operator)?;
        if is_required {
            return Err(self.error(
                "operator",
                "`required` cannot modify an operator (RFC 003)".into(),
            ));
        }
        if is_async {
            return Err(self.error(
                "operator",
                "`async` cannot modify an operator (RFC 003)".into(),
            ));
        }
        if modifier != MethodModifier::Static {
            return Err(self.error(
                "operator",
                "user operators must be `static` (RFC 003)".into(),
            ));
        }
        // 硬拒绝复合赋值 / true/false / 未列符号
        match &self.peek().token {
            Token::PlusEq
            | Token::MinusEq
            | Token::StarEq
            | Token::SlashEq
            | Token::BitOrEq
            | Token::BitAndEq
            | Token::BitXorEq => {
                return Err(self.error(
                    "operator",
                    "user `operator+=` / `-=` / `*=` / `/=` / `|=` / `&=` / `^=` is hard-rejected; \
                     declare `operator +` (etc.) and use builtin compound assign (RFC 003 / 076)"
                        .into(),
                ));
            }
            Token::True | Token::False => {
                return Err(self.error(
                    "operator",
                    "`operator true` / `operator false` is hard-rejected (RFC 003)".into(),
                ));
            }
            Token::Ident(_) => {
                return Err(self.error(
                    "operator",
                    "conversion operators (`operator Type`) are hard-rejected (RFC 003)".into(),
                ));
            }
            _ => {}
        }
        let op_tok = self.peek().token.clone();
        self.advance();
        if self.check(&Token::Lt) {
            return Err(self.error(
                "operator",
                "generic operators are not supported in RFC 003 M1".into(),
            ));
        }
        let params = self.parse_params_after_name()?;
        let name = map_operator_method_name(&op_tok, params.len())
            .map_err(|msg| self.error("operator", msg))?;
        let where_clause = self.parse_where_clause()?;
        if !where_clause.is_empty() {
            return Err(self.error(
                "operator",
                "`where` on operators is not supported in RFC 003 M1".into(),
            ));
        }
        let sig = MethodSig {
            vis,
            name: name.into(),
            generics: vec![],
            where_clause,
            params,
            ret: Some(ty),
            is_async: false,
            modifier: MethodModifier::Static,
            attributes: attrs,
            is_static_abstract: false,
            doc: None,
        };
        let body = if self.match_token(&Token::LBrace) {
            Some(self.parse_block_inner()?)
        } else {
            self.expect(Token::Semi)?;
            None
        };
        Ok(ClassBodyMember::Method(MethodDef { sig, body, doc }))
    }
}

/// RFC 003：运算符 token + 形参个数 → `op_*` 方法名。
fn map_operator_method_name(op: &Token, arity: usize) -> Result<&'static str, String> {
    match (op, arity) {
        (Token::Plus, 2) => Ok("op_Addition"),
        (Token::Minus, 2) => Ok("op_Subtraction"),
        (Token::Minus, 1) => Ok("op_UnaryNegation"),
        (Token::Star, 2) => Ok("op_Multiply"),
        (Token::Slash, 2) => Ok("op_Division"),
        (Token::Percent, 2) => Ok("op_Modulus"),
        (Token::EqEq, 2) => Ok("op_Equality"),
        (Token::NotEq, 2) => Ok("op_Inequality"),
        (
            Token::Plus | Token::Star | Token::Slash | Token::Percent | Token::EqEq | Token::NotEq,
            _,
        ) => Err(format!(
            "operator requires exactly 2 parameters (found {arity}); RFC 003"
        )),
        (Token::Minus, _) => Err(format!(
            "operator `-` requires 1 (unary) or 2 (binary) parameters (found {arity}); RFC 003"
        )),
        _ => Err(
            "unsupported operator symbol; RFC 003 M1 allows + - * / % == != and unary - only"
                .into(),
        ),
    }
}
