//! Expression tree IR for the IQueryable / AOT path.
//!
//! Compile-time expansion contract (see `docs/rfc/011-expression-trees-query.md`,
//! RFC 003): `Expression<T>`-typed lambdas are lowered to constexpr `ExpressionTree` at typeck; codegen
//! serializes to `.rodata`. No runtime AST walker belongs on the user-program hot path.
//!
//! # RFC 022 §2.2.10 分层覆盖
//!
//! `ExpressionNode` 完整覆盖 Arc 语法节点，按 Arc 侧 `ExpressionType` 枚举分层：
//!
//! - **L1 查询子集（12 变体）**：ORM 翻译用，纯函数式。codegen `emit_expr_tree.rs`
//!   消费此层——序列化为 `.rodata` 供运行时构造 `Expression` 对象。
//! - **L2 表达式扩展（18 变体）**：覆盖 Arc 所有表达式语法（This/Base/Null/Path/
//!   If/Switch/Coalesce/NullConditional/ForceDeref/Is/TypeOf/Default/Await/Block/
//!   Collection/Box/Unbox/Query）。用于编译期扩展（D10.6 解释器/Source Generator）。
//! - **L3 语句层（10 变体）**：覆盖 Arc 所有语句（Let/Assign/Return/Break/Throw/
//!   While/For/TryCatch/TryFinally/Using），仅在 `Block.statements` 中出现。
//!
//! ## 设计原则 4（更新版）
//!
//! Rust 侧 `ExpressionNode` 镜像 Arc 侧 `ExpressionType` 以保持 IR 完备性；
//! codegen 仅消费 L1 12 变体（`emit_expr_tree.rs` 不变）；L2/L3 节点用于
//! D10.6 解释器等编译期扩展路径（typeck 内部构造 `Value` 处理语义，不进入
//! codegen 发射路径）。`lower_expr`/`lower_stmt` 完整覆盖 AST → IR lowering，
//! 保证表达式树信息完备以支撑编译期扩展能力与 ORM 完整落地。

use crate::{
    BinOp, Block, Expr, FloatLitValue, Ident, InterpPart, LambdaBody, LambdaExpr, Pattern,
    QueryClause, QueryExpr, Spanned, Stmt, SwitchExpr, SwitchExprForm, Type, UnaryOp,
};
use smol_str::SmolStr;

/// 编译期表达式树节点 IR——完整覆盖 Arc 语法节点（L1 + L2 + L3）。
///
/// 分层与字段结构对齐 Arc 侧 `ExpressionType` 枚举与 AST 节点结构，
/// 保证信息完备。codegen 仅消费 L1 12 变体；L2/L3 节点供编译期扩展
/// 路径（D10.6 解释器、Source Generator）使用。
#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionNode {
    // ── L1 查询子集（12 变体，codegen 消费）──
    Constant(ConstantValue),
    Parameter {
        name: Ident,
        ty: SmolStr,
    },
    /// 外部捕获变量（`u => u.Age >= threshold` 中的 `threshold`）。
    ///
    /// 与 `Parameter` 区分：`Parameter` 是 Lambda 的形参，`Capture` 是
    /// Lambda 外部作用域的变量。运行时翻译时，Capture 携带变量在构造
    /// 表达式树时的快照值（C# 语义）。
    ///
    /// `local_id` 是 MIR local id，用于 codegen 生成读取变量当前值的代码
    /// （值快照）。typeck 调用时传 -1（仅做类型检查，不需要值快照）；
    /// MIR lowerer 调用时传入实际 local id。
    Capture {
        name: Ident,
        ty: SmolStr,
        local_id: i32,
    },
    /// 字段/属性访问。`ty` 为成员类型名（如 `int`/`bool`），供 codegen 写入
    /// `MemberExpression.TypeName`，使 `Member==Member` 的 `==`/`!=` 能走 bool 分派。
    /// AST 初降为 `"unknown"`；MIR 经 TypeRegistry 调用 `annotate_types` 填充。
    MemberAccess {
        object: Box<ExpressionNode>,
        member: Ident,
        ty: SmolStr,
    },
    Binary {
        op: BinOp,
        left: Box<ExpressionNode>,
        right: Box<ExpressionNode>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<ExpressionNode>,
    },
    Call {
        method: Ident,
        target: Option<Box<ExpressionNode>>,
        args: Vec<ExpressionNode>,
    },
    Lambda {
        params: Vec<(Ident, SmolStr)>,
        body: Box<ExpressionNode>,
    },
    /// 索引访问 `arr[i]` 或 `dict[key]`（RFC 022 §2.3 IndexExpression）。
    Index {
        object: Box<ExpressionNode>,
        index: Box<ExpressionNode>,
    },
    /// 三元条件 `cond ? a : b`（RFC 022 §2.3 ConditionalExpression）。
    Conditional {
        test: Box<ExpressionNode>,
        if_true: Box<ExpressionNode>,
        if_false: Box<ExpressionNode>,
    },
    /// 对象构造 `new T(args...)`（RFC 022 §2.3 NewExpression）。
    New {
        type_name: Ident,
        args: Vec<ExpressionNode>,
    },
    /// 类型转换 `(T)operand`（RFC 022 §2.3 CastExpression）。
    Cast {
        operand: Box<ExpressionNode>,
        target_type: Ident,
    },

    // ── L2 表达式扩展（18 变体，覆盖 Arc 所有表达式语法）──
    /// `this` 引用——当前实例。
    This,
    /// `base` 引用——基类实例。
    Base,
    /// `null` 字面量。
    Null,
    /// 路径访问 `A.B.C`——多段标识符路径（无接收者对象语义）。
    ///
    /// 与 `MemberAccess` 区分：`MemberAccess` 的 object 是表达式（带运行时值），
    /// `Path` 是静态路径（如命名空间.类型.静态成员），无运行时接收者。
    Path {
        segments: Vec<Ident>,
    },
    /// `if (cond) { then } else { else_ }`——if-else 表达式（含语句块形式）。
    ///
    /// `then`/`else_` 通常是 `Block` 节点；`else_` 为 None 表示无 else 分支。
    If {
        test: Box<ExpressionNode>,
        then: Box<ExpressionNode>,
        else_: Option<Box<ExpressionNode>>,
    },
    /// `switch (scrutinee) { case pattern: body; ... }`——switch 表达式。
    ///
    /// `cases` 中 `pattern = None` 表示 default 分支。`body` 通常是 `Block` 节点。
    /// Pattern 直接复用 AST `Pattern` 类型（IR 不重新表达模式语法）。
    Switch {
        scrutinee: Box<ExpressionNode>,
        cases: Vec<SwitchCaseNode>,
    },
    /// `left ?? right`——空合并表达式。
    Coalesce {
        left: Box<ExpressionNode>,
        right: Box<ExpressionNode>,
    },
    /// `receiver?.field` / `receiver?.method(args)`——空条件访问。
    ///
    /// `access` 是 `MemberAccess` 或 `Call` 节点（receiver 已设为被空条件修饰的接收者）。
    NullConditional {
        access: Box<ExpressionNode>,
    },
    /// `receiver!.field` / `receiver!.method(args)`——强制解引用。
    ForceDeref {
        access: Box<ExpressionNode>,
    },
    /// `expr is T` / `expr is T name` / `expr is var x` / `expr is null`——类型测试。
    ///
    /// `pattern` 复用 AST `IsPattern` 类型（含 Type/Var/Null 三种形式）。
    Is {
        expr: Box<ExpressionNode>,
        pattern: IsPatternNode,
    },
    /// `typeof(T)`——编译期类型标识（RFC 026 M1）。
    TypeOf {
        ty: SmolStr,
    },
    /// `default(T)`——类型默认值。
    Default {
        ty: SmolStr,
    },
    /// `await expr`——异步等待。
    Await {
        expr: Box<ExpressionNode>,
    },
    /// 语句块——含 L3 语句序列 + 可选 tail 表达式。
    ///
    /// 与 AST `Block` 结构对齐：`statements` 是 L3 语句节点列表，
    /// `tail` 是块末尾表达式（作为块值返回）。
    Block {
        statements: Vec<ExpressionNode>,
        tail: Option<Box<ExpressionNode>>,
    },
    /// `[e1, e2, ...]`——集合表达式（RFC 017 C# 12 collection expression）。
    Collection {
        elements: Vec<ExpressionNode>,
    },
    /// 装箱——值类型 → object 引用类型（FFI Marshal，RFC 016 v2 M2）。
    Box {
        expr: Box<ExpressionNode>,
        value_ty: SmolStr,
    },
    /// 拆箱——object 引用类型 → 值类型（FFI Marshal，RFC 016 v2 M2）。
    Unbox {
        expr: Box<ExpressionNode>,
        value_ty: SmolStr,
    },
    /// LINQ comprehension `from x in xs where ... select ...`——查询表达式。
    ///
    /// `clauses` 复用 AST `QueryClause` 类型；`select` 是投影表达式。
    Query {
        clauses: Vec<QueryClauseNode>,
        select: Box<ExpressionNode>,
    },

    // ── L3 语句层（10 变体，仅在 Block.statements 中出现）──
    /// `let name = init;` / `let name: T;`——局部变量声明。
    Let {
        name: Ident,
        ty: Option<SmolStr>,
        init: Option<Box<ExpressionNode>>,
    },
    /// `target = value;`——赋值语句。
    Assign {
        target: Box<ExpressionNode>,
        value: Box<ExpressionNode>,
    },
    /// `return expr;` / `return;`——返回语句。
    Return {
        value: Option<Box<ExpressionNode>>,
    },
    /// `break;`——循环中断。
    Break,
    /// `continue;`——进入最近一层循环的下一轮。
    Continue,
    /// `throw expr;`——抛出异常。
    Throw {
        expr: Box<ExpressionNode>,
    },
    /// `while (cond) { body }`——while 循环。
    While {
        cond: Box<ExpressionNode>,
        body: Box<ExpressionNode>,
    },
    /// `for (var x in iter) { body }`——for/foreach 循环。
    For {
        var: Ident,
        iter: Box<ExpressionNode>,
        body: Box<ExpressionNode>,
    },
    /// `try { ... } catch (T name) { ... }`——try-catch 语句。
    TryCatch {
        try_body: Box<ExpressionNode>,
        catch_ty: SmolStr,
        catch_name: Ident,
        catch_body: Box<ExpressionNode>,
    },
    /// `try { ... } finally { ... }`——try-finally 语句。
    TryFinally {
        body: Box<ExpressionNode>,
        finally: Box<ExpressionNode>,
    },
    /// `using (T name = init) { body }`——using 语句（资源管理）。
    Using {
        name: Ident,
        ty: Option<SmolStr>,
        init: Box<ExpressionNode>,
        body: Box<ExpressionNode>,
    },
}

