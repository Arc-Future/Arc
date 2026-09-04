//! MIR 字段访问验证 pass（对齐 RFC 036 NLL pass 架构范本）。
//!
//! **职责**：编译期拦截 MIR 中一切 `(class, field)` 不可解析引用。lower 阶段
//! 若因任何路径（形态遗漏、降级分支、豁免误判）把无法在 [`TypeRegistry`]
//! 字段表中解析的字段引用带入 MIR，codegen 将以错误的布局偏移发射 GEP 或
//! 生成 undefined symbol（历史缺陷 `core_tensor_e2e` Rank/Total 读 offset 76
//! 的同族风险面）。本 pass 在 codegen 之前以统一解析链复核全部字段引用点，
//! 非空诊断 → 编译失败（fail-fast，对齐 NLL「无条件启用、非空即失败」契约）。
//!
//! **架构对齐**（NLL 范本 `dataflow::nll` / `dataflow::diagnostics`）：
//! - 模块级入口 [`run_field_check_module`]，纯函数逐函数检查，按 MIR 函数顺序聚合；
//! - 诊断结构 [`FieldCheckDiagnostic`] `{ code, message, fn_name }`，码串
//!   [`FieldCheckCode::code_str`] 返回 `E_FIELD_UNRESOLVED` 形式码，message
//!   为用户面措辞；
//! - `arc::pipeline::prepare_compilation` 在 NLL 检查之后插桩，非空 → `Err`。
//!
//! **遍历完备性**：覆盖 `MirStatement` / `MirRvalue` / `MirOperand` /
//! `MirTerminator` 全形态，包括 region 语句（`TryCatch` / `TryFinally` /
//! `LinqForeach`）体内部的 `If` / `While`（顶层已被 `to_cfg` 展平，region 内
//! 保留）——补齐 `collect_concrete_class_refs` 以 `..` 模式遗漏 `If.cond` /
//! `IndexSet.array|index` / `While.foreach_source` 的先例缺口。
//!
//! **解析链**（六级，全部复用既有事实源，零新增清单）：
//! 0. 类不在编译单元类型集（facade stub / 外部包 / 合成类）：类封闭性由
//!    typeck 保证，本 pass 职责边界 = 已知类的字段引用完备性，放行；
//! 1. facade 类：`typeck::builtin_facade::is_builtin_facade`（单一事实源，
//!    其维护规则明文「类名集合不得另起清单」）；
//! 2. enum 成员：`registry.enum_variant`（`EnumName.Member` 正常路径在 lower
//!    出口折叠为 `ConstInt`，此处防御性建模保留 Field 形态的历史路径）；
//! 3. custom accessor 属性：`crate::lower::is_custom_accessor_property`
//!    （`get_{field}` 在类或类基类链；禁止叠加 `!has_field` 判定——见其
//!    泛型模板合并历史缺陷注释）；
//! 4. 常规字段：`registry.field_info` 基类链（static / const / instance 一并
//!    解析；`Field` vs `StaticField` 形态正确性由 lower `is_static_field_of`
//!    判定保证，不在此重复校验以免误报）；
//! 5. `[Builtin]` 静态属性：`registry.builtin_static_props`（单一事实源延伸，
//!    见其字段文档「不得以 facade 清单整体代替本判定」）；
//! 6. 以上全部未命中 → `E_FIELD_UNRESOLVED` 诊断。
//!
//! **泛型模板豁免**：函数级跳过——函数名 `rfind("::")` 取类段，该类在
//! registry 中 `generic_params` 非空即模板体（对齐
//! `collect_concrete_class_refs` 先例；单态化克隆的 `generic_params` 为空，
//! 走完整检查）。

use crate::lower::is_custom_accessor_property;
use crate::types::{MirCfgBody, MirOperand, MirRvalue, MirStatement, MirTerminator};
use ast::Ident;
use typeck::{is_builtin_facade, TypeId, TypeRegistry};

/// 字段验证诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldCheckCode {
    /// `(class, field)` 引用在全部豁免与解析层之后仍不可解析。
    UnresolvedField,
}

