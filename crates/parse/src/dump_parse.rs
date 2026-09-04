//! Serialize `parse_program` AST for golden / Arc parser parity (RFC 019 M4).
//!
//! Format (frozen with [`019-m4-parser-subset.md`] §2.1):
//! - One node summary per line; **2-space** indent for children
//! - `Tag` or `Tag\t` + escaped fields (field order fixed per tag)
//! - Escapes match `dump_lex`: `\\` `\n` `\t` `\r`
//! - No span / file_id payloads
//! - `FloatLit` uses the source slice (avoids float print drift)
//! - `if` / `switch` statement forms flatten to `Stmt.If` / `Stmt.Switch`

use crate::error::ParseError;
use crate::parser::Parser;
use ast::*;

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

fn line(tag: &str, fields: &[&str]) -> String {
    if fields.is_empty() {
        tag.to_string()
    } else {
        let mut s = tag.to_string();
        for f in fields {
            s.push('\t');
            s.push_str(&dump_escape(f));
        }
        s
    }
}

fn path_join(path: &[Ident]) -> String {
    path.iter()
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn vis_str(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "Public",
        Visibility::Private => "Private",
        Visibility::Internal => "Internal",
        Visibility::Protected => "Protected",
    }
}

fn bin_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "Add",
        BinOp::Sub => "Sub",
        BinOp::Mul => "Mul",
        BinOp::Div => "Div",
        BinOp::Mod => "Mod",
        BinOp::Eq => "Eq",
        BinOp::NotEq => "NotEq",
        BinOp::Lt => "Lt",
        BinOp::Le => "Le",
        BinOp::Gt => "Gt",
        BinOp::Ge => "Ge",
        BinOp::And => "And",
        BinOp::Or => "Or",
        BinOp::BitAnd => "BitAnd",
        BinOp::BitOr => "BitOr",
        BinOp::BitXor => "BitXor",
        BinOp::Shl => "Shl",
        BinOp::Shr => "Shr",
    }
}

fn unary_op_str(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "Not",
        UnaryOp::Neg => "Neg",
        UnaryOp::BitNot => "BitNot",
    }
}

struct Dumper<'a> {
    source: &'a str,
    out: String,
    depth: usize,
}

