use ast::ExpressionTree;
use ast::*;
use hir::DefId;

use crate::type_id::{LinqPath, TypeId};

/// RFC 017 M4-link Phase B：函数符号的来源类别。
///
/// 由 typeck 在生成 `TypedFn` 时按符号来源标注，MIR lower 转换为
/// `mir::Linkage` 供 codegen 发射 LLVM linkage。
///
/// 覆盖 RFC 017 D2 节「`linkonce_odr` 弱符号消解策略」决策表：
/// - `User`：用户源码定义的 class/struct/method/free fn → `mir::Linkage::External`
///   （单一权威定义来源；保证 ABI 稳定）
/// - `Monomorphized`：泛型单态化实例 → `mir::Linkage::LinkonceOdr`
///   （单态化是 Arc 既定硬约束；C++ template 模型已验证；ODR 保证语义等价。
///   std 库泛型实例化也归此类——std 库的非泛型函数经 `.ao` exports 走 declare，
///   其泛型实例化在用户 `.o` 中以单态化形式出现，与用户代码单态化同形。）
///
/// 其他类别（`.ao` exports 外部符号 / runtime ABI 函数）不进入 `TypedFn`，
/// 由 codegen 直接消费 `typeck::external_symbols` / runtime ABI 表发射 `declare`/`external`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FnLinkage {
    #[default]
    User,
    Monomorphized,
}

/// P0 双引擎收敛：span 键表达式类型表（typeck → MIR 结构化下传）。
///
/// `check_expr_at(span, expr)` 在表达式检查出口记录 (span → TypeId)。
/// MIR lower 的 `infer_type_from_expr` 改为优先查此表（命中即采用 typeck
/// 结论，未命中回落旧推断），消除两套推断引擎对 builtin 知识的重复维护。
///
/// 冲突降级：同一 span 记录出不同类型（宏展开 / 泛型单态化克隆共享模板
/// span）时，该 span 进入 `ambiguous` 集合并从 `table` 移除——消费端视为
/// 未命中自动回落，保证不静默错型。
///
/// `Span::DUMMY`（合成节点）不记录：无跨克隆体身份意义。
#[derive(Clone, Debug, Default)]
pub struct ExprTypeTable {
    pub table: std::collections::HashMap<ast::Span, TypeId>,
    pub ambiguous: std::collections::HashSet<ast::Span>,
}

impl ExprTypeTable {
    /// 记录一次表达式类型结论；同 span 异型 → Ambiguous 降级。
    pub fn record(&mut self, span: ast::Span, ty: TypeId) {
        if span == ast::Span::DUMMY {
            return;
        }
        if self.ambiguous.contains(&span) {
            return;
        }
        match self.table.get(&span) {
            Some(existing) if *existing != ty => {
                self.table.remove(&span);
                self.ambiguous.insert(span);
            }
            Some(_) => {}
            None => {
                self.table.insert(span, ty);
            }
        }
    }

    /// 查询：命中返回 Some(ty)；未命中或 Ambiguous 返回 None（消费端回落旧推断）。
    pub fn get(&self, span: ast::Span) -> Option<&TypeId> {
        if self.ambiguous.contains(&span) {
            return None;
        }
        self.table.get(&span)
    }
}

#[derive(Clone, Debug)]
pub struct TypedExpr {
    pub ty: TypeId,
    pub expr: Expr,
    pub linq_path: Option<LinqPath>,
    pub expression_tree: Option<ExpressionTree>,
}

#[derive(Clone, Debug)]
pub struct TypedBlock {
    pub stmts: Vec<TypedStmt>,
    /// RFC 045 P3：尾部表达式（else-if 链等）的重写结果。check_block 只对 tail
    /// 做类型检查，若丢弃重写（收窄 Cast / Unbox），MIR 重下降时 else-if 链的
    /// scrut 按 locals 原始类型处理（@object_Member 未定义符号 / 直读 ArcBox）。
    pub tail: Option<Box<Spanned<Expr>>>,
}

