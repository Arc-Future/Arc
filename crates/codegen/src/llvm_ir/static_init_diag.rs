//! 静态初始化依赖分析的编译期结构化诊断（`arc-sinit-XXX`）。
//!
//! 通道设计对齐 typeck `TypeWarning`（`arc-cycle-001`）：codegen 侧产出结构化
//! 数据，由 arc CLI pipeline 统一渲染为 `warning[<code>]: <message>` 打印到
//! stderr，不阻断编译（exit 0）。分析阶段只产出结构化载荷，渲染文本在
//! [`StaticInitDiagnostic::render`] 内派生，避免与输出格式耦合（单通道、无双轨）。

use ast::Ident;

/// 静态初始化依赖分析的结构化编译期诊断。
#[derive(Debug, Clone)]
pub enum StaticInitDiagnostic {
    /// `arc-sinit-001`：静态初始化序环（真实诊断，默认可见）。
    InitCycle {
        /// 环成员（Kahn 拓扑排序后剩余、按声明序）。
        members: Vec<Ident>,
    },
    /// `arc-sinit-002`：跨包/外部调用符号不可穿透（保守降级提示）。
    UnresolvedCallee {
        /// 不可解析的调用符号（LLVM mangle 形式）。
        symbol: String,
    },
    /// `arc-sinit-003`：静态字段初始化器含未覆盖的表达式形态，已回退零值
    /// （**可能产生错误代码**——历史上枚举成员访问静默折叠为 0 即此族）。
    ///
    /// 完整性纪律：`emit_static_init_expr` 的任何零值兜底都必须同步推送本诊断，
    /// 禁止静默降级——排查一次静默零值的代价远高于一条编译期警告。
    UnsupportedInitExpr {
        /// 字段宿主类。
        class: Ident,
        /// 字段名。
        field: Ident,
        /// 未覆盖形态描述（如 "限定名路径成员访问"、"const 引用"、"二元运算"）。
        kind: &'static str,
    },
}

impl StaticInitDiagnostic {
    /// 诊断码（`arc-sinit-001` / `arc-sinit-002` / `arc-sinit-003`）。
    pub fn code(&self) -> &'static str {
        match self {
            StaticInitDiagnostic::InitCycle { .. } => "arc-sinit-001",
            StaticInitDiagnostic::UnresolvedCallee { .. } => "arc-sinit-002",
            StaticInitDiagnostic::UnsupportedInitExpr { .. } => "arc-sinit-003",
        }
    }

    /// 渲染为 `warning[<code>]: <message>` 单行（对齐 `TypeWarning::render`）。
    pub fn render(&self) -> String {
        format!("warning[{}]: {}", self.code(), self.message())
    }

    fn message(&self) -> String {
        match self {
            StaticInitDiagnostic::InitCycle { members } => format!(
                "静态初始化器依赖环：{}。这些类的 `__sinit` 互相依赖，\
                 无法唯一确定初始化序；已按声明序回退，环内成员在运行期可能读到零值静态字段",
                members
                    .iter()
                    .map(|c| c.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
            StaticInitDiagnostic::UnresolvedCallee { symbol } => format!(
                "静态初始化器调用 `{symbol}` 的函数体不可见\
                 （跨包/外部符号或 stub），无法穿透其静态字段依赖；若其读写本模块\
                 静态字段，`__arc_module_init` 排序可能不完整（保守降级）"
            ),
            StaticInitDiagnostic::UnsupportedInitExpr { class, field, kind } => format!(
                "静态字段 `{class}.{field}` 的初始化器含未覆盖的表达式形态（{kind}），\
                 已回退为零值——运行期读到的可能不是源码书写的初值；请改用字面量/\
                 `new`/静态方法调用/静态字段引用/枚举成员等已覆盖形态，\
                 或扩展 `emit_static_init_expr`"
            ),
        }
    }
}
