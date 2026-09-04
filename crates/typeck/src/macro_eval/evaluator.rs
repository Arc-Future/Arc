//! RFC 009 M4-4: 受限求值器核心。
//!
//! 执行 `Func<string>` 委托体，返回展开代码字符串。求值器是受限子集
//! 解释器（RFC 009 D10.2），仅支持白名单内的构造。
//!
//! # 设计要点
//!
//! - **执行时机**：typeck 完成后、MIR lowering 之前。求值器在
//!   `MacroRegistration.expansion`（一个 `LambdaExpr`）上工作，输入是
//!   AST 节点，输出是字符串。
//! - **受限子集**：字面量、局部变量、`StringBuilder` 系列、`if/else`、
//!   `return`、`+` 字符串拼接、`new StringBuilder()`。其他构造一律报错。
//! - **诊断锚点**：错误 span 指向委托体内具体节点，便于定位
//!   （D10.4 要求「展开代码错误指向委托位置」）。
//! - **无副作用**：求值器进程内无 IO、无外部状态访问；唯一可变状态是
//!   局部变量与 `StringBuilder` 实例的累积字符串。
//!
//! # 与 RFC 009 D10 章节对应
//!
//! - D10.1 核心机制 → [`Evaluator::eval_lambda`]
//! - D10.2 受限子集边界 → [`Evaluator::eval_stmt`] / [`Evaluator::eval_expr`]
//!   中的分支选择与禁用构造报错
//! - D10.4 诊断锚点 → [`EvalError`] 各变体携带 span 字段
//! - D10.5 Expression 访问器 → M4-5 扩展（暂占位为 `Unsupported`）

use ast::{Block, Expr, FloatLitValue, Ident, LambdaBody, LambdaExpr, Span, Spanned, Stmt, Type};
use hir::DefId;
use std::cell::RefCell;
use std::rc::Rc;

use super::whitelist::Whitelist;

/// 求值过程中可能产生的错误。
///
/// 所有变体携带 `span` 字段，指向委托体内具体出错节点
/// （RFC 009 D10.4 诊断锚点要求）。
#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    /// 命中禁用构造（`while`/`for`/`try`/`throw`/`break`/`using`/lambda 创建）。
    ForbiddenConstruct { construct: &'static str, span: Span },
    /// 调用了白名单外的方法。
    NotInWhitelist {
        receiver_ty: String,
        method: Ident,
        span: Span,
    },
    /// `new T()` 中 T 不在白名单 `newable` 集合中。
    NotNewable { type_name: String, span: Span },
    /// 未定义的变量名。
    UndefinedName { name: Ident, span: Span },
    /// 类型不匹配（如对 `Int` 调用 `StringBuilder.Append`）。
    ValueTypeMismatch {
        expected: &'static str,
        found: &'static str,
        span: Span,
    },
    /// Lambda 体未返回 `string`（如返回 `int` 或无 `return` 的 block）。
    ReturnTypeMismatch { found: &'static str, span: Span },
    /// 方法参数数量错误。
    ArgCount {
        method: Ident,
        expected: usize,
        found: usize,
        span: Span,
    },
    /// 不支持的 AST 节点（白名单内但未实现，如 M4-5 待补的 Expression 访问器）。
    Unsupported { node: &'static str, span: Span },
}

impl EvalError {
    /// 转换为 `TypeError` 加入 typeck 错误流。
    ///
    /// TODO（M4-8）：分配 `arc-macro-XXX` 错误码并迁移到独立诊断模块。
    pub fn to_type_error(&self) -> crate::error::TypeError {
        use crate::error::TypeError;
        match self {
            EvalError::ForbiddenConstruct { construct, .. } => {
                TypeError::Generic(format!("受限求值器禁用构造: {construct}"))
            }
            EvalError::NotInWhitelist {
                receiver_ty,
                method,
                ..
            } => TypeError::Generic(format!("受限求值器白名单外调用: {receiver_ty}.{method}")),
            EvalError::NotNewable { type_name, .. } => TypeError::Generic(format!(
                "受限求值器禁止 new {type_name}()（仅 StringBuilder 允许）"
            )),
            EvalError::UndefinedName { name, .. } => TypeError::Undefined(name.to_string()),
            EvalError::ValueTypeMismatch {
                expected, found, ..
            } => TypeError::Mismatch {
                expected: expected.to_string(),
                found: found.to_string(),
            },
            EvalError::ReturnTypeMismatch { found, .. } => TypeError::Mismatch {
                expected: "string (Func<string> 委托返回类型)".to_string(),
                found: found.to_string(),
            },
            EvalError::ArgCount {
                method,
                expected,
                found,
                ..
            } => TypeError::Generic(format!("{method}: 参数数量期望 {expected}，实际 {found}")),
            EvalError::Unsupported { node, .. } => {
                TypeError::Generic(format!("受限求值器暂不支持节点: {node}"))
            }
        }
    }
}

/// 求值器中的运行时值。
#[derive(Clone, Debug)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    Null,
    /// `StringBuilder` 实例——存储累积的字符串内容。
    ///
    /// 使用 `Rc<RefCell<String>>` 共享可变状态：当 `sb.Append("a").Append("b")`
    /// 链式调用时，外层 `.Append("b")` 持有的是与 `sb` 局部变量**同一**缓冲区
    /// 的引用，mutation 即时可见，无需复杂写回逻辑。
    /// 这是受限求值器进程内的唯一可变状态。
    StringBuilder(Rc<RefCell<String>>),
    /// RFC 028 M5-3: `List<string>` 实例——存储生成的源代码字符串列表。
    ///
    /// Source Generator 的 `Generate(GeneratorContext) -> List<string>` 方法
    /// 在受限求值器内求值时，使用此变体累积生成的源文件字符串。
    /// `Rc<RefCell<Vec<Value>>>` 同 StringBuilder 一样支持链式 `Add` 调用。
    /// 仅支持 `List<string>`（元素必须为 string），其他泛型实参被拒绝。
    List(Rc<RefCell<Vec<Value>>>),
    /// RFC 028 M4-5/M4-7: Expression 对象——编译期求值器视角下的值。
    ///
    /// 实际 Expression 对象来自 attribute 参数解析（如 `[Inject(x => x + 1)]`
    /// 中的 `x => x + 1` 被 typeck 树化为 `Expression<Func<int,int>>`）。
    ///
    /// M4-5 阶段：仅持有 `type_name` 与 `props`，支持有限的属性访问
    /// （`NodeType` / `TypeName`）与虚方法调用（`GetStringValue` / `GetMember` 等）。
    ///
    /// M4-7 阶段：`node` 携带完整 `ExpressionNode` IR 树，使
    /// `GetLeft` / `GetRight` / `GetOperand` 等子节点访问器返回真实
    /// 子 `Value::Expression` 而非 `Value::Null`。`node=None` 表示该
    /// Expression 是测试构造的占位值（仅含 props）。
    Expression {
        /// 节点具体类名（如 "ConstantExpression" / "MemberExpression"）。
        type_name: String,
        /// 属性名 → 字符串值（如 "NodeType" → "Constant", "StringValue" → "hello"）。
        props: indexmap::IndexMap<String, String>,
        /// RFC 028 M4-7: 完整 ExpressionNode IR 树（可选）。
        ///
        /// 由 `expression_tree_to_value` / `expression_node_to_value` 在
        /// 注入 locals 时填充。`None` 表示此值仅含 props（测试场景或
        /// M4-5 占位），子节点访问器将返回 `Value::Null`。
        node: Option<Box<ast::ExpressionNode>>,
    },
    /// RFC 028 M5-2b: `GeneratorContext` 实例——编译期可访问的全量信息。
    ///
    /// Source Generator 的 `Generate(GeneratorContext context)` 方法在
    /// 受限求值器中求值时，`context` 形参被绑定为此变体（由调用方
    /// 通过 [`Evaluator::with_locals`] 或
    /// [`Evaluator::eval_generate_method_with_context`] 注入）。
    ///
    /// [`eval_field`](Evaluator::eval_field) 拦截 `context.Attributes` /
    /// `context.Symbols` / `context.SourceFiles` 字段访问返回对应内部值。
    /// 内部三个字段均为 `Rc` 共享——求值器进程内不可变，纯查询。
    GeneratorContext {
        /// 全量属性表（共享 typeck 产物 `TypeChecker.attribute_table`）。
        attributes: Rc<crate::AttributeTable>,
        /// 符号表快照：`DefId → (类型名, 成员名)`（由调用方从
        /// `class_def_ids` 与 `def_id_members` 合并构造）。
        /// `GetTypeName` 返回元组第 0 项，`GetMemberName` 返回第 1 项。
        symbols: Rc<indexmap::IndexMap<DefId, (String, String)>>,
        /// 当前编译单元的源文件路径列表。
        source_files: Rc<Vec<String>>,
        /// Phase 2 序列化体系：类型表快照——`DefId → TypeTableEntry`。
        /// Source Generator 通过此表查询类型的字段名和类型，
        /// 用于生成编译期序列化代码。
        type_table: Rc<indexmap::IndexMap<DefId, TypeTableEntry>>,
    },
    /// RFC 028 M5-2b: `AttributeTable` 占位值——通过 `context.Attributes` 注入。
    ///
    /// 求值器拦截 `table.Count`（field 访问）与 `table.GetDefIdAt(i)` /
    /// `table.GetAttrs(defId)`（method 调用）返回真实数据。
    /// `Rc<crate::AttributeTable>` 共享 typeck 产物（不可变，纯查询）。
    AttributeTable(Rc<crate::AttributeTable>),
    /// RFC 028 M5-2b: `AttributeList` 占位值——`table.GetAttrs(defId)` 返回。
    ///
    /// 求值器拦截 `list.Has(name)` 方法调用并返回真实判断结果。
    /// 内部存储该 DefId 对应的 `Vec<ResolvedAttribute>`（clone 自
    /// [`AttributeTable::get_attrs`](crate::AttributeTable::get_attrs)）。
    AttributeList(Rc<Vec<crate::ResolvedAttribute>>),
    /// RFC 028 M5-2b: `SymbolTable` 占位值——通过 `context.Symbols` 注入。
    ///
    /// 求值器拦截 `table.GetTypeName(defId)` / `table.GetMemberName(defId)`
    /// 方法调用并返回真实符号名。内部含 `DefId → (类型名, 成员名)` 映射，
    /// 由调用方从 `TypeChecker.def_id_members` 构造（仅含方法成员；类/字段
    /// 成员的成员名为空串）。未命中返回空串（与 `std/Arc/CodeGeneration/Generators.as`
    /// 占位实现一致）。
    ///
    /// RFC 028 M5-2b（GetMemberName 扩展）：QifRegistryGenerator 等生成器
    /// 需要构造 `<Class::Method>` 符号引用，必须能反查方法名。`GetTypeName`
    /// 返回元组第 0 项，`GetMemberName` 返回第 1 项。
    SymbolTable(Rc<indexmap::IndexMap<DefId, (String, String)>>),
    /// RFC 028 M4 D10.6: `ClassExpression` 实例——构造函数体编译期解释器
    /// （RFC 034 QIF 路径扩展）的核心值类型之一。
    ///
    /// 当 typeck 识别到「被赋能类标注了派生属性」（如 `[Fact] class MyTests`），
    /// 在调用 D10.6 解释器之前，typeck 构造此 Value 并绑定到派生属性构造函数
    /// 的 `Expression expression` 形参。D10.6 解释器执行 `expression is
    /// ClassExpression classDef` 模式匹配时按此变体分派。
    ///
    /// 内部字段对齐 RFC 022 §2.2.9 `ClassExpression` Arc 类设计——`ClassName`
    /// /`Methods`/`Attributes` 三字段。`Methods` 是 `MethodExpressionValue`
    /// 列表，`Attributes` 是属性名字符串列表（不携带属性实例，避免反射）。
    ///
    /// 仅在编译期 typeck 内部使用，**不**进入 codegen 发射路径——D10.6
    /// 解释器在 Pass 2 识别 Build 调用后即丢弃此 Value，Pass 3 由 D10.2
    /// 受限求值器执行 Build 委托体生成展开字符串。
    ClassExpression(ClassExpressionValue),
    /// RFC 028 M4 D10.6: `MethodExpression` 实例——`ClassExpressionValue.methods`
    /// 列表元素类型。
    ///
    /// D10.6 解释器执行 `foreach (var method in classDef.Methods)` 时遍历
    /// `ClassExpressionValue.methods` Vec，每个元素是此 Value。`method.Name`
    /// /`method.Attributes` 等字段访问按此变体分派。
    MethodExpression(MethodExpressionValue),
    /// RFC 034 M2 D-3: `AttributeData` 实例——attribute 名 + 参数列表。
    ///
    /// `MethodExpressionValue.attributes` / `ClassExpressionValue.attributes`
    /// 元素类型。D10.6 解释器执行 `method.Attributes` 返回 `Value::List`，
    /// 每个元素是此 Value。`attr.Name`/`attr.Args` 字段访问按此变体分派。
    ///
    /// `args` 仅含位置参数（`[InlineData(1, 2)]` 中的 `1`/`2`），命名参数
    /// （`[InlineData(data: 1)]`）暂不支持——M2 范围仅需参数数量校验，
    /// 命名参数留待后续阶段。
    AttributeData(AttributeDataValue),
    /// RFC 034 M2 D-4: `ParameterData` 实例——方法形参名 + 类型名。
    ///
    /// `MethodExpressionValue.parameters` 元素类型。D10.6 解释器执行
    /// `method.Parameters` 返回 `Value::List`，每个元素是此 Value。
    /// `param.Name`/`param.Type` 字段访问按此变体分派。
    ///
    /// 与 `AttributeDataValue` 对称设计——同为「携带元数据的 Value 载体」。
    /// `ty` 取 `ast::MethodSig.params[i].ty` 的字符串形式（如 `"int"`/
    /// `"string"`/`"bool"`），不携带类型解析后的 `TypeId`——避免 D10.6
    /// 解释器依赖 typeck 类型表，保持解释器零类型系统知识。
    ParameterData(ParameterDataValue),
    /// Phase 2 序列化体系：TypeTable 占位值——通过 `context.TypeTable` 注入。
    ///
    /// Source Generator 通过 TypeTable 查询类型的成员元数据（字段/属性名与类型），
    /// 用于生成编译期序列化代码。求值器拦截 `GetTypeName` / `GetKind` /
    /// `GetFieldCount` / `GetFieldName` / `GetFieldType` / `GetBaseType`
    /// 方法调用并返回真实数据。
    /// `Rc<IndexMap<DefId, TypeTableEntry>>` 由 checker 从 `TypeRegistry` 构建。
    TypeTable(Rc<indexmap::IndexMap<DefId, TypeTableEntry>>),
}

/// RFC 028 M4 D10.6: `ClassExpression` Value 内部数据（RFC 034 QIF 路径扩展）。
///
/// 对齐 RFC 022 §2.2.9 `ClassExpression` Arc 类字段——`ClassName`/`Methods`/
/// `Attributes` 三字段。由 typeck 在调用 D10.6 解释器之前从 `class_defs`
/// 中提取构造（详见 `crates/typeck/src/macro_eval/ctor_interpreter.rs`
/// `build_class_expression_value`）。
#[derive(Clone, Debug)]
pub struct ClassExpressionValue {
    /// 类名（不含命名空间前缀，对齐 `ClassExpression.ClassName`）。
    pub class_name: String,
    /// 类中定义的方法签名列表（不含方法体，对齐 `ClassExpression.Methods`）。
    pub methods: Vec<MethodExpressionValue>,
    /// 类上声明的属性列表（RFC 034 M2 D-3：从 `Vec<String>` 升级为
    /// `Vec<AttributeDataValue>`，携带 attribute 参数数据，对齐
    /// `ClassExpression.Attributes`）。
    pub attributes: Vec<AttributeDataValue>,
}

/// RFC 028 M4 D10.6: `MethodExpression` Value 内部数据（RFC 034 QIF 路径扩展）。
///
/// 对齐 RFC 022 §2.2.9 `MethodExpression` Arc 类字段——`Name`/`Parameters`/
/// `ReturnType`/`Attributes` 四字段。仅含方法签名，**不含方法体**（方法体
/// 可能含循环/递归等不可树化语句，符合 RFC 022 §2.6 约束）。
#[derive(Clone, Debug)]
pub struct MethodExpressionValue {
    /// 方法名（对齐 `MethodExpression.Name`）。
    pub name: String,
    /// 方法形参列表 `(name, type)`（对齐 `MethodExpression.Parameters`，
    /// 简化为 `(String, String)` 元组避免反射体系）。
    pub parameters: Vec<(String, String)>,
    /// 方法返回类型名（字符串形式，如 `"void"`/`"int"`/`"string"`，对齐
    /// `MethodExpression.ReturnType`）。
    pub return_type: String,
    /// 方法上声明的属性列表（RFC 034 M2 D-3：从 `Vec<String>` 升级为
    /// `Vec<AttributeDataValue>`，携带 attribute 参数数据，对齐
    /// `MethodExpression.Attributes`）。
    pub attributes: Vec<AttributeDataValue>,
}

/// RFC 034 M2 D-3: `AttributeData` Value 内部数据——attribute 名 + 位置参数列表。
///
/// 由 typeck 在 `build_class_expression_value_for` 中从 `ast::Attribute`
/// 构造：`path.last()` 作为 `name`，`args` 中**位置参数**（`AttributeArg::String`
/// /`Int`/`Bool`）转为 `Value` 装入 `args`。命名参数与 `Type`/`MemberPath`/
/// `Lambda` 变体暂不支持——M2 范围仅需参数数量校验。
///
/// D10.6 解释器执行 `attr.Name` / `attr.Args` 字段访问按此结构字段分派。
#[derive(Clone, Debug)]
pub struct AttributeDataValue {
    /// Attribute 名（取 `ast::Attribute.path` 末段，如 `"Fact"`/`"InlineData"`）。
    pub name: String,
    /// 位置参数列表（按声明顺序，仅 `String`/`Int`/`Bool` 字面量）。
    pub args: Vec<Value>,
}

/// RFC 034 M2 D-4: `ParameterData` Value 内部数据——方法形参名 + 类型名。
///
/// 由 typeck 在 `build_class_expression_value_for` 中从 `ast::MethodSig`
/// 构造：`params[i].name` 作为 `name`，`params[i].ty` 的字符串形式作为 `ty`。
/// D10.6 解释器执行 `method.Parameters` 返回 `Value::List`，每个元素是
/// `Value::ParameterData(ParameterDataValue)`。
///
/// 与 `AttributeDataValue` 对称设计——同为「携带元数据的 Value 载体」。
/// D10.6 解释器执行 `param.Name` / `param.Type` 字段访问按此结构字段分派。
///
/// `ty` 取类型名字符串（如 `"int"`/`"string"`/`"bool"`），不携带类型解析后
/// 的 `TypeId`——避免 D10.6 解释器依赖 typeck 类型表，保持解释器零类型系统
/// 知识。类型等价性判断（如 `int` vs `i32`）归 Arc 侧派生类。
#[derive(Clone, Debug)]
pub struct ParameterDataValue {
    /// 形参名（对齐 `ParameterExpression.Name`）。
    pub name: String,
    /// 形参类型名（字符串形式，如 `"int"`/`"string"`/`"bool"`）。
    pub ty: String,
}

/// Phase 2 序列化体系：TypeTable 条目——单个类型的编译期元数据快照。
///
/// 由 checker 在 `expand_source_generators` 中从 `TypeRegistry` 提取构建。
/// 每个条目对应一个 `NominalType`，包含类型名、种类、字段列表和基类型。
///
/// 此结构仅含 Source Generator 所需的元数据子集（字段名+类型+基类），
/// 不携带方法列表或完整泛型参数——避免求值器依赖过重。
#[derive(Clone, Debug)]
pub struct TypeTableEntry {
    /// 类型名（如 "User"、"Person"）。
    pub type_name: String,
    /// 类型种类："class"、"struct"、"interface"、"enum"。
    pub kind: String,
    /// 字段名列表（按声明顺序，与 field_types 一一对应）。
    pub field_names: Vec<String>,
    /// 字段类型名列表（字符串形式，如 "string"、"int"、"List<Item>"）。
    pub field_types: Vec<String>,
    /// 枚举成员名列表（仅 kind == "enum" 时有值；否则为空）。
    pub enum_member_names: Vec<String>,
    /// 基类名（class 有值，struct/interface/enum 为空串）。
    pub base_type: String,
}

impl Value {
    /// 返回值的类型名（用于诊断与白名单查询）。
    ///
    /// `Value::Expression` 返回 `"Expression"`——所有派生类（ConstantExpression
    /// / MemberExpression 等）在白名单中统一按基类 `Expression` 注册。
    ///
    /// RFC 028 M5-2b: `GeneratorContext` / `AttributeTable` / `AttributeList` /
    /// `SymbolTable` 各返回自身类型名，与 `std/Arc/CodeGeneration/Generators.as`
    /// 中类名对齐，供白名单查询与诊断输出。
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "string",
            Value::Int(_) => "int",
            Value::Bool(_) => "bool",
            Value::Null => "null",
            Value::StringBuilder(_) => "StringBuilder",
            Value::List(_) => "List",
            Value::Expression { .. } => "Expression",
            Value::GeneratorContext { .. } => "GeneratorContext",
            Value::AttributeTable(_) => "AttributeTable",
            Value::AttributeList(_) => "AttributeList",
            Value::SymbolTable(_) => "SymbolTable",
            Value::ClassExpression(_) => "ClassExpression",
            Value::MethodExpression(_) => "MethodExpression",
            Value::AttributeData(_) => "AttributeData",
            Value::ParameterData(_) => "ParameterData",
            Value::TypeTable(_) => "TypeTable",
        }
    }

    /// 转换为字符串值（用于 `sb.Append(x)` 参数与 `+` 拼接）。
    ///
    /// - `String` / `Int` / `Bool` / `Null` → 字符串形式
    /// - `StringBuilder` / `List` / `Expression` / `GeneratorContext` /
    ///   `AttributeTable` / `AttributeList` / `SymbolTable` → 报错
    ///   （不允许隐式 ToString，必须显式调用）
    pub fn coerce_to_string(&self) -> Result<String, EvalError> {
        match self {
            Value::String(s) => Ok(s.clone()),
            Value::Int(i) => Ok(i.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Null => Ok("null".to_string()),
            Value::StringBuilder(_)
            | Value::List(_)
            | Value::Expression { .. }
            | Value::GeneratorContext { .. }
            | Value::AttributeTable(_)
            | Value::AttributeList(_)
            | Value::SymbolTable(_)
            | Value::ClassExpression(_)
            | Value::MethodExpression(_)
            | Value::AttributeData(_)
            | Value::ParameterData(_)
            | Value::TypeTable(_) => Err(EvalError::ValueTypeMismatch {
                expected: "string-coercible (string/int/bool/null)",
                found: match self {
                    Value::StringBuilder(_) => "StringBuilder",
                    Value::List(_) => "List",
                    Value::Expression { .. } => "Expression",
                    Value::GeneratorContext { .. } => "GeneratorContext",
                    Value::AttributeTable(_) => "AttributeTable",
                    Value::AttributeList(_) => "AttributeList",
                    Value::SymbolTable(_) => "SymbolTable",
                    Value::ClassExpression(_) => "ClassExpression",
                    Value::MethodExpression(_) => "MethodExpression",
                    Value::AttributeData(_) => "AttributeData",
                    Value::ParameterData(_) => "ParameterData",
                    Value::TypeTable(_) => "TypeTable",
                    _ => unreachable!(),
                },
                span: Span::DUMMY,
            }),
        }
    }
}