#[derive(Clone, Debug)]
pub enum TypedStmt {
    Let {
        name: Ident,
        ty: TypeId,
        init: Option<Spanned<Expr>>,
    },
    Expr(Spanned<Expr>),
    Return(Option<Spanned<Expr>>),
    While {
        cond: Spanned<Expr>,
        body: TypedBlock,
    },
    For {
        var: Ident,
        elem_ty: TypeId,
        iter: Spanned<Expr>,
        body: TypedBlock,
    },
    /// C-style `for (init; cond; inc) { body }`. init/inc as Spanned<Stmt> (evaluated inline).
    ForC {
        init: Option<Spanned<Stmt>>,
        cond: Option<Spanned<Expr>>,
        inc: Option<Spanned<Stmt>>,
        body: TypedBlock,
    },
    Assign {
        target: Spanned<Expr>,
        value: Spanned<Expr>,
    },
    Break,
    Continue,
    Throw {
        expr: Spanned<Expr>,
    },
    TryCatch {
        try_body: TypedBlock,
        catch_ty: TypeId,
        catch_name: Ident,
        /// P1-B2：`when` 条件（已校验为 bool）；MIR 脱糖为条件 rethrow。
        when_cond: Option<Spanned<Expr>>,
        catch_body: TypedBlock,
        finally: Option<TypedBlock>,
    },
    TryFinally {
        body: TypedBlock,
        finally: TypedBlock,
    },
    Using {
        name: Ident,
        ty: TypeId,
        init: Spanned<Expr>,
        body: TypedBlock,
    },
    /// RFC 010：`using var` — 无内嵌 body，Dispose 在 MIR 包剩余语句。
    UsingVar {
        name: Ident,
        ty: TypeId,
        init: Spanned<Expr>,
    },
    /// `await using (Type name = init) { body }`.
    AwaitUsing {
        name: Ident,
        ty: TypeId,
        init: Spanned<Expr>,
        body: TypedBlock,
    },
    /// `await using var name = init;`.
    AwaitUsingVar {
        name: Ident,
        ty: TypeId,
        init: Spanned<Expr>,
    },
}

#[derive(Clone, Debug)]
pub struct TypedFn {
    pub def_id: DefId,
    pub name: Ident,
    pub params: Vec<(Ident, TypeId)>,
    pub ret: TypeId,
    pub body: Option<Block>,
    pub typed_body: Option<TypedBlock>,
    pub is_async: bool,
    /// Owning class for ctor / instance methods.
    pub owner: Option<Ident>,
    pub is_ctor: bool,
    /// Field names visible in instance method / ctor bodies (for MIR field access).
    pub class_fields: Vec<Ident>,
    /// RFC 006 M2：当前函数是否为 `static` 方法。
    ///
    /// `true` 时 `class_fields` 仅含静态字段（`is_static || is_const`），
    /// MIR lower 据此将字段访问降级为 `MirOperand::StaticField`（M3 实现）。
    /// 自由函数（`owner == None`）与构造函数恒为 `false`。
    pub is_static: bool,
    /// RFC 017 M4-link Phase B：函数符号来源类别，供 MIR lower 转换为
    /// `mir::Linkage`。默认 `User`（用户源码）；单态化路径标注 `Monomorphized`。
    pub linkage: FnLinkage,
    /// RFC 009 M3：`[Parallelize]` attribute 标记。true 时 codegen 在函数内
    /// 所有 `while` 循环的 backedge 上附加 `!llvm.loop.vectorize.enable`
    /// metadata，强制 LLVM loop-vectorize pass 向量化（即使启发式判定不值得）。
    ///
    /// **多平台说明**：此标记为平台无关的编译提示。实际向量化效果取决于：
    /// - **x86-64**（Linux/macOS/Windows）：SSE2 强制启用，AVX2/AVX-512 由
    ///   `rt_simd_width_bytes()` 运行时检测，LLVM 据目标 CPU 特征选择指令集。
    /// - **AArch64**（ARM64）：NEON 恒定可用（128-bit），LLVM 发射 NEON 指令。
    /// - **其他平台**（如 ARMv7 无 NEON、RISC-V 无 V 扩展）：LLVM 退化为
    ///   标量指令，`[Parallelize]` 不产生错误，仅无性能收益。
    ///
    /// 默认 false；仅用户源码方法/函数可经 `[Parallelize]` 属性置 true。
    /// 构造函数、lambda、单态化实例（跟随模板）恒为 false。
    pub parallelize: bool,
}
