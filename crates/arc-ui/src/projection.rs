//! RFC 016 §11.5 求值模型编译期侧（M-U2）：状态空间枚举 + 投影表 + 索引公式。
//!
//! §11.5 核心洞察：除容器尺寸外，快照所有坐标（idiom/tier/media 坐标/density）
//! 都是离散枚举 ⇒ 可编译期穷举。本模块：
//!
//! 1. **状态空间枚举**：收集全窗口用到的谓词 → 有效状态空间（只含实际用到的
//!    维度组合；未在任何谓词出现的维度被剔除 = 死组合剔除）。
//! 2. **每 Token 投影表**：静态坐标 C（idiom × tier × media-vector × density）
//!    编组为一个整数 idx → 值；判定树折叠为扁平数组。
//! 3. **索引公式**：`idx = Σ coord_i × stride_i`（整数运算），窗口级缓存，
//!    变化时一次重算；每 Token 一次内存读 `values[idx]`。
//!
//! **确定性规则（§11.5，口头可复述）**：维度独立求值 → 档位区间唯一 →
//! 特异性次序（属性多者胜；同数量按 Media > Density > Idiom > Tier > 容器断点）
//! → 兜底。同一快照必然同一布局。
//!
//! **范围边界（M-U2）**：本模块产出投影表（含非 `<Double>` 类型 Token 的
//! 规范化字面量表 `table_str`）；运行期求值器（`std/UI/Core/Adaptive/`，Arc 实现）
//! 消费 `<Double>` 的数值表。非数值类型的运行期解析属 M-U3（Token=资源字典
//! 容器）。容器断点（MinWidth/MaxWidth）作为每个 Token 独立的「区间维」并入
//! 投影表；`<Adaptive>` 子树谓词单独收集（运行期容器查询）。
//!
//! 单位（§11.1）：投影表存储**声明单位下的数值幅度** + 单位码
//! （0=vp 1=px 2=% 3=lpx）；运行期换算（`px = vp × density`、
//! `% = avail × pct / 100`、`lpx = 1 vp × clamp(W_vp/1280, 0.5, 2.0)`）。

use crate::adaptive_lit::{split_length, ValueType};
use crate::ast::*;
use crate::error::ArmlError;
use indexmap::IndexMap;
use smol_str::SmolStr;
use std::collections::BTreeMap;

/// 维度种类（§11.5 前四维）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DimKind {
    /// 设备形态（Desktop/Mobile/Tablet/TV/Watch）
    Idiom,
    /// 空间预算·窗口档位
    Tier,
    /// 环境偏好坐标（每个被引用的坐标 = 一个独立维度）
    Media,
    /// 密度档
    Density,
}

/// 单位码（§11.1，运行期换算用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCode {
    /// `vp`（默认）：`px = vp × density`
    Vp = 0,
    /// `px`：物理像素
    Px = 1,
    /// `%`：`v = avail × pct / 100`
    Percent = 2,
    /// `lpx`：`1 lpx = 1 vp × clamp(W_vp/1280, 0.5, 2.0)`
    Lpx = 3,
}