/// 控制流（用于 `return` 语句处理）。
enum ControlFlow {
    /// 正常 fallthrough（值若为表达式语句则为 unit，丢弃）。
    Normal(Option<Value>),
    /// 命中 `return` 语句——短路外层求值。
    Return(Value),
}

/// 受限求值器——执行 `Func<string>` 委托体。
///
/// 一个 `Evaluator` 实例对应一次委托求值；不可跨委托复用（局部变量
/// 上下文不共享）。
pub struct Evaluator<'a> {
    /// 局部变量绑定（按声明顺序）。
    locals: indexmap::IndexMap<Ident, Value>,
    /// 白名单引用。
    whitelist: &'a Whitelist,
}

impl<'a> Evaluator<'a> {
    pub fn new(whitelist: &'a Whitelist) -> Self {
        Self {
            locals: indexmap::IndexMap::new(),
            whitelist,
        }
    }

    /// RFC 028 M4-5: 用预填充的局部变量构造求值器。
    ///
    /// 用于注入 ctor 参数（如 `Expression expression`）到求值器环境。
    /// 实际注入逻辑在 M4-7（两轮 typeck）中由 typeck 调用方填充——
    /// 从 attribute 参数解析出 Expression 对象后传入。
    ///
    /// M4-5 测试用此方法直接注入 Expression 值验证字段访问。
    pub fn with_locals(whitelist: &'a Whitelist, locals: indexmap::IndexMap<Ident, Value>) -> Self {
        Self { locals, whitelist }
    }

    /// RFC 009 M4-7: 把 `(形参名, ExpressionTree)` 绑定列表注入 locals。
    ///
    /// Pass 3 (`expand_feature_registrations_with_locals`) 生成带
    /// `expression_locals` 的 `MacroRegistration`；`expand_macros` 在
    /// 求值每个委托之前调用此方法，把 Expression 形参对应的
    /// `ExpressionTree` 转换为 `Value::Expression` 注入求值器环境，
    /// 使委托体内对形参名的引用能解析到完整 Expression 对象。
    ///
    /// 转换由 [`expression_tree_to_value`] 完成——保留 ExpressionTree
    /// 的完整 IR 子树（`node` 字段），供子节点访问器（`GetLeft` 等）遍历。
    pub fn inject_expression_locals(&mut self, locals: &[(Ident, ast::ExpressionTree)]) {
        for (name, tree) in locals {
            self.locals
                .insert(name.clone(), expression_tree_to_value(tree));
        }
    }

    /// 求值一个 `Func<string>` 委托，返回展开代码字符串。
    ///
    /// 入口点（RFC 009 D10.1）。委托必须返回 `string`——若 body 末尾
    /// 表达式或 `return` 语句返回非 `string`，报 `ReturnTypeMismatch`。
    pub fn eval_lambda(&mut self, lambda: &LambdaExpr) -> Result<String, EvalError> {
        let result = match &lambda.body {
            LambdaBody::Expr(expr) => {
                let v = self.eval_expr(expr)?;
                ControlFlow::Normal(Some(v))
            }
            LambdaBody::Block(block) => self.eval_block(block)?,
        };
        let v = match result {
            ControlFlow::Normal(Some(v)) => v,
            ControlFlow::Normal(None) => {
                // Block 无 return 也无 tail——视为返回 null，触发 ReturnTypeMismatch
                return Err(EvalError::ReturnTypeMismatch {
                    found: "void",
                    span: Span::DUMMY,
                });
            }
            ControlFlow::Return(v) => v,
        };
        match v {
            Value::String(s) => Ok(s),
            _ => Err(EvalError::ReturnTypeMismatch {
                found: v.type_name(),
                span: Span::DUMMY,
            }),
        }
    }

