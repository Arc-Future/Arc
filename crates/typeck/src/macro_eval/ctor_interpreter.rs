//! RFC 012 M4 D10.6 构造函数体编译期解释器（RFC 032 QIF 路径扩展）。
//!
//! D10.6 解释器是 typeck 通用机制，服务于 `GenerateToAttribute<T>` 派生类
//! 构造函数体的编译期解释。它解决 D10.2 受限求值器无法处理控制流嵌套注册
//! 调用的问题——D10.2 仅支持 `Func<string>` 委托体（顶层语句 + `if`），
//! 无法解释派生类构造函数体本身的 `if`/`foreach`/`is` 等控制流。
//!
//! # 调用时机
//!
//! D10.6 解释器在 Pass 2 `collect_feature_registrations` 阶段调用，替换
//! 原 `walk_block_for_registrations` 的「顶层语句扫描」模式。当派生类构造
//! 函数体含控制流嵌套（`if`/`foreach`/`is` 等）时，走 D10.6 解释器路径；
//! 仅含顶层 `this.<slot>(lambda)` 调用时，保留原 `walk_block_for_registrations`
//! 快路径（避免过度泛化）。
//!
//! # 支持语法
//!
//! | 构造 | 支持 | 说明 |
//! |------|------|------|
//! | `if (cond) { ... } else { ... }` | ✅ | 条件分支；cond 必须编译期可求值 bool |
//! | `expression is ClassExpression classDef` | ✅ | 模式匹配；按 Value 变体分派 |
//! | `foreach (var m in classDef.Methods) { ... }` | ✅ | 遍历编译期已知集合（含 List） |
//! | `var x = expr;` | ✅ | 局部变量声明 |
//! | `this.Build(() => { ... })` / `this.Register(() => { ... })` | ✅ | 注册展开委托 |
//! | `sb.Append(...)` / `sb.ToString()` | ✅ | 白名单方法调用 |
//! | `expression.Method` / `classDef.Methods` / `method.Parameters` / `method.Attributes` | ✅ | 成员访问 |
//! | `list.Count` / `a > b` / `a == b` 等 | ✅ | List 长度属性 + 二元比较（Int/String）|
//! | `throw new Error("E0340: ...")` / `throw "E0340: ..."` | ✅ | 编译期诊断抛出（错误码透传） |
//! | `while` / `for` / 递归 / IO / 反射 | ❌ | 禁止 |
//!
//! # `throw` 错误码透传机制（D-2 扩展）
//!
//! D10.6 解释器**不感知 QIF 语义**，但支持 `throw new Error("E0340: msg")`
//! 形式的编译期诊断抛出——字符串内容遵循 `"<CODE>: <MESSAGE>"` 约定
//! （如 `"E0340: [Fact] 方法必须无参数"`），解释器解析前缀错误码 + 消息，
//! 透传为 [`CtorInterpError::UserThrown`]。调用方（`expand_feature_registrations_with_locals`）
//! 将其转译为 `TypeError::Generic(format!("error[{code}]: {message}"))`，
//! 保留错误码与消息。错误码语义归 Arc 侧派生类（如 `std/QIF/`），解释器零领域知识。
//!
//! # 架构红线
//!
//! D10.6 解释器**不感知 QIF 语义**——仅识别 `GenerateToAttribute<T>` 派生类
//! 构造函数体中的通用构造，按 Value 类型分派。QIF 的 FactAttribute 等
//! marker attribute 是 D10.6 解释器的**消费者**，不是**特化**。

use std::collections::HashMap;

use ast::{Block, Expr, Ident, IsPattern, Span, Spanned, Stmt, Type};
#[cfg(test)]
use ast::{LambdaBody, LambdaExpr};

#[cfg(test)]
use super::evaluator::{AttributeDataValue, ClassExpressionValue, MethodExpressionValue};
use super::evaluator::{ParameterDataValue, Value};
use super::{MacroRegistration, MacroSlot};

/// D10.6 解释器错误（Pass 2 阶段诊断）。
///
/// 字段通过 `format!("{:?}", err)` Debug 格式化被上层（mod.rs 的
/// `expand_feature_registrations_with_locals`）读取转译为 `TypeError::Generic`，
/// 编译器无法识别 Debug derive 的使用——`#[allow(dead_code)]` 抑制误报。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum CtorInterpError {
    /// 不支持的语句构造（如 `while`/`for`/`try` 等）。
    UnsupportedStmt { stmt: String, span: Span },
    /// 不支持的表达式构造。
    UnsupportedExpr { expr: String, span: Span },
    /// `foreach` 迭代对象不是编译期已知集合（非 ClassExpression.Methods/
    /// ClassExpression.Attributes 等）。
    ForeachOverNonIterable { actual_type: String, span: Span },
    /// `if` 条件求值失败或非 bool 类型。
    IfCondNotBool { actual_type: String, span: Span },
    /// RFC 032 D-2: 用户在派生类构造函数体中通过 `throw new Error("E0340: ...")`
    /// 或 `throw "..."` 主动抛出的编译期诊断。
    ///
    /// D10.6 解释器**不感知 QIF 语义**——`code` 与 `message` 由 Arc 侧派生类
    /// （如 `std/QIF/FactAttribute.as`）决定。解释器仅做字符串解析与透传。
    ///
    /// 解析约定：throw 表达式求值结果为字符串，字符串遵循 `"<CODE>: <MESSAGE>"`
    /// 格式（如 `"E0340: [Fact] 方法必须无参数"`）。前缀 `<CODE>:` 缺失时，
    /// `code = "E0000"`，`message` 取整个字符串。
    ///
    /// 调用方转译为 `TypeError::Generic(format!("error[{code}]: {message}"))`
    /// 报告给用户，保留 Arc 侧派生类定义的错误码与消息。
    UserThrown {
        code: String,
        message: String,
        span: Span,
    },
}

/// D10.6 解释器环境——变量名到 Value 的绑定。
///
/// 模式匹配绑定的变量（`expression is ClassExpression classDef` 中的 `classDef`）
/// 与 `foreach` 迭代变量（`foreach (var method in ...)` 中的 `method`）都注册到此环境。
#[derive(Clone, Debug, Default)]
pub struct Environment {
    bindings: HashMap<String, Value>,
}

impl Environment {
    pub fn new() -> Self {
        Self::default()
    }

    /// 绑定变量名到 Value。
    pub fn bind(&mut self, name: &str, value: Value) {
        self.bindings.insert(name.to_string(), value);
    }

    /// 查询变量绑定。返回 `None` 表示变量未绑定。
    pub fn lookup(&self, name: &str) -> Option<&Value> {
        self.bindings.get(name)
    }

    /// 进入子作用域——创建环境副本（D10.6 不支持变量 shadowing，但 `if`/`foreach`
    /// 嵌套作用域中绑定的变量仅在子作用域内可见）。
    ///
    /// 当前简化实现：直接 clone 整个环境，子作用域内的 bind 不影响父作用域。
    pub fn enter_scope(&self) -> Self {
        self.clone()
    }
}

/// D10.6 构造函数体编译期解释器主体。
///
/// 调用方：`TypeChecker::collect_feature_registrations` 在派生类构造函数体
/// 含控制流嵌套时构造此解释器并调用 `interpret`。
///
/// 输入：派生类构造函数体 AST `Block` + 初始环境（含 `expression` 形参绑定）+
///       关联容器的 public 方法槽位列表（识别 `this.<slot>(lambda)` 调用）。
///
/// 输出：`Vec<MacroRegistration>`——识别到的所有 `this.Build`/`this.Register`
///       注册调用。每个 registration 含一个 `LambdaExpr`（待 Pass 3 由 D10.2
///       受限求值器求值）。
pub struct CtorInterpreter<'a> {
    /// 关联容器类的 public 方法槽位（识别 `this.<slot>` 调用合法性）。
    slots: &'a [MacroSlot],
    /// 累积诊断错误（不中断解释，继续扫描剩余语句）。
    errors: Vec<CtorInterpError>,
    /// RFC 032 D-3: `throw` 短路标志——一旦 `Stmt::Throw` 抛出 UserThrown
    /// 错误，置位此字段，所有后续 `interp_*` 调用立即返回，使同一作用域内
    /// 第一个 throw 后的语句不再执行（符合 Arc 侧 throw 短路语义）。
    ///
    /// 此标志在 `interpret` 整个生命周期内单调置位——一旦 true 永久 true，
    /// 跨 `foreach` 迭代也保持（同一构造函数体内首个 throw 即终止全部后续
    /// 解释，避免多个 throw 在同一方法上同时触发产生重复诊断）。
    halted: bool,
}

impl<'a> CtorInterpreter<'a> {
    /// 构造解释器。`slots` 是关联容器类的 public 方法槽位列表
    /// （来自 `MacroCatalog.slots_of(container)`）。
    pub fn new(slots: &'a [MacroSlot]) -> Self {
        Self {
            slots,
            errors: Vec::new(),
            halted: false,
        }
    }

    /// 解释执行构造函数体 `body`，返回识别到的注册列表与累积错误。
    ///
    /// `initial_env` 应已绑定派生类构造函数的 Expression 形参（如
    /// `expression` → `Value::ClassExpression(...)`）。
    pub fn interpret(
        mut self,
        body: &Block,
        initial_env: &Environment,
    ) -> (Vec<MacroRegistration>, Vec<CtorInterpError>) {
        let mut regs = Vec::new();
        let mut env = initial_env.clone();
        self.interp_block(body, &mut env, &mut regs);
        (regs, self.errors)
    }

    /// 解释执行 Block——遍历语句列表，对每条语句分派 `interp_stmt`。
    fn interp_block(
        &mut self,
        block: &Block,
        env: &mut Environment,
        regs: &mut Vec<MacroRegistration>,
    ) {
        for stmt in &block.stmts {
            // D-3 短路：throw 抛出后同作用域内后续语句不再执行。
            if self.halted {
                return;
            }
            self.interp_stmt(&stmt.node, env, regs);
        }
        // Block 的 tail 表达式（如 `void Method() { ...; expr }` 的末尾 expr）
        // 在构造函数体中较少见，但为完整性也尝试识别其中的注册调用。
        if !self.halted {
            if let Some(tail) = &block.tail {
                self.interp_expr(&tail.node, env, regs);
            }
        }
    }

