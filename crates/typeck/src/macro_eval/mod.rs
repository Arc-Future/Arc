//! RFC 012 M4 宏特性代码注入体系——typeck 侧数据结构与识别逻辑。
//!
//! 本 crate 模块定义 M4 宏特性的核心数据结构与识别 API：
//!
//! - [`MacroContainer`]：宏容器类（其 public 方法为「展开槽位」）
//! - [`MacroFeature`]：派生自 `GenerateToAttribute<T>` 的宏特性派生类
//! - [`MacroSlot`]：宏容器的 public 方法槽位元数据
//! - [`MacroCatalog`]：宏容器 + 宏特性的统一目录
//!
//! 子模块：
//! - [`evaluator`]：M4-4 受限求值器核心——执行 `Action<StringBuilder>` 委托体
//! - [`whitelist`]：M4-4/M4-5 受限子集白名单
//!
//! # 识别规则（RFC 009 D9.2 / D9.3，v1.0 修订）
//!
//! - **宏容器**：v1.0 改为**反向推断**——扫描 `GenerateToAttribute<T>` 派生类
//!   的 T 参数，T 即为容器类。容器无需任何特殊标注（v0.11 修订移除了
//!   `[GenerateTo]` 属性标记与非泛型 `GenerateToAttribute` 标记类）。
//!   容器本身是普通类（可静态、可有任意方法），其 public 方法成为展开槽位。
//! - **宏特性**：类的 `bases` 中存在 `GenerateToAttribute<T>`（泛型实例化），
//!   其中 `T` 为宏容器类名。派生类在构造函数中通过
//!   `this.<T 的某 public 方法>(Action<StringBuilder> expansion)` 注册展开委托
//!   （由 M4-3 识别，M4-4 求值，M4-6 注入）。
//!
//! # 调用时机
//!
//! 识别需在 typeck 处理完所有类后调用（[`TypeChecker::collect_macros`]），
//! 因宏容器与宏特性可能相互引用（容器与特性分处不同类）。
//!
//! # 架构红线
//!
//! 本模块是编译器**通用机制**——只识别「标了哪个属性」「派生自哪个基类」，
//! 不感知「依赖注入」「ORM」等领域语义。领域应用由 arc-orm / 业务代码完成。

use ast::{Block, Expr, ExpressionTree, Ident, LambdaExpr, Span, Spanned, Stmt, Type, Visibility};
use indexmap::IndexMap;
use std::collections::HashSet;

use crate::checker::TypeChecker;
use crate::error::TypeError;

/// M4 D10.6 构造函数体编译期解释器（RFC 032 QIF 路径扩展）。
///
/// 解释 `GenerateToAttribute<T>` 派生类构造函数体中的 `if`/`foreach`/`is`
/// 等控制流嵌套，识别其中的 `this.<slot>(lambda)` 注册调用。D10.6 是
/// typeck 通用机制，不感知 QIF 语义——详见 RFC 009 M4 D10.6。
pub mod ctor_interpreter;
/// M4-4 受限求值器核心——执行 `Action<StringBuilder>` 委托体。
pub mod evaluator;
/// M4-6 展开字符串解析 + span 映射。
pub mod splice;
/// M4-4/M4-5 受限子集白名单。
pub mod whitelist;

// RFC 032 D-1a: D10.6 解释器集成所需类型导入（必须在 `pub mod` 声明之后）。
// - `Value`/`ClassExpressionValue`/`MethodExpressionValue`：构造解释器输入值
// - `CtorInterpreter`/`Environment`/`CtorInterpError`：解释器主体与诊断
use ctor_interpreter::{CtorInterpError, CtorInterpreter, Environment};
use evaluator::{AttributeDataValue, ClassExpressionValue, MethodExpressionValue, Value};

/// 宏容器类（RFC 009 D9.2，v1.0 修订）。
///
/// v1.0 改为反向推断识别——扫描 `GenerateToAttribute<T>` 派生类的 T 参数，
/// T 即为容器类。容器无需任何特殊标注（v0.11 修订移除了 `[GenerateTo]`
/// 属性标记与非泛型 `GenerateToAttribute` 标记类）。
/// 其每个 public 方法（实例方法或静态方法）成为「展开槽位」，
/// 由宏特性派生类通过 `this.<方法名>(Action<StringBuilder>)` 注册展开委托，
/// 编译器在 Pass 3 求值委托并将 StringBuilder 累积内容 splice 到对应方法体前部。
#[derive(Clone, Debug)]
pub struct MacroContainer {
    /// 宏容器类名。
    pub class_name: Ident,
    /// 展开槽位列表（容器类的所有 public 方法）。
    pub slots: Vec<MacroSlot>,
}

/// 宏容器的 public 方法槽位（RFC 009 D9.2）。
///
/// 槽位是宏容器类的 public 方法（实例或静态），编译器在 Pass 3
/// 将宏特性注册的展开代码 splice 到槽位方法体。槽位方法本身可
/// 已有开发者编写的业务代码——M4-6 splice 策略是「追加」而非「替换」。
#[derive(Clone, Debug)]
pub struct MacroSlot {
    /// 方法名（如 `Register` / `Test`）。
    pub method_name: Ident,
    /// 方法参数类型名列表（与 [`OopMethodSig::params`] 对齐）。
    pub param_types: Vec<Ident>,
    /// 方法返回类型名（`void` 表示无返回值）。
    pub return_type: Ident,
    /// 方法修饰符（静态 / 实例 / async 等）。
    pub modifier: MethodModifier,
    /// 是否为 async 方法。
    pub is_async: bool,
}

/// 复用 ast 的 `MethodModifier`，避免在本模块重复定义。
pub use ast::MethodModifier;

/// 宏特性派生类（RFC 009 D9.3，v1.0 修订）。
///
/// 派生自 `GenerateToAttribute<T>` 的类是宏特性派生类：
/// 泛型参数 `T` 关联一个宏容器类。派生类在构造函数中通过
/// `this.<T 的某 public 方法>(Action<StringBuilder> expansion)` 注册展开委托。
///
/// 编译器在 Pass 3 调用受限求值器求值委托，把 StringBuilder 累积内容解析为
/// Arc 代码片段并 splice 到 T 容器类对应方法体前部。
#[derive(Clone, Debug)]
pub struct MacroFeature {
    /// 宏特性派生类名（如 `InjectAttribute`）。
    pub class_name: Ident,
    /// 关联的宏容器类名（泛型实参 `T`）。
    pub container: Ident,
    /// 派生类的构造函数列表（用于 M4-3 识别 `this.<method>(Action<StringBuilder>)` 调用）。
    pub constructors: Vec<MacroFeatureCtor>,
    /// RFC 009 M4-3: 构造函数中识别到的展开委托注册。
    ///
    /// 形如 `this.<slot_method>(<lambda>)` 的调用被识别为一次注册：
    /// `slot_method` 必须是关联容器 T 的某个 public 方法名，
    /// `<lambda>` 必须是 `Action<StringBuilder>` 类型的 Lambda。
    /// M4-4 受限求值器据此求值 lambda body 把代码片段累积到 StringBuilder。
    pub registrations: Vec<MacroRegistration>,
}