    /// RFC 009 M5-3: 求值 Source Generator 的 `Generate(GeneratorContext)`
    /// 方法体，返回 `List<string>` 内的字符串列表。
    ///
    /// 与 [`eval_lambda`](Self::eval_lambda) 的差异：返回类型为 `List<string>`
    /// 而非 `string`。求值器将 `List<string>.Add(s)` 视作白名单内方法，
    /// 把生成的源代码字符串累积到 `Value::List` 缓冲区；最后从 List
    /// 中提取所有 String 元素作为 `Vec<String>` 返回。
    ///
    /// 复用受限求值器的子集规则（D10.2）与白名单（D13.6 共享），新增
    /// `List<string>` 构造与 `Add` 方法到白名单（D13.6 扩展）。
    ///
    /// # 错误
    ///
    /// - 方法体未返回 List 或返回了非 List 值 → `ReturnTypeMismatch`
    /// - List 中包含非 String 元素 → `ValueTypeMismatch`
    pub fn eval_generate_method(&mut self, body: &Block) -> Result<Vec<String>, EvalError> {
        let result = self.eval_block(body)?;
        let v = match result {
            ControlFlow::Normal(Some(v)) => v,
            ControlFlow::Normal(None) => {
                return Err(EvalError::ReturnTypeMismatch {
                    found: "void",
                    span: Span::DUMMY,
                });
            }
            ControlFlow::Return(v) => v,
        };
        match v {
            Value::List(rc_list) => {
                let list = rc_list.borrow();
                let mut out = Vec::with_capacity(list.len());
                for item in list.iter() {
                    match item {
                        Value::String(s) => out.push(s.clone()),
                        other => {
                            return Err(EvalError::ValueTypeMismatch {
                                expected: "string (List<string> 元素)",
                                found: other.type_name(),
                                span: Span::DUMMY,
                            });
                        }
                    }
                }
                Ok(out)
            }
            other => Err(EvalError::ReturnTypeMismatch {
                found: other.type_name(),
                span: Span::DUMMY,
            }),
        }
    }

    /// RFC 009 M5-2b: 求值 Source Generator 的 `Generate(GeneratorContext)`
    /// 方法体，返回 `List<string>` 内的字符串列表，并通过 `context_param_name`
    /// 把 `GeneratorContext` 值注入求值器环境。
    ///
    /// 与 [`eval_generate_method`](Self::eval_generate_method) 的差异：调用方
    /// 显式传入 `GeneratorContext` 值与其在 Generate 方法签名中的形参名。
    /// 形参名取自 Generate 方法签名（由 `collect_generate_method` 提取，
    /// 存入 `SourceGenerator.context_param_name`）。
    ///
    /// 若 `context_param_name` 或 `context` 为 `None`，退化为
    /// [`eval_generate_method`](Self::eval_generate_method) 行为
    /// （不注入 context，向后兼容 M5-3 既有调用路径）。
    ///
    /// # 错误
    ///
    /// - `context` 不是 `Value::GeneratorContext` 变体 → 求值体内访问
    ///   `context.Attributes` 等字段时由 `eval_field` 报 `Unsupported`
    /// - 方法体求值失败 → 透传 `EvalError`
    pub fn eval_generate_method_with_context(
        &mut self,
        body: &Block,
        context_param_name: Option<Ident>,
        context: Option<Value>,
    ) -> Result<Vec<String>, EvalError> {
        if let (Some(name), Some(ctx)) = (context_param_name, context) {
            self.locals.insert(name, ctx);
        }
        self.eval_generate_method(body)
    }

    fn eval_block(&mut self, block: &Block) -> Result<ControlFlow, EvalError> {
        for stmt in &block.stmts {
            match self.eval_stmt(stmt)? {
                ControlFlow::Normal(_) => {}
                cf @ ControlFlow::Return(_) => return Ok(cf),
            }
        }
        if let Some(tail) = &block.tail {
            let v = self.eval_expr(tail)?;
            return Ok(ControlFlow::Normal(Some(v)));
        }
        Ok(ControlFlow::Normal(None))
    }

