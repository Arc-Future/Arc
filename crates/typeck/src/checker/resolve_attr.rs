//! 属性解析与注册（RFC 012 D3.2 + M3 用户自定义属性）。
//!
//! 将 AST 层 [`ast::Attribute`] 转换为 typeck 层
//! [`ResolvedAttribute`](crate::ResolvedAttribute) 并注册到
//! [`TypeChecker::attribute_table`](super::TypeChecker::attribute_table)。
//!
//! # 流程
//!
//! M3 起，[`resolve_attributes`](TypeChecker::resolve_attributes) 执行以下步骤：
//! 1. **位置/命名参数分离**：[`AttributeArg::Named`] 抽到 `named_args`，
//!    其余进 `args`（C# 规范：命名参数必须在位置参数之后）
//! 2. **参数类型解析**：[`convert_arg`](TypeChecker::convert_arg) 把 AST 节点
//!    转换为 [`ResolvedArg`]，支持字面量 / `typeof(T)` / `AttributeTargets.X`
//!    成员路径常量
//! 3. **属性类查找**：[`lookup_attr_class`](TypeChecker::lookup_attr_class) 按
//!    `<name>` 与 `<name>Attribute` 两种形式查找派生自 `Attribute` 的类
//! 4. **构造函数签名校验**：[`validate_ctor_signature`] 校验位置参数
//! 5. **命名参数属性校验**：[`validate_named_args`] 校验命名参数对应
//!    public settable 属性（auto-property）
//! 6. **AttributeUsage 约束校验**（M3-6）：[`validate_attribute_usage`]
//!    查询属性类上的 `[AttributeUsage]` 元属性，按 `ValidOn` 位掩码校验
//!    目标合法性，按 `AllowMultiple` 校验重复附加
//! 7. **内置属性快速路径**：[`validate_builtin`] 仍按 [`BUILTIN_SPECS`]
//!    校验内置属性的目标与参数签名（与 M3 并存，因 std 已声明这些类的
//!    Attribute 派生类，但快速路径提供更精确的错误信息）
//!
//! 7.5. **`[Observable]` 形态校验（RFC 037 M-D0）**：仅作用于可写
//!    auto-property——get-only 无 setter 无处合成通知 = 编译错误；
//!    custom-accessor 不插桩、不报错
//! 8. **注册**：校验通过则注册到 `attribute_table`；失败则推入 `self.errors`
//!    并跳过注册（无效属性不入表）

use ast::{Attribute, AttributeArg, ExpressionTree, Ident, PropertyDef};
use hir::DefId;

use super::*;
use crate::oop_types::ConstValue;
use crate::{AttributeTarget, AttributeTargetsBit, ResolvedArg, ResolvedAttribute};

impl TypeChecker {
    /// 解析 AST 属性列表并注册到符号属性表。
    ///
    /// 空属性列表直接 return（避免分配 `Vec`）。每个属性的失败不影响
    /// 后续属性处理——错误推入 `self.errors` 后继续下一个属性。
    ///
    /// `property`：当 `target` 为 [`AttributeTarget::Property`] 时传入所属
    /// `PropertyDef`（auto/custom、get-only 等访问器形态信息），供
    /// `[Observable]` 形态校验（RFC 037 M-D0，见 [`validate_observable_property`]）
    /// 使用；其余目标传 `None`。
    pub(crate) fn resolve_attributes(
        &mut self,
        attrs: &[Attribute],
        target: AttributeTarget,
        owner_def_id: DefId,
        property: Option<&PropertyDef>,
    ) {
        if attrs.is_empty() {
            return;
        }
        for attr in attrs {
            let name = attr
                .path
                .last()
                .cloned()
                .unwrap_or_else(|| Ident::from("_"));

            // 步骤 1: 位置/命名参数分离 + 步骤 2: 类型解析
            let mut positional: Vec<ResolvedArg> = Vec::new();
            let mut named_args: Vec<(Ident, ResolvedArg)> = Vec::new();
            let mut had_error = false;
            for arg in &attr.args {
                match arg {
                    AttributeArg::Named { name, value } => match self.convert_arg(value) {
                        Ok(v) => named_args.push((name.clone(), v)),
                        Err(e) => {
                            self.errors.push(e);
                            had_error = true;
                        }
                    },
                    other => match self.convert_arg(other) {
                        Ok(v) => positional.push(v),
                        Err(e) => {
                            self.errors.push(e);
                            had_error = true;
                        }
                    },
                }
            }
            if had_error {
                continue;
            }

            // 步骤 3: 属性类查找（含 Attribute 后缀省略）
            let attr_class = self.lookup_attr_class(&name);
            let attr_type = match &attr_class {
                // RFC 012 M4-1: attr_class 来自 registry.types 查找，
                // 不含泛型类模板（registry 已跳过 generics 非空的类），
                // 故 attr_class.name 总是 arity 0 的实体类，使用 arity=0。
                Some(nom) => self.ensure_class_def_id(&nom.name, 0),
                None => crate::BUILTIN_ATTR_TYPE,
            };

            // 步骤 4 + 5: 用户自定义属性类的构造函数与命名参数校验
            if let Some(nom) = &attr_class {
                if let Err(e) = self.validate_ctor_signature(nom, &positional) {
                    self.errors.push(e);
                    continue;
                }
                if let Err(e) = self.validate_named_args(nom, &named_args) {
                    self.errors.push(e);
                    continue;
                }
            }

            let mut resolved =
                ResolvedAttribute::builtin(name.clone(), positional, target, ast::Span::DUMMY);
            resolved.attr_type = attr_type;
            resolved.named_args = named_args;

            // 步骤 6: AttributeUsage 约束校验（M3-6）
            if let Some(nom) = &attr_class {
                if let Err(e) = self.validate_attribute_usage(nom, target, owner_def_id) {
                    self.errors.push(e);
                    continue;
                }
            }

            // 步骤 7: 内置属性快速路径（仍保留以提供更精确的错误信息）
            if let Err(e) = self.validate_builtin(&resolved) {
                self.errors.push(e);
                continue;
            }

            // 步骤 7.5: [Observable] 属性形态校验（RFC 037 M-D0 §5.3 / §16 非目标 9）。
            // 目标合法性（仅 Property）已由步骤 7 的 `valid_targets` 覆盖；
            // 此处校验 auto-property 的可写性（get-only = 编译错误）。
            if name.as_str() == "Observable" {
                if let Err(e) = validate_observable_property(&resolved, property) {
                    self.errors.push(e);
                    continue;
                }
            }

            // 步骤 8: 注册
            self.attribute_table.register(owner_def_id, resolved);
        }
    }

