use super::*;

impl Parser {
    /// Parse zero or more `[Attr("arg")]` attributes at declaration position.
    ///
    /// At item/member start, `[` can only be an attribute (not a collection
    /// expression or index), so positional disambiguation is sufficient.
    pub(crate) fn parse_attributes(&mut self) -> Result<Vec<Attribute>, ParseError> {
        let mut attrs = Vec::new();
        while self.check(&Token::LBracket) {
            self.expect(Token::LBracket)?;
            let path = self.parse_dotted_path()?;
            let mut args = Vec::new();
            if self.match_token(&Token::LParen) {
                if !self.check(&Token::RParen) {
                    loop {
                        let arg = self.parse_attribute_arg()?;
                        // RFC 012 M3：支持 `|` 常量折叠（如
                        // `AttributeTargets.Class | AttributeTargets.Struct`）。
                        // parser 层把两端 MemberPath / Int 折叠为单个 Int；
                        // 非 MemberPath/Int 的 `|` 操作数报错（避免伪装成
                        // 通用表达式求值器）。
                        let arg = self.try_fold_bit_or(arg)?;
                        args.push(arg);
                        if !self.match_token(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Token::RParen)?;
            }
            self.expect(Token::RBracket)?;
            attrs.push(Attribute { path, args });
        }
        Ok(attrs)
    }

    /// RFC 012 M3：解析单个属性参数。
    ///
    /// 支持四种形式：
    /// - 字面量：`"str"` / `42` / `true` / `false`
    /// - 命名参数：`Label = "value"`（必须出现在位置参数之后，由调用方保证）
    /// - 类型引用：`typeof(Type)`
    /// - 成员路径常量：`AttributeTargets.Class`（用于 `[AttributeUsage]`）
    /// - RFC 009 M4-7：Lambda 表达式：`x => ...` / `(x) => ...` / `() => ...`
    ///   （用于宏特性派生类构造函数的 `Expression<T>` 形参）
    fn parse_attribute_arg(&mut self) -> Result<AttributeArg, ParseError> {
        let token = self.peek().token.clone();
        // 1. 字面量（保持 M1 行为）
        let arg = match &token {
            Token::StringLit(s) | Token::VerbatimString(s) => {
                self.advance();
                AttributeArg::String(s.clone())
            }
            Token::IntLit(n) => {
                self.advance();
                AttributeArg::Int(*n)
            }
            Token::True => {
                self.advance();
                AttributeArg::Bool(true)
            }
            Token::False => {
                self.advance();
                AttributeArg::Bool(false)
            }
            // 2. 类型引用：`typeof(Type)`
            Token::TypeOf => {
                self.advance();
                self.expect(Token::LParen)?;
                let ty = self.parse_type()?;
                self.expect(Token::RParen)?;
                AttributeArg::Type(ty)
            }
            // RFC 012 M4-7: 带括号 Lambda `(params) => body` / `() => body`
            // 先用 check_lambda_at_lparen 扫描到匹配 RParen 后是否紧跟 FatArrow
            Token::LParen if self.check_lambda_at_lparen() => {
                let start = self.peek().span;
                self.expect(Token::LParen)?;
                let spanned = self.parse_lambda(start, false, false)?;
                let Expr::Lambda(lambda) = spanned.node else {
                    return Err(self.error("lambda expression", self.describe_current()));
                };
                AttributeArg::Lambda(lambda)
            }
            // 3. 命名参数、成员路径、或单参数无括号 Lambda：均以 Ident 起始
            Token::Ident(_) => {
                if self.peek_is_named_arg() {
                    // `Ident = <value>`
                    let name = self.parse_ident()?;
                    self.expect(Token::Eq)?;
                    let value = self.parse_attribute_arg()?;
                    AttributeArg::Named {
                        name,
                        value: Box::new(value),
                    }
                } else if self.peek_is_single_param_lambda() {
                    // RFC 009 M4-7: `Ident => body`（单参数无括号 Lambda）
                    let start = self.peek().span;
                    let name = self.parse_ident()?;
                    self.expect(Token::FatArrow)?;
                    let spanned =
                        self.parse_lambda_from_param(start, name.to_string(), false, false)?;
                    let Expr::Lambda(lambda) = spanned.node else {
                        return Err(self.error("lambda expression", self.describe_current()));
                    };
                    AttributeArg::Lambda(lambda)
                } else {
                    // `Ident(.Ident)*` — 成员路径
                    let path = self.parse_dotted_path()?;
                    AttributeArg::MemberPath(path)
                }
            }
            _ => {
                return Err(self.error(
                    "string, int, bool, typeof(Type), Ident = value, Ident.Ident, or lambda",
                    self.describe_current(),
                ));
            }
        };
        Ok(arg)
    }

    /// RFC 009 M4-7: 检测当前位置是否为单参数无括号 Lambda `Ident => body`。
    ///
    /// 规则：当前 token 是 `Ident`，下一个 token 是 `FatArrow`。
    /// 注意：`Ident = value`（命名参数）由 `peek_is_named_arg` 先识别，
    /// 不会进入此分支。
    fn peek_is_single_param_lambda(&self) -> bool {
        matches!(
            self.tokens.get(self.pos).map(|t| &t.token),
            Some(Token::Ident(_))
        ) && matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::FatArrow)
        )
    }