    fn eval_stmt(&mut self, stmt: &Spanned<Stmt>) -> Result<ControlFlow, EvalError> {
        match &stmt.node {
            Stmt::Let { name, init, .. } => {
                if let Some(init_expr) = init {
                    let v = self.eval_expr(init_expr)?;
                    self.locals.insert(name.clone(), v);
                } else {
                    // `let x: T;` 无 init——插入 null 占位（受限子集场景罕见）
                    self.locals.insert(name.clone(), Value::Null);
                }
                Ok(ControlFlow::Normal(None))
            }
            Stmt::Assign { target, value, .. } => {
                let v = self.eval_expr(value)?;
                // 仅支持简单标识符赋值（包括 sb.X=... 等字段赋值不允许）
                match &target.node {
                    Expr::Ident(name) => {
                        if !self.locals.contains_key(name) {
                            return Err(EvalError::UndefinedName {
                                name: name.clone(),
                                span: target.span,
                            });
                        }
                        // `insert` 覆盖现有键值（保留插入顺序），等价于
                        // `self.locals[name] = v`。
                        self.locals.insert(name.clone(), v);
                        Ok(ControlFlow::Normal(None))
                    }
                    _ => Err(EvalError::Unsupported {
                        node: "complex assignment target",
                        span: stmt.span,
                    }),
                }
            }
            Stmt::Return(opt) => {
                let v = match opt {
                    Some(e) => self.eval_expr(e)?,
                    None => Value::Null,
                };
                Ok(ControlFlow::Return(v))
            }
            Stmt::Expr(e) => {
                self.eval_expr(e)?;
                Ok(ControlFlow::Normal(None))
            }
            // 禁用构造
            Stmt::While { .. } | Stmt::For { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "loop (while/for)",
                span: stmt.span,
            }),
            Stmt::Throw { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "throw",
                span: stmt.span,
            }),
            Stmt::TryCatch { .. } | Stmt::TryFinally { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "try/catch/finally",
                span: stmt.span,
            }),
            Stmt::Using { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "using",
                span: stmt.span,
            }),
            Stmt::UsingVar { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "using var",
                span: stmt.span,
            }),
            Stmt::AwaitUsing { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "await using",
                span: stmt.span,
            }),
            Stmt::AwaitUsingVar { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "await using var",
                span: stmt.span,
            }),
            Stmt::YieldReturn { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "yield return",
                span: stmt.span,
            }),
            Stmt::YieldBreak => Err(EvalError::ForbiddenConstruct {
                construct: "yield break",
                span: stmt.span,
            }),
            Stmt::Lock { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "lock",
                span: stmt.span,
            }),
            Stmt::ForC { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "c-style for",
                span: stmt.span,
            }),
            Stmt::Break => Err(EvalError::ForbiddenConstruct {
                construct: "break",
                span: stmt.span,
            }),
            Stmt::Continue => Err(EvalError::ForbiddenConstruct {
                construct: "continue",
                span: stmt.span,
            }),
            Stmt::DeconstructAssign { .. } => Err(EvalError::Unsupported {
                node: "deconstruct assignment",
                span: stmt.span,
            }),
        }
    }

    fn eval_expr(&mut self, expr: &Spanned<Expr>) -> Result<Value, EvalError> {
        match &expr.node {
            // 字面量
            Expr::StringLit(s) => Ok(Value::String(s.clone())),
            Expr::IntLit(i) => Ok(Value::Int(*i)),
            Expr::BoolLit(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            // 浮点与字符字面量——evaluator 暂不支持参与运算，仅可作 Append 参数
            Expr::FloatLit(FloatLitValue::Float(f)) => Ok(Value::String(f.to_string())),
            Expr::FloatLit(FloatLitValue::Double(f)) => Ok(Value::String(f.to_string())),
            Expr::CharLit(c) => Ok(Value::String(c.to_string())),
            // 标识符
            Expr::Ident(name) => {
                self.locals
                    .get(name)
                    .cloned()
                    .ok_or_else(|| EvalError::UndefinedName {
                        name: name.clone(),
                        span: expr.span,
                    })
            }
            // 嵌套块——不创建新作用域（受限子集内局部变量在委托体顶层定义即可）
            Expr::Block(b) => match self.eval_block(b)? {
                ControlFlow::Normal(Some(v)) => Ok(v),
                ControlFlow::Normal(None) => Ok(Value::Null),
                ControlFlow::Return(v) => Ok(v),
            },
            // 条件分支
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.eval_expr(cond)?;
                let cond_bool = match c {
                    Value::Bool(b) => b,
                    _ => {
                        return Err(EvalError::ValueTypeMismatch {
                            expected: "bool",
                            found: c.type_name(),
                            span: cond.span,
                        });
                    }
                };
                let block = if cond_bool {
                    then_branch
                } else {
                    else_branch.as_ref().ok_or(EvalError::Unsupported {
                        node: "if without else",
                        span: expr.span,
                    })?
                };
                match self.eval_block(block)? {
                    ControlFlow::Normal(Some(v)) => Ok(v),
                    ControlFlow::Normal(None) => Ok(Value::Null),
                    ControlFlow::Return(v) => Ok(v),
                }
            }
            // new T(...)
            Expr::New { ty, args, .. } => self.eval_new(ty, args, expr.span),
            // 方法调用
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => self.eval_method_call(receiver, method, args, expr.span),
            // 字符串拼接
            Expr::Binary { op, left, right } => self.eval_binary(op, left, right, expr.span),
            // RFC 012：comptime 表达式在 macro evaluator 中按其内部表达式求值
            // （宏体运行时求值语义；comptime 折叠由主 typeck 路径承担）。
            Expr::Comptime(inner) => self.eval_expr(inner),
            // 禁用与不支持
            Expr::Lambda(_) => Err(EvalError::ForbiddenConstruct {
                construct: "lambda creation (only direct to Register is allowed)",
                span: expr.span,
            }),
            Expr::Await(_) => Err(EvalError::ForbiddenConstruct {
                construct: "await",
                span: expr.span,
            }),
            Expr::Query(_) => Err(EvalError::ForbiddenConstruct {
                construct: "LINQ query",
                span: expr.span,
            }),
            Expr::Switch(_) => Err(EvalError::ForbiddenConstruct {
                construct: "switch",
                span: expr.span,
            }),
            Expr::SwitchForm(_) => Err(EvalError::ForbiddenConstruct {
                construct: "switch expression",
                span: expr.span,
            }),
            Expr::Coalesce { .. }
            | Expr::NullCond { .. }
            | Expr::ForceDeref { .. }
            | Expr::Ternary { .. } => Err(EvalError::Unsupported {
                node: "null-flow operator",
                span: expr.span,
            }),
            Expr::Cast { .. } | Expr::Default { .. } | Expr::TypeOf(_) => {
                Err(EvalError::Unsupported {
                    node: "cast/default/typeof",
                    span: expr.span,
                })
            }
            Expr::Box { .. } | Expr::Unbox { .. } => Err(EvalError::ForbiddenConstruct {
                construct: "FFI box/unbox",
                span: expr.span,
            }),
            Expr::RefArg { .. } => Err(EvalError::Unsupported {
                node: "ref/out arg",
                span: expr.span,
            }),
            Expr::NamedArg { .. } => Err(EvalError::Unsupported {
                node: "named argument",
                span: expr.span,
            }),
            Expr::StackSpanLit { .. } => Err(EvalError::Unsupported {
                node: "params stack span",
                span: expr.span,
            }),
            Expr::CollectionExpr { .. } => Err(EvalError::Unsupported {
                node: "collection expression",
                span: expr.span,
            }),
            // RFC 009 M4-5: 字段访问——仅 Expression 值的属性访问允许。
            // 其他 receiver（struct 实例等）暂不支持。
            Expr::Field { receiver, field } => self.eval_field(receiver, field, expr.span),
            Expr::Index { .. }
            | Expr::Call { .. }
            | Expr::ExpressionLit(_)
            | Expr::Unary { .. } => Err(EvalError::Unsupported {
                node: "index/call/expression-lit/unary",
                span: expr.span,
            }),
            // RFC 009 M4-5: 路径访问——用于 ExpressionType.Constant 等枚举常量。
            // 当前返回字符串形式（路径末段），不解析为具体枚举值。
            Expr::Path(path) => {
                if path.len() == 2 && path[0].as_str() == "ExpressionType" {
                    // ExpressionType.Constant / ExpressionType.Member / ...
                    Ok(Value::String(path[1].to_string()))
                } else {
                    Err(EvalError::Unsupported {
                        node: "path (only ExpressionType.X allowed)",
                        span: expr.span,
                    })
                }
            }
            Expr::This | Expr::Base => Err(EvalError::ForbiddenConstruct {
                construct: "this/base in restricted evaluator",
                span: expr.span,
            }),
            // `expr is pattern` — 受限求值器不支持 is 表达式（RFC 036 M1）。
            Expr::Is { .. } => Err(EvalError::Unsupported {
                node: "is expression",
                span: expr.span,
            }),
            // RFC 006 M2：`with` 不在宏受限求值器内。
            Expr::With { .. } => Err(EvalError::Unsupported {
                node: "with expression",
                span: expr.span,
            }),
            // 赋值表达式不在宏受限求值器子集内（宏展开期无写入语义）。
            Expr::Assign { .. } => Err(EvalError::Unsupported {
                node: "assignment expression",
                span: expr.span,
            }),
            // RFC 007：受限求值器内插值 → 拼接（洞须已求值为可转 string 的值）
            // M2a：对齐/格式说明符在宏求值器内硬拒绝（须走普通 typeck 脱糖路径）。
            Expr::InterpolatedString { parts } => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        ast::InterpPart::Lit(s) => out.push_str(s),
                        ast::InterpPart::Expr(hole) => {
                            if hole.alignment.is_some() || hole.format.is_some() {
                                return Err(EvalError::Unsupported {
                                    node: "interpolation format/align in macro evaluator (RFC 007 M2a)",
                                    span: hole.expr.span,
                                });
                            }
                            let v = self.eval_expr(&hole.expr)?;
                            match v {
                                Value::String(s) => out.push_str(&s),
                                Value::Int(i) => out.push_str(&i.to_string()),
                                Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
                                Value::Null => out.push_str("null"),
                                other => {
                                    return Err(EvalError::ValueTypeMismatch {
                                        expected: "stringifiable value in interpolation",
                                        found: other.type_name(),
                                        span: hole.expr.span,
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(Value::String(out))
            }
            // `new T[n]` 数组分配在宏求值器受限子集内不支持。
            Expr::NewArray { length, .. } => Err(EvalError::Unsupported {
                node: "new T[n] array allocation in macro evaluator",
                span: length.span,
            }),
        }
    }

    /// RFC 009 M4-5: 求值 `receiver.field` 形式的字段访问。
    ///
    /// 支持的 receiver 类型：
    /// - `Value::Expression`：查询 `props` 映射返回字符串值（M4-5）
    /// - `Value::GeneratorContext`：拦截 `Attributes` / `Symbols` /
    ///   `SourceFiles` property 访问返回对应内部值（M5-2b）
    /// - `Value::AttributeTable`：拦截 `Count` property 访问返回符号数（M5-2b）
    ///
    /// `Value::StringBuilder` 不允许字段访问（必须通过 ToString 获取内容）。
    /// 其他值类型报错。
    fn eval_field(
        &mut self,
        receiver: &Spanned<Expr>,
        field: &Ident,
        span: Span,
    ) -> Result<Value, EvalError> {
        let recv = self.eval_expr(receiver)?;
        match recv {
            Value::Expression { props, .. } => {
                if let Some(v) = props.get(field.as_str()) {
                    Ok(Value::String(v.clone()))
                } else {
                    Err(EvalError::Unsupported {
                        node: "unknown Expression property (not in props map)",
                        span,
                    })
                }
            }
            // RFC 009 M5-2b: GeneratorContext.{Attributes, Symbols, SourceFiles, TypeTable}
            Value::GeneratorContext {
                attributes,
                symbols,
                source_files,
                type_table,
            } => {
                match field.as_str() {
                    "Attributes" => Ok(Value::AttributeTable(attributes)),
                    "Symbols" => Ok(Value::SymbolTable(symbols)),
                    "SourceFiles" => {
                        // List<string> 内部表示是 Rc<RefCell<Vec<Value>>>，
                        // 把 Rc<Vec<String>> 转为 List<Value::String>。
                        let list: Vec<Value> = source_files
                            .iter()
                            .map(|s| Value::String(s.clone()))
                            .collect();
                        Ok(Value::List(Rc::new(RefCell::new(list))))
                    }
                    "TypeTable" => Ok(Value::TypeTable(type_table)),
                    _ => Err(EvalError::Unsupported {
                        node: "unknown GeneratorContext property (allowed: Attributes/Symbols/SourceFiles/TypeTable)",
                        span,
                    }),
                }
            }
            // RFC 009 M5-2b: AttributeTable.Count（property → field 访问）
            Value::AttributeTable(table) => match field.as_str() {
                "Count" => Ok(Value::Int(table.iter().count() as i64)),
                _ => Err(EvalError::Unsupported {
                    node: "unknown AttributeTable property (allowed: Count)",
                    span,
                }),
            },
            _ => Err(EvalError::Unsupported {
                node: "field access on non-Expression/GeneratorContext/AttributeTable value",
                span,
            }),
        }
    }

    fn eval_new(
        &mut self,
        ty: &Spanned<Type>,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        let type_name = extract_type_name(&ty.node).ok_or(EvalError::Unsupported {
            node: "complex type in new",
            span: ty.span,
        })?;
        if !self.whitelist.allows_new(&type_name) {
            return Err(EvalError::NotNewable { type_name, span });
        }
        match type_name.as_str() {
            "StringBuilder" => {
                if !args.is_empty() {
                    return Err(EvalError::ArgCount {
                        method: Ident::from("StringBuilder.ctor"),
                        expected: 0,
                        found: args.len(),
                        span,
                    });
                }
                Ok(Value::StringBuilder(Rc::new(RefCell::new(String::new()))))
            }
            // RFC 009 M5-3: `new List<string>()` —— 仅支持单泛型实参 string
            "List" => {
                // 取出 generics 引用（避免 move）——已通过 extract_type_name 确认是 Named
                let generics = match &ty.node {
                    Type::Named { generics, .. } => generics,
                    _ => unreachable!(),
                };
                if generics.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: Ident::from("List.ctor"),
                        expected: 1,
                        found: generics.len(),
                        span: ty.span,
                    });
                }
                // 校验唯一泛型实参为 string
                let elem_ok = match &generics[0].node {
                    Type::Named { path, generics: g } if g.is_empty() => {
                        path.last().map(|n| n.as_str() == "string").unwrap_or(false)
                    }
                    _ => false,
                };
                if !elem_ok {
                    return Err(EvalError::NotNewable {
                        type_name: "List<非 string> (受限求值器仅支持 List<string>)".to_string(),
                        span,
                    });
                }
                if !args.is_empty() {
                    return Err(EvalError::ArgCount {
                        method: Ident::from("List.ctor"),
                        expected: 0,
                        found: args.len(),
                        span,
                    });
                }
                Ok(Value::List(Rc::new(RefCell::new(Vec::new()))))
            }
            _ => Err(EvalError::NotNewable { type_name, span }),
        }
    }

    fn eval_method_call(
        &mut self,
        receiver: &Spanned<Expr>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        // 先求值 receiver——若为 local var ident，求值后修改需要写回
        let recv = self.eval_expr(receiver)?;
        let receiver_ty = recv.type_name().to_string();

        // 白名单查询（早期拒绝）
        if !self.whitelist.allows(&receiver_ty, method.as_str()) {
            return Err(EvalError::NotInWhitelist {
                receiver_ty,
                method: method.clone(),
                span,
            });
        }

        // 由于 `Value::StringBuilder` 使用 `Rc<RefCell<String>>` 共享缓冲区，
        // `recv` 已经是 `sb` 局部变量的同一缓冲区的引用（clone Rc 即可），
        // mutation 即时可见，无需在调用后写回 locals。
        match recv {
            Value::StringBuilder(content) => match method.as_str() {
                "Append" => {
                    if args.len() != 1 {
                        return Err(EvalError::ArgCount {
                            method: method.clone(),
                            expected: 1,
                            found: args.len(),
                            span,
                        });
                    }
                    let arg = self.eval_expr(&args[0])?;
                    let s = arg.coerce_to_string().map_err(|mut e| {
                        if let EvalError::ValueTypeMismatch { span: s, .. } = &mut e {
                            *s = span;
                        }
                        e
                    })?;
                    content.borrow_mut().push_str(&s);
                    // 返回同一 StringBuilder 实例以支持链式调用
                    Ok(Value::StringBuilder(content))
                }
                "AppendLine" => {
                    if args.is_empty() {
                        content.borrow_mut().push('\n');
                        Ok(Value::StringBuilder(content))
                    } else if args.len() == 1 {
                        let arg = self.eval_expr(&args[0])?;
                        let s = arg.coerce_to_string().map_err(|mut e| {
                            if let EvalError::ValueTypeMismatch { span: s, .. } = &mut e {
                                *s = span;
                            }
                            e
                        })?;
                        let mut buf = content.borrow_mut();
                        buf.push_str(&s);
                        buf.push('\n');
                        drop(buf);
                        Ok(Value::StringBuilder(content))
                    } else {
                        Err(EvalError::ArgCount {
                            method: method.clone(),
                            expected: 1,
                            found: args.len(),
                            span,
                        })
                    }
                }
                "ToString" => {
                    if !args.is_empty() {
                        return Err(EvalError::ArgCount {
                            method: method.clone(),
                            expected: 0,
                            found: args.len(),
                            span,
                        });
                    }
                    Ok(Value::String(content.borrow().clone()))
                }
                "Clear" => {
                    if !args.is_empty() {
                        return Err(EvalError::ArgCount {
                            method: method.clone(),
                            expected: 0,
                            found: args.len(),
                            span,
                        });
                    }
                    content.borrow_mut().clear();
                    Ok(Value::StringBuilder(content))
                }
                _ => Err(EvalError::NotInWhitelist {
                    receiver_ty: "StringBuilder".to_string(),
                    method: method.clone(),
                    span,
                }),
            },
            // RFC 009 M5-3: List<string> 实例方法
            Value::List(buf) => match method.as_str() {
                "Add" => {
                    if args.len() != 1 {
                        return Err(EvalError::ArgCount {
                            method: method.clone(),
                            expected: 1,
                            found: args.len(),
                            span,
                        });
                    }
                    let arg = self.eval_expr(&args[0])?;
                    // List<string> 仅接受 string 元素——其他类型拒绝
                    let s = match arg {
                        Value::String(s) => s,
                        other => {
                            return Err(EvalError::ValueTypeMismatch {
                                expected: "string (List<string>.Add 参数)",
                                found: other.type_name(),
                                span,
                            });
                        }
                    };
                    buf.borrow_mut().push(Value::String(s));
                    // 返回同一 List 实例以支持链式调用
                    Ok(Value::List(buf))
                }
                _ => Err(EvalError::NotInWhitelist {
                    receiver_ty: "List".to_string(),
                    method: method.clone(),
                    span,
                }),
            },
            // RFC 009 M4-5/M4-7: Expression 类层次访问器
            Value::Expression { type_name, props, node } => {
                self.eval_expression_method(&type_name, &props, &node, method, args, span)
            }
            // RFC 009 M5-2b: AttributeTable 方法（GetDefIdAt / GetAttrs）
            Value::AttributeTable(table) => {
                self.eval_attribute_table_method(&table, method, args, span)
            }
            // RFC 012 M5-2b: AttributeList 方法（Has）
            Value::AttributeList(attrs) => {
                self.eval_attribute_list_method(&attrs, method, args, span)
            }
            // RFC 012 M5-2b: SymbolTable 方法（GetTypeName）
            Value::SymbolTable(symbols) => {
                self.eval_symbol_table_method(&symbols, method, args, span)
            }
            // Phase 2 序列化体系：TypeTable 方法
            Value::TypeTable(table) => {
                self.eval_type_table_method(&table, method, args, span)
            }
            // 其他类型的白名单方法暂未实现
            _ => Err(EvalError::Unsupported {
                node: "method call on non-StringBuilder/List/Expression/AttributeTable/AttributeList/SymbolTable/TypeTable",
                span,
            }),
        }
    }

    /// RFC 009 M5-2b: 求值 `AttributeTable` 实例方法。
    ///
    /// 拦截 `GetDefIdAt(int index) -> int` 与 `GetAttrs(int defId) -> AttributeList`
    /// 调用并返回真实数据。其他方法报 `NotInWhitelist`。
    ///
    /// `GetDefIdAt` 按插入顺序返回第 `index` 个符号的 DefId（与
    /// [`AttributeTable::iter`] 顺序一致）；越界报 `Unsupported`。
    /// `GetAttrs` 返回 `Value::AttributeList`，内部 clone 该 DefId 的
    /// `Vec<ResolvedAttribute>`（共享不可变）。
    fn eval_attribute_table_method(
        &mut self,
        table: &Rc<crate::AttributeTable>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        match method.as_str() {
            "GetDefIdAt" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let idx = self.eval_int_arg(&args[0])?;
                if idx < 0 {
                    return Err(EvalError::Unsupported {
                        node: "AttributeTable.GetDefIdAt negative index",
                        span,
                    });
                }
                let def_id = table.iter().nth(idx as usize).map(|(d, _)| d).ok_or(
                    EvalError::Unsupported {
                        node: "AttributeTable.GetDefIdAt index out of range",
                        span,
                    },
                )?;
                Ok(Value::Int(def_id.0 as i64))
            }
            "GetAttrs" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::AttributeList(Rc::new(Vec::new())));
                }
                let attrs: Vec<crate::ResolvedAttribute> =
                    table.get_attrs(DefId(def_id_val as u32)).to_vec();
                Ok(Value::AttributeList(Rc::new(attrs)))
            }
            _ => Err(EvalError::NotInWhitelist {
                receiver_ty: "AttributeTable".to_string(),
                method: method.clone(),
                span,
            }),
        }
    }

    /// RFC 012 M5-2b: 求值 `AttributeList` 实例方法。
    ///
    /// 拦截 `Has(string name) -> bool` 调用并返回真实判断结果——遍历
    /// `Vec<ResolvedAttribute>` 查找 `name` 匹配项（C# Attribute 后缀
    /// 省略规则：`Has("Fact")` 同时匹配 `Fact` 与 `FactAttribute`）。
    fn eval_attribute_list_method(
        &mut self,
        attrs: &Rc<Vec<crate::ResolvedAttribute>>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        match method.as_str() {
            "Has" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let name = self.eval_string_arg(&args[0])?;
                // C# 规范：[Fact] 与 [FactAttribute] 等价。
                // 同时匹配短名（name）与长名（name + "Attribute"）。
                let long_name = format!("{name}Attribute");
                let found = attrs.iter().any(|a| {
                    let n = a.name.as_str();
                    n == name || n == long_name
                });
                Ok(Value::Bool(found))
            }
            "GetArgCount" => {
                if !args.is_empty() {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 0,
                        found: args.len(),
                        span,
                    });
                }
                let total: i64 = attrs.iter().map(|a| a.args.len() as i64).sum();
                Ok(Value::Int(total))
            }
            "GetArg" => {
                if args.len() != 2 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 2,
                        found: args.len(),
                        span,
                    });
                }
                let attr_name = self.eval_string_arg(&args[0])?;
                let idx = self.eval_int_arg(&args[1])?;
                let long_name = format!("{attr_name}Attribute");
                for attr in attrs.iter() {
                    let n = attr.name.as_str();
                    if n == attr_name || n == long_name {
                        if idx >= 0 && (idx as usize) < attr.args.len() {
                            return Ok(match &attr.args[idx as usize] {
                                crate::ResolvedArg::String(s) => Value::String(s.clone()),
                                crate::ResolvedArg::Int(i) => Value::Int(*i),
                                crate::ResolvedArg::Bool(b) => Value::Bool(*b),
                                crate::ResolvedArg::Type(t) => Value::String(t.to_string()),
                                crate::ResolvedArg::Expression(_) => {
                                    Value::String("Expression".to_string())
                                }
                                crate::ResolvedArg::Enum { variant, .. } => {
                                    Value::String(variant.to_string())
                                }
                            });
                        }
                        return Ok(Value::String(String::new()));
                    }
                }
                Ok(Value::String(String::new()))
            }
            "GetNamedArg" => {
                if args.len() != 2 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 2,
                        found: args.len(),
                        span,
                    });
                }
                let attr_name = self.eval_string_arg(&args[0])?;
                let arg_name = self.eval_string_arg(&args[1])?;
                let long_name = format!("{attr_name}Attribute");
                for attr in attrs.iter() {
                    let n = attr.name.as_str();
                    if n == attr_name || n == long_name {
                        for (name, value) in &attr.named_args {
                            if name.as_str() == arg_name {
                                return Ok(match value {
                                    crate::ResolvedArg::String(s) => Value::String(s.clone()),
                                    crate::ResolvedArg::Int(i) => Value::Int(*i),
                                    crate::ResolvedArg::Bool(b) => Value::Bool(*b),
                                    crate::ResolvedArg::Type(t) => Value::String(t.to_string()),
                                    crate::ResolvedArg::Expression(_) => {
                                        Value::String("Expression".to_string())
                                    }
                                    crate::ResolvedArg::Enum { variant, .. } => {
                                        Value::String(variant.to_string())
                                    }
                                });
                            }
                        }
                        return Ok(Value::String(String::new()));
                    }
                }
                Ok(Value::String(String::new()))
            }
            _ => Err(EvalError::NotInWhitelist {
                receiver_ty: "AttributeList".to_string(),
                method: method.clone(),
                span,
            }),
        }
    }

    /// RFC 012 M5-2b: 求值 `SymbolTable` 实例方法。
    ///
    /// 拦截以下方法调用并返回真实符号数据：
    /// - `GetTypeName(int defId) -> string`：返回元组第 0 项（类型名）
    /// - `GetMemberName(int defId) -> string`：返回元组第 1 项（成员名）
    ///
    /// 未命中返回空串（与 `std/Arc/CodeGeneration/Generators.as` 占位一致）。
    /// 对类/字段等非方法成员，调用方在构造映射时成员名填空串。
    fn eval_symbol_table_method(
        &mut self,
        symbols: &Rc<indexmap::IndexMap<DefId, (String, String)>>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        match method.as_str() {
            "GetTypeName" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::String(String::new()));
                }
                let name = symbols
                    .get(&DefId(def_id_val as u32))
                    .map(|(ty, _)| ty.clone())
                    .unwrap_or_default();
                Ok(Value::String(name))
            }
            "GetMemberName" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::String(String::new()));
                }
                let name = symbols
                    .get(&DefId(def_id_val as u32))
                    .map(|(_, member)| member.clone())
                    .unwrap_or_default();
                Ok(Value::String(name))
            }
            _ => Err(EvalError::NotInWhitelist {
                receiver_ty: "SymbolTable".to_string(),
                method: method.clone(),
                span,
            }),
        }
    }

    /// Phase 2 序列化体系：求值 `TypeTable` 实例方法。
    ///
    /// 拦截以下方法调用并返回类型元数据：
    /// - `GetTypeName(int defId) -> string`：返回类型名
    /// - `GetKind(int defId) -> string`：返回类型种类（"class"/"struct"/"interface"/"enum"）
    /// - `GetFieldCount(int defId) -> int`：返回字段数
    /// - `GetFieldName(int defId, int index) -> string`：返回指定字段名
    /// - `GetFieldType(int defId, int index) -> string`：返回指定字段类型
    /// - `GetEnumMemberCount(int defId) -> int`：返回枚举成员数（非枚举返回 0）
    /// - `GetEnumMemberName(int defId, int index) -> string`：返回枚举成员名
    /// - `GetBaseType(int defId) -> string`：返回基类名
    ///
    /// 未命中 DefId 返回默认值（空串/0），与占位实现一致。
    fn eval_type_table_method(
        &mut self,
        table: &Rc<indexmap::IndexMap<DefId, TypeTableEntry>>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        match method.as_str() {
            "GetTypeName" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::String(String::new()));
                }
                let name = table
                    .get(&DefId(def_id_val as u32))
                    .map(|e| e.type_name.clone())
                    .unwrap_or_default();
                Ok(Value::String(name))
            }
            "GetKind" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::String(String::new()));
                }
                let kind = table
                    .get(&DefId(def_id_val as u32))
                    .map(|e| e.kind.clone())
                    .unwrap_or_default();
                Ok(Value::String(kind))
            }
            "GetFieldCount" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::Int(0));
                }
                let count = table
                    .get(&DefId(def_id_val as u32))
                    .map(|e| e.field_names.len() as i64)
                    .unwrap_or(0);
                Ok(Value::Int(count))
            }
            "GetFieldName" => {
                if args.len() != 2 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 2,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                let idx = self.eval_int_arg(&args[1])?;
                if def_id_val < 0 || idx < 0 {
                    return Ok(Value::String(String::new()));
                }
                let name = table
                    .get(&DefId(def_id_val as u32))
                    .and_then(|e| e.field_names.get(idx as usize))
                    .cloned()
                    .unwrap_or_default();
                Ok(Value::String(name))
            }
            "GetFieldType" => {
                if args.len() != 2 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 2,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                let idx = self.eval_int_arg(&args[1])?;
                if def_id_val < 0 || idx < 0 {
                    return Ok(Value::String(String::new()));
                }
                let ftype = table
                    .get(&DefId(def_id_val as u32))
                    .and_then(|e| e.field_types.get(idx as usize))
                    .cloned()
                    .unwrap_or_default();
                Ok(Value::String(ftype))
            }
            "GetEnumMemberCount" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::Int(0));
                }
                let count = table
                    .get(&DefId(def_id_val as u32))
                    .map(|e| e.enum_member_names.len() as i64)
                    .unwrap_or(0);
                Ok(Value::Int(count))
            }
            "GetEnumMemberName" => {
                if args.len() != 2 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 2,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                let idx = self.eval_int_arg(&args[1])?;
                if def_id_val < 0 || idx < 0 {
                    return Ok(Value::String(String::new()));
                }
                let name = table
                    .get(&DefId(def_id_val as u32))
                    .and_then(|e| e.enum_member_names.get(idx as usize))
                    .cloned()
                    .unwrap_or_default();
                Ok(Value::String(name))
            }
            "GetBaseType" => {
                if args.len() != 1 {
                    return Err(EvalError::ArgCount {
                        method: method.clone(),
                        expected: 1,
                        found: args.len(),
                        span,
                    });
                }
                let def_id_val = self.eval_int_arg(&args[0])?;
                if def_id_val < 0 {
                    return Ok(Value::String(String::new()));
                }
                let base = table
                    .get(&DefId(def_id_val as u32))
                    .map(|e| e.base_type.clone())
                    .unwrap_or_default();
                Ok(Value::String(base))
            }
            _ => Err(EvalError::NotInWhitelist {
                receiver_ty: "TypeTable".to_string(),
                method: method.clone(),
                span,
            }),
        }
    }

    /// RFC 012 M5-2b: 求值方法参数并断言为 `Int`。
    ///
    /// 供 `AttributeTable.GetDefIdAt` / `GetAttrs` / `SymbolTable.GetTypeName`
    /// 等接受 `int` 参数的方法复用——参数类型校验统一报
    /// `ValueTypeMismatch`，锚点指向参数表达式 span。
    fn eval_int_arg(&mut self, arg: &Spanned<Expr>) -> Result<i64, EvalError> {
        let v = self.eval_expr(arg)?;
        match v {
            Value::Int(i) => Ok(i),
            other => Err(EvalError::ValueTypeMismatch {
                expected: "int",
                found: other.type_name(),
                span: arg.span,
            }),
        }
    }

    /// RFC 012 M5-2b: 求值方法参数并断言为 `String`。
    ///
    /// 供 `AttributeList.Has(string name)` 复用——参数类型校验统一报
    /// `ValueTypeMismatch`，锚点指向参数表达式 span。
    fn eval_string_arg(&mut self, arg: &Spanned<Expr>) -> Result<String, EvalError> {
        let v = self.eval_expr(arg)?;
        match v {
            Value::String(s) => Ok(s),
            other => Err(EvalError::ValueTypeMismatch {
                expected: "string",
                found: other.type_name(),
                span: arg.span,
            }),
        }
    }

    /// RFC 009 M4-5/M4-7: 求值 Expression 类层次的虚方法访问器。
    ///
    /// RFC 022 Expression 类层次使用虚方法（GetLeft/GetRight/GetStringValue
    /// 等）暴露子节点结构。M4-5 求值器从 `props` 映射中查询属性值并
    /// 按方法语义返回。
    ///
    /// RFC 009 M4-7: `node` 携带完整 `ExpressionNode` IR 树时，子节点访问器
    /// （`GetLeft` / `GetRight` / `GetOperand` / `GetTarget` / `GetArg0` /
    /// `GetBody` / `GetCond` / `GetThen` / `GetElse` / `GetExpr`）返回真实
    /// 子 `Value::Expression`。`node=None` 时（测试占位或 M4-5 兼容路径），
    /// 子节点访问器返回 `Value::Null`，与 M4-5 行为一致。
    fn eval_expression_method(
        &self,
        type_name: &str,
        props: &indexmap::IndexMap<String, String>,
        node: &Option<Box<ast::ExpressionNode>>,
        method: &Ident,
        args: &[Spanned<Expr>],
        span: Span,
    ) -> Result<Value, EvalError> {
        // 所有 Expression 访问器都是零参数（除 ToString 也是零参数）
        if !args.is_empty() {
            return Err(EvalError::ArgCount {
                method: method.clone(),
                expected: 0,
                found: args.len(),
                span,
            });
        }
        match method.as_str() {
            // 字符串访问器：直接从 props 查询
            "GetStringValue" | "GetMethodName" | "GetMember" | "GetTargetType" | "GetName" => Ok(
                Value::String(props.get(method.as_str()).cloned().unwrap_or_default()),
            ),
            // 布尔访问器：IsStringConstant → 查询 IsString 属性
            "IsStringConstant" => {
                let v = props.get("IsString").map(|s| s == "true").unwrap_or(false);
                Ok(Value::Bool(v))
            }
            // ToString：返回 type_name + props 摘要
            "ToString" => Ok(Value::String(format!(
                "{type_name}({})",
                props
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
            // RFC 009 M4-7: 子节点访问器——从 `node` 取出对应子节点，
            // 转换为 `Value::Expression`。`node=None` 或字段不匹配时
            // 返回 `Value::Null`（M4-5 兼容行为）。
            "GetLeft" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Binary { left, .. } => Some((**left).clone()),
                _ => None,
            })),
            "GetRight" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Binary { right, .. } => Some((**right).clone()),
                _ => None,
            })),
            "GetOperand" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Unary { operand, .. } => Some((**operand).clone()),
                ast::ExpressionNode::Cast { operand, .. } => Some((**operand).clone()),
                _ => None,
            })),
            "GetTarget" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Call { target, .. } => target.clone().map(|t| *t),
                _ => None,
            })),
            "GetArg0" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Call { args, .. } => args.first().cloned(),
                _ => None,
            })),
            "GetBody" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Lambda { body, .. } => Some((**body).clone()),
                _ => None,
            })),
            "GetCond" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Conditional { test, .. } => Some((**test).clone()),
                _ => None,
            })),
            "GetThen" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Conditional { if_true, .. } => Some((**if_true).clone()),
                _ => None,
            })),
            "GetElse" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Conditional { if_false, .. } => Some((**if_false).clone()),
                _ => None,
            })),
            "GetExpr" => Ok(self.child_expr(node, |n| match n.as_ref() {
                ast::ExpressionNode::Index { object, .. } => Some((**object).clone()),
                _ => None,
            })),
            // 内存执行后端：M4-5 暂不实现
            "EvalInt" | "EvalBool" | "EvalString" => Err(EvalError::Unsupported {
                node: "Expression.Eval* (M4-5 暂未实现内存执行)",
                span,
            }),
            _ => Err(EvalError::NotInWhitelist {
                receiver_ty: "Expression".to_string(),
                method: method.clone(),
                span,
            }),
        }
    }

    /// RFC 009 M4-7: 从 `node` 中按 `extract` 抽取子节点，转换为
    /// `Value::Expression`。`node=None` 或 `extract` 返回 `None` 时
    /// 返回 `Value::Null`（M4-5 兼容行为）。
    fn child_expr(
        &self,
        node: &Option<Box<ast::ExpressionNode>>,
        extract: impl Fn(&Box<ast::ExpressionNode>) -> Option<ast::ExpressionNode>,
    ) -> Value {
        match node {
            Some(n) => match extract(n) {
                Some(child) => expression_node_to_value(&child),
                None => Value::Null,
            },
            None => Value::Null,
        }
    }

    fn eval_binary(
        &mut self,
        op: &ast::BinOp,
        left: &Spanned<Expr>,
        right: &Spanned<Expr>,
        span: Span,
    ) -> Result<Value, EvalError> {
        use ast::BinOp;
        let lv = self.eval_expr(left)?;
        let rv = self.eval_expr(right)?;
        match op {
            // 字符串拼接（仅 string + string 与 string + coercible）
            BinOp::Add => {
                let ls = match &lv {
                    Value::String(s) => s.clone(),
                    Value::Int(i) => i.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    Value::StringBuilder(_)
                    | Value::List(_)
                    | Value::Expression { .. }
                    | Value::GeneratorContext { .. }
                    | Value::AttributeTable(_)
                    | Value::AttributeList(_)
                    | Value::SymbolTable(_)
                    | Value::TypeTable(_)
                    | Value::ClassExpression(_)
                    | Value::MethodExpression(_)
                    | Value::AttributeData(_)
                    | Value::ParameterData(_) => {
                        return Err(EvalError::ValueTypeMismatch {
                            expected: "string-coercible",
                            found: lv.type_name(),
                            span: left.span,
                        });
                    }
                };
                let rs = match &rv {
                    Value::String(s) => s.clone(),
                    Value::Int(i) => i.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    Value::StringBuilder(_)
                    | Value::List(_)
                    | Value::Expression { .. }
                    | Value::GeneratorContext { .. }
                    | Value::AttributeTable(_)
                    | Value::AttributeList(_)
                    | Value::SymbolTable(_)
                    | Value::TypeTable(_)
                    | Value::ClassExpression(_)
                    | Value::MethodExpression(_)
                    | Value::AttributeData(_)
                    | Value::ParameterData(_) => {
                        return Err(EvalError::ValueTypeMismatch {
                            expected: "string-coercible",
                            found: rv.type_name(),
                            span: right.span,
                        });
                    }
                };
                Ok(Value::String(format!("{ls}{rs}")))
            }
            // 整数运算
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let (Value::Int(li), Value::Int(ri)) = (&lv, &rv) else {
                    return Err(EvalError::ValueTypeMismatch {
                        expected: "int",
                        found: lv.type_name(),
                        span,
                    });
                };
                let r = match op {
                    BinOp::Sub => li.wrapping_sub(*ri),
                    BinOp::Mul => li.wrapping_mul(*ri),
                    BinOp::Div => {
                        if *ri == 0 {
                            return Err(EvalError::Unsupported {
                                node: "division by zero",
                                span,
                            });
                        }
                        li.wrapping_div(*ri)
                    }
                    BinOp::Mod => {
                        if *ri == 0 {
                            return Err(EvalError::Unsupported {
                                node: "modulo by zero",
                                span,
                            });
                        }
                        li.wrapping_rem(*ri)
                    }
                    _ => unreachable!(),
                };
                Ok(Value::Int(r))
            }
            // 整数位运算与移位
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                let (Value::Int(li), Value::Int(ri)) = (&lv, &rv) else {
                    return Err(EvalError::ValueTypeMismatch {
                        expected: "int",
                        found: lv.type_name(),
                        span,
                    });
                };
                let r = match op {
                    BinOp::BitAnd => li & ri,
                    BinOp::BitOr => li | ri,
                    BinOp::BitXor => li ^ ri,
                    BinOp::Shl => li.wrapping_shl(*ri as u32),
                    BinOp::Shr => li.wrapping_shr(*ri as u32),
                    _ => unreachable!(),
                };
                Ok(Value::Int(r))
            }
            // 整数比较
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::NotEq => {
                // RFC 009 M5-2b: `Eq` / `NotEq` 支持 String 与 Bool 相等比较
                // （Source Generator 场景需要 `if (typeName == "Fact")` 等
                // 字符串判断）。Lt/Le/Gt/Ge 仍仅支持 Int（字符串大小比较
                // 无明确语义）。
                match op {
                    BinOp::Eq | BinOp::NotEq => match (&lv, &rv) {
                        (Value::Int(li), Value::Int(ri)) => Ok(Value::Bool(match op {
                            BinOp::Eq => li == ri,
                            BinOp::NotEq => li != ri,
                            _ => unreachable!(),
                        })),
                        (Value::String(ls), Value::String(rs)) => Ok(Value::Bool(match op {
                            BinOp::Eq => ls == rs,
                            BinOp::NotEq => ls != rs,
                            _ => unreachable!(),
                        })),
                        (Value::Bool(lb), Value::Bool(rb)) => Ok(Value::Bool(match op {
                            BinOp::Eq => lb == rb,
                            BinOp::NotEq => lb != rb,
                            _ => unreachable!(),
                        })),
                        _ => Err(EvalError::ValueTypeMismatch {
                            expected: "int/string/bool (相同类型双目)",
                            found: lv.type_name(),
                            span,
                        }),
                    },
                    _ => {
                        let (Value::Int(li), Value::Int(ri)) = (&lv, &rv) else {
                            return Err(EvalError::ValueTypeMismatch {
                                expected: "int",
                                found: lv.type_name(),
                                span,
                            });
                        };
                        let b = match op {
                            BinOp::Lt => li < ri,
                            BinOp::Le => li <= ri,
                            BinOp::Gt => li > ri,
                            BinOp::Ge => li >= ri,
                            _ => unreachable!(),
                        };
                        Ok(Value::Bool(b))
                    }
                }
            }
            // 逻辑运算
            BinOp::And | BinOp::Or => {
                let (Value::Bool(lb), Value::Bool(rb)) = (&lv, &rv) else {
                    return Err(EvalError::ValueTypeMismatch {
                        expected: "bool",
                        found: lv.type_name(),
                        span,
                    });
                };
                Ok(Value::Bool(match op {
                    BinOp::And => *lb && *rb,
                    BinOp::Or => *lb || *rb,
                    _ => unreachable!(),
                }))
            }
        }
    }
}

