//! RFC 016 §11.5 投影规格 → Arc 字面量渲染（M-U2）。
//!
//! 将 [`ProjectionSpec`] 渲染为 `AdaptiveSpec`（`std/UI/Core/Adaptive/AdaptiveSpec.as`）
//! 的构造代码。供两处消费：
//! - `arc ui codegen`（窗口 partial class 内的规格数据 + 求值器/宿主接线）；
//! - e2e 测试（把同一份规格嵌入 Arc 程序，验证运行期求值 == 编译期表格）。
//!
//! 渲染产物为确定性文本（§11.5：投影是纯函数；同一规格必然同一布局）。
//! 数据以 `List<int>/List<double>` 承载（Arc 集合字面量经局部变量赋字段）。

use crate::projection::{DimKind, ProjectionSpec};

/// 渲染规格为 Arc 构造代码块（`AdaptiveSpec s = ...;` 变量名固定 `_adaptiveSpec`）。
///
/// 返回的文本可整体嵌入窗口 partial class 的 `InitializeComponent`（codegen）
/// 或 e2e 测试方法体。渲染含 `List<AdaptiveToken>` 需要的 `using Arc.Collections;`。
pub fn render_spec_arc(spec: &ProjectionSpec) -> String {
    let mut out = String::new();
    out.push_str("AdaptiveSpec _adaptiveSpec = new AdaptiveSpec();\n");
    out.push_str(&format!("_adaptiveSpec.NumStates = {};\n", spec.num_states));
    out.push_str(&format!("_adaptiveSpec.DimCount = {};\n", spec.dims.len()));
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.DimCards",
        "_cards",
        &spec.dims.iter().map(|d| d.card as i32).collect::<Vec<_>>(),
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.DimStrides",
        "_strides",
        &spec
            .dims
            .iter()
            .map(|d| d.stride as i32)
            .collect::<Vec<_>>(),
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.DimKinds",
        "_kinds",
        &spec
            .dims
            .iter()
            .map(|d| dim_kind_code(d.kind))
            .collect::<Vec<_>>(),
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.IdiomRef",
        "_idiomRef",
        &spec.idiom_ref,
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.DensityRef",
        "_densityRef",
        &spec.density_ref,
    );
    out.push_str(&format!(
        "_adaptiveSpec.TierCount = {};\n",
        spec.tiers.len()
    ));
    emit_list_f64(
        &mut out,
        "_adaptiveSpec.TierThresholds",
        "_tiers",
        &spec.tiers.iter().map(|(_, t)| *t).collect::<Vec<_>>(),
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.TierRef",
        "_tierRef",
        &spec.tier_ref,
    );

    // Media 维度编码（每维引用值字面量 → double；命名坐标 "true"/"false" → 1.0/0.0）
    let mut media_counts = Vec::with_capacity(spec.dims.len());
    let mut media_offsets = Vec::with_capacity(spec.dims.len());
    let mut media_values: Vec<f64> = Vec::new();
    for d in &spec.dims {
        if d.kind == DimKind::Media {
            media_offsets.push(media_values.len() as i32);
            let mut vals: Vec<f64> = Vec::new();
            for v in &d.values {
                vals.push(media_literal_to_f64(v));
            }
            media_counts.push(vals.len() as i32);
            media_values.extend(vals);
        } else {
            media_counts.push(0);
            media_offsets.push(media_values.len() as i32);
        }
    }
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.MediaValueCount",
        "_mediaCounts",
        &media_counts,
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.MediaRefOffset",
        "_mediaOffsets",
        &media_offsets,
    );
    emit_list_f64(
        &mut out,
        "_adaptiveSpec.MediaRefValues",
        "_mediaValues",
        &media_values,
    );

    // Token 投影表
    out.push_str("_adaptiveSpec.Tokens = new List<AdaptiveToken>();\n");
    for (i, t) in spec.tokens.iter().enumerate() {
        let var = format!("_tok{i}");
        out.push_str(&format!("AdaptiveToken {var} = new AdaptiveToken();\n"));
        out.push_str(&format!("{var}.IntervalCount = {};\n", t.intervals));
        emit_list_f64(
            &mut out,
            &format!("{var}.Thresholds"),
            &format!("{var}Th"),
            &t.thresholds,
        );
        emit_list_f64(
            &mut out,
            &format!("{var}.Table"),
            &format!("{var}Table"),
            &t.table,
        );
        emit_list_i32(
            &mut out,
            &format!("{var}.Units"),
            &format!("{var}Units"),
            &t.units.iter().map(|u| *u as i32).collect::<Vec<_>>(),
        );
        out.push_str(&format!("_adaptiveSpec.Tokens.Add({var});\n"));
    }

    // `<Adaptive>` 子树谓词
    out.push_str(&format!(
        "_adaptiveSpec.AdaptiveCount = {};\n",
        spec.adaptives.len()
    ));
    let mins: Vec<f64> = spec
        .adaptives
        .iter()
        .map(|a| a.min_width.unwrap_or(0.0))
        .collect();
    let maxs: Vec<f64> = spec
        .adaptives
        .iter()
        .map(|a| a.max_width.unwrap_or(f64::INFINITY))
        .collect();
    emit_list_f64(&mut out, "_adaptiveSpec.AdaptiveMin", "_amin", &mins);
    emit_list_f64(&mut out, "_adaptiveSpec.AdaptiveMax", "_amax", &maxs);
    let mut cond_offset = Vec::with_capacity(spec.adaptives.len());
    let mut cond_count = Vec::with_capacity(spec.adaptives.len());
    let mut cond_dim: Vec<i32> = Vec::new();
    let mut cond_value: Vec<i32> = Vec::new();
    for a in &spec.adaptives {
        cond_offset.push(cond_dim.len() as i32);
        cond_count.push(a.conditions.len() as i32);
        for (di, vi) in &a.conditions {
            cond_dim.push(*di as i32);
            cond_value.push(*vi as i32);
        }
    }
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.AdaptiveCondOffset",
        "_condOff",
        &cond_offset,
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.AdaptiveCondCount",
        "_condCnt",
        &cond_count,
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.AdaptiveCondDim",
        "_condDim",
        &cond_dim,
    );
    emit_list_i32(
        &mut out,
        "_adaptiveSpec.AdaptiveCondValue",
        "_condVal",
        &cond_value,
    );

    out
}