impl<'a> Dumper<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            out: String::new(),
            depth: 0,
        }
    }

    fn emit(&mut self, tag: &str, fields: &[&str]) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(&line(tag, fields));
        self.out.push('\n');
    }

    fn with_child<F: FnOnce(&mut Self)>(&mut self, f: F) {
        self.depth += 1;
        f(self);
        self.depth -= 1;
    }

    fn slice(&self, span: Span) -> &str {
        let start = span.start as usize;
        let end = (span.end as usize).min(self.source.len());
        if start >= end || start >= self.source.len() {
            ""
        } else {
            &self.source[start..end]
        }
    }

    fn dump_program(&mut self, prog: &Program) {
        self.emit("Program", &[]);
        self.with_child(|d| {
            for item in &prog.items {
                d.dump_item(&item.node);
            }
        });
    }

    fn dump_item(&mut self, item: &Item) {
        match item {
            Item::Use(u) => {
                // M4 subset: no alias, no global using
                self.emit("Using", &[&path_join(&u.path)]);
            }
            Item::Namespace(ns) => {
                self.emit("Namespace", &[&path_join(&ns.path)]);
                self.with_child(|d| {
                    for it in &ns.items {
                        d.dump_item(&it.node);
                    }
                });
            }
            Item::Fn(f) => {
                self.emit("Fn", &[vis_str(f.vis), f.name.as_str()]);
                self.with_child(|d| {
                    if let Some(ret) = &f.ret {
                        d.dump_type(&ret.node);
                    } else {
                        d.emit("Type.Named", &["void"]);
                    }
                    for p in &f.params {
                        d.dump_param(p);
                    }
                    if let Some(body) = &f.body {
                        d.dump_block(body);
                    }
                });
            }
            Item::Class(c) => {
                self.emit("Class", &[vis_str(c.vis), c.name.as_str()]);
                self.with_child(|d| {
                    for field in &c.fields {
                        d.dump_field(field);
                    }
                    for m in &c.methods {
                        d.dump_method(&m.node);
                    }
                });
            }
            Item::Struct(s) => {
                self.emit("Struct", &[vis_str(s.vis), s.name.as_str()]);
                self.with_child(|d| {
                    for field in &s.fields {
                        d.dump_field(field);
                    }
                    for m in &s.methods {
                        d.dump_method(&m.node);
                    }
                });
            }
            Item::Enum(e) => {
                self.emit("Enum", &[vis_str(e.vis), e.name.as_str()]);
                self.with_child(|d| {
                    for v in &e.variants {
                        d.emit("EnumVariant", &[v.name.as_str()]);
                    }
                });
            }
            Item::Delegate(def) => {
                self.emit("Delegate", &[vis_str(def.vis), def.name.as_str()]);
                self.with_child(|dumper| {
                    if let Some(ret) = &def.ret {
                        dumper.dump_type(&ret.node);
                    } else {
                        dumper.emit("Type.Named", &["void"]);
                    }
                    for p in &def.params {
                        dumper.dump_param(p);
                    }
                });
            }
            other => {
                let s = format!("{other:?}");
                let preview: String = s.chars().take(40).collect();
                self.emit("Unsupported.Item", &[&preview]);
            }
        }
    }

    fn dump_field(&mut self, f: &FieldDef) {
        self.emit("Field", &[vis_str(f.vis), f.name.as_str()]);
        self.with_child(|d| {
            d.dump_type(&f.ty.node);
            if let Some(init) = &f.init {
                d.dump_expr(init);
            }
        });
    }

    fn dump_method(&mut self, m: &MethodDef) {
        let sig = &m.sig;
        self.emit("Method", &[vis_str(sig.vis), sig.name.as_str()]);
        self.with_child(|d| {
            if let Some(ret) = &sig.ret {
                d.dump_type(&ret.node);
            } else {
                d.emit("Type.Named", &["void"]);
            }
            for p in &sig.params {
                d.dump_param(p);
            }
            if let Some(body) = &m.body {
                d.dump_block(body);
            }
        });
    }

    fn dump_param(&mut self, p: &Param) {
        self.emit("Param", &[p.name.as_str()]);
        self.with_child(|d| d.dump_type(&p.ty.node));
    }

    fn dump_type(&mut self, ty: &Type) {
        match ty {
            Type::Named { path, generics } if generics.is_empty() => {
                self.emit("Type.Named", &[&path_join(path)]);
            }
            Type::Array { inner } => {
                self.emit("Type.Array", &[]);
                self.with_child(|d| d.dump_type(&inner.node));
            }
            other => {
                let s = format!("{other:?}");
                let preview: String = s.chars().take(40).collect();
                self.emit("Unsupported.Type", &[&preview]);
            }
        }
    }

    fn dump_block(&mut self, block: &Block) {
        self.emit("Block", &[]);
        self.with_child(|d| {
            for s in &block.stmts {
                d.dump_stmt(&s.node);
            }
            if let Some(tail) = &block.tail {
                d.dump_expr(tail);
            }
        });
    }

    fn dump_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                let kind = if ty.is_none() { "var" } else { "typed" };
                self.emit("Stmt.Let", &[kind, name.as_str()]);
                self.with_child(|d| {
                    if let Some(t) = ty {
                        d.dump_type(&t.node);
                    }
                    if let Some(e) = init {
                        d.dump_expr(e);
                    }
                });
            }
            Stmt::Expr(e) => match &e.node {
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    self.emit("Stmt.If", &[]);
                    self.with_child(|d| {
                        d.dump_expr(cond);
                        d.dump_block(then_branch);
                        if let Some(els) = else_branch {
                            d.dump_block(els);
                        }
                    });
                }
                Expr::Switch(sw) => {
                    self.emit("Stmt.Switch", &[]);
                    self.with_child(|d| {
                        d.dump_expr(&sw.scrutinee);
                        for case in &sw.cases {
                            d.dump_switch_case(case);
                        }
                    });
                }
                _ => {
                    self.emit("Stmt.Expr", &[]);
                    self.with_child(|d| d.dump_expr(e));
                }
            },
            Stmt::Return(val) => {
                self.emit("Stmt.Return", &[]);
                if let Some(e) = val {
                    self.with_child(|d| d.dump_expr(e));
                }
            }
            Stmt::While { cond, body } => {
                self.emit("Stmt.While", &[]);
                self.with_child(|d| {
                    d.dump_expr(cond);
                    d.dump_block(body);
                });
            }
            Stmt::ForC {
                init,
                cond,
                inc,
                body,
            } => {
                self.emit("Stmt.ForC", &[]);
                self.with_child(|d| {
                    if let Some(i) = init {
                        d.dump_stmt(&i.node);
                    } else {
                        d.emit("ForC.InitEmpty", &[]);
                    }
                    if let Some(c) = cond {
                        d.dump_expr(c);
                    } else {
                        d.emit("ForC.CondEmpty", &[]);
                    }
                    if let Some(i) = inc {
                        d.dump_stmt(&i.node);
                    } else {
                        d.emit("ForC.IncEmpty", &[]);
                    }
                    d.dump_block(body);
                });
            }
            Stmt::Assign { target, value } => {
                self.emit("Stmt.Assign", &[]);
                self.with_child(|d| {
                    d.dump_expr(target);
                    d.dump_expr(value);
                });
            }
            Stmt::Break => self.emit("Stmt.Break", &[]),
            Stmt::Continue => self.emit("Stmt.Continue", &[]),
            other => {
                let s = format!("{other:?}");
                let preview: String = s.chars().take(40).collect();
                self.emit("Unsupported.Stmt", &[&preview]);
            }
        }
    }

    fn dump_switch_case(&mut self, case: &SwitchCase) {
        match &case.pattern {
            None => self.emit("SwitchCase", &["default"]),
            Some(Pattern::Ident(id)) => self.emit("SwitchCase", &["Ident", id.as_str()]),
            Some(Pattern::Literal(lit)) => {
                self.emit("SwitchCase", &["Lit"]);
                self.with_child(|d| {
                    d.dump_expr(lit);
                    d.dump_block(&case.body);
                });
                return;
            }
            Some(other) => {
                let s = format!("{other:?}");
                let preview: String = s.chars().take(40).collect();
                self.emit("SwitchCase", &["Unsupported", &preview]);
            }
        }
        self.with_child(|d| d.dump_block(&case.body));
    }

    fn dump_expr(&mut self, expr: &Spanned<Expr>) {
        match &expr.node {
            Expr::IntLit(n) => self.emit("Expr.IntLit", &[&n.to_string()]),
            Expr::FloatLit(_) => {
                let slice = self.slice(expr.span).to_string();
                self.emit("Expr.FloatLit", &[&slice]);
            }
            Expr::BoolLit(b) => self.emit("Expr.BoolLit", &[if *b { "true" } else { "false" }]),
            Expr::StringLit(s) => self.emit("Expr.StringLit", &[s]),
            Expr::CharLit(c) => {
                let mut buf = String::new();
                buf.push(*c);
                self.emit("Expr.CharLit", &[&buf]);
            }
            Expr::Null => self.emit("Expr.Null", &[]),
            Expr::This => self.emit("Expr.This", &[]),
            Expr::Ident(id) => self.emit("Expr.Ident", &[id.as_str()]),
            Expr::Path(segs) => self.emit("Expr.Path", &[&path_join(segs)]),
            Expr::Binary { op, left, right } => {
                self.emit("Expr.Binary", &[bin_op_str(*op)]);
                self.with_child(|d| {
                    d.dump_expr(left);
                    d.dump_expr(right);
                });
            }
            Expr::Unary { op, expr: inner } => {
                self.emit("Expr.Unary", &[unary_op_str(*op)]);
                self.with_child(|d| d.dump_expr(inner));
            }
            Expr::Call { func, args, .. } => {
                self.emit("Expr.Call", &[]);
                self.with_child(|d| {
                    d.dump_expr(func);
                    for a in args {
                        d.dump_expr(a);
                    }
                });
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                self.emit("Expr.MethodCall", &[method.as_str()]);
                self.with_child(|d| {
                    d.dump_expr(receiver);
                    for a in args {
                        d.dump_expr(a);
                    }
                });
            }
            Expr::Field { receiver, field } => {
                self.emit("Expr.Field", &[field.as_str()]);
                self.with_child(|d| d.dump_expr(receiver));
            }
            Expr::Index { receiver, index } => {
                self.emit("Expr.Index", &[]);
                self.with_child(|d| {
                    d.dump_expr(receiver);
                    d.dump_expr(index);
                });
            }
            Expr::New { ty, args, obj_init } if obj_init.is_none() => {
                self.emit("Expr.New", &[]);
                self.with_child(|d| {
                    d.dump_type(&ty.node);
                    for a in args {
                        d.dump_expr(a);
                    }
                });
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.emit("Expr.If", &[]);
                self.with_child(|d| {
                    d.dump_expr(cond);
                    d.dump_block(then_branch);
                    if let Some(els) = else_branch {
                        d.dump_block(els);
                    }
                });
            }
            other => {
                let s = format!("{other:?}");
                let preview: String = s.chars().take(40).collect();
                self.emit("Unsupported.Expr", &[&preview]);
            }
        }
    }
}