/// 从 AST `Type` 提取顶层类型名（用于 `new T()` 与白名单查询）。
///
/// RFC 009 M5-3: 即便 `T` 携带泛型实参（如 `List<string>`），也返回
/// 顶层名字（`"List"`）——`eval_new` 中按 type_name 分派后另行校验泛型
/// 实参与白名单约束（如仅允许 `List<string>`，禁止 `List<int>`）。
fn extract_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Named { path, .. } => path.last().map(|n| n.to_string()),
        _ => None,
    }
}

/// RFC 009 M4-7: 把 `ExpressionTree` 转换为求值器 `Value::Expression`。
///
/// 复用 `expression_node_to_value` 转换根节点，附带 ExpressionTree 的
/// 类型名到 props（`NodeType` 与 `TypeName`）。供
/// [`Evaluator::inject_expression_locals`] 在 Pass 3 注入 locals 时调用。
fn expression_tree_to_value(tree: &ast::ExpressionTree) -> Value {
    let mut value = expression_node_to_value(&tree.root);
    if let Value::Expression { props, .. } = &mut value {
        if !props.contains_key("TypeName") {
            props.insert("TypeName".to_string(), tree.ty.to_string());
        }
    }
    value
}

/// RFC 022 Sprint 2d Slice 2: `BinOp` → per-op `ExpressionType` 名称（与
/// `std/Arc/Linq/Expressions/ExpressionType.as` 一致）。Binary 节点不再有
/// `GetOp` 访问器，NodeType 即运算符（C# 对齐）。
fn binop_expr_type_name(op: &ast::BinOp) -> &'static str {
    match op {
        ast::BinOp::Add => "Add",
        ast::BinOp::Sub => "Subtract",
        ast::BinOp::Mul => "Multiply",
        ast::BinOp::Div => "Divide",
        ast::BinOp::Mod => "Modulo",
        ast::BinOp::Eq => "Equal",
        ast::BinOp::NotEq => "NotEqual",
        ast::BinOp::Lt => "LessThan",
        ast::BinOp::Le => "LessThanOrEqual",
        ast::BinOp::Gt => "GreaterThan",
        ast::BinOp::Ge => "GreaterThanOrEqual",
        ast::BinOp::And => "AndAlso",
        ast::BinOp::Or => "OrElse",
        ast::BinOp::BitAnd => "And",
        ast::BinOp::BitOr => "Or",
        ast::BinOp::BitXor => "ExclusiveOr",
        ast::BinOp::Shl => "LeftShift",
        ast::BinOp::Shr => "RightShift",
    }
}