/// 维度种类 → 运行期编码（0=Tier 1=Idiom 2=Density 3=Media）。
fn dim_kind_code(kind: DimKind) -> i32 {
    match kind {
        DimKind::Tier => 0,
        DimKind::Idiom => 1,
        DimKind::Density => 2,
        DimKind::Media => 3,
    }
}

/// Media 坐标引用字面量 → 运行期坐标值（命名坐标 `true`/`false` → 1.0/0.0）。
pub(crate) fn media_literal_to_f64(lit: &str) -> f64 {
    let t = lit.trim();
    if t.eq_ignore_ascii_case("true") {
        1.0
    } else if t.eq_ignore_ascii_case("false") {
        0.0
    } else {
        t.parse::<f64>().unwrap_or(0.0)
    }
}

/// 发射 `List<int>` 字段赋值（经局部变量，Arc 集合字面量）。
fn emit_list_i32(out: &mut String, field: &str, local: &str, vals: &[i32]) {
    if vals.is_empty() {
        out.push_str(&format!("{field} = new List<int>();\n"));
        return;
    }
    let body = vals
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("List<int> {local} = [{body}];\n"));
    out.push_str(&format!("{field} = {local};\n"));
}

/// 发射 `List<double>` 字段赋值（经局部变量，Arc 集合字面量）。
///
/// `+inf` 渲染为 `1000000000000.0`（容器尺寸无上界哨兵——`<Adaptive>` 无
/// `MaxWidth`；Arc 数值字面量不支持指数记法）。
fn emit_list_f64(out: &mut String, field: &str, local: &str, vals: &[f64]) {
    if vals.is_empty() {
        out.push_str(&format!("{field} = new List<double>();\n"));
        return;
    }
    let body = vals
        .iter()
        .map(|v| {
            if *v == f64::INFINITY {
                "1000000000000.0".to_string()
            } else if v.fract() == 0.0 {
                format!("{v:.1}")
            } else {
                format!("{v}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("List<double> {local} = [{body}];\n"));
    out.push_str(&format!("{field} = {local};\n"));
}
