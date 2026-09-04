//! RFC 022 Sprint 2b: Runtime Expression tree construction.
//!
//! Replaces the RFC 003 rodata emission path. When codegen encounters
//! `MirRvalue::ExpressionTreeConst`, it recursively generates LLVM IR that
//! constructs the corresponding Arc-side Expression objects (defined in
//! std/Linq/Expressions/nodes.as) at runtime.
//!
//! Node mapping (ExpressionNode → Arc class):
//!   - Constant       → ConstantExpression (IntValue/FloatValue/BoolValue/StringValue)
//!   - Parameter      → ParameterExpression (Name)
//!   - Capture        → CaptureExpression (Name, IntValue/BoolValue/StringValue)
//!   - MemberAccess   → MemberExpression (Object, MemberName, TypeName)
//!   - Binary         → BinaryExpression (Op, Left, Right)
//!   - Unary          → UnaryExpression (Op, Operand)
//!   - Lambda         → LambdaExpression (Parameters: List<ParameterExpression>, Body)
//!   - Call           → MethodCallExpression (MethodName, Target, Arguments: List<Expression>)
//!   - Index          → IndexExpression (Object, Index)
//!   - Conditional    → ConditionalExpression (Test, IfTrue, IfFalse)
//!   - New            → NewExpression (TypeName, ArgValues: List<Expression>)
//!   - Cast           → CastExpression (Expr, TargetType)
//!
//! All constructed objects are heap-allocated via `@calloc` with refcount=1
//! (managed by ARC). Field values are set via GEP + store.
//!
//! A2 (debt repayment): `List<T>` fields (`Parameters`, `Arguments`) are now
//! populated with real `List<T>` objects backed by `rt_list_create` +
//! `rt_list_push`. When the monomorphization is registered in layouts
//! (e.g. `List_ParameterExpression`), `emit_new` is used (proper vtable +
//! ctor stub); otherwise a manual `calloc` + `rt_list_create` fallback
//! constructs the list (e.g. `List_Expression`, which is not instantiated
//! by field-type declarations alone).

use super::*;
use ast::{BinOp, ConstantValue, ExpressionNode, UnaryOp};

/// `ExpressionType` 枚举判别值（与 `std/Arc/Linq/Expressions/ExpressionType.as` 顺序一致）。
mod expr_type_disc {
    pub const CONSTANT: i32 = 0;
    pub const PARAMETER: i32 = 1;
    pub const CAPTURE: i32 = 2;
    pub const MEMBER: i32 = 3;
    pub const INDEX: i32 = 4;
    pub const CONDITIONAL: i32 = 7;
    pub const CALL: i32 = 8;
    pub const NEW: i32 = 9;
    pub const LAMBDA: i32 = 10;
    pub const CAST: i32 = 11;
    // RFC 022 Sprint 2d Slice 0/2：per-operator 节点类型。Binary/Unary 节点
    // 不再携带 `Op` 字符串字段，改以 per-op NodeType 标识具体运算（C# 对齐）。
    // （枚举 5=Binary、6=Unary 为 L1 结构性占位；codegen 发射均为 per-op 判别值。）
    pub const ADD: i32 = 12;
    pub const SUBTRACT: i32 = 13;
    pub const MULTIPLY: i32 = 14;
    pub const DIVIDE: i32 = 15;
    pub const MODULO: i32 = 16;
    pub const EQUAL: i32 = 17;
    pub const NOT_EQUAL: i32 = 18;
    pub const LESS_THAN: i32 = 19;
    pub const LESS_THAN_OR_EQUAL: i32 = 20;
    pub const GREATER_THAN: i32 = 21;
    pub const GREATER_THAN_OR_EQUAL: i32 = 22;
    pub const AND_ALSO: i32 = 23;
    pub const OR_ELSE: i32 = 24;
    pub const AND: i32 = 25;
    pub const OR: i32 = 26;
    pub const NOT: i32 = 27;
    pub const NEGATE: i32 = 28;
    // 位运算 / 移位（ExpressionType.as 末尾追加，保持既有判别值稳定）
    pub const EXCLUSIVE_OR: i32 = 59;
    pub const LEFT_SHIFT: i32 = 60;
    pub const RIGHT_SHIFT: i32 = 61;
}