    /// 解释执行单条语句。
    ///
    /// 支持的语句变体：
    /// - `Stmt::Expr(expr)`：表达式语句（含 `this.Build(...)` 注册调用，
    ///   以及 `Stmt::Expr(Expr::If { .. })` 形式的 if 语句——parser 把
    ///   `if` 解析为 `Expr::If` 表达式，作为 `Stmt::Expr` 包装出现）
    /// - `Stmt::Let { init, .. }`：局部变量声明
    /// - `Stmt::For { var, iter, body }`：遍历编译期已知集合（parser 把
    ///   `foreach` 与 `for` 统一解析为 `Stmt::For`）
    /// - `Stmt::Assign { target, value }`：变量赋值
    /// - `Stmt::Throw { expr }`：编译期诊断抛出——求值 expr 为字符串，
    ///   解析 `"<CODE>: <MESSAGE>"` 前缀，累积 `UserThrown` 错误（D-2 扩展）
    /// - 其他（`while`/`try`/`using`）：累积 UnsupportedStmt 错误
    fn interp_stmt(
        &mut self,
        stmt: &Stmt,
        env: &mut Environment,
        regs: &mut Vec<MacroRegistration>,
    ) {
        match stmt {
            Stmt::Expr(expr) => {
                // 识别 `if (cond) { ... } else { ... }` —— parser 把 if
                // 解析为 `Expr::If` 表达式，作为 `Stmt::Expr` 包装。
                if let Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } = &expr.node
                {
                    self.interp_if(cond, then_branch, else_branch.as_ref(), env, regs);
                } else {
                    self.interp_expr(&expr.node, env, regs);
                }
            }
            Stmt::Let { name, init, .. } => {
                if let Some(init_expr) = init {
                    let val = self.eval_expr(&init_expr.node, env);
                    if let Some(v) = val {
                        env.bind(name.as_str(), v);
                    }
                }
            }
            Stmt::For { var, iter, body } => {
                self.interp_foreach(var, iter, body, env, regs);
            }
            Stmt::Assign { target, value } => {
                // 简化：仅支持变量名赋值（`x = expr`）
                if let Expr::Ident(name) = &target.node {
                    let val = self.eval_expr(&value.node, env);
                    if let Some(v) = val {
                        env.bind(name.as_str(), v);
                    }
                }
            }
            Stmt::Return(_) => {
                // 构造函数体的 return 不影响注册识别——忽略。
            }
            Stmt::Throw { expr } => {
                // RFC 034 D-2: 编译期诊断抛出。求值 throw 表达式为字符串，
                // 解析 `"<CODE>: <MESSAGE>"` 前缀，累积 UserThrown 错误。
                // 解释器不感知 QIF 语义——错误码与消息由 Arc 侧派生类决定。
                let thrown_span = expr.span;
                if let Some(thrown_val) = self.eval_expr(&expr.node, env) {
                    if let Some((code, message)) = self.extract_throw_message(&thrown_val) {
                        self.errors.push(CtorInterpError::UserThrown {
                            code,
                            message,
                            span: thrown_span,
                        });
                    } else {
                        // throw 表达式求值成功但非字符串——记录原始 Debug 形式
                        self.errors.push(CtorInterpError::UserThrown {
                            code: "E0000".to_string(),
                            message: format!("<non-string throw value: {:?}>", thrown_val),
                            span: thrown_span,
                        });
                    }
                    // D-3 短路：throw 抛出后置位 halted，使同作用域内后续语句
                    // 不再执行（含跨 foreach 迭代，halted 在 interpret 整个
                    // 生命周期内单调置位）。
                    self.halted = true;
                } else {
                    // throw 表达式求值失败——记录 UnsupportedExpr，让上层处理
                    self.errors.push(CtorInterpError::UnsupportedExpr {
                        expr: format!("{:?}", expr.node)
                            .split('(')
                            .next()
                            .unwrap_or("?")
                            .to_string(),
                        span: thrown_span,
                    });
                    // D-3 短路：求值失败的 throw 同样视为抛出，置位 halted。
                    self.halted = true;
                }
            }
            other => {
                self.errors.push(CtorInterpError::UnsupportedStmt {
                    stmt: format!("{:?}", other)
                        .split('(')
                        .next()
                        .unwrap_or("?")
                        .to_string(),
                    span: Span::DUMMY,
                });
            }
        }
    }

    /// 从 throw 表达式求值结果中提取错误码与消息（D-2 扩展）。
    ///
    /// 支持的 throw 形式：
    /// - `throw new Error("E0340: msg")` → 求值时识别 `Error` 类型构造，
    ///   从参数列表提取首个字符串字面量
    /// - `throw "E0340: msg"` → 直接求值为 `Value::String`
    ///
    /// 字符串遵循 `"<CODE>: <MESSAGE>"` 约定：
    /// - 前缀 `<CODE>:` 形如 `E0340: `（错误码 + 冒号 + 空格）
    /// - 前缀缺失时，`code = "E0000"`，`message` 取整个字符串
    ///
    /// 返回 `None` 表示 throw 表达式求值结果不是字符串。
    fn extract_throw_message(&self, val: &Value) -> Option<(String, String)> {
        let s = match val {
            Value::String(s) => s.clone(),
            _ => return None,
        };
        // 解析 `"<CODE>: <MESSAGE>"` 前缀——CODE 形如 `[A-Z][0-9]+`
        if let Some((code, rest)) = parse_error_code_prefix(&s) {
            // rest 形如 ": <MESSAGE>"——去掉冒号 + 前导空白
            let message = rest.get(1..).unwrap_or("").trim_start().to_string();
            Some((code.to_string(), message))
        } else {
            Some(("E0000".to_string(), s))
        }
    }

    /// 解释 `if` 语句——先进入子作用域再求值条件，使 `is T name` 绑定落到
    /// branch_env（不影响 if 语句外的环境），按 bool 结果选择分支递归
    /// `interp_block`。
    fn interp_if(
        &mut self,
        cond: &Spanned<Expr>,
        then_block: &Block,
        else_block: Option<&Block>,
        env: &mut Environment,
        regs: &mut Vec<MacroRegistration>,
    ) {
        // 进入子作用域——`if` 内的 is 模式绑定仅落在 branch_env。
        let mut branch_env = env.enter_scope();
        let cond_val = self.eval_expr(&cond.node, &mut branch_env);
        let Some(Value::Bool(b)) = cond_val else {
            self.errors.push(CtorInterpError::IfCondNotBool {
                actual_type: match cond_val {
                    Some(v) => v.type_name().to_string(),
                    None => "<unknown>".into(),
                },
                span: cond.span,
            });
            return;
        };
        if b {
            self.interp_block(then_block, &mut branch_env, regs);
        } else if let Some(else_b) = else_block {
            self.interp_block(else_b, &mut branch_env, regs);
        }
    }

    /// 解释 `foreach` 语句——求值迭代集合，对每个元素绑定迭代变量后递归 body。
    ///
    /// 当前支持的迭代源：
    /// - `ClassExpression`：默认迭代 `Methods`（最常见场景），每个元素绑定
    ///   为 `Value::MethodExpression`
    /// - `Value::List(buf)`：迭代列表元素（D-2 扩展），每个元素按其在 buf 中
    ///   的原 Value 类型绑定（如 `Value::String` 用于参数名/attribute 名列表）
    fn interp_foreach(
        &mut self,
        var: &Ident,
        iter: &Spanned<Expr>,
        body: &Block,
        env: &mut Environment,
        regs: &mut Vec<MacroRegistration>,
    ) {
        // 求值 iter 表达式——应为成员访问（`classDef.Methods` 等）
        let iter_val = self.eval_expr(&iter.node, env);
        let Some(iter_value) = iter_val else {
            self.errors.push(CtorInterpError::ForeachOverNonIterable {
                actual_type: "<unknown>".into(),
                span: iter.span,
            });
            return;
        };
        match iter_value {
            Value::ClassExpression(ce) => {
                // 默认迭代 Methods（最常见场景）
                //
                // D-4 修复：不使用 `enter_scope()`——foreach 迭代间需保留 body 内
                // 对外层变量的修改（如 E0342 校验中 `i = i + 1` 计数器递增需跨
                // 迭代累积）。`enter_scope()` 的 clone 语义会使每轮迭代的修改丢失。
                // 迭代变量直接绑定到 `env`，下一轮迭代覆盖绑定。
                for method in ce.methods.clone() {
                    // D-3 短路：throw 抛出后停止后续迭代。
                    if self.halted {
                        break;
                    }
                    env.bind(var.as_str(), Value::MethodExpression(method));
                    self.interp_block(body, env, regs);
                }
            }
            Value::List(buf) => {
                // D-2 扩展：迭代 List 元素（如 `method.Parameters`/`method.Attributes`）
                //
                // D-4 修复：同上，不使用 `enter_scope()` 以保留 body 内的变量修改。
                let elements: Vec<Value> = buf.borrow().clone();
                for element in elements {
                    // D-3 短路：throw 抛出后停止后续迭代。
                    if self.halted {
                        break;
                    }
                    env.bind(var.as_str(), element);
                    self.interp_block(body, env, regs);
                }
            }
            other => {
                self.errors.push(CtorInterpError::ForeachOverNonIterable {
                    actual_type: other.type_name().to_string(),
                    span: iter.span,
                });
            }
        }
    }

    /// 解释表达式——可能是注册调用（`this.Build(lambda)`）或求值表达式（`x.Y`）。
    ///
    /// 注册调用识别规则（与 `walk_expr_for_registrations` 一致）：
    /// - `Expr::MethodCall { receiver: Expr::This, method: <slot_name>, args: [Lambda] }`
    /// - `method` 必须在 `self.slots` 中
    /// - `args.len() == 1` 且唯一参数是 `Expr::Lambda`
    fn interp_expr(
        &mut self,
        expr: &Expr,
        env: &mut Environment,
        regs: &mut Vec<MacroRegistration>,
    ) {
        if let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = expr
        {
            if matches!(receiver.node, Expr::This) && args.len() == 1 {
                if let Expr::Lambda(lambda) = &args[0].node {
                    if self.slots.iter().any(|s| &s.method_name == method) {
                        regs.push(MacroRegistration {
                            slot_name: method.clone(),
                            expansion: lambda.clone(),
                            span: Span::DUMMY,
                            expression_locals: Vec::new(),
                        });
                        return;
                    }
                }
            }
        }
        // 非注册调用——尝试求值（用于 if 条件、foreach 迭代源等）
        let _ = self.eval_expr(expr, env);
    }

    /// 求值表达式——返回 Value 或 None（无法求值时不报错，让上层处理）。
    ///
    /// 支持的表达式：
    /// - `Expr::Ident(name)`：变量查找（含 `expression` 形参绑定）
    /// - `Expr::This`：返回 Value::Null（this 不参与值求值，仅用于注册调用识别）
    /// - `Expr::Is { expr, pattern }`：模式匹配——仅支持 `IsPattern::Type { ty, binding }`，
    ///   返回 Value::Bool；匹配成功且 `binding` 存在时绑定到 env（在 `interp_if` 中
    ///   env 即 branch_env，使 `if (e is T name) { ... }` 内 `name` 可见）
    /// - `Expr::Field { receiver, field }`：成员访问（`classDef.Methods` 等）
    /// - `Expr::BoolLit(b)` / `Expr::StringLit(s)` / `Expr::IntLit(n)`：字面量
    /// - `Expr::Binary { op, left, right }`：二元比较（D-2 扩展）——
    ///   支持 `>`/`<`/`>=`/`<=`/`==`/`!=`，操作数限 Int vs Int 或 String vs String
    /// - `Expr::New { ty, args, .. }`：构造调用（D-2 扩展）——仅识别
    ///   `new Error(stringLit)` 形式，返回 `Value::String(s)`（透传字符串内容）
    /// - `Expr::MethodCall { receiver, method, args, .. }`：方法调用（D-2 扩展）——
    ///   仅识别 `list.Contains(value)` 形式，返回 `Value::Bool`
    /// - 其他：返回 None
    fn eval_expr(&mut self, expr: &Expr, env: &mut Environment) -> Option<Value> {
        match expr {
            Expr::Ident(name) => env.lookup(name.as_str()).cloned(),
            Expr::This => Some(Value::Null),
            Expr::BoolLit(b) => Some(Value::Bool(*b)),
            Expr::IntLit(n) => Some(Value::Int(*n)),
            Expr::StringLit(s) => Some(Value::String(s.clone())),
            Expr::Is {
                expr: inner,
                pattern,
            } => {
                // 求值 inner——可能递归 is（罕见，但理论可嵌套）
                let inner_val = self.eval_expr(&inner.node, env)?;
                // 仅支持 IsPattern::Type { ty, binding }——Var/Null 模式不应用于
                // D10.6 解释器场景（被赋能类 attribute 的 Expression 形参恒为
                // ClassExpression/MethodExpression，不需要 var/null 模式）
                let IsPattern::Type { ty, binding } = pattern else {
                    return None;
                };
                // 从 Type 节点提取最末段名（如 `ClassExpression`、`MethodExpression`）
                let type_name = match &ty.node {
                    Type::Named { path, .. } => path.last().map(|i| i.as_str().to_string()),
                    _ => None,
                };
                let type_name = type_name?;
                let type_matches = matches!(
                    (&inner_val, type_name.as_str()),
                    (Value::ClassExpression(_), "ClassExpression")
                        | (Value::MethodExpression(_), "MethodExpression")
                        | (Value::Expression { .. }, "Expression")
                );
                // 匹配成功且存在绑定名——绑定到 env（在 interp_if 中 env 是 branch_env）
                if type_matches {
                    if let Some(name) = binding {
                        env.bind(name.as_str(), inner_val.clone());
                    }
                }
                Some(Value::Bool(type_matches))
            }
            Expr::Field { receiver, field } => {
                let recv_val = self.eval_expr(&receiver.node, env)?;
                self.eval_field_access(&recv_val, field.as_str())
            }
            Expr::Index { receiver, index } => {
                // D-4 新增：`list[i]` 索引访问——支持 E0342 类型匹配校验中
                // `attr.Args[i]`/`method.Parameters[i]` 等并行索引访问场景。
                //
                // receiver 必须求值为 `Value::List`，index 必须求值为 `Value::Int`。
                // 越界返回 None（让上层处理为 IfCondNotBool 或忽略）。
                let recv_val = self.eval_expr(&receiver.node, env)?;
                let idx_val = self.eval_expr(&index.node, env)?;
                match (recv_val, idx_val) {
                    (Value::List(buf), Value::Int(i)) => {
                        let idx = i as usize;
                        let guard = buf.borrow();
                        if idx < guard.len() {
                            Some(guard[idx].clone())
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Expr::Binary { op, left, right } => {
                // D-2 扩展：支持 `>`/`<`/`>=`/`<=`/`==`/`!=` 二元比较。
                // 操作数限 Int vs Int 或 String vs String——其他类型组合返回 None。
                // D-4 扩展：支持 `BinOp::Add` 整数加法（用于计数变量 `i = i + 1`）。
                let lv = self.eval_expr(&left.node, env)?;
                let rv = self.eval_expr(&right.node, env)?;
                eval_binary_op(*op, &lv, &rv)
            }
            Expr::New { ty, args, .. } => {
                // D-2 扩展：识别 `new Error("...")` 形式，提取字符串参数。
                // 仅识别 Error 类型构造——其他类型返回 None。
                let type_name = match &ty.node {
                    Type::Named { path, .. } => path.last().map(|i| i.as_str().to_string()),
                    _ => None,
                };
                if type_name.as_deref() != Some("Error") {
                    return None;
                }
                // 取首个参数——必须为字符串字面量
                if args.len() != 1 {
                    return None;
                }
                if let Expr::StringLit(s) = &args[0].node {
                    Some(Value::String(s.clone()))
                } else {
                    // 非 StringLit 参数——尝试递归求值
                    self.eval_expr(&args[0].node, env)
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                // D-2 扩展：识别 `list.Contains(value)` 形式（List<string> 的 Contains
                // 方法），返回 Value::Bool。其他方法调用返回 None（如 `sb.Append(...)`
                // 等注册类调用由 interp_expr 顶层处理，不会进入此处）。
                let recv_val = self.eval_expr(&receiver.node, env)?;
                self.eval_method_call(&recv_val, method.as_str(), args)
            }
            _ => None,
        }
    }

    /// 求值方法调用 `recv.method(args)`——按 Value 变体分派（D-2/D-3 扩展）。
    ///
    /// 当前支持：
    /// - `Value::List(buf).Contains(stringLit)` → `Value::Bool`
    ///   用于 `method.Attributes.Contains("Fact")` 等场景。
    ///   D-3 扩展：列表元素为 `Value::AttributeData` 时按 `name` 匹配
    ///   （`method.Attributes.Contains("Fact")` 中 Attributes 是
    ///   `List<AttributeData>`，按 AttributeData.Name 匹配）。
    /// - `Value::List(buf).Get(stringLit)` → `Value::List<AttributeData>`
    ///   用于 `method.Attributes.Get("InlineData")` 场景——返回过滤后
    ///   仅含指定名称 attribute 的新列表（D-3 新增）。
    /// - 其他方法调用 → None
    ///
    /// 简化：`Contains`/`Get` 参数仅支持字符串字面量（避免 &mut self 与 env &mut
    /// 借用冲突）。如需支持变量参数，可在外层预先求值 args 后传入。
    fn eval_method_call(
        &self,
        recv: &Value,
        method: &str,
        args: &[Spanned<Expr>],
    ) -> Option<Value> {
        match recv {
            Value::List(buf) => {
                if method == "Contains" && args.len() == 1 {
                    if let Expr::StringLit(s) = &args[0].node {
                        // D-3: 同时支持 List<string> 与 List<AttributeData>
                        // - List<string>：直接字符串相等
                        // - List<AttributeData>：按 AttributeData.Name 匹配
                        let contains = buf.borrow().iter().any(|v| match v {
                            Value::String(item) => item == s,
                            Value::AttributeData(ad) => ad.name == *s,
                            _ => false,
                        });
                        return Some(Value::Bool(contains));
                    }
                }
                if method == "Get" && args.len() == 1 {
                    // D-3 新增：List<AttributeData>.Get(name) → List<AttributeData>
                    // 过滤保留指定名称的 AttributeData，其他变体不保留
                    if let Expr::StringLit(s) = &args[0].node {
                        let filtered: Vec<Value> = buf
                            .borrow()
                            .iter()
                            .filter(|v| matches!(v, Value::AttributeData(ad) if ad.name == *s))
                            .cloned()
                            .collect();
                        return Some(Value::List(std::rc::Rc::new(std::cell::RefCell::new(
                            filtered,
                        ))));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 求值成员访问 `recv.field`——按 Value 变体分派。
    ///
    /// 当前支持：
    /// - `ClassExpression.ClassName` → `Value::String`
    /// - `ClassExpression.Methods` → `Value::ClassExpression` 副本（含 methods 列表）
    ///   注：实际 foreach 通过 `interp_foreach` 直接读取 `ClassExpressionValue.methods`
    /// - `ClassExpression.Attributes` → `Value::List<AttributeData>`（类级 attribute 列表，D-3 升级）
    /// - `MethodExpression.Name` → `Value::String`
    /// - `MethodExpression.ReturnType` → `Value::String`
    /// - `MethodExpression.Parameters` → `Value::List<string>`（参数名列表，D-2 扩展）
    ///   注：返回参数名字符串列表（简化，不带类型信息——类型信息可通过
    ///   `ParameterExpression.Type` 进一步访问，当前不实现）
    /// - `MethodExpression.Attributes` → `Value::List<AttributeData>`（方法级 attribute 列表，D-3 升级）
    /// - `Value::List(buf).Count` → `Value::Int`（列表长度，D-2 扩展）
    /// - `AttributeData.Name` → `Value::String`（D-3 新增）
    /// - `AttributeData.Args` → `Value::List`（位置参数列表，D-3 新增）
    /// - 其他成员访问 → None
    fn eval_field_access(&self, recv: &Value, field: &str) -> Option<Value> {
        match recv {
            Value::ClassExpression(ce) => {
                match field {
                    "ClassName" => Some(Value::String(ce.class_name.clone())),
                    "Methods" => {
                        // foreach 路径已直接读 `ce.methods`，此处返回 Clone 副本
                        // 供其他场景（如 `var ms = classDef.Methods;`）使用。
                        Some(Value::ClassExpression(ce.clone()))
                    }
                    "Attributes" => {
                        // D-3 升级：类级 attribute 列表 → List<AttributeData>
                        let list: Vec<Value> = ce
                            .attributes
                            .iter()
                            .map(|ad| Value::AttributeData(ad.clone()))
                            .collect();
                        Some(Value::List(std::rc::Rc::new(std::cell::RefCell::new(list))))
                    }
                    _ => None,
                }
            }
            Value::MethodExpression(me) => match field {
                "Name" => Some(Value::String(me.name.clone())),
                "ReturnType" => Some(Value::String(me.return_type.clone())),
                "Parameters" => {
                    // D-4 升级：参数列表 → List<ParameterData>（携带 Name+Type，
                    // 与 AttributeDataValue 对称设计）。原先 D-2 仅返回参数名
                    // 字符串列表，D-4 升级为完整参数元数据以支持 E0342 类型匹配。
                    let list: Vec<Value> = me
                        .parameters
                        .iter()
                        .map(|(name, ty)| {
                            Value::ParameterData(ParameterDataValue {
                                name: name.clone(),
                                ty: ty.clone(),
                            })
                        })
                        .collect();
                    Some(Value::List(std::rc::Rc::new(std::cell::RefCell::new(list))))
                }
                "Attributes" => {
                    // D-3 升级：方法级 attribute 列表 → List<AttributeData>
                    let list: Vec<Value> = me
                        .attributes
                        .iter()
                        .map(|ad| Value::AttributeData(ad.clone()))
                        .collect();
                    Some(Value::List(std::rc::Rc::new(std::cell::RefCell::new(list))))
                }
                _ => None,
            },
            Value::List(buf) => match field {
                "Count" => Some(Value::Int(buf.borrow().len() as i64)),
                _ => None,
            },
            Value::AttributeData(ad) => match field {
                // D-3 新增：AttributeData 字段访问
                "Name" => Some(Value::String(ad.name.clone())),
                "Args" => {
                    // 位置参数列表 → List<Value>（按声明顺序，可能含 String/Int/Bool）
                    let list: Vec<Value> = ad.args.to_vec();
                    Some(Value::List(std::rc::Rc::new(std::cell::RefCell::new(list))))
                }
                _ => None,
            },
            Value::ParameterData(pd) => match field {
                // D-4 新增：ParameterData 字段访问
                "Name" => Some(Value::String(pd.name.clone())),
                "Type" => Some(Value::String(pd.ty.clone())),
                _ => None,
            },
            // D-4 新增：Value.TypeName 字段访问——返回值类型名字符串。
            // 用于 E0342 类型匹配：`attr.Args[i].TypeName != method.Parameters[i].Type`。
            // D10.6 解释器不感知类型等价性（如 int vs i32），仅返回规范化类型名，
            // 类型等价性判断归 Arc 侧派生类。
            v => match field {
                "TypeName" => Some(Value::String(value_type_name(v))),
                _ => None,
            },
        }
    }
}

/// 解析错误码前缀——`"E0340: msg"` → `("E0340", ": msg")`（D-2 扩展）。
///
/// 错误码形如 `[A-Z]+[0-9]+`（如 `E0340`/`W0123`/`E0001`），后跟冒号。
/// 返回 `Some((code, rest))` 其中 `rest` 含冒号与剩余文本（调用方自行 trim）。
/// 无匹配返回 `None`。
fn parse_error_code_prefix(s: &str) -> Option<(&str, &str)> {
    // 找到首个冒号位置
    let colon_pos = s.find(':')?;
    let prefix = &s[..colon_pos];
    // 校验前缀形如 `[A-Z]+[0-9]+`
    if prefix.is_empty() {
        return None;
    }
    let mut chars = prefix.chars();
    let first = chars.next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let rest: String = chars.collect();
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((prefix, &s[colon_pos..]))
}

/// 返回 Value 的规范化类型名（D-4 扩展）。
///
/// 用于 `value.TypeName` 字段访问——支持 E0342 类型匹配校验：
/// `attr.Args[i].TypeName != method.Parameters[i].Type`。
///
/// 规范化映射：
/// - `Int` → `"int"`
/// - `String` → `"string"`
/// - `Bool` → `"bool"`
/// - `Null` → `"null"`
/// - 其他 → `Value::type_name()` 返回值（如 `"AttributeData"`/`"List"` 等）
///
/// D10.6 解释器不感知类型等价性（如 `int` vs `i32` vs `long`）——仅返回
/// 字面量值的规范化类型名。类型等价性判断归 Arc 侧派生类（如 TheoryAttribute
/// 可在比较前规范化方法形参类型名 `i32`/`i64`/`long` → `"int"`）。
fn value_type_name(v: &Value) -> String {
    match v {
        Value::Int(_) => "int".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Null => "null".to_string(),
        other => other.type_name().to_string(),
    }
}

/// 求值二元比较运算（D-2 扩展 + D-4 整数加法扩展）。
///
/// 支持的运算符：
/// - 比较运算（D-2）：`>`/`<`/`>=`/`<=`/`==`/`!=`
/// - 算术运算（D-4）：`+`（整数加法，用于计数变量 `i = i + 1`）
///
/// 操作数类型组合：
/// - `Int` vs `Int` → 数值比较（D-2）+ 整数加法（D-4）
/// - `String` vs `String` → 字符串相等性比较（仅 `==`/`!=`，其他返回 None）
/// - 其他组合 → 返回 None（让上层处理为 IfCondNotBool）
fn eval_binary_op(op: ast::BinOp, lv: &Value, rv: &Value) -> Option<Value> {
    use ast::BinOp;
    match (lv, rv) {
        (Value::Int(l), Value::Int(r)) => {
            // D-4 扩展：整数加法——用于 E0342 校验中的计数变量 `i = i + 1`。
            if let BinOp::Add = op {
                return Some(Value::Int(l + r));
            }
            let b = match op {
                BinOp::Gt => l > r,
                BinOp::Lt => l < r,
                BinOp::Ge => l >= r,
                BinOp::Le => l <= r,
                BinOp::Eq => l == r,
                BinOp::NotEq => l != r,
                _ => return None,
            };
            Some(Value::Bool(b))
        }
        (Value::String(l), Value::String(r)) => {
            // 字符串仅支持相等性比较
            let b = match op {
                BinOp::Eq => l == r,
                BinOp::NotEq => l != r,
                _ => return None,
            };
            Some(Value::Bool(b))
        }
        _ => None,
    }
}

/// 从 typeck `class_defs` 构造 `ClassExpressionValue`（测试辅助函数）。
///
/// 当 typeck 识别到「被赋能类标注了派生属性」（如 `[Fact] class MyTests { ... }`），
/// 在调用 D10.6 解释器之前调用此函数，从 `class_defs` 中提取类的方法与属性
/// 构造 `Value::ClassExpression`。
///
/// `class_methods` 是该类的所有方法 AST 列表（含方法名、返回类型、属性）。
/// `class_attributes` 是类上声明的属性列表（如 `[attr("Fact")]`）。
///
/// 简化：`MethodExpressionValue.parameters` 当前填空 Vec（D10.6 解释器对参数
/// 列表的访问暂不要求，后续会话按需补全）。
///
/// RFC 034 M2 D-3: `class_methods` 元组第 3 项与 `class_attributes` 元素类型
/// 从 `Vec<String>` 升级为 `Vec<AttributeDataValue>`——携带 attribute 参数数据。
/// 测试中可使用 `attr("Fact")`/`attr_with_args("InlineData", vec![...])` 便捷构造。
///
/// **仅测试使用**：生产代码调用 `TypeChecker::build_class_expression_value_for`
/// （从 `ClassDef` 直接构造），本函数提供给测试构造简化输入（元组列表）。
#[cfg(test)]
pub fn build_class_expression_value(
    class_name: &str,
    class_methods: &[(&str, &str, Vec<AttributeDataValue>)], // (method_name, return_type, attributes)
    class_attributes: Vec<AttributeDataValue>,
) -> Value {
    let methods: Vec<MethodExpressionValue> = class_methods
        .iter()
        .map(|(name, ret, attrs)| MethodExpressionValue {
            name: name.to_string(),
            parameters: Vec::new(),
            return_type: ret.to_string(),
            attributes: attrs.clone(),
        })
        .collect();
    Value::ClassExpression(ClassExpressionValue {
        class_name: class_name.to_string(),
        methods,
        attributes: class_attributes,
    })
}

/// 测试便捷构造子：构造无参数的 `AttributeDataValue`（如 `attr("Fact")`）。
#[cfg(test)]
fn attr(name: &str) -> AttributeDataValue {
    AttributeDataValue {
        name: name.to_string(),
        args: vec![],
    }
}

/// 测试便捷构造子：构造带位置参数的 `AttributeDataValue`
/// （如 `attr_with_args("InlineData", vec![Value::Int(1), Value::Int(2)])`）。
#[cfg(test)]
fn attr_with_args(name: &str, args: Vec<Value>) -> AttributeDataValue {
    AttributeDataValue {
        name: name.to_string(),
        args,
    }
}

// ─── D-3/D-4 测试 AST 构造辅助函数 ───────────────────────────────────────
//
// 以下辅助函数减少 E0341/E0342 端到端测试中 AST 构造的样板代码。每个函数
// 返回 `Spanned<T>`（span 为 `Span::DUMMY`），与现有 D-2 测试的内联构造
// 风格一致——仅是抽取出重复模式。

#[cfg(test)]
fn ident_e(name: &str) -> Spanned<Expr> {
    Spanned::new(Expr::Ident(Ident::from(name)), Span::DUMMY)
}

#[cfg(test)]
fn int_e(n: i64) -> Spanned<Expr> {
    Spanned::new(Expr::IntLit(n), Span::DUMMY)
}

#[cfg(test)]
fn string_e(s: &str) -> Spanned<Expr> {
    Spanned::new(Expr::StringLit(s.to_string()), Span::DUMMY)
}

#[cfg(test)]
fn field_e(receiver: Spanned<Expr>, field: &str) -> Spanned<Expr> {
    Spanned::new(
        Expr::Field {
            receiver: Box::new(receiver),
            field: Ident::from(field),
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn binary_e(op: ast::BinOp, left: Spanned<Expr>, right: Spanned<Expr>) -> Spanned<Expr> {
    Spanned::new(
        Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn method_call_e(receiver: Spanned<Expr>, method: &str, args: Vec<Spanned<Expr>>) -> Spanned<Expr> {
    Spanned::new(
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method: Ident::from(method),
            args,
            type_args: vec![],
            params_span: None,
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn index_e(receiver: Spanned<Expr>, index: Spanned<Expr>) -> Spanned<Expr> {
    Spanned::new(
        Expr::Index {
            receiver: Box::new(receiver),
            index: Box::new(index),
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn if_e(cond: Spanned<Expr>, then_block: Block, else_block: Option<Block>) -> Spanned<Expr> {
    Spanned::new(
        Expr::If {
            cond: Box::new(cond),
            then_branch: then_block,
            else_branch: else_block,
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn throw_stmt(msg: &str) -> Spanned<Stmt> {
    Spanned::new(
        Stmt::Throw {
            expr: Spanned::new(
                Expr::New {
                    ty: Spanned::new(
                        Type::Named {
                            path: vec![Ident::from("Error")],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    args: vec![string_e(msg)],
                    obj_init: None,
                },
                Span::DUMMY,
            ),
        },
        Span::DUMMY,
    )
}

#[cfg(test)]
fn expr_stmt(expr: Spanned<Expr>) -> Spanned<Stmt> {
    Spanned::new(Stmt::Expr(expr), Span::DUMMY)
}

#[cfg(test)]
fn block(stmts: Vec<Spanned<Stmt>>) -> Block {
    Block { stmts, tail: None }
}

/// 构造 `this.Build(() => "")` 表达式语句（E0341/E0342 通过场景的注册调用）。
#[cfg(test)]
fn build_call_stmt() -> Spanned<Stmt> {
    let lambda = LambdaExpr {
        params: vec![],
        body: LambdaBody::Expr(Box::new(Spanned::new(
            Expr::StringLit(String::new()),
            Span::DUMMY,
        ))),
        is_expression_tree: false,
        is_async: false,
        captures: vec![],
    };
    let call = Spanned::new(
        Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
            method: Ident::from("Build"),
            args: vec![Spanned::new(Expr::Lambda(lambda), Span::DUMMY)],
            type_args: vec![],
            params_span: None,
        },
        Span::DUMMY,
    );
    expr_stmt(call)
}

/// 构造 TheoryAttribute 构造函数体的 AST——镜像 `std/QIF/Attributes/TheoryAttribute.as`。
///
/// 镜像的 Arc 代码：
/// ```arc
/// if (expression is ClassExpression classDef) {
///     foreach (var method in classDef.Methods) {
///         if (method.Attributes.Contains("Theory")) {
///             if (method.Parameters.Count == 0) {
///                 throw new Error("E0341: [Theory] 方法必须有参数");
///             }
///             var inlineDataAttrs = method.Attributes.Get("InlineData");
///             if (inlineDataAttrs.Count == 0) {
///                 throw new Error("E0341: [Theory] 方法须配合至少一个 [InlineData]");
///             }
///             foreach (var attr in inlineDataAttrs) {
///                 if (attr.Args.Count != method.Parameters.Count) {
///                     throw new Error("E0341: [Theory] 方法参数数量须与 [InlineData] 匹配");
///                 }
///             }
///             foreach (var attr in inlineDataAttrs) {
///                 var i = 0;
///                 foreach (var arg in attr.Args) {
///                     var param = method.Parameters[i];
///                     if (arg.TypeName != param.Type) {
///                         throw new Error("E0342: [InlineData] 参数类型与方法形参不匹配");
///                     }
///                     i = i + 1;
///                 }
///             }
///             this.Build(() => "");
///         }
///     }
/// }
/// ```
///
/// D-3 短路语义保证：首个 throw 后 halted 置位，后续 foreach 迭代与 stmt
/// 跳过——故 E0341 三重校验中首个失败即终止，不会产生重复诊断。
#[cfg(test)]
fn build_theory_attribute_body() -> Block {
    // ── E0341 check 1: method.Parameters.Count == 0 ──
    let params_count = field_e(field_e(ident_e("method"), "Parameters"), "Count");
    let e0341_no_params_cond = binary_e(ast::BinOp::Eq, params_count, int_e(0));
    let e0341_no_params_if = if_e(
        e0341_no_params_cond,
        block(vec![throw_stmt("E0341: [Theory] 方法必须有参数")]),
        None,
    );

    // ── E0341 check 2: inlineDataAttrs.Count == 0 ──
    // var inlineDataAttrs = method.Attributes.Get("InlineData");
    let get_call = method_call_e(
        field_e(ident_e("method"), "Attributes"),
        "Get",
        vec![string_e("InlineData")],
    );
    let let_inline = Spanned::new(
        Stmt::Let {
            mutable: false,
            name: Ident::from("inlineDataAttrs"),
            init: Some(get_call),
            ty: None,
        },
        Span::DUMMY,
    );
    let inline_count = field_e(ident_e("inlineDataAttrs"), "Count");
    let e0341_no_inline_cond = binary_e(ast::BinOp::Eq, inline_count, int_e(0));
    let e0341_no_inline_if = if_e(
        e0341_no_inline_cond,
        block(vec![throw_stmt(
            "E0341: [Theory] 方法须配合至少一个 [InlineData]",
        )]),
        None,
    );

    // ── E0341 check 3: attr.Args.Count != method.Parameters.Count ──
    let attr_args_count = field_e(field_e(ident_e("attr"), "Args"), "Count");
    let method_params_count = field_e(field_e(ident_e("method"), "Parameters"), "Count");
    let e0341_mismatch_cond = binary_e(ast::BinOp::NotEq, attr_args_count, method_params_count);
    let e0341_mismatch_if = if_e(
        e0341_mismatch_cond,
        block(vec![throw_stmt(
            "E0341: [Theory] 方法参数数量须与 [InlineData] 匹配",
        )]),
        None,
    );
    let e0341_count_foreach = Spanned::new(
        Stmt::For {
            var: Ident::from("attr"),
            iter: ident_e("inlineDataAttrs"),
            body: block(vec![expr_stmt(e0341_mismatch_if)]),
        },
        Span::DUMMY,
    );

    // ── E0342: arg.TypeName != param.Type ──
    // var i = 0;
    let let_i = Spanned::new(
        Stmt::Let {
            mutable: false,
            name: Ident::from("i"),
            init: Some(int_e(0)),
            ty: None,
        },
        Span::DUMMY,
    );
    // var param = method.Parameters[i];
    let param_index = index_e(field_e(ident_e("method"), "Parameters"), ident_e("i"));
    let let_param = Spanned::new(
        Stmt::Let {
            mutable: false,
            name: Ident::from("param"),
            init: Some(param_index),
            ty: None,
        },
        Span::DUMMY,
    );
    // if (arg.TypeName != param.Type) { throw E0342 }
    let arg_typename = field_e(ident_e("arg"), "TypeName");
    let param_type = field_e(ident_e("param"), "Type");
    let e0342_cond = binary_e(ast::BinOp::NotEq, arg_typename, param_type);
    let e0342_if = if_e(
        e0342_cond,
        block(vec![throw_stmt(
            "E0342: [InlineData] 参数类型与方法形参不匹配",
        )]),
        None,
    );
    // i = i + 1;
    let i_assign = Spanned::new(
        Stmt::Assign {
            target: ident_e("i"),
            value: binary_e(ast::BinOp::Add, ident_e("i"), int_e(1)),
        },
        Span::DUMMY,
    );
    let e0342_inner_foreach = Spanned::new(
        Stmt::For {
            var: Ident::from("arg"),
            iter: field_e(ident_e("attr"), "Args"),
            body: block(vec![let_param, expr_stmt(e0342_if), i_assign]),
        },
        Span::DUMMY,
    );
    let e0342_outer_foreach = Spanned::new(
        Stmt::For {
            var: Ident::from("attr"),
            iter: ident_e("inlineDataAttrs"),
            body: block(vec![let_i, e0342_inner_foreach]),
        },
        Span::DUMMY,
    );

    // ── 组装 Theory 校验 + Build 注册（在 [Theory] 分支内） ──
    let theory_branch_body = block(vec![
        expr_stmt(e0341_no_params_if),
        let_inline,
        expr_stmt(e0341_no_inline_if),
        e0341_count_foreach,
        e0342_outer_foreach,
        build_call_stmt(),
    ]);

    // if (method.Attributes.Contains("Theory")) { ... }
    let contains_theory = method_call_e(
        field_e(ident_e("method"), "Attributes"),
        "Contains",
        vec![string_e("Theory")],
    );
    let theory_if = if_e(contains_theory, theory_branch_body, None);

    // foreach (var method in classDef.Methods) { ... }
    let methods_iter = field_e(ident_e("classDef"), "Methods");
    let methods_foreach = Spanned::new(
        Stmt::For {
            var: Ident::from("method"),
            iter: methods_iter,
            body: block(vec![expr_stmt(theory_if)]),
        },
        Span::DUMMY,
    );

    // if (expression is ClassExpression classDef) { foreach (...) }
    let is_class = Spanned::new(
        Expr::Is {
            expr: Box::new(ident_e("expression")),
            pattern: IsPattern::Type {
                ty: Spanned::new(
                    Type::Named {
                        path: vec![Ident::from("ClassExpression")],
                        generics: vec![],
                    },
                    Span::DUMMY,
                ),
                binding: Some(Ident::from("classDef")),
            },
        },
        Span::DUMMY,
    );
    let top_if = if_e(is_class, block(vec![methods_foreach]), None);

    block(vec![expr_stmt(top_if)])
}

/// 构造含单个方法的 ClassExpression Value（D-3/D-4 测试便捷构造子）。
///
/// `method_params` 是 `(name, type)` 元组列表；`method_attrs` 是方法的
/// attribute 列表（用 `attr()`/`attr_with_args()` 构造）。
#[cfg(test)]
fn class_expr_with_method(
    class_name: &str,
    method_name: &str,
    method_params: Vec<(&str, &str)>,
    method_attrs: Vec<AttributeDataValue>,
) -> Value {
    let method_val = MethodExpressionValue {
        name: method_name.to_string(),
        parameters: method_params
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect(),
        return_type: "void".to_string(),
        attributes: method_attrs,
    };
    Value::ClassExpression(ClassExpressionValue {
        class_name: class_name.to_string(),
        methods: vec![method_val],
        attributes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::super::MethodModifier;
    use super::*;

    /// 骨架测试：构造 ClassExpressionValue 并验证字段访问。
    #[test]
    fn d10_6_build_class_expression_value() {
        let val = build_class_expression_value(
            "MyTests",
            &[
                ("Test1", "void", vec![attr("Fact")]),
                ("Test2", "void", vec![attr("Theory")]),
            ],
            vec![],
        );
        match val {
            Value::ClassExpression(ce) => {
                assert_eq!(ce.class_name, "MyTests");
                assert_eq!(ce.methods.len(), 2);
                assert_eq!(ce.methods[0].name, "Test1");
                assert_eq!(ce.methods[0].attributes.len(), 1);
                assert_eq!(ce.methods[0].attributes[0].name, "Fact");
                assert_eq!(ce.methods[1].name, "Test2");
            }
            _ => panic!("expected ClassExpression value"),
        }
    }

    /// 骨架测试：Environment bind/lookup。
    #[test]
    fn d10_6_environment_bind_lookup() {
        let mut env = Environment::new();
        env.bind("expression", build_class_expression_value("X", &[], vec![]));
        assert!(matches!(
            env.lookup("expression"),
            Some(Value::ClassExpression(_))
        ));
        assert!(env.lookup("nonexistent").is_none());
    }

    /// 骨架测试：CtorInterpreter 空 slots 时识别零注册。
    #[test]
    fn d10_6_empty_slots_no_regs() {
        let interp = CtorInterpreter::new(&[]);
        let env = Environment::new();
        let block = Block {
            stmts: vec![],
            tail: None,
        };
        let (regs, errs) = interp.interpret(&block, &env);
        assert!(regs.is_empty());
        assert!(errs.is_empty());
    }

    /// 端到端测试：模拟 FactAttribute 构造函数体识别。
    ///
    /// 构造 AST：
    /// ```
    /// if (expression is ClassExpression classDef) {
    ///     foreach (var method in classDef.Methods) {
    ///         this.Build(() => "");
    ///     }
    /// }
    /// ```
    ///
    /// 验证 D10.6 解释器能识别嵌套控制流中的 Build 注册调用，
    /// 对一个含 2 个方法的 ClassExpression 应识别到 2 个注册。
    #[test]
    fn d10_6_e2e_if_is_foreach_build() {
        // 构造 ClassExpression Value 绑定到 expression 形参
        let class_value = build_class_expression_value(
            "MyTests",
            &[
                ("Test1", "void", vec![attr("Fact")]),
                ("Test2", "void", vec![attr("Fact")]),
            ],
            vec![],
        );
        let mut env = Environment::new();
        env.bind("expression", class_value);

        // 构造 Build lambda: () => ""
        let lambda = LambdaExpr {
            params: vec![],
            body: LambdaBody::Expr(Box::new(Spanned::new(
                Expr::StringLit(String::new()),
                Span::DUMMY,
            ))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        };

        // 构造内层 stmt: this.Build(lambda)
        let build_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                method: Ident::from("Build"),
                args: vec![Spanned::new(Expr::Lambda(lambda), Span::DUMMY)],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );
        let build_stmt = Spanned::new(Stmt::Expr(build_call), Span::DUMMY);

        // 构造 foreach body block
        let foreach_body = Block {
            stmts: vec![build_stmt],
            tail: None,
        };

        // 构造 foreach iter: classDef.Methods
        let iter_expr = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("classDef")),
                    Span::DUMMY,
                )),
                field: Ident::from("Methods"),
            },
            Span::DUMMY,
        );

        // 构造 foreach (var method in classDef.Methods) { body }
        // parser 把 foreach 解析为 Stmt::For（与 for 共用变体）
        let foreach_stmt = Spanned::new(
            Stmt::For {
                var: Ident::from("method"),
                iter: iter_expr,
                body: foreach_body,
            },
            Span::DUMMY,
        );

        // 构造 then_branch block
        let then_block = Block {
            stmts: vec![foreach_stmt],
            tail: None,
        };

        // 构造 cond: expression is ClassExpression classDef
        let cond = Spanned::new(
            Expr::Is {
                expr: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("expression")),
                    Span::DUMMY,
                )),
                pattern: IsPattern::Type {
                    ty: Spanned::new(
                        Type::Named {
                            path: vec![Ident::from("ClassExpression")],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    binding: Some(Ident::from("classDef")),
                },
            },
            Span::DUMMY,
        );

        // 构造 if (cond) { then } —— parser 把 if 解析为 Expr::If 表达式
        // 作为 Stmt::Expr 包装
        let if_expr = Spanned::new(
            Expr::If {
                cond: Box::new(cond),
                then_branch: then_block,
                else_branch: None,
            },
            Span::DUMMY,
        );
        let if_stmt = Spanned::new(Stmt::Expr(if_expr), Span::DUMMY);

        // 构造 outer block
        let body = Block {
            stmts: vec![if_stmt],
            tail: None,
        };

        // 构造 slot: Build
        let slot = MacroSlot {
            method_name: Ident::from("Build"),
            param_types: vec![],
            return_type: Ident::from("void"),
            modifier: MethodModifier::None,
            is_async: false,
        };
        let slots = [slot];

        let interp = CtorInterpreter::new(&slots);
        let (regs, errs) = interp.interpret(&body, &env);

        // 验证：识别到 2 个注册（Test1 和 Test2 各触发一次 Build）
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(
            regs.len(),
            2,
            "expected 2 Build registrations (one per method)"
        );
        assert_eq!(regs[0].slot_name.as_str(), "Build");
        assert_eq!(regs[1].slot_name.as_str(), "Build");
    }

    /// 端到端测试：is 模式不匹配时跳过 then 分支。
    ///
    /// 构造 AST：`if (expression is ClassExpression classDef) { this.Build(...) }`
    /// 但 expression 绑定到 Value::Null（非 ClassExpression），验证：
    /// - if 条件求值为 Bool(false)
    /// - then 分支不执行，零注册
    /// - 无错误（条件不匹配不是错误）
    #[test]
    fn d10_6_is_pattern_not_match_skips_then() {
        let mut env = Environment::new();
        env.bind("expression", Value::Null);

        // 构造 Build lambda: () => ""
        let lambda = LambdaExpr {
            params: vec![],
            body: LambdaBody::Expr(Box::new(Spanned::new(
                Expr::StringLit(String::new()),
                Span::DUMMY,
            ))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        };

        // this.Build(lambda)
        let build_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                method: Ident::from("Build"),
                args: vec![Spanned::new(Expr::Lambda(lambda), Span::DUMMY)],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );
        let then_block = Block {
            stmts: vec![Spanned::new(Stmt::Expr(build_call), Span::DUMMY)],
            tail: None,
        };

        // cond: expression is ClassExpression classDef
        let cond = Spanned::new(
            Expr::Is {
                expr: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("expression")),
                    Span::DUMMY,
                )),
                pattern: IsPattern::Type {
                    ty: Spanned::new(
                        Type::Named {
                            path: vec![Ident::from("ClassExpression")],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    binding: Some(Ident::from("classDef")),
                },
            },
            Span::DUMMY,
        );

        let if_expr = Spanned::new(
            Expr::If {
                cond: Box::new(cond),
                then_branch: then_block,
                else_branch: None,
            },
            Span::DUMMY,
        );
        let body = Block {
            stmts: vec![Spanned::new(Stmt::Expr(if_expr), Span::DUMMY)],
            tail: None,
        };

        let slot = MacroSlot {
            method_name: Ident::from("Build"),
            param_types: vec![],
            return_type: Ident::from("void"),
            modifier: MethodModifier::None,
            is_async: false,
        };
        let slots = [slot];

        let interp = CtorInterpreter::new(&slots);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert!(
            regs.is_empty(),
            "expected 0 registrations when is pattern not matched"
        );
    }

    // ========================================================================
    // D-2 扩展能力测试（RFC 034: E0340/E0341 校验路径）
    // ========================================================================

    /// 测试 `parse_error_code_prefix` 函数——解析 `"<CODE>: <MESSAGE>"` 前缀。
    #[test]
    fn d2_parse_error_code_prefix() {
        // 标准格式
        assert_eq!(
            parse_error_code_prefix("E0340: [Fact] 方法必须无参数"),
            Some(("E0340", ": [Fact] 方法必须无参数"))
        );
        // 多位错误码
        assert_eq!(
            parse_error_code_prefix("W12345: warning msg"),
            Some(("W12345", ": warning msg"))
        );
        // 单字母前缀
        assert_eq!(
            parse_error_code_prefix("E0001: msg"),
            Some(("E0001", ": msg"))
        );
        // 无冒号 → None
        assert_eq!(parse_error_code_prefix("E0340 msg"), None);
        // 前缀无数字 → None
        assert_eq!(parse_error_code_prefix("E: msg"), None);
        // 前缀无字母 → None
        assert_eq!(parse_error_code_prefix("0340: msg"), None);
        // 小写字母 → None（要求大写）
        assert_eq!(parse_error_code_prefix("e0340: msg"), None);
        // 空字符串 → None
        assert_eq!(parse_error_code_prefix(""), None);
        // 数字后跟字母 → None
        assert_eq!(parse_error_code_prefix("E0340A: msg"), None);
    }

    /// 测试 `eval_binary_op` 函数——Int vs Int 数值比较。
    #[test]
    fn d2_eval_binary_op_int_comparison() {
        use ast::BinOp;
        assert!(matches!(
            eval_binary_op(BinOp::Gt, &Value::Int(5), &Value::Int(3)),
            Some(Value::Bool(true))
        ));
        assert!(matches!(
            eval_binary_op(BinOp::Gt, &Value::Int(3), &Value::Int(5)),
            Some(Value::Bool(false))
        ));
        assert!(matches!(
            eval_binary_op(BinOp::Lt, &Value::Int(3), &Value::Int(5)),
            Some(Value::Bool(true))
        ));
        assert!(matches!(
            eval_binary_op(BinOp::Eq, &Value::Int(0), &Value::Int(0)),
            Some(Value::Bool(true))
        ));
        assert!(matches!(
            eval_binary_op(BinOp::NotEq, &Value::Int(0), &Value::Int(1)),
            Some(Value::Bool(true))
        ));
        // D-4 扩展：支持整数加法（用于计数变量 `i = i + 1`）
        assert!(matches!(
            eval_binary_op(BinOp::Add, &Value::Int(1), &Value::Int(2)),
            Some(Value::Int(3))
        ));
        // 仍不支持 Sub/Mul/Div/Mod 等其他算术运算
        assert!(eval_binary_op(BinOp::Sub, &Value::Int(1), &Value::Int(2)).is_none());
    }

    /// 测试 `eval_binary_op` 函数——String vs String 相等性比较。
    #[test]
    fn d2_eval_binary_op_string_equality() {
        use ast::BinOp;
        assert!(matches!(
            eval_binary_op(
                BinOp::Eq,
                &Value::String("Fact".into()),
                &Value::String("Fact".into())
            ),
            Some(Value::Bool(true))
        ));
        assert!(matches!(
            eval_binary_op(
                BinOp::NotEq,
                &Value::String("Fact".into()),
                &Value::String("Theory".into())
            ),
            Some(Value::Bool(true))
        ));
        // 字符串不支持 > 比较
        assert!(eval_binary_op(
            BinOp::Gt,
            &Value::String("a".into()),
            &Value::String("b".into())
        )
        .is_none());
    }

    /// 测试 `eval_field_access`——`MethodExpression.Parameters` 与
    /// `MethodExpression.Attributes` 字段访问（D-2 扩展，D-3 升级）。
    ///
    /// D-3 升级：`Attributes` 现返回 `List<AttributeData>`（非 `List<string>`），
    /// 每个元素是 `Value::AttributeData`，可通过 `attr.Name`/`attr.Args` 访问。
    #[test]
    fn d2_method_field_access_parameters_attributes() {
        let method_val = Value::MethodExpression(MethodExpressionValue {
            name: "TestMethod".to_string(),
            parameters: vec![
                ("a".to_string(), "int".to_string()),
                ("b".to_string(), "string".to_string()),
            ],
            return_type: "void".to_string(),
            attributes: vec![
                AttributeDataValue {
                    name: "Fact".to_string(),
                    args: vec![],
                },
                AttributeDataValue {
                    name: "Category".to_string(),
                    args: vec![Value::String("slow".into())],
                },
            ],
        });
        let interp = CtorInterpreter::new(&[]);
        // Parameters → List<ParameterData>（D-4 升级），长度 2
        let params = interp.eval_field_access(&method_val, "Parameters");
        assert!(matches!(params, Some(Value::List(_))));
        if let Some(Value::List(buf)) = params {
            let list = buf.borrow();
            assert_eq!(list.len(), 2);
            // 第一个：name="a", ty="int"
            assert!(matches!(
                &list[0],
                Value::ParameterData(pd) if pd.name == "a" && pd.ty == "int"
            ));
            // 第二个：name="b", ty="string"
            assert!(matches!(
                &list[1],
                Value::ParameterData(pd) if pd.name == "b" && pd.ty == "string"
            ));
        }
        // Attributes → List<AttributeData>，长度 2（D-3 升级）
        let attrs = interp.eval_field_access(&method_val, "Attributes");
        assert!(matches!(attrs, Some(Value::List(_))));
        if let Some(Value::List(buf)) = attrs {
            let list = buf.borrow();
            assert_eq!(list.len(), 2);
            // 第一个：Fact 无参数
            assert!(
                matches!(&list[0], Value::AttributeData(ad) if ad.name == "Fact" && ad.args.is_empty())
            );
            // 第二个：Category 带 1 个字符串参数 "slow"
            if let Value::AttributeData(ad) = &list[1] {
                assert_eq!(ad.name, "Category");
                assert_eq!(ad.args.len(), 1);
                assert!(matches!(&ad.args[0], Value::String(s) if s == "slow"));
            } else {
                panic!("expected AttributeData for Category");
            }
        }
    }

    /// 测试 `eval_field_access`——`Value::List.Count` 字段访问（D-2 扩展）。
    #[test]
    fn d2_list_field_access_count() {
        let list_val = Value::List(std::rc::Rc::new(std::cell::RefCell::new(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ])));
        let interp = CtorInterpreter::new(&[]);
        let count = interp.eval_field_access(&list_val, "Count");
        assert!(matches!(count, Some(Value::Int(3))));
        // 不支持的字段
        assert!(interp.eval_field_access(&list_val, "Length").is_none());
    }

    /// 测试 `eval_method_call`——`Value::List.Contains(stringLit)` 方法调用（D-2 扩展）。
    #[test]
    fn d2_list_contains_method_call() {
        let list_val = Value::List(std::rc::Rc::new(std::cell::RefCell::new(vec![
            Value::String("Fact".into()),
            Value::String("Theory".into()),
        ])));
        let interp = CtorInterpreter::new(&[]);
        // Contains("Fact") → true
        let args = vec![Spanned::new(
            Expr::StringLit("Fact".to_string()),
            Span::DUMMY,
        )];
        let result = interp.eval_method_call(&list_val, "Contains", &args);
        assert!(matches!(result, Some(Value::Bool(true))));
        // Contains("Benchmark") → false
        let args = vec![Spanned::new(
            Expr::StringLit("Benchmark".to_string()),
            Span::DUMMY,
        )];
        let result = interp.eval_method_call(&list_val, "Contains", &args);
        assert!(matches!(result, Some(Value::Bool(false))));
        // 不支持的方法
        let result = interp.eval_method_call(&list_val, "Add", &args);
        assert!(result.is_none());
    }

    /// E0340 端到端测试——模拟 FactAttribute 构造函数体执行 E0340 校验。
    ///
    /// 构造 AST（对应 FactAttribute.as 构造函数体的核心逻辑）：
    /// ```
    /// if (expression is ClassExpression classDef) {
    ///     foreach (var method in classDef.Methods) {
    ///         if (method.Attributes.Contains("Fact")) {
    ///             if (method.Parameters.Count > 0) {
    ///                 throw new Error("E0340: [Fact] 方法必须无参数");
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    ///
    /// 输入 ClassExpression 含 1 个 [Fact] 方法（带 1 个参数），
    /// 验证：解释器产生 UserThrown 错误，错误码 "E0340"，消息 "[Fact] 方法必须无参数"。
    #[test]
    fn d2_e2e_e0340_throws_on_fact_method_with_params() {
        // 构造 ClassExpression Value：1 个 [Fact] 方法（带 1 个参数）
        let method_val = MethodExpressionValue {
            name: "BadTest".to_string(),
            parameters: vec![("x".to_string(), "int".to_string())], // 有参数
            return_type: "void".to_string(),
            attributes: vec![AttributeDataValue {
                name: "Fact".to_string(),
                args: vec![],
            }],
        };
        let class_value = Value::ClassExpression(ClassExpressionValue {
            class_name: "MyTests".to_string(),
            methods: vec![method_val],
            attributes: vec![],
        });
        let mut env = Environment::new();
        env.bind("expression", class_value);

        // 构造 throw new Error("E0340: [Fact] 方法必须无参数")
        let throw_stmt = Spanned::new(
            Stmt::Throw {
                expr: Spanned::new(
                    Expr::New {
                        ty: Spanned::new(
                            Type::Named {
                                path: vec![Ident::from("Error")],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        args: vec![Spanned::new(
                            Expr::StringLit("E0340: [Fact] 方法必须无参数".to_string()),
                            Span::DUMMY,
                        )],
                        obj_init: None,
                    },
                    Span::DUMMY,
                ),
            },
            Span::DUMMY,
        );

        // 构造 if (method.Parameters.Count > 0) { throw ... }
        let count_field = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(
                            Expr::Ident(Ident::from("method")),
                            Span::DUMMY,
                        )),
                        field: Ident::from("Parameters"),
                    },
                    Span::DUMMY,
                )),
                field: Ident::from("Count"),
            },
            Span::DUMMY,
        );
        let inner_if_cond = Spanned::new(
            Expr::Binary {
                op: ast::BinOp::Gt,
                left: Box::new(count_field),
                right: Box::new(Spanned::new(Expr::IntLit(0), Span::DUMMY)),
            },
            Span::DUMMY,
        );
        let inner_if = Spanned::new(
            Expr::If {
                cond: Box::new(inner_if_cond),
                then_branch: Block {
                    stmts: vec![throw_stmt],
                    tail: None,
                },
                else_branch: None,
            },
            Span::DUMMY,
        );

        // 构造 if (method.Attributes.Contains("Fact")) { if (...) }
        let attrs_field = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("method")),
                    Span::DUMMY,
                )),
                field: Ident::from("Attributes"),
            },
            Span::DUMMY,
        );
        let contains_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(attrs_field),
                method: Ident::from("Contains"),
                args: vec![Spanned::new(
                    Expr::StringLit("Fact".to_string()),
                    Span::DUMMY,
                )],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );
        let outer_if = Spanned::new(
            Expr::If {
                cond: Box::new(contains_call),
                then_branch: Block {
                    stmts: vec![Spanned::new(Stmt::Expr(inner_if), Span::DUMMY)],
                    tail: None,
                },
                else_branch: None,
            },
            Span::DUMMY,
        );

        // 构造 foreach (var method in classDef.Methods) { if (...) }
        let iter = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("classDef")),
                    Span::DUMMY,
                )),
                field: Ident::from("Methods"),
            },
            Span::DUMMY,
        );
        let foreach_stmt = Spanned::new(
            Stmt::For {
                var: Ident::from("method"),
                iter,
                body: Block {
                    stmts: vec![Spanned::new(Stmt::Expr(outer_if), Span::DUMMY)],
                    tail: None,
                },
            },
            Span::DUMMY,
        );

        // 构造 if (expression is ClassExpression classDef) { foreach (...) }
        let cond = Spanned::new(
            Expr::Is {
                expr: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("expression")),
                    Span::DUMMY,
                )),
                pattern: IsPattern::Type {
                    ty: Spanned::new(
                        Type::Named {
                            path: vec![Ident::from("ClassExpression")],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    binding: Some(Ident::from("classDef")),
                },
            },
            Span::DUMMY,
        );
        let top_if = Spanned::new(
            Expr::If {
                cond: Box::new(cond),
                then_branch: Block {
                    stmts: vec![foreach_stmt],
                    tail: None,
                },
                else_branch: None,
            },
            Span::DUMMY,
        );
        let body = Block {
            stmts: vec![Spanned::new(Stmt::Expr(top_if), Span::DUMMY)],
            tail: None,
        };

        let interp = CtorInterpreter::new(&[]); // 无 slot——本测试不验证 Build 注册
        let (regs, errs) = interp.interpret(&body, &env);

        // 验证：产生 1 个 UserThrown 错误，错误码 "E0340"
        assert!(
            regs.is_empty(),
            "expected no registrations, got: {:?}",
            regs
        );
        assert_eq!(
            errs.len(),
            1,
            "expected 1 UserThrown error, got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0340");
                assert_eq!(message, "[Fact] 方法必须无参数");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    /// E0340 通过场景测试——[Fact] 方法无参数时不抛错。
    ///
    /// 输入 ClassExpression 含 1 个 [Fact] 方法（无参数），
    /// 验证：解释器不产生错误，识别到 Build 注册。
    #[test]
    fn d2_e2e_e0340_passes_when_fact_method_no_params() {
        let method_val = MethodExpressionValue {
            name: "GoodTest".to_string(),
            parameters: vec![], // 无参数
            return_type: "void".to_string(),
            attributes: vec![AttributeDataValue {
                name: "Fact".to_string(),
                args: vec![],
            }],
        };
        let class_value = Value::ClassExpression(ClassExpressionValue {
            class_name: "MyTests".to_string(),
            methods: vec![method_val],
            attributes: vec![],
        });
        let mut env = Environment::new();
        env.bind("expression", class_value);

        // this.Build(() => "")
        let lambda = LambdaExpr {
            params: vec![],
            body: LambdaBody::Expr(Box::new(Spanned::new(
                Expr::StringLit(String::new()),
                Span::DUMMY,
            ))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        };
        let build_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(Spanned::new(Expr::This, Span::DUMMY)),
                method: Ident::from("Build"),
                args: vec![Spanned::new(Expr::Lambda(lambda), Span::DUMMY)],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );

        // throw new Error("E0340: ...")（不应触发）
        let throw_stmt = Spanned::new(
            Stmt::Throw {
                expr: Spanned::new(
                    Expr::New {
                        ty: Spanned::new(
                            Type::Named {
                                path: vec![Ident::from("Error")],
                                generics: vec![],
                            },
                            Span::DUMMY,
                        ),
                        args: vec![Spanned::new(
                            Expr::StringLit("E0340: should not trigger".to_string()),
                            Span::DUMMY,
                        )],
                        obj_init: None,
                    },
                    Span::DUMMY,
                ),
            },
            Span::DUMMY,
        );

        // if (method.Parameters.Count > 0) { throw ... } else { this.Build(...) }
        let count_field = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Field {
                        receiver: Box::new(Spanned::new(
                            Expr::Ident(Ident::from("method")),
                            Span::DUMMY,
                        )),
                        field: Ident::from("Parameters"),
                    },
                    Span::DUMMY,
                )),
                field: Ident::from("Count"),
            },
            Span::DUMMY,
        );
        let inner_if = Spanned::new(
            Expr::If {
                cond: Box::new(Spanned::new(
                    Expr::Binary {
                        op: ast::BinOp::Gt,
                        left: Box::new(count_field),
                        right: Box::new(Spanned::new(Expr::IntLit(0), Span::DUMMY)),
                    },
                    Span::DUMMY,
                )),
                then_branch: Block {
                    stmts: vec![throw_stmt],
                    tail: None,
                },
                else_branch: Some(Block {
                    stmts: vec![Spanned::new(Stmt::Expr(build_call), Span::DUMMY)],
                    tail: None,
                }),
            },
            Span::DUMMY,
        );

        // if (method.Attributes.Contains("Fact")) { if (...) }
        let attrs_field = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("method")),
                    Span::DUMMY,
                )),
                field: Ident::from("Attributes"),
            },
            Span::DUMMY,
        );
        let contains_call = Spanned::new(
            Expr::MethodCall {
                receiver: Box::new(attrs_field),
                method: Ident::from("Contains"),
                args: vec![Spanned::new(
                    Expr::StringLit("Fact".to_string()),
                    Span::DUMMY,
                )],
                type_args: vec![],
                params_span: None,
            },
            Span::DUMMY,
        );
        let outer_if = Spanned::new(
            Expr::If {
                cond: Box::new(contains_call),
                then_branch: Block {
                    stmts: vec![Spanned::new(Stmt::Expr(inner_if), Span::DUMMY)],
                    tail: None,
                },
                else_branch: None,
            },
            Span::DUMMY,
        );

        // foreach (var method in classDef.Methods) { if (...) }
        let iter = Spanned::new(
            Expr::Field {
                receiver: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("classDef")),
                    Span::DUMMY,
                )),
                field: Ident::from("Methods"),
            },
            Span::DUMMY,
        );
        let foreach_stmt = Spanned::new(
            Stmt::For {
                var: Ident::from("method"),
                iter,
                body: Block {
                    stmts: vec![Spanned::new(Stmt::Expr(outer_if), Span::DUMMY)],
                    tail: None,
                },
            },
            Span::DUMMY,
        );

        // if (expression is ClassExpression classDef) { foreach (...) }
        let cond = Spanned::new(
            Expr::Is {
                expr: Box::new(Spanned::new(
                    Expr::Ident(Ident::from("expression")),
                    Span::DUMMY,
                )),
                pattern: IsPattern::Type {
                    ty: Spanned::new(
                        Type::Named {
                            path: vec![Ident::from("ClassExpression")],
                            generics: vec![],
                        },
                        Span::DUMMY,
                    ),
                    binding: Some(Ident::from("classDef")),
                },
            },
            Span::DUMMY,
        );
        let top_if = Spanned::new(
            Expr::If {
                cond: Box::new(cond),
                then_branch: Block {
                    stmts: vec![foreach_stmt],
                    tail: None,
                },
                else_branch: None,
            },
            Span::DUMMY,
        );
        let body = Block {
            stmts: vec![Spanned::new(Stmt::Expr(top_if), Span::DUMMY)],
            tail: None,
        };

        let slot = MacroSlot {
            method_name: Ident::from("Build"),
            param_types: vec![],
            return_type: Ident::from("void"),
            modifier: MethodModifier::None,
            is_async: false,
        };
        let slots = [slot];

        let interp = CtorInterpreter::new(&slots);
        let (regs, errs) = interp.interpret(&body, &env);

        // 验证：无错误，识别到 1 个 Build 注册
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(
            regs.len(),
            1,
            "expected 1 Build registration, got: {:?}",
            regs
        );
        assert_eq!(regs[0].slot_name.as_str(), "Build");
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  D-3 测试：E0341 [Theory] 方法签名校验（三重检查 + throw 短路语义）
    // ═══════════════════════════════════════════════════════════════════════
    //
    // D-3 激活 E0341 三重校验（详 `std/QIF/Attributes/TheoryAttribute.as`）：
    //   1. [Theory] 方法必须有参数
    //   2. [Theory] 方法须配合至少一个 [InlineData]
    //   3. [Theory] 方法参数数量须与每个 [InlineData] 参数数量匹配
    //
    // 配合 throw 短路语义（halted 字段）——首个 throw 后续语句跳过，故每个
    // 测试仅期望 1 个 UserThrown 错误，不会因后续校验产生重复诊断。

    /// E0341 check 1：[Theory] 方法无参数 → 抛 E0341。
    ///
    /// 验证 `if (method.Parameters.Count == 0) { throw E0341 }` 路径。
    #[test]
    fn d3_e2e_e0341_throws_on_theory_method_no_params() {
        let class_val = class_expr_with_method(
            "MyTests",
            "BadTest",
            vec![], // 无参数
            vec![attr("Theory")],
        );
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let interp = CtorInterpreter::new(&[]);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(
            regs.is_empty(),
            "expected no registrations, got: {:?}",
            regs
        );
        assert_eq!(
            errs.len(),
            1,
            "expected 1 UserThrown error, got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0341");
                assert_eq!(message, "[Theory] 方法必须有参数");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    /// E0341 check 2：[Theory] 方法有参数但无 [InlineData] → 抛 E0341。
    ///
    /// 验证 `if (inlineDataAttrs.Count == 0) { throw E0341 }` 路径。
    #[test]
    fn d3_e2e_e0341_throws_on_theory_method_no_inline_data() {
        let class_val = class_expr_with_method(
            "MyTests",
            "Test1",
            vec![("a", "int")],   // 有参数
            vec![attr("Theory")], // 但无 [InlineData]
        );
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let interp = CtorInterpreter::new(&[]);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(
            regs.is_empty(),
            "expected no registrations, got: {:?}",
            regs
        );
        assert_eq!(
            errs.len(),
            1,
            "expected 1 UserThrown error, got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0341");
                assert_eq!(message, "[Theory] 方法须配合至少一个 [InlineData]");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    /// E0341 check 3：[Theory] 方法参数数量与 [InlineData] 参数数量不匹配 → 抛 E0341。
    ///
    /// 方法有 2 个参数，[InlineData(1)] 仅 1 个参数 → 不匹配。
    /// 验证 `foreach (var attr in inlineDataAttrs) { if (count != count) { throw } }` 路径。
    #[test]
    fn d3_e2e_e0341_throws_on_arg_count_mismatch() {
        let class_val = class_expr_with_method(
            "MyTests",
            "Test1",
            vec![("a", "int"), ("b", "int")], // 2 个参数
            vec![
                attr("Theory"),
                attr_with_args("InlineData", vec![Value::Int(1)]), // 仅 1 个参数
            ],
        );
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let interp = CtorInterpreter::new(&[]);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(
            regs.is_empty(),
            "expected no registrations, got: {:?}",
            regs
        );
        assert_eq!(
            errs.len(),
            1,
            "expected 1 UserThrown error, got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0341");
                assert_eq!(message, "[Theory] 方法参数数量须与 [InlineData] 匹配");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    /// E0341 全部通过：[Theory] 方法有 1 个 int 参数 + [InlineData(1)] 1 个 int 参数
    /// → 不抛错，识别到 Build 注册。
    ///
    /// 此测试同时验证 D-3 短路语义未误触发——throw 路径未执行时 halted 保持 false，
    /// 正常流程继续到 `this.Build(...)` 注册调用。
    #[test]
    fn d3_e2e_e0341_passes_when_matches() {
        let class_val = class_expr_with_method(
            "MyTests",
            "GoodTest",
            vec![("a", "int")], // 1 个 int 参数
            vec![
                attr("Theory"),
                attr_with_args("InlineData", vec![Value::Int(1)]), // 1 个 int 参数
            ],
        );
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let slot = MacroSlot {
            method_name: Ident::from("Build"),
            param_types: vec![],
            return_type: Ident::from("void"),
            modifier: MethodModifier::None,
            is_async: false,
        };
        let slots = [slot];
        let interp = CtorInterpreter::new(&slots);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(
            regs.len(),
            1,
            "expected 1 Build registration, got: {:?}",
            regs
        );
        assert_eq!(regs[0].slot_name.as_str(), "Build");
    }

    /// D-3 短路语义专项测试——首个 throw 后同 foreach 迭代内后续 throw 不触发。
    ///
    /// 构造含 2 个 [Theory] 方法的 ClassExpression，第一个方法触发 E0341（无参数），
    /// 第二个方法若被迭代也会触发 E0341（无 InlineData）。D-3 短路使首个 throw 后
    /// halted 置位，foreach 跳过后续迭代——验证仅产生 1 个错误（非 2 个）。
    #[test]
    fn d3_short_circuit_halts_foreach_after_first_throw() {
        let method1 = MethodExpressionValue {
            name: "BadTest1".to_string(),
            parameters: vec![], // 触发 E0341 check 1
            return_type: "void".to_string(),
            attributes: vec![attr("Theory")],
        };
        let method2 = MethodExpressionValue {
            name: "BadTest2".to_string(),
            parameters: vec![("x".to_string(), "int".to_string())], // 触发 E0341 check 2
            return_type: "void".to_string(),
            attributes: vec![attr("Theory")], // 无 [InlineData]
        };
        let class_val = Value::ClassExpression(ClassExpressionValue {
            class_name: "MyTests".to_string(),
            methods: vec![method1, method2],
            attributes: vec![],
        });
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let interp = CtorInterpreter::new(&[]);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(regs.is_empty());
        // D-3 短路：仅 1 个错误（首个 throw 后 halted=true，foreach 跳过 method2）
        assert_eq!(
            errs.len(),
            1,
            "expected 1 error (short-circuit), got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0341");
                assert_eq!(message, "[Theory] 方法必须有参数");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    //  D-4 测试：ParameterData/TypeName/Index/Add 单元测试 + E0342 类型匹配校验
    // ═══════════════════════════════════════════════════════════════════════
    //
    // D-4 在 D-3 基础上扩展 D10.6 解释器以支持 E0342 类型匹配校验：
    //   - Value::ParameterData 新变体（携带 name+ty）
    //   - method.Parameters 升级为 List<ParameterData>（携带 Name+Type）
    //   - Value.TypeName 字段访问（返回规范化类型名字符串）
    //   - Expr::Index 索引访问（list[i]）
    //   - BinOp::Add 整数加法（用于计数变量 i = i + 1）

    /// D-4 单元测试：ParameterData.Name/Type 字段访问。
    ///
    /// 验证 `eval_field_access` 对 `Value::ParameterData` 的 "Name"/"Type" 字段
    /// 返回正确的 `Value::String`。
    #[test]
    fn d4_parameter_data_field_access_name_type() {
        let pd_val = Value::ParameterData(ParameterDataValue {
            name: "x".to_string(),
            ty: "int".to_string(),
        });
        let interp = CtorInterpreter::new(&[]);

        // Name 字段
        let name_val = interp.eval_field_access(&pd_val, "Name");
        assert!(matches!(
            name_val,
            Some(Value::String(s)) if s == "x"
        ));

        // Type 字段
        let type_val = interp.eval_field_access(&pd_val, "Type");
        assert!(matches!(
            type_val,
            Some(Value::String(s)) if s == "int"
        ));

        // 未知字段返回 None
        assert!(interp.eval_field_access(&pd_val, "Unknown").is_none());
    }

    /// D-4 单元测试：Value.TypeName 字段访问——返回规范化类型名。
    ///
    /// 验证 `eval_field_access` 对任意 Value 的 "TypeName" 字段返回
    /// `Value::String`，内容为规范化类型名（int/string/bool/null/...）。
    #[test]
    fn d4_value_type_name_field_access() {
        let interp = CtorInterpreter::new(&[]);

        // Int → "int"
        assert!(matches!(
            interp.eval_field_access(&Value::Int(42), "TypeName"),
            Some(Value::String(s)) if s == "int"
        ));

        // String → "string"
        assert!(matches!(
            interp.eval_field_access(&Value::String("hi".into()), "TypeName"),
            Some(Value::String(s)) if s == "string"
        ));

        // Bool → "bool"
        assert!(matches!(
            interp.eval_field_access(&Value::Bool(true), "TypeName"),
            Some(Value::String(s)) if s == "bool"
        ));

        // Null → "null"
        assert!(matches!(
            interp.eval_field_access(&Value::Null, "TypeName"),
            Some(Value::String(s)) if s == "null"
        ));
    }

    /// D-4 单元测试：Expr::Index 索引访问——`list[i]` 端到端通过 interpret 验证。
    ///
    /// 构造最小 AST：`if (expression is ClassExpression classDef) { ... }`
    /// 内部对 `method.Parameters[0]` 索引访问并验证 Name 字段。由于 Index
    /// 在 E0342 校验中通过 `method.Parameters[i]` 使用，此处通过 E0342
    /// e2e 测试间接覆盖——本测试改为单元级直接验证 `eval_expr` 对
    /// `Expr::Index` 的求值。
    #[test]
    fn d4_list_index_access_via_eval_expr() {
        // 构造 List Value: [Int(10), Int(20), Int(30)]
        let list_val = Value::List(std::rc::Rc::new(std::cell::RefCell::new(vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
        ])));

        let mut env = Environment::new();
        env.bind("my_list", list_val);
        let mut interp = CtorInterpreter::new(&[]);

        // my_list[0] → Int(10)
        let index_expr = Expr::Index {
            receiver: Box::new(ident_e("my_list")),
            index: Box::new(int_e(0)),
        };
        let result = interp.eval_expr(&index_expr, &mut env);
        assert!(matches!(result, Some(Value::Int(10))));

        // my_list[2] → Int(30)
        let index_expr = Expr::Index {
            receiver: Box::new(ident_e("my_list")),
            index: Box::new(int_e(2)),
        };
        let result = interp.eval_expr(&index_expr, &mut env);
        assert!(matches!(result, Some(Value::Int(30))));

        // my_list[5] 越界 → None
        let index_expr = Expr::Index {
            receiver: Box::new(ident_e("my_list")),
            index: Box::new(int_e(5)),
        };
        let result = interp.eval_expr(&index_expr, &mut env);
        assert!(result.is_none(), "expected None for out-of-bounds index");
    }

    /// D-4 单元测试：BinOp::Add 整数加法（在 d2_eval_binary_op_int_comparison
    /// 中已基础覆盖，此处补充更多边界值验证）。
    #[test]
    fn d4_binop_add_integer_addition() {
        use ast::BinOp;
        // 基础加法
        assert!(matches!(
            eval_binary_op(BinOp::Add, &Value::Int(0), &Value::Int(0)),
            Some(Value::Int(0))
        ));
        // 计数器典型场景：i + 1
        assert!(matches!(
            eval_binary_op(BinOp::Add, &Value::Int(0), &Value::Int(1)),
            Some(Value::Int(1))
        ));
        // 多次累加后的状态
        assert!(matches!(
            eval_binary_op(BinOp::Add, &Value::Int(5), &Value::Int(1)),
            Some(Value::Int(6))
        ));
        // 负数
        assert!(matches!(
            eval_binary_op(BinOp::Add, &Value::Int(-3), &Value::Int(1)),
            Some(Value::Int(-2))
        ));
        // String + String 不支持 Add（仅支持 == / !=）
        assert!(eval_binary_op(
            BinOp::Add,
            &Value::String("a".into()),
            &Value::String("b".into())
        )
        .is_none());
    }

    /// D-4 E2E 测试：E0342 [InlineData] 参数类型与方法形参不匹配 → 抛 E0342。
    ///
    /// 方法形参类型为 `string`，[InlineData(1)] 参数为 `int` →
    /// `arg.TypeName ("int") != param.Type ("string")` → 抛 E0342。
    ///
    /// 验证 E0342 校验路径：
    ///   - foreach (var attr in inlineDataAttrs) 遍历
    ///   - var i = 0; 计数器声明
    ///   - foreach (var arg in attr.Args) 内层遍历
    ///   - var param = method.Parameters[i]; 索引访问
    ///   - if (arg.TypeName != param.Type) { throw E0342 } 类型比较
    ///   - i = i + 1; 计数器递增（BinOp::Add）
    #[test]
    fn d4_e2e_e0342_throws_on_type_mismatch() {
        let class_val = class_expr_with_method(
            "MyTests",
            "BadTest",
            vec![("a", "string")], // 方法形参类型 string
            vec![
                attr("Theory"),
                attr_with_args("InlineData", vec![Value::Int(1)]), // 参数类型 int
            ],
        );
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let interp = CtorInterpreter::new(&[]);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(
            regs.is_empty(),
            "expected no registrations, got: {:?}",
            regs
        );
        assert_eq!(
            errs.len(),
            1,
            "expected 1 UserThrown error, got: {:?}",
            errs
        );
        match &errs[0] {
            CtorInterpError::UserThrown { code, message, .. } => {
                assert_eq!(code, "E0342");
                assert_eq!(message, "[InlineData] 参数类型与方法形参不匹配");
            }
            other => panic!("expected UserThrown, got: {:?}", other),
        }
    }

    /// D-4 E2E 测试：E0342 通过场景——[InlineData] 参数类型与方法形参类型匹配
    /// （多个参数、多个 [InlineData]）→ 不抛 E0342，识别到 Build 注册。
    ///
    /// 方法 `(int a, string b)` + 两个 [InlineData]：
    ///   - [InlineData(1, "hello")]：int + string → 匹配
    ///   - [InlineData(2, "world")]：int + string → 匹配
    ///
    /// 验证 E0342 校验内层 foreach 多次迭代 + 计数器 `i = i + 1` 多次累加
    /// 正确工作（不越界、不误判类型）。
    #[test]
    fn d4_e2e_e0342_passes_when_types_match_multiple_inline_data() {
        let method_val = MethodExpressionValue {
            name: "GoodTest".to_string(),
            parameters: vec![
                ("a".to_string(), "int".to_string()),
                ("b".to_string(), "string".to_string()),
            ],
            return_type: "void".to_string(),
            attributes: vec![
                attr("Theory"),
                attr_with_args(
                    "InlineData",
                    vec![Value::Int(1), Value::String("hello".into())],
                ),
                attr_with_args(
                    "InlineData",
                    vec![Value::Int(2), Value::String("world".into())],
                ),
            ],
        };
        let class_val = Value::ClassExpression(ClassExpressionValue {
            class_name: "MyTests".to_string(),
            methods: vec![method_val],
            attributes: vec![],
        });
        let mut env = Environment::new();
        env.bind("expression", class_val);

        let body = build_theory_attribute_body();
        let slot = MacroSlot {
            method_name: Ident::from("Build"),
            param_types: vec![],
            return_type: Ident::from("void"),
            modifier: MethodModifier::None,
            is_async: false,
        };
        let slots = [slot];
        let interp = CtorInterpreter::new(&slots);
        let (regs, errs) = interp.interpret(&body, &env);

        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(
            regs.len(),
            1,
            "expected 1 Build registration, got: {:?}",
            regs
        );
        assert_eq!(regs[0].slot_name.as_str(), "Build");
    }
}