impl UnitCode {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// 单个静态维度规格。
#[derive(Debug, Clone)]
pub struct DimSpec {
    pub kind: DimKind,
    /// 被引用的值（首见序；如 Tier 名 / Idiom 名 / Density 名 / Media 字面量）。
    pub values: Vec<SmolStr>,
    /// 基数 = `values.len() + 1`（末位 = 「无引用值命中」槽，保证任何快照
    /// 坐标都映射到合法下标，见 §11.5 确定性）。
    pub card: usize,
    /// 索引公式步长：`∏_{j>i} card_j`。
    pub stride: usize,
    /// Media 维度的坐标名（如 `dark`/`font-scale`）；非 Media 维度为 `None`。
    pub coordinate: Option<SmolStr>,
}

impl DimSpec {
    pub fn value_index(&self, value: &str) -> Option<usize> {
        self.values.iter().position(|v| v == value)
    }
}

/// 单个 Token 的投影表。
#[derive(Debug, Clone)]
pub struct TokenProjection {
    pub name: SmolStr,
    pub value_type: ValueType,
    /// 断点区间数（`thresholds.len() + 1`；无断点 = 1）。
    pub intervals: usize,
    /// 升序断点阈值（每个 Token 独立）。
    pub thresholds: Vec<f64>,
    /// 数值表（`<Double>`）：长度 = `num_states × intervals`；声明单位下幅度。
    pub table: Vec<f64>,
    /// 单位码表：长度与 `table` 相同。
    pub units: Vec<u8>,
    /// 规范化字面量表（全类型）：长度与 `table` 相同。
    pub table_str: Vec<SmolStr>,
}

/// `<Adaptive>` 子树谓词（§11.4/§11.5：P(container) 是子树谓词）。
#[derive(Debug, Clone)]
pub struct AdaptiveProjection {
    pub id: usize,
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    /// 静态条件（维度索引, 维度值索引）——与 Token Match 同一套编码。
    pub conditions: Vec<(usize, usize)>,
}

/// 窗口级投影规格（§11.5 编译期产物）。
#[derive(Debug, Clone)]
pub struct ProjectionSpec {
    /// 静态维度（仅实际用到的；死组合已剔除）。
    pub dims: Vec<DimSpec>,
    /// 有效状态数 = `∏ card_i`。
    pub num_states: usize,
    /// 规范 Idiom 序（Desktop/Mobile/Tablet/TV/Watch）→ 维度引用索引（或 -1）。
    pub idiom_ref: Vec<i32>,
    /// 规范 Density 序（compact/comfortable/cozy）→ 维度引用索引（或 -1）。
    pub density_ref: Vec<i32>,
    /// 声明档位序（名, 阈值）升序。
    pub tiers: Vec<(SmolStr, f64)>,
    /// 档位位置 → 维度引用索引（或 -1）。
    pub tier_ref: Vec<i32>,
    /// Token 投影表（定义序）。
    pub tokens: Vec<TokenProjection>,
    /// `<Adaptive>` 子树谓词。
    pub adaptives: Vec<AdaptiveProjection>,
    /// 窗口是否含 lpx 单位值（运行期需按快照重算 lpx 系数）。
    pub has_lpx: bool,
    /// 该文档是否引用任何 Token（供 codegen 决定是否发射求值器）。
    pub uses_tokens: bool,
    /// 是否含任何 `<Adaptive>` 子树。
    pub has_adaptives: bool,
}

/// 特异性优先级（§11.3：Media > Density > Idiom > Tier > 容器断点）。
fn dim_priority(kind: DimKind) -> u8 {
    match kind {
        DimKind::Media => 4,
        DimKind::Density => 3,
        DimKind::Idiom => 2,
        DimKind::Tier => 1,
    }
}

const BREAKPOINT_PRIORITY: u8 = 0;

/// 一个 Match 的静态条件编码。
#[derive(Debug, Clone)]
struct MatchPred {
    /// 静态条件：(维度索引, 值索引)
    statics: Vec<(usize, usize)>,
    /// 静态条件优先级向量（降序）——特异性次序用
    priority: Vec<u8>,
    /// 断点区间 [min, max)（无断点 = None）
    bp: Option<(f64, f64)>,
    /// 值（字面量）
    value: SmolStr,
    /// 是否无条件（兜底候选）
    unconditional: bool,
}

/// 构建窗口级投影规格（§11.5 编译期侧）。
pub fn build_projection_spec(doc: &ArmlDocument) -> Result<ProjectionSpec, Vec<ArmlError>> {
    let mut ctx = BuildCtx::default();
    ctx.collect(&doc.root);
    ctx.finish()
}

#[derive(Default)]
struct BuildCtx {
    /// Token 名 → (值类型, base 值字面量, Match 元素)
    tokens: BTreeMap<String, (ValueType, Option<SmolStr>, Vec<Element>)>,
    /// Token 定义序
    token_order: Vec<String>,
    /// `<Adaptive>` 子树（原始条件）
    adaptives: Vec<AdaptiveRaw>,
    /// 维度值集（首见序）
    idiom_used: IndexMap<String, Span>,
    tier_used: IndexMap<String, Span>,
    density_used: IndexMap<String, Span>,
    /// Media 坐标名 → (引用字面量 → span)，均首见序
    media_used: IndexMap<String, IndexMap<String, Span>>,
    /// 档位声明（全局 `<Application.Tiers>` ∪ 窗口 `<*.Tiers>`；M-U1 已校验唯一性）
    tier_decls: BTreeMap<String, f64>,
    /// Token 名 → base 值（别名解析用；与 `tokens` 分开维护，遍历时不随 remove 消失）
    token_bases: BTreeMap<String, Option<SmolStr>>,
    has_lpx: bool,
    uses_tokens: bool,
}

/// `<Adaptive>` 原始条件。
struct AdaptiveRaw {
    min_width: Option<f64>,
    max_width: Option<f64>,
    conditions: Vec<(DimKind, SmolStr)>,
}

impl BuildCtx {
    fn collect(&mut self, el: &Element) {
        match el.name.as_str() {
            "Application" | "Window" | "UserControl" | "Page" => {
                // `<*.Tiers Default="name:value ...">` 档位声明（全局/局部覆盖）
                for child in el.child_elements() {
                    if child.name == "Tiers" {
                        self.collect_tiers_decl(child);
                    }
                }
                for child in el.child_elements() {
                    self.collect(child);
                }
                return;
            }
            "Tiers" | "Media" | "Match" => return,
            _ => {}
        }
        if let Some(vt) = ValueType::from_element_name(&el.name) {
            self.collect_token_definition(el, vt);
            return;
        }
        if el.name == "Adaptive" {
            self.collect_adaptive(el);
            for child in el.child_elements() {
                self.collect(child);
            }
            return;
        }
        // 常规元素：`{Token}` 引用标记（供 codegen 决定是否发射求值器）
        for attr in &el.attributes {
            if let Some(ext) = attr.value.as_markup() {
                if ext.kind == MarkupKind::Token {
                    self.uses_tokens = true;
                }
            }
        }
        // 内联响应式值（匿名 Token，如 `<Grid.Columns><TrackList>…`）
        if el.name == "TrackList" || el.name == "Double" {
            for m in el.child_elements().filter(|c| c.name == "Match") {
                self.collect_predicates(m);
            }
        }
        for child in el.child_elements() {
            self.collect(child);
        }
    }