/// BinOp → per-op `ExpressionType` 判别值（与 ExpressionType.as 顺序一致）。
fn binop_expr_type_disc(op: &BinOp) -> i32 {
    match op {
        BinOp::Add => expr_type_disc::ADD,
        BinOp::Sub => expr_type_disc::SUBTRACT,
        BinOp::Mul => expr_type_disc::MULTIPLY,
        BinOp::Div => expr_type_disc::DIVIDE,
        BinOp::Mod => expr_type_disc::MODULO,
        BinOp::Eq => expr_type_disc::EQUAL,
        BinOp::NotEq => expr_type_disc::NOT_EQUAL,
        BinOp::Lt => expr_type_disc::LESS_THAN,
        BinOp::Le => expr_type_disc::LESS_THAN_OR_EQUAL,
        BinOp::Gt => expr_type_disc::GREATER_THAN,
        BinOp::Ge => expr_type_disc::GREATER_THAN_OR_EQUAL,
        BinOp::And => expr_type_disc::AND_ALSO,
        BinOp::Or => expr_type_disc::OR_ELSE,
        BinOp::BitAnd => expr_type_disc::AND,
        BinOp::BitOr => expr_type_disc::OR,
        BinOp::BitXor => expr_type_disc::EXCLUSIVE_OR,
        BinOp::Shl => expr_type_disc::LEFT_SHIFT,
        BinOp::Shr => expr_type_disc::RIGHT_SHIFT,
    }
}

/// UnaryOp → per-op `ExpressionType` 判别值（与 ExpressionType.as 顺序一致）。
fn unaryop_expr_type_disc(op: &UnaryOp) -> i32 {
    match op {
        UnaryOp::Not => expr_type_disc::NOT,
        UnaryOp::Neg => expr_type_disc::NEGATE,
        UnaryOp::BitNot => expr_type_disc::NOT,
    }
}

impl<'a> FnEmitter<'a> {
    /// Construct an Expression object tree at runtime, returning the root
    /// Expression* pointer (`("ptr", "%tN")`).
    pub(super) fn emit_expression_tree(&mut self, tree: &ast::ExpressionTree) -> TyVal {
        self.emit_expr_node(&tree.root)
    }

    fn emit_expr_node(&mut self, node: &ExpressionNode) -> TyVal {
        match node {
            ExpressionNode::Constant(cv) => self.emit_const_expr_node(cv),
            ExpressionNode::Parameter { name, ty } => {
                self.emit_param_expr_node(name.as_str(), ty.as_str())
            }
            ExpressionNode::Capture { name, ty, local_id } => {
                self.emit_capture_expr_node(name.as_str(), ty.as_str(), *local_id)
            }
            ExpressionNode::MemberAccess { object, member, ty } => {
                let obj = self.emit_expr_node(object);
                self.emit_member_expr_node(&obj.1, member.as_str(), ty.as_str())
            }
            ExpressionNode::Binary { op, left, right } => {
                let l = self.emit_expr_node(left);
                let r = self.emit_expr_node(right);
                self.emit_binary_expr_node(op, &l.1, &r.1)
            }
            ExpressionNode::Unary { op, operand } => {
                let o = self.emit_expr_node(operand);
                self.emit_unary_expr_node(op, &o.1)
            }
            ExpressionNode::Lambda { params, body } => {
                let body_val = self.emit_expr_node(body);
                self.emit_lambda_expr_node(params, &body_val.1)
            }
            ExpressionNode::Call {
                method,
                target,
                args,
            } => {
                let target_val = target
                    .as_ref()
                    .map(|t| self.emit_expr_node(t))
                    .unwrap_or_else(|| ("ptr".into(), "null".into()));
                // A2: build ALL argument nodes (not just arg0) so Args can be
                // populated as a real List<Expression>.
                let arg_vals: Vec<String> = args.iter().map(|a| self.emit_expr_node(a).1).collect();
                self.emit_call_expr_node(method.as_str(), &target_val.1, &arg_vals)
            }
            ExpressionNode::Index { object, index } => {
                let obj = self.emit_expr_node(object);
                let idx = self.emit_expr_node(index);
                self.emit_index_expr_node(&obj.1, &idx.1)
            }
            ExpressionNode::Conditional {
                test,
                if_true,
                if_false,
            } => {
                let t = self.emit_expr_node(test);
                let tt = self.emit_expr_node(if_true);
                let ff = self.emit_expr_node(if_false);
                // 结果类型取真分支推断，供嵌套 ==/!= 走 EvalString/EvalBool。
                let result_ty = if_true.inferred_type_name();
                self.emit_conditional_expr_node(&t.1, &tt.1, &ff.1, result_ty.as_str())
            }
            ExpressionNode::New { type_name, args } => {
                let arg_vals: Vec<String> = args.iter().map(|a| self.emit_expr_node(a).1).collect();
                self.emit_new_expr_node(type_name.as_str(), &arg_vals)
            }
            ExpressionNode::Cast {
                operand,
                target_type,
            } => {
                let o = self.emit_expr_node(operand);
                self.emit_cast_expr_node(&o.1, target_type.as_str())
            }
            // RFC 022 §2.2.10 L2/L3 节点（28 变体）不进入 codegen 发射路径
            // （设计原则 4：codegen 仅消费 L1 12 变体）。L2/L3 节点用于编译期
            // 扩展（D10.6 解释器/Source Generator），由 typeck 内部构造 `Value`
            // 处理语义，不应触发 codegen 发射。若意外到达此处，说明上游路径有 bug
            // （如把 L2/L3 节点塞进了 `MirRvalue::ExpressionTreeConst`），需排查。
            _ => panic!(
                "codegen emit_expr_node received non-L1 ExpressionNode: {:?} \
                 (L2/L3 nodes must not enter codegen emission path; see RFC 022 §2.2.10)",
                node
            ),
        }
    }