    /// 分配符号 DefId 供属性表反查使用。
    ///
    /// Phase 0 字段/属性/类型符号没有独立的 HIR DefId（只有 `typed_fns` 有），
    /// 由 typeck 内部 `alloc_def_id` 分配器生成唯一 ID 作为
    /// `attribute_table` 的键。反查通过 [`class_def_id`](super::TypeChecker::class_def_id)
    /// / [`member_def_id`](super::TypeChecker::member_def_id) 进行。
    pub(crate) fn alloc_symbol_def_id(&mut self) -> DefId {
        self.alloc_def_id()
    }

    /// RFC 012 B2: 分配成员符号 DefId 并同时注册到 `member_def_ids` 与
    /// `def_id_members`（双向反查表），保持两表一致。
    ///
    /// 调用方传入 `(类型名, 成员名)`，本方法分配新 DefId 后同时插入：
    /// - `member_def_ids[(类型名, 成员名)] = DefId`（正向查询）
    /// - `def_id_members[DefId] = (类型名, 成员名)`（反向查询，供
    ///   [`method_signature`](super::TypeChecker::method_signature) 使用）
    ///
    /// 供 `check_class` 中 field / property / method 属性收集调用，
    /// 替代原本 `alloc_symbol_def_id` + 手动 `member_def_ids.insert` 模式。
    pub(crate) fn alloc_member_def_id(&mut self, type_name: &Ident, member_name: &Ident) -> DefId {
        let def_id = self.alloc_symbol_def_id();
        self.member_def_ids
            .insert((type_name.clone(), member_name.clone()), def_id);
        self.def_id_members
            .insert(def_id, (type_name.clone(), member_name.clone()));
        def_id
    }

    /// RFC 012 M3-5: 按属性名查找用户自定义属性类。
    ///
    /// 尝试两种形式（C# 规范允许 Attribute 后缀省略）：
    /// 1. `<name>` 直接匹配（如 `[AttributeUsage]` → `AttributeUsageAttribute`）
    /// 2. `<name>Attribute` 后缀匹配（如 `[Table]` → `TableAttribute`）
    ///
    /// 仅返回派生自 `Attribute` 根基类的类（[`derives_from_attribute`] 校验）。
    /// 未找到时返回 `None`，由调用方回退到 [`BUILTIN_ATTR_TYPE`] 占位。
    fn lookup_attr_class(&self, name: &Ident) -> Option<crate::oop_types::NominalType> {
        // 直接匹配
        if let Some(nom) = self.registry.types.get(name) {
            if self.derives_from_attribute(&nom.name) {
                return Some(nom.clone());
            }
        }
        // Attribute 后缀省略：`[Table]` → `TableAttribute`
        let with_suffix = format!("{}Attribute", name.as_str());
        if let Some(nom) = self.registry.types.get(&Ident::from(with_suffix)) {
            if self.derives_from_attribute(&nom.name) {
                return Some(nom.clone());
            }
        }
        None
    }

    /// RFC 012 M3-5: 检查类是否派生自 `Attribute` 根基类。
    ///
    /// 沿 `bases` 链向上查找。`Attribute` 类自身返回 `true`（根基类）。
    /// 防止循环继承：用 `visited` 集合记录已访问类名。
    fn derives_from_attribute(&self, class_name: &Ident) -> bool {
        let mut visited: std::collections::HashSet<Ident> = std::collections::HashSet::new();
        let mut current = Some(class_name.clone());
        while let Some(cn) = current {
            if !visited.insert(cn.clone()) {
                return false; // 循环继承
            }
            if cn.as_str() == "Attribute" {
                return true;
            }
            let Some(nom) = self.registry.types.get(&cn) else {
                return false;
            };
            // 沿 base class 链查找（class 仅允许单继承，但 bases 可能含 interface）
            current = nom
                .bases
                .iter()
                .find(|b| self.registry.is_class(b))
                .cloned();
        }
        false
    }