/// RFC 022 Sprint 2d Slice 2: `UnaryOp` → per-op `ExpressionType` 名称。
fn unaryop_expr_type_name(op: &ast::UnaryOp) -> &'static str {
    match op {
        ast::UnaryOp::Not => "Not",
        ast::UnaryOp::Neg => "Negate",
        ast::UnaryOp::BitNot => "Not",
    }
}

/// RFC 009 M4-7: 把 `ExpressionNode` IR 转换为求值器 `Value::Expression`。
///
/// 根据 IR 节点变体填充：
/// - `type_name`：C# Expression 类层次对应的具体类名
///   （`ConstantExpression` / `ParameterExpression` / `BinaryExpression` 等）
/// - `props`：属性名 → 字符串值（如 `NodeType` / `GetStringValue` / `IsString`）
/// - `node`：完整 IR 子树，供子节点访问器（`GetLeft` / `GetRight` 等）遍历
///
/// 字符串常量节点：`IsString=true` + `GetStringValue=<值>`，使
/// `expr.IsStringConstant()` 返回 `true` 且 `expr.GetStringValue()` 返回值。
/// 整数常量节点：`IsString=false` + `GetStringValue=<十进制文本>`。
fn expression_node_to_value(node: &ast::ExpressionNode) -> Value {
    use ast::ExpressionNode as N;
    let (type_name, props): (String, indexmap::IndexMap<String, String>) = match node {
        N::Constant(cv) => {
            let mut p = indexmap::IndexMap::new();
            match cv {
                ast::ConstantValue::Int(n) => {
                    p.insert("NodeType".to_string(), "Constant".to_string());
                    p.insert("IsString".to_string(), "false".to_string());
                    p.insert("GetStringValue".to_string(), n.to_string());
                    ("ConstantExpression".to_string(), p)
                }
                ast::ConstantValue::Float(f) => {
                    p.insert("NodeType".to_string(), "Constant".to_string());
                    p.insert("IsString".to_string(), "false".to_string());
                    p.insert("GetStringValue".to_string(), f.to_string());
                    ("ConstantExpression".to_string(), p)
                }
                ast::ConstantValue::Bool(b) => {
                    p.insert("NodeType".to_string(), "Constant".to_string());
                    p.insert("IsString".to_string(), "false".to_string());
                    p.insert("GetStringValue".to_string(), b.to_string());
                    ("ConstantExpression".to_string(), p)
                }
                ast::ConstantValue::String(s) => {
                    p.insert("NodeType".to_string(), "Constant".to_string());
                    p.insert("IsString".to_string(), "true".to_string());
                    p.insert("GetStringValue".to_string(), s.clone());
                    ("ConstantExpression".to_string(), p)
                }
            }
        }
        N::Parameter { name, ty } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Parameter".to_string());
            p.insert("GetName".to_string(), name.to_string());
            p.insert("TypeName".to_string(), ty.to_string());
            ("ParameterExpression".to_string(), p)
        }
        N::Capture { name, ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Capture".to_string());
            p.insert("GetName".to_string(), name.to_string());
            p.insert("TypeName".to_string(), ty.to_string());
            ("CaptureExpression".to_string(), p)
        }
        N::MemberAccess { member, ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "MemberAccess".to_string());
            p.insert("GetMember".to_string(), member.to_string());
            p.insert("TypeName".to_string(), ty.to_string());
            ("MemberExpression".to_string(), p)
        }
        N::Binary { op, .. } => {
            let mut p = indexmap::IndexMap::new();
            // RFC 022 Sprint 2d Slice 2: per-op NodeType（GetOp 访问器已随 Op 字段移除）。
            p.insert("NodeType".to_string(), binop_expr_type_name(op).to_string());
            ("BinaryExpression".to_string(), p)
        }
        N::Unary { op, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert(
                "NodeType".to_string(),
                unaryop_expr_type_name(op).to_string(),
            );
            ("UnaryExpression".to_string(), p)
        }
        N::Call { method, target, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Call".to_string());
            p.insert("GetMethodName".to_string(), method.to_string());
            if target.is_none() {
                p.insert("GetTargetType".to_string(), "static".to_string());
            }
            ("MethodCallExpression".to_string(), p)
        }
        N::Lambda { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Lambda".to_string());
            ("LambdaExpression".to_string(), p)
        }
        N::Index { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Index".to_string());
            ("IndexExpression".to_string(), p)
        }
        N::Conditional { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Conditional".to_string());
            ("ConditionalExpression".to_string(), p)
        }
        N::New { type_name, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "New".to_string());
            p.insert("GetTargetType".to_string(), type_name.to_string());
            ("NewExpression".to_string(), p)
        }
        N::Cast { target_type, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Cast".to_string());
            p.insert("GetTargetType".to_string(), target_type.to_string());
            ("UnaryExpression".to_string(), p)
        }

        // ── L2 表达式扩展（RFC 022 §2.2.10，18 变体）──
        // D10.6 解释器等编译期扩展路径使用；codegen 不消费（emit_expr_tree 仅识别 L1）。
        // 每个 L2 节点映射到 Arc 侧对应的 Expression 派生类名 + NodeType 属性。
        N::This => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "This".to_string());
            ("ThisExpression".to_string(), p)
        }
        N::Base => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Base".to_string());
            ("BaseExpression".to_string(), p)
        }
        N::Null => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Null".to_string());
            p.insert("IsNull".to_string(), "true".to_string());
            ("NullExpression".to_string(), p)
        }
        N::Path { segments } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Path".to_string());
            p.insert(
                "GetPath".to_string(),
                segments
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("."),
            );
            ("PathExpression".to_string(), p)
        }
        N::If { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "If".to_string());
            ("IfExpression".to_string(), p)
        }
        N::Switch { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Switch".to_string());
            ("SwitchExpression".to_string(), p)
        }
        N::Coalesce { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Coalesce".to_string());
            ("CoalesceExpression".to_string(), p)
        }
        N::NullConditional { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "NullConditional".to_string());
            ("NullConditionalExpression".to_string(), p)
        }
        N::ForceDeref { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "ForceDeref".to_string());
            ("ForceDerefExpression".to_string(), p)
        }
        N::Is { pattern, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Is".to_string());
            let pattern_kind = match pattern {
                ast::IsPatternNode::Type { ty, binding } => {
                    if let Some(b) = binding {
                        format!("Type({} {})", ty, b)
                    } else {
                        format!("Type({})", ty)
                    }
                }
                ast::IsPatternNode::Var(name) => format!("Var({})", name),
                ast::IsPatternNode::Null => "Null".to_string(),
            };
            p.insert("GetPattern".to_string(), pattern_kind);
            ("IsExpression".to_string(), p)
        }
        N::TypeOf { ty } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "TypeOf".to_string());
            p.insert("GetTargetType".to_string(), ty.to_string());
            ("TypeOfExpression".to_string(), p)
        }
        N::Default { ty } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Default".to_string());
            p.insert("GetTargetType".to_string(), ty.to_string());
            ("DefaultExpression".to_string(), p)
        }
        N::Await { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Await".to_string());
            ("AwaitExpression".to_string(), p)
        }
        N::Block { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Block".to_string());
            ("BlockExpression".to_string(), p)
        }
        N::Collection { elements } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Collection".to_string());
            p.insert("GetCount".to_string(), elements.len().to_string());
            ("CollectionExpression".to_string(), p)
        }
        N::Box { value_ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Box".to_string());
            p.insert("GetValueType".to_string(), value_ty.to_string());
            ("BoxExpression".to_string(), p)
        }
        N::Unbox { value_ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Unbox".to_string());
            p.insert("GetValueType".to_string(), value_ty.to_string());
            ("UnboxExpression".to_string(), p)
        }
        N::Query { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Query".to_string());
            ("QueryExpression".to_string(), p)
        }

        // ── L3 语句层（RFC 022 §2.2.10，10 变体）──
        // 仅在 BlockExpression.Statements 中出现；D10.6 解释器遍历 Block 时使用。
        N::Let { name, ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Let".to_string());
            p.insert("GetName".to_string(), name.to_string());
            if let Some(t) = ty {
                p.insert("TypeName".to_string(), t.to_string());
            }
            ("LetExpression".to_string(), p)
        }
        N::Assign { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Assign".to_string());
            ("AssignExpression".to_string(), p)
        }
        N::Return { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Return".to_string());
            ("ReturnExpression".to_string(), p)
        }
        N::Break => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Break".to_string());
            ("BreakExpression".to_string(), p)
        }
        N::Continue => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Continue".to_string());
            ("ContinueExpression".to_string(), p)
        }
        N::Throw { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Throw".to_string());
            ("ThrowExpression".to_string(), p)
        }
        N::While { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "While".to_string());
            ("WhileExpression".to_string(), p)
        }
        N::For { var, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "For".to_string());
            p.insert("GetVar".to_string(), var.to_string());
            ("ForExpression".to_string(), p)
        }
        N::TryCatch {
            catch_ty,
            catch_name,
            ..
        } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "TryCatch".to_string());
            p.insert("GetCatchType".to_string(), catch_ty.to_string());
            p.insert("GetCatchName".to_string(), catch_name.to_string());
            ("TryCatchExpression".to_string(), p)
        }
        N::TryFinally { .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "TryFinally".to_string());
            ("TryFinallyExpression".to_string(), p)
        }
        N::Using { name, ty, .. } => {
            let mut p = indexmap::IndexMap::new();
            p.insert("NodeType".to_string(), "Using".to_string());
            p.insert("GetName".to_string(), name.to_string());
            if let Some(t) = ty {
                p.insert("TypeName".to_string(), t.to_string());
            }
            ("UsingExpression".to_string(), p)
        }
    };
    Value::Expression {
        type_name,
        props,
        node: Some(Box::new(node.clone())),
    }
}