/// RFC 009 M4-3: 宏特性派生类构造函数中识别到的展开委托注册。
///
/// 派生类构造函数中形如 `this.<slot_method>(<lambda>)` 的调用：
/// - `slot_method` 必须是关联容器 T 的某个 public 方法名（即在
///   `MacroCatalog.slots_of(T)` 中）
/// - `<lambda>` 必须是 `Action<StringBuilder>` 类型的 Lambda（单参 StringBuilder，返回 void）
///
/// 编译器在 M4-4 调用受限求值器求值 `expansion` 的 body，把代码片段累积到
/// StringBuilder；M4-6 将 StringBuilder 最终内容解析为 AST 片段并 splice 到
/// `slot_method` 对应的容器方法体前部。
#[derive(Clone, Debug)]
pub struct MacroRegistration {
    /// 容器方法名（展开槽位）—— 必须存在于 `MacroContainer.slots` 中。
    pub slot_name: Ident,
    /// `Action<StringBuilder>` 委托的 Lambda AST（M4-4 求值器输入）。
    pub expansion: LambdaExpr,
    /// 调用表达式的源码位置（M4-6 splice 诊断锚点）。
    pub span: Span,
    /// RFC 009 M4-7: 求值时注入的 Expression 绑定（形参名 → ExpressionTree）。
    ///
    /// 由 Pass 3 扫描被赋能类（标了 feature attribute 的类）的 attribute
    /// 实例生成：attribute 位置参数中的 Lambda 树化为 ExpressionTree 后，
    /// 按 feature 构造函数 Expression 形参名绑定。求值器在求值 `expansion`
    /// 之前调用 `inject_expression_locals` 把这些绑定注入 locals 环境。
    ///
    /// 空Vec表示该注册无 Expression 绑定（feature 构造函数无 Expression 形参，
    /// 或被赋能类的 attribute 实例无 Lambda 位置参数）。
    pub expression_locals: Vec<(Ident, ExpressionTree)>,
}

/// 宏特性派生类的构造函数元数据（RFC 009 D9.3）。
///
/// 用于 M4-3 在构造函数体内扫描 `this.<T方法>(Func<string>)` 调用模式。
#[derive(Clone, Debug)]
pub struct MacroFeatureCtor {
    /// 构造函数参数类型名列表。
    pub param_types: Vec<Ident>,
    /// RFC 009 M4-7: 构造函数参数完整信息（名 + 类型 + 是否 Expression）。
    ///
    /// 由 `collect_feature_ctors` 从 `class_defs` 中提取，供 Pass 3 在
    /// 被赋能类 attribute 实例中按形参名匹配 Expression 实参。
    pub params: Vec<MacroFeatureCtorParam>,
}

/// RFC 009 M4-7: 宏特性派生类构造函数参数元数据。
///
/// 单个参数的完整信息：参数名、类型名、是否为 `Expression<T>` 类型。
/// Pass 3 据此把被赋能类 attribute 实例的 Lambda 位置实参绑定到对应
/// 形参名，生成 `MacroRegistration.expression_locals`。
#[derive(Clone, Debug)]
pub struct MacroFeatureCtorParam {
    /// 参数名（如 `expression` / `selector`）。
    pub name: Ident,
    /// 参数类型名（如 `Expression` / `Expression<Func<int,bool>>` 简化为 `Expression`）。
    pub ty: Ident,
    /// 是否为 `Expression<T>` 或 `Expression` 类型参数。
    ///
    /// 判定规则（`is_expression_type`）：参数类型为 `Expression`（无泛型）
    /// 或 `Expression<...>`（带泛型实参）。其他类型（如 `Type` / `string`）
    /// 返回 false。
    pub is_expression: bool,
}

/// 宏目录（容器 + 特性 + Source Generator）。
#[derive(Clone, Debug, Default)]
pub struct MacroCatalog {
    /// 类名 → 宏容器信息。
    pub containers: IndexMap<Ident, MacroContainer>,
    /// 派生类名 → 宏特性信息。
    pub features: IndexMap<Ident, MacroFeature>,
    /// RFC 009 M5-2: 类名 → Source Generator 信息。
    pub source_generators: IndexMap<Ident, SourceGenerator>,
}

/// RFC 009 M5-2: Source Generator 元数据。
///
/// 被 `[SourceGenerator]` 标记且实现 `IGenerator` 接口的类。
/// 编译器在 Pass 3 调用其 `Generate(GeneratorContext)` 方法，
/// 返回的字符串列表中每个字符串被解析为独立的 Arc 源文件
/// 并追加到当前编译单元。
///
/// `generate_method_body` 保存 `Generate` 方法的 AST 方法体，
/// 供受限求值器（M5-3）求值。求值结果为 `List<string>`，
/// 每个元素是一个 Arc 源代码字符串。
///
/// RFC 009 M5-2b: `context_param_name` 保存 `Generate` 方法首个形参名
/// （如 `Generate(GeneratorContext context)` 中的 `context`），供
/// Pass 3 `expand_source_generators` 把构造好的 `GeneratorContext` 值
/// 注入求值器 locals——使方法体内 `context.Attributes` 等访问能解析到
/// 真实 typeck 产物。None 表示 Generate 方法无参数（兼容 M5-3 旧路径）。
#[derive(Clone, Debug)]
pub struct SourceGenerator {
    /// 生成器类名（如 `DtoGenerator`）。
    pub class_name: Ident,
    /// `Generate(GeneratorContext)` 方法的 AST 方法体。
    /// None 表示方法体缺失或无法识别（Pass 2 会报错）。
    pub generate_method_body: Option<Block>,
    /// RFC 009 M5-2b: Generate 方法首个形参名（如有）。
    /// Pass 3 据此把 `GeneratorContext` 值注入求值器 locals。
    pub context_param_name: Option<Ident>,
    /// `Generate` 方法声明处的 span（诊断锚点）。
    pub span: Span,
}