    /// RFC 009 M4-7: 检测从当前 `(` 起始是否为带括号 Lambda `(params) => body`。
    ///
    /// 扫描从 `(` 到匹配的 `)`，检查紧随 `)` 之后是否为 `FatArrow`。
    /// 处理嵌套括号（如 `(x: List<int>) => ...` 中的泛型参数 `<>` 不影响
    /// 括号匹配——type 内的 `<>` 不增加 paren depth）。
    fn check_lambda_at_lparen(&self) -> bool {
        if !matches!(self.peek().token, Token::LParen) {
            return false;
        }
        let mut depth: i32 = 1;
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].token {
                Token::LParen => depth += 1,
                Token::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        // 检查 `)` 之后是否是 `=>`
                        return self
                            .tokens
                            .get(i + 1)
                            .map(|t| matches!(t.token, Token::FatArrow))
                            .unwrap_or(false);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// RFC 012 M3：当前位置是否为命名参数（`Ident =`）。
    ///
    /// 用于在 `parse_attribute_arg` 中区分 `Label = "x"`（命名参数）与
    /// `AttributeTargets.Class`（成员路径）。两者均以 Ident 起始，需 lookahead
    /// 下一 token 是否为 `=`。
    fn peek_is_named_arg(&self) -> bool {
        matches!(
            self.tokens.get(self.pos).map(|t| &t.token),
            Some(Token::Ident(_))
        ) && matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Eq)
        )
    }

    /// RFC 012 M3：尝试折叠 `<arg> | <arg> | ...` 为单个 Int。
    ///
    /// 仅支持两端均为 `MemberPath` 或 `Int` 的 `|` 操作（用于
    /// `AttributeTargets.A | AttributeTargets.B`）。其他类型的 `|` 报错。
    /// MemberPath 的实际求值（如 `AttributeTargets.Class → 1`）由 typeck
    /// 完成；此处仅在两端均为 MemberPath 时返回特殊的组合 MemberPath，
    /// typeck 再统一解析。但为简化设计，本实现要求两端为 MemberPath 时
    /// 报错——用户应直接使用 typeck 已识别的 `AttributeTargets.X` 单值。
    ///
    /// **当前实现**：若下一 token 是 `|`，要求左侧为 Int 或 MemberPath，
    /// 折叠为 Int（MemberPath 由 typeck 解析为 int 后再折叠）。为避免 parser
    /// 依赖 typeck 语义，此处对 MemberPath 间的 `|` 不做求值，而是保留
    /// 左侧 MemberPath 不变，把右侧 MemberPath 重新组合——但 AST 层无此变体。
    ///
    /// **决策**：parser 不做 MemberPath 折叠。`A | B` 语法在 M3 暂不支持，
    /// 待 enum + `|` 运算符正式落地后由 typeck 求值。当前用户应使用单值
    /// `AttributeTargets.All` 或整数字面量。
    fn try_fold_bit_or(&mut self, left: AttributeArg) -> Result<AttributeArg, ParseError> {
        if !self.check(&Token::BitOr) {
            return Ok(left);
        }
        // 消耗 `|` 并解析右侧，但报错——M3 不支持 `|` 组合语法
        let _ = self.match_token(&Token::BitOr);
        let _right = self.parse_attribute_arg()?;
        Err(self.error(
            "single attribute argument (bitwise OR `|` combinations not yet supported — use AttributeTargets.All or integer literal)",
            "`A | B` syntax".to_string(),
        ))
    }

    /// 成员可见性：无修饰符默认 `private`（C# 成员惯例）。
    pub(crate) fn parse_vis(&mut self) -> Visibility {
        self.parse_vis_with_default(Visibility::Private)
    }

    /// 顶层类型可见性：无修饰符默认 `internal`（RFC 006 D3 / RFC 009 B2）。
    pub(crate) fn parse_item_vis(&mut self) -> Visibility {
        self.parse_vis_with_default(Visibility::Internal)
    }

    fn parse_vis_with_default(&mut self, default: Visibility) -> Visibility {
        if self.match_token(&Token::Public) {
            Visibility::Public
        } else if self.match_token(&Token::Private) {
            Visibility::Private
        } else if self.match_token(&Token::Protected) {
            Visibility::Protected
        } else if self.match_token(&Token::Internal) {
            Visibility::Internal
        } else {
            default
        }
    }

    /// RFC 006：解析字段修饰符，返回 `(is_const, is_readonly, is_static)`。
    ///
    /// `static` 与 `const` 互斥（const 隐含 static）；`static` 与 `readonly` 可组合。
    /// 顺序灵活：`static readonly`、`readonly static`、`static`、`readonly`、`const` 均合法。
    /// `static const` 组合报错。
    ///
    /// 注意：`parse_class_body_member` 中此函数先于 `parse_method_modifier` 调用，
    /// 会消费 `static` 关键字；当成员最终是方法时，调用方需根据 `is_static`
    /// 设置 `MethodModifier::Static`（见 item_body.rs）。
    pub(crate) fn parse_field_modifier(&mut self) -> Result<(bool, bool, bool), ParseError> {
        let mut is_static = false;
        let mut is_const = false;
        let mut is_readonly = false;
        loop {
            if self.match_token(&Token::Static) {
                if is_const {
                    return Err(self.error(
                        "field modifier",
                        "`static` after `const` (const 隐含 static，禁止冗余修饰)".into(),
                    ));
                }
                is_static = true;
            } else if self.match_token(&Token::Const) {
                if is_static {
                    return Err(self.error(
                        "field modifier",
                        "`const` after `static` (const 隐含 static，禁止冗余修饰)".into(),
                    ));
                }
                is_const = true;
            } else if self.match_token(&Token::Readonly) {
                is_readonly = true;
            } else {
                break;
            }
        }
        Ok((is_const, is_readonly, is_static))
    }

    pub(crate) fn parse_generics(&mut self) -> Result<Vec<GenericParam>, ParseError> {
        self.parse_generics_inner(false)
    }

    /// RFC 009 P1-C2：接口允许 `in`/`out`；class/struct/method 禁止。
    pub(crate) fn parse_interface_generics(&mut self) -> Result<Vec<GenericParam>, ParseError> {
        self.parse_generics_inner(true)
    }

    fn parse_generics_inner(
        &mut self,
        allow_variance: bool,
    ) -> Result<Vec<GenericParam>, ParseError> {
        if !self.match_token(&Token::Lt) {
            return Ok(vec![]);
        }
        let mut params = Vec::new();
        loop {
            let variance = if self.match_token(&Token::Out) {
                if !allow_variance {
                    return Err(ParseError::Unexpected {
                        span: self.prev_span(),
                        expected: "type parameter name".into(),
                        found: "`out` (variance only on interface type parameters)".into(),
                    });
                }
                Variance::Covariant
            } else if self.match_token(&Token::In) {
                if !allow_variance {
                    return Err(ParseError::Unexpected {
                        span: self.prev_span(),
                        expected: "type parameter name".into(),
                        found: "`in` (variance only on interface type parameters)".into(),
                    });
                }
                Variance::Contravariant
            } else {
                Variance::Invariant
            };
            params.push(GenericParam {
                name: self.parse_ident()?,
                variance,
            });
            if !self.match_token(&Token::Comma) {
                break;
            }
        }
        self.expect_gt_close()?;
        Ok(params)
    }

    /// Parse a `where` clause: `where T : IComparable, U : IEnumerable<int>`.
    ///
    /// Returns an empty vec if the next token is not `where`. Each constraint
    /// is `param : <bound>` where `<bound>` 可为：
    /// - 类型（接口/基类）：`where T : IComparable<T>`
    /// - `class` 元约束：`where T : class`
    /// - `struct` 元约束：`where T : struct`
    /// - `new()` 构造约束：`where T : new()`
    ///
    /// 支持同 param 的多约束组合（C# 规范）：`where T : class, new()`。
    /// 逗号后若接 `ident + colon` 则视为新 param，否则为同 param 下一约束。
    /// `new()` 必须是同 param 的最后一个约束（C# 规范强制）。
    pub(crate) fn parse_where_clause(&mut self) -> Result<Vec<TypeConstraint>, ParseError> {
        if !self.check(&Token::Where) {
            return Ok(vec![]);
        }
        let mut constraints = Vec::new();
        // RFC 009 P1-C2：支持多个 `where` 关键字（`where T : A where U : B`）。
        while self.match_token(&Token::Where) {
            loop {
                let param = self.parse_ident()?;
                self.expect(Token::Colon)?;
                let group_start = constraints.len();
                loop {
                    let kind = self.parse_constraint_kind()?;
                    constraints.push(TypeConstraint {
                        param: param.clone(),
                        kind,
                    });
                    if !self.match_token(&Token::Comma) {
                        self.validate_new_constraint_last(&constraints[group_start..])?;
                        break;
                    }
                    if self.peek_is_ident_followed_by_colon() {
                        break;
                    }
                }
                self.validate_new_constraint_last(&constraints[group_start..])?;
                // 下一 token 是 `where` → 外层 while；否则若仍是 `ident :` 则继续同子句多 param
                if self.check(&Token::Where) {
                    break;
                }
                if self.peek_is_ident_followed_by_colon() {
                    continue;
                }
                break;
            }
        }
        Ok(constraints)
    }

    /// 校验同 param 的约束组中 `new()` 必须是最后一个约束（C# 规范）。
    ///
    /// 若 `new()` 后还有其他约束则报错。
    fn validate_new_constraint_last(&self, group: &[TypeConstraint]) -> Result<(), ParseError> {
        let mut found_new = false;
        for c in group {
            if matches!(c.kind, ConstraintKind::New) {
                found_new = true;
            } else if found_new {
                return Err(ParseError::Unexpected {
                    span: self.current_span(),
                    expected: "new() constraint must be the last constraint in a where clause"
                        .into(),
                    found: "additional constraint after new()".into(),
                });
            }
        }
        Ok(())
    }

    /// 当前位置是否为 `ident :` 模式（用于 where 子句中区分新 param 与同 param 下一约束）。
    fn peek_is_ident_followed_by_colon(&self) -> bool {
        matches!(
            self.tokens.get(self.pos).map(|t| &t.token),
            Some(Token::Ident(_))
        ) && matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.token),
            Some(Token::Colon)
        )
    }

    /// 解析单个约束种类。
    ///
    /// 优先识别 `class`/`struct`/`new()` 元约束关键字；否则按类型解析（接口/基类）。
    fn parse_constraint_kind(&mut self) -> Result<ConstraintKind, ParseError> {
        if self.match_token(&Token::Class) {
            return Ok(ConstraintKind::Class);
        }
        if self.match_token(&Token::Struct) {
            return Ok(ConstraintKind::Struct);
        }
        // new() 构造约束：`new` 关键字后接 `()`
        if self.match_token(&Token::New) {
            self.expect(Token::LParen)?;
            self.expect(Token::RParen)?;
            return Ok(ConstraintKind::New);
        }
        let bound = self.parse_type()?;
        Ok(ConstraintKind::Type(bound))
    }

    pub(crate) fn parse_struct(&mut self) -> Result<StructDef, ParseError> {
        let vis = self.parse_item_vis();
        let is_readonly = self.match_token(&Token::Readonly);
        self.expect(Token::Struct)?;
        let name = self.parse_ident()?;
        let struct_name = name.clone();
        let generics = self.parse_generics()?;
        // 结构体可通过 : 实现接口（如 IEquatable<T>）
        let mut bases = Vec::new();
        if self.match_token(&Token::Colon) {
            loop {
                bases.push(self.parse_type()?.node);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        let where_clause = self.parse_where_clause()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut constructors = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.current_span();
            // 结构体方法不允许 virtual/override/abstract 修饰符
            if matches!(
                self.peek().token,
                Token::Virtual | Token::Override | Token::Abstract
            ) {
                return Err(self.error(
                    "struct member",
                    format!(
                        "struct cannot have virtual/override/abstract members (found `{:?}`)",
                        self.peek().token
                    ),
                ));
            }
            match self.parse_class_body_member(&struct_name)? {
                ClassBodyMember::Field(f) => fields.push(f),
                ClassBodyMember::MultiField(fs) => fields.extend(fs),
                ClassBodyMember::Property(p) => properties.push(p),
                ClassBodyMember::Method(m) => {
                    methods.push(Spanned::new(m, start.merge(self.prev_span())));
                }
                ClassBodyMember::Constructor(c) => {
                    constructors.push(Spanned::new(c, start.merge(self.prev_span())));
                }
            }
        }
        self.expect(Token::RBrace)?;
        Ok(StructDef {
            vis,
            is_readonly,
            is_record: false,
            name,
            generics,
            where_clause,
            fields,
            bases,
            properties,
            methods,
            constructors,
            attributes: vec![],
            doc: None,
        })
    }

    pub(crate) fn parse_class(&mut self) -> Result<ClassDef, ParseError> {
        let vis = self.parse_item_vis();
        // C# `abstract class` — `abstract` 是专用 Token（Token::Abstract），
        // 由 parse_item 顶层路由到 parse_class。此处仅消费该 token 并设置
        // is_abstract 标志。RFC 012 M4-1：GenerateToAttribute<T> 标记为
        // abstract，强制用户派生。
        let is_abstract = self.match_token(&Token::Abstract);
        // RFC 037：识别 `partial` 上下文关键字。
        //
        // `partial` 是上下文关键字——仅在 `class` 声明前（可前缀 `static`）作为
        // 修饰符识别；其他位置作为普通标识符。识别规则：当前 token 为
        // `Ident("partial")` 且下一 token 为 `Static` 或 `Class`。
        let is_partial = self.match_ident_keyword("partial", &[Token::Static, Token::Class]);
        let is_static = self.match_token(&Token::Static);
        self.expect(Token::Class)?;
        let name = self.parse_ident()?;
        let class_name = name.clone();
        let generics = self.parse_generics()?;
        // RFC 009 L1 完整关账：primary constructor 最小子集。
        // `class C(int x)` — 类名后、基类列表前的参数列表；与 record 解耦。
        // 解析后立即脱糖为 private 字段捕获 + 合成构造函数（见
        // `apply_primary_constructor`）。
        let primary_params = if self.check(&Token::LParen) {
            if is_static {
                return Err(self.error(
                    "class body",
                    "static class cannot have a primary constructor".into(),
                ));
            }
            Some(self.parse_params_after_name()?)
        } else {
            None
        };
        let mut bases = Vec::new();
        // Primary：`class D(int x) : Base(x)` — 基类构造实参写入合成 ctor 的
        // `base_args`（复用显式构造器 `: base(args)` 管线）。无 primary 时禁止。
        let mut primary_base_args: Option<Vec<Spanned<Expr>>> = None;
        if is_static && self.check(&Token::Colon) {
            return Err(self.error(
                "static class body",
                "static class cannot inherit — remove base list after `:`".into(),
            ));
        }
        if self.match_token(&Token::Colon) {
            loop {
                let base_ty = self.parse_type()?.node;
                if self.check(&Token::LParen) {
                    if primary_params.is_none() {
                        return Err(self.error(
                            "primary constructor",
                            "base constructor arguments (`: Base(...)`) require a primary constructor \
                             (use an explicit constructor with `: base(...)` instead)"
                                .into(),
                        ));
                    }
                    if primary_base_args.is_some() {
                        return Err(self.error(
                            "base list",
                            "only one base class constructor call is allowed in a primary constructor"
                                .into(),
                        ));
                    }
                    self.expect(Token::LParen)?;
                    primary_base_args = Some(self.parse_call_args()?);
                }
                bases.push(base_ty);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        let where_clause = self.parse_where_clause()?;
        self.expect(Token::LBrace)?;
        let mut fields = Vec::new();
        let mut properties = Vec::new();
        let mut methods = Vec::new();
        let mut constructors = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.current_span();
            match self.parse_class_body_member(&class_name)? {
                ClassBodyMember::Field(f) => fields.push(f),
                ClassBodyMember::MultiField(fs) => fields.extend(fs),
                ClassBodyMember::Property(p) => properties.push(p),
                ClassBodyMember::Method(m) => {
                    methods.push(Spanned::new(m, start.merge(self.prev_span())));
                }
                ClassBodyMember::Constructor(c) => {
                    constructors.push(Spanned::new(c, start.merge(self.prev_span())));
                }
            }
        }
        self.expect(Token::RBrace)?;
        if let Some(params) = primary_params {
            self.apply_primary_constructor(
                vis,
                &params,
                primary_base_args,
                &mut fields,
                &properties,
                &mut constructors,
            )?;
        }
        Ok(ClassDef {
            vis,
            is_static,
            is_abstract,
            is_partial,
            is_record: false,
            name,
            generics,
            where_clause,
            bases,
            fields,
            properties,
            methods,
            constructors,
            attributes: vec![],
            doc: None,
            synthesized_host: None,
        })
    }

    /// Primary constructor 脱糖（RFC 009 L1；含 `: Base(args)` 与 `ref`/`out`/`in`）。
    ///
    /// `class C(T x, U y) { … }` →
    /// - **按值**参数：注入同名 `private` 字段 + `this.x = x;`（字段捕获）
    /// - **`ref`/`out`/`in`**：保留在合成 ctor 形参上，**禁止**字段捕获（对齐 C# CS9109）
    /// - 若声明侧写了 `: Base(args)`，合成 ctor 的 `base_args` 填入实参
    ///
    /// 仍排除：扩展接收者 `this`；按值参数与同名字段/属性冲突；同 arity 显式构造；
    /// `record` / `struct` primary。
    fn apply_primary_constructor(
        &self,
        class_vis: Visibility,
        params: &[Param],
        base_args: Option<Vec<Spanned<Expr>>>,
        fields: &mut Vec<FieldDef>,
        properties: &[PropertyDef],
        constructors: &mut Vec<Spanned<ConstructorDef>>,
    ) -> Result<(), ParseError> {
        for p in params {
            if p.is_extension_receiver {
                return Err(self.error(
                    "primary constructor parameter without `this`",
                    format!("extension receiver `this` is not allowed on primary constructor parameter `{}`", p.name),
                ));
            }
            // C#：`ref`/`out`/`in` 不可捕获为字段，故不与同名字段/属性冲突。
            let by_ref = p.is_ref || p.is_out || p.is_in;
            if !by_ref {
                if fields.iter().any(|f| f.name == p.name) {
                    return Err(self.error(
                        "unique primary constructor parameter name",
                        format!(
                            "primary constructor parameter `{}` conflicts with a field",
                            p.name
                        ),
                    ));
                }
                if properties.iter().any(|prop| prop.name == p.name) {
                    return Err(self.error(
                        "unique primary constructor parameter name",
                        format!(
                            "primary constructor parameter `{}` conflicts with a property",
                            p.name
                        ),
                    ));
                }
            }
        }
        let arity = params.len();
        if constructors.iter().any(|c| c.node.params.len() == arity) {
            return Err(self.error(
                "unique constructor signature",
                format!(
                    "primary constructor conflicts with an explicit constructor of arity {arity}"
                ),
            ));
        }

        let mut captured: Vec<FieldDef> = Vec::new();
        let mut stmts: Vec<Spanned<Stmt>> = Vec::new();
        for p in params {
            if p.is_ref || p.is_out || p.is_in {
                // 不捕获：仅出现在合成 ctor 形参；可用于字段初始化器 / `: Base(args)`。
                continue;
            }
            captured.push(FieldDef {
                vis: Visibility::Private,
                name: p.name.clone(),
                ty: p.ty.clone(),
                is_readonly: false,
                is_const: false,
                is_static: false,
                init: None,
                attributes: vec![],
                doc: None,
            });
            let target = Spanned::new(
                Expr::Field {
                    receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                    field: p.name.clone(),
                },
                Span::DUMMY,
            );
            let value = Spanned::new(Expr::Ident(p.name.clone()), Span::DUMMY);
            stmts.push(Spanned::new(Stmt::Assign { target, value }, Span::DUMMY));
        }
        fields.splice(0..0, captured);
        constructors.insert(
            0,
            Spanned::new(
                ConstructorDef {
                    vis: class_vis,
                    params: params.to_vec(),
                    body: Block { stmts, tail: None },
                    base_args,
                    doc: None,
                },
                Span::DUMMY,
            ),
        );
        Ok(())
    }

    /// RFC 006：解析 `record` / `record struct` 声明。
    ///
    /// 位置参数在本函数内脱糖为字段 + 构造器；`record` → `Item::Class`，
    /// `record struct` → `Item::Struct`。拒绝：`record class`（RFC 002 单一惯用法）、
    /// `: Base(args)`、与 `partial` 组合。
    pub(crate) fn parse_record(&mut self) -> Result<Item, ParseError> {
        let vis = self.parse_item_vis();
        self.expect(Token::Record)?;
        let is_struct = self.match_token(&Token::Struct);
        // RFC 002：`record class` 是 C# 兼容同义拼写，Arc 收口单一惯用法 → 硬拒。
        if !is_struct && self.check(&Token::Class) {
            return Err(self.error(
                "record class",
                "`record class` is not supported (RFC 002 single idiom); use `record` or `record struct`".into(),
            ));
        }
        let name = self.parse_ident()?;
        let record_name = name.clone();
        let generics = self.parse_generics()?;

        let mut positional_props = Vec::new();
        let mut positional_ctors = Vec::new();
        if self.check(&Token::LParen) {
            let param_span = self.current_span();
            let params = self.parse_record_positional_params()?;
            let end = self.prev_span();
            let span = param_span.merge(end);
            let (props, ctor) = desugar_record_positional(&record_name, params, span);
            positional_props = props;
            positional_ctors.push(ctor);
        }
        // 位置属性快照：Deconstruct 仅覆盖位置参数（与 C# 一致；体字段不进入）
        let deconstruct_members = positional_props
            .iter()
            .map(|p| (p.name.clone(), p.ty.clone()))
            .collect::<Vec<_>>();

        let mut bases = Vec::new();
        if self.match_token(&Token::Colon) {
            if is_struct {
                // record struct 可实现接口，但不支持类继承形态的 `: Base(args)`
                loop {
                    let base_ty = self.parse_type()?.node;
                    if self.check(&Token::LParen) {
                        return Err(self.error(
                            "record struct base list",
                            "record struct cannot use base constructor arguments (`: Base(...)`) (RFC 006)".into(),
                        ));
                    }
                    bases.push(base_ty);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
            } else {
                loop {
                    // `Base(args)` — C# record 继承惯用形式，依赖 primary ctor，硬拒绝
                    let base_ty = self.parse_type()?.node;
                    if self.check(&Token::LParen) {
                        return Err(self.error(
                            "record base list",
                            "record inheritance with base constructor arguments (`: Base(...)`) is not supported yet (RFC 006); use `: Base` without arguments, or an explicit constructor".into(),
                        ));
                    }
                    bases.push(base_ty);
                    if !self.match_token(&Token::Comma) {
                        break;
                    }
                }
            }
        }
        let where_clause = self.parse_where_clause()?;

        let mut fields = Vec::new();
        let mut properties = positional_props;
        let mut methods = Vec::new();
        let mut constructors = positional_ctors;

        if self.match_token(&Token::Semi) {
            // `record Point(int X, int Y);` — 无体
        } else {
            self.expect(Token::LBrace)?;
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let start = self.current_span();
                if is_struct
                    && matches!(
                        self.peek().token,
                        Token::Virtual | Token::Override | Token::Abstract
                    )
                {
                    return Err(self.error(
                        "record struct member",
                        format!(
                            "record struct cannot have virtual/override/abstract members (found `{:?}`)",
                            self.peek().token
                        ),
                    ));
                }
                match self.parse_class_body_member(&record_name)? {
                    ClassBodyMember::Field(f) => fields.push(f),
                    ClassBodyMember::MultiField(fs) => fields.extend(fs),
                    ClassBodyMember::Property(p) => properties.push(p),
                    ClassBodyMember::Method(m) => {
                        methods.push(Spanned::new(m, start.merge(self.prev_span())));
                    }
                    ClassBodyMember::Constructor(c) => {
                        constructors.push(Spanned::new(c, start.merge(self.prev_span())));
                    }
                }
            }
            self.expect(Token::RBrace)?;
            // 可选尾随分号（C# 允许 `};`）
            let _ = self.match_token(&Token::Semi);
        }

        let synth_span = self.prev_span();
        // RFC 006 M2：合成实例 Equals（用户已手写同名实例方法则不覆盖）
        // record struct 为值类型：无 null 守卫。
        if !methods.iter().any(|m| {
            m.node.sig.name.as_str() == "Equals"
                && !matches!(m.node.sig.modifier, MethodModifier::Static)
        }) {
            let eq_members = record_equality_members(&properties, &fields);
            methods.push(synthesize_record_equals(
                &record_name,
                &eq_members,
                synth_span,
                /* nullable_ref */ !is_struct,
            ));
        }
        // RFC 006：合成实例 GetHashCode，与 Equals 覆盖同一组实例成员。
        if !methods.iter().any(|m| {
            m.node.sig.name.as_str() == "GetHashCode"
                && !matches!(m.node.sig.modifier, MethodModifier::Static)
        }) {
            let eq_members = record_equality_members(&properties, &fields);
            methods.push(synthesize_record_get_hash_code(&eq_members, synth_span));
        }
        // RFC 006：合成 Deconstruct（仅位置参数；record 与 record struct）。
        if !deconstruct_members.is_empty()
            && !methods
                .iter()
                .any(|m| m.node.sig.name.as_str() == "Deconstruct")
        {
            methods.push(synthesize_record_deconstruct(
                &deconstruct_members,
                synth_span,
            ));
        }
        // RFC 006：record / record struct 自动实现 IEquatable<T> /
        // IHashable<T>（static abstract；与实例 Equals/GetHashCode 对齐）。
        // 引用类型静态 Equals 含 null 安全；struct 值类型直接转发实例 Equals。
        ensure_record_equality_iface_bases(&mut bases, &record_name, synth_span);
        if !methods.iter().any(|m| {
            m.node.sig.name.as_str() == "Equals"
                && matches!(m.node.sig.modifier, MethodModifier::Static)
        }) {
            methods.push(synthesize_record_static_equals(
                &record_name,
                synth_span,
                /* nullable_ref */ !is_struct,
            ));
        }
        if !methods.iter().any(|m| {
            m.node.sig.name.as_str() == "GetHashCode"
                && matches!(m.node.sig.modifier, MethodModifier::Static)
        }) {
            methods.push(synthesize_record_static_get_hash_code(
                &record_name,
                synth_span,
            ));
        }

        if is_struct {
            Ok(Item::Struct(StructDef {
                vis,
                is_readonly: false,
                is_record: true,
                name,
                generics,
                where_clause,
                bases,
                fields,
                properties,
                methods,
                constructors,
                attributes: vec![],
                doc: None,
            }))
        } else {
            Ok(Item::Class(ClassDef {
                vis,
                is_static: false,
                is_abstract: false,
                is_partial: false,
                is_record: true,
                name,
                generics,
                where_clause,
                bases,
                fields,
                properties,
                methods,
                constructors,
                attributes: vec![],
                doc: None,
                synthesized_host: None,
            }))
        }
    }

    /// 解析 record 位置参数列表 `(Type Name, …)`（非普通 class primary ctor）。
    fn parse_record_positional_params(
        &mut self,
    ) -> Result<Vec<(Spanned<Type>, Ident)>, ParseError> {
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                let ty = self.parse_type()?;
                let name = self.parse_ident()?;
                params.push((ty, name));
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RParen)?;
        Ok(params)
    }

    pub(crate) fn parse_interface(&mut self) -> Result<InterfaceDef, ParseError> {
        let vis = self.parse_item_vis();
        self.expect(Token::Interface)?;
        let name = self.parse_ident()?;
        let generics = self.parse_interface_generics()?;
        let mut bases = Vec::new();
        if self.match_token(&Token::Colon) {
            loop {
                bases.push(self.parse_type()?.node);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        let where_clause = self.parse_where_clause()?;
        self.expect(Token::LBrace)?;
        let mut methods = Vec::new();
        let mut properties = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            match self.parse_interface_member()? {
                InterfaceBodyMember::Method(m) => methods.push(m),
                InterfaceBodyMember::Property(p) => properties.push(p),
            }
            self.match_token(&Token::Semi);
        }
        self.expect(Token::RBrace)?;
        Ok(InterfaceDef {
            vis,
            name,
            generics,
            where_clause,
            bases,
            methods,
            properties,
            attributes: vec![],
            doc: None,
        })
    }

    pub(crate) fn parse_enum(&mut self) -> Result<EnumDef, ParseError> {
        let vis = self.parse_item_vis();
        self.expect(Token::Enum)?;
        let name = self.parse_ident()?;
        self.expect(Token::LBrace)?;
        let mut variants = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            self.skip_doc_comments();
            if self.check(&Token::RBrace) {
                break;
            }
            // RFC 038：枚举成员可附加属性（`[Display("无")] None`）——通用
            // 属性系统：任何声明均可附加属性。解析后随 EnumVariant 存入 AST。
            let vattrs = self.parse_attributes()?;
            let vname = self.parse_ident()?;
            // Optional explicit discriminant: `Variant = 200` / `Variant = -1`（负数，
            // 对齐 ADO.NET 枚举如 `IsolationLevel.Unspecified = -1`）。
            let disc = if self.match_token(&Token::Eq) {
                // 可选一元负号：`-` 后必须跟无符号整数字面量。
                let negate = self.match_token(&Token::Minus);
                match &self.peek().token {
                    Token::IntLit(n) => {
                        let base = *n;
                        if negate {
                            if base > 0 {
                                let val = -base;
                                self.advance();
                                Some(val)
                            } else {
                                return Err(self.error(
                                    "integer literal",
                                    "enum variant discriminant cannot negate a non-positive literal".into(),
                                ));
                            }
                        } else {
                            let val = base;
                            self.advance();
                            Some(val)
                        }
                    }
                    other => {
                        return Err(self.error(
                            "integer literal",
                            format!("unexpected {:?} after `=` in enum variant", other),
                        ));
                    }
                }
            } else {
                None
            };
            let fields = if self.match_token(&Token::LBrace) {
                let mut f = Vec::new();
                while !self.check(&Token::RBrace) && !self.is_at_end() {
                    f.push(match self.parse_field_or_property()? {
                        FieldOrProperty::Field(field) => field,
                        FieldOrProperty::Property => {
                            return Err(self.error("field", "property in enum variant".into()));
                        }
                    });
                }
                self.expect(Token::RBrace)?;
                f
            } else {
                vec![]
            };
            variants.push(EnumVariant {
                name: vname,
                discriminant: disc,
                fields,
                attributes: vattrs,
                doc: None,
            });
            self.match_token(&Token::Comma);
        }
        self.expect(Token::RBrace)?;
        Ok(EnumDef {
            vis,
            name,
            variants,
            attributes: vec![],
            doc: None,
        })
    }

    /// RFC 004 M1：解析 `variant` 类型声明。
    ///
    /// 语法：`variant Name { | Case1 of T1 | Case2 of T2 | Case3 }`
    /// - `|` 起始每个 case（首项可省略 `|`）
    /// - `of PayloadType` 可选（无 payload case 如 `Null`）
    /// - case 名称遵循 PascalCase
    /// - M1 不支持泛型参数（M2 扩展）
    pub(crate) fn parse_variant(&mut self) -> Result<VariantDef, ParseError> {
        let vis = self.parse_item_vis();
        self.expect(Token::Variant)?;
        let name = self.parse_ident()?;
        // RFC 004 M1：暂不支持泛型参数与 where 子句（M2 扩展），但保留解析以容错
        let generics = self.parse_generics()?;
        let where_clause = self.parse_where_clause()?;
        self.expect(Token::LBrace)?;
        let mut cases = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            self.skip_doc_comments();
            if self.check(&Token::RBrace) {
                break;
            }
            // `|` (BitOr) 起始每个 case（首项可省略，后续也容错省略）
            self.match_token(&Token::BitOr);
            let case_name = self.parse_ident()?;
            // `of PayloadType` 可选——`of` 作为上下文关键字（仅在 variant case 后识别）
            let payload = if matches!(&self.peek().token, Token::Ident(s) if s == "of") {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            cases.push(VariantCase {
                name: case_name,
                payload,
                doc: None,
            });
            // case 之间可选 `,` 或仅靠 `|` 分隔
            self.match_token(&Token::Comma);
        }
        self.expect(Token::RBrace)?;
        Ok(VariantDef {
            vis,
            name,
            generics,
            where_clause,
            cases,
            attributes: vec![],
            doc: None,
        })
    }

    pub(crate) fn parse_delegate(&mut self) -> Result<DelegateDef, ParseError> {
        let vis = self.parse_item_vis();
        self.expect(Token::Delegate)?;
        let ret = if self.check(&Token::Void) {
            self.advance();
            None
        } else {
            Some(self.parse_type()?)
        };
        let name = self.parse_ident()?;
        // GAP #5 扩展：泛型委托（`delegate R Map<T, R>(T x);`）。
        let generics = self.parse_generics()?;
        self.expect(Token::LParen)?;
        let params = self.parse_delegate_params()?;
        self.expect(Token::RParen)?;
        // C# 语法顺序：where 子句位于参数列表之后、分号之前；复用
        // parse_where_clause 全语法（多段/多 param/多 bound/new() last）。
        let where_clause = self.parse_where_clause()?;
        self.expect(Token::Semi)?;
        Ok(DelegateDef {
            vis,
            name,
            generics,
            params,
            ret,
            where_clause,
            attributes: vec![],
            doc: None,
        })
    }

    fn parse_delegate_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.check(&Token::RParen) {
            return Ok(params);
        }
        loop {
            let ty = self.parse_type()?;
            let name = self.parse_ident()?;
            params.push(Param {
                name,
                ty,
                attributes: vec![],
                is_extension_receiver: false,
                is_ref: false,
                is_out: false,
                is_in: false,
                is_params: false,
                default: None,
            });
            if self.match_token(&Token::Comma) {
                continue;
            }
            break;
        }
        Ok(params)
    }

    pub(crate) fn starts_with_return_type(&self) -> bool {
        match &self.peek().token {
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
            | Token::LParen => true,
            // RFC 026 M1：`Ident?`（可空引用类型，如 `object?`/`string?`/`T?`）也作为返回类型起点。
            // 原 `Lt | Ident | Dot` 漏掉 `Question`，导致 `object? GetService(...)` 被误判为无返回类型。
            Token::Ident(_) => self.tokens.get(self.pos + 1).is_some_and(|t| {
                matches!(
                    t.token,
                    Token::Lt | Token::Ident(_) | Token::Dot | Token::Question
                )
            }),
            _ => false,
        }
    }
}