    /// Token 定义收集：`<Double x:Key="...">` + `<Match>` 子元素。
    fn collect_token_definition(&mut self, el: &Element, vt: ValueType) {
        let Some(key) = key_of(el) else {
            return;
        };
        let key = key.to_string();
        let base = el.attr("Value").and_then(|a| match &a.value {
            AttributeValue::Literal(v) => Some(v.clone()),
            // `{Token X}` 别名：保留标记形态供兜底解析（编译期单层展开）
            AttributeValue::MarkupExtension(ext) if ext.kind == MarkupKind::Token => {
                self.uses_tokens = true;
                ext.args
                    .first()
                    .map(|arg| format!("{{Token {arg}}}").into())
            }
            _ => None,
        });
        let matches: Vec<Element> = el
            .child_elements()
            .filter(|c| c.name == "Match")
            .cloned()
            .collect();
        if !self.tokens.contains_key(&key) {
            self.token_order.push(key.clone());
        }
        self.token_bases.insert(key.clone(), base.clone());
        for m in &matches {
            self.collect_predicates(m);
        }
        self.tokens.insert(key, (vt, base, matches));
    }

    /// `<*.Tiers Default="name:value ...">` 档位声明收集。
    fn collect_tiers_decl(&mut self, el: &Element) {
        let Some(default) = el.attr("Default") else {
            return;
        };
        let Some(lit) = default.value.as_literal() else {
            return;
        };
        for pair in lit.split_whitespace() {
            if let Some((name, value)) = pair.split_once(':') {
                if let Ok(v) = value.parse::<f64>() {
                    self.tier_decls.insert(name.to_string(), v);
                }
            }
        }
    }