/// `switch` 表达式的 case 分支 IR（与 AST `SwitchCase` 对齐）。
///
/// `pattern = None` 表示 `default:` 分支；`body` 通常是 `Block` 节点。
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCaseNode {
    pub pattern: Option<Pattern>,
    pub body: Box<ExpressionNode>,
}

/// `is` 类型测试的 pattern IR（与 AST `IsPattern` 对齐）。
///
/// 三种形式：
/// - `Type { ty, binding }`：类型模式 `is T` 或 `is T name`（声明模式）
/// - `Var(name)`：var 模式 `is var x`（永远匹配，绑定到原类型）
/// - `Null`：null 模式 `is null`（测试对象是否为 null）
#[derive(Clone, Debug, PartialEq)]
pub enum IsPatternNode {
    Type { ty: SmolStr, binding: Option<Ident> },
    Var(Ident),
    Null,
}

/// LINQ 查询子句 IR（与 AST `QueryClause` 对齐）。
///
/// 复用 AST `QueryClause` 的子句类型，但内部 `Expr` 已 lower 为 `ExpressionNode`。
#[derive(Clone, Debug, PartialEq)]
pub enum QueryClauseNode {
    From {
        ident: Ident,
        source: Box<ExpressionNode>,
    },
    Let {
        ident: Ident,
        value: Box<ExpressionNode>,
    },
    Where(Box<ExpressionNode>),
    OrderBy {
        key: Box<ExpressionNode>,
        descending: bool,
    },
    Join {
        ident: Ident,
        source: Box<ExpressionNode>,
        on_left: Box<ExpressionNode>,
        on_right: Box<ExpressionNode>,
    },
    GroupBy {
        key: Box<ExpressionNode>,
        element: Option<Box<ExpressionNode>>,
    },
}

impl ExpressionNode {
    /// 推断节点结果类型名（供 codegen `TypeName` 与 annotate 使用）。
    pub fn inferred_type_name(&self) -> SmolStr {
        infer_type_name(self)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstantValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

/// Typed expression tree wrapper.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionTree {
    pub root: ExpressionNode,
    /// 表达式树整体类型名（RFC 018 §6.1：对应 C# `Expression.Type` 的字符串侧）。
    ///
    /// **RFC 018 M3**：Arc 侧 `Expression.Type: Type` 已落地（class 属性，非 struct）；
    /// 本 Rust IR 字段仍为 `SmolStr`，由 codegen `emit_expr_tree` 映射为 RuntimeType。
    /// 详见 RFC 018 §6.3。
    pub ty: SmolStr,
    pub nodes: Vec<ExpressionNode>,
}

impl ExpressionTree {
    pub fn from_ast(
        expr: &Spanned<Expr>,
        param_types: &[(Ident, SmolStr)],
        captures: &[(Ident, i32, SmolStr)],
    ) -> Option<Self> {
        let root = lower_expr(&expr.node, param_types, captures)?;
        let mut nodes = Vec::new();
        flatten(&root, &mut nodes);
        Some(ExpressionTree {
            ty: infer_type_name(&root),
            root,
            nodes,
        })
    }

    /// 将 `Expression<T>` / 属性 Lambda 树化为以 `Lambda` 为根的树。
    ///
    /// C# 对齐：`Expression<TDelegate>` 运行时是 `LambdaExpression`；根节点必须是
    /// `ExpressionNode::Lambda`，以便 `NodeType == Lambda` 与 `Eval*` 经 Body 委托。
    /// SqlTranslator 已按 `ExpressionType.Lambda` 解包 Body，行为兼容。
    pub fn from_lambda(lambda: &LambdaExpr, captures: &[(Ident, i32, SmolStr)]) -> Option<Self> {
        let root = lower_lambda(lambda, &[], captures)?;
        let mut nodes = Vec::new();
        flatten(&root, &mut nodes);
        Some(ExpressionTree {
            ty: infer_type_name(&root),
            root,
            nodes,
        })
    }