/// RFC 006 / RFC 006 M4：将 record 位置参数脱糖为 `{ get; init; }` + 构造器赋值体。
///
/// 属性保留位置参数的 PascalCase 名；构造器形参降为 camelCase，避免
/// `this.X = X` 同名遮蔽。
fn desugar_record_positional(
    _record_name: &ast::Ident,
    params: Vec<(ast::Spanned<ast::Type>, ast::Ident)>,
    span: ast::Span,
) -> (Vec<ast::PropertyDef>, ast::Spanned<ast::ConstructorDef>) {
    let mut properties = Vec::with_capacity(params.len());
    let mut ctor_params = Vec::with_capacity(params.len());
    let mut stmts = Vec::with_capacity(params.len());

    for (ty, prop_name) in params {
        let param_name = positional_param_name(&prop_name);
        properties.push(ast::PropertyDef {
            vis: ast::Visibility::Public,
            name: prop_name.clone(),
            ty: ty.clone(),
            has_get: true,
            has_set: false,
            has_init: true,
            is_required: false,
            get_body: None,
            set_body: None,
            get_vis: None,
            set_vis: None,
            modifier: ast::MethodModifier::None,
            attributes: vec![],
            is_static_abstract: false,
            index_params: vec![],
            init: None,
            doc: None,
        });
        ctor_params.push(ast::Param {
            name: param_name.clone(),
            ty: ty.clone(),
            attributes: vec![],
            is_extension_receiver: false,
            is_ref: false,
            is_out: false,
            is_in: false,
            is_params: false,
            default: None,
        });
        let this_prop = ast::Spanned::new(
            ast::Expr::Field {
                receiver: Box::new(ast::Spanned::new(ast::Expr::Ident("this".into()), span)),
                field: prop_name,
            },
            span,
        );
        let value = ast::Spanned::new(ast::Expr::Ident(param_name), span);
        stmts.push(ast::Spanned::new(
            ast::Stmt::Assign {
                target: this_prop,
                value,
            },
            span,
        ));
    }

    let ctor = ast::Spanned::new(
        ast::ConstructorDef {
            vis: ast::Visibility::Public,
            params: ctor_params,
            body: ast::Block { stmts, tail: None },
            base_args: None,
            doc: None,
        },
        span,
    );
    (properties, ctor)
}