    // ---- Leaf nodes ----

    fn emit_const_expr_node(&mut self, cv: &ConstantValue) -> TyVal {
        let class = "ConstantExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        // __ctor_* 对 Expression 派生类目前为空桩；NodeType 须在此显式写入。
        self.set_expr_node_type(&obj, class, expr_type_disc::CONSTANT);
        let type_name = match cv {
            ConstantValue::Int(n) => {
                // Arc `int` is i32 (types.rs:103). Store as i32 to match the
                // declared field type — writing i64 overflows into the adjacent
                // FloatValue field, corrupting EvalInt reads.
                self.set_expr_field(&obj, class, "IntValue", "i32", &(*n as i32).to_string());
                // Also populate StringValue with the decimal representation so
                // runtime translators can emit text without needing int→string.
                let s = n.to_string();
                let str_global = self.intern_string(&s);
                self.set_expr_field(&obj, class, "StringValue", "ptr", &str_global);
                "int"
            }
            ConstantValue::Float(f) => {
                self.set_expr_field(&obj, class, "FloatValue", "double", &format!("{f:e}"));
                let s = format!("{f}");
                let str_global = self.intern_string(&s);
                self.set_expr_field(&obj, class, "StringValue", "ptr", &str_global);
                "double"
            }
            ConstantValue::Bool(b) => {
                self.set_expr_field(&obj, class, "BoolValue", "i1", if *b { "1" } else { "0" });
                let s = if *b { "TRUE" } else { "FALSE" };
                let str_global = self.intern_string(s);
                self.set_expr_field(&obj, class, "StringValue", "ptr", &str_global);
                "bool"
            }
            ConstantValue::String(s) => {
                let str_global = self.intern_string(s);
                self.set_expr_field(&obj, class, "StringValue", "ptr", &str_global);
                // Mark as string so runtime translators wrap it in single quotes.
                self.set_expr_field(&obj, class, "IsString", "i1", "1");
                "string"
            }
        };
        // TypeName 供 BinaryExpression ==/!= 区分 bool/int 操作数求值路径。
        let tn_global = self.intern_string(type_name);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        ("ptr".into(), obj)
    }