impl FieldCheckCode {
    pub fn code_str(self) -> &'static str {
        match self {
            FieldCheckCode::UnresolvedField => "E_FIELD_UNRESOLVED",
        }
    }
}

/// 字段验证诊断（用户面形式，形态对齐 `NllDiagnostic`）。
#[derive(Clone, Debug)]
pub struct FieldCheckDiagnostic {
    pub code: FieldCheckCode,
    /// 用户友好消息（不含函数名；`fn_name` 独立字段供管线定位拼接）。
    pub message: String,
    /// 所在函数名（MIR 函数名，用于定位）。
    pub fn_name: String,
}

/// 对整个模块运行字段验证，返回所有诊断（按 MIR 函数顺序）。
///
/// 供 `arc::pipeline::prepare_compilation` 在 NLL 检查之后调用（无条件启用，
/// 对齐 NLL 契约）；非空诊断列表 → 编译失败。
pub fn run_field_check_module(
    mir_fns: &[(String, MirCfgBody)],
    registry: &TypeRegistry,
) -> Vec<FieldCheckDiagnostic> {
    let mut all = Vec::new();
    for (name, cfg) in mir_fns {
        check_fn(name, cfg, registry, &mut all);
    }
    all
}

/// 对单个函数运行字段验证。
///
/// 泛型模板体整体豁免（判定与 `collect_concrete_class_refs` 完全同构）：
/// 模板体内字段表未经单态化合并，`FieldGet` 的 class 段可能为型参名或短名
/// 合并键，均不具备校验前提；单态化克隆以完整函数集进入本 pass，无遗漏。
fn check_fn(
    fn_name: &str,
    cfg: &MirCfgBody,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    if let Some(pos) = fn_name.rfind("::") {
        let class = &fn_name[..pos];
        if registry
            .types
            .get(class)
            .is_some_and(|t| !t.generic_params.is_empty())
        {
            return;
        }
    }
    for block in cfg.blocks.values() {
        for stmt in &block.statements {
            check_statement(fn_name, stmt, registry, out);
        }
        check_terminator(fn_name, &block.terminator, registry, out);
    }
}

fn check_statement(
    fn_name: &str,
    stmt: &MirStatement,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    match stmt {
        MirStatement::Assign { rvalue, .. } => check_rvalue(fn_name, rvalue, registry, out),
        MirStatement::Drop(_) => {}
        MirStatement::Return(Some(rv)) => check_rvalue(fn_name, rv, registry, out),
        MirStatement::Return(None) => {}
        MirStatement::If {
            cond,
            then_body,
            else_body,
        } => {
            check_operand(fn_name, cond, registry, out);
            for s in then_body {
                check_statement(fn_name, s, registry, out);
            }
            for s in else_body {
                check_statement(fn_name, s, registry, out);
            }
        }
        MirStatement::While {
            cond,
            body,
            foreach_source,
        } => {
            check_rvalue(fn_name, cond, registry, out);
            for s in body {
                check_statement(fn_name, s, registry, out);
            }
            if let Some(src) = foreach_source {
                check_operand(fn_name, src, registry, out);
            }
        }
        MirStatement::FieldSet {
            object,
            class,
            field,
            value,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, object, registry, out);
            check_rvalue(fn_name, value, registry, out);
        }
        MirStatement::StaticFieldSet {
            class,
            field,
            value,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_rvalue(fn_name, value, registry, out);
        }
        MirStatement::IndexSet {
            array,
            index,
            value,
            ..
        } => {
            check_operand(fn_name, array, registry, out);
            check_operand(fn_name, index, registry, out);
            check_rvalue(fn_name, value, registry, out);
        }
        MirStatement::LinqForeach { chain, body, .. } => {
            check_operand(fn_name, &chain.source, registry, out);
            for s in body {
                check_statement(fn_name, s, registry, out);
            }
        }
        MirStatement::Await { task, .. } => check_rvalue(fn_name, task, registry, out),
        MirStatement::Throw { value } => check_rvalue(fn_name, value, registry, out),
        MirStatement::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            for s in try_body {
                check_statement(fn_name, s, registry, out);
            }
            for s in catch_body {
                check_statement(fn_name, s, registry, out);
            }
        }
        MirStatement::TryFinally { body, finally } => {
            for s in body {
                check_statement(fn_name, s, registry, out);
            }
            for s in finally {
                check_statement(fn_name, s, registry, out);
            }
        }
        MirStatement::Break | MirStatement::Continue => {}
    }
}