/// Equals/GetHashCode 成员：位置 `{ get; init; }` 属性 + 体实例字段（顺序：属性先）。
fn record_equality_members(
    properties: &[ast::PropertyDef],
    fields: &[ast::FieldDef],
) -> Vec<(ast::Ident, ast::Spanned<ast::Type>)> {
    let mut out = Vec::new();
    for p in properties {
        if p.is_indexer() || p.is_static_abstract || !p.has_get {
            continue;
        }
        out.push((p.name.clone(), p.ty.clone()));
    }
    for f in fields {
        if f.is_static {
            continue;
        }
        out.push((f.name.clone(), f.ty.clone()));
    }
    out
}

/// `X` → `x`，`Name` → `name`（位置参数字段名 → 构造器形参名）。
fn positional_param_name(field: &ast::Ident) -> ast::Ident {
    let s = field.as_str();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let lower: String = c.to_lowercase().collect();
            format!("{lower}{}", chars.as_str()).into()
        }
        None => field.clone(),
    }
}

/// RFC 006 M2：合成 `public bool Equals(R other)`（字段值相等）。
///
/// `nullable_ref == true`（record 引用类型）：入口 `other is null` → `false`。
/// `nullable_ref == false`（record struct）：值类型，无 null 守卫。
fn synthesize_record_equals(
    record_name: &ast::Ident,
    members: &[(ast::Ident, ast::Spanned<ast::Type>)],
    span: ast::Span,
    nullable_ref: bool,
) -> ast::Spanned<ast::MethodDef> {
    let other: ast::Ident = "other".into();
    let record_ty = ast::Spanned::new(
        ast::Type::Named {
            path: vec![record_name.clone()],
            generics: vec![],
        },
        span,
    );

    let mut stmts = Vec::new();
    if nullable_ref {
        // `other is null` → return false
        let other_is_null = ast::Spanned::new(
            ast::Expr::Is {
                expr: Box::new(ast::Spanned::new(ast::Expr::Ident(other.clone()), span)),
                pattern: ast::IsPattern::Null,
            },
            span,
        );
        let return_false = ast::Spanned::new(
            ast::Stmt::Return(Some(ast::Spanned::new(ast::Expr::BoolLit(false), span))),
            span,
        );
        stmts.push(ast::Spanned::new(
            ast::Stmt::Expr(ast::Spanned::new(
                ast::Expr::If {
                    cond: Box::new(other_is_null),
                    then_branch: ast::Block {
                        stmts: vec![return_false],
                        tail: None,
                    },
                    else_branch: None,
                },
                span,
            )),
            span,
        ));
    }

    // this.F == other.F && …
    let mut cmp: Option<ast::Spanned<ast::Expr>> = None;
    for (fname, _) in members {
        let left = ast::Spanned::new(
            ast::Expr::Field {
                receiver: Box::new(ast::Spanned::new(ast::Expr::This, span)),
                field: fname.clone(),
            },
            span,
        );
        let right = ast::Spanned::new(
            ast::Expr::Field {
                receiver: Box::new(ast::Spanned::new(ast::Expr::Ident(other.clone()), span)),
                field: fname.clone(),
            },
            span,
        );
        let eq = ast::Spanned::new(
            ast::Expr::Binary {
                op: ast::BinOp::Eq,
                left: Box::new(left),
                right: Box::new(right),
            },
            span,
        );
        cmp = Some(match cmp {
            None => eq,
            Some(prev) => ast::Spanned::new(
                ast::Expr::Binary {
                    op: ast::BinOp::And,
                    left: Box::new(prev),
                    right: Box::new(eq),
                },
                span,
            ),
        });
    }
    let body_expr = cmp.unwrap_or_else(|| ast::Spanned::new(ast::Expr::BoolLit(true), span));
    stmts.push(ast::Spanned::new(ast::Stmt::Return(Some(body_expr)), span));

    ast::Spanned::new(
        ast::MethodDef {
            sig: ast::MethodSig {
                vis: ast::Visibility::Public,
                name: "Equals".into(),
                generics: vec![],
                where_clause: vec![],
                params: vec![ast::Param {
                    name: other,
                    ty: record_ty,
                    attributes: vec![],
                    is_extension_receiver: false,
                    is_ref: false,
                    is_out: false,
                    is_in: false,
                    is_params: false,
                    default: None,
                }],
                ret: Some(ast::Spanned::new(
                    ast::Type::Named {
                        path: vec!["bool".into()],
                        generics: vec![],
                    },
                    span,
                )),
                is_async: false,
                modifier: ast::MethodModifier::None,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(ast::Block { stmts, tail: None }),
            doc: None,
        },
        span,
    )
}

/// RFC 006：合成 `public int GetHashCode()`，与 Equals 覆盖同一组实例字段。
///
/// 组合规则：`((((f0 * 31) + f1) * 31) + …)`，其中 `fi = this.Fi.GetHashCode()`；
/// 无实例字段时返回 `0`。与 Equals 同字段集合，保证相等 ⇒ 同哈希（字段
/// `GetHashCode` 自身满足该契约时）。
fn synthesize_record_get_hash_code(
    members: &[(ast::Ident, ast::Spanned<ast::Type>)],
    span: ast::Span,
) -> ast::Spanned<ast::MethodDef> {
    let body_expr = if members.is_empty() {
        ast::Spanned::new(ast::Expr::IntLit(0), span)
    } else {
        let mut acc: Option<ast::Spanned<ast::Expr>> = None;
        for (fname, _) in members {
            let field_recv = ast::Spanned::new(
                ast::Expr::Field {
                    receiver: Box::new(ast::Spanned::new(ast::Expr::This, span)),
                    field: fname.clone(),
                },
                span,
            );
            let field_hash = ast::Spanned::new(
                ast::Expr::MethodCall {
                    receiver: Box::new(field_recv),
                    method: "GetHashCode".into(),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                },
                span,
            );
            acc = Some(match acc {
                None => field_hash,
                Some(prev) => {
                    let scaled = ast::Spanned::new(
                        ast::Expr::Binary {
                            op: ast::BinOp::Mul,
                            left: Box::new(prev),
                            right: Box::new(ast::Spanned::new(ast::Expr::IntLit(31), span)),
                        },
                        span,
                    );
                    ast::Spanned::new(
                        ast::Expr::Binary {
                            op: ast::BinOp::Add,
                            left: Box::new(scaled),
                            right: Box::new(field_hash),
                        },
                        span,
                    )
                }
            });
        }
        acc.expect("non-empty members yield hash expr")
    };

    ast::Spanned::new(
        ast::MethodDef {
            sig: ast::MethodSig {
                vis: ast::Visibility::Public,
                name: "GetHashCode".into(),
                generics: vec![],
                where_clause: vec![],
                params: vec![],
                ret: Some(ast::Spanned::new(
                    ast::Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    span,
                )),
                is_async: false,
                modifier: ast::MethodModifier::None,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(ast::Block {
                stmts: vec![ast::Spanned::new(ast::Stmt::Return(Some(body_expr)), span)],
                tail: None,
            }),
            doc: None,
        },
        span,
    )
}

/// RFC 006：合成 `public void Deconstruct(out T1 p1, …)`（仅位置参数字段）。
///
/// out 形参用 camelCase，避免与同名字段/`this.F` 在 struct 方法体内遮蔽
///（`X = this.X` 在值类型路径上会写错目标）。
fn synthesize_record_deconstruct(
    positional_members: &[(ast::Ident, ast::Spanned<ast::Type>)],
    span: ast::Span,
) -> ast::Spanned<ast::MethodDef> {
    let mut params = Vec::with_capacity(positional_members.len());
    let mut stmts = Vec::with_capacity(positional_members.len());
    for (fname, fty) in positional_members {
        let param_name = positional_param_name(fname);
        params.push(ast::Param {
            name: param_name.clone(),
            ty: fty.clone(),
            attributes: vec![],
            is_extension_receiver: false,
            is_ref: false,
            is_out: true,
            is_in: false,
            is_params: false,
            default: None,
        });
        let target = ast::Spanned::new(ast::Expr::Ident(param_name), span);
        let value = ast::Spanned::new(
            ast::Expr::Field {
                receiver: Box::new(ast::Spanned::new(ast::Expr::This, span)),
                field: fname.clone(),
            },
            span,
        );
        stmts.push(ast::Spanned::new(ast::Stmt::Assign { target, value }, span));
    }
    ast::Spanned::new(
        ast::MethodDef {
            sig: ast::MethodSig {
                vis: ast::Visibility::Public,
                name: "Deconstruct".into(),
                generics: vec![],
                where_clause: vec![],
                params,
                ret: Some(ast::Spanned::new(
                    ast::Type::Named {
                        path: vec!["void".into()],
                        generics: vec![],
                    },
                    span,
                )),
                is_async: false,
                modifier: ast::MethodModifier::None,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(ast::Block { stmts, tail: None }),
            doc: None,
        },
        span,
    )
}

/// RFC 006：若 bases 尚无 `IEquatable<R>` / `IHashable<R>` 则追加。
fn ensure_record_equality_iface_bases(
    bases: &mut Vec<ast::Type>,
    record_name: &ast::Ident,
    span: ast::Span,
) {
    for iface in ["IEquatable", "IHashable"] {
        if bases
            .iter()
            .any(|b| record_iface_base_matches(b, iface, record_name))
        {
            continue;
        }
        bases.push(ast::Type::Named {
            path: vec![iface.into()],
            generics: vec![ast::Spanned::new(
                ast::Type::Named {
                    path: vec![record_name.clone()],
                    generics: vec![],
                },
                span,
            )],
        });
    }
}

fn record_iface_base_matches(base: &ast::Type, iface: &str, record_name: &ast::Ident) -> bool {
    let ast::Type::Named { path, generics } = base else {
        return false;
    };
    if path.last().map(|n| n.as_str()) != Some(iface) || generics.len() != 1 {
        return false;
    }
    matches!(
        &generics[0].node,
        ast::Type::Named { path, generics }
            if generics.is_empty()
                && path.last().map(|n| n.as_str()) == Some(record_name.as_str())
    )
}

/// RFC 006：合成 `public static bool Equals(R a, R b)`，转发实例 Equals（IEquatable）。
fn synthesize_record_static_equals(
    record_name: &ast::Ident,
    span: ast::Span,
    nullable_ref: bool,
) -> ast::Spanned<ast::MethodDef> {
    let record_ty = ast::Spanned::new(
        ast::Type::Named {
            path: vec![record_name.clone()],
            generics: vec![],
        },
        span,
    );
    let a: ast::Ident = "a".into();
    let b: ast::Ident = "b".into();
    let a_expr = ast::Spanned::new(ast::Expr::Ident(a.clone()), span);
    let b_expr = ast::Spanned::new(ast::Expr::Ident(b.clone()), span);
    let instance_call = ast::Spanned::new(
        ast::Expr::MethodCall {
            receiver: Box::new(a_expr.clone()),
            method: "Equals".into(),
            args: vec![b_expr.clone()],
            type_args: vec![],
            params_span: None,
        },
        span,
    );
    let body_expr = if nullable_ref {
        // (a is null) ? (b is null) : a.Equals(b)
        let a_is_null = ast::Spanned::new(
            ast::Expr::Is {
                expr: Box::new(a_expr),
                pattern: ast::IsPattern::Null,
            },
            span,
        );
        let b_is_null = ast::Spanned::new(
            ast::Expr::Is {
                expr: Box::new(b_expr),
                pattern: ast::IsPattern::Null,
            },
            span,
        );
        ast::Spanned::new(
            ast::Expr::Ternary {
                cond: Box::new(a_is_null),
                then_branch: Box::new(b_is_null),
                else_branch: Box::new(instance_call),
            },
            span,
        )
    } else {
        instance_call
    };
    ast::Spanned::new(
        ast::MethodDef {
            sig: ast::MethodSig {
                vis: ast::Visibility::Public,
                name: "Equals".into(),
                generics: vec![],
                where_clause: vec![],
                params: vec![
                    ast::Param {
                        name: a,
                        ty: record_ty.clone(),
                        attributes: vec![],
                        is_extension_receiver: false,
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    },
                    ast::Param {
                        name: b,
                        ty: record_ty,
                        attributes: vec![],
                        is_extension_receiver: false,
                        is_ref: false,
                        is_out: false,
                        is_in: false,
                        is_params: false,
                        default: None,
                    },
                ],
                ret: Some(ast::Spanned::new(
                    ast::Type::Named {
                        path: vec!["bool".into()],
                        generics: vec![],
                    },
                    span,
                )),
                is_async: false,
                modifier: ast::MethodModifier::Static,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(ast::Block {
                stmts: vec![ast::Spanned::new(ast::Stmt::Return(Some(body_expr)), span)],
                tail: None,
            }),
            doc: None,
        },
        span,
    )
}

/// RFC 006：合成 `public static int GetHashCode(R value)`，转发实例 GetHashCode（IHashable）。
fn synthesize_record_static_get_hash_code(
    record_name: &ast::Ident,
    span: ast::Span,
) -> ast::Spanned<ast::MethodDef> {
    let record_ty = ast::Spanned::new(
        ast::Type::Named {
            path: vec![record_name.clone()],
            generics: vec![],
        },
        span,
    );
    let value: ast::Ident = "value".into();
    let body_expr = ast::Spanned::new(
        ast::Expr::MethodCall {
            receiver: Box::new(ast::Spanned::new(ast::Expr::Ident(value.clone()), span)),
            method: "GetHashCode".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        },
        span,
    );
    ast::Spanned::new(
        ast::MethodDef {
            sig: ast::MethodSig {
                vis: ast::Visibility::Public,
                name: "GetHashCode".into(),
                generics: vec![],
                where_clause: vec![],
                params: vec![ast::Param {
                    name: value,
                    ty: record_ty,
                    attributes: vec![],
                    is_extension_receiver: false,
                    is_ref: false,
                    is_out: false,
                    is_in: false,
                    is_params: false,
                    default: None,
                }],
                ret: Some(ast::Spanned::new(
                    ast::Type::Named {
                        path: vec!["int".into()],
                        generics: vec![],
                    },
                    span,
                )),
                is_async: false,
                modifier: ast::MethodModifier::Static,
                attributes: vec![],
                is_static_abstract: false,
                doc: None,
            },
            body: Some(ast::Block {
                stmts: vec![ast::Spanned::new(ast::Stmt::Return(Some(body_expr)), span)],
                tail: None,
            }),
            doc: None,
        },
        span,
    )
}
