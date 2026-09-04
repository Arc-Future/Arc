//! RFC 009 M4-4 / M4-5 / M5-3 / M5-2b: 受限求值器白名单。
//!
//! 编译器维护一个「可在受限求值器中调用的方法」白名单（RFC 009 D10.2）。
//! 白名单外的方法调用一律编译错误。
//!
//! # 设计
//!
//! 白名单是 **闭集**：仅编译器内置的纯函数可入。M4-4 落地 StringBuilder
//! 系列；M4-5 扩展 `Expression` 类层次访问器（`Expression.GetLeft` /
//! `ConstantExpression.GetStringValue` 等，详见 RFC 022 Expression 类层次）；
//! M5-3 扩展 `List<string>` 构造与 `Add` 方法（RFC 009 D13.6 共享求值器）；
//! M5-2b 扩展 `GeneratorContext` 拦截器方法（`AttributeTable.GetDefIdAt` /
//! `AttributeTable.GetAttrs` / `AttributeList.Has` / `SymbolTable.GetTypeName` /
//! `SymbolTable.GetMemberName`，RFC 012 D13.7 GeneratorContext 拦截）。
//! 未来扩展须通过 RFC 增补。
//!
//! # 与 evaluator.rs 的关系
//!
//! - `Whitelist` 是 **声明**：列出「允许调用」的方法集合，供诊断与
//!   早期拒绝。
//! - `Evaluator` 是 **执行**：对白名单内方法硬编码实现求值语义。若某
//!   方法在白名单内但 evaluator 未实现，求值器返回 `EvalError::Unsupported`。
//!
//! # M4-5: Expression 访问器
//!
//! RFC 022 的 Expression 类层次使用虚方法访问器（`GetLeft` / `GetRight` /
//! `GetMethodName` / `GetStringValue` 等）暴露子节点结构，避免 `is/as`
//! 下转。M4-5 将这些虚方法加入白名单：
//! - `Expression.GetLeft/GetRight/GetOperand/GetTarget/GetArg0/GetBody`
//! - `Expression.GetMethodName/GetMember/GetTargetType/GetName`
//! - `Expression.GetStringValue/IsStringConstant`
//! - `Expression.EvalInt/EvalBool/EvalString`
//! - `Expression.NodeType/TypeName`（字段属性访问）
//!
//! 实际 Expression 对象的注入（来自 attribute 参数解析）将在 M4-7
//! 两轮 typeck 拆分中落地；M4-5 仅扩展白名单 + 提供求值器基础设施。
//!
//! # M5-3: List<string> 扩展（RFC 009 D13.6）
//!
//! Source Generator 的 `Generate(GeneratorContext) -> List<string>` 方法
//! 在受限求值器中求值时需要构造与累积字符串列表。M5-3 将 `List`
//! 加入 newable 集合（仅允许 `List<string>` 实参，由 evaluator 校验），
//! 将 `List.Add` 加入方法白名单。
//!
//! # M5-2b: GeneratorContext 拦截器方法（RFC 012 D13.7）
//!
//! Source Generator 的 `Generate(GeneratorContext context)` 方法体中
//! 访问 `context.Attributes` / `context.Symbols` 返回 `AttributeTable` /
//! `SymbolTable` 占位值，再调用其方法查询编译期数据：
//! - `AttributeTable.GetDefIdAt(int index) -> int`：按插入顺序返回第 i 个符号的 DefId
//! - `AttributeTable.GetAttrs(int defId) -> AttributeList`：取该符号的属性列表
//! - `AttributeList.Has(string name) -> bool`：判断是否标有指定属性（C# 风格
//!   名称省略 `Attribute` 后缀匹配）
//! - `SymbolTable.GetTypeName(int defId) -> string`：反查 DefId 对应的类型名
//! - `SymbolTable.GetMemberName(int defId) -> string`：反查 DefId 对应的成员名
//!   （仅方法成员有值，类/字段成员为空串）
//!
//! `GeneratorContext` 本身的字段访问（`Attributes` / `Symbols` / `SourceFiles`）
//! 与 `AttributeTable.Count` 由 evaluator 在 `eval_field` 中拦截，**不走**
//! 方法白名单。

use std::collections::HashSet;

/// 受限求值器白名单。
///
/// 列出可在 `Func<string>` 委托体或 `Generate` 方法体中调用的
/// `(receiver_type, method_name)` 组合。求值器在调用前查询 `allows`
/// 决定是否允许。
///
/// # 何时查询
///
/// - 求值 `Expr::MethodCall` 时：先查白名单，未通过则报
///   `EvalError::NotInWhitelist`；通过后由 evaluator 分支决定具体执行
///   （已实现 → 求值；未实现 → `EvalError::Unsupported`）。
/// - 求值 `Expr::New` 时：`StringBuilder` 与 `List<string>` 被允许。
#[derive(Clone, Debug)]
pub struct Whitelist {
    /// 允许的 `(receiver_type, method_name)` 对。
    methods: HashSet<(String, String)>,
    /// 允许 `new T()` 构造的类型集合。
    newable_types: HashSet<String>,
}

