use crate::{Expr, Ident, Span, Spanned, Type};

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Spanned<Stmt>>,
    pub tail: Option<Box<Spanned<Expr>>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Let {
        mutable: bool,
        name: Ident,
        ty: Option<Spanned<Type>>,
        init: Option<Spanned<Expr>>,
    },
    Expr(Spanned<Expr>),
    Return(Option<Spanned<Expr>>),
    While {
        cond: Spanned<Expr>,
        body: Block,
    },
    For {
        var: Ident,
        iter: Spanned<Expr>,
        body: Block,
    },
    /// C-style `for (init; cond; inc) { body }` loop.
    /// All three clauses are optional (`for (;;)` is an infinite loop).
    /// Desugared to `While` in MIR lowering — no dedicated codegen needed.
    ForC {
        init: Option<Spanned<Box<Stmt>>>,
        cond: Option<Spanned<Expr>>,
        inc: Option<Spanned<Box<Stmt>>>,
        body: Block,
    },
    Assign {
        target: Spanned<Expr>,
        value: Spanned<Expr>,
    },
    Break,
    /// `continue;` — 跳到最近一层循环的下一轮（while header / for 条件）。
    Continue,
    Throw {
        expr: Spanned<Expr>,
    },
    TryCatch {
        try_body: Block,
        catch_ty: Spanned<Type>,
        catch_name: Ident,
        /// RFC 009 P1-B2: `catch (Ex e) when (cond)` — `None` means no filter.
        when_cond: Option<Spanned<Expr>>,
        catch_body: Block,
        /// RFC 009 P1-B2: optional `finally`.
        finally: Option<Block>,
    },
    /// `try { body } finally { cleanup }`.
    TryFinally {
        body: Block,
        finally: Block,
    },
    /// `using (Type name = init) { body }`.
    Using {
        name: Ident,
        ty: Option<Spanned<Type>>,
        init: Spanned<Expr>,
        body: Block,
    },
    /// RFC 010: `using var name = init;`.
    UsingVar {
        name: Ident,
        ty: Option<Spanned<Type>>,
        init: Spanned<Expr>,
    },
    /// `await using (Type name = init) { body }`.
    AwaitUsing {
        name: Ident,
        ty: Option<Spanned<Type>>,
        init: Spanned<Expr>,
        body: Block,
    },
    /// `await using var name = init;`.
    AwaitUsingVar {
        name: Ident,
        ty: Option<Spanned<Type>>,
        init: Spanned<Expr>,
    },
    /// RFC 005 §7.3：`lock (expr) { body }` — typeck 脱糖为
    /// `Monitor.Enter` + `try/finally Monitor.Exit`（expr 求值一次）。
    Lock {
        expr: Spanned<Expr>,
        body: Block,
    },
    /// RFC 004 M2/M7: deconstruct assign with discard and nested targets.
    DeconstructAssign {
        declare: bool,
        targets: Vec<DeconstructTarget>,
        value: Spanned<Expr>,
    },
    /// RFC 044：`yield return expr;` — 迭代器方法内的序列生产点。
    /// 仅合法于返回 IEnumerable&lt;T&gt;/IEnumerator&lt;T&gt;/IAsyncEnumerable&lt;T&gt;
    /// 的方法体；hir 的 yield 脱糖将其重写为合成状态机，typeck/MIR 不会
    /// 见到本节点（见到即错误）。
    YieldReturn {
        value: Spanned<Expr>,
    },
    /// RFC 044：`yield break;` — 提前终结迭代器序列。
    YieldBreak,
}

/// RFC 004: deconstruct lvalue (bind / discard / nested).
#[derive(Clone, Debug, PartialEq)]
pub enum DeconstructTarget {
    /// `Some(id)` bind; `None` is discard `_`.
    Bind(Option<Ident>),
    /// Nested `(t0, t1, …)`, arity >= 2.
    Nested(Vec<DeconstructTarget>),
}

impl DeconstructTarget {
    pub fn is_nested(&self) -> bool {
        matches!(self, Self::Nested(_))
    }

    pub fn has_discard(&self) -> bool {
        match self {
            Self::Bind(None) => true,
            Self::Bind(Some(_)) => false,
            Self::Nested(inner) => inner.iter().any(Self::has_discard),
        }
    }

    pub fn collect_binds(&self, out: &mut Vec<Ident>) {
        match self {
            Self::Bind(Some(id)) => out.push(id.clone()),
            Self::Bind(None) => {}
            Self::Nested(inner) => {
                for t in inner {
                    t.collect_binds(out);
                }
            }
        }
    }
}

impl Block {
    pub fn empty() -> Self {
        Self {
            stmts: vec![],
            tail: None,
        }
    }

    pub fn span(&self) -> Span {
        self.stmts
            .first()
            .map(|s| s.span)
            .unwrap_or(Span::DUMMY)
            .merge(
                self.tail
                    .as_ref()
                    .map(|t| t.span)
                    .unwrap_or_else(|| self.stmts.last().map(|s| s.span).unwrap_or(Span::DUMMY)),
            )
    }
}
