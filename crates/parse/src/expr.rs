use crate::lexer::Token;
use ast::*;

use crate::error::ParseError;
use crate::parser::Parser;

mod expr_lambda;
mod expr_special;
mod expr_switch_form;
mod expr_with;

impl Parser {
    pub(crate) fn parse_expr(&mut self) -> Result<Spanned<Expr>, ParseError> {
        self.parse_expr_bp(0)
    }

    pub(crate) fn parse_expr_bp(&mut self, min_bp: u8) -> Result<Spanned<Expr>, ParseError> {
        let start = self.current_span();
        let mut left = self.parse_prefix()?;

        loop {
            // 赋值表达式（C# assignment：最低优先级、右结合）。BP 1 与 `||`/`??`
            // 同层；右结合由 rhs 以 min_bp=1 解析达成（`a = b = c` 右折叠）。
            // 复合赋值与 `??=` 就地脱糖为 `Assign + Binary/Coalesce`（RFC 076
            // 契约延续：无复合赋值 AST 变体；与语句层 try_parse_compound_assign
            // 同构——lhs 双求值限制同一）。min_bp >= 2（`||`/`??` rhs）时不消费，
            // 赋值自然归入更外层。is_at_end 守卫：prefix 后可能即 EOF。
            if min_bp <= 1 && !self.is_at_end() {
                let compound: Option<Option<BinOp>> = match &self.peek().token {
                    Token::Eq => Some(None),
                    Token::PlusEq => Some(Some(BinOp::Add)),
                    Token::MinusEq => Some(Some(BinOp::Sub)),
                    Token::StarEq => Some(Some(BinOp::Mul)),
                    Token::SlashEq => Some(Some(BinOp::Div)),
                    Token::BitOrEq => Some(Some(BinOp::BitOr)),
                    Token::BitAndEq => Some(Some(BinOp::BitAnd)),
                    Token::BitXorEq => Some(Some(BinOp::BitXor)),
                    _ => None,
                };
                if let Some(compound) = compound {
                    self.advance();
                    let right = self.parse_expr_bp(1)?;
                    let end_span = right.span;
                    let value = match compound {
                        Some(op) => Spanned::new(
                            Expr::Binary {
                                op,
                                left: Box::new(left.clone()),
                                right: Box::new(right),
                            },
                            left.span.merge(end_span),
                        ),
                        None => right,
                    };
                    left = Spanned::new(
                        Expr::Assign {
                            target: Box::new(left),
                            value: Box::new(value),
                        },
                        start.merge(end_span),
                    );
                    continue;
                }
                // `a ??= b` — null-coalesce 赋值，脱糖为 `a = a ?? b`。
                if self.check(&Token::NullCoalesceEq) {
                    self.advance();
                    let right = self.parse_expr_bp(1)?;
                    let end_span = right.span;
                    let value = Spanned::new(
                        Expr::Coalesce {
                            left: Box::new(left.clone()),
                            right: Box::new(right),
                        },
                        left.span.merge(end_span),
                    );
                    left = Spanned::new(
                        Expr::Assign {
                            target: Box::new(left),
                            value: Box::new(value),
                        },
                        start.merge(end_span),
                    );
                    continue;
                }
            }
            if self.check(&Token::NullCoalesce) && min_bp <= 1 {
                self.advance();
                let right = self.parse_expr_bp(2)?;
                let end_span = right.span;
                left = Spanned::new(
                    Expr::Coalesce {
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    start.merge(end_span),
                );
                continue;
            }
            // Ternary conditional: `cond ? then : else` — lowest precedence,
            // right-associative (`a ? b : c ? d : e` = `a ? b : (c ? d : e)`).
            // The lexer produces QuestionDot for `?.` and NullCoalesce for `??`,
            // so `Question` here is unambiguously the ternary operator.
            // min_bp <= 1（而非 == 0）：赋值 rhs（min_bp=1）内三元须消费，
            // 否则 `x = c ? a : b` 被解析为 `(x = c) ? a : b`。
            if self.check(&Token::Question) && min_bp <= 1 {
                self.advance(); // consume `?`
                let then_branch = self.parse_expr()?;
                self.expect(Token::Colon)?;
                // Right-associative: else-branch parsed with min_bp=0 so nested
                // ternaries in the else position are parsed as a single expression.
                let else_branch = self.parse_expr_bp(0)?;
                let end_span = else_branch.span;
                left = Spanned::new(
                    Expr::Ternary {
                        cond: Box::new(left),
                        then_branch: Box::new(then_branch),
                        else_branch: Box::new(else_branch),
                    },
                    start.merge(end_span),
                );
                continue;
            }
            // RFC 036 D9.1: `as` 关键字已剔除——强制转换统一使用 `(T)x` 语法。
            // 原 `x as T` 分支已移除，由 parse_prefix 中的 `(T)x` speculative
            // parse 替代。AST 仍复用 Expr::Cast。
            // RFC 036 M1: `expr is pattern` — 类型测试 + 模式匹配。
            // 优先级与关系运算（`<`/`>`/`<=`/`>=`，BP 13）同级（C# relational &
            // type-testing 同级，高于 `==`/`!=`/`&`/`&&`/`||`）。若错置为更低优先级，
            // `a && b is C` 会被解析成 `(a && b) is C`——scrutinee 误收为整个布尔
            // 表达式（type bool），`is` 被折叠为裸指针布尔（is+&& 短路误标缺陷）。
            if self.check(&Token::Is) && min_bp <= 13 {
                self.advance();
                let pattern = self.parse_is_pattern()?;
                let end_span = self.prev_span();
                left = Spanned::new(
                    Expr::Is {
                        expr: Box::new(left),
                        pattern,
                    },
                    start.merge(end_span),
                );
                continue;
            }
            // EOF 后无 infix 运算可消费——直接返回 left。
            // 此 guard 修复 `peek()` 在 end-of-tokens 时的越界 panic
            // （RFC 009 M4-6 splice 路径触发：展开字符串可能在 prefix 后立即结束）。
            if self.is_at_end() {
                return Ok(left);
            }
            let (left_bp, right_bp, op) = match &self.peek().token {
                Token::OrOr => (1, 2, Some(BinOp::Or)),
                Token::AndAnd => (3, 4, Some(BinOp::And)),
                // C# 优先级：| < ^ < & < == < 关系 < 移位 < 加减 < 乘除
                Token::BitOr => (5, 6, Some(BinOp::BitOr)),
                Token::BitXor => (7, 8, Some(BinOp::BitXor)),
                Token::BitAnd => (9, 10, Some(BinOp::BitAnd)),
                Token::EqEq => (11, 12, Some(BinOp::Eq)),
                Token::NotEq => (11, 12, Some(BinOp::NotEq)),
                Token::Lt if !self.is_generic_call_start() => (13, 14, Some(BinOp::Lt)),
                Token::Lt => (0, 0, None),
                Token::Le => (13, 14, Some(BinOp::Le)),
                Token::Gt => (13, 14, Some(BinOp::Gt)),
                Token::Ge => (13, 14, Some(BinOp::Ge)),
                Token::Shl => (15, 16, Some(BinOp::Shl)),
                Token::Shr => (15, 16, Some(BinOp::Shr)),
                Token::Plus => (17, 18, Some(BinOp::Add)),
                Token::Minus => (17, 18, Some(BinOp::Sub)),
                Token::Star => (19, 20, Some(BinOp::Mul)),
                Token::Slash => (19, 20, Some(BinOp::Div)),
                Token::Percent => (19, 20, Some(BinOp::Mod)),
                _ => (0, 0, None),
            };
            if op.is_none() || left_bp < min_bp {
                if self.check(&Token::LBracket) {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.expect(Token::RBracket)?;
                    // 后缀节点 span 覆盖完整表达式（`a[i]` 含 `a` 与 `[i]`）：
                    // P0 表达式类型表以 span 为键，若 Field/Index/MethodCall 的
                    // span 仅等于 receiver，会与 receiver 自身的表条目键冲突
                    //（`t.Name` 与 `t` 同键 → mir 查表误得 receiver 类型）。
                    let span = left.span.merge(self.prev_span());
                    left = Spanned::new(
                        Expr::Index {
                            receiver: Box::new(left),
                            index: Box::new(index),
                        },
                        span,
                    );
                    continue;
                }
                if self.check(&Token::QuestionDot) || self.check(&Token::BangDot) {
                    let is_force = self.check(&Token::BangDot);
                    self.advance();
                    let field = self.parse_ident()?;
                    let span = left.span.merge(self.prev_span());
                    let mut access = Spanned::new(
                        Expr::Field {
                            receiver: Box::new(left),
                            field,
                        },
                        span,
                    );
                    if self.check(&Token::LParen) {
                        self.advance();
                        let args = self.parse_call_args()?;
                        let mspan = access.span.merge(self.prev_span());
                        access = match access.node {
                            Expr::Field { receiver, field } => Spanned::new(
                                Expr::MethodCall {
                                    receiver,
                                    method: field,
                                    args,
                                    type_args: vec![],
                                    params_span: None,
                                },
                                mspan,
                            ),
                            _ => unreachable!(),
                        };
                    }
                    let fspan = access.span;
                    left = if is_force {
                        Spanned::new(
                            Expr::ForceDeref {
                                access: Box::new(access),
                            },
                            fspan,
                        )
                    } else {
                        Spanned::new(
                            Expr::NullCond {
                                access: Box::new(access),
                            },
                            fspan,
                        )
                    };
                    continue;
                }
                if self.check(&Token::Dot) {
                    self.advance();
                    let field = self.parse_ident()?;
                    let span = left.span.merge(self.prev_span());
                    left = Spanned::new(
                        Expr::Field {
                            receiver: Box::new(left),
                            field,
                        },
                        span,
                    );
                    continue;
                }
                if self.check(&Token::LParen) {
                    self.advance();
                    let args = self.parse_call_args()?;
                    let span = left.span.merge(self.prev_span());
                    left = match left.node {
                        Expr::Field { receiver, field } => Spanned::new(
                            Expr::MethodCall {
                                receiver,
                                method: field,
                                args,
                                type_args: vec![],
                                params_span: None,
                            },
                            span,
                        ),
                        _ => Spanned::new(
                            Expr::Call {
                                func: Box::new(left),
                                args,
                                type_args: vec![],
                                params_span: None,
                            },
                            span,
                        ),
                    };
                    continue;
                }
                // RFC 036 M4：`e switch { ... }` postfix
                if self.check(&Token::Switch) {
                    left = self.parse_switch_expr_form(left)?;
                    continue;
                }
                // RFC 006 M2：`e with { Member = value, … }` postfix
                if self.check(&Token::With) {
                    left = self.parse_with_expr(left)?;
                    continue;
                }
                if self.is_generic_call_start() {
                    self.advance();
                    let mut type_args = vec![self.parse_type()?];
                    while self.match_token(&Token::Comma) {
                        type_args.push(self.parse_type()?);
                    }
                    self.expect_gt_close()?;
                    // RFC 004 M2：泛型 variant 表达式级调用 `Option<int>.Some(42)` /
                    // 无 payload case `Option<int>.None`。
                    // `>` 后跟 `.` 表示泛型类型限定 + 成员访问。
                    if self.check(&Token::Dot) {
                        self.advance(); // 消费 `.`
                        let member = self.parse_ident()?;
                        let span = left.span;
                        // 将 receiver 包裹为 `Call { func, args: [], type_args }`，
                        // 使 MIR lower 可识别 type_args 为 receiver 类型的泛型参数。
                        let receiver = Spanned::new(
                            Expr::Call {
                                func: Box::new(left),
                                args: vec![],
                                type_args,
                                params_span: None,
                            },
                            span,
                        );
                        if self.check(&Token::LParen) {
                            self.advance();
                            let args = self.parse_call_args()?;
                            left = Spanned::new(
                                Expr::MethodCall {
                                    receiver: Box::new(receiver),
                                    method: member,
                                    args,
                                    type_args: vec![],
                                    params_span: None,
                                },
                                span,
                            );
                        } else {
                            // 无 payload：`Option<int>.None` → Field
                            left = Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(receiver),
                                    field: member,
                                },
                                span,
                            );
                        }
                        continue;
                    }
                    self.expect(Token::LParen)?;
                    let args = self.parse_call_args()?;
                    let span = left.span;
                    left = match left.node {
                        Expr::Field { receiver, field } => Spanned::new(
                            Expr::MethodCall {
                                receiver,
                                method: field,
                                args,
                                type_args,
                                params_span: None,
                            },
                            span,
                        ),
                        _ => Spanned::new(
                            Expr::Call {
                                func: Box::new(left),
                                args,
                                type_args,
                                params_span: None,
                            },
                            span,
                        ),
                    };
                    continue;
                }
                break;
            }
            let op = op.unwrap();
            self.advance();
            let right = self.parse_expr_bp(right_bp)?;
            let end_span = right.span;
            // 常量折叠：相邻字符串字面量的 `+` 折叠为单个字面量（"a"+"b" → "ab"）。
            // 消除左结合深层嵌套表达式在 typeck 期导致的栈溢出（0xC00000FD）——
            // 长字符串拼接链（如 wgpu shader 源串）会构造数十层嵌套 Binary(Add)，
            // debug 版巨大栈帧下溢出。字符串字面量拼接语义无歧义，折叠恒正确。
            let folded = match (op, &left.node, &right.node) {
                (BinOp::Add, Expr::StringLit(a), Expr::StringLit(b)) => {
                    let mut s = a.clone();
                    s.push_str(b);
                    Expr::StringLit(s)
                }
                _ => Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            };
            left = Spanned::new(folded, start.merge(end_span));
        }
        Ok(left)
    }

    /// `Name<Type>(` generic invocation — not a comparison.
    ///
    /// 前向扫描匹配的 `>`，支持：
    /// - 单类型参数 `<T>(`
    /// - 多类型参数 `<TService, TImpl>(`（RFC 023 M1：DI 扩展方法 AddTransient<T,S>）
    /// - 嵌套泛型 `<List<T>>(`
    /// - 可空类型 `<T?>(`
    /// - 限定类型名 `<Arc.IFoo>(`
    ///
    /// 遇到非类型上下文 token（运算符、字面量、`(` 等）立即返回 false，
    /// 避免 `a < b` 比较表达式被误识别为泛型调用。
    pub(crate) fn is_generic_call_start(&self) -> bool {
        if !self.check(&Token::Lt) {
            return false;
        }
        let mut depth: i32 = 1;
        let mut pos = self.pos + 1;
        while depth > 0 {
            match self.tokens.get(pos).map(|t| &t.token) {
                Some(Token::Lt) => depth += 1,
                Some(Token::Gt) => depth -= 1,
                // `<<` / `>>` 词法合并后等价于两层嵌套深度。
                Some(Token::Shl) => depth += 2,
                Some(Token::Shr) => depth -= 2,
                // 类型名组件：标识符、命名空间分隔符、可空标记。
                Some(Token::Ident(_)) | Some(Token::Dot) | Some(Token::Question) => {}
                // 多类型参数分隔符。
                Some(Token::Comma) => {}
                // RFC 004 M1：基元类型关键字（double/float/long/short/byte/char/void/int）
                // 必须允许作为泛型实参，否则 `Add<double>(1.5, 2.5)` 会被误判为
                // `<` 比较运算符而非泛型调用，导致 parse error。
                // 注意：`int` 不是关键字——lexer 将其识别为 `Token::Ident("int")`，
                // 因此已由 `Token::Ident(_)` 分支覆盖。这里仅处理真正的关键字 token。
                Some(Token::Float) | Some(Token::Double) | Some(Token::Long)
                | Some(Token::Short) | Some(Token::Byte) | Some(Token::Char)
                | Some(Token::UInt) | Some(Token::ULong) | Some(Token::UShort)
                | Some(Token::SByte) | Some(Token::Void) => {}
                Some(Token::LParen) => return false, // 泛型参数中不应出现 (
                None => return false,                // EOF
                _ => return false,                   // 运算符/字面量等 → 非泛型调用
            }
            pos += 1;
        }
        // pos 指向 `>` 之后的 token，必须是 `(` 或 `.` 才构成泛型调用。
        // `.` 支持：`Option<int>.Some(42)` —— 泛型类型 + 成员访问。
        matches!(
            self.tokens.get(pos).map(|t| &t.token),
            Some(Token::LParen) | Some(Token::Dot)
        )
    }

    /// RFC 036 D9.1: 判断当前 token 是否能作为 `(T)x` cast 的操作数起点。
    ///
    /// 用于 cast 消歧：`(T)` 后若是这些 token，则视为 cast；否则视为括号表达式。
    /// 能开启 prefix 表达式的 token：字面量、标识符、`(`、`[`、`new`、`this`、
    /// `default`、`typeof`、`base`、`await`、`from`、`!`、`-`。
    ///
    /// 关键排除：`{`（block 起点）、`}`、`;`、`,`、`)`、`]`、`.`、`=>`、
    /// 二元运算符（`+`/`*`/`==`/`<`/`&&`/etc.）——这些都不能开启 prefix 表达式。
    fn can_start_cast_operand(&self) -> bool {
        matches!(
            &self.peek().token,
            Token::Null
                | Token::IntLit(_)
                | Token::FloatLit(_)
                | Token::True
                | Token::False
                | Token::StringLit(_)
                | Token::VerbatimString(_)
                | Token::CharLit(_)
                | Token::Ident(_)
                | Token::LParen
                | Token::LBracket
                | Token::New
                | Token::This
                | Token::Default
                | Token::TypeOf
                | Token::NameOf
                | Token::Base
                | Token::Await
                | Token::From
                | Token::Bang
                | Token::Minus
                // RFC 009 M6: `async` 可作为表达式起点（async lambda）
                | Token::Async
        )
    }

    /// RFC 017 #8：`e` 或 `..e`。
    pub(crate) fn parse_collection_element(&mut self) -> Result<CollectionElement, ParseError> {
        if self.match_token(&Token::DotDot) {
            Ok(CollectionElement::Spread(self.parse_expr()?))
        } else {
            Ok(CollectionElement::Element(self.parse_expr()?))
        }
    }

    pub(crate) fn parse_prefix(&mut self) -> Result<Spanned<Expr>, ParseError> {
        let start = self.current_span();
        if self.match_token(&Token::LBracket) {
            let mut elements = Vec::new();
            if !self.check(&Token::RBracket) {
                elements.push(self.parse_collection_element()?);
                while self.match_token(&Token::Comma) {
                    if self.check(&Token::RBracket) {
                        break;
                    }
                    elements.push(self.parse_collection_element()?);
                }
            }
            self.expect(Token::RBracket)?;
            return Ok(Spanned::new(
                Expr::CollectionExpr { elements },
                start.merge(self.prev_span()),
            ));
        }
        if self.match_token(&Token::New) {
            // RFC 006：目标类型 `new(...)` —— `new` 后紧跟 `(` 时省略类型名，
            // `ty` 记为 `Type::Infer`，由 typeck 按期望类型填入。
            let ty = if self.check(&Token::LParen) {
                Spanned::new(Type::Infer, start)
            } else {
                // 元素/构造类型基（不含 `[]` 后缀，避免 `new T[n]` 的长度被误判）。
                let mut ty = self.parse_type_base()?;
                // `new T[<len>]` — C# 数组分配。
                if self.match_token(&Token::LBracket) {
                    if self.check(&Token::RBracket) {
                        return Err(ParseError::Unexpected {
                            span: start,
                            expected: "array length expression after `new T[`".into(),
                            found: "`new T[]` (removed; use `[...]` collection expression)".into(),
                        });
                    }
                    let length = self.parse_expr()?;
                    self.expect(Token::RBracket)?;
                    // 交错数组后缀 `new T[n][]` / `new T[n][][]`：每个空 `[]` 使元素
                    // 类型多包一层数组（`T` → `T[]` → `T[][]`）。外层长度为 n，内层
                    // 数组须各自 `new` 填充（C# 交错数组 = 数组的数组，复用一维数组
                    // 运行时 ABI，无需新 rank 元数据）。
                    while self.match_token(&Token::LBracket) {
                        self.expect(Token::RBracket)?;
                        let span = ty.span.merge(self.prev_span());
                        ty = Spanned::new(
                            Type::Array {
                                inner: Box::new(ty),
                            },
                            span,
                        );
                    }
                    return Ok(Spanned::new(
                        Expr::NewArray {
                            elem_type: ty,
                            length: Box::new(length),
                        },
                        start.merge(self.prev_span()),
                    ));
                }
                if matches!(ty.node, Type::Array { .. }) {
                    return Err(ParseError::Unexpected {
                        span: start,
                        expected: "type name after `new`".into(),
                        found: "array type `T[]` (removed; use `[...]` collection expression)"
                            .into(),
                    });
                }
                ty
            };
            // C# 风格：`new T { ... }` 允许省略 `()`。当 `(` 缺失时 args 为空，
            // 紧跟的 `{ ... }` 由对象初始化器分支处理。目标类型形式必须带 `()`。
            let args = if self.match_token(&Token::LParen) {
                self.parse_call_args()?
            } else {
                Vec::new()
            };
            let obj_init = if self.check(&Token::LBrace) && !self.is_block_start_after_lbrace() {
                self.advance();
                Some(self.parse_object_initializer_fields()?)
            } else {
                None
            };
            return Ok(Spanned::new(
                Expr::New { ty, args, obj_init },
                start.merge(self.prev_span()),
            ));
        }
        if self.match_token(&Token::This) {
            return Ok(Spanned::new(Expr::This, start.merge(self.prev_span())));
        }
        if self.match_token(&Token::Default) {
            // C# 7.1+ 对称：`default(T)` 显式类型 / `default` 裸关键字（类型推断）
            if self.match_token(&Token::LParen) {
                let ty = self.parse_type()?;
                self.expect(Token::RParen)?;
                return Ok(Spanned::new(
                    Expr::Default { ty },
                    start.merge(self.prev_span()),
                ));
            }
            // 裸 `default` — 类型从上下文推断（参数类型、赋值目标等）
            return Ok(Spanned::new(
                Expr::Default {
                    ty: Spanned::new(Type::Infer, start),
                },
                start,
            ));
        }
        if self.match_token(&Token::TypeOf) {
            self.expect(Token::LParen)?;
            let ty = self.parse_type()?;
            self.expect(Token::RParen)?;
            return Ok(Spanned::new(
                Expr::TypeOf(ty),
                start.merge(self.prev_span()),
            ));
        }
        // RFC 037 M1.1: `nameof(ident)` / `nameof(path.to.ident)` ——
        // 编译期解析符号名为字符串。Parser desugar 为 `Expr::StringLit`，
        // 避免新增 AST 节点 / typeck / MIR / codegen 改动。
        //
        // 语义：取实参的标识符路径最后一段作为字符串。
        //   - `nameof(Title)`         → "Title"
        //   - `nameof(Arc.UI.Window)` → "Window"
        //   - `nameof(int)`           → "int"（int 词法为 Ident）
        //
        // M1.1 限制：不验证符号是否存在（编译期常量折叠留待后续增强）。
        // 当前实现足以支撑 DP.Register(nameof(prop), typeof(owner), default) 编码模型。
        if self.match_token(&Token::NameOf) {
            self.expect(Token::LParen)?;
            let mut name = self.parse_ident()?;
            while self.match_token(&Token::Dot) {
                // 路径形式：取最后一段标识符
                name = self.parse_ident()?;
            }
            self.expect(Token::RParen)?;
            return Ok(Spanned::new(
                Expr::StringLit(name.to_string()),
                start.merge(self.prev_span()),
            ));
        }
        if self.match_token(&Token::Base) {
            return Ok(Spanned::new(Expr::Base, start.merge(self.prev_span())));
        }
        if self.match_token(&Token::Await) {
            let inner = self.parse_expr_bp(0)?;
            let end = inner.span;
            return Ok(Spanned::new(Expr::Await(Box::new(inner)), start.merge(end)));
        }
        // RFC 009 M6: `async` 前缀的异步 lambda。
        // 形式：`async () => ...` / `async (x: T) => ...` / `async x => ...`
        if self.check_async_lambda() {
            self.expect(Token::Async)?;
            if self.match_token(&Token::LParen) {
                return self.parse_lambda(start, false, true);
            }
            // `async ident => body` —— 单参数无括号形式
            let name = self.parse_ident()?;
            return self.parse_lambda_from_param(start, name.to_string(), false, true);
        }
        if self.is_deprecated_expression_lambda() {
            return Err(self.deprecated_expression_keyword_error(start));
        }
        if self.match_token(&Token::From) {
            return self.parse_query(start);
        }
        if self.match_token(&Token::Bang) {
            // Operand includes postfix (`.`, `()`) so `!a.B()` parses as `!(a.B())`,
            // matching C# precedence where member access binds tighter than unary `!`.
            // bp 13 > any binary operator's right-bp, so binary ops stay outside.
            let expr = self.parse_expr_bp(13)?;
            let end = expr.span;
            return Ok(Spanned::new(
                Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                start.merge(end),
            ));
        }
        if self.match_token(&Token::Minus) {
            let expr = self.parse_expr_bp(13)?;
            let end = expr.span;
            return Ok(Spanned::new(
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                start.merge(end),
            ));
        }
        if self.match_token(&Token::Tilde) {
            // `~` 位取反（整数）。绑定同 `!`/`-`（bp 13）。
            let expr = self.parse_expr_bp(13)?;
            let end = expr.span;
            return Ok(Spanned::new(
                Expr::Unary {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                },
                start.merge(end),
            ));
        }
        if self.match_token(&Token::LParen) {
            if self.check_lambda() {
                return self.parse_lambda(start, false, false);
            }
            // RFC 036 D9.1: `(T)x` 强制转换语法（取代 `x as T`）。
            //
            // 消歧策略：speculative parse — 保存 pos，尝试解析 Type + RParen，
            // 若成功且 `)` 后的 token 能作为 cast 操作数起点，则视为 cast；
            // 否则恢复 pos 并按括号表达式解析。
            //
            // 关键消歧规则（对齐 C#）：`(T)` 后必须是能开启 unary/prefix 表达式的
            // token 才视为 cast。例如：
            //   `(int)x`        → cast（`x` 是 ident，能开启 prefix）
            //   `(int)null`     → cast（`null` 能开启 prefix）
            //   `(passed) {`    → 括号表达式（`{` 不能开启 prefix，是 block 起点）
            //   `(a + b)`       → 括号表达式（`a` 后是 `+` 非 `)`）
            //   `(MyClass)obj`  → cast（`obj` 是 ident）
            let save_pos = self.pos;
            if let Ok(ty) = self.parse_type() {
                if self.match_token(&Token::RParen) && self.can_start_cast_operand() {
                    // 是 `(T)x` cast — 操作数包含 postfix（`.`/`[]`/`()`），
                    // 对齐 C# 语义：`(int)u.Score` = `(int)(u.Score)`，成员访问
                    // 比一元 cast 绑定更紧。**min_bp 须高于全部二元运算符
                    // 的 left_bp**（最高 19 = 乘/除/模），否则 `(long)a * 16777216`
                    // 会被解析为 `(long)(a * 16777216)`——乘法被 Cast 吞入，
                    // 乘法在窄域（int）执行后高位符号扩展失真
                    // （barcode argb 打包错位根因）。
                    let expr = self.parse_expr_bp(20)?;
                    let end = expr.span;
                    return Ok(Spanned::new(
                        Expr::Cast {
                            expr: Box::new(expr),
                            ty: ty.clone(),
                        },
                        start.merge(end),
                    ));
                }
            }
            // 恢复 pos 并按括号表达式解析
            self.pos = save_pos;
            let expr = self.parse_expr()?;
            self.expect(Token::RParen)?;
            return Ok(expr);
        }
        if self.match_token(&Token::If) {
            return self.parse_if_expr(start);
        }
        if self.match_token(&Token::Switch) {
            return self.parse_switch_expr(start);
        }
        if self.match_token(&Token::LBrace) {
            if self.is_block_start_after_lbrace() {
                let block = self.parse_block_inner()?;
                return Ok(Spanned::new(
                    Expr::Block(block),
                    start.merge(self.prev_span()),
                ));
            }
            return Err(self.bare_brace_initializer_error());
        }

        let expr = match self.advance().token.clone() {
            Token::Null => Expr::Null,
            Token::IntLit(n) => Expr::IntLit(n),
            Token::FloatLit(f) => Expr::FloatLit(f),
            Token::True => Expr::BoolLit(true),
            Token::False => Expr::BoolLit(false),
            Token::StringLit(s) => Expr::StringLit(s),
            Token::VerbatimString(s) => Expr::StringLit(s),
            Token::InterpolatedString(interior) => {
                // advance() 已消费 token；span 为整段 `$"..."`。
                let span = self.prev_span();
                return Ok(Spanned::new(
                    Self::parse_interpolated_from_interior(&interior, span)?,
                    span.merge(self.prev_span()),
                ));
            }
            Token::VerbatimInterpolatedString(interior) => {
                let span = self.prev_span();
                return Ok(Spanned::new(
                    Self::parse_verbatim_interpolated_from_interior(&interior, span)?,
                    span.merge(self.prev_span()),
                ));
            }
            Token::CharLit(c) => Expr::CharLit(c),
            // 基元类型关键字在表达式中可作标识符使用（如 long.Zero, float.Parse 等）。
            // int/bool/string 非关键字，走 Ident 分支即可。
            Token::Long => Expr::Ident("long".into()),
            Token::Short => Expr::Ident("short".into()),
            Token::Byte => Expr::Ident("byte".into()),
            Token::Char => Expr::Ident("char".into()),
            Token::Float => Expr::Ident("float".into()),
            Token::Double => Expr::Ident("double".into()),
            Token::UInt => Expr::Ident("uint".into()),
            Token::ULong => Expr::Ident("ulong".into()),
            Token::UShort => Expr::Ident("ushort".into()),
            Token::SByte => Expr::Ident("sbyte".into()),
            Token::Ident(name) => {
                if self.check(&Token::LBrace) && !self.is_block_start_after_lbrace() {
                    return Err(self.struct_without_new_error(&name));
                }
                if self.match_token(&Token::FatArrow) {
                    return self.parse_lambda_from_param(start, name, false, false);
                } else {
                    Expr::Ident(name.into())
                }
            }
            other => {
                return Err(ParseError::Unexpected {
                    span: start,
                    expected: "expression".into(),
                    found: format!("{other:?}"),
                });
            }
        };
        Ok(Spanned::new(expr, start.merge(self.prev_span())))
    }

    pub(crate) fn parse_call_args(&mut self) -> Result<Vec<Spanned<Expr>>, ParseError> {
        let mut args = Vec::new();
        let mut seen_named = false;
        if !self.check(&Token::RParen) {
            loop {
                let arg = if self.match_token(&Token::Ref) {
                    if seen_named {
                        return Err(self.error(
                            "positional argument",
                            "positional after named argument".into(),
                        ));
                    }
                    let inner = self.parse_expr()?;
                    Spanned::new(
                        Expr::RefArg {
                            is_out: false,
                            expr: Box::new(inner.clone()),
                        },
                        inner.span,
                    )
                } else if self.match_token(&Token::In) {
                    // RFC 009 P1-F #8：`in expr` 调用实参。
                    // 语义上等同于 `ref expr`（addr-of），readonly 约束在声明侧由 typeck 强制。
                    if seen_named {
                        return Err(self.error(
                            "positional argument",
                            "positional after named argument".into(),
                        ));
                    }
                    let inner = self.parse_expr()?;
                    Spanned::new(
                        Expr::RefArg {
                            is_out: false,
                            expr: Box::new(inner.clone()),
                        },
                        inner.span,
                    )
                } else if self.match_token(&Token::Out) {
                    if seen_named {
                        return Err(self.error(
                            "positional argument",
                            "positional after named argument".into(),
                        ));
                    }
                    let inner = self.parse_expr()?;
                    Spanned::new(
                        Expr::RefArg {
                            is_out: true,
                            expr: Box::new(inner.clone()),
                        },
                        inner.span,
                    )
                } else if self.peek_is_named_call_arg() {
                    // RFC 007：必须在 check_lambda 之前——`Ident :` 在调用实参中是命名实参，
                    // 而 check_lambda 把 `Ident :` 当作 typed lambda 形参。
                    seen_named = true;
                    let name_tok = self.advance();
                    let name = match &name_tok.token {
                        Token::Ident(s) => Ident::from(s.as_str()),
                        _ => unreachable!(),
                    };
                    let start = name_tok.span;
                    self.expect(Token::Colon)?;
                    let value = self.parse_expr()?;
                    Spanned::new(
                        Expr::NamedArg {
                            name,
                            expr: Box::new(value.clone()),
                        },
                        start.merge(value.span),
                    )
                } else if self.check_lambda() {
                    if seen_named {
                        return Err(self.error(
                            "positional argument",
                            "positional after named argument".into(),
                        ));
                    }
                    self.parse_prefix()?
                } else {
                    if seen_named {
                        return Err(self.error(
                            "positional argument",
                            "positional after named argument".into(),
                        ));
                    }
                    self.parse_expr()?
                };
                args.push(arg);
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RParen)?;
        Ok(args)
    }

    /// RFC 007：lookahead `Ident :`（命名实参），避免把普通表达式误判。
    fn peek_is_named_call_arg(&self) -> bool {
        matches!(self.peek().token, Token::Ident(_)) && self.check_at(1, &Token::Colon)
    }
}