    fn collect_adaptive(&mut self, el: &Element) {
        let min = el
            .attr("MinWidth")
            .and_then(|a| a.value.as_literal())
            .and_then(|v| v.trim().parse::<f64>().ok());
        let max = el
            .attr("MaxWidth")
            .and_then(|a| a.value.as_literal())
            .and_then(|v| v.trim().parse::<f64>().ok());
        let mut conditions = Vec::new();
        for (attr, kind) in [
            (el.attr("Idiom"), DimKind::Idiom),
            (el.attr("Tier"), DimKind::Tier),
            (el.attr("Density"), DimKind::Density),
            (el.attr("Media"), DimKind::Media),
        ] {
            if let Some(a) = attr {
                if let Some(v) = a.value.as_literal() {
                    conditions.push((kind, SmolStr::from(v)));
                }
            }
        }
        self.adaptives.push(AdaptiveRaw {
            min_width: min,
            max_width: max,
            conditions,
        });
    }

    /// 收集一个 Match 的全部谓词 → 维度值集 + lpx 检测。
    fn collect_predicates(&mut self, m: &Element) {
        if let Some(a) = m.attr("Tier") {
            if let Some(v) = a.value.as_literal() {
                self.tier_used.entry(v.to_string()).or_insert(a.span);
            }
        }
        if let Some(a) = m.attr("Idiom") {
            if let Some(v) = a.value.as_literal() {
                self.idiom_used.entry(v.to_string()).or_insert(a.span);
            }
        }
        if let Some(a) = m.attr("Density") {
            if let Some(v) = a.value.as_literal() {
                self.density_used.entry(v.to_string()).or_insert(a.span);
            }
        }
        if let Some(a) = m.attr("Media") {
            if let Some(v) = a.value.as_literal() {
                let coord = v.to_string();
                let lit = m
                    .attr("MediaValue")
                    .and_then(|x| x.value.as_literal())
                    .map(|x| x.to_string())
                    .unwrap_or_else(|| "true".to_string());
                self.media_used
                    .entry(coord)
                    .or_default()
                    .entry(lit)
                    .or_insert(a.span);
            }
        }
        if let Some(a) = m.attr("Value") {
            if let Some(v) = a.value.as_literal() {
                if let Ok((_, u)) = split_length(v) {
                    if u == Some("lpx") {
                        self.has_lpx = true;
                    }
                }
            }
        }
    }