impl Default for Whitelist {
    fn default() -> Self {
        let mut w = Whitelist {
            methods: HashSet::new(),
            newable_types: HashSet::new(),
        };
        // M4-4: StringBuilder 系列（v1.0 完整 API 表面：构造重载 + Append 全类型 + 修改 + 索引器 + 容量/输出）
        for m in [
            "Append",
            "AppendLine",
            "Clear",
            "Insert",
            "Remove",
            "Replace",
            "EnsureCapacity",
            "ToString",
            "get_Length",
            "get_Capacity",
            "get_Item",
            "set_Item",
        ] {
            w.methods
                .insert(("StringBuilder".to_string(), m.to_string()));
        }
        w.newable_types.insert("StringBuilder".to_string());

        // RFC 009 M5-3: List<string> 系列（D13.6 扩展共享白名单）
        // 仅 `List<string>` 由 evaluator 校验泛型实参后允许构造；
        // 其他 `List<T>` 实参（如 `List<int>`）在 eval_new 阶段被拒绝。
        w.methods.insert(("List".to_string(), "Add".to_string()));
        w.newable_types.insert("List".to_string());

        // M4-5: Expression 类层次访问器（RFC 022）。
        //
        // Expression 基类与其派生类共享同一组虚方法访问器（通过 vtable
        // 分派）。白名单按 `receiver_type` 注册——所有派生类（ConstantExpression
        // / MemberExpression / BinaryExpression 等）的方法调用都按基类
        // `Expression` 注册，简化维护。
        //
        // 注意：求值器目前仅实现 `GetStringValue` / `GetMember` / `GetLeft` /
        // `GetRight` / `GetMethodName` / `NodeType` / `TypeName`，其余访问器
        // 即便在白名单内也返回 `Unsupported`，待实际使用场景驱动补全。
        for m in [
            // 子节点访问器（返回 Expression 或 null）
            "GetLeft",
            "GetRight",
            "GetOperand",
            "GetTarget",
            "GetArg0",
            "GetBody",
            "GetCond",
            "GetThen",
            "GetElse",
            "GetExpr",
            // 字符串/标量访问器
            "GetMethodName",
            "GetMember",
            "GetTargetType",
            "GetName",
            "GetStringValue",
            "IsStringConstant",
            // 内存执行后端（M4-5 暂不实现，仅入白名单）
            "EvalInt",
            "EvalBool",
            "EvalString",
            // ToString 通用方法（与 StringBuilder 共用名，但 receiver 不同）
            "ToString",
        ] {
            w.methods.insert(("Expression".to_string(), m.to_string()));
        }

        // RFC 009 M5-2b: GeneratorContext 拦截器方法（D13.7）
        //
        // Source Generator 的 `Generate(GeneratorContext)` 方法体中通过
        // `context.Attributes.GetDefIdAt(i)` / `.GetAttrs(defId)` /
        // `attributeList.Has(name)` / `context.Symbols.GetTypeName(defId)`
        // 查询编译期数据。evaluator 在 `eval_method_call` 中拦截这些
        // receiver 类型（AttributeTable / AttributeList / SymbolTable），
        // 返回真实数据。
        //
        // 注意：`GeneratorContext.{Attributes, Symbols, SourceFiles}` 字段
        // 访问与 `AttributeTable.Count` 字段访问由 `eval_field` 拦截，
        // **不走方法白名单**——只注册真正的 method call。
        for (ty, m) in [
            ("AttributeTable", "GetDefIdAt"),
            ("AttributeTable", "GetAttrs"),
            ("AttributeList", "Has"),
            ("AttributeList", "GetArgCount"),
            ("AttributeList", "GetArg"),
            ("AttributeList", "GetNamedArg"),
            ("SymbolTable", "GetTypeName"),
            ("SymbolTable", "GetMemberName"),
        ] {
            w.methods.insert((ty.to_string(), m.to_string()));
        }

        // Phase 2 序列化体系：TypeTable 方法
        for (ty, m) in [
            ("TypeTable", "GetTypeName"),
            ("TypeTable", "GetKind"),
            ("TypeTable", "GetFieldCount"),
            ("TypeTable", "GetFieldName"),
            ("TypeTable", "GetFieldType"),
            ("TypeTable", "GetBaseType"),
        ] {
            w.methods.insert((ty.to_string(), m.to_string()));
        }

        w
    }
}