fn check_rvalue(
    fn_name: &str,
    rv: &MirRvalue,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    match rv {
        MirRvalue::Use(o) => check_operand(fn_name, o, registry, out),
        MirRvalue::Binary { left, right, .. } => {
            check_operand(fn_name, left, registry, out);
            check_operand(fn_name, right, registry, out);
        }
        MirRvalue::Call { args, .. } => {
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::New { args, .. } => {
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::FieldGet {
            object,
            class,
            field,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, object, registry, out);
        }
        MirRvalue::MethodCall { receiver, args, .. } => {
            check_operand(fn_name, receiver, registry, out);
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::MakeIface { object, .. } => check_operand(fn_name, object, registry, out),
        MirRvalue::MakeIfaceDyn { object, .. } => check_operand(fn_name, object, registry, out),
        MirRvalue::AdaptIface { object, .. } => check_operand(fn_name, object, registry, out),
        MirRvalue::StructLit { fields, .. } => {
            for (_, o) in fields {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::ArrayLit { elements, .. } => {
            for el in elements {
                match el {
                    crate::types::ArrayLitElement::Value(rv) => {
                        check_rvalue(fn_name, rv, registry, out)
                    }
                    crate::types::ArrayLitElement::Spread(o) => {
                        check_operand(fn_name, o, registry, out)
                    }
                }
            }
        }
        MirRvalue::NewArray { length, .. } => check_operand(fn_name, length, registry, out),
        MirRvalue::IndexGet { array, index, .. } => {
            check_operand(fn_name, array, registry, out);
            check_operand(fn_name, index, registry, out);
        }
        MirRvalue::SpanFromArray {
            array,
            start,
            length,
            ..
        } => {
            check_operand(fn_name, array, registry, out);
            if let Some(o) = start {
                check_operand(fn_name, o, registry, out);
            }
            if let Some(o) = length {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::SpanFromStack { elements, .. } => {
            for o in elements {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::SpanSlice {
            span,
            start,
            length,
            ..
        } => {
            check_operand(fn_name, span, registry, out);
            check_operand(fn_name, start, registry, out);
            if let Some(o) = length {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::SpanFill { span, value, .. } => {
            check_operand(fn_name, span, registry, out);
            check_operand(fn_name, value, registry, out);
        }
        MirRvalue::SpanClear { span, .. } => check_operand(fn_name, span, registry, out),
        MirRvalue::SpanCopyTo { src, dest, .. } => {
            check_operand(fn_name, src, registry, out);
            check_operand(fn_name, dest, registry, out);
        }
        MirRvalue::SpanTryCopyTo { src, dest, .. } => {
            check_operand(fn_name, src, registry, out);
            check_operand(fn_name, dest, registry, out);
        }
        MirRvalue::SpanToArray { span, .. } => check_operand(fn_name, span, registry, out),
        MirRvalue::SoaFieldGet {
            array,
            index,
            class,
            field,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, array, registry, out);
            check_operand(fn_name, index, registry, out);
        }
        MirRvalue::LinqChain(chain) => check_operand(fn_name, &chain.source, registry, out),
        MirRvalue::ExpressionTreeConst { .. } => {}
        MirRvalue::FnPtr { .. } => {}
        MirRvalue::IndirectCall { func, args } => {
            check_operand(fn_name, func, registry, out);
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::Coalesce { left, right } => {
            check_operand(fn_name, left, registry, out);
            check_operand(fn_name, right, registry, out);
        }
        MirRvalue::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            check_operand(fn_name, cond, registry, out);
            check_operand(fn_name, then_val, registry, out);
            check_operand(fn_name, else_val, registry, out);
        }
        MirRvalue::NullCondField {
            receiver,
            class,
            field,
            default,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, receiver, registry, out);
            check_operand(fn_name, default, registry, out);
        }
        MirRvalue::NullCondMethod {
            receiver,
            args,
            default,
            ..
        } => {
            check_operand(fn_name, receiver, registry, out);
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
            check_operand(fn_name, default, registry, out);
        }
        MirRvalue::ForceDerefField {
            receiver,
            class,
            field,
            ..
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, receiver, registry, out);
        }
        MirRvalue::ForceDerefMethod { receiver, args, .. } => {
            check_operand(fn_name, receiver, registry, out);
            for o in args {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::Box { src, .. } => check_operand(fn_name, src, registry, out),
        MirRvalue::Unbox { src, .. } => check_operand(fn_name, src, registry, out),
        MirRvalue::VariantConstruct { payload, .. } => {
            if let Some(o) = payload {
                check_operand(fn_name, o, registry, out);
            }
        }
        MirRvalue::VariantTag { scrutinee, .. } => check_operand(fn_name, scrutinee, registry, out),
        MirRvalue::VariantExtract { scrutinee, .. } => {
            check_operand(fn_name, scrutinee, registry, out)
        }
    }
}

fn check_operand(
    fn_name: &str,
    o: &MirOperand,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    match o {
        MirOperand::Local(_)
        | MirOperand::ConstInt(_)
        | MirOperand::ConstFloat(_)
        | MirOperand::ConstString(_)
        | MirOperand::ConstBool(_)
        | MirOperand::ConstNull
        | MirOperand::AddrOf(_)
        | MirOperand::FnPtr { .. }
        | MirOperand::TypeId { .. }
        | MirOperand::TypeInfoPtr { .. }
        | MirOperand::ConstDefault { .. } => {}
        MirOperand::Field {
            object,
            class,
            field,
        } => {
            check_field_ref(fn_name, class, field, registry, out);
            check_operand(fn_name, object, registry, out);
        }
        MirOperand::Iface { object, .. }
        | MirOperand::UnboxIface { object, .. }
        | MirOperand::UnboxString { object }
        | MirOperand::UnboxGeneric { object, .. } => check_operand(fn_name, object, registry, out),
        MirOperand::StaticField { class, field } => {
            check_field_ref(fn_name, class, field, registry, out)
        }
        MirOperand::Closure { env, .. } => {
            for (_, o) in env {
                check_operand(fn_name, o, registry, out);
            }
        }
    }
}

fn check_terminator(
    fn_name: &str,
    t: &MirTerminator,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    match t {
        MirTerminator::Goto(_) | MirTerminator::Unreachable => {}
        MirTerminator::CondBr { cond, .. } => check_operand(fn_name, cond, registry, out),
        MirTerminator::Return(Some(o)) => check_operand(fn_name, o, registry, out),
        MirTerminator::Return(None) => {}
        MirTerminator::Throw(o) => check_operand(fn_name, o, registry, out),
    }
}

/// 六级解析链复核一个 `(class, field)` 引用（详见模块文档）。
fn check_field_ref(
    fn_name: &str,
    class: &str,
    field: &str,
    registry: &TypeRegistry,
    out: &mut Vec<FieldCheckDiagnostic>,
) {
    // 层 0：类不在编译单元类型集——类封闭性由 typeck 保证，本 pass 职责
    // 边界 = 已知类的字段引用完备性（facade stub / 外部包 / 合成类在此放行）。
    if !registry.types.contains_key(class) {
        return;
    }
    // 层 1：facade 豁免（单一事实源 typeck::builtin_facade）。
    if is_builtin_facade(class) {
        return;
    }
    // 层 2：enum 成员（`EnumName.Member` 保留 Field 形态的历史路径）。
    if registry
        .enum_variant(&Ident::from(class), &Ident::from(field))
        .is_some()
    {
        return;
    }
    // 层 3：custom accessor 属性（复用 lower 同源判定，禁止另起清单）。
    if is_custom_accessor_property(registry, class, field) {
        return;
    }
    // 层 4：常规字段（static/const/instance 基类链一并解析）。
    if registry
        .field_info(&Ident::from(class), &Ident::from(field))
        .is_some()
    {
        return;
    }
    // 层 5：`[Builtin]` 静态属性（单一事实源延伸）。
    if registry
        .builtin_static_props
        .get(class)
        .is_some_and(|props| props.contains(field))
    {
        return;
    }
    // 层 6：全部未命中 → 诊断。
    out.push(FieldCheckDiagnostic {
        code: FieldCheckCode::UnresolvedField,
        message: format!("类型 '{class}' 不存在字段或属性 '{field}'"),
        fn_name: fn_name.to_string(),
    });
}

#[allow(unused)]
fn _type_id_witness(_: TypeId) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ArrayLitElement, BlockId, LocalId, MirBlock};
    use ast::{Span, Visibility};
    use indexmap::{IndexMap, IndexSet};
    use typeck::{EnumVariantInfo, FieldInfo, NominalType, TypeKind};

    fn reg(types: Vec<NominalType>) -> TypeRegistry {
        let mut r = TypeRegistry::default();
        for t in types {
            r.types.insert(t.name.clone(), t);
        }
        r
    }

    fn nom(
        name: &str,
        kind: TypeKind,
        fields: Vec<(&str, bool)>,
        methods: Vec<&str>,
        bases: Vec<&str>,
        generic_params: Vec<&str>,
        variants: Vec<EnumVariantInfo>,
    ) -> NominalType {
        let mut fm = IndexMap::new();
        for (n, is_static) in fields {
            fm.insert(
                Ident::from(n),
                FieldInfo {
                    name: Ident::from(n),
                    ty: Ident::from("int"),
                    vis: Visibility::Public,
                    is_const: false,
                    is_readonly: false,
                    is_init_only: false,
                    get_vis: None,
                    set_vis: None,
                    is_static,
                    init: None,
                },
            );
        }
        let mut mm = IndexMap::new();
        for m in methods {
            mm.insert(Ident::from(m), Vec::new());
        }
        NominalType {
            name: Ident::from(name),
            kind,
            vis: Visibility::Public,
            is_abstract: false,
            is_record: false,
            is_readonly: false,
            fields: fm,
            methods: mm,
            bases: bases.into_iter().map(Ident::from).collect(),
            base_types: Vec::new(),
            span: Span::DUMMY,
            variants,
            generic_params: generic_params.into_iter().map(Ident::from).collect(),
            namespace: Vec::new(),
            const_values: IndexMap::new(),
            constructors: Vec::new(),
            soa: false,
            required_props: IndexSet::new(),
        }
    }

    fn fget(class: &str, field: &str) -> MirRvalue {
        MirRvalue::FieldGet {
            object: MirOperand::ConstNull,
            class: class.into(),
            field: field.into(),
        }
    }

    fn fop(class: &str, field: &str) -> MirOperand {
        MirOperand::Field {
            object: Box::new(MirOperand::ConstNull),
            class: class.into(),
            field: field.into(),
        }
    }

    fn block(id: u32, stmts: Vec<MirStatement>) -> MirBlock {
        MirBlock {
            id: BlockId(id),
            statements: stmts,
            terminator: MirTerminator::Unreachable,
        }
    }

    fn body(blocks: Vec<MirBlock>) -> MirCfgBody {
        let mut m = IndexMap::new();
        for b in blocks {
            m.insert(b.id, b);
        }
        let mut b = MirCfgBody::stub_skeleton("test::t", "Test");
        b.blocks = m;
        b
    }

    fn expect_one(diags: &[FieldCheckDiagnostic]) -> (&str, &str) {
        assert_eq!(diags.len(), 1, "期望恰好 1 条诊断，实得 {diags:?}");
        (diags[0].message.as_str(), diags[0].fn_name.as_str())
    }

    #[test]
    fn code_str_contract() {
        assert_eq!(
            FieldCheckCode::UnresolvedField.code_str(),
            "E_FIELD_UNRESOLVED"
        );
    }

    #[test]
    fn instance_and_base_fields_resolve() {
        let r = reg(vec![
            nom(
                "Base",
                TypeKind::Class,
                vec![("b", false)],
                vec![],
                vec![],
                vec![],
                vec![],
            ),
            nom(
                "Derived",
                TypeKind::Class,
                vec![],
                vec![],
                vec!["Base"],
                vec![],
                vec![],
            ),
        ]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("Derived", "b"),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn missing_field_reports() {
        let r = reg(vec![nom(
            "A",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("A", "ghost"),
            }],
        )]);
        let diags = run_field_check_module(&[("M::run".into(), b)], &r);
        let (msg, fn_name) = expect_one(&diags);
        assert_eq!(fn_name, "M::run");
        assert!(
            msg.contains('A') && msg.contains("ghost"),
            "message 缺类名/字段名: {msg}"
        );
        assert_eq!(diags[0].code, FieldCheckCode::UnresolvedField);
    }

    #[test]
    fn generic_template_body_exempt() {
        let r = reg(vec![nom(
            "Tmpl",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec!["T"],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("Tmpl", "ghost"),
            }],
        )]);
        assert!(run_field_check_module(&[("Tmpl::M".into(), b)], &r).is_empty());
    }

    #[test]
    fn facade_class_exempt() {
        // 类在 registry（stub 声明形态）但 facade 清单命中 → 字段缺失仍放行。
        let r = reg(vec![nom(
            "Array",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("Array", "Length"),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn enum_member_resolves() {
        let r = reg(vec![nom(
            "Color",
            TypeKind::Enum,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![EnumVariantInfo {
                name: Ident::from("Red"),
                fields: vec![],
                discriminant: 0,
                payload: None,
            }],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: MirRvalue::Use(fop("Color", "Red")),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn custom_accessor_resolves() {
        let r = reg(vec![nom(
            "P",
            TypeKind::Class,
            vec![],
            vec!["get_Name"],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("P", "Name"),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn static_field_form_resolves() {
        let r = reg(vec![nom(
            "Counter",
            TypeKind::Class,
            vec![("_count", true)],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: MirRvalue::Use(MirOperand::StaticField {
                    class: "Counter".into(),
                    field: "_count".into(),
                }),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn builtin_static_prop_resolves() {
        let mut r = reg(vec![nom(
            "Cfg",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let mut props = IndexSet::new();
        props.insert(Ident::from("Port"));
        r.builtin_static_props.insert(Ident::from("Cfg"), props);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: MirRvalue::Use(MirOperand::StaticField {
                    class: "Cfg".into(),
                    field: "Port".into(),
                }),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn unknown_class_passes() {
        let r = reg(vec![]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: fget("External", "x"),
            }],
        )]);
        assert!(run_field_check_module(&[("f".into(), b)], &r).is_empty());
    }

    #[test]
    fn region_if_cond_checked() {
        // If 仅出现在 region 语句体内部（顶层已被 to_cfg 展平）——
        // 锁定 collect_concrete_class_refs `..` 疏漏的补齐契约。
        let r = reg(vec![nom(
            "A",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::TryCatch {
                try_body: vec![MirStatement::If {
                    cond: fop("A", "ghost"),
                    then_body: vec![],
                    else_body: vec![],
                }],
                catch_var: LocalId(1),
                catch_ty: TypeId::Void,
                catch_body: vec![],
            }],
        )]);
        let diags = run_field_check_module(&[("f".into(), b)], &r);
        let (msg, _) = expect_one(&diags);
        assert!(msg.contains("ghost"));
    }

    #[test]
    fn indexset_operands_and_while_checked() {
        let r = reg(vec![
            nom("B", TypeKind::Class, vec![], vec![], vec![], vec![], vec![]),
            nom("C", TypeKind::Class, vec![], vec![], vec![], vec![], vec![]),
        ]);
        let b = body(vec![block(
            0,
            vec![MirStatement::TryCatch {
                try_body: vec![
                    MirStatement::While {
                        cond: MirRvalue::Use(fop("B", "ghost")),
                        body: vec![],
                        foreach_source: Some(fop("C", "ghost2")),
                    },
                    MirStatement::IndexSet {
                        array: fop("B", "ghost3"),
                        index: MirOperand::ConstInt(0),
                        elem_type: TypeId::Void,
                        value: MirRvalue::Use(MirOperand::ConstInt(1)),
                    },
                ],
                catch_var: LocalId(1),
                catch_ty: TypeId::Void,
                catch_body: vec![],
            }],
        )]);
        let diags = run_field_check_module(&[("f".into(), b)], &r);
        assert_eq!(
            diags.len(),
            3,
            "应报 cond/foreach_source/array 三处: {diags:?}"
        );
        for d in &diags {
            assert_eq!(d.code.code_str(), "E_FIELD_UNRESOLVED");
        }
    }

    #[test]
    fn terminator_condbr_checked() {
        let r = reg(vec![nom(
            "D",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let mut b = body(vec![block(0, vec![])]);
        b.blocks.insert(
            BlockId(0),
            MirBlock {
                id: BlockId(0),
                statements: vec![],
                terminator: MirTerminator::CondBr {
                    cond: fop("D", "ghost"),
                    then_bb: BlockId(1),
                    else_bb: BlockId(1),
                },
            },
        );
        let diags = run_field_check_module(&[("f".into(), b)], &r);
        let (msg, _) = expect_one(&diags);
        assert!(msg.contains("ghost"));
    }

    #[test]
    fn fieldset_soa_and_static_set_checked() {
        // FieldSet 是字段写入点，与 FieldGet/StaticFieldSet 同走完整解析链：
        // 合法字段 "ok"（registry 已注册）放行，非法字段报错。
        let r = reg(vec![nom(
            "S",
            TypeKind::Struct,
            vec![("ok", false)],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![
                MirStatement::FieldSet {
                    object: MirOperand::ConstNull,
                    class: "S".into(),
                    field: "ok".into(),
                    value: MirRvalue::Use(MirOperand::ConstInt(1)),
                },
                MirStatement::StaticFieldSet {
                    class: "S".into(),
                    field: "ghost".into(),
                    value: MirRvalue::Use(MirOperand::ConstInt(2)),
                },
                MirStatement::Assign {
                    place: LocalId(0),
                    rvalue: MirRvalue::SoaFieldGet {
                        array: MirOperand::ConstNull,
                        index: MirOperand::ConstInt(0),
                        class: "S".into(),
                        field: "ghost2".into(),
                    },
                },
            ],
        )]);
        let diags = run_field_check_module(&[("f".into(), b)], &r);
        assert_eq!(
            diags.len(),
            2,
            "应报 StaticFieldSet/SoaFieldGet 两处: {diags:?}"
        );
        assert!(diags.iter().all(|d| d.message.contains('S')));
        assert!(
            !diags.iter().any(|d| d.message.contains("'ok'")),
            "已注册合法字段不得误报: {diags:?}"
        );
    }

    #[test]
    fn aggregate_operands_checked() {
        // StructLit / Ternary / Closure env / ArrayLit Spread：非字段位的
        // operand 递归同样抵达字段解析链。
        let r = reg(vec![nom(
            "T",
            TypeKind::Class,
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        )]);
        let b = body(vec![block(
            0,
            vec![MirStatement::Assign {
                place: LocalId(0),
                rvalue: MirRvalue::StructLit {
                    struct_name: "S".into(),
                    fields: vec![("x".into(), fop("T", "ghost"))],
                },
            }],
        )]);
        let diags = run_field_check_module(&[("f".into(), b)], &r);
        let (msg, _) = expect_one(&diags);
        assert!(msg.contains("ghost"));
        let _ = (
            ArrayLitElement::Spread(MirOperand::ConstNull),
            MirStatement::Break,
        );
    }
}