    fn finish(mut self) -> Result<ProjectionSpec, Vec<ArmlError>> {
        // ---- 维度构建（规范序：Idiom → Tier → Density → Media 坐标首见序）----
        let mut dims: Vec<DimSpec> = Vec::new();

        let idiom_values: Vec<SmolStr> =
            self.idiom_used.keys().cloned().map(SmolStr::from).collect();
        let idiom_ref = CANONICAL_IDIOMS
            .iter()
            .map(|c| {
                idiom_values
                    .iter()
                    .position(|v| v == c)
                    .map_or(-1, |i| i as i32)
            })
            .collect::<Vec<i32>>();
        if !idiom_values.is_empty() {
            dims.push(DimSpec {
                kind: DimKind::Idiom,
                card: idiom_values.len() + 1,
                values: idiom_values,
                stride: 0,
                coordinate: None,
            });
        }

        let tier_values: Vec<SmolStr> = self.tier_used.keys().cloned().map(SmolStr::from).collect();
        // 档位序（按声明阈值升序；未声明档位回退内置 sm/md/lg 阈值）
        let mut tiers: Vec<(SmolStr, f64)> = Vec::new();
        let mut tier_ref: Vec<i32> = Vec::new();
        if !tier_values.is_empty() {
            let mut with_t: Vec<(SmolStr, f64)> = tier_values
                .iter()
                .map(|n| {
                    let t = self
                        .tier_decls
                        .get(n.as_str())
                        .copied()
                        .or_else(|| {
                            BUILTIN_TIER_THRESHOLDS
                                .iter()
                                .find(|(bn, _)| *bn == n.as_str())
                                .map(|(_, bt)| *bt)
                        })
                        .unwrap_or(f64::MAX);
                    (n.clone(), t)
                })
                .collect();
            with_t.sort_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            tiers = with_t.clone();
            for (n, _) in &tiers {
                let idx = tier_values.iter().position(|x| x == n);
                tier_ref.push(match idx {
                    Some(i) => i as i32,
                    None => -1,
                });
            }
            dims.push(DimSpec {
                kind: DimKind::Tier,
                card: tier_values.len() + 1,
                values: tier_values,
                stride: 0,
                coordinate: None,
            });
        }

        let density_values: Vec<SmolStr> = self
            .density_used
            .keys()
            .cloned()
            .map(SmolStr::from)
            .collect();
        let density_ref = CANONICAL_DENSITIES
            .iter()
            .map(|c| {
                density_values
                    .iter()
                    .position(|v| v == c)
                    .map_or(-1, |i| i as i32)
            })
            .collect::<Vec<i32>>();
        if !density_values.is_empty() {
            dims.push(DimSpec {
                kind: DimKind::Density,
                card: density_values.len() + 1,
                values: density_values,
                stride: 0,
                coordinate: None,
            });
        }

        // Media 维度（坐标首见序）
        for coord in self.media_used.keys() {
            let vals: IndexMap<String, Span> = self.media_used[coord].clone();
            let vs: Vec<SmolStr> = vals.keys().cloned().map(SmolStr::from).collect();
            dims.push(DimSpec {
                kind: DimKind::Media,
                card: vs.len() + 1,
                values: vs,
                stride: 0,
                coordinate: Some(SmolStr::from(coord)),
            });
        }

        // 步长 + 状态数（末位 = no-match 槽）
        let mut stride_acc = 1usize;
        for d in dims.iter_mut().rev() {
            d.stride = stride_acc;
            stride_acc *= d.card;
        }
        let num_states = stride_acc;

        // ---- 每 Token 投影表 ----
        let mut token_tables: Vec<TokenProjection> = Vec::new();
        for key in self.token_order.clone() {
            let Some((vt, base, matches)) = self.tokens.remove(&key) else {
                continue;
            };
            let tname = key.clone();

            // 断点阈值（升序去重）
            let mut thresholds: Vec<f64> = Vec::new();
            for m in &matches {
                for bp in ["MinWidth", "MaxWidth"] {
                    if let Some(a) = m.attr(bp) {
                        if let Some(v) = a.value.as_literal() {
                            if let Ok(n) = v.trim().parse::<f64>() {
                                thresholds.push(n);
                            }
                        }
                    }
                }
            }
            thresholds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            thresholds.dedup();
            let intervals = thresholds.len() + 1;
            let interval_lo = |k: usize| -> f64 {
                if k == 0 {
                    f64::NEG_INFINITY
                } else {
                    thresholds[k - 1]
                }
            };
            let interval_hi = |k: usize| -> f64 {
                if k == intervals - 1 {
                    f64::INFINITY
                } else {
                    thresholds[k]
                }
            };

            // 构建 Match 谓词（维度编码）
            let mut preds: Vec<MatchPred> = Vec::new();
            for m in &matches {
                let mut statics = Vec::new();
                let mut priority = Vec::new();
                let mut push_static = |kind: DimKind, value: &str, dims: &[DimSpec]| {
                    if let Some(di) = dims.iter().position(|d| d.kind == kind) {
                        if let Some(vi) = dims[di].value_index(value) {
                            statics.push((di, vi));
                            priority.push(dim_priority(kind));
                        }
                    }
                };
                if let Some(a) = m.attr("Tier") {
                    if let Some(v) = a.value.as_literal() {
                        push_static(DimKind::Tier, v, &dims);
                    }
                }
                if let Some(a) = m.attr("Idiom") {
                    if let Some(v) = a.value.as_literal() {
                        push_static(DimKind::Idiom, v, &dims);
                    }
                }
                if let Some(a) = m.attr("Density") {
                    if let Some(v) = a.value.as_literal() {
                        push_static(DimKind::Density, v, &dims);
                    }
                }
                if let Some(a) = m.attr("Media") {
                    if let Some(coord) = a.value.as_literal() {
                        // 该坐标的引用字面量（MediaValue 或 "true"）
                        let lit = m
                            .attr("MediaValue")
                            .and_then(|x| x.value.as_literal())
                            .unwrap_or("true");
                        if let Some(di) = dims.iter().position(|d| {
                            d.kind == DimKind::Media && d.coordinate.as_deref() == Some(coord)
                        }) {
                            if let Some(vi) = dims[di].value_index(lit) {
                                statics.push((di, vi));
                                priority.push(dim_priority(DimKind::Media));
                            }
                        }
                    }
                }
                priority.sort_by(|a, b| b.cmp(a));
                let bp = {
                    let min = m
                        .attr("MinWidth")
                        .and_then(|a| a.value.as_literal())
                        .and_then(|v| v.trim().parse::<f64>().ok());
                    let max = m
                        .attr("MaxWidth")
                        .and_then(|a| a.value.as_literal())
                        .and_then(|v| v.trim().parse::<f64>().ok());
                    match (min, max) {
                        (Some(mn), Some(mx)) => Some((mn, mx)),
                        (Some(mn), None) => Some((mn, f64::INFINITY)),
                        (None, Some(mx)) => Some((f64::NEG_INFINITY, mx)),
                        (None, None) => None,
                    }
                };
                if bp.is_some() {
                    priority.push(BREAKPOINT_PRIORITY);
                    priority.sort_by(|a, b| b.cmp(a));
                }
                let value = m
                    .attr("Value")
                    .and_then(|a| a.value.as_literal())
                    .unwrap_or_default()
                    .to_string();
                let unconditional = statics.is_empty() && bp.is_none();
                preds.push(MatchPred {
                    statics,
                    priority,
                    bp,
                    value: SmolStr::from(value),
                    unconditional,
                });
            }

            // 兜底值：base Value（别名解析）> 无条件 Match > 首个值
            let fallback = resolve_fallback(base.as_ref(), &self, &preds);

            // 展开表
            let mut table = Vec::with_capacity(num_states * intervals);
            let mut units = Vec::with_capacity(num_states * intervals);
            let mut table_str = Vec::with_capacity(num_states * intervals);
            for s in 0..num_states {
                let coords = decode_state(&dims, s);
                for k in 0..intervals {
                    let (lo, hi) = (interval_lo(k), interval_hi(k));
                    let mut satisfied: Vec<&MatchPred> = Vec::new();
                    for p in &preds {
                        let mut ok = true;
                        for (di, vi) in &p.statics {
                            if coords[*di] != *vi {
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            if let Some((mn, mx)) = p.bp {
                                if lo < mn || hi > mx {
                                    ok = false;
                                }
                            }
                        }
                        if ok {
                            satisfied.push(p);
                        }
                    }
                    let value = match satisfied.iter().max_by(|a, b| compare_specificity(a, b)) {
                        Some(p) => p.value.clone(),
                        None => fallback.clone(),
                    };
                    let (mag, unit) = resolve_value(vt, &value);
                    table.push(mag);
                    units.push(unit.as_u8());
                    table_str.push(value);
                }
            }

            token_tables.push(TokenProjection {
                name: SmolStr::from(&tname),
                value_type: vt,
                intervals,
                thresholds,
                table,
                units,
                table_str,
            });
        }

        // ---- `<Adaptive>` 谓词（编码到维度索引）----
        let mut adaptives = Vec::new();
        for (id, raw) in self.adaptives.iter().enumerate() {
            let mut conditions = Vec::new();
            for (kind, value) in &raw.conditions {
                if let Some(di) = dims.iter().position(|d| d.kind == *kind) {
                    if let Some(vi) = dims[di].value_index(value.as_str()) {
                        conditions.push((di, vi));
                    }
                }
            }
            adaptives.push(AdaptiveProjection {
                id,
                min_width: raw.min_width,
                max_width: raw.max_width,
                conditions,
            });
        }

        let has_lpx = self.has_lpx;
        let uses_tokens = self.uses_tokens || !self.token_order.is_empty();
        let has_adaptives = !adaptives.is_empty();

        Ok(ProjectionSpec {
            dims,
            num_states,
            idiom_ref,
            density_ref,
            tiers,
            tier_ref,
            tokens: token_tables,
            adaptives,
            has_lpx,
            uses_tokens,
            has_adaptives,
        })
    }
}

/// 兜底值解析：base `Value`（含 `{Token}` 别名，单层 + 循环防护）> 无条件 Match > 首个值。
fn resolve_fallback(base: Option<&SmolStr>, ctx: &BuildCtx, preds: &[MatchPred]) -> SmolStr {
    if let Some(b) = base {
        let lit = b.as_str();
        // `{Token X}` 别名：解析被引用 Token 的 base 常量（递归，循环防护）
        if let Some(target) = lit
            .strip_prefix("{Token ")
            .and_then(|x| x.strip_suffix('}'))
            .map(|x| x.to_string())
        {
            let mut guard = 0usize;
            let mut cur = target;
            while guard < 8 {
                guard += 1;
                let Some(b2) = ctx.token_bases.get(cur.as_str()).and_then(|x| x.clone()) else {
                    break;
                };
                if let Some(t2) = b2
                    .as_str()
                    .strip_prefix("{Token ")
                    .and_then(|x| x.strip_suffix('}'))
                    .map(|x| x.to_string())
                {
                    cur = t2;
                    continue;
                }
                return b2;
            }
            return SmolStr::new("");
        }
        return SmolStr::from(lit);
    }
    preds
        .iter()
        .find(|p| p.unconditional)
        .map(|p| p.value.clone())
        .unwrap_or_else(|| {
            preds
                .first()
                .map(|p| p.value.clone())
                .unwrap_or_else(|| SmolStr::new(""))
        })
}

/// 特异性比较：属性多者胜；同数量按优先级向量（降序）字典序。
fn compare_specificity(a: &MatchPred, b: &MatchPred) -> std::cmp::Ordering {
    let ac = a.statics.len() + usize::from(a.bp.is_some());
    let bc = b.statics.len() + usize::from(b.bp.is_some());
    if ac != bc {
        return ac.cmp(&bc);
    }
    a.priority.cmp(&b.priority)
}

/// 解码状态索引 → 每维坐标（§11.5 索引公式逆映射）。
fn decode_state(dims: &[DimSpec], state: usize) -> Vec<usize> {
    let mut coords = vec![0usize; dims.len()];
    let mut rem = state;
    for (i, d) in dims.iter().enumerate() {
        coords[i] = (rem / d.stride) % d.card;
        rem %= d.stride;
    }
    coords
}

/// 编码静态坐标 → 状态索引（§11.5 索引公式）。
pub fn encode_state(dims: &[DimSpec], coords: &[usize]) -> usize {
    let mut idx = 0usize;
    for (i, d) in dims.iter().enumerate() {
        let c = coords[i].min(d.card - 1);
        idx += c * d.stride;
    }
    idx
}

/// 解析 `<Double>` 值 → (幅度, 单位)；非数值类型幅度为 0、单位 vp。
fn resolve_value(vt: ValueType, value: &SmolStr) -> (f64, UnitCode) {
    if vt != ValueType::Double {
        return (0.0, UnitCode::Vp);
    }
    match split_length(value.as_str()) {
        Ok((num, unit)) => {
            let mag = num.trim().parse::<f64>().unwrap_or(0.0);
            let code = match unit {
                None => UnitCode::Vp,
                Some("px") => UnitCode::Px,
                Some("%") => UnitCode::Percent,
                Some("lpx") => UnitCode::Lpx,
                _ => UnitCode::Vp,
            };
            (mag, code)
        }
        Err(_) => (0.0, UnitCode::Vp),
    }
}

const CANONICAL_IDIOMS: &[&str] = &["Desktop", "Mobile", "Tablet", "TV", "Watch"];
const CANONICAL_DENSITIES: &[&str] = &["compact", "comfortable", "cozy"];
const BUILTIN_TIER_THRESHOLDS: &[(&str, f64)] = &[("sm", 600.0), ("md", 960.0), ("lg", 1280.0)];

fn key_of(el: &Element) -> Option<&str> {
    el.attr_with_prefix("x", "Key")
        .or_else(|| el.attr("Key"))
        .and_then(|a| a.value.as_literal())
}
