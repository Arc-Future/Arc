//! `.ani` 契约文件解析器（RFC 016 M1/M2/M3）。
//!
//! 复用现有 lexer 与 `Parser` 基础设施，解析 `native module <Name> { ... }` 语法。
//! 参数语法与 Arc 主体一致（`type name` 风格）；M2 扩展 `out`/`ref` 方向修饰符。
//! 类型白名单校验留待 typeck 阶段。
//!
//! `native`/`module`/`fn`/`out`/`ref`/`type`/`stdcall`/`capability`/`library`/`load`
//! 在 `.ani` 中作为关键字识别，但不修改 lexer——通过 `Ident` 匹配
//! 实现，避免影响 `.as` 文件的标识符空间。
//!
//! M3 扩展语法：
//! - `native type Name;`：不透明指针类型（对应 C `void*`）
//! - `native type Name { T1 f1; T2 f2; };`：契约 struct（按值传递）
//! - `stdcall fn ...`：函数级 stdcall 调用约定
//! - `capability <name>`：模块级能力 gating 标签（Phase 0 仅记录）
//!
//! RFC 016（用户裁决简化 2026-08-03：单一 `.ani` 协议，无多层路径托底）扩展语法：
//! - `load = "static" | "runtime" | "auto";`：模块加载策略
//! - `library = "vendor/gpu/lib";`：字面量相对路径（相对**执行程序根目录**）
//! - `library = Environment.GetEnvironmentVariable("NAME");`：环境变量形式——
//!   固定形态识别（不做通用表达式），typeck 做 registry 级强类型校验

use ast::*;

use crate::error::ParseError;
use crate::lexer::Token;
use crate::parser::Parser;
use std::path::PathBuf;

impl Parser {
    /// 解析 `.ani` 契约文件源码，返回 `NativeModule` AST。
    ///
    /// 入口方法供 loader 按 `.ani` 扩展名分派调用；不经过 `parse_program_items`，
    /// 因为契约文件不产生 `Program`/`Item::Namespace` 等常规顶层结构。
    pub fn parse_native_module(source: &str, file_id: FileId) -> Result<NativeModule, ParseError> {
        let tokens = crate::lexer::lex(source, file_id).map_err(|e| ParseError::Unexpected {
            span: e.span,
            expected: "valid token".into(),
            found: "invalid character".into(),
        })?;
        Self::new(tokens).parse_native_module_body()
    }