// 局部别名，避免在模块顶层污染 `ControlFlow` 命名空间——所有使用处显式
// 写 `ControlFlow::Normal` / `ControlFlow::Return` 提升可读性。

/// RFC 012 M5-2b: 构造 `Value::GeneratorContext` 实例。
///
/// 供 `expand_source_generators` 在求值 Source Generator 前调用，
/// 把 typeck 产物的 `AttributeTable` 与构造的 `DefId → (类型名, 成员名)`
/// 映射包装为 `Value::GeneratorContext` 注入求值器环境。
///
/// # 参数
///
/// - `attributes`：`Rc` 共享的 `AttributeTable`（来自
///   `TypeChecker.attribute_table`，求值器只读访问）
/// - `symbols`：`DefId → (类型名, 成员名)` 映射，由调用方从
///   `TypeChecker.class_def_ids`（类自身，成员名为空串）与
///   `def_id_members`（方法成员，含 owner 类名 + 方法名）合并构造。
///   供 Generate 方法内 `context.Symbols.GetTypeName(defId)` 与
///   `context.Symbols.GetMemberName(defId)` 反查。
/// - `source_files`：当前编译单元的源文件路径列表
///
/// # 调用方
///
/// `checker::expand_source_generators` 在求值 Generate 方法前调用：
///
/// ```ignore
/// let ctx = evaluator::make_generator_context(
///     Rc::new(self.attribute_table.clone()),
///     build_symbols_map(self),
///     source_files,
/// );
/// evaluator.eval_generate_method_with_context(
///     &body,
///     Some(Ident::from("context")),
///     Some(ctx),
/// )?;
/// ```
pub fn make_generator_context(
    attributes: Rc<crate::AttributeTable>,
    symbols: indexmap::IndexMap<DefId, (String, String)>,
    source_files: Vec<String>,
    type_table: indexmap::IndexMap<DefId, TypeTableEntry>,
) -> Value {
    Value::GeneratorContext {
        attributes,
        symbols: Rc::new(symbols),
        source_files: Rc::new(source_files),
        type_table: Rc::new(type_table),
    }
}

#[cfg(test)]
mod tests {
    //! 单元测试：通过手动构造 AST 验证 evaluator 求值语义。
    //!
    //! 端到端测试（解析 → typeck → 提取 registration → 求值）位于
    //! `tests/macro_e2e.rs` M4-4 章节中。

    use super::*;
    use ast::{Block, Spanned};

    /// 构造 `() => expr` 形式的 lambda（无参，表达式体）。
    fn lambda_expr(expr: Expr) -> LambdaExpr {
        LambdaExpr {
            params: vec![],
            body: LambdaBody::Expr(Box::new(Spanned::new(expr, Span::DUMMY))),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }
    }

    /// 构造 `() => { stmts }` 形式的 lambda（无参，块体）。
    fn lambda_block(stmts: Vec<Stmt>) -> LambdaExpr {
        let block = Block {
            stmts: stmts
                .into_iter()
                .map(|s| Spanned::new(s, Span::DUMMY))
                .collect(),
            tail: None,
        };
        LambdaExpr {
            params: vec![],
            body: LambdaBody::Block(block),
            is_expression_tree: false,
            is_async: false,
            captures: vec![],
        }
    }

    fn eval(lambda: LambdaExpr) -> Result<String, EvalError> {
        let w = Whitelist::new();
        let mut e = Evaluator::new(&w);
        e.eval_lambda(&lambda)
    }

    #[test]
    fn literal_string_lambda() {
        let l = lambda_expr(Expr::StringLit("hello".into()));
        assert_eq!(eval(l).unwrap(), "hello");
    }

    #[test]
    fn string_concat_with_plus() {
        // "a" + "b"
        let l = lambda_expr(Expr::Binary {
            op: ast::BinOp::Add,
            left: Box::new(Spanned::new(Expr::StringLit("a".into()), Span::DUMMY)),
            right: Box::new(Spanned::new(Expr::StringLit("b".into()), Span::DUMMY)),
        });
        assert_eq!(eval(l).unwrap(), "ab");
    }

    #[test]
    fn block_lambda_with_string_return() {
        let l = lambda_block(vec![Stmt::Return(Some(Spanned::new(
            Expr::StringLit("world".into()),
            Span::DUMMY,
        )))]);
        assert_eq!(eval(l).unwrap(), "world");
    }

    #[test]
    fn forbidden_loop_rejected() {
        let l = lambda_block(vec![Stmt::While {
            cond: Spanned::new(Expr::BoolLit(true), Span::DUMMY),
            body: Block::empty(),
        }]);
        assert!(matches!(
            eval(l),
            Err(EvalError::ForbiddenConstruct { construct, .. }) if construct.contains("loop")
        ));
    }

    #[test]
    fn forbidden_throw_rejected() {
        let l = lambda_block(vec![Stmt::Throw {
            expr: Spanned::new(Expr::StringLit("err".into()), Span::DUMMY),
        }]);
        assert!(matches!(
            eval(l),
            Err(EvalError::ForbiddenConstruct {
                construct: "throw",
                ..
            })
        ));
    }

    #[test]
    fn forbidden_try_catch_rejected() {
        let l = lambda_block(vec![Stmt::TryCatch {
            try_body: Block::empty(),
            catch_ty: ast::Type::named("Exception"),
            catch_name: "e".into(),
            when_cond: None,
            catch_body: Block::empty(),
            finally: None,
        }]);
        assert!(matches!(
            eval(l),
            Err(EvalError::ForbiddenConstruct {
                construct: "try/catch/finally",
                ..
            })
        ));
    }

    #[test]
    fn forbidden_lambda_creation_rejected() {
        let inner_lambda = lambda_expr(Expr::StringLit("inner".into()));
        let l = lambda_block(vec![Stmt::Let {
            mutable: false,
            name: "f".into(),
            ty: None,
            init: Some(Spanned::new(Expr::Lambda(inner_lambda), Span::DUMMY)),
        }]);
        assert!(matches!(
            eval(l),
            Err(EvalError::ForbiddenConstruct { construct, .. }) if construct.contains("lambda")
        ));
    }

    #[test]
    fn undefined_name_rejected() {
        let l = lambda_expr(Expr::Ident("unknown".into()));
        assert!(matches!(eval(l), Err(EvalError::UndefinedName { .. })));
    }

    #[test]
    fn return_non_string_rejected() {
        let l = lambda_block(vec![Stmt::Return(Some(Spanned::new(
            Expr::IntLit(42),
            Span::DUMMY,
        )))]);
        assert!(matches!(eval(l), Err(EvalError::ReturnTypeMismatch { .. })));
    }

    #[test]
    fn missing_return_in_block_rejected() {
        // 块无 return 也无 tail——返回 void 触发 ReturnTypeMismatch
        let l = lambda_block(vec![]);
        assert!(matches!(eval(l), Err(EvalError::ReturnTypeMismatch { .. })));
    }

    #[test]
    fn non_newable_type_rejected() {
        // RFC 009 M5-3: `List` 现已 newable（List<string>），改用 `Foo`
        // 验证白名单外类型仍被拒绝。
        let l = lambda_block(vec![
            Stmt::Let {
                mutable: false,
                name: "lst".into(),
                ty: None,
                init: Some(Spanned::new(
                    Expr::New {
                        ty: ast::Type::named("Foo"),
                        args: vec![],
                        obj_init: None,
                    },
                    Span::DUMMY,
                )),
            },
            Stmt::Return(Some(Spanned::new(Expr::StringLit("x".into()), Span::DUMMY))),
        ]);
        assert!(matches!(eval(l), Err(EvalError::NotNewable { .. })));
    }

    // ── M4-5: Expression 访问器测试 ──

    /// 构造一个 Expression 值并注入 locals，求值 lambda 验证字段访问。
    fn eval_with_expr(lambda: LambdaExpr, expr_val: Value) -> Result<String, EvalError> {
        let w = Whitelist::new();
        let mut locals = indexmap::IndexMap::new();
        locals.insert(Ident::from("expr"), expr_val);
        let mut e = Evaluator::with_locals(&w, locals);
        e.eval_lambda(&lambda)
    }

    /// 构造 ConstantExpression 值（StringValue="hello"）。
    fn constant_expr_value() -> Value {
        let mut props = indexmap::IndexMap::new();
        props.insert("NodeType".to_string(), "Constant".to_string());
        props.insert("GetStringValue".to_string(), "hello".to_string());
        props.insert("IsString".to_string(), "true".to_string());
        Value::Expression {
            type_name: "ConstantExpression".to_string(),
            props,
            node: None,
        }
    }

    #[test]
    fn m4_5_expression_field_access() {
        // `expr.NodeType` → "Constant"
        let l = lambda_expr(Expr::Field {
            receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
            field: "NodeType".into(),
        });
        let r = eval_with_expr(l, constant_expr_value()).unwrap();
        assert_eq!(r, "Constant");
    }