    /// RFC 012 M3-5: AST [`AttributeArg`] → [`ResolvedArg`] 类型转换。
    ///
    /// 处理四种 AST 形式：
    /// - 字面量：`"str"` / `42` / `true` / `false`
    /// - 类型引用：`typeof(T)` → [`ResolvedArg::Type`] (via [`lower_type`])
    /// - 成员路径常量：`AttributeTargets.X` → [`ResolvedArg::Int`]
    ///   (via [`resolve_member_path`])
    ///
    /// [`AttributeArg::Named`] 由调用方（`resolve_attributes`）分离处理，
    /// 此方法不接受 `Named` 变体（panic 以暴露调用方 bug）。
    ///
    /// [`lower_type`]: super::TypeChecker::lower_type
    fn convert_arg(&mut self, arg: &AttributeArg) -> Result<ResolvedArg, TypeError> {
        match arg {
            AttributeArg::String(s) => Ok(ResolvedArg::String(s.clone())),
            AttributeArg::Int(n) => Ok(ResolvedArg::Int(*n)),
            AttributeArg::Bool(b) => Ok(ResolvedArg::Bool(*b)),
            AttributeArg::Type(ty) => {
                let type_id = self.lower_type(&ty.node)?;
                Ok(ResolvedArg::Type(type_id))
            }
            AttributeArg::MemberPath(path) => self.resolve_member_path(path),
            // RFC 009 M4-7：Lambda 表达式参数树化为 ExpressionTree。
            // 复用 RFC 022 的 `ExpressionTree::from_lambda`：把 Lambda AST
            // 降级为 ExpressionNode IR，存入 `ResolvedArg::Expression`。
            // 捕获列表传空——attribute 位置的 Lambda 不允许捕获外部作用域
            // 变量（M4-7 受限求值器环境内求值，无外部 locals 可捕获）。
            AttributeArg::Lambda(lambda) => match ExpressionTree::from_lambda(lambda, &[]) {
                Some(tree) => Ok(ResolvedArg::Expression(tree)),
                None => Err(TypeError::Unsupported(
                    "Lambda attribute arg cannot be lowered to ExpressionTree (unsupported \
                     syntax in lambda body)"
                        .to_string(),
                )),
            },
            AttributeArg::Named { .. } => {
                // 调用方应在分离位置/命名参数时已处理 Named，此处不可达
                unreachable!("Named arg should be split by caller (resolve_attributes)")
            }
        }
    }