    fn emit_param_expr_node(&mut self, name: &str, ty: &str) -> TyVal {
        let class = "ParameterExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::PARAMETER);
        let str_global = self.intern_string(name);
        self.set_expr_field(&obj, class, "Name", "ptr", &str_global);
        let tn_global = self.intern_string(ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        // RFC 018 M3: fill Type field when type is known (class types only for now;
        // primitives need rt_type_by_name ABI call infrastructure not yet available).
        self.try_fill_expr_type_field(&obj, class, ty);
        ("ptr".into(), obj)
    }

    fn emit_capture_expr_node(&mut self, name: &str, ty: &str, local_id: i32) -> TyVal {
        let class = "CaptureExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::CAPTURE);
        let str_global = self.intern_string(name);
        self.set_expr_field(&obj, class, "Name", "ptr", &str_global);
        let tn_global = self.intern_string(ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);

        // Value snapshot: load the captured variable's current value from its
        // MIR local slot and store into IntValue / BoolValue / StringValue.
        // local_id < 0 means typeck-only path (no value snapshot available).
        if local_id >= 0 {
            let local_ptr = format!("%v{}", local_id);
            match ty {
                "int" => {
                    // CaptureExpression.IntValue is `int` (i32); int locals are i32.
                    let loaded = self.fresh_temp();
                    self.emit(&format!("  {loaded} = load i32, ptr {local_ptr}"));
                    self.set_expr_field(&obj, class, "IntValue", "i32", &loaded);
                }
                "bool" => {
                    // bool locals are i1; class field BoolValue is C ABI i32.
                    let loaded = self.fresh_temp();
                    self.emit(&format!("  {loaded} = load i1, ptr {local_ptr}"));
                    let widened = self.fresh_temp();
                    self.emit(&format!("  {widened} = zext i1 {loaded} to i32"));
                    self.set_expr_field(&obj, class, "BoolValue", "i32", &widened);
                }
                "string" => {
                    let loaded = self.fresh_temp();
                    self.emit(&format!("  {loaded} = load ptr, ptr {local_ptr}"));
                    self.set_expr_field(&obj, class, "StringValue", "ptr", &loaded);
                }
                _ => {
                    // Unknown type: best-effort int snapshot (i32 field).
                    let loaded = self.fresh_temp();
                    self.emit(&format!("  {loaded} = load i32, ptr {local_ptr}"));
                    self.set_expr_field(&obj, class, "IntValue", "i32", &loaded);
                }
            }
        }
        // RFC 018 M3: fill Type field.
        self.try_fill_expr_type_field(&obj, class, ty);
        ("ptr".into(), obj)
    }

    // ---- Composite nodes ----

    fn emit_member_expr_node(&mut self, object_val: &str, member: &str, ty: &str) -> TyVal {
        let class = "MemberExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::MEMBER);
        self.set_expr_field(&obj, class, "Object", "ptr", object_val);
        let str_global = self.intern_string(member);
        self.set_expr_field(&obj, class, "MemberName", "ptr", &str_global);
        // TypeName 供 BinaryExpression ==/!= 在 Member==Member 两侧走 bool/int 分派。
        let tn_global = self.intern_string(ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        ("ptr".into(), obj)
    }

    fn emit_binary_expr_node(&mut self, op: &BinOp, left_val: &str, right_val: &str) -> TyVal {
        let class = "BinaryExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        // RFC 022 Sprint 2d Slice 2: per-op NodeType（C# 对齐）；不再写 Op 字符串字段。
        self.set_expr_node_type(&obj, class, binop_expr_type_disc(op));
        self.set_expr_field(&obj, class, "Left", "ptr", left_val);
        self.set_expr_field(&obj, class, "Right", "ptr", right_val);
        // 关系/逻辑运算结果为 bool，供嵌套 ==/!= 走 EvalBool。
        let result_ty = match op {
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => "bool",
            _ => "int",
        };
        let tn_global = self.intern_string(result_ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        ("ptr".into(), obj)
    }

    fn emit_unary_expr_node(&mut self, op: &UnaryOp, operand_val: &str) -> TyVal {
        let class = "UnaryExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        // RFC 022 Sprint 2d Slice 2: per-op NodeType（C# 对齐）；不再写 Op 字符串字段。
        self.set_expr_node_type(&obj, class, unaryop_expr_type_disc(op));
        self.set_expr_field(&obj, class, "Operand", "ptr", operand_val);
        let result_ty = match op {
            UnaryOp::Not => "bool",
            UnaryOp::Neg => "int",
            UnaryOp::BitNot => "int",
        };
        let tn_global = self.intern_string(result_ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        ("ptr".into(), obj)
    }

    fn emit_lambda_expr_node(
        &mut self,
        params: &[(ast::Ident, ast::SmolStr)],
        body_val: &str,
    ) -> TyVal {
        let class = "LambdaExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::LAMBDA);
        // A2: populate Parameters with a real List<ParameterExpression>.
        let param_vals: Vec<String> = params
            .iter()
            .map(|(name, ty)| self.emit_param_expr_node(name.as_str(), ty.as_str()).1)
            .collect();
        let params_obj = self.emit_list_of_class_refs("List_ParameterExpression", &param_vals);
        self.set_expr_field(&obj, class, "Parameters", "ptr", &params_obj);
        self.set_expr_field(&obj, class, "Body", "ptr", body_val);
        ("ptr".into(), obj)
    }

    fn emit_call_expr_node(
        &mut self,
        method: &str,
        target_val: &str,
        arg_vals: &[String],
    ) -> TyVal {
        let class = "MethodCallExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::CALL);
        let method_global = self.intern_string(method);
        self.set_expr_field(&obj, class, "MethodName", "ptr", &method_global);
        self.set_expr_field(&obj, class, "Target", "ptr", target_val);
        // A2: populate Arguments with a real List<Expression>.
        let args_obj = self.emit_list_of_class_refs("List_Expression", arg_vals);
        self.set_expr_field(&obj, class, "Arguments", "ptr", &args_obj);
        // Arg0: first argument as a fixed slot. Kept for backward compat with
        // the runtime evaluator (GetArg0 accessor), which predates List<T>.
        let arg0 = arg_vals.first().map(String::as_str).unwrap_or("null");
        self.set_expr_field(&obj, class, "Arg0", "ptr", arg0);
        ("ptr".into(), obj)
    }

    // ---- RFC 022 新增节点 ----

    /// IndexExpression: 索引访问 `arr[i]` / `dict[key]`。
    fn emit_index_expr_node(&mut self, object_val: &str, index_val: &str) -> TyVal {
        let class = "IndexExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::INDEX);
        self.set_expr_field(&obj, class, "Object", "ptr", object_val);
        self.set_expr_field(&obj, class, "Index", "ptr", index_val);
        ("ptr".into(), obj)
    }

    /// ConditionalExpression: 三元条件 `test ? ifTrue : ifFalse`。
    fn emit_conditional_expr_node(
        &mut self,
        cond_val: &str,
        then_val: &str,
        else_val: &str,
        result_ty: &str,
    ) -> TyVal {
        let class = "ConditionalExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::CONDITIONAL);
        self.set_expr_field(&obj, class, "Test", "ptr", cond_val);
        self.set_expr_field(&obj, class, "IfTrue", "ptr", then_val);
        self.set_expr_field(&obj, class, "IfFalse", "ptr", else_val);
        // TypeName 供 BinaryExpression ==/!= 在三元结果上分派 EvalString/EvalBool。
        let tn_global = self.intern_string(result_ty);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        ("ptr".into(), obj)
    }

    /// NewExpression: 对象构造 `new T(args...)`。
    /// ArgValues 用 List<Expression> 承载；ArgNames 留空（IR 不携带参数名）。
    fn emit_new_expr_node(&mut self, type_name: &str, arg_vals: &[String]) -> TyVal {
        let class = "NewExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::NEW);
        let tn_global = self.intern_string(type_name);
        self.set_expr_field(&obj, class, "TypeName", "ptr", &tn_global);
        let args_obj = self.emit_list_of_class_refs("List_Expression", arg_vals);
        self.set_expr_field(&obj, class, "ArgValues", "ptr", &args_obj);
        // RFC 018 M3: fill Type field.
        self.try_fill_expr_type_field(&obj, class, type_name);
        ("ptr".into(), obj)
    }

    /// CastExpression: 类型转换 `(T)operand`。
    fn emit_cast_expr_node(&mut self, operand_val: &str, target_type: &str) -> TyVal {
        let class = "CastExpression";
        let (_, obj) = self.emit_new(class, &[], &[]);
        self.set_expr_node_type(&obj, class, expr_type_disc::CAST);
        self.set_expr_field(&obj, class, "Expr", "ptr", operand_val);
        let tt_global = self.intern_string(target_type);
        self.set_expr_field(&obj, class, "TargetType", "ptr", &tt_global);
        // RFC 018 M3: fill Type field.
        self.try_fill_expr_type_field(&obj, class, target_type);
        ("ptr".into(), obj)
    }

    /// A2: Construct a `List<T>` of class-type elements (each element is an
    /// ARC-managed `ptr`) and push every item in `items` into it.
    ///
    /// When `list_class` is registered in `layouts.classes` (e.g.
    /// `List_ParameterExpression`, instantiated wherever a `new
    /// List<ParameterExpression>()` appears in source), `emit_new` is used —
    /// this yields a proper vtable + the `__ctor` stub that calls
    /// `rt_list_create`.
    ///
    /// Otherwise (e.g. `List_Expression`, which field-type declarations alone
    /// do not instantiate), fall back to a manual `calloc(24)` +
    /// `rt_list_create`. The resulting object lacks an itable, but translators
    /// access `Args` via direct `Get(i)`/`Count` calls (not virtual dispatch),
    /// so this is safe.
    ///
    /// In both cases the handle lives at offset 16 (after the 16-byte
    /// `ArcHeader`); we load it and call `rt_list_push` per element.
    fn emit_list_of_class_refs(&mut self, list_class: &str, items: &[String]) -> String {
        let obj = if self.layouts.classes.contains_key(list_class) {
            // Registered monomorphization: emit_new → vtable + __ctor stub
            // (which calls rt_list_create and stores the handle at offset 16).
            let (_, obj) = self.emit_new(list_class, &[], &[]);
            obj
        } else {
            // Uninstantiated: manual calloc. Layout = ArcHeader(16) + handle(8).
            let obj = self.fresh_temp();
            self.emit(&format!("{obj} = call ptr @calloc(i64 1, i64 24)"));
            self.emit(&format!("store i32 1, ptr {obj}")); // refcount = 1
                                                           // Class-type elements: elem_size=8, ARC-managed (inc/dec callbacks).
            let handle = self.fresh_temp();
            self.emit(&format!(
                "{handle} = call ptr @rt_list_create(i32 8, ptr null, ptr @rt_list_arc_inc_ref, ptr @rt_list_arc_dec_ref)"
            ));
            let hp = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
            ));
            self.emit(&format!("store ptr {handle}, ptr {hp}"));
            obj
        };

        // Load handle from offset 16 and push each element.
        let hp = self.fresh_temp();
        self.emit(&format!(
            "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
        ));
        let handle = self.fresh_temp();
        self.emit(&format!("{handle} = load ptr, ptr {hp}"));
        for item in items {
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = alloca ptr"));
            self.emit(&format!("store ptr {item}, ptr {slot}"));
            self.emit(&format!(
                "call void @rt_list_push(ptr {handle}, ptr {slot})"
            ));
        }
        obj
    }

    // ---- Helper: set a field on a heap-allocated Expression object ----

    /// RFC 018 M3: 尝试为 Expression 节点填充 `Type` 强类型字段。
    ///
    /// 当前仅对 class 类型（有 `@.typeinfo.{T}` 全局常量的类型）创建 RuntimeType
    /// 实例并设置到 `Type` 字段。对于基元类型（int/string/double/bool 等），
    /// 因尚未实现 `rt_type_by_name` ABI 的 codegen 调用路径，暂时返回而不填充。
    ///
    /// `type_name` 为 Arc 语言中的类型名（如 "Point"、"Customer" 等）。
    fn try_fill_expr_type_field(&mut self, obj: &str, class: &str, type_name: &str) {
        if let Some(rt_ptr) = self.try_emit_typeof_as_runtime_type(type_name) {
            self.set_expr_field(obj, class, "Type", "ptr", &rt_ptr);
        }
        // 基元类型暂不填充 Type 字段（null），待 rt_type_by_name ABI 对接后补齐。
    }

    /// Write `Expression.NodeType` — required because Expression `__ctor_*` stubs
    /// currently do not run field initializers from `.as` constructors.
    fn set_expr_node_type(&mut self, obj: &str, class: &str, disc: i32) {
        self.set_expr_field(obj, class, "NodeType", "i32", &disc.to_string());
    }

    fn set_expr_field(&mut self, obj: &str, class: &str, field: &str, ty_str: &str, val_str: &str) {
        let (offset, _) = self.field_info(class, field);
        let addr = self.fresh_temp();
        self.emit(&format!(
            "{addr} = getelementptr inbounds i8, ptr {obj}, i32 {offset}"
        ));
        self.emit(&format!("store {ty_str} {val_str}, ptr {addr}"));
    }
}

pub(super) fn binop_to_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

pub(super) fn unaryop_to_str(op: &UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "!",
        UnaryOp::Neg => "-",
        UnaryOp::BitNot => "~",
    }
}