impl MacroCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// 查询类名是否为宏容器。
    pub fn is_container(&self, name: &Ident) -> bool {
        self.containers.contains_key(name)
    }

    /// 查询类名是否为宏特性派生类。
    pub fn is_feature(&self, name: &Ident) -> bool {
        self.features.contains_key(name)
    }

    /// 按容器类名查询其展开槽位列表。
    pub fn slots_of(&self, container: &Ident) -> Option<&[MacroSlot]> {
        self.containers.get(container).map(|c| c.slots.as_slice())
    }

    /// 按宏特性类名查询其关联的容器名。
    pub fn container_of(&self, feature: &Ident) -> Option<&Ident> {
        self.features.get(feature).map(|f| &f.container)
    }

    /// RFC 009 M5-2: 查询类名是否为 Source Generator。
    pub fn is_source_generator(&self, name: &Ident) -> bool {
        self.source_generators.contains_key(name)
    }
}

impl TypeChecker {
    /// RFC 032 v0.11: 反向推断宏容器类名集合。
    ///
    /// 与 [`collect_macros`](Self::collect_macros) Pass 1b 使用相同识别规则：
    /// 扫描 `registry.types` 中所有 class/static class，若其 `base_types`
    /// 含 `GenerateToAttribute<T>` 形式，则 T 被收入容器集合。
    ///
    /// **何时调用**：必须在 `TypeRegistry::from_module` 完成（`base_types`
    /// 已基于 AST 填充）之后调用。`from_module` 不依赖 typeck 后续处理，
    /// 故可在 `check_module_items` 之前预计算——供 Pass 2 骨架模式
    /// `check_class` 决定是否跳过宏容器方法体检查。
    ///
    /// 与 `collect_macros` 的差异：本方法仅返回容器类名集合（轻量预计算），
    /// `collect_macros` 在 `check_module` 末尾调用，构建完整 `MacroCatalog`
    /// （含 slots / features / registrations 等元数据）。
    pub(crate) fn compute_macro_container_names(&self) -> HashSet<Ident> {
        let mut names = HashSet::new();
        for (_name, nom) in &self.registry.types {
            // 跳过非 class 类型（struct / interface / enum 不参与宏体系）
            if !matches!(
                nom.kind,
                crate::oop_types::TypeKind::Class | crate::oop_types::TypeKind::StaticClass
            ) {
                continue;
            }
            if let Some(container) = self.find_generate_to_base(nom) {
                names.insert(container);
            }
        }
        names
    }

