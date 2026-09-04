//! RFC 012 B2: GenerateTo 属性标记方法元数据表（通用机制）——过渡期占位。
//!
//! # 当前状态（2026-07-19 修订——M4 GenerateTo + Expression 树路径）
//!
//! 本模块原定义 [`GenerateToTable`] 与 [`GenerateToEntry`]——通用的「attribute
//! 标记的方法元数据」数据结构，供 codegen 发射 rodata 全局表
//! `@__generateto_attr_table`。该路径偏离用户设计基准（详见 RFC 032 D2.2
//! 2026-07-19 修订），已清理：
//!
//! - **删除** `attr_name_to_kind` 函数（硬编码 8 种 QIF marker → kind 映射，
//!   违反架构红线——codegen 不应感知 QIF 语义）
//! - **保留** [`GenerateToTable`] / [`GenerateToEntry`] 通用数据结构
//!   （codegen API 签名仍引用，过渡期 `entries` 永远为空 `Vec`）
//!
//! # 过渡期行为
//!
//! `arc` crate `qif_collector::collect_generate_to_entries` 永远返回空
//! `GenerateToTable`。codegen `emit_generateto_table` 发射 count=0 的空全局表，
//! `emit_qif_build` 循环 0 次，registry 保持空状态。
//!
//! QIF 测试发现机制临时失效，待 RFC 009 M4 D10.6（构造函数体编译期解释器）
//! + RFC 022 ClassExpression 落地后，由 `std/QIF/` 通过 M4 GenerateTo +
//!   Expression 树机制重新实现（新路径不再使用 `__generateto_attr_table`
//!   rodata 全局表，registry 数据由 splice 后的 `QIFRegistryBuilder.Build()`
//!   方法体直接构造）。

/// RFC 032 B2: GenerateTo 属性标记的方法元数据条目（通用数据结构，过渡期保留）。
///
/// **当前状态**：`arc` crate `qif_collector` 已清理，不再填充此结构。
/// 过渡期 `GenerateToTable.entries` 永远为空 `Vec`。保留此类型仅因 codegen
/// API 签名引用，待新机制落地后随 `generate_to_table` 模块一并删除。
#[derive(Clone, Debug)]
pub struct GenerateToEntry {
    /// 被标记方法的 LLVM 符号名（mangled）。
    pub fn_symbol: String,
    /// 方法简单名。
    pub method_name: String,
    /// 方法所属类名。
    pub class_name: String,
    /// 属性名（如 `Fact` / `Theory`）。
    pub attr_name: String,
    /// InlineData 序列化数据（Theory 专用；Fact 为空 Vec）。
    pub inline_data: Vec<Vec<u8>>,
}

/// RFC 032 B2: GenerateTo 属性表（通用容器，过渡期保留）。
///
/// **当前状态**：`entries` 永远为空。保留此类型仅因 codegen API 签名引用。
#[derive(Clone, Debug, Default)]
pub struct GenerateToTable {
    /// 所有被 GenerateTo 机制标记的方法条目（过渡期永远为空）。
    pub entries: Vec<GenerateToEntry>,
}

impl GenerateToTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn push(&mut self, entry: GenerateToEntry) {
        self.entries.push(entry);
    }
}