/// Parse `source` and serialize the AST dump for M4 parity.
pub fn dump_parse(source: &str) -> Result<String, ParseError> {
    let prog = Parser::parse_program(source)?;
    let mut d = Dumper::new(source);
    d.dump_program(&prog);
    Ok(d.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        // RFC 045：fixtures 随 0514f024（清理提交）从 `compiler/parser/fixtures`
        // 迁入本 crate（`crates/parse/fixtures`）——旧路径引用已删除资源导致
        // 单元测试红（dump_parse 形状断言）。
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
    }

    fn read_fixture(name: &str) -> String {
        let path = fixtures_dir().join(name);
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    #[test]
    fn dump_parse_smoke_shape() {
        let dump = dump_parse(&read_fixture("smoke.as")).expect("parse smoke");
        assert!(dump.starts_with("Program\n"), "root Program");
        assert!(dump.contains("  Using\tArc\n"), "I1 using");
        assert!(dump.contains("  Namespace\tDemo\n"), "I2 namespace");
        assert!(
            dump.contains("    Fn\tPrivate\tAdd\n"),
            "I3 free fn in namespace (default Private)"
        );
        assert!(dump.contains("Stmt.Let\ttyped\tz\n"), "S1 typed let");
        assert!(dump.contains("Stmt.Let\tvar\tw\n"), "S1 var let");
        assert!(dump.contains("Stmt.Return\n"), "S3 return");
        assert!(dump.contains("Expr.Binary\tAdd\n"), "E4 binary");
        assert!(dump.contains("Expr.Call\n"), "E5 call");
        assert!(dump.contains("Expr.Index\n"), "E6 index");
        assert!(dump.contains("Expr.IntLit\t1\n"), "E1 int");
        assert!(dump.contains("Expr.StringLit\thi\\n\n"), "E1 string escape");
        assert!(
            dump.contains("Expr.FloatLit\t1.5\n"),
            "E1 float source slice"
        );
        assert!(
            !dump.contains("Unsupported."),
            "smoke must stay in subset: {dump}"
        );
    }

    #[test]
    fn dump_parse_types_shape() {
        let dump = dump_parse(&read_fixture("types.as")).expect("parse types");
        assert!(dump.contains("Class\tInternal\tPoint\n"), "I4 class");
        assert!(dump.contains("Struct\tInternal\tPair\n"), "I5 struct");
        assert!(dump.contains("Enum\tInternal\tColor\n"), "I6 enum");
        assert!(dump.contains("EnumVariant\tRed\n"), "I6 variant");
        assert!(dump.contains("Field\tPublic\tx\n"), "I7 field");
        assert!(dump.contains("Method\tPublic\tLen\n"), "I8 method");
        assert!(dump.contains("Type.Array\n"), "T2 array");
        assert!(dump.contains("Expr.New\n"), "E7 new");
        assert!(dump.contains("Expr.This\n"), "this receiver");
        assert!(dump.contains("Expr.Null\n"), "null");
        assert!(
            !dump.contains("Unsupported."),
            "types must stay in subset: {dump}"
        );
    }

    #[test]
    fn dump_parse_control_shape() {
        let dump = dump_parse(&read_fixture("control.as")).expect("parse control");
        assert!(dump.contains("Stmt.If\n"), "S4 if");
        assert!(dump.contains("Stmt.While\n"), "S5 while");
        assert!(dump.contains("Stmt.ForC\n"), "S6 for");
        assert!(dump.contains("Stmt.Break\n"), "S7 break");
        assert!(dump.contains("Stmt.Continue\n"), "S7 continue");
        assert!(dump.contains("Stmt.Assign\n"), "S8 assign");
        assert!(dump.contains("Stmt.Switch\n"), "S9 switch");
        assert!(dump.contains("SwitchCase\tIdent\tA\n"), "S9 case");
        assert!(dump.contains("Expr.Unary\tNot\n"), "E3 unary");
        assert!(
            !dump.contains("Unsupported."),
            "control must stay in subset: {dump}"
        );
    }
}