    /// RFC 012 M4-2: 扫描已注册类型，构建宏目录。
    ///
    /// 必须在所有类 typeck 完成后调用（如 `check_module` 末尾）。
    /// 返回的目录包含两类条目：
    /// - **宏容器**：通过 features 反向推断识别（v0.11 修订——移除
    ///   `[GenerateTo]` 属性标记检查，与 [`compute_macro_container_names`]
    ///   使用相同规则）
    /// - **宏特性**：registry.types 中 `base_types` 含
    ///   `GenerateToAttribute<T>` 的类
    ///
    /// 泛型类模板（如 `GenerateToAttribute<T>` 自身）不在 registry.types 中
    /// （registry.rs 已跳过 generics 非空的类），故不会出现在 features 列表中——
    /// 仅用户派生的非泛型类（如 `class InjectAttribute : GenerateToAttribute<X>`）
    /// 会被识别为 feature。
    ///
    /// RFC 012 M4-3: 在目录构建后，对每个 feature 扫描其构造函数 AST，
    /// 识别 `this.<slot_method>(<lambda>)` 调用为展开委托注册。两遍扫描：
    /// 第一遍识别容器与特性派生类（填充 slots 与基本元数据）；
    /// 第二遍针对每个 feature 在已建好的 catalog 上下文中查找其 ctor body，
    /// 据此填充 `MacroFeature.registrations`。
    ///
    /// RFC 009 M4-7: 第三遍扫描被赋能类（标了 feature attribute 的类），
    /// 把 attribute 实例的 Expression 位置参数绑定到 feature 构造函数
    /// Expression 形参名，生成带 `expression_locals` 的 MacroRegistration
    /// 副本。返回 `(catalog, errors)`——errors 累积 Pass 3 中的诊断
    /// （如 attribute 实参数量与构造函数 Expression 形参数量不匹配）。
    pub fn collect_macros(&self) -> (MacroCatalog, Vec<TypeError>) {
        let mut catalog = MacroCatalog::new();
        let mut macro_errors: Vec<TypeError> = Vec::new();

        // Pass 1: 识别宏特性派生类 + Source Generator
        //
        // RFC 012 v0.11 修订：容器识别完全通过 features 反向推断——
        // 移除原 `[GenerateTo]` 属性标记检查。扫描所有
        // `GenerateToAttribute<T>` 派生类的 T 参数，T 即为容器类。
        // 容器类无需任何特殊标注（如 [GenerateTo]），普通业务类即可。
        for (name, nom) in &self.registry.types {
            // 跳过非 class 类型（struct / interface / enum 不参与宏体系）
            if !matches!(
                nom.kind,
                crate::oop_types::TypeKind::Class | crate::oop_types::TypeKind::StaticClass
            ) {
                continue;
            }

            // 1. 宏特性识别：base_types 中查找 GenerateToAttribute<T> 形式
            // RFC 009 M4-7: 同时提取构造函数完整参数信息（含 Expression 形参判定）。
            if let Some(container) = self.find_generate_to_base(nom) {
                let ctors = self.collect_feature_ctors(name);
                catalog.features.insert(
                    name.clone(),
                    MacroFeature {
                        class_name: name.clone(),
                        container,
                        constructors: ctors,
                        registrations: Vec::new(),
                    },
                );
            }

            // 2. RFC 009 M5-2: Source Generator 识别
            //    类自身标 [SourceGenerator] / [SourceGeneratorAttribute]
            //    且实现 IGenerator 接口（方法体由 collect_generate_method 提取）
            if self.has_source_generator_attr(name) {
                let (generate_body, context_param_name, span) = self.collect_generate_method(name);
                catalog.source_generators.insert(
                    name.clone(),
                    SourceGenerator {
                        class_name: name.clone(),
                        generate_method_body: generate_body,
                        context_param_name,
                        span,
                    },
                );
            }
        }

        // Pass 1b: 容器反向推断——扫描所有 features 的 container 字段，
        // 为每个唯一容器名查找 NominalType 并收集 slots，插入 catalog.containers。
        //
        // RFC 012 v0.11：容器无需 [GenerateTo] 标注，完全通过 features 反向推断。
        // 若容器类未在当前 registry.types 中（如预编译外部库），插入空 slots 占位
        // ——方案 A（埋点配对）不依赖 slots，方案 B（Build 前置插入）在 splice
        // 阶段单独处理 Build 方法查找。
        let container_names: Vec<Ident> = catalog
            .features
            .values()
            .map(|f| f.container.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        for container_name in container_names {
            if catalog.containers.contains_key(&container_name) {
                continue;
            }
            let slots = if let Some(nom) = self.registry.types.get(&container_name) {
                self.collect_slots(nom)
            } else {
                // 容器类不在当前 registry（预编译外部库）——空 slots 占位。
                // 方案 A 埋点配对不依赖 slots；方案 B Build 前置插入在 splice
                // 阶段通过 class_defs 查找 Build 方法。
                Vec::new()
            };
            catalog.containers.insert(
                container_name.clone(),
                MacroContainer {
                    class_name: container_name,
                    slots,
                },
            );
        }

        // Pass 2 (M4-3): 对每个 feature 扫描构造函数体识别展开委托注册。
        // 容器与特性派生类均已就位，可安全查询 catalog.slots_of(container)。
        // RFC 009 M4-7: 借用迭代（`&feature_names`）以便 Pass 3 再次借用。
        let feature_names: Vec<Ident> = catalog.features.keys().cloned().collect();
        for feature_name in &feature_names {
            let container_name = catalog
                .features
                .get(feature_name)
                .map(|f| f.container.clone())
                .unwrap_or_else(|| Ident::from(""));
            let slots = catalog
                .slots_of(&container_name)
                .unwrap_or(&[])
                .iter()
                .map(|s| s.method_name.clone())
                .collect::<HashSet<_>>();
            let registrations = self.collect_feature_registrations(feature_name, &slots);
            if let Some(feature) = catalog.features.get_mut(feature_name) {
                feature.registrations = registrations;
            }
        }

        // Pass 3 (M4-7): 扫描被赋能类（标了 feature attribute 的类），
        // 把 attribute 实例的 Expression 位置参数绑定到 feature 构造函数
        // Expression 形参名，生成带 expression_locals 的 MacroRegistration 副本。
        for feature_name in &feature_names {
            let ctor_expression_params: Vec<Ident> = catalog
                .features
                .get(feature_name)
                .expect("feature exists in catalog")
                .constructors
                .first()
                .map(|c| {
                    c.params
                        .iter()
                        .filter(|p| p.is_expression)
                        .map(|p| p.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            if ctor_expression_params.is_empty() {
                continue;
            }
            // 取出 Pass 2 生成的基础注册作为模板，避免在原地表修改时借用冲突。
            let template_regs: Vec<MacroRegistration> = std::mem::take(
                &mut catalog
                    .features
                    .get_mut(feature_name)
                    .expect("feature exists in catalog")
                    .registrations,
            );
            // RFC 032 D-1a: 查询关联容器槽位列表，供 D10.6 解释器识别
            // `this.<slot>(lambda)` 调用合法性。容器名来自 Pass 1 catalog。
            let container_name = catalog
                .features
                .get(feature_name)
                .map(|f| f.container.clone())
                .unwrap_or_else(|| Ident::from(""));
            let slots: Vec<MacroSlot> = catalog.slots_of(&container_name).unwrap_or(&[]).to_vec();
            let (expanded_regs, errs) = self.expand_feature_registrations_with_locals(
                feature_name,
                &ctor_expression_params,
                &template_regs,
                &slots,
            );
            macro_errors.extend(errs);
            if let Some(feature) = catalog.features.get_mut(feature_name) {
                feature.registrations = expanded_regs;
            }
        }

        (catalog, macro_errors)
    }

    /// RFC 009 M4-7: 从 `class_defs` 中提取 feature 派生类的构造函数
    /// 完整参数信息（名 + 类型 + 是否 Expression）。
    ///
    /// 与 Pass 1 中仅基于 `nom.constructors` 提取 `param_types` 不同，本方法
    /// 直接读取原始 AST `ClassDef.constructors[i].params`，提取每个参数的
    /// `name` / `ty` / `is_expression` 三元组，供 Pass 3 在被赋能类 attribute
    /// 实例中按形参名匹配 Expression 实参。
    ///
    /// 若类未在 `class_defs` 中（理论不可达，因 Pass 1 已识别为 feature），
    /// 返回空 Vec——Pass 3 据此跳过该 feature 的 expression_locals 生成。
    fn collect_feature_ctors(&self, feature_name: &Ident) -> Vec<MacroFeatureCtor> {
        let Some(class_def) = self.class_defs.get(feature_name) else {
            return Vec::new();
        };
        class_def
            .constructors
            .iter()
            .map(|ctor| {
                let params: Vec<MacroFeatureCtorParam> = ctor
                    .node
                    .params
                    .iter()
                    .map(|p| {
                        let ty = type_name_of(&p.ty.node).unwrap_or_else(|| Ident::from("unknown"));
                        let is_expression = is_expression_type(&p.ty.node);
                        MacroFeatureCtorParam {
                            name: p.name.clone(),
                            ty,
                            is_expression,
                        }
                    })
                    .collect();
                let param_types: Vec<Ident> = ctor
                    .node
                    .params
                    .iter()
                    .map(|p| type_name_of(&p.ty.node).unwrap_or_else(|| Ident::from("unknown")))
                    .collect();
                MacroFeatureCtor {
                    param_types,
                    params,
                }
            })
            .collect()
    }

    /// RFC 009 M4-7: 扫描被赋能类（标了 `feature_name` attribute 的类），
    /// 把 attribute 实例的 Expression 位置参数绑定到 feature 构造函数
    /// Expression 形参名，生成带 `expression_locals` 的 MacroRegistration 副本。
    ///
    /// RFC 032 D-1a 扩展：除原类级 attribute 扫描外，新增**方法级 attribute 扫描**——
    /// 当类的方法标了 `feature_name` attribute（如 `[Fact] void TestMethod()`）时，
    /// typeck 自动构造 `Value::ClassExpression`（含类名 + 方法签名列表 + 类级 attribute
    /// 名），调用 D10.6 构造函数体解释器解释派生类 ctor body 中的
    /// `if (expression is ClassExpression classDef) foreach (var m in classDef.Methods)`
    /// 控制流，识别嵌套其中的 `this.<slot>(lambda)` 注册调用。
    ///
    /// 流程：
    /// 1. 遍历 `class_defs` 中所有类，查询其**类级** attribute 实例是否标了
    ///    `feature_name`（含 Attribute 后缀省略匹配）。
    /// 2. 对每个被赋能类的 attribute 实例，提取位置参数中的 Lambda
    ///    （已由 typeck 树化为 `ResolvedArg::Expression`）。
    /// 3. 按 feature 构造函数 Expression 形参名顺序绑定，生成
    ///    `(name, ExpressionTree)` 列表。
    /// 4. 为每个 template registration 生成一个副本，附上 expression_locals。
    /// 5. （D-1a 新增）扫描类中所有方法的**方法级** attribute，对匹配的方法
    ///    构造 ClassExpression Value 并调用 D10.6 解释器解释派生类 ctor body。
    ///
    /// 返回 `(expanded_regs, errors)`——errors 累积诊断（如 Expression
    /// 实参数量与形参数量不匹配），不中断扫描。
    fn expand_feature_registrations_with_locals(
        &self,
        feature_name: &Ident,
        ctor_expression_params: &[Ident],
        template_regs: &[MacroRegistration],
        slots: &[MacroSlot],
    ) -> (Vec<MacroRegistration>, Vec<TypeError>) {
        let mut errors: Vec<TypeError> = Vec::new();
        let mut expanded_regs: Vec<MacroRegistration> = Vec::new();

        // 候选 attribute 名：feature_name 与其 C# 短/长形式。
        // C# 规范允许 Attribute 后缀省略：[Inject] 与 [InjectAttribute] 等价。
        // feature_name 形如 `InjectAttribute` 时，候选含 `InjectAttribute` 与
        // 去后缀的 `Inject`；形如 `Inject` 时，候选含 `Inject` 与 `InjectAttribute`。
        let attr_candidates: Vec<Ident> = if feature_name.as_str().ends_with("Attribute") {
            let short = &feature_name.as_str()[..feature_name.as_str().len() - "Attribute".len()];
            vec![feature_name.clone(), Ident::from(short.to_string())]
        } else {
            vec![
                feature_name.clone(),
                Ident::from(format!("{}Attribute", feature_name.as_str())),
            ]
        };

        // D-1a: 查找 feature 派生类的构造函数体（用于 D10.6 解释器调用）。
        // 仅当 feature 有 ctor body 且至少一个 Expression 形参时启用方法级扫描。
        let feature_ctor_body = self.find_feature_ctor_body(feature_name);
        let expression_param_name: Option<&Ident> = ctor_expression_params.first();

        // 扫描所有类，查找标了 attr_candidates 中任一 attribute 的被赋能类
        for (class_name, class_def) in &self.class_defs {
            // 查询 attribute_table 中此类的所有 attribute 实例
            let Some(def_id) = self.class_def_ids.get(class_name).copied() else {
                continue;
            };

            // ── 1. 类级 attribute 扫描（原 Pass 3 逻辑）──
            let attrs = self.attribute_table.get_attrs(def_id);
            for attr in attrs {
                if !attr_candidates.iter().any(|c| c == &attr.name) {
                    continue;
                }
                // 此类是被赋能类——提取位置参数中的 Expression 实参
                let expression_args: Vec<&crate::ResolvedArg> = attr
                    .args
                    .iter()
                    .filter(|a| matches!(a, crate::ResolvedArg::Expression(_)))
                    .collect();
                // 校验：Expression 实参数量必须与构造函数 Expression 形参数量一致
                if expression_args.len() != ctor_expression_params.len() {
                    errors.push(TypeError::Macro {
                        code: "arc-macro-030",
                        message: format!(
                            "feature `{}` constructor expects {} Expression param(s) but \
                             attribute instance on class `{}` has {} Lambda arg(s)",
                            feature_name,
                            ctor_expression_params.len(),
                            class_name,
                            expression_args.len()
                        ),
                    });
                    continue;
                }
                // 按形参名顺序绑定 Expression 实参
                let mut expression_locals: Vec<(Ident, ExpressionTree)> = Vec::new();
                for (param_name, arg) in ctor_expression_params.iter().zip(expression_args.iter()) {
                    let crate::ResolvedArg::Expression(tree) = arg else {
                        unreachable!("filtered above");
                    };
                    expression_locals.push((param_name.clone(), tree.clone()));
                }
                // 为每个 template registration 生成带 expression_locals 的副本
                for template in template_regs {
                    expanded_regs.push(MacroRegistration {
                        slot_name: template.slot_name.clone(),
                        expansion: template.expansion.clone(),
                        span: template.span,
                        expression_locals: expression_locals.clone(),
                    });
                }
            }

            // ── 2. 方法级 attribute 扫描（D-1a 新增）──
            // 仅当 feature 有 ctor body 且有 Expression 形参时执行——D10.6 解释器
            // 需要构造函数体作为输入，并将 ClassExpression 绑定到 Expression 形参名。
            if let (Some(body), Some(expr_param_name)) = (feature_ctor_body, expression_param_name)
            {
                for method in &class_def.methods {
                    let method_key = (class_name.clone(), method.node.sig.name.clone());
                    let Some(method_def_id) = self.member_def_ids.get(&method_key).copied() else {
                        continue;
                    };
                    let method_attrs = self.attribute_table.get_attrs(method_def_id);
                    let matched = method_attrs
                        .iter()
                        .any(|a| attr_candidates.iter().any(|c| c == &a.name));
                    if !matched {
                        continue;
                    }
                    // 构造 ClassExpression Value 并调用 D10.6 解释器。
                    // Value 含类名 + 全部方法签名 + 类级 attribute 列表，
                    // 供 ctor body 中 `if (expression is ClassExpression classDef)
                    // foreach (var m in classDef.Methods) { ... }` 解释执行。
                    let class_expr_value =
                        self.build_class_expression_value_for(class_name, class_def);
                    let (regs, interp_errs) = self.invoke_ctor_interpreter(
                        body,
                        expr_param_name,
                        slots,
                        class_expr_value,
                    );
                    for err in interp_errs {
                        // RFC 032 D-2: UserThrown 错误透传 Arc 侧派生类定义的
                        // 错误码与消息——保留 `error[code]: message` 格式一致性，
                        // 错误码语义归 std/QIF/ 派生类，typeck 零领域知识。
                        match &err {
                            CtorInterpError::UserThrown { code, message, .. } => {
                                errors.push(TypeError::Generic(format!(
                                    "error[{}]: {}",
                                    code, message
                                )));
                            }
                            _ => {
                                errors.push(TypeError::Macro {
                                    code: "arc-macro-031",
                                    message: format!(
                                        "D10.6 构造函数体解释器在 feature `{}` (被赋能类 `{}`) \
                                         上报错: {:?}",
                                        feature_name, class_name, err
                                    ),
                                });
                            }
                        }
                    }
                    expanded_regs.extend(regs);
                }
            }
        }

        // 若没有任何被赋能类，保留原 template registrations（不丢失 Pass 2 成果）
        if expanded_regs.is_empty() {
            return (template_regs.to_vec(), errors);
        }
        (expanded_regs, errors)
    }

    /// RFC 012 D-1a: 从 `ClassDef` 构造 `Value::ClassExpression`。
    ///
    /// 遍历 `class_def.methods` 提取方法签名（名、参数、返回类型、attribute 列表），
    /// 构造 `Vec<MethodExpressionValue>`，连同类名与类级 attribute 列表构造
    /// `Value::ClassExpression(ClassExpressionValue)`。
    ///
    /// 此 Value 作为 D10.6 解释器的初始环境绑定（`expression` 形参 → ClassExpression），
    /// 供派生类构造函数体中 `if (expression is ClassExpression classDef)` 模式匹配使用。
    /// 内部字段对齐 RFC 022 §2.2.9 `ClassExpression` Arc 类设计——
    /// `ClassName`/`Methods`/`Attributes` 三字段。
    ///
    /// 简化：`MethodExpressionValue.parameters` 元组 `(name, type_name)` 中 type_name
    /// 仅取 `Type::Named` 末段（如 `int`/`string`），其他变体（`Ref`/`Func`/`Array`）
    /// 标记为 `"unknown"`。完整类型序列化在 D10.6 解释器暂不需要。
    ///
    /// RFC 012 M2 D-3: `attributes` 字段从 `Vec<String>` 升级为
    /// `Vec<AttributeDataValue>`——每个 attribute 携带名 + 位置参数列表
    /// （仅 `AttributeArg::String`/`Int`/`Bool` 字面量；命名参数与 `Type`/
    /// `MemberPath`/`Lambda` 变体跳过，M2 范围仅需参数数量校验）。
    fn build_class_expression_value_for(
        &self,
        class_name: &Ident,
        class_def: &ast::ClassDef,
    ) -> Value {
        let methods: Vec<MethodExpressionValue> = class_def
            .methods
            .iter()
            .map(|m| {
                let sig = &m.node.sig;
                let parameters: Vec<(String, String)> = sig
                    .params
                    .iter()
                    .map(|p| {
                        let ty_name = type_name_of(&p.ty.node)
                            .map(|n| n.as_str().to_string())
                            .unwrap_or_else(|| "unknown".to_string());
                        (p.name.as_str().to_string(), ty_name)
                    })
                    .collect();
                let return_type = sig
                    .ret
                    .as_ref()
                    .and_then(|r| type_name_of(&r.node).map(|n| n.as_str().to_string()))
                    .unwrap_or_else(|| "void".to_string());
                let attributes: Vec<AttributeDataValue> = sig
                    .attributes
                    .iter()
                    .map(build_attribute_data_value)
                    .collect();
                MethodExpressionValue {
                    name: sig.name.as_str().to_string(),
                    parameters,
                    return_type,
                    attributes,
                }
            })
            .collect();

        let class_attributes: Vec<AttributeDataValue> = class_def
            .attributes
            .iter()
            .map(build_attribute_data_value)
            .collect();

        Value::ClassExpression(ClassExpressionValue {
            class_name: class_name.as_str().to_string(),
            methods,
            attributes: class_attributes,
        })
    }

    /// RFC 012 D-1a: 调用 D10.6 构造函数体解释器。
    ///
    /// 构造初始环境，绑定 `expression_param_name` → `class_expr_value`，
    /// 然后构造 `CtorInterpreter` 并调用 `interpret(body, &env)` 返回
    /// `(Vec<MacroRegistration>, Vec<CtorInterpError>)`。
    ///
    /// `body` 是 feature 派生类的构造函数体 AST（由 `find_feature_ctor_body` 查找）。
    /// `expression_param_name` 是构造函数的 Expression 形参名（如 `expression`）。
    /// `slots` 是关联容器的 public 方法槽位列表（识别 `this.<slot>(lambda)` 合法性）。
    /// `class_expr_value` 是被赋能类的 ClassExpression Value（含方法列表）。
    ///
    /// 返回的 `MacroRegistration` 列表中的 `expression_locals` 为空——D10.6 解释器
    /// 识别的注册调用 `this.Build(lambda)` 中的 lambda 不依赖外部 Expression 绑定
    /// （lambda 体仅访问 `classDef.ClassName`/`method.Name` 等已通过环境绑定到
    /// 解释器内部的 Value）。M4-4 受限求值器后续会重新求值这些 lambda 体。
    fn invoke_ctor_interpreter(
        &self,
        body: &Block,
        expression_param_name: &Ident,
        slots: &[MacroSlot],
        class_expr_value: Value,
    ) -> (Vec<MacroRegistration>, Vec<CtorInterpError>) {
        let mut env = Environment::new();
        env.bind(expression_param_name.as_str(), class_expr_value);
        let interp = CtorInterpreter::new(slots);
        interp.interpret(body, &env)
    }

    /// RFC 009 M4-3: 在 feature 派生类的构造函数体中扫描
    /// `this.<slot_method>(<lambda>)` 调用，返回识别到的注册列表。
    ///
    /// 扫描规则（RFC 009 D9.3）：
    /// - 调用 receiver 必须是 `this`（`Expr::Ident("this")`）
    /// - 方法名必须在 `slot_names` 中（关联容器 T 的 public 方法集）
    /// - 参数列表长度恰好为 1，且唯一参数是 `Expr::Lambda(LambdaExpr)`
    ///
    /// 不符合以上模式的调用按普通方法调用处理，不识别为宏展开注册。
    /// 同一构造函数中可包含多个注册（如先 `this.Register(...)` 再 `this.Test(...)`）。
    fn collect_feature_registrations(
        &self,
        feature_name: &Ident,
        slot_names: &HashSet<Ident>,
    ) -> Vec<MacroRegistration> {
        let Some(body) = self.find_feature_ctor_body(feature_name) else {
            return Vec::new();
        };
        let mut regs = Vec::new();
        self.walk_block_for_registrations(body, slot_names, &mut regs);
        regs
    }

    /// 在 `class_defs` 中查找 feature 派生类的构造函数体（原始 AST Block）。
    ///
    /// 直接访问原始 ClassDef 而非 `typed_fns`——派生宏特性类的构造函数中
    /// `this.<slot>(Func<string>)` 调用是宏注册语义，typeck 的方法查找会
    /// 因派生类自身未定义 `<slot>` 方法而失败并提前 return Err，导致
    /// `typed_fns` 中没有对应条目。直接读 ClassDef.constructors[i].body
    /// 保证即便 typeck 失败也能访问构造函数体。
    ///
    /// 返回首个 ctor 的 body（多 ctor 场景在 M4-3 测试用例覆盖）。
    fn find_feature_ctor_body(&self, class_name: &Ident) -> Option<&Block> {
        let class_def = self.class_defs.get(class_name)?;
        class_def.constructors.first().map(|c| &c.node.body)
    }

    /// 递归遍历 Block 中的语句，识别匹配模式的注册调用。
    ///
    /// 不递归进入 `while` / `for` / `try` / `using` 等控制流结构内部——
    /// RFC 009 D9 期望注册调用位于构造函数顶层表达式语句中，
    /// 控制流内部的 `this.<slot>(lambda)` 视为普通方法调用。
    fn walk_block_for_registrations(
        &self,
        block: &Block,
        slot_names: &HashSet<Ident>,
        out: &mut Vec<MacroRegistration>,
    ) {
        for stmt in &block.stmts {
            match &stmt.node {
                Stmt::Expr(expr) => {
                    self.walk_expr_for_registrations(expr, slot_names, out);
                }
                Stmt::Return(Some(expr)) => {
                    self.walk_expr_for_registrations(expr, slot_names, out);
                }
                Stmt::Let {
                    init: Some(init), ..
                } => {
                    self.walk_expr_for_registrations(init, slot_names, out);
                }
                Stmt::Assign { value, .. } => {
                    self.walk_expr_for_registrations(value, slot_names, out);
                }
                _ => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.walk_expr_for_registrations(tail, slot_names, out);
        }
    }

    fn walk_expr_for_registrations(
        &self,
        expr: &Spanned<Expr>,
        slot_names: &HashSet<Ident>,
        out: &mut Vec<MacroRegistration>,
    ) {
        let Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } = &expr.node
        else {
            return;
        };
        // RFC 009 M4-3: receiver 必须是 `this`（`Expr::This`，由 parser 对
        // `this` 关键字专门发射；非 `Expr::Ident("this")`）。
        let is_this_receiver = matches!(receiver.node, Expr::This);
        if !is_this_receiver {
            return;
        }
        if !slot_names.contains(method) {
            return;
        }
        if args.len() != 1 {
            return;
        }
        let Expr::Lambda(lambda) = &args[0].node else {
            return;
        };
        out.push(MacroRegistration {
            slot_name: method.clone(),
            expansion: lambda.clone(),
            span: expr.span,
            // RFC 009 M4-7: Pass 2 生成的基础注册无 Expression 绑定；
            // Pass 3 会扫描被赋能类生成带 expression_locals 的副本。
            expression_locals: Vec::new(),
        });
    }

    /// RFC 009 M5-2: 查询类是否标有 `[SourceGenerator]` 或
    /// `[SourceGeneratorAttribute]`。
    ///
    /// C# 规范允许 Attribute 后缀省略：`[SourceGenerator]` 与
    /// `[SourceGeneratorAttribute]` 等价。属性表存储时按 parser 传入的 name
    /// 保存，故此处两种形式都查。
    fn has_source_generator_attr(&self, class_name: &Ident) -> bool {
        let Some(def_id) = self.class_def_ids.get(class_name).copied() else {
            return false;
        };
        let attrs = self.attribute_table.get_attrs(def_id);
        attrs.iter().any(|a| {
            let n = a.name.as_str();
            n == "SourceGenerator" || n == "SourceGeneratorAttribute"
        })
    }

    /// RFC 012 M5-2: 提取 Source Generator 类的 `Generate` 方法体。
    ///
    /// 在 `class_defs` 中查找名为 `Generate` 的方法，返回其方法体 AST
    /// 与方法声明 span。方法签名应匹配 `Generate(GeneratorContext) ->
    /// List<string>`（签名校验在 Pass 2 typeck 中完成，此处仅按方法名
    /// 提取——M5-2 不做严格签名校验，留待后续完善）。
    ///
    /// RFC 012 M5-2b: 同时提取 Generate 方法的首个形参名（如有），供
    /// Pass 3 `expand_source_generators` 把构造好的 `GeneratorContext`
    /// 值注入求值器 locals——使方法体内 `context.Attributes` 等访问能
    /// 解析到真实 typeck 产物。
    ///
    /// 返回 `(None, None, dummy_span)` 当类未定义 `Generate` 方法——此时
    /// `SourceGenerator.generate_method_body` 为 None，Pass 3 跳过
    /// 该生成器并报 arc-macro-020 错误。
    fn collect_generate_method(&self, class_name: &Ident) -> (Option<Block>, Option<Ident>, Span) {
        let dummy = Span {
            file_id: 0,
            start: 0,
            end: 0,
        };
        let Some(class_def) = self.class_defs.get(class_name) else {
            return (None, None, dummy);
        };
        for method in &class_def.methods {
            if method.node.sig.name.as_str() == "Generate" {
                let span = method.span;
                let body = method.node.body.clone();
                // M5-2b: 提取首个形参名（Generate(GeneratorContext context) 中的 context）
                let context_param_name = method.node.sig.params.first().map(|p| p.name.clone());
                return (body, context_param_name, span);
            }
        }
        (None, None, dummy)
    }

    /// 收集宏容器类的所有 public 方法作为展开槽位。
    ///
    /// 规则：
    /// - 只收 public 方法（visibility == Public）
    /// - 收实例方法与静态方法
    /// - 不收构造函数与属性访问器（`get_*` / `set_*`）
    /// - 同名重载每个签名都收为独立槽位
    fn collect_slots(&self, nom: &crate::oop_types::NominalType) -> Vec<MacroSlot> {
        let mut slots = Vec::new();
        for (method_name, overloads) in &nom.methods {
            // 跳过属性访问器（get_XXX / set_XXX）
            if method_name.as_str().starts_with("get_") || method_name.as_str().starts_with("set_")
            {
                continue;
            }
            for sig in overloads {
                if sig.vis != Visibility::Public {
                    continue;
                }
                slots.push(MacroSlot {
                    method_name: method_name.clone(),
                    param_types: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    return_type: sig.ret.clone(),
                    modifier: sig.modifier,
                    is_async: sig.is_async,
                });
            }
        }
        slots
    }

    /// 在类的 `base_types` 中查找 `GenerateToAttribute<T>` 形式，
    /// 返回泛型实参 T 的类名。
    ///
    /// 匹配规则：
    /// - base_type 形如 `Type::Named { path: ["GenerateToAttribute"], generics: [T] }`
    /// - T 必须是 `Type::Named`（简单类型引用），取其 path 末段作为容器类名
    /// - 也接受 `[GenerateToAttribute]` 形式的长前缀路径（如 `Arc.GenerateToAttribute<T>`）
    fn find_generate_to_base(&self, nom: &crate::oop_types::NominalType) -> Option<Ident> {
        for base_ty in &nom.base_types {
            if let Type::Named { path, generics } = base_ty {
                let base_name = path.last()?;
                if base_name.as_str() != "GenerateToAttribute" {
                    continue;
                }
                // 必须为单泛型实参：GenerateToAttribute<T>
                if generics.len() != 1 {
                    continue;
                }
                // T 必须是简单类型引用（Type::Named，无嵌套泛型）
                if let Type::Named {
                    path: t_path,
                    generics: t_generics,
                } = &generics[0].node
                {
                    if !t_generics.is_empty() {
                        continue;
                    }
                    return t_path.last().cloned();
                }
            }
        }
        None
    }

    /// RFC 012 M4-8 D12.4: 循环依赖检测。
    ///
    /// 禁止宏容器类与自身派生的宏特性形成循环编译依赖。两类循环：
    ///
    /// 1. **直接自引用**：类 F 同时是宏容器（标 `[GenerateTo]`）和宏特性
    ///    （派生 `GenerateToAttribute<F>`）——F 把代码注入到自身。
    ///
    /// 2. **间接循环**：宏容器 C 被标注了 `[FeatureAttr]`，而
    ///    `FeatureAttr : GenerateToAttribute<C>`——C 被自身派生的宏特性标注，
    ///    导致「C 处理自身 → 代码注入 C → C 结构变化 → 再次处理」的循环。
    ///
    /// 检测时机：Pass 2 `collect_macros` 之后，在 `check_module` 末尾调用。
    /// 违规时报告 `arc-macro-010` 错误，错误加入 `self.errors`。
    pub fn check_cyclic_macro_dependencies(&mut self) {
        let cyclic_errors: Vec<TypeError> = self.collect_cyclic_macro_errors();
        for e in cyclic_errors {
            self.errors.push(e);
        }
    }

    /// 收集所有循环依赖错误（借用安全的分离方法）。
    fn collect_cyclic_macro_errors(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // 检查 1：直接自引用 —— F 同时是容器和特性，且 feature.container == F
        for (feature_name, feature) in &self.macro_catalog.features {
            if feature.container == *feature_name {
                errors.push(TypeError::Macro {
                    code: "arc-macro-010",
                    message: format!(
                        "cyclic compile-to dependency — \
                         macro container `{}` derives from `GenerateToAttribute<{}>` (self-reference)",
                        feature_name, feature.container
                    ),
                });
            }
        }

        // 检查 2：间接循环 —— 容器 C 被标注了指向自身的特性
        for container_name in self.macro_catalog.containers.keys() {
            let Some(class_def) = self.class_defs.get(container_name) else {
                continue;
            };
            for attr in &class_def.attributes {
                let Some(attr_name) = attr.path.last() else {
                    continue;
                };
                // C# 规范允许省略 Attribute 后缀：[Foo] 和 [FooAttribute] 等价
                let candidate_names = if attr_name.as_str().ends_with("Attribute") {
                    vec![attr_name.clone()]
                } else {
                    vec![
                        attr_name.clone(),
                        Ident::from(format!("{}Attribute", attr_name)),
                    ]
                };
                for candidate in &candidate_names {
                    if let Some(feature) = self.macro_catalog.features.get(candidate) {
                        if feature.container == *container_name {
                            errors.push(TypeError::Macro {
                                code: "arc-macro-010",
                                message: format!(
                                    "cyclic compile-to dependency — \
                                     macro container `{}` is annotated with `[{}]` \
                                     whose feature targets `{}` (cyclic compile dependency)",
                                    container_name, attr_name, container_name
                                ),
                            });
                        }
                    }
                }
            }
        }

        errors
    }
}

/// RFC 012 M2 D-3: 从 `ast::Attribute` 构造 `AttributeDataValue`。
///
/// `path.last()` 作为 `name`；`args` 中**位置参数**按声明顺序装入：
/// - `AttributeArg::String(s)` → `Value::String(s)`
/// - `AttributeArg::Int(n)` → `Value::Int(n)`
/// - `AttributeArg::Bool(b)` → `Value::Bool(b)`
/// - 命名参数 / `Type` / `MemberPath` / `Lambda` 变体跳过（M2 范围仅需
///   参数数量校验，非字面量参数不支持）
///
/// path 为空时 `name` 为空串（理论不可达，parser 保证 path 非空）。
fn build_attribute_data_value(attr: &ast::Attribute) -> AttributeDataValue {
    let name = attr
        .path
        .last()
        .map(|n| n.as_str().to_string())
        .unwrap_or_default();
    let args: Vec<Value> = attr
        .args
        .iter()
        .filter_map(|arg| match arg {
            ast::AttributeArg::String(s) => Some(Value::String(s.clone())),
            ast::AttributeArg::Int(n) => Some(Value::Int(*n)),
            ast::AttributeArg::Bool(b) => Some(Value::Bool(*b)),
            // 命名参数 / Type / MemberPath / Lambda 不支持——M2 范围跳过
            _ => None,
        })
        .collect();
    AttributeDataValue { name, args }
}

/// RFC 009 M4-7: 提取 `Type` 节点的最末段类型名（如 `Expression<Func<int,bool>>`
/// → `Expression`；`Arc.Linq.Expression` → `Expression`）。
///
/// 仅处理 `Type::Named`；其他变体（`Ref` / `Func` / `Array` / `Nullable`）
/// 返回 `None`。空 path 返回 `None`（理论不可达，parser 保证 path 非空）。
fn type_name_of(ty: &ast::Type) -> Option<Ident> {
    match ty {
        Type::Named { path, .. } => path.last().cloned(),
        _ => None,
    }
}

/// RFC 009 M4-7: 判断 `Type` 节点是否为 `Expression` 或 `Expression<T>` 类型。
///
/// 用于 Pass 1 中识别 feature 构造函数的 Expression 形参。判定规则：
/// `Type::Named` 且 path 末段为 `"Expression"`（不区分泛型实参有无）。
fn is_expression_type(ty: &ast::Type) -> bool {
    matches!(ty, Type::Named { path, .. } if path
        .last()
        .map(|n| n.as_str() == "Expression")
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_catalog_default_is_empty() {
        let cat = MacroCatalog::new();
        assert!(cat.containers.is_empty());
        assert!(cat.features.is_empty());
        assert!(!cat.is_container(&Ident::from("X")));
        assert!(!cat.is_feature(&Ident::from("Y")));
        assert!(cat.slots_of(&Ident::from("X")).is_none());
        assert_eq!(cat.container_of(&Ident::from("Y")), None);
    }

    #[test]
    fn macro_catalog_lookup_helpers() {
        let mut cat = MacroCatalog::new();
        cat.containers.insert(
            Ident::from("Host"),
            MacroContainer {
                class_name: Ident::from("Host"),
                slots: vec![MacroSlot {
                    method_name: Ident::from("Register"),
                    param_types: vec![],
                    return_type: Ident::from("void"),
                    modifier: MethodModifier::None,
                    is_async: false,
                }],
            },
        );
        cat.features.insert(
            Ident::from("InjectAttribute"),
            MacroFeature {
                class_name: Ident::from("InjectAttribute"),
                container: Ident::from("Host"),
                constructors: vec![],
                registrations: vec![],
            },
        );

        assert!(cat.is_container(&Ident::from("Host")));
        assert!(cat.is_feature(&Ident::from("InjectAttribute")));
        assert_eq!(
            cat.container_of(&Ident::from("InjectAttribute")),
            Some(&Ident::from("Host"))
        );
        let slots = cat
            .slots_of(&Ident::from("Host"))
            .expect("Host must have slots");
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].method_name.as_str(), "Register");
    }
}