    /// 填充 Parameter / MemberAccess 类型名，供 codegen 写入 `TypeName`。
    ///
    /// `lambda_param_tys`：按最外层 Lambda 参数顺序的类型（来自
    /// `Expression<Func<T,...>>` 的 Func 形参），覆盖源码未标注的参数。
    /// `resolve_field(owner_ty, member) → field_ty`：由 MIR 经 TypeRegistry 提供。
    pub fn annotate_types(
        &mut self,
        lambda_param_tys: &[SmolStr],
        mut resolve_field: impl FnMut(&str, &str) -> Option<SmolStr>,
    ) {
        let mut known: Vec<(Ident, SmolStr)> = Vec::new();
        annotate_node(
            &mut self.root,
            lambda_param_tys,
            &mut known,
            &mut resolve_field,
            true,
        );
        self.nodes.clear();
        flatten(&self.root, &mut self.nodes);
        self.ty = infer_type_name(&self.root);
    }
}

/// 递归填充 Parameter / MemberAccess 类型。
///
/// `is_outer_lambda`：仅最外层 Lambda 用 `lambda_param_tys` 覆盖未标注形参。
fn annotate_node(
    node: &mut ExpressionNode,
    lambda_param_tys: &[SmolStr],
    known: &mut Vec<(Ident, SmolStr)>,
    resolve_field: &mut impl FnMut(&str, &str) -> Option<SmolStr>,
    is_outer_lambda: bool,
) {
    match node {
        ExpressionNode::Lambda { params, body } => {
            if is_outer_lambda {
                for (i, (name, ty)) in params.iter_mut().enumerate() {
                    if ty.as_str() == "unknown" || ty.is_empty() {
                        if let Some(pt) = lambda_param_tys.get(i) {
                            *ty = pt.clone();
                        }
                    }
                    known.push((name.clone(), ty.clone()));
                }
            } else {
                for (name, ty) in params.iter() {
                    known.push((name.clone(), ty.clone()));
                }
            }
            annotate_node(body, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Parameter { name, ty } => {
            if ty.as_str() == "unknown" || ty.is_empty() {
                if let Some((_, kt)) = known.iter().rev().find(|(n, _)| n == name) {
                    *ty = kt.clone();
                }
            }
        }
        ExpressionNode::MemberAccess { object, member, ty } => {
            annotate_node(object, lambda_param_tys, known, resolve_field, false);
            let obj_ty = infer_type_name(object);
            if let Some(fty) = resolve_field(obj_ty.as_str(), member.as_str()) {
                *ty = fty;
            }
        }
        ExpressionNode::Binary { left, right, .. } => {
            annotate_node(left, lambda_param_tys, known, resolve_field, false);
            annotate_node(right, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Unary { operand, .. } | ExpressionNode::Cast { operand, .. } => {
            annotate_node(operand, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Await { expr }
        | ExpressionNode::Box { expr, .. }
        | ExpressionNode::Unbox { expr, .. }
        | ExpressionNode::Is { expr, .. }
        | ExpressionNode::Throw { expr } => {
            annotate_node(expr, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Call { target, args, .. } => {
            if let Some(t) = target {
                annotate_node(t, lambda_param_tys, known, resolve_field, false);
            }
            for a in args {
                annotate_node(a, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Index { object, index } => {
            annotate_node(object, lambda_param_tys, known, resolve_field, false);
            annotate_node(index, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Conditional {
            test,
            if_true,
            if_false,
        } => {
            annotate_node(test, lambda_param_tys, known, resolve_field, false);
            annotate_node(if_true, lambda_param_tys, known, resolve_field, false);
            annotate_node(if_false, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::New { args, .. } => {
            for a in args {
                annotate_node(a, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::If { test, then, else_ } => {
            annotate_node(test, lambda_param_tys, known, resolve_field, false);
            annotate_node(then, lambda_param_tys, known, resolve_field, false);
            if let Some(e) = else_ {
                annotate_node(e, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Coalesce { left, right } => {
            annotate_node(left, lambda_param_tys, known, resolve_field, false);
            annotate_node(right, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::NullConditional { access } | ExpressionNode::ForceDeref { access } => {
            annotate_node(access, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Block { statements, tail } => {
            for s in statements {
                annotate_node(s, lambda_param_tys, known, resolve_field, false);
            }
            if let Some(t) = tail {
                annotate_node(t, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Switch { scrutinee, cases } => {
            annotate_node(scrutinee, lambda_param_tys, known, resolve_field, false);
            for c in cases {
                annotate_node(&mut c.body, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Collection { elements } => {
            for e in elements {
                annotate_node(e, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Query { clauses, select } => {
            for c in clauses {
                annotate_clause(c, lambda_param_tys, known, resolve_field);
            }
            annotate_node(select, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Let { init, .. } => {
            if let Some(i) = init {
                annotate_node(i, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::Assign { target, value } => {
            annotate_node(target, lambda_param_tys, known, resolve_field, false);
            annotate_node(value, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Return { value } => {
            if let Some(v) = value {
                annotate_node(v, lambda_param_tys, known, resolve_field, false);
            }
        }
        ExpressionNode::While { cond, body } => {
            annotate_node(cond, lambda_param_tys, known, resolve_field, false);
            annotate_node(body, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::For { iter, body, .. } => {
            annotate_node(iter, lambda_param_tys, known, resolve_field, false);
            annotate_node(body, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            annotate_node(try_body, lambda_param_tys, known, resolve_field, false);
            annotate_node(catch_body, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::TryFinally { body, finally } => {
            annotate_node(body, lambda_param_tys, known, resolve_field, false);
            annotate_node(finally, lambda_param_tys, known, resolve_field, false);
        }
        ExpressionNode::Using { init, body, .. } => {
            annotate_node(init, lambda_param_tys, known, resolve_field, false);
            annotate_node(body, lambda_param_tys, known, resolve_field, false);
        }
        // leaves / no-child
        ExpressionNode::Constant(_)
        | ExpressionNode::Capture { .. }
        | ExpressionNode::This
        | ExpressionNode::Base
        | ExpressionNode::Null
        | ExpressionNode::Path { .. }
        | ExpressionNode::TypeOf { .. }
        | ExpressionNode::Default { .. }
        | ExpressionNode::Break
        | ExpressionNode::Continue => {}
    }
}

fn annotate_clause(
    clause: &mut QueryClauseNode,
    lambda_param_tys: &[SmolStr],
    known: &mut Vec<(Ident, SmolStr)>,
    resolve_field: &mut impl FnMut(&str, &str) -> Option<SmolStr>,
) {
    match clause {
        QueryClauseNode::From { source, .. } => {
            annotate_node(source, lambda_param_tys, known, resolve_field, false);
        }
        QueryClauseNode::Let { value, .. } => {
            annotate_node(value, lambda_param_tys, known, resolve_field, false);
        }
        QueryClauseNode::Where(e) | QueryClauseNode::OrderBy { key: e, .. } => {
            annotate_node(e, lambda_param_tys, known, resolve_field, false);
        }
        QueryClauseNode::Join {
            source,
            on_left,
            on_right,
            ..
        } => {
            annotate_node(source, lambda_param_tys, known, resolve_field, false);
            annotate_node(on_left, lambda_param_tys, known, resolve_field, false);
            annotate_node(on_right, lambda_param_tys, known, resolve_field, false);
        }
        QueryClauseNode::GroupBy { key, element } => {
            annotate_node(key, lambda_param_tys, known, resolve_field, false);
            if let Some(e) = element {
                annotate_node(e, lambda_param_tys, known, resolve_field, false);
            }
        }
    }
}

fn flatten(node: &ExpressionNode, out: &mut Vec<ExpressionNode>) {
    out.push(node.clone());
    match node {
        // ── L1 ──
        ExpressionNode::MemberAccess { object, .. } => flatten(object, out),
        ExpressionNode::Binary { left, right, .. } => {
            flatten(left, out);
            flatten(right, out);
        }
        ExpressionNode::Unary { operand, .. } => flatten(operand, out),
        ExpressionNode::Call { target, args, .. } => {
            if let Some(t) = target {
                flatten(t, out);
            }
            for a in args {
                flatten(a, out);
            }
        }
        ExpressionNode::Lambda { body, .. } => flatten(body, out),
        ExpressionNode::Index { object, index } => {
            flatten(object, out);
            flatten(index, out);
        }
        ExpressionNode::Conditional {
            test,
            if_true,
            if_false,
        } => {
            flatten(test, out);
            flatten(if_true, out);
            flatten(if_false, out);
        }
        ExpressionNode::New { args, .. } => {
            for a in args {
                flatten(a, out);
            }
        }
        ExpressionNode::Cast { operand, .. } => flatten(operand, out),

        // ── L2 ──
        ExpressionNode::If { test, then, else_ } => {
            flatten(test, out);
            flatten(then, out);
            if let Some(e) = else_ {
                flatten(e, out);
            }
        }
        ExpressionNode::Switch { scrutinee, cases } => {
            flatten(scrutinee, out);
            for c in cases {
                flatten(&c.body, out);
            }
        }
        ExpressionNode::Coalesce { left, right } => {
            flatten(left, out);
            flatten(right, out);
        }
        ExpressionNode::NullConditional { access } | ExpressionNode::ForceDeref { access } => {
            flatten(access, out)
        }
        ExpressionNode::Is { expr, .. } => flatten(expr, out),
        ExpressionNode::Await { expr } => flatten(expr, out),
        ExpressionNode::Block { statements, tail } => {
            for s in statements {
                flatten(s, out);
            }
            if let Some(t) = tail {
                flatten(t, out);
            }
        }
        ExpressionNode::Collection { elements } => {
            for e in elements {
                flatten(e, out);
            }
        }
        ExpressionNode::Box { expr, .. } | ExpressionNode::Unbox { expr, .. } => flatten(expr, out),
        ExpressionNode::Query { clauses, select } => {
            for c in clauses {
                flatten_clause(c, out);
            }
            flatten(select, out);
        }
        // 无子节点的 L2 变体
        ExpressionNode::This
        | ExpressionNode::Base
        | ExpressionNode::Null
        | ExpressionNode::Path { .. }
        | ExpressionNode::TypeOf { .. }
        | ExpressionNode::Default { .. } => {}

        // ── L3 ──
        ExpressionNode::Let { init, .. } => {
            if let Some(i) = init {
                flatten(i, out);
            }
        }
        ExpressionNode::Assign { target, value } => {
            flatten(target, out);
            flatten(value, out);
        }
        ExpressionNode::Return { value } => {
            if let Some(v) = value {
                flatten(v, out);
            }
        }
        ExpressionNode::Throw { expr } => flatten(expr, out),
        ExpressionNode::While { cond, body } => {
            flatten(cond, out);
            flatten(body, out);
        }
        ExpressionNode::For { iter, body, .. } => {
            flatten(iter, out);
            flatten(body, out);
        }
        ExpressionNode::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            flatten(try_body, out);
            flatten(catch_body, out);
        }
        ExpressionNode::TryFinally { body, finally } => {
            flatten(body, out);
            flatten(finally, out);
        }
        ExpressionNode::Using { init, body, .. } => {
            flatten(init, out);
            flatten(body, out);
        }
        // 无子节点的 L3 变体
        ExpressionNode::Break | ExpressionNode::Continue => {}

        // L1 无子节点变体（Constant/Parameter/Capture）已隐式覆盖
        ExpressionNode::Constant(_)
        | ExpressionNode::Parameter { .. }
        | ExpressionNode::Capture { .. } => {}
    }
}

fn flatten_clause(clause: &QueryClauseNode, out: &mut Vec<ExpressionNode>) {
    match clause {
        QueryClauseNode::From { source, .. } => flatten(source, out),
        QueryClauseNode::Let { value, .. } => flatten(value, out),
        QueryClauseNode::Where(e) => flatten(e, out),
        QueryClauseNode::OrderBy { key, .. } => flatten(key, out),
        QueryClauseNode::Join {
            source,
            on_left,
            on_right,
            ..
        } => {
            flatten(source, out);
            flatten(on_left, out);
            flatten(on_right, out);
        }
        QueryClauseNode::GroupBy { key, element } => {
            flatten(key, out);
            if let Some(e) = element {
                flatten(e, out);
            }
        }
    }
}

fn infer_type_name(node: &ExpressionNode) -> SmolStr {
    match node {
        // ── L1 ──
        ExpressionNode::Constant(ConstantValue::Int(_)) => "int".into(),
        ExpressionNode::Constant(ConstantValue::Float(_)) => "double".into(),
        ExpressionNode::Constant(ConstantValue::Bool(_)) => "bool".into(),
        ExpressionNode::Constant(ConstantValue::String(_)) => "string".into(),
        ExpressionNode::Parameter { ty, .. } => ty.clone(),
        ExpressionNode::Capture { ty, .. } => ty.clone(),
        ExpressionNode::MemberAccess { ty, .. } => {
            if ty.as_str() == "unknown" || ty.is_empty() {
                "unknown".into()
            } else {
                ty.clone()
            }
        }
        ExpressionNode::Binary { op, .. } => match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                "bool".into()
            }
            BinOp::And | BinOp::Or => "bool".into(),
            _ => "int".into(),
        },
        ExpressionNode::Unary { op, .. } => match op {
            UnaryOp::Not => "bool".into(),
            UnaryOp::Neg => "int".into(),
            UnaryOp::BitNot => "int".into(),
        },
        ExpressionNode::Call { .. } => "unknown".into(),
        ExpressionNode::Lambda { .. } => "Func".into(),
        ExpressionNode::Index { .. } => "unknown".into(),
        ExpressionNode::Conditional { if_true, .. } => infer_type_name(if_true),
        ExpressionNode::New { type_name, .. } => type_name.clone(),
        ExpressionNode::Cast { target_type, .. } => target_type.clone(),

        // ── L2 ──
        ExpressionNode::This | ExpressionNode::Base => "unknown".into(),
        ExpressionNode::Null => "null".into(),
        ExpressionNode::Path { .. } => "unknown".into(),
        ExpressionNode::If { then, .. } => infer_type_name(then),
        ExpressionNode::Switch { cases, .. } => cases
            .first()
            .map(|c| infer_type_name(&c.body))
            .unwrap_or_else(|| "void".into()),
        ExpressionNode::Coalesce { left, .. } => infer_type_name(left),
        ExpressionNode::NullConditional { access } | ExpressionNode::ForceDeref { access } => {
            infer_type_name(access)
        }
        ExpressionNode::Is { .. } => "bool".into(),
        ExpressionNode::TypeOf { .. } => "RuntimeType".into(),
        ExpressionNode::Default { ty, .. } => ty.clone(),
        ExpressionNode::Await { expr } => {
            // await Task<T> → T；无法静态剥离，返回表达式类型的 Task 包装名
            infer_type_name(expr)
        }
        ExpressionNode::Block { tail, .. } => tail
            .as_ref()
            .map(|t| infer_type_name(t))
            .unwrap_or_else(|| "void".into()),
        ExpressionNode::Collection { .. } => "unknown".into(),
        ExpressionNode::Box { .. } => "object".into(),
        ExpressionNode::Unbox { value_ty, .. } => value_ty.clone(),
        ExpressionNode::Query { .. } => "unknown".into(),

        // ── L3 ──
        ExpressionNode::Let { ty, .. } => ty.clone().unwrap_or_else(|| "unknown".into()),
        ExpressionNode::Assign { value, .. } => infer_type_name(value),
        ExpressionNode::Return { .. } => "void".into(),
        ExpressionNode::Break | ExpressionNode::Continue => "void".into(),
        ExpressionNode::Throw { .. } => "never".into(),
        ExpressionNode::While { .. } => "void".into(),
        ExpressionNode::For { .. } => "void".into(),
        ExpressionNode::TryCatch { try_body, .. } => infer_type_name(try_body),
        ExpressionNode::TryFinally { body, .. } => infer_type_name(body),
        ExpressionNode::Using { body, .. } => infer_type_name(body),
    }
}

fn lower_expr(
    expr: &Expr,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> Option<ExpressionNode> {
    match expr {
        // ── L1 字面量 ──
        Expr::IntLit(n) => Some(ExpressionNode::Constant(ConstantValue::Int(*n))),
        Expr::FloatLit(FloatLitValue::Double(f)) => {
            Some(ExpressionNode::Constant(ConstantValue::Float(*f)))
        }
        Expr::FloatLit(FloatLitValue::Float(f)) => {
            Some(ExpressionNode::Constant(ConstantValue::Float(*f as f64)))
        }
        Expr::BoolLit(b) => Some(ExpressionNode::Constant(ConstantValue::Bool(*b))),
        Expr::StringLit(s) => Some(ExpressionNode::Constant(ConstantValue::String(s.clone()))),
        // RFC 012：comptime 包裹的表达式在表达式树中递归降级为其内部表达式
        // （comptime 折叠由 typeck 完成，此处仅透传树结构）。
        Expr::Comptime(inner) => lower_expr(&inner.node, params, captures),
        Expr::CharLit(c) => Some(ExpressionNode::Constant(ConstantValue::Int(
            *c as u32 as i64,
        ))),
        Expr::Ident(name) => {
            // Distinguish Lambda parameters from captured outer variables.
            // Identifiers in `params` are Lambda formal parameters; any other
            // identifier is an outer capture (e.g., `age` in `u => u.Age >= age`).
            if let Some((_, ty)) = params.iter().find(|(n, _)| n == name) {
                Some(ExpressionNode::Parameter {
                    name: name.clone(),
                    ty: ty.clone(),
                })
            } else if let Some((_, lid, ty)) = captures.iter().find(|(n, _, _)| n == name) {
                Some(ExpressionNode::Capture {
                    name: name.clone(),
                    ty: ty.clone(),
                    local_id: *lid,
                })
            } else {
                Some(ExpressionNode::Capture {
                    name: name.clone(),
                    ty: "unknown".into(),
                    local_id: -1,
                })
            }
        }

        // ── L1 表达式 ──
        Expr::Field { receiver, field } => Some(ExpressionNode::MemberAccess {
            object: Box::new(lower_expr(&receiver.node, params, captures)?),
            member: field.clone(),
            ty: "unknown".into(),
        }),
        Expr::Binary { op, left, right } => Some(ExpressionNode::Binary {
            op: *op,
            left: Box::new(lower_expr(&left.node, params, captures)?),
            right: Box::new(lower_expr(&right.node, params, captures)?),
        }),
        Expr::Unary { op, expr: operand } => Some(ExpressionNode::Unary {
            op: *op,
            operand: Box::new(lower_expr(&operand.node, params, captures)?),
        }),
        Expr::Lambda(l) => lower_lambda(l, params, captures),
        Expr::ExpressionLit(e) => lower_lambda(&e.lambda, params, captures),
        Expr::Index { receiver, index } => Some(ExpressionNode::Index {
            object: Box::new(lower_expr(&receiver.node, params, captures)?),
            index: Box::new(lower_expr(&index.node, params, captures)?),
        }),
        Expr::Cast { expr: operand, ty } => Some(ExpressionNode::Cast {
            operand: Box::new(lower_expr(&operand.node, params, captures)?),
            target_type: type_name(&ty.node),
        }),
        Expr::New { ty, args, .. } => {
            // 对象构造：仅记录类型名与构造实参，忽略 obj_init（表达式树
            // IR 不携带对象初始化器，RFC 022 §2.3 NewExpression 仅含 Args）。
            let mut lowered_args = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(&a.node, params, captures)?);
            }
            Some(ExpressionNode::New {
                type_name: type_name(&ty.node),
                args: lowered_args,
            })
        }

        // ── L1 Call/MethodCall lowering ──
        // Call 形式：func(args) → method=func 名（若为 Ident），target=None
        // MethodCall 形式：receiver.method(args) → method=方法名，target=receiver
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            let target = Box::new(lower_expr(&receiver.node, params, captures)?);
            let mut lowered_args = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(&a.node, params, captures)?);
            }
            Some(ExpressionNode::Call {
                method: method.clone(),
                target: Some(target),
                args: lowered_args,
            })
        }
        Expr::Call { func, args, .. } => {
            // 函数调用：func 是 Ident 时 method=name/target=None；
            // 是其他形式（如 Path）时 method=空/target=func 表达式。
            let mut lowered_args = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(&a.node, params, captures)?);
            }
            match &func.node {
                Expr::Ident(name) => Some(ExpressionNode::Call {
                    method: name.clone(),
                    target: None,
                    args: lowered_args,
                }),
                _ => {
                    // 非 Ident 调用：把 func 作为 target，method 留空识别
                    let target = lower_expr(&func.node, params, captures)?;
                    Some(ExpressionNode::Call {
                        method: Ident::from(""),
                        target: Some(Box::new(target)),
                        args: lowered_args,
                    })
                }
            }
        }

        // ── L2 节点 lowering ──
        Expr::This => Some(ExpressionNode::This),
        Expr::Base => Some(ExpressionNode::Base),
        Expr::Null => Some(ExpressionNode::Null),
        Expr::Path(segments) => Some(ExpressionNode::Path {
            segments: segments.clone(),
        }),
        Expr::Coalesce { left, right } => Some(ExpressionNode::Coalesce {
            left: Box::new(lower_expr(&left.node, params, captures)?),
            right: Box::new(lower_expr(&right.node, params, captures)?),
        }),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => Some(ExpressionNode::Conditional {
            test: Box::new(lower_expr(&cond.node, params, captures)?),
            if_true: Box::new(lower_expr(&then_branch.node, params, captures)?),
            if_false: Box::new(lower_expr(&else_branch.node, params, captures)?),
        }),
        Expr::NullCond { access } => Some(ExpressionNode::NullConditional {
            access: Box::new(lower_expr(&access.node, params, captures)?),
        }),
        Expr::ForceDeref { access } => Some(ExpressionNode::ForceDeref {
            access: Box::new(lower_expr(&access.node, params, captures)?),
        }),
        Expr::Default { ty } => Some(ExpressionNode::Default {
            ty: type_name(&ty.node),
        }),
        Expr::TypeOf(ty) => Some(ExpressionNode::TypeOf {
            ty: type_name(&ty.node),
        }),
        Expr::Is {
            expr: operand,
            pattern,
        } => Some(ExpressionNode::Is {
            expr: Box::new(lower_expr(&operand.node, params, captures)?),
            pattern: lower_is_pattern(pattern),
        }),
        // RFC 006 M2：`with` 在 typeck 脱糖为 New，表达式树路径不物化。
        // 赋值表达式不可进入表达式树（对齐 C# CS0832：expression tree
        // may not contain an assignment operator）——None 交由调用方报错。
        Expr::Assign { .. } => None,
        Expr::With { .. } => None,
        // `new T[n]` 数组分配非表达式树可表示（无对应 ExpressionNode）。
        Expr::NewArray { .. } => None,
        Expr::Await(operand) => Some(ExpressionNode::Await {
            expr: Box::new(lower_expr(&operand.node, params, captures)?),
        }),
        Expr::Block(b) => Some(lower_block(b, params, captures)),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Some(ExpressionNode::If {
            test: Box::new(lower_expr(&cond.node, params, captures)?),
            then: Box::new(lower_block(then_branch, params, captures)),
            else_: else_branch
                .as_ref()
                .map(|b| Box::new(lower_block(b, params, captures))),
        }),
        Expr::Switch(s) => Some(lower_switch(s, params, captures)),
        Expr::SwitchForm(s) => Some(lower_switch_form(s, params, captures)),
        Expr::CollectionExpr { elements } => {
            let mut lowered = Vec::with_capacity(elements.len());
            for el in elements {
                // spread 在表达式树中近似为普通元素（展开语义由编译期 codegen 承担）。
                lowered.push(lower_expr(&el.expr().node, params, captures)?);
            }
            Some(ExpressionNode::Collection { elements: lowered })
        }
        Expr::Box { expr, value_ty } => Some(ExpressionNode::Box {
            expr: Box::new(lower_expr(&expr.node, params, captures)?),
            value_ty: type_name(&value_ty.node),
        }),
        Expr::Unbox { expr, value_ty } => Some(ExpressionNode::Unbox {
            expr: Box::new(lower_expr(&expr.node, params, captures)?),
            value_ty: type_name(&value_ty.node),
        }),
        Expr::Query(q) => Some(lower_query(q, params, captures)),

        // ── 不支持 lowering 的表达式（如 RefArg 仅在调用位置有效）──
        Expr::RefArg { .. } | Expr::NamedArg { .. } | Expr::StackSpanLit { .. } => None,
        // RFC 007：脱糖为 Constant / ToString / Binary(+)，与 typeck 同形。
        // M2a：带 format/align 的洞在表达式树路径硬拒绝（须先 typeck 脱糖）。
        Expr::InterpolatedString { parts } => {
            let mut acc: Option<ExpressionNode> = None;
            for part in parts {
                let piece = match part {
                    InterpPart::Lit(s) => {
                        ExpressionNode::Constant(ConstantValue::String(s.clone()))
                    }
                    InterpPart::Expr(hole) => {
                        if hole.alignment.is_some() || hole.format.is_some() {
                            return None;
                        }
                        let inner = lower_expr(&hole.expr.node, params, captures)?;
                        match &inner {
                            ExpressionNode::Constant(ConstantValue::String(_)) => inner,
                            _ => ExpressionNode::Call {
                                method: "ToString".into(),
                                target: Some(Box::new(inner)),
                                args: vec![],
                            },
                        }
                    }
                };
                acc = Some(match acc {
                    None => piece,
                    Some(left) => ExpressionNode::Binary {
                        op: BinOp::Add,
                        left: Box::new(left),
                        right: Box::new(piece),
                    },
                });
            }
            Some(
                acc.unwrap_or(ExpressionNode::Constant(ConstantValue::String(
                    String::new(),
                ))),
            )
        }
    }
}

fn lower_lambda(
    lambda: &LambdaExpr,
    outer_params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> Option<ExpressionNode> {
    let mut params: Vec<(Ident, SmolStr)> = outer_params.to_vec();
    for p in &lambda.params {
        let ty =
            p.ty.as_ref()
                .map(|t| type_name(&t.node))
                .unwrap_or_else(|| "unknown".into());
        params.push((p.name.clone(), ty));
    }
    let body = match &lambda.body {
        LambdaBody::Expr(e) => lower_expr(&e.node, &params, captures)?,
        LambdaBody::Block(b) => lower_block(b, &params, captures),
    };
    Some(ExpressionNode::Lambda {
        params: params.into_iter().skip(outer_params.len()).collect(),
        body: Box::new(body),
    })
}

fn lower_block(
    b: &Block,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> ExpressionNode {
    let mut statements = Vec::with_capacity(b.stmts.len());
    for s in &b.stmts {
        if let Some(node) = lower_stmt(&s.node, params, captures) {
            statements.push(node);
        }
    }
    let tail = b
        .tail
        .as_ref()
        .and_then(|t| lower_expr(&t.node, params, captures).map(Box::new));
    ExpressionNode::Block { statements, tail }
}

fn lower_switch(
    s: &SwitchExpr,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> ExpressionNode {
    let scrutinee = Box::new(
        lower_expr(&s.scrutinee.node, params, captures)
            .unwrap_or(ExpressionNode::Constant(ConstantValue::Bool(false))),
    );
    let cases: Vec<SwitchCaseNode> = s
        .cases
        .iter()
        .map(|c| SwitchCaseNode {
            pattern: c.pattern.clone(),
            body: Box::new(lower_block(&c.body, params, captures)),
        })
        .collect();
    ExpressionNode::Switch { scrutinee, cases }
}

fn lower_switch_form(
    s: &SwitchExprForm,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> ExpressionNode {
    let scrutinee = Box::new(
        lower_expr(&s.scrutinee.node, params, captures)
            .unwrap_or(ExpressionNode::Constant(ConstantValue::Bool(false))),
    );
    let cases: Vec<SwitchCaseNode> = s
        .arms
        .iter()
        .map(|arm| {
            let body_expr = lower_expr(&arm.body.node, params, captures)
                .unwrap_or(ExpressionNode::Constant(ConstantValue::Bool(false)));
            SwitchCaseNode {
                pattern: Some(arm.pattern.clone()),
                body: Box::new(ExpressionNode::Block {
                    statements: vec![],
                    tail: Some(Box::new(body_expr)),
                }),
            }
        })
        .collect();
    ExpressionNode::Switch { scrutinee, cases }
}

fn lower_query(
    q: &QueryExpr,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> ExpressionNode {
    let clauses: Vec<QueryClauseNode> = q
        .clauses
        .iter()
        .filter_map(|c| match c {
            QueryClause::From { ident, source } => Some(QueryClauseNode::From {
                ident: ident.clone(),
                source: Box::new(lower_expr(&source.node, params, captures)?),
            }),
            QueryClause::Let { ident, value } => Some(QueryClauseNode::Let {
                ident: ident.clone(),
                value: Box::new(lower_expr(&value.node, params, captures)?),
            }),
            QueryClause::Where(e) => Some(QueryClauseNode::Where(Box::new(lower_expr(
                &e.node, params, captures,
            )?))),
            QueryClause::OrderBy { key, descending } => Some(QueryClauseNode::OrderBy {
                key: Box::new(lower_expr(&key.node, params, captures)?),
                descending: *descending,
            }),
            QueryClause::Join {
                ident,
                source,
                on_left,
                on_right,
            } => Some(QueryClauseNode::Join {
                ident: ident.clone(),
                source: Box::new(lower_expr(&source.node, params, captures)?),
                on_left: Box::new(lower_expr(&on_left.node, params, captures)?),
                on_right: Box::new(lower_expr(&on_right.node, params, captures)?),
            }),
            QueryClause::GroupBy { key, element, .. } => Some(QueryClauseNode::GroupBy {
                key: Box::new(lower_expr(&key.node, params, captures)?),
                element: element
                    .as_ref()
                    .and_then(|e| lower_expr(&e.node, params, captures).map(Box::new)),
            }),
        })
        .collect();
    let select = Box::new(
        lower_expr(&q.select.node, params, captures)
            .unwrap_or(ExpressionNode::Constant(ConstantValue::Bool(false))),
    );
    ExpressionNode::Query { clauses, select }
}

fn lower_is_pattern(p: &crate::IsPattern) -> IsPatternNode {
    match p {
        crate::IsPattern::Type { ty, binding } => IsPatternNode::Type {
            ty: type_name(&ty.node),
            binding: binding.clone(),
        },
        crate::IsPattern::Var(name) => IsPatternNode::Var(name.clone()),
        crate::IsPattern::Null => IsPatternNode::Null,
        crate::IsPattern::Positional(_) => IsPatternNode::Null,
        // RFC 004 常量模式在表达式树 IR 中暂不支持，折叠为 Null 占位（与 Positional 一致）。
        crate::IsPattern::Constant(_) => IsPatternNode::Null,
        // C# 9 逻辑组合（and/or/not）在表达式树 IR 中暂不支持，折叠为 Null 占位
        //（与 Positional 一致）。表达式树上下文中极少出现组合模式。
        crate::IsPattern::And { .. }
        | crate::IsPattern::Or { .. }
        | crate::IsPattern::Not { .. } => IsPatternNode::Null,
    }
}

fn lower_stmt(
    stmt: &Stmt,
    params: &[(Ident, SmolStr)],
    captures: &[(Ident, i32, SmolStr)],
) -> Option<ExpressionNode> {
    match stmt {
        Stmt::Let { name, ty, init, .. } => Some(ExpressionNode::Let {
            name: name.clone(),
            ty: ty.as_ref().map(|t| type_name(&t.node)),
            init: init
                .as_ref()
                .and_then(|i| lower_expr(&i.node, params, captures).map(Box::new)),
        }),
        Stmt::Expr(e) => lower_expr(&e.node, params, captures),
        Stmt::Return(v) => Some(ExpressionNode::Return {
            value: v
                .as_ref()
                .and_then(|e| lower_expr(&e.node, params, captures).map(Box::new)),
        }),
        Stmt::While { cond, body } => Some(ExpressionNode::While {
            cond: Box::new(lower_expr(&cond.node, params, captures)?),
            body: Box::new(lower_block(body, params, captures)),
        }),
        Stmt::For { var, iter, body } => Some(ExpressionNode::For {
            var: var.clone(),
            iter: Box::new(lower_expr(&iter.node, params, captures)?),
            body: Box::new(lower_block(body, params, captures)),
        }),
        Stmt::Assign { target, value } => Some(ExpressionNode::Assign {
            target: Box::new(lower_expr(&target.node, params, captures)?),
            value: Box::new(lower_expr(&value.node, params, captures)?),
        }),
        Stmt::Break => Some(ExpressionNode::Break),
        Stmt::Continue => Some(ExpressionNode::Continue),
        Stmt::Throw { expr } => Some(ExpressionNode::Throw {
            expr: Box::new(lower_expr(&expr.node, params, captures)?),
        }),
        Stmt::TryCatch {
            try_body,
            catch_ty,
            catch_name,
            catch_body,
            ..
        } => Some(ExpressionNode::TryCatch {
            try_body: Box::new(lower_block(try_body, params, captures)),
            catch_ty: type_name(&catch_ty.node),
            catch_name: catch_name.clone(),
            catch_body: Box::new(lower_block(catch_body, params, captures)),
        }),
        Stmt::TryFinally { body, finally } => Some(ExpressionNode::TryFinally {
            body: Box::new(lower_block(body, params, captures)),
            finally: Box::new(lower_block(finally, params, captures)),
        }),
        Stmt::Using {
            name,
            ty,
            init,
            body,
        } => Some(ExpressionNode::Using {
            name: name.clone(),
            ty: ty.as_ref().map(|t| type_name(&t.node)),
            init: Box::new(lower_expr(&init.node, params, captures)?),
            body: Box::new(lower_block(body, params, captures)),
        }),
        // RFC 010：表达式树中将 using var 近似为无 body 的 Using（init only）。
        Stmt::UsingVar { name, ty, init } => Some(ExpressionNode::Using {
            name: name.clone(),
            ty: ty.as_ref().map(|t| type_name(&t.node)),
            init: Box::new(lower_expr(&init.node, params, captures)?),
            body: Box::new(ExpressionNode::Block {
                statements: vec![],
                tail: None,
            }),
        }),
        // RFC 009：lock 语句不进入表达式树（与 using 不同，无 ExpressionNode::Lock）。
        Stmt::Lock { .. } => None,
        Stmt::ForC { .. } => None,
        // RFC 004 M2：表达式树不支持解构赋值
        Stmt::DeconstructAssign { .. } => None,
        Stmt::AwaitUsing { .. } => None,
        Stmt::AwaitUsingVar { .. } => None,
        // yield 是迭代器协议语句，不进入表达式树（与 lock/for-c 一致）。
        Stmt::YieldReturn { .. } => None,
        Stmt::YieldBreak => None,
    }
}

fn type_name(ty: &Type) -> SmolStr {
    match ty {
        Type::Named { path, .. } => path.last().cloned().unwrap_or_else(|| "unknown".into()),
        Type::Infer => "unknown".into(),
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn build_expression_tree() {
        let expr = Spanned::new(
            Expr::Binary {
                op: BinOp::Ge,
                left: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
                right: Box::new(Spanned::new(Expr::IntLit(18), Span::DUMMY)),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        assert!(tree.ty == "bool");
        assert!(tree.nodes.len() >= 3);
    }

    /// C# 对齐：`Expression<T>` ≡ `LambdaExpression`，`from_lambda` 根须为 Lambda。
    #[test]
    fn from_lambda_root_is_lambda() {
        let lambda = LambdaExpr {
            params: vec![],
            body: LambdaBody::Expr(Box::new(Spanned::new(Expr::IntLit(42), Span::DUMMY))),
            is_expression_tree: true,
            is_async: false,
            captures: vec![],
        };
        let tree = ExpressionTree::from_lambda(&lambda, &[]).unwrap();
        match &tree.root {
            ExpressionNode::Lambda { body, params } => {
                assert!(params.is_empty());
                assert!(matches!(
                    body.as_ref(),
                    ExpressionNode::Constant(ConstantValue::Int(42))
                ));
            }
            other => panic!("expected Lambda root, got: {other:?}"),
        }
    }

    /// `u => u.Age >= age` — `age` is an outer capture, not a Lambda parameter.
    #[test]
    fn capture_distinguishes_from_parameter() {
        let expr = Spanned::new(
            Expr::Binary {
                op: BinOp::Ge,
                left: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
                right: Box::new(Spanned::new(Expr::Ident("age".into()), Span::DUMMY)),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        // Root must be Binary; left = MemberAccess(Parameter("u"), "Age"),
        // right = Capture("age") — NOT Parameter("age").
        match &tree.root {
            ExpressionNode::Binary { left, right, .. } => {
                // left: u.Age → MemberAccess { Parameter("u"), "Age" }
                assert!(matches!(
                    left.as_ref(),
                    ExpressionNode::MemberAccess { member, .. } if member == "Age"
                ));
                // right: age → Capture (not Parameter!)
                assert!(
                    matches!(right.as_ref(), ExpressionNode::Capture { name, .. } if name == "age"),
                    "expected Capture for `age`, got: {:?}",
                    right
                );
            }
            other => panic!("expected Binary, got: {other:?}"),
        }
    }

    /// `u => u.Age >= ux.Age` — `ux` is an outer capture, `ux.Age` is a
    /// member access on a captured variable (not on the Lambda parameter).
    #[test]
    fn capture_with_member_access() {
        let expr = Spanned::new(
            Expr::Binary {
                op: BinOp::Ge,
                left: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
                right: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("ux".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        match &tree.root {
            ExpressionNode::Binary { left, right, .. } => {
                // left: u.Age → MemberAccess { Parameter("u"), "Age" }
                match left.as_ref() {
                    ExpressionNode::MemberAccess { object, member, .. } => {
                        assert_eq!(member, "Age");
                        assert!(
                            matches!(object.as_ref(), ExpressionNode::Parameter { name, .. } if name == "u"),
                            "left object should be Parameter(\"u\"), got: {:?}",
                            object
                        );
                    }
                    other => panic!("left should be MemberAccess, got: {other:?}"),
                }
                // right: ux.Age → MemberAccess { Capture("ux"), "Age" }
                match right.as_ref() {
                    ExpressionNode::MemberAccess { object, member, .. } => {
                        assert_eq!(member, "Age");
                        assert!(
                            matches!(object.as_ref(), ExpressionNode::Capture { name, .. } if name == "ux"),
                            "right object should be Capture(\"ux\"), got: {:?}",
                            object
                        );
                    }
                    other => panic!("right should be MemberAccess, got: {other:?}"),
                }
            }
            other => panic!("expected Binary, got: {other:?}"),
        }
    }

    /// `u => u.Age >= u.Age` — both sides reference the Lambda parameter `u`.
    /// No Capture nodes should appear.
    #[test]
    fn both_sides_parameter_no_capture() {
        let expr = Spanned::new(
            Expr::Binary {
                op: BinOp::Ge,
                left: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
                right: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Age".into(),
                    },
                    Span::DUMMY,
                )),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        // No node should be a Capture.
        assert!(
            !tree
                .nodes
                .iter()
                .any(|n| matches!(n, ExpressionNode::Capture { .. })),
            "no Capture nodes expected when both sides are parameters"
        );
    }

    /// RFC 022 新增变体：flatten/infer_type_name 对 Conditional/Index/Cast/New
    /// 的处理。当前 AST 无三元节点，Conditional 直接构造 IR 验证；Index/Cast/New
    /// 通过 AST lowering 验证 flatten 收集与类型推导。
    #[test]
    fn conditional_flatten_and_infer_type() {
        let root = ExpressionNode::Conditional {
            test: Box::new(ExpressionNode::Constant(ConstantValue::Bool(true))),
            if_true: Box::new(ExpressionNode::Constant(ConstantValue::Int(1))),
            if_false: Box::new(ExpressionNode::Constant(ConstantValue::Int(0))),
        };
        let mut nodes = Vec::new();
        flatten(&root, &mut nodes);
        // root + test + if_true + if_false = 4
        assert_eq!(nodes.len(), 4, "Conditional should flatten to 4 nodes");
        // 类型从 if_true 推导
        assert_eq!(infer_type_name(&root), "int");
    }

    /// `u.Scores[0]` → Index，flatten 收集 Index + MemberAccess + Parameter + Constant。
    #[test]
    fn index_lowering_flatten() {
        let expr = Spanned::new(
            Expr::Index {
                receiver: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Scores".into(),
                    },
                    Span::DUMMY,
                )),
                index: Box::new(Spanned::new(Expr::IntLit(0), Span::DUMMY)),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        // nodes: Index, MemberAccess, Parameter, Constant = 4
        assert_eq!(tree.nodes.len(), 4);
        assert!(matches!(tree.root, ExpressionNode::Index { .. }));
        assert_eq!(tree.ty, "unknown");
    }

    /// `(int)u.Score` → Cast，infer_type_name 返回 target_type。
    #[test]
    fn cast_lowering_infer_type() {
        let expr = Spanned::new(
            Expr::Cast {
                expr: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                        field: "Score".into(),
                    },
                    Span::DUMMY,
                )),
                ty: crate::Type::named("int"),
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        assert!(matches!(tree.root, ExpressionNode::Cast { .. }));
        assert_eq!(tree.ty, "int");
        // nodes: Cast, MemberAccess, Parameter = 3
        assert_eq!(tree.nodes.len(), 3);
    }

    /// `new Point(1, 2)` → New，infer_type_name 返回 type_name。
    #[test]
    fn new_lowering_infer_type() {
        let expr = Spanned::new(
            Expr::New {
                ty: crate::Type::named("Point"),
                args: vec![
                    Spanned::new(Expr::IntLit(1), Span::DUMMY),
                    Spanned::new(Expr::IntLit(2), Span::DUMMY),
                ],
                obj_init: None,
            },
            Span::DUMMY,
        );
        let tree = ExpressionTree::from_ast(&expr, &[], &[]).unwrap();
        assert!(matches!(tree.root, ExpressionNode::New { .. }));
        assert_eq!(tree.ty, "Point");
        // nodes: New, Constant, Constant = 3
        assert_eq!(tree.nodes.len(), 3);
    }

    /// RFC 022 §2.2.10 L2 节点 lowering：`this`、`null`、`a ?? b`、`typeof(T)`、
    /// `default(T)`、`await expr` 等 L2 节点从 AST 正确 lower 为 IR。
    #[test]
    fn l2_nodes_lowering() {
        // this → This
        let tree =
            ExpressionTree::from_ast(&Spanned::new(Expr::This, Span::DUMMY), &[], &[]).unwrap();
        assert!(matches!(tree.root, ExpressionNode::This));
        assert_eq!(tree.ty, "unknown");

        // null → Null
        let tree =
            ExpressionTree::from_ast(&Spanned::new(Expr::Null, Span::DUMMY), &[], &[]).unwrap();
        assert!(matches!(tree.root, ExpressionNode::Null));
        assert_eq!(tree.ty, "null");

        // typeof(int) → TypeOf
        let tree = ExpressionTree::from_ast(
            &Spanned::new(Expr::TypeOf(crate::Type::named("int")), Span::DUMMY),
            &[],
            &[],
        )
        .unwrap();
        assert!(matches!(tree.root, ExpressionNode::TypeOf { .. }));
        assert_eq!(tree.ty, "RuntimeType");

        // default(int) → Default
        let tree = ExpressionTree::from_ast(
            &Spanned::new(
                Expr::Default {
                    ty: crate::Type::named("int"),
                },
                Span::DUMMY,
            ),
            &[],
            &[],
        )
        .unwrap();
        assert!(matches!(tree.root, ExpressionNode::Default { .. }));
        assert_eq!(tree.ty, "int");

        // a ?? b → Coalesce
        let tree = ExpressionTree::from_ast(
            &Spanned::new(
                Expr::Coalesce {
                    left: Box::new(Spanned::new(Expr::Ident("a".into()), Span::DUMMY)),
                    right: Box::new(Spanned::new(Expr::IntLit(0), Span::DUMMY)),
                },
                Span::DUMMY,
            ),
            &[],
            &[],
        )
        .unwrap();
        assert!(matches!(tree.root, ExpressionNode::Coalesce { .. }));
        // nodes: Coalesce, Capture, Constant = 3
        assert_eq!(tree.nodes.len(), 3);
    }

    /// L2 MethodCall lowering：`u.GetName()` → Call { method, target, args }。
    #[test]
    fn method_call_lowering() {
        let expr = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                method: "GetName".into(),
                args: vec![],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );
        let params = vec![("u".into(), "User".into())];
        let tree = ExpressionTree::from_ast(&expr, &params, &[]).unwrap();
        match &tree.root {
            ExpressionNode::Call {
                method,
                target,
                args,
            } => {
                assert_eq!(method, "GetName");
                assert!(target.is_some());
                assert!(args.is_empty());
            }
            other => panic!("expected Call, got: {other:?}"),
        }
        // nodes: Call, Parameter = 2
        assert_eq!(tree.nodes.len(), 2);
    }

    /// L2 Block lowering：含 L3 语句序列 + tail。
    #[test]
    fn block_lowering_with_stmts() {
        let block = Block {
            stmts: vec![Spanned::new(
                Stmt::Let {
                    mutable: false,
                    name: "x".into(),
                    ty: None,
                    init: Some(Spanned::new(Expr::IntLit(42), Span::DUMMY)),
                },
                Span::DUMMY,
            )],
            tail: Some(Box::new(Spanned::new(Expr::Ident("x".into()), Span::DUMMY))),
        };
        let tree =
            ExpressionTree::from_ast(&Spanned::new(Expr::Block(block), Span::DUMMY), &[], &[])
                .unwrap();
        match &tree.root {
            ExpressionNode::Block { statements, tail } => {
                assert_eq!(statements.len(), 1, "should have 1 statement");
                assert!(
                    matches!(&statements[0], ExpressionNode::Let { name, .. } if name == "x"),
                    "statement should be Let"
                );
                assert!(tail.is_some(), "should have tail");
            }
            other => panic!("expected Block, got: {other:?}"),
        }
        // nodes: Block, Let, Constant(int), Capture(x) = 4
        assert_eq!(tree.nodes.len(), 4);
        assert_eq!(tree.ty, "unknown");
    }

    /// annotate_types：注入 Lambda 形参类型并经 resolve_field 填充 MemberAccess.ty。
    #[test]
    fn annotate_types_fills_member_access_ty() {
        let lambda = LambdaExpr {
            params: vec![crate::LambdaParam {
                name: "u".into(),
                ty: None,
                default: None,
            }],
            body: LambdaBody::Expr(Box::new(Spanned::new(
                Expr::Field {
                    receiver: Box::new(Spanned::new(Expr::Ident("u".into()), Span::DUMMY)),
                    field: "Active".into(),
                },
                Span::DUMMY,
            ))),
            is_expression_tree: true,
            is_async: false,
            captures: vec![],
        };
        let mut tree = ExpressionTree::from_lambda(&lambda, &[]).unwrap();
        // 初降：参数与成员均为 unknown
        match &tree.root {
            ExpressionNode::Lambda { params, body } => {
                assert_eq!(params[0].1.as_str(), "unknown");
                assert!(matches!(
                    body.as_ref(),
                    ExpressionNode::MemberAccess { ty, .. } if ty.as_str() == "unknown"
                ));
            }
            other => panic!("expected Lambda, got: {other:?}"),
        }
        tree.annotate_types(&["User".into()], |owner, member| {
            assert_eq!(owner, "User");
            assert_eq!(member, "Active");
            Some("bool".into())
        });
        match &tree.root {
            ExpressionNode::Lambda { params, body } => {
                assert_eq!(params[0].1.as_str(), "User");
                match body.as_ref() {
                    ExpressionNode::MemberAccess { ty, member, .. } => {
                        assert_eq!(member.as_str(), "Active");
                        assert_eq!(ty.as_str(), "bool");
                    }
                    other => panic!("expected MemberAccess, got: {other:?}"),
                }
            }
            other => panic!("expected Lambda, got: {other:?}"),
        }
    }

    /// L3 语句 lowering：Return/While/Assign/If 等通过 Block 承载。
    #[test]
    fn l3_stmts_lowering() {
        // if (cond) { return 1; } → If { test, then: Block{Return}, else: None }
        let then_block = Block {
            stmts: vec![Spanned::new(
                Stmt::Return(Some(Spanned::new(Expr::IntLit(1), Span::DUMMY))),
                Span::DUMMY,
            )],
            tail: None,
        };
        let expr = Spanned::new(
            Expr::If {
                cond: Box::new(Spanned::new(Expr::BoolLit(true), Span::DUMMY)),
                then_branch: then_block,
                else_branch: None,
            },
            Span::DUMMY,
        );
        let tree = ExpressionTree::from_ast(&expr, &[], &[]).unwrap();
        match &tree.root {
            ExpressionNode::If { test, then, else_ } => {
                assert!(matches!(
                    test.as_ref(),
                    ExpressionNode::Constant(ConstantValue::Bool(true))
                ));
                assert!(matches!(then.as_ref(), ExpressionNode::Block { .. }));
                assert!(else_.is_none(), "no else branch");
            }
            other => panic!("expected If, got: {other:?}"),
        }
        // nodes: If, Constant(bool), Block, Return, Constant(int) = 5
        assert_eq!(tree.nodes.len(), 5);
    }

    /// L2 Is lowering：`expr is T` → Is { expr, pattern: Type }。
    #[test]
    fn is_lowering() {
        let expr = Spanned::new(
            Expr::Is {
                expr: Box::new(Spanned::new(Expr::Ident("x".into()), Span::DUMMY)),
                pattern: crate::IsPattern::Type {
                    ty: crate::Type::named("string"),
                    binding: None,
                },
            },
            Span::DUMMY,
        );
        let tree = ExpressionTree::from_ast(&expr, &[], &[]).unwrap();
        match &tree.root {
            ExpressionNode::Is { pattern, .. } => {
                assert!(matches!(pattern, IsPatternNode::Type { ty, .. } if ty == "string"));
            }
            other => panic!("expected Is, got: {other:?}"),
        }
        assert_eq!(tree.ty, "bool");
        // nodes: Is, Capture(x) = 2
        assert_eq!(tree.nodes.len(), 2);
    }

    /// L2 Collection lowering：`[1, 2, 3]` → Collection { elements }。
    #[test]
    fn collection_lowering() {
        let expr = Spanned::new(
            Expr::CollectionExpr {
                elements: vec![
                    CollectionElement::Element(Spanned::new(Expr::IntLit(1), Span::DUMMY)),
                    CollectionElement::Element(Spanned::new(Expr::IntLit(2), Span::DUMMY)),
                    CollectionElement::Element(Spanned::new(Expr::IntLit(3), Span::DUMMY)),
                ],
            },
            Span::DUMMY,
        );
        let tree = ExpressionTree::from_ast(&expr, &[], &[]).unwrap();
        match &tree.root {
            ExpressionNode::Collection { elements } => {
                assert_eq!(elements.len(), 3);
            }
            other => panic!("expected Collection, got: {other:?}"),
        }
        // nodes: Collection, Constant, Constant, Constant = 4
        assert_eq!(tree.nodes.len(), 4);
    }
}