impl Whitelist {
    pub fn new() -> Self {
        Self::default()
    }

    /// 查询 `(receiver_type, method_name)` 是否在白名单内。
    pub fn allows(&self, receiver_type: &str, method: &str) -> bool {
        self.methods
            .contains(&(receiver_type.to_string(), method.to_string()))
    }

    /// 查询类型是否允许 `new T()` 构造。
    pub fn allows_new(&self, type_name: &str) -> bool {
        self.newable_types.contains(type_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stringbuilder_methods_in_whitelist() {
        let w = Whitelist::new();
        // Core methods
        assert!(w.allows("StringBuilder", "Append"));
        assert!(w.allows("StringBuilder", "AppendLine"));
        assert!(w.allows("StringBuilder", "ToString"));
        assert!(w.allows("StringBuilder", "Clear"));
        // Mutation methods (v1.0)
        assert!(w.allows("StringBuilder", "Insert"));
        assert!(w.allows("StringBuilder", "Remove"));
        assert!(w.allows("StringBuilder", "Replace"));
        assert!(w.allows("StringBuilder", "EnsureCapacity"));
        // Properties (v1.0)
        assert!(w.allows("StringBuilder", "get_Length"));
        assert!(w.allows("StringBuilder", "get_Capacity"));
        assert!(w.allows("StringBuilder", "get_Item"));
        assert!(w.allows("StringBuilder", "set_Item"));
    }

    #[test]
    fn stringbuilder_newable() {
        let w = Whitelist::new();
        assert!(w.allows_new("StringBuilder"));
        // RFC 009 M5-3: List 已加入 newable 集合（泛型实参由 evaluator 校验）
        assert!(w.allows_new("List"));
        assert!(!w.allows_new("Dictionary"));
    }

    #[test]
    fn list_add_in_whitelist() {
        // RFC 009 M5-3: List.Add 已加入方法白名单
        let w = Whitelist::new();
        assert!(w.allows("List", "Add"));
        // 其他 List 方法（如 RemoveAt、Sort）不在白名单
        assert!(!w.allows("List", "RemoveAt"));
        assert!(!w.allows("List", "Sort"));
    }

    #[test]
    fn unknown_methods_not_in_whitelist() {
        let w = Whitelist::new();
        assert!(!w.allows("StringBuilder", "Sort"));
        assert!(!w.allows("Console", "WriteLine"));
        assert!(!w.allows("File", "ReadAllText"));
    }

    /// RFC 009 M5-2b: AttributeTable 拦截方法在白名单内
    #[test]
    fn m5_2b_attribute_table_methods_in_whitelist() {
        let w = Whitelist::new();
        assert!(w.allows("AttributeTable", "GetDefIdAt"));
        assert!(w.allows("AttributeTable", "GetAttrs"));
        // 其他 AttributeTable 方法（如 Count）由 eval_field 拦截，不走白名单
        // 不应被误允许
        assert!(!w.allows("AttributeTable", "Sort"));
        assert!(!w.allows("AttributeTable", "Remove"));
    }

    /// RFC 012 M5-2b: AttributeList.Has 在白名单内
    #[test]
    fn m5_2b_attribute_list_has_in_whitelist() {
        let w = Whitelist::new();
        assert!(w.allows("AttributeList", "Has"));
        // AttributeList 其他方法不在白名单
        assert!(!w.allows("AttributeList", "Add"));
        assert!(!w.allows("AttributeList", "Get"));
    }

    /// RFC 012 M5-2b: SymbolTable.GetTypeName / GetMemberName 在白名单内
    #[test]
    fn m5_2b_symbol_table_get_type_name_in_whitelist() {
        let w = Whitelist::new();
        assert!(w.allows("SymbolTable", "GetTypeName"));
        assert!(w.allows("SymbolTable", "GetMemberName"));
        // SymbolTable 其他方法不在白名单
        assert!(!w.allows("SymbolTable", "GetDefId"));
        assert!(!w.allows("SymbolTable", "Count"));
    }

    /// RFC 012 M5-2b: GeneratorContext 与 AttributeTable.Count 是字段访问
    /// （eval_field 拦截），不应进入方法白名单
    #[test]
    fn m5_2b_field_access_not_in_method_whitelist() {
        let w = Whitelist::new();
        // GeneratorContext.{Attributes,Symbols,SourceFiles} 走 eval_field
        assert!(!w.allows("GeneratorContext", "Attributes"));
        assert!(!w.allows("GeneratorContext", "Symbols"));
        assert!(!w.allows("GeneratorContext", "SourceFiles"));
        // AttributeTable.Count 走 eval_field
        assert!(!w.allows("AttributeTable", "Count"));
    }
}