    /// RFC 012 M3-5: 解析成员路径为编译期常量。
    ///
    /// 支持两类：
    /// - `AttributeTargets.<Name>`（std/Arc/Attribute.as 中 `AttributeTargets`
    ///   类的 `public const int` 字段）→ [`ResolvedArg::Int`]（位掩码）
    /// - `<EnumName>.<Variant>`（已注册枚举的成员，如 `ServiceLifetime.Singleton`）
    ///   → [`ResolvedArg::Enum`]
    ///
    /// `|` 组合（`AttributeTargets.A | AttributeTargets.B`）暂不支持，
    /// parser 层 [`try_fold_bit_or`](parse::Parser::try_fold_bit_or) 报错。
    fn resolve_member_path(&self, path: &[Ident]) -> Result<ResolvedArg, TypeError> {
        if path.len() != 2 {
            return Err(TypeError::Oop(format!(
                "unsupported attribute member path: {} (only `<Type>.<Member>` is supported)",
                path.iter()
                    .map(|i| i.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            )));
        }
        let type_name = &path[0];
        let member = &path[1];
        // 枚举成员路径：`ServiceLifetime.Singleton` 等。
        if self.registry.is_enum(type_name) {
            if self.registry.enum_variant(type_name, member).is_some() {
                return Ok(ResolvedArg::Enum {
                    name: type_name.clone(),
                    variant: member.clone(),
                });
            }
            return Err(TypeError::Oop(format!(
                "enum `{}` has no member `{}`",
                type_name, member
            )));
        }
        // 类 `const int` 成员路径（`AttributeTargets.X`）。
        if type_name.as_str() != "AttributeTargets" {
            return Err(TypeError::Oop(format!(
                "unsupported attribute member path: {} (only `AttributeTargets.<Name>` and `<Enum>.<Member>` are supported)",
                path.iter()
                    .map(|i| i.as_str())
                    .collect::<Vec<_>>()
                    .join(".")
            )));
        }
        let nom = self
            .registry
            .types
            .get(type_name)
            .ok_or_else(|| TypeError::Oop("undefined type `AttributeTargets`".into()))?;
        let cv = nom.const_values.get(member).ok_or_else(|| {
            TypeError::Oop(format!("AttributeTargets has no const member `{}`", member))
        })?;
        match cv {
            ConstValue::Int(n) => Ok(ResolvedArg::Int(*n)),
            _ => Err(TypeError::Oop(format!(
                "AttributeTargets.{} is not an int constant",
                member
            ))),
        }
    }

    /// RFC 012 M3-5: 校验位置参数与构造函数签名匹配。
    ///
    /// 规则：
    /// - 属性类无构造函数时，仅允许 0 个位置参数
    /// - 否则在 `constructors` 中找一个匹配的签名：参数数量一致且
    ///   逐个类型兼容（基本类型严格匹配，Type 变体不参与重载选择）
    fn validate_ctor_signature(
        &self,
        attr_class: &crate::oop_types::NominalType,
        args: &[ResolvedArg],
    ) -> Result<(), TypeError> {
        if attr_class.constructors.is_empty() {
            if args.is_empty() {
                return Ok(());
            }
            return Err(TypeError::Oop(format!(
                "attribute class `{}` has no constructors but {} positional arg(s) provided",
                attr_class.name,
                args.len()
            )));
        }
        for ctor in &attr_class.constructors {
            let total = ctor.param_types.len();
            if args.len() > total {
                continue;
            }
            // 省略实参时，被省略的尾部形参必须带默认值（C# 可选参数语义）。
            // params 与 param_types 等长；防御性退化：params 未填充时要求精确匹配。
            let required = if ctor.params.len() == total {
                ctor.params.iter().filter(|p| p.default.is_none()).count()
            } else {
                total
            };
            if args.len() < required {
                continue;
            }
            let matched = ctor
                .param_types
                .iter()
                .zip(args.iter())
                .all(|(p, a)| arg_matches_type(a, p));
            if matched {
                return Ok(());
            }
        }
        Err(TypeError::Oop(format!(
            "no constructor of `{}` matches the {} positional arg(s) provided",
            attr_class.name,
            args.len()
        )))
    }

    /// RFC 009 M3-5: 校验命名参数对应 public settable 属性。
    ///
    /// 规则：
    /// - 命名参数名必须匹配属性类的 auto-property 字段（`fields` 表）
    /// - 字段不可为 `is_const` 或 `is_readonly`（C# 命名参数仅可赋给
    ///   public 可读写属性；M3 简化：所有 fields 表项均视为可设置）
    /// - 类型兼容性校验（与构造函数相同的基本类型匹配规则）
    fn validate_named_args(
        &self,
        attr_class: &crate::oop_types::NominalType,
        named: &[(Ident, ResolvedArg)],
    ) -> Result<(), TypeError> {
        for (name, value) in named {
            let field = attr_class.fields.get(name).ok_or_else(|| {
                TypeError::Oop(format!(
                    "attribute class `{}` has no settable property `{}`",
                    attr_class.name, name
                ))
            })?;
            if field.is_const || field.is_readonly {
                return Err(TypeError::Oop(format!(
                    "property `{}` on attribute class `{}` is not settable (const/readonly)",
                    name, attr_class.name
                )));
            }
            if !arg_matches_type(value, &field.ty) {
                return Err(TypeError::Oop(format!(
                    "named arg `{}` expects `{}` but got {}",
                    name,
                    field.ty,
                    format_resolved_arg(value)
                )));
            }
        }
        Ok(())
    }

    /// RFC 012 M3-6: AttributeUsage 约束校验（ValidOn + AllowMultiple）。
    ///
    /// 查询属性类上的 `[AttributeUsage]` 元属性，按 `ValidOn` 位掩码校验
    /// 目标合法性，按 `AllowMultiple` 校验同一符号是否重复附加。
    /// `Inherited` 标志 M3 不处理（继承链属性传播留作后续扩展）。
    ///
    /// 未标注 `[AttributeUsage]` 的属性类默认 `ValidOn=All`、
    /// `AllowMultiple=false`、`Inherited=true`（C# 规范默认值）。
    fn validate_attribute_usage(
        &self,
        attr_class: &crate::oop_types::NominalType,
        target: AttributeTarget,
        owner_def_id: DefId,
    ) -> Result<(), TypeError> {
        // RFC 012 M4-1: attr_class 来自 registry.types 查找（不含泛型模板），
        // 故查 class_def_ids（arity 0 表）即可。
        let attr_class_def_id = self
            .class_def_ids
            .get(&attr_class.name)
            .copied()
            .unwrap_or(crate::BUILTIN_ATTR_TYPE);

        // 查询属性类自身是否标注 [AttributeUsage]
        let usage_attr = self
            .attribute_table
            .find_attr(attr_class_def_id, "AttributeUsage");

        let valid_on: i64 = match usage_attr {
            Some(attr) => attr
                .args
                .first()
                .and_then(|a| match a {
                    ResolvedArg::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(AttributeTargetsBit::All as i64),
            None => AttributeTargetsBit::All as i64, // 默认 All
        };

        // 校验目标合法性（位掩码）
        let target_bit = target_to_bit(target);
        if (valid_on & target_bit) == 0 {
            return Err(TypeError::Oop(format!(
                "attribute `[{}]` ValidOn={:#x} does not permit target {} (bit {:#x})",
                attr_class.name,
                valid_on,
                format_target(target),
                target_bit
            )));
        }

        // 校验 AllowMultiple（默认 false）
        let allow_multiple = usage_attr
            .and_then(|a| {
                a.named_args
                    .iter()
                    .find(|(n, _)| n.as_str() == "AllowMultiple")
                    .and_then(|(_, v)| match v {
                        ResolvedArg::Bool(b) => Some(*b),
                        _ => None,
                    })
            })
            .unwrap_or(false);

        if !allow_multiple {
            // 同一符号上是否已有同类型属性
            let existing = self.attribute_table.get_attrs(owner_def_id);
            let count = existing
                .iter()
                .filter(|a| {
                    a.name == attr_class.name || {
                        a.name.as_str() == format!("{}Attribute", attr_class.name.as_str()).as_str()
                            || format!("{}Attribute", a.name.as_str()) == attr_class.name.as_str()
                    }
                })
                .count();
            if count > 0 {
                return Err(TypeError::Oop(format!(
                    "attribute `[{}]` does not allow multiple instances on the same target",
                    attr_class.name
                )));
            }
        }

        Ok(())
    }

    /// RFC 012 M1 Task #5: 校验内置属性的目标合法性与参数签名（快速路径）。
    ///
    /// 非内置属性（未在 [`BUILTIN_SPECS`] 中登记）直接通过——M3 起由
    /// 用户自定义属性类查找 + AttributeUsage 校验接管。
    ///
    /// 返回 `Err(TypeError)` 时调用方应跳过注册。
    fn validate_builtin(&self, attr: &ResolvedAttribute) -> Result<(), TypeError> {
        let name = attr.name.as_str();
        let spec = match BUILTIN_SPECS.iter().find(|s| s.name == name) {
            Some(s) => s,
            None => return Ok(()),
        };

        // 步骤 4: 目标合法性校验
        if !spec.valid_targets.contains(&attr.target) {
            return Err(TypeError::Oop(format!(
                "attribute target mismatch: `[{}]` 仅允许附加到 {}，此处为 {}",
                name,
                format_targets(spec.valid_targets),
                format_target(attr.target)
            )));
        }

        // 步骤 3: 参数签名校验（仅校验位置参数；命名参数由 validate_named_args 处理）
        if let Err(msg) = (spec.validate_args)(&attr.args) {
            return Err(TypeError::Oop(format!(
                "attribute argument mismatch: `[{}]` {}",
                name, msg
            )));
        }

        Ok(())
    }
}

/// [`ResolvedArg`] 与构造函数/属性类型名的基本类型兼容性校验。
///
/// M3 简化规则：
/// - `Int(n)` ↔ `int` 类型
/// - `String(s)` ↔ `string` 类型
/// - `Bool(b)` ↔ `bool` 类型
/// - `Type(_)` 不参与重载选择（用户自定义属性的类型化参数类型校验留作 M4）
///
/// 其他组合视为不匹配，由调用方报告具体错误。
fn arg_matches_type(arg: &ResolvedArg, type_name: &Ident) -> bool {
    match (arg, type_name.as_str()) {
        (ResolvedArg::Int(_), "int") => true,
        (ResolvedArg::String(_), "string") => true,
        (ResolvedArg::Bool(_), "bool") => true,
        // 类型引用参数：构造函数签名可能为具体类型，类型化参数类型校验留作 M4
        (ResolvedArg::Type(_), _) => true,
        // RFC 009 M4-7：Expression 参数（`Expression<T>`）不参与重载选择，
        // 由 M4-7 Pass 3 单独绑定到 feature 构造函数 Expression 形参。
        (ResolvedArg::Expression(_), _) => true,
        // 枚举成员路径：仅形参类型为同一枚举时匹配（如 ServiceLifetime 形参 ← ServiceLifetime.Singleton）。
        // 形参类型名可能带命名空间前缀（`Arc.DI.ServiceLifetime`），与源码 `using` 后的
        // 简单名比较末段即可判定同一枚举。
        (ResolvedArg::Enum { name, .. }, type_name) => {
            let last = type_name.rsplit('.').next().unwrap_or("");
            last == name.to_string().as_str()
        }
        _ => false,
    }
}

/// RFC 037 M-D0：校验 `[Observable]` 的属性形态约束（free function，便于单测）。
///
/// 依据 RFC 037 §5.3「只合成 **auto-property**；custom-accessor 属性不插桩」：
/// - 目标合法性（仅 Property）由 [`BUILTIN_SPECS`] 的 `valid_targets` 覆盖，
///   标在 Class/Field/Method 等非 Property 目标上已在 `validate_builtin` 报错，
///   故本函数只处理 Property 目标（`property` 为 `Some`）；
/// - **get-only auto-property 标注 = 编译错误**：get-only 无 setter，编译器
///   无处合成变更通知（RFC §16 非目标 9；§13 M-D0 验收样例「get-only 集合
///   属性标注 = 编译期错误」）。init-only（`{ get; init; }`）亦无 set 触发点，
///   按同一裁定一并拒绝；
/// - **custom-accessor（显式 get/set 体或索引器）→ 不报错、不插桩**：
///   通知由开发者显式 `Set` 组合（RFC §5.3）。此处静默通过（TypeWarning 通道
///   目前仅承载字段环诊断、且输出受 pipeline 策略门控，故不引入 warning）。
fn validate_observable_property(
    attr: &ResolvedAttribute,
    property: Option<&PropertyDef>,
) -> Result<(), TypeError> {
    let Some(prop) = property else {
        // 非 Property 目标不经过本校验（valid_targets 已拒绝）
        return Ok(());
    };
    // 与 check_class 中 auto-property 判定一致：非索引器且无显式访问器体
    let is_auto = !prop.is_indexer() && prop.get_body.is_none() && prop.set_body.is_none();
    if !is_auto {
        // custom-accessor：不插桩、不报错（RFC §5.3）
        return Ok(());
    }
    if !prop.has_set {
        return Err(TypeError::Oop(format!(
            "`[{}]` 不能标注在 get-only auto-property `{}` 上：无 setter 可合成 \
             变更通知（RFC 027 §16 非目标 9，get-only 无触发点）",
            attr.name.as_str(),
            prop.name.as_str()
        )));
    }
    Ok(())
}

/// [`AttributeTarget`] → 位掩码常量（与 std/Arc/Attribute.as 中 `AttributeTargets` 对应）。
fn target_to_bit(target: AttributeTarget) -> i64 {
    match target {
        AttributeTarget::Class => AttributeTargetsBit::Class as i64,
        AttributeTarget::Struct => AttributeTargetsBit::Struct as i64,
        AttributeTarget::Interface => AttributeTargetsBit::Interface as i64,
        AttributeTarget::Enum => AttributeTargetsBit::Enum as i64,
        AttributeTarget::Method => AttributeTargetsBit::Method as i64,
        AttributeTarget::Property => AttributeTargetsBit::Property as i64,
        AttributeTarget::Field => AttributeTargetsBit::Field as i64,
        AttributeTarget::Parameter => AttributeTargetsBit::Parameter as i64,
        AttributeTarget::EnumMember => AttributeTargetsBit::EnumMember as i64,
    }
}

/// 内置属性规格（RFC 012 D2）。
///
/// 定义每个内置属性的合法目标列表与参数签名校验函数。M3 起由
/// `AttributeUsageAttribute` 接管目标合法性校验，本表退化为「内置属性
/// 参数签名快速路径」——仍保留以提供更精确的错误信息。
struct BuiltinAttrSpec {
    name: &'static str,
    valid_targets: &'static [AttributeTarget],
    validate_args: fn(&[ResolvedArg]) -> Result<(), String>,
}

/// Phase 0 内置属性规格表（RFC 012 D2）。
static BUILTIN_SPECS: &[BuiltinAttrSpec] = &[
    BuiltinAttrSpec {
        name: "Table",
        valid_targets: &[AttributeTarget::Class, AttributeTarget::Struct],
        validate_args: validate_optional_string_arg,
    },
    BuiltinAttrSpec {
        name: "Column",
        valid_targets: &[AttributeTarget::Property, AttributeTarget::Field],
        validate_args: validate_optional_string_arg,
    },
    BuiltinAttrSpec {
        name: "Key",
        valid_targets: &[AttributeTarget::Property, AttributeTarget::Field],
        validate_args: validate_no_args,
    },
    BuiltinAttrSpec {
        name: "Required",
        valid_targets: &[AttributeTarget::Property, AttributeTarget::Field],
        validate_args: validate_no_args,
    },
    BuiltinAttrSpec {
        name: "MaxLength",
        valid_targets: &[AttributeTarget::Property, AttributeTarget::Field],
        validate_args: validate_required_int_arg,
    },
    // [Builtin] — 编译器内建标记。允许在 method 与 property 上使用（property
    // 的 getter 由 codegen 拦截为 `get_PropertyName`），接受可选的 ABI 命名参数。
    BuiltinAttrSpec {
        name: "Builtin",
        valid_targets: &[AttributeTarget::Method, AttributeTarget::Property],
        validate_args: validate_no_args,
    },
    // [Observable] — UI 数据驱动闭环属性级特性（RFC 037 M-D0 §5.3）。仅允许
    // 附加到 property；只作用于可写 auto-property——get-only 无 setter 无法
    // 合成通知 = 编译错误（§16 非目标 9），形态校验见 validate_observable_property。
    BuiltinAttrSpec {
        name: "Observable",
        valid_targets: &[AttributeTarget::Property],
        validate_args: validate_no_args,
    },
];

/// 校验：无参数。
fn validate_no_args(args: &[ResolvedArg]) -> Result<(), String> {
    if !args.is_empty() {
        return Err(format!("期望 0 个参数，实际得到 {}", args.len()));
    }
    Ok(())
}

/// 校验：0 或 1 个 string 参数。
fn validate_optional_string_arg(args: &[ResolvedArg]) -> Result<(), String> {
    match args.len() {
        0 => Ok(()),
        1 => match &args[0] {
            ResolvedArg::String(_) => Ok(()),
            other => Err(format!(
                "期望 string 参数，实际得到 {}",
                format_resolved_arg(other)
            )),
        },
        n => Err(format!("期望 0 或 1 个参数，实际得到 {}", n)),
    }
}

/// 校验：必选 1 个 int 参数。
fn validate_required_int_arg(args: &[ResolvedArg]) -> Result<(), String> {
    if args.len() != 1 {
        return Err(format!("期望 1 个 int 参数，实际得到 {}", args.len()));
    }
    match &args[0] {
        ResolvedArg::Int(_) => Ok(()),
        other => Err(format!(
            "期望 int 参数，实际得到 {}",
            format_resolved_arg(other)
        )),
    }
}

/// 格式化 `AttributeTarget` 为人类可读字符串（用于诊断）。
fn format_target(t: AttributeTarget) -> &'static str {
    match t {
        AttributeTarget::Class => "class",
        AttributeTarget::Struct => "struct",
        AttributeTarget::Interface => "interface",
        AttributeTarget::Enum => "enum",
        AttributeTarget::Method => "method",
        AttributeTarget::Property => "property",
        AttributeTarget::Field => "field",
        AttributeTarget::Parameter => "parameter",
        AttributeTarget::EnumMember => "enum member",
    }
}

/// 格式化目标列表为 "class/struct" 形式（用于诊断）。
fn format_targets(targets: &[AttributeTarget]) -> String {
    targets
        .iter()
        .map(|t| format_target(*t))
        .collect::<Vec<_>>()
        .join("/")
}

/// 格式化 `ResolvedArg` 为人类可读字符串（用于诊断）。
fn format_resolved_arg(arg: &ResolvedArg) -> &'static str {
    match arg {
        ResolvedArg::String(_) => "string",
        ResolvedArg::Int(_) => "int",
        ResolvedArg::Bool(_) => "bool",
        ResolvedArg::Type(_) => "type",
        ResolvedArg::Expression(_) => "expression",
        ResolvedArg::Enum { .. } => "enum",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast::{Block, MethodModifier, Type, Visibility};

    fn attr(name: &str, args: Vec<ResolvedArg>, target: AttributeTarget) -> ResolvedAttribute {
        ResolvedAttribute::builtin(Ident::from(name), args, target, ast::Span::DUMMY)
    }

    /// 构造一个仅含 `validate_builtin` 所需字段的 `TypeChecker` 视图。
    /// `validate_builtin` 只读 `attr` 参数，不访问 `self` 字段，故用 `Default`
    /// 风格的占位实例即可。但 `TypeChecker` 没有 `Default` 实现，改为直接
    /// 调用 `validate_builtin` 的等价自由函数逻辑：把 spec 查找与校验
    /// 抽出为独立函数，便于单元测试。
    #[test]
    fn table_accepts_class_target_with_string_arg() {
        // [Table("users")] class User
        let a = attr(
            "Table",
            vec![ResolvedArg::String("users".into())],
            AttributeTarget::Class,
        );
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Table").unwrap();
        assert!(spec.valid_targets.contains(&a.target));
        assert!((spec.validate_args)(&a.args).is_ok());
    }

    #[test]
    fn table_accepts_struct_target_without_args() {
        // [Table] struct LogEntry  （无参，回退到类型名）
        let a = attr("Table", vec![], AttributeTarget::Struct);
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Table").unwrap();
        assert!(spec.valid_targets.contains(&a.target));
        assert!((spec.validate_args)(&a.args).is_ok());
    }

    #[test]
    fn table_rejects_method_target() {
        let a = attr("Table", vec![], AttributeTarget::Method);
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Table").unwrap();
        assert!(!spec.valid_targets.contains(&a.target));
    }

    #[test]
    fn table_rejects_int_arg() {
        let a = attr("Table", vec![ResolvedArg::Int(42)], AttributeTarget::Class);
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Table").unwrap();
        let err = (spec.validate_args)(&a.args).unwrap_err();
        assert!(err.contains("期望 string"));
    }

    #[test]
    fn column_accepts_property_and_field_targets() {
        for target in [AttributeTarget::Property, AttributeTarget::Field] {
            let a = attr("Column", vec![ResolvedArg::String("c".into())], target);
            let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Column").unwrap();
            assert!(spec.valid_targets.contains(&a.target));
            assert!((spec.validate_args)(&a.args).is_ok());
        }
    }

    #[test]
    fn column_rejects_class_target() {
        let a = attr("Column", vec![], AttributeTarget::Class);
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Column").unwrap();
        assert!(!spec.valid_targets.contains(&a.target));
    }

    #[test]
    fn key_and_required_accept_no_args_on_field() {
        for name in ["Key", "Required"] {
            let a = attr(name, vec![], AttributeTarget::Field);
            let spec = BUILTIN_SPECS.iter().find(|s| s.name == name).unwrap();
            assert!(spec.valid_targets.contains(&a.target));
            assert!((spec.validate_args)(&a.args).is_ok());
        }
    }

    #[test]
    fn key_rejects_extra_args() {
        let a = attr(
            "Key",
            vec![ResolvedArg::String("x".into())],
            AttributeTarget::Field,
        );
        let spec = BUILTIN_SPECS.iter().find(|s| s.name == "Key").unwrap();
        let err = (spec.validate_args)(&a.args).unwrap_err();
        assert!(err.contains("期望 0 个参数"));
    }

    #[test]
    fn maxlength_requires_int_arg_on_property() {
        let a = attr(
            "MaxLength",
            vec![ResolvedArg::Int(100)],
            AttributeTarget::Property,
        );
        let spec = BUILTIN_SPECS
            .iter()
            .find(|s| s.name == "MaxLength")
            .unwrap();
        assert!(spec.valid_targets.contains(&a.target));
        assert!((spec.validate_args)(&a.args).is_ok());
    }

    #[test]
    fn maxlength_rejects_string_arg() {
        let a = attr(
            "MaxLength",
            vec![ResolvedArg::String("abc".into())],
            AttributeTarget::Property,
        );
        let spec = BUILTIN_SPECS
            .iter()
            .find(|s| s.name == "MaxLength")
            .unwrap();
        let err = (spec.validate_args)(&a.args).unwrap_err();
        assert!(err.contains("期望 int"));
    }

    #[test]
    fn maxlength_rejects_missing_arg() {
        let a = attr("MaxLength", vec![], AttributeTarget::Property);
        let spec = BUILTIN_SPECS
            .iter()
            .find(|s| s.name == "MaxLength")
            .unwrap();
        let err = (spec.validate_args)(&a.args).unwrap_err();
        assert!(err.contains("期望 1 个 int"));
    }

    #[test]
    fn non_builtin_attr_passes_validation() {
        // [Foo] 不是内置属性，Phase 0 不校验
        let found = BUILTIN_SPECS.iter().find(|s| s.name == "Foo");
        assert!(found.is_none());
        // validate_builtin 中 found 为 None 时直接返回 Ok(())
    }

    #[test]
    fn target_to_bit_matches_std_constants() {
        // 与 std/Arc/Attribute.as 中 AttributeTargets 类的 const int 值对齐
        assert_eq!(target_to_bit(AttributeTarget::Class), 1);
        assert_eq!(target_to_bit(AttributeTarget::Struct), 2);
        assert_eq!(target_to_bit(AttributeTarget::Interface), 4);
        assert_eq!(target_to_bit(AttributeTarget::Enum), 8);
        assert_eq!(target_to_bit(AttributeTarget::Method), 16);
        assert_eq!(target_to_bit(AttributeTarget::Property), 32);
        assert_eq!(target_to_bit(AttributeTarget::Field), 64);
        assert_eq!(target_to_bit(AttributeTarget::Parameter), 128);
    }

    #[test]
    fn arg_matches_type_basic_rules() {
        assert!(arg_matches_type(&ResolvedArg::Int(42), &Ident::from("int")));
        assert!(arg_matches_type(
            &ResolvedArg::String("x".into()),
            &Ident::from("string")
        ));
        assert!(arg_matches_type(
            &ResolvedArg::Bool(true),
            &Ident::from("bool")
        ));
        assert!(!arg_matches_type(
            &ResolvedArg::Int(42),
            &Ident::from("string")
        ));
        assert!(!arg_matches_type(
            &ResolvedArg::String("x".into()),
            &Ident::from("int")
        ));
    }

    /// 构造属性形态（RFC 037 M-D0 单测辅助）。
    ///
    /// `has_set` / `has_init` 对应 `{ get; set; }` / `{ get; init; }`；
    /// `get_body` / `set_body` 为 true 表示显式访问器体（custom-accessor）。
    fn prop(
        name: &str,
        has_set: bool,
        has_init: bool,
        get_body: bool,
        set_body: bool,
    ) -> PropertyDef {
        PropertyDef {
            vis: Visibility::Public,
            name: Ident::from(name),
            ty: Type::named("string"),
            has_get: true,
            has_set,
            has_init,
            is_required: false,
            get_body: if get_body {
                Some(Block {
                    stmts: vec![],
                    tail: None,
                })
            } else {
                None
            },
            set_body: if set_body {
                Some(Block {
                    stmts: vec![],
                    tail: None,
                })
            } else {
                None
            },
            get_vis: None,
            set_vis: None,
            modifier: MethodModifier::None,
            attributes: vec![],
            is_static_abstract: false,
            index_params: vec![],
            init: None,
            doc: None,
        }
    }

    #[test]
    fn observable_accepts_property_target_with_no_args() {
        // [Observable] public string Name { get; set; }  —— spec 层：目标 + 无参
        let a = attr("Observable", vec![], AttributeTarget::Property);
        let spec = BUILTIN_SPECS
            .iter()
            .find(|s| s.name == "Observable")
            .unwrap();
        assert!(spec.valid_targets.contains(&a.target));
        assert!((spec.validate_args)(&a.args).is_ok());
    }

    #[test]
    fn observable_rejects_non_property_target() {
        // [Observable] 标在 Class/Field/Method 等非 Property 目标 → 目标不匹配
        for target in [
            AttributeTarget::Class,
            AttributeTarget::Struct,
            AttributeTarget::Field,
            AttributeTarget::Method,
        ] {
            let a = attr("Observable", vec![], target);
            let spec = BUILTIN_SPECS
                .iter()
                .find(|s| s.name == "Observable")
                .unwrap();
            assert!(!spec.valid_targets.contains(&a.target));
        }
    }

    #[test]
    fn observable_rejects_extra_args() {
        let a = attr(
            "Observable",
            vec![ResolvedArg::String("x".into())],
            AttributeTarget::Property,
        );
        let spec = BUILTIN_SPECS
            .iter()
            .find(|s| s.name == "Observable")
            .unwrap();
        let err = (spec.validate_args)(&a.args).unwrap_err();
        assert!(err.contains("期望 0 个参数"));
    }

    #[test]
    fn observable_accepts_writable_auto_property() {
        // { get; set; } / { get; private set; } —— has_set=true 的可写 auto-property
        let a = attr("Observable", vec![], AttributeTarget::Property);
        assert!(
            validate_observable_property(&a, Some(&prop("Name", true, false, false, false)))
                .is_ok()
        );
        assert!(
            validate_observable_property(&a, Some(&prop("Age", true, false, false, false))).is_ok()
        );
    }

    #[test]
    fn observable_rejects_get_only_auto_property() {
        // RFC §13 M-D0 验收样例：get-only 属性标注 [Observable] = 编译期错误
        let a = attr("Observable", vec![], AttributeTarget::Property);
        let err =
            validate_observable_property(&a, Some(&prop("Items", false, false, false, false)))
                .unwrap_err();
        assert!(err.to_string().contains("Observable"));
        assert!(err.to_string().contains("get-only"));
        // init-only（{ get; init; }）亦无 set 触发点，按同一裁定一并拒绝
        let err = validate_observable_property(&a, Some(&prop("Items", false, true, false, false)))
            .unwrap_err();
        assert!(err.to_string().contains("get-only"));
    }

    #[test]
    fn observable_skips_custom_accessor_property() {
        // custom-accessor 不插桩、不报错（RFC §5.3）
        let a = attr("Observable", vec![], AttributeTarget::Property);
        // custom getter 体
        assert!(validate_observable_property(
            &a,
            Some(&prop("FullName", false, false, true, false))
        )
        .is_ok());
        // custom setter 体
        assert!(validate_observable_property(
            &a,
            Some(&prop("FullName", true, false, false, true))
        )
        .is_ok());
    }

    #[test]
    fn observable_without_property_context_is_noop() {
        // 非 Property 目标不经过形态校验（valid_targets 已拒绝）
        let a = attr("Observable", vec![], AttributeTarget::Class);
        assert!(validate_observable_property(&a, None).is_ok());
    }
}
