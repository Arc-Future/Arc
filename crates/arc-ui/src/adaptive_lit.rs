//! RFC 016 §11.1/§11.2 值字面量文法：类型化值元素的 `Value` 解析与单位校验。
//!
//! 编译器对值**零自定义字符串解析**（§11.2「关键裁决」）：`Value="1,2,1"`、
//! `Value="#2D6CDF"` 等是元素**类型**的字面量文法，由本模块按类型解析并检查
//! （`TrackList`/`Color` 各自文法、单位受 `AllowedUnits` 约束），不是自定义
//! 字符串 DSL——与「逻辑谓词必须结构化」的原则正交。

/// 值类型元素（RFC 016 §11.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    /// `<Double>`：长度（vp/px/%/lpx），默认 vp
    Double,
    /// `<Color>`：颜色十六进制
    Color,
    /// `<TrackList>`：Grid 轨道串（逗号 = 轨道分隔）
    TrackList,
    /// `<Thickness>`：四边厚度（1/2/4 个长度）
    Thickness,
    /// `<Boolean>`：布尔
    Boolean,
    /// `<String>`：字符串
    String,
}

impl ValueType {
    /// 从元素名解析（非 `Double`/`Color`/`TrackList`/`Thickness`/`Boolean`/`String` 返回 `None`）。
    pub fn from_element_name(name: &str) -> Option<Self> {
        match name {
            "Double" => Some(Self::Double),
            "Color" => Some(Self::Color),
            "TrackList" => Some(Self::TrackList),
            "Thickness" => Some(Self::Thickness),
            "Boolean" => Some(Self::Boolean),
            "String" => Some(Self::String),
            _ => None,
        }
    }

    pub fn element_name(self) -> &'static str {
        match self {
            Self::Double => "Double",
            Self::Color => "Color",
            Self::TrackList => "TrackList",
            Self::Thickness => "Thickness",
            Self::Boolean => "Boolean",
            Self::String => "String",
        }
    }
}

/// 解析「数值 + 可选单位后缀」。
///
/// 返回 `(number_part, unit_part)`；`unit` 为 `None` 表示纯数字（默认单位 vp）。
/// 单位集合 = `vp`（默认）/`px`/`%`/`lpx`（§11.1）。未知单位 = 错误。
pub fn split_length(s: &str) -> Result<(&str, Option<&str>), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty length".into());
    }
    for unit in ["lpx", "px", "vp", "%"] {
        if let Some(num) = t.strip_suffix(unit) {
            let num = num.trim();
            if num.parse::<f64>().is_err() {
                return Err(format!("`{num}` is not a number"));
            }
            return Ok((num, Some(unit)));
        }
    }
    if t.parse::<f64>().is_ok() {
        return Ok((t, None));
    }
    // 非纯数字：区分未知单位后缀与畸形数值
    let suffix = t.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    if !suffix.is_empty() && suffix != t {
        let unit = &t[suffix.len()..];
        Err(format!("unknown length unit `{unit}`"))
    } else {
        Err(format!("`{t}` is not a number"))
    }
}

fn check_double(value: &str) -> Result<(), String> {
    split_length(value).map(|_| ())
}

fn check_color(value: &str) -> Result<(), String> {
    let t = value.trim();
    let hex = t.strip_prefix('#').unwrap_or(t);
    if ![3, 6, 8].contains(&hex.len()) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "`{t}` is not a color literal (expected `#RGB`/`#RRGGBB`/`#AARRGGBB`)"
        ));
    }
    Ok(())
}

fn check_tracklist(value: &str) -> Result<(), String> {
    let tracks: Vec<&str> = value.split(',').map(str::trim).collect();
    if tracks.iter().any(|t| t.is_empty()) {
        return Err("empty track in `<TrackList>`".into());
    }
    for t in &tracks {
        if matches!(*t, "Auto" | "auto" | "*") {
            continue;
        }
        split_length(t).map_err(|e| format!("track `{t}`: {e}"))?;
    }
    Ok(())
}

fn check_thickness(value: &str) -> Result<(), String> {
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if ![1, 2, 4].contains(&parts.len()) || parts.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "`{value}` must be 1, 2, or 4 comma-separated lengths"
        ));
    }
    for p in &parts {
        split_length(p).map_err(|e| format!("thickness part `{p}`: {e}"))?;
    }
    Ok(())
}

fn check_boolean(value: &str) -> Result<(), String> {
    let t = value.trim();
    if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        Ok(())
    } else {
        Err(format!(
            "`{t}` is not a boolean literal (expected true/false)"
        ))
    }
}

/// 校验类型化值元素的 `Value` 字面量文法（§11.2）。
pub fn check_value_literal(ty: ValueType, value: &str) -> Result<(), String> {
    match ty {
        ValueType::Double => check_double(value),
        ValueType::Color => check_color(value),
        ValueType::TrackList => check_tracklist(value),
        ValueType::Thickness => check_thickness(value),
        ValueType::Boolean => check_boolean(value),
        ValueType::String => Ok(()),
    }
}

/// 断点阈值（`MinWidth`/`MaxWidth`）：**纯数字**，单位固定 vp，
/// 写单位符号 = 编译错误（§11.1/§11.3）。
pub fn parse_breakpoint(value: &str) -> Result<f64, String> {
    let t = value.trim();
    if t.is_empty() {
        return Err("empty breakpoint threshold".into());
    }
    t.parse::<f64>().map_err(|_| {
        format!("`{t}` is not a plain number (breakpoint unit is fixed vp; no unit symbol allowed)")
    })
}