    fn parse_native_module_body(&mut self) -> Result<NativeModule, ParseError> {
        self.expect_keyword("native")?;
        self.expect_keyword("module")?;
        let name = self.parse_ident()?;
        self.expect(Token::LBrace)?;

        let mut functions = Vec::new();
        let mut types = Vec::new();
        let mut callbacks = Vec::new();
        let mut capability = None;
        let mut library = None;
        let mut library_env_var = None;
        let mut source = None;
        let mut load = LoadStrategy::Static;
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // RFC 016：跳过 `///` 文档注释——ani 文件允许在函数间书写文档，
            // 但 ani 解析器目前不收集 doc 到 AST，仅消费 token 以免阻塞。
            self.skip_doc_comments();
            if self.check(&Token::RBrace) || self.is_at_end() {
                break;
            }
            // RFC 016 M3：`capability <name>;` 模块级声明
            if self.peek_keyword("capability") {
                self.advance(); // consume `capability`
                let cap = self.parse_ident()?;
                self.expect(Token::Semi)?;
                capability = Some(cap);
                continue;
            }
            // RFC 016 M4：`library = ...;` 模块级库路径声明（单一 `.ani` 协议，
            // 无多层路径托底）。
            // 相对路径基准 = **执行程序根目录**（codegen 按 -o 输出可执行文件
            // 所在目录解析为绝对路径）；也支持绝对路径。
            //
            // 两种形态（单一惯用法，二选一）：
            // - 字面量路径：`library = "vendor/foo/lib";`（现状保留）
            // - 环境变量形式：`library = Environment.GetEnvironmentVariable("FOO_LIB");`
            //   识别**固定形态**（接收者 `Environment` · 方法 `GetEnvironmentVariable` ·
            //   单个字符串字面量参数），不引入通用表达式语法——`.ani` 目前只有简单声明，
            //   避免改动 parse 稳定面。typeck 阶段再做 registry 级强类型校验。
            if self.peek_keyword("library") {
                self.advance(); // consume `library`
                self.expect(Token::Eq)?;
                match &self.advance().token {
                    Token::StringLit(s) => {
                        let raw = s.clone();
                        self.expect(Token::Semi)?;
                        library = Some(PathBuf::from(raw));
                    }
                    Token::Ident(recv) if recv == "Environment" => {
                        self.expect(Token::Dot)?;
                        let method = self.parse_ident()?;
                        if method != "GetEnvironmentVariable" {
                            return Err(self.error(
                                "Environment.GetEnvironmentVariable",
                                format!("expected method `GetEnvironmentVariable`, got `{method}`"),
                            ));
                        }
                        self.expect(Token::LParen)?;
                        let name = match &self.advance().token {
                            Token::StringLit(s) => s.clone(),
                            _ => {
                                return Err(self.error(
                                    "environment variable name as string literal",
                                    self.describe_prev(),
                                ));
                            }
                        };
                        self.expect(Token::RParen)?;
                        self.expect(Token::Semi)?;
                        library_env_var = Some(name);
                    }
                    _ => {
                        return Err(self.error(
                            "string literal or Environment.GetEnvironmentVariable(\"...\")",
                            self.describe_prev(),
                        ));
                    }
                }
                continue;
            }
            // RFC 016：`load = "static" | "runtime" | "auto";` 模块加载策略。
            if self.peek_keyword("load") {
                self.advance(); // consume `load`
                self.expect(Token::Eq)?;
                let raw = match &self.advance().token {
                    Token::StringLit(s) => s.clone(),
                    _ => return Err(self.error("string literal", self.describe_prev())),
                };
                self.expect(Token::Semi)?;
                load = match raw.as_str() {
                    "static" => LoadStrategy::Static,
                    "runtime" => LoadStrategy::Runtime,
                    "auto" => LoadStrategy::Auto,
                    other => {
                        return Err(self.error(
                            "load strategy \"static\" | \"runtime\" | \"auto\"",
                            format!("invalid load strategy `{other}`"),
                        ));
                    }
                };
                continue;
            }
            // RFC 016（native 源实现增补）：`source = "path/to/foo.c";` 模块级 C 源
            // 实现声明——与 `library`（已编译外部库/DLL）平行。编译器据此发现并
            // 编译该 C 源、纳入链接；符号由本地 `.o` 提供，跳过外部 `-l<name>` 与
            // 外部库符号验证。路径基准 = 该 `.ani` 文件所在目录（加载器解析为绝对路径）。
            if self.peek_keyword("source") {
                self.advance(); // consume `source`
                self.expect(Token::Eq)?;
                let raw = match &self.advance().token {
                    Token::StringLit(s) => s.clone(),
                    _ => {
                        return Err(
                            self.error("string literal (C source path)", self.describe_prev())
                        )
                    }
                };
                self.expect(Token::Semi)?;
                source = Some(PathBuf::from(raw));
                continue;
            }
            // RFC 016 M3：`native type Name;` 或 `native type Name { ... };`
            if self.peek_keyword("native") && self.peek_n_is_keyword(1, "type") {
                self.advance(); // consume `native`
                self.advance(); // consume `type`
                types.push(self.parse_native_type_decl_tail()?);
                continue;
            }
            // RFC 016 M1：`native callback Name(params) -> ret;`
            if self.peek_keyword("native") && self.peek_n_is_keyword(1, "callback") {
                self.advance(); // consume `native`
                self.advance(); // consume `callback`
                callbacks.push(self.parse_native_callback()?);
                continue;
            }
            functions.push(self.parse_native_fn()?);
        }
        self.expect(Token::RBrace)?;
        Ok(NativeModule {
            name,
            functions,
            types,
            capability,
            callbacks,
            library,
            library_env_var,
            source,
            load,
        })
    }

    fn parse_native_fn(&mut self) -> Result<NativeFn, ParseError> {
        // RFC 016 M3：可选 `stdcall` 修饰符，指定函数调用约定。
        let calling_conv = if self.match_keyword("stdcall") {
            CallingConv::Stdcall
        } else {
            CallingConv::C
        };
        self.expect_keyword("fn")?;
        let name = self.parse_ident()?;

        // 显式 C 符号名覆盖：`fn Name = symbol ( ... )`
        // 用于 Arc 风格 PascalCase 与 C 风格 snake_case 不一致的场景。
        let symbol = if self.match_token(&Token::Eq) {
            Some(self.parse_ident()?)
        } else {
            None
        };

        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                // RFC 016 M2：可选 `out`/`ref` 方向修饰符，默认 `In`。
                let direction = self.parse_param_direction()?;
                let ty = self.parse_type()?;
                let pname = self.parse_arci_param_name()?;
                params.push(NativeParam {
                    name: pname,
                    ty,
                    direction,
                });
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RParen)?;

        let ret = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(Token::Semi)?;

        Ok(NativeFn {
            name,
            symbol,
            params,
            ret,
            calling_conv,
        })
    }

    /// 解析 `native callback` 声明——已消费 `native` `callback` 两个关键字。
    ///
    /// RFC 016 M1：语法 `native callback Name(T1 p1, T2 p2) -> Ret;`
    /// 参数语法与 `extern fn` 一致（RFC 016 §3.3 类型白名单约束），
    /// 但不支持 `out`/`ref` 方向修饰符（回调参数仅 `In`）。
    fn parse_native_callback(&mut self) -> Result<NativeCallback, ParseError> {
        // RFC 016 M3：可选 `stdcall` 修饰符。
        let calling_conv = if self.match_keyword("stdcall") {
            CallingConv::Stdcall
        } else {
            CallingConv::C
        };
        let name = self.parse_ident()?;
        self.expect(Token::LParen)?;
        let mut params = Vec::new();
        if !self.check(&Token::RParen) {
            loop {
                let ty = self.parse_type()?;
                let pname = self.parse_arci_param_name()?;
                params.push(NativeParam {
                    name: pname,
                    ty,
                    direction: ParamDirection::In,
                });
                if !self.match_token(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(Token::RParen)?;

        let ret = if self.match_token(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(Token::Semi)?;

        Ok(NativeCallback {
            name,
            params,
            ret,
            calling_conv,
        })
    }

    /// 解析 `native type` 声明尾部——已消费 `native` `type` 两个关键字。
    ///
    /// 两种形式：
    /// - `Name;` → `NativeTypeKind::OpaquePtr`
    /// - `Name { T1 f1; T2 f2; };` → `NativeTypeKind::Struct { fields }`
    fn parse_native_type_decl_tail(&mut self) -> Result<NativeTypeDecl, ParseError> {
        let name = self.parse_ident()?;
        let kind = if self.match_token(&Token::Semi) {
            NativeTypeKind::OpaquePtr
        } else {
            self.expect(Token::LBrace)?;
            let mut fields = Vec::new();
            while !self.check(&Token::RBrace) && !self.is_at_end() {
                let fty = self.parse_type()?;
                let fname = self.parse_arci_param_name()?;
                self.expect(Token::Semi)?;
                fields.push((fname, fty));
            }
            self.expect(Token::RBrace)?;
            self.expect(Token::Semi)?;
            NativeTypeKind::Struct { fields }
        };
        Ok(NativeTypeDecl { name, kind })
    }

    /// 解析可选的参数方向修饰符（RFC 016 M2）。
    ///
    /// - `out` → `ParamDirection::Out`
    /// - `ref` → `ParamDirection::InOut`
    /// - 无修饰符 → `ParamDirection::In`（默认）
    fn parse_param_direction(&mut self) -> Result<ParamDirection, ParseError> {
        match &self.peek().token {
            Token::Out => {
                self.advance();
                Ok(ParamDirection::Out)
            }
            Token::Ref => {
                self.advance();
                Ok(ParamDirection::InOut)
            }
            _ => Ok(ParamDirection::In),
        }
    }

    /// 解析 native 函数参数名——接受 Arc 关键字 token 作为参数名。
    ///
    /// `.ani` 中的参数名可能碰巧是 Arc 语言保留字（如 `base` 会被 lexer
    /// 生成 `Token::Base`），但在 native 模块上下文中它们只是 C 参数名，
    /// 不承载 Arc 语言语义。此方法接受所有可映射为字符串的 token。
    fn parse_arci_param_name(&mut self) -> Result<Ident, ParseError> {
        let token = self.advance().token.clone();
        if let Some(s) = token_to_ident_str(&token) {
            Ok(s.into())
        } else {
            Err(self.error("identifier", self.describe_prev()))
        }
    }

    /// 匹配作为关键字使用的标识符（`native`/`module`/`fn`）。
    ///
    /// 不修改 lexer，避免影响 `.as` 文件的标识符空间——这些词在 `.as` 中
    /// 仍是合法标识符，仅在 `.ani` 上下文按关键字解析。
    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        match &self.peek().token.clone() {
            Token::Ident(s) if s == kw => {
                self.advance();
                Ok(())
            }
            _ => Err(self.error(kw, self.describe_current())),
        }
    }

    /// RFC 016 M3：匹配作为关键字使用的标识符；匹配成功消费并返回 `true`。
    fn match_keyword(&mut self, kw: &str) -> bool {
        if self.peek_keyword(kw) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// RFC 016 M3：当前 token 是否为指定名称的标识符关键字。
    fn peek_keyword(&self, kw: &str) -> bool {
        matches!(&self.peek().token, Token::Ident(s) if s == kw)
    }

    /// RFC 016 M3：第 `n` 个 lookahead token 是否为指定名称的标识符关键字。
    /// `n=0` 等价于 `peek_keyword`。Parser 未暴露 `peek_n`，直接访问 `tokens` 字段。
    fn peek_n_is_keyword(&self, n: usize, kw: &str) -> bool {
        matches!(
            self.tokens.get(self.pos + n).map(|t| &t.token),
            Some(Token::Ident(s)) if s == kw
        )
    }
}

/// 将 Arc 语言关键字 token 还原为其字符串表示。
///
/// `.ani` 契约文件中使用的标识符（如参数名 `base`、类型名 `class` 等）
/// 可能被 lexer 按 Arc 语言规范识别为关键字 token，但在 native 模块上下文
/// 中它们只是普通 C 标识符。此函数将所有关键字 token 映射回其源字符串。
fn token_to_ident_str(token: &Token) -> Option<&str> {
    match token {
        Token::Ident(s) => Some(s.as_str()),
        Token::Namespace => Some("namespace"),
        Token::Using => Some("using"),
        Token::Global => Some("global"),
        Token::Struct => Some("struct"),
        Token::Class => Some("class"),
        Token::Record => Some("record"),
        Token::With => Some("with"),
        Token::Interface => Some("interface"),
        Token::Enum => Some("enum"),
        Token::Variant => Some("variant"),
        Token::Async => Some("async"),
        Token::Await => Some("await"),
        Token::From => Some("from"),
        Token::Where => Some("where"),
        Token::Select => Some("select"),
        Token::OrderBy => Some("orderby"),
        Token::Join => Some("join"),
        Token::On => Some("on"),
        Token::Group => Some("group"),
        Token::By => Some("by"),
        Token::Into => Some("into"),
        Token::Let => Some("let"),
        Token::Var => Some("var"),
        Token::If => Some("if"),
        Token::Else => Some("else"),
        Token::While => Some("while"),
        Token::For => Some("for"),
        Token::Foreach => Some("foreach"),
        Token::In => Some("in"),
        Token::Return => Some("return"),
        Token::Switch => Some("switch"),
        Token::Case => Some("case"),
        Token::Default => Some("default"),
        Token::Break => Some("break"),
        Token::Continue => Some("continue"),
        Token::Throw => Some("throw"),
        Token::Try => Some("try"),
        Token::Catch => Some("catch"),
        Token::Finally => Some("finally"),
        Token::Lock => Some("lock"),
        Token::Public => Some("public"),
        Token::Private => Some("private"),
        Token::Internal => Some("internal"),
        Token::Protected => Some("protected"),
        Token::Void => Some("void"),
        Token::Float => Some("float"),
        Token::Double => Some("double"),
        Token::Long => Some("long"),
        Token::Short => Some("short"),
        Token::Byte => Some("byte"),
        Token::Char => Some("char"),
        Token::UInt => Some("uint"),
        Token::ULong => Some("ulong"),
        Token::UShort => Some("ushort"),
        Token::SByte => Some("sbyte"),
        Token::True => Some("true"),
        Token::False => Some("false"),
        Token::New => Some("new"),
        Token::Virtual => Some("virtual"),
        Token::Override => Some("override"),
        Token::Abstract => Some("abstract"),
        Token::Static => Some("static"),
        Token::Operator => Some("operator"),
        Token::Const => Some("const"),
        Token::Readonly => Some("readonly"),
        Token::Ref => Some("ref"),
        Token::Out => Some("out"),
        Token::This => Some("this"),
        Token::Base => Some("base"),
        Token::Descending => Some("descending"),
        Token::Null => Some("null"),
        Token::TypeOf => Some("typeof"),
        Token::NameOf => Some("nameof"),
        Token::Is => Some("is"),
        Token::When => Some("when"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_fn_with_ret() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.name, "libc");
        assert_eq!(m.functions.len(), 1);
        assert!(m.types.is_empty());
        assert!(m.capability.is_none());
        let f = &m.functions[0];
        assert_eq!(f.name, "puts");
        assert!(f.symbol.is_none());
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name, "s");
        assert!(f.ret.is_some());
        assert_eq!(f.calling_conv, CallingConv::C);
    }

    #[test]
    fn parse_no_params_no_ret() {
        let src = "native module libc { fn exit(int code); }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.functions.len(), 1);
        let f = &m.functions[0];
        assert_eq!(f.name, "exit");
        assert_eq!(f.params.len(), 1);
        assert!(f.ret.is_none());
    }

    #[test]
    fn parse_multi_params() {
        let src = "native module libc { fn foo(int a, int b) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        let f = &m.functions[0];
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].name, "a");
        assert_eq!(f.params[1].name, "b");
    }

    #[test]
    fn parse_symbol_override() {
        let src = "native module libc { fn Puts = puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        let f = &m.functions[0];
        assert_eq!(f.name, "Puts");
        assert_eq!(f.symbol.as_ref().unwrap(), "puts");
    }

    #[test]
    fn parse_nullable_ret() {
        let src = "native module libc { fn getenv(string name) -> string?; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        let f = &m.functions[0];
        assert!(f.ret.is_some());
        // 返回类型是 string?（Nullable）
        match &f.ret.as_ref().unwrap().node {
            Type::Nullable { .. } => {}
            other => panic!("expected Nullable, got {other:?}"),
        }
    }

    #[test]
    fn parse_multi_fns() {
        let src = "native module libc { fn puts(string s) -> int; fn getpid() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.functions.len(), 2);
        assert_eq!(m.functions[0].name, "puts");
        assert_eq!(m.functions[1].name, "getpid");
    }

    #[test]
    fn parse_out_param() {
        let src = "native module libc { fn sscanf(string s, string fmt, out int result) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        let f = &m.functions[0];
        assert_eq!(f.params.len(), 3);
        assert_eq!(f.params[2].name, "result");
        assert_eq!(f.params[2].direction, ParamDirection::Out);
    }

    #[test]
    fn parse_ref_param() {
        let src = "native module libc { fn fread(ref int count) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        let f = &m.functions[0];
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].direction, ParamDirection::InOut);
    }

    #[test]
    fn parse_default_in_direction() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.functions[0].params[0].direction, ParamDirection::In);
    }

    #[test]
    fn err_missing_native_keyword() {
        let src = "module libc { fn puts(string s) -> int; }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    #[test]
    fn err_missing_semi() {
        let src = "native module libc { fn puts(string s) -> int }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    #[test]
    fn err_missing_lbrace() {
        let src = "native module libc fn puts(string s) -> int;";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    #[test]
    fn parse_stdcall_calling_conv() {
        let src = "native module win32 { stdcall fn MessageBox(int hwnd, string text, string caption, int flags) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.functions[0].calling_conv, CallingConv::Stdcall);
    }

    #[test]
    fn parse_default_calling_conv_is_c() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.functions[0].calling_conv, CallingConv::C);
    }

    #[test]
    fn parse_native_type_opaque_ptr() {
        let src = "native module libc { native type FILE; fn fputs(string s, FILE f) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.types.len(), 1);
        assert_eq!(m.types[0].name, "FILE");
        assert_eq!(m.types[0].kind, NativeTypeKind::OpaquePtr);
    }

    #[test]
    fn parse_native_type_struct() {
        let src = "native module libc { native type Point { int x; int y; }; fn make_point(int x, int y) -> Point; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.types.len(), 1);
        assert_eq!(m.types[0].name, "Point");
        match &m.types[0].kind {
            NativeTypeKind::Struct { fields } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "x");
                assert_eq!(fields[1].0, "y");
            }
            other => panic!("expected Struct, got {other:?}"),
        }
    }

    #[test]
    fn parse_capability_modifier() {
        let src = "native module libc { capability os_io; fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.capability.as_deref(), Some("os_io"));
        assert_eq!(m.functions.len(), 1);
    }

    /// RFC 016 M4：解析 `library = "..."` 模块级库目录声明。
    #[test]
    fn parse_library_declaration() {
        let src =
            "native module browser { library = \"vendor/chromium/lib\"; fn launch() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(
            m.library.as_deref(),
            Some(std::path::Path::new("vendor/chromium/lib"))
        );
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].name, "launch");
    }

    /// RFC 016 M4：library 与 capability 可同时声明，顺序无关。
    #[test]
    fn parse_library_and_capability_together() {
        let src = "native module foo { capability gpu; library = \"vendor/vk/lib\"; fn vk_init() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.capability.as_deref(), Some("gpu"));
        assert_eq!(
            m.library.as_deref(),
            Some(std::path::Path::new("vendor/vk/lib"))
        );
    }

    /// RFC 016 M4：缺省时 library 为 None。
    #[test]
    fn parse_library_default_none() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert!(m.library.is_none());
        assert!(m.library_env_var.is_none());
    }

    /// RFC 016：`library = Environment.GetEnvironmentVariable("NAME");` 环境变量
    /// 形式解析为 `library_env_var`（固定形态识别）。
    #[test]
    fn parse_library_env_var_expression() {
        let src = "native module gpu { load = \"runtime\"; library = Environment.GetEnvironmentVariable(\"ARC_GPU_LIB\"); fn gpu_init() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert!(m.library.is_none());
        assert_eq!(m.library_env_var.as_deref(), Some("ARC_GPU_LIB"));
        assert_eq!(m.load, LoadStrategy::Runtime);
        assert_eq!(m.functions.len(), 1);
    }

    /// RFC 016：环境变量形式与 capability/load 可同时声明，顺序无关。
    #[test]
    fn parse_library_env_var_with_capability_and_load() {
        let src = "native module gpu { capability gpu; library = Environment.GetEnvironmentVariable(\"ARC_GPU_PATH\"); load = \"auto\"; fn gpu_init() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.capability.as_deref(), Some("gpu"));
        assert_eq!(m.library_env_var.as_deref(), Some("ARC_GPU_PATH"));
        assert!(m.library.is_none());
        assert_eq!(m.load, LoadStrategy::Auto);
    }

    /// RFC 016：环境变量形式方法名错误 → 解析错误。
    #[test]
    fn err_library_env_var_wrong_method() {
        let src = "native module gpu { library = Environment.GetEnv(\"ARC_GPU_LIB\"); fn f(); }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    /// RFC 016：环境变量形式参数必须为字符串字面量。
    #[test]
    fn err_library_env_var_non_literal_arg() {
        let src =
            "native module gpu { library = Environment.GetEnvironmentVariable(FOO); fn f(); }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    /// RFC 016：library 右侧既非字符串字面量也非 Environment 表达式 → 解析错误。
    #[test]
    fn err_library_invalid_rhs() {
        let src = "native module gpu { library = GpuSearchPaths; fn f(); }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    /// RFC 016 M1：解析 `native callback` 声明。
    #[test]
    fn parse_native_callback_simple() {
        let src = "native module libc { native callback CmpFn(NativePtr a, NativePtr b) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.callbacks.len(), 1);
        let cb = &m.callbacks[0];
        assert_eq!(cb.name, "CmpFn");
        assert_eq!(cb.params.len(), 2);
        assert_eq!(cb.params[0].name, "a");
        assert_eq!(cb.params[1].name, "b");
        assert!(cb.ret.is_some());
        assert_eq!(cb.calling_conv, CallingConv::C);
    }

    /// RFC 016 M1：解析 `native callback` 与 `native type` 和 `fn` 混合。
    /// 回调类型名（如 CmpFn）作为 fn 参数类型时，由 `parse_type()` 通过
    /// `parse_ident()` 路径识别为 `Type::Named { path: ["CmpFn"] }`。
    #[test]
    fn parse_native_callback_mixed() {
        let src = "native module libc {
            native callback CmpFn(NativePtr a, NativePtr b) -> int;
            fn qsort(NativePtr base, NativePtr nmemb, NativePtr size, CmpFn cmp);
        }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.callbacks.len(), 1);
        assert_eq!(m.functions.len(), 1);
    }

    /// RFC 016 M1：`native callback` 无返回值（void）。
    #[test]
    fn parse_native_callback_no_ret() {
        let src = "native module libc { native callback Action(NativePtr ctx); }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.callbacks.len(), 1);
        assert!(m.callbacks[0].ret.is_none());
    }

    /// RFC 016：`load = "runtime"` 解析为 `LoadStrategy::Runtime`。
    #[test]
    fn parse_load_runtime() {
        let src = "native module gpu { load = \"runtime\"; fn gpu_init() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.load, LoadStrategy::Runtime);
        assert_eq!(m.functions.len(), 1);
    }

    /// RFC 016：`load = "static"` / `load = "auto"` 解析。
    #[test]
    fn parse_load_static_and_auto() {
        let src = "native module a { load = \"static\"; fn fa(); }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.load, LoadStrategy::Static);
        let src2 = "native module b { load = \"auto\"; fn fb(); }";
        let m2 = Parser::parse_native_module(src2, 0).unwrap();
        assert_eq!(m2.load, LoadStrategy::Auto);
    }

    /// RFC 016：缺省时 `load` 为 `Static`（零行为变更基线）。
    #[test]
    fn parse_load_default_static() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.load, LoadStrategy::Static);
    }

    /// RFC 016：非法 `load` 值 → 解析错误。
    #[test]
    fn err_invalid_load_strategy() {
        let src = "native module gpu { load = \"lazy\"; fn f(); }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }

    /// RFC 016：`load`、`library`、`capability` 可同时声明，顺序无关。
    #[test]
    fn parse_load_library_capability_together() {
        let src = "native module gpu {
            capability gpu;
            load = \"auto\";
            library = \"vendor/vk/lib\";
            fn vk_init() -> int;
        }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.capability.as_deref(), Some("gpu"));
        assert_eq!(m.load, LoadStrategy::Auto);
        assert_eq!(
            m.library.as_deref(),
            Some(std::path::Path::new("vendor/vk/lib"))
        );
    }

    /// RFC 016：缺省时 library 为 None（相对路径保持相对，基准解析在 codegen）。
    #[test]
    fn parse_library_relative_stays_relative() {
        let src = "native module libc { fn puts(string s) -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert!(m.library.is_none());
    }

    /// RFC 016（native 源实现增补）：`source = "foo.c";` C 源实现声明。
    #[test]
    fn parse_source_declaration() {
        let src = "native module foo { source = \"src/foo.c\"; fn ping() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert_eq!(m.source.as_deref(), Some(std::path::Path::new("src/foo.c")));
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].name, "ping");
    }

    /// RFC 016（native 源实现增补）：缺省时 source 为 None（DLL 设计不变）。
    #[test]
    fn parse_source_default_none() {
        let src = "native module foo { fn ping() -> int; }";
        let m = Parser::parse_native_module(src, 0).unwrap();
        assert!(m.source.is_none());
    }

    /// RFC 016（native 源实现增补）：source 值非字符串字面量 → 解析错误。
    #[test]
    fn err_source_non_literal() {
        let src = "native module foo { source = foo_c; fn ping() -> int; }";
        assert!(Parser::parse_native_module(src, 0).is_err());
    }
}