    #[test]
    fn m4_5_expression_method_call_get_string_value() {
        // `expr.GetStringValue()` → "hello"
        let l = lambda_expr(Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
            method: "GetStringValue".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        });
        let r = eval_with_expr(l, constant_expr_value()).unwrap();
        assert_eq!(r, "hello");
    }

    #[test]
    fn m4_5_expression_method_call_is_string_constant() {
        // `expr.IsStringConstant()` 返回 bool——不能直接作 Func<string> 返回值。
        // 用 if 分支消费 bool：`if (expr.IsStringConstant()) "yes" else "no"`
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
                    method: "IsStringConstant".into(),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("yes".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("no".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_expr(l, constant_expr_value()).unwrap();
        assert_eq!(r, "yes");
    }

    #[test]
    fn m4_5_expression_stringbuilder_integration() {
        // 复合场景：遍历 Expression，将 GetStringValue 拼到 StringBuilder
        // `() => { var sb = new StringBuilder(); sb.Append(expr.GetStringValue()); return sb.ToString(); }`
        let l = lambda_block(vec![
            Stmt::Let {
                mutable: false,
                name: "sb".into(),
                ty: None,
                init: Some(Spanned::new(
                    Expr::New {
                        ty: ast::Type::named("StringBuilder"),
                        args: vec![],
                        obj_init: None,
                    },
                    Span::DUMMY,
                )),
            },
            Stmt::Expr(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "Append".into(),
                    args: vec![Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Ident("expr".into()),
                                Span::DUMMY,
                            )),
                            method: "GetStringValue".into(),
                            args: vec![],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            Stmt::Return(Some(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "ToString".into(),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            ))),
        ]);
        let r = eval_with_expr(l, constant_expr_value()).unwrap();
        assert_eq!(r, "hello");
    }

    #[test]
    fn m4_5_expression_path_access() {
        // ExpressionType.Constant → "Constant"
        let l = lambda_expr(Expr::Path(vec!["ExpressionType".into(), "Constant".into()]));
        let w = Whitelist::new();
        let mut e = Evaluator::new(&w);
        let r = e.eval_lambda(&l).unwrap();
        assert_eq!(r, "Constant");
    }

    #[test]
    fn m4_5_expression_unknown_property_rejected() {
        // expr.NonExistent → Unsupported
        let l = lambda_expr(Expr::Field {
            receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
            field: "NonExistent".into(),
        });
        assert!(matches!(
            eval_with_expr(l, constant_expr_value()),
            Err(EvalError::Unsupported { .. })
        ));
    }

    #[test]
    fn m4_5_expression_eval_methods_unsupported() {
        // expr.EvalInt(ctx) → Unsupported（M4-5 未实现内存执行）
        let l = lambda_expr(Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
            method: "EvalInt".into(),
            args: vec![Spanned::new(Expr::Null, Span::DUMMY)],
            type_args: vec![],
            params_span: None,
        });
        // EvalInt 在白名单内但 args 非空 → ArgCount 错误（所有访问器要求零参数）
        assert!(matches!(
            eval_with_expr(l, constant_expr_value()),
            Err(EvalError::ArgCount { .. })
        ));
    }

    #[test]
    fn m4_5_expression_to_string_returns_summary() {
        // expr.ToString() → "ConstantExpression(NodeType=Constant, ...)"
        let l = lambda_expr(Expr::MethodCall {
            receiver: Box::new(Spanned::new(Expr::Ident("expr".into()), Span::DUMMY)),
            method: "ToString".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        });
        let r = eval_with_expr(l, constant_expr_value()).unwrap();
        assert!(r.starts_with("ConstantExpression("));
        assert!(r.contains("NodeType=Constant"));
    }

    // ── RFC 009 M5-2b: GeneratorContext 拦截机制测试 ──

    /// 构造测试用 `AttributeTable`：DefId(1) 上附加 `Fact` 属性。
    fn make_test_attribute_table() -> crate::AttributeTable {
        let mut t = crate::AttributeTable::new();
        t.register(
            DefId(1),
            crate::ResolvedAttribute::builtin(
                Ident::from("Fact"),
                vec![],
                crate::AttributeTarget::Method,
                Span::DUMMY,
            ),
        );
        t
    }

    /// 构造测试用 `GeneratorContext` 值：
    /// - attributes: 含 DefId(1) → [Fact]
    /// - symbols: DefId(1) → ("MyClass", "TestMethod")
    /// - source_files: ["test.as"]
    fn make_test_generator_context() -> Value {
        let mut symbols = indexmap::IndexMap::new();
        symbols.insert(DefId(1), ("MyClass".to_string(), "TestMethod".to_string()));
        make_generator_context(
            Rc::new(make_test_attribute_table()),
            symbols,
            vec!["test.as".to_string()],
            indexmap::IndexMap::new(),
        )
    }

    /// 用预填充 `context` 局部变量构造求值器。
    fn eval_with_context(lambda: LambdaExpr, ctx: Value) -> Result<String, EvalError> {
        let w = Whitelist::new();
        let mut locals = indexmap::IndexMap::new();
        locals.insert(Ident::from("context"), ctx);
        let mut e = Evaluator::with_locals(&w, locals);
        e.eval_lambda(&lambda)
    }

    #[test]
    fn m5_2b_generator_context_attributes_field_returns_attribute_table() {
        // context.Attributes.Count > 0 → "non-empty" else "empty"
        // 嵌套字段访问：context.Attributes → Value::AttributeTable，再 .Count → int
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::Binary {
                    op: ast::BinOp::Gt,
                    left: Box::new(Spanned::new(
                        Expr::Field {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Attributes".into(),
                                },
                                Span::DUMMY,
                            )),
                            field: "Count".into(),
                        },
                        Span::DUMMY,
                    )),
                    right: Box::new(Spanned::new(Expr::IntLit(0), Span::DUMMY)),
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("non-empty".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("empty".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "non-empty");
    }

    #[test]
    fn m5_2b_attribute_table_get_def_id_at_returns_first_def_id() {
        // if (context.Attributes.GetDefIdAt(0) == 1) "ok" else "fail"
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::Binary {
                    op: ast::BinOp::Eq,
                    left: Box::new(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Attributes".into(),
                                },
                                Span::DUMMY,
                            )),
                            method: "GetDefIdAt".into(),
                            args: vec![Spanned::new(Expr::IntLit(0), Span::DUMMY)],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )),
                    right: Box::new(Spanned::new(Expr::IntLit(1), Span::DUMMY)),
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("ok".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("fail".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "ok");
    }

    #[test]
    fn m5_2b_attribute_list_has_returns_true_for_fact_attribute() {
        // if (context.Attributes.GetAttrs(1).Has("Fact")) "yes" else "no"
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Attributes".into(),
                                },
                                Span::DUMMY,
                            )),
                            method: "GetAttrs".into(),
                            args: vec![Spanned::new(Expr::IntLit(1), Span::DUMMY)],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )),
                    method: "Has".into(),
                    args: vec![Spanned::new(Expr::StringLit("Fact".into()), Span::DUMMY)],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("yes".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("no".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "yes");
    }

    #[test]
    fn m5_2b_attribute_list_has_returns_false_for_missing_attribute() {
        // if (context.Attributes.GetAttrs(1).Has("Theory")) "yes" else "no"
        // DefId(1) 上只有 Fact，没有 Theory → false
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Attributes".into(),
                                },
                                Span::DUMMY,
                            )),
                            method: "GetAttrs".into(),
                            args: vec![Spanned::new(Expr::IntLit(1), Span::DUMMY)],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )),
                    method: "Has".into(),
                    args: vec![Spanned::new(Expr::StringLit("Theory".into()), Span::DUMMY)],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("yes".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("no".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "no");
    }

    #[test]
    fn m5_2b_symbol_table_get_type_name_returns_class_name() {
        // context.Symbols.GetTypeName(1) → "MyClass"
        // 用 StringBuilder 拼接验证字符串内容
        let l = lambda_block(vec![
            Stmt::Let {
                mutable: false,
                name: "sb".into(),
                ty: None,
                init: Some(Spanned::new(
                    Expr::New {
                        ty: ast::Type::named("StringBuilder"),
                        args: vec![],
                        obj_init: None,
                    },
                    Span::DUMMY,
                )),
            },
            Stmt::Expr(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "Append".into(),
                    args: vec![Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Symbols".into(),
                                },
                                Span::DUMMY,
                            )),
                            method: "GetTypeName".into(),
                            args: vec![Spanned::new(Expr::IntLit(1), Span::DUMMY)],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            Stmt::Return(Some(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "ToString".into(),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            ))),
        ]);
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "MyClass");
    }

    #[test]
    fn m5_2b_symbol_table_get_type_name_returns_empty_for_unknown_def_id() {
        // context.Symbols.GetTypeName(999) → "" (empty)
        // 用 if 验证：空串 + "x" == "x" → "default"
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::Binary {
                    op: ast::BinOp::Eq,
                    left: Box::new(Spanned::new(
                        Expr::Binary {
                            op: ast::BinOp::Add,
                            left: Box::new(Spanned::new(
                                Expr::MethodCall {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Field {
                                            receiver: Box::new(Spanned::new(
                                                Expr::Ident("context".into()),
                                                Span::DUMMY,
                                            )),
                                            field: "Symbols".into(),
                                        },
                                        Span::DUMMY,
                                    )),
                                    method: "GetTypeName".into(),
                                    args: vec![Spanned::new(Expr::IntLit(999), Span::DUMMY)],
                                    type_args: vec![],
                                    params_span: None,
                                },
                                Span::DUMMY,
                            )),
                            right: Box::new(Spanned::new(Expr::StringLit("x".into()), Span::DUMMY)),
                        },
                        Span::DUMMY,
                    )),
                    right: Box::new(Spanned::new(Expr::StringLit("x".into()), Span::DUMMY)),
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("default".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("named".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "default");
    }

    #[test]
    fn m5_2b_symbol_table_get_member_name_returns_method_name() {
        // context.Symbols.GetMemberName(1) → "TestMethod"
        // 用 StringBuilder 拼接验证字符串内容
        let l = lambda_block(vec![
            Stmt::Let {
                mutable: false,
                name: "sb".into(),
                ty: None,
                init: Some(Spanned::new(
                    Expr::New {
                        ty: ast::Type::named("StringBuilder"),
                        args: vec![],
                        obj_init: None,
                    },
                    Span::DUMMY,
                )),
            },
            Stmt::Expr(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "Append".into(),
                    args: vec![Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Field {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Ident("context".into()),
                                        Span::DUMMY,
                                    )),
                                    field: "Symbols".into(),
                                },
                                Span::DUMMY,
                            )),
                            method: "GetMemberName".into(),
                            args: vec![Spanned::new(Expr::IntLit(1), Span::DUMMY)],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            )),
            Stmt::Return(Some(Spanned::new(
                Expr::MethodCall {
                    receiver: Box::new(Spanned::new(Expr::Ident("sb".into()), Span::DUMMY)),
                    method: "ToString".into(),
                    args: vec![],
                    type_args: vec![],
                    params_span: None,
                },
                Span::DUMMY,
            ))),
        ]);
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "TestMethod");
    }

    #[test]
    fn m5_2b_symbol_table_get_member_name_returns_empty_for_unknown_def_id() {
        // context.Symbols.GetMemberName(999) → "" (empty)
        // 用 if 验证：空串 + "y" == "y" → "default"
        let l = lambda_expr(Expr::If {
            cond: Box::new(Spanned::new(
                Expr::Binary {
                    op: ast::BinOp::Eq,
                    left: Box::new(Spanned::new(
                        Expr::Binary {
                            op: ast::BinOp::Add,
                            left: Box::new(Spanned::new(
                                Expr::MethodCall {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Field {
                                            receiver: Box::new(Spanned::new(
                                                Expr::Ident("context".into()),
                                                Span::DUMMY,
                                            )),
                                            field: "Symbols".into(),
                                        },
                                        Span::DUMMY,
                                    )),
                                    method: "GetMemberName".into(),
                                    args: vec![Spanned::new(Expr::IntLit(999), Span::DUMMY)],
                                    type_args: vec![],
                                    params_span: None,
                                },
                                Span::DUMMY,
                            )),
                            right: Box::new(Spanned::new(Expr::StringLit("y".into()), Span::DUMMY)),
                        },
                        Span::DUMMY,
                    )),
                    right: Box::new(Spanned::new(Expr::StringLit("y".into()), Span::DUMMY)),
                },
                Span::DUMMY,
            )),
            then_branch: Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("default".into()),
                    Span::DUMMY,
                ))),
            },
            else_branch: Some(Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Expr::StringLit("named".into()),
                    Span::DUMMY,
                ))),
            }),
        });
        let r = eval_with_context(l, make_test_generator_context()).unwrap();
        assert_eq!(r, "default");
    }

    #[test]
    fn m5_2b_generator_context_unknown_property_rejected() {
        // context.UnknownProp → Unsupported
        let l = lambda_expr(Expr::Field {
            receiver: Box::new(Spanned::new(Expr::Ident("context".into()), Span::DUMMY)),
            field: "UnknownProp".into(),
        });
        // Field 返回非 string 触发 ReturnTypeMismatch，但实际错误应是 Unsupported
        // 用 if 消费：if (context.UnknownProp == null) ...
        // 实际上 eval_field 会直接返回 Err，所以 eval_with_context 应返回 Err
        let result = eval_with_context(l, make_test_generator_context());
        assert!(matches!(
            result,
            Err(EvalError::Unsupported { node, .. })
                if node.contains("GeneratorContext")
        ));
    }

    #[test]
    fn m5_2b_attribute_table_unknown_method_rejected() {
        // context.Attributes.UnknownMethod() → NotInWhitelist
        let l = lambda_expr(Expr::MethodCall {
            receiver: Box::new(Spanned::new(
                Expr::Field {
                    receiver: Box::new(Spanned::new(Expr::Ident("context".into()), Span::DUMMY)),
                    field: "Attributes".into(),
                },
                Span::DUMMY,
            )),
            method: "UnknownMethod".into(),
            args: vec![],
            type_args: vec![],
            params_span: None,
        });
        let result = eval_with_context(l, make_test_generator_context());
        // eval_method_call 先查白名单（早期拒绝），UnknownMethod 不在白名单 → NotInWhitelist
        assert!(matches!(
            result,
            Err(EvalError::NotInWhitelist { receiver_ty, method, .. })
                if receiver_ty == "AttributeTable" && method.as_str() == "UnknownMethod"
        ));
    }

    #[test]
    fn m5_2b_eval_generate_method_with_context_injects_context_local() {
        // Generate 方法体：
        //   var list = new List<string>();
        //   list.Add(context.Symbols.GetTypeName(1));
        //   return list;
        // 注入 context 后应能正常求值，返回 ["MyClass"]
        let list_ty = Spanned::new(
            ast::Type::Named {
                path: vec!["List".into()],
                generics: vec![Spanned::new(
                    ast::Type::Named {
                        path: vec!["string".into()],
                        generics: vec![],
                    },
                    Span::DUMMY,
                )],
            },
            Span::DUMMY,
        );
        let body = Block {
            stmts: vec![
                Spanned::new(
                    Stmt::Let {
                        mutable: false,
                        name: "list".into(),
                        ty: None,
                        init: Some(Spanned::new(
                            Expr::New {
                                ty: list_ty,
                                args: vec![],
                                obj_init: None,
                            },
                            Span::DUMMY,
                        )),
                    },
                    Span::DUMMY,
                ),
                Spanned::new(
                    Stmt::Expr(Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(Spanned::new(
                                Expr::Ident("list".into()),
                                Span::DUMMY,
                            )),
                            method: "Add".into(),
                            args: vec![Spanned::new(
                                Expr::MethodCall {
                                    receiver: Box::new(Spanned::new(
                                        Expr::Field {
                                            receiver: Box::new(Spanned::new(
                                                Expr::Ident("context".into()),
                                                Span::DUMMY,
                                            )),
                                            field: "Symbols".into(),
                                        },
                                        Span::DUMMY,
                                    )),
                                    method: "GetTypeName".into(),
                                    args: vec![Spanned::new(Expr::IntLit(1), Span::DUMMY)],
                                    type_args: vec![],
                                    params_span: None,
                                },
                                Span::DUMMY,
                            )],
                            type_args: vec![],
                            params_span: None,
                        },
                        Span::DUMMY,
                    )),
                    Span::DUMMY,
                ),
            ],
            tail: Some(Box::new(Spanned::new(
                Expr::Ident("list".into()),
                Span::DUMMY,
            ))),
        };
        let w = Whitelist::new();
        let mut e = Evaluator::new(&w);
        let result = e
            .eval_generate_method_with_context(
                &body,
                Some(Ident::from("context")),
                Some(make_test_generator_context()),
            )
            .unwrap();
        // Generate 返回 List<string>，第一个元素应为 "MyClass"
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "MyClass");
    }
}
