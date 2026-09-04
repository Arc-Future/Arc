//! `.arcgr` SymbolTable（RFC 034）。
//!
//! ## 二进制布局
//!
//! ```text
//! SymbolTable section:
//!   count: u32 LE
//!   entries[]:
//!     SymbolEntry[i]:
//!       symbol_id: 4 bytes u32 LE
//!       name_len: 2 bytes u16 LE
//!       name: name_len bytes UTF-8
//!       kind: 1 byte (SymbolKind enum)
//!       visibility: 1 byte (Visibility enum)
//!       file_id: 4 bytes u32 LE
//!       span_start: 4 bytes u32 LE
//!       span_end: 4 bytes u32 LE
//!       type_sig_len: 2 bytes u16 LE
//!       type_sig: type_sig_len bytes (TypeSig 递归编码)
//!       doc_summary_len: 2 bytes u16 LE (0 表示无)
//!       doc_summary: doc_summary_len bytes UTF-8 (if len > 0)
//!       intent_meta: IntentMeta 编码（role 1 byte + has_metadata 1 byte + 可选变长 metadata）
//! ```

use crate::error::{ArcgrError, Result};
use crate::intent_meta::IntentMeta;
use crate::io::{read_str, read_u16, read_u32, read_u8, write_str, write_u16, write_u32};
use crate::typesig::TypeSig;

/// 符号类别（1 字节，与 `.ao` `ExportKind` 二进制表示对齐）。
///
/// schema 共享但实现独立——便于 `.arcgr` 符号跳转到 `.ao` 包导出符号时无需转换。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolKind {
    Function = 0,
    Method = 1,
    StaticMethod = 2,
    Property = 3,
    Field = 4,
    Class = 5,
    Struct = 6,
    Interface = 7,
    Enum = 8,
    Variant = 9,
    Constant = 10,
    Module = 11,
}

impl SymbolKind {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Function,
            1 => Self::Method,
            2 => Self::StaticMethod,
            3 => Self::Property,
            4 => Self::Field,
            5 => Self::Class,
            6 => Self::Struct,
            7 => Self::Interface,
            8 => Self::Enum,
            9 => Self::Variant,
            10 => Self::Constant,
            11 => Self::Module,
            other => return Err(ArcgrError::InvalidSymbolKind(other)),
        })
    }
}

/// 符号可见性（1 字节，与 `.ao` `Visibility` 二进制表示对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Visibility {
    Public = 0,
    Internal = 1,
    Protected = 2,
    Private = 3,
}

impl Visibility {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Public,
            1 => Self::Internal,
            2 => Self::Protected,
            3 => Self::Private,
            other => return Err(ArcgrError::InvalidVisibility(other)),
        })
    }
}

/// 单个符号条目。
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolEntry {
    pub symbol_id: u32,
    /// 符号名（短名，非 FQN）。
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub file_id: u32,
    pub span_start: u32,
    pub span_end: u32,
    pub type_sig: TypeSig,
    /// 文档摘要（可选）。
    pub doc_summary: Option<String>,
    /// IntentMeta——符号意图元数据（M2 占位 None，M5 填充真实数据）。
    pub intent_meta: IntentMeta,
}

impl SymbolEntry {
    /// 创建新符号条目（M2 阶段 intent_meta 为占位默认值 None）。
    pub fn new(
        symbol_id: u32,
        name: impl Into<String>,
        kind: SymbolKind,
        visibility: Visibility,
        file_id: u32,
        span_start: u32,
        span_end: u32,
        type_sig: TypeSig,
        doc_summary: Option<String>,
    ) -> Self {
        Self {
            symbol_id,
            name: name.into(),
            kind,
            visibility,
            file_id,
            span_start,
            span_end,
            type_sig,
            doc_summary,
            intent_meta: IntentMeta::none(),
        }
    }

    /// 设置 IntentMeta（M5 实施期使用，M2 阶段不需要）。
    #[allow(dead_code)]
    pub fn with_intent_meta(mut self, meta: IntentMeta) -> Self {
        self.intent_meta = meta;
        self
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.symbol_id);
        write_str(w, &self.name);
        w.push(self.kind as u8);
        w.push(self.visibility as u8);
        write_u32(w, self.file_id);
        write_u32(w, self.span_start);
        write_u32(w, self.span_end);

        // type_sig: u16 长度前缀 + 递归编码
        let mut sig_buf = Vec::new();
        self.type_sig.serialize(&mut sig_buf);
        let sig_len = sig_buf.len().min(u16::MAX as usize) as u16;
        write_u16(w, sig_len);
        w.extend_from_slice(&sig_buf[..sig_len as usize]);

        // doc_summary: u16 长度前缀（0 = None）
        match &self.doc_summary {
            Some(s) => {
                let s_bytes = s.as_bytes();
                let s_len = s_bytes.len().min(u16::MAX as usize) as u16;
                write_u16(w, s_len);
                w.extend_from_slice(&s_bytes[..s_len as usize]);
            }
            None => {
                write_u16(w, 0);
            }
        }

        // IntentMeta（role + 可选 metadata）
        self.intent_meta.serialize(w);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let symbol_id = read_u32(r)?;
        let name = read_str(r)?;
        let kind = SymbolKind::from_u8(read_u8(r)?)?;
        let visibility = Visibility::from_u8(read_u8(r)?)?;
        let file_id = read_u32(r)?;
        let span_start = read_u32(r)?;
        let span_end = read_u32(r)?;

        // type_sig
        let sig_len = read_u16(r)? as usize;
        if r.len() < sig_len {
            return Err(ArcgrError::SectionTruncated("symbol type_sig"));
        }
        let mut sig_slice = &r[..sig_len];
        let type_sig = TypeSig::deserialize(&mut sig_slice)?;
        if !sig_slice.is_empty() {
            return Err(ArcgrError::TypeSigTrailingBytes(sig_slice.len()));
        }
        *r = &r[sig_len..];

        // doc_summary
        let doc_len = read_u16(r)? as usize;
        let doc_summary = if doc_len == 0 {
            None
        } else {
            if r.len() < doc_len {
                return Err(ArcgrError::SectionTruncated("symbol doc_summary"));
            }
            let s = std::str::from_utf8(&r[..doc_len])
                .map_err(|_| ArcgrError::Utf8Error("symbol doc_summary"))?
                .to_string();
            *r = &r[doc_len..];
            Some(s)
        };

        // IntentMeta
        let intent_meta = IntentMeta::deserialize(r)?;

        Ok(Self {
            symbol_id,
            name,
            kind,
            visibility,
            file_id,
            span_start,
            span_end,
            type_sig,
            doc_summary,
            intent_meta,
        })
    }
}

/// SymbolTable——符号定义表。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolTable {
    pub entries: Vec<SymbolEntry>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: SymbolEntry) {
        self.entries.push(entry);
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.entries.len() as u32);
        for entry in &self.entries {
            entry.serialize(w);
        }
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let count = read_u32(r)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(SymbolEntry::deserialize(r)?);
        }
        Ok(Self { entries })
    }

    /// 按 symbol_id 查找条目。
    pub fn find(&self, symbol_id: u32) -> Option<&SymbolEntry> {
        self.entries.iter().find(|e| e.symbol_id == symbol_id)
    }

    /// 按名称查找所有同名符号（短路返回第一个用于 LSP 单点跳转）。
    pub fn find_by_name(&self, name: &str) -> Vec<&SymbolEntry> {
        self.entries.iter().filter(|e| e.name == name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(id: u32, name: &str) -> SymbolEntry {
        SymbolEntry::new(
            id,
            name,
            SymbolKind::Function,
            Visibility::Public,
            0,
            10,
            20,
            TypeSig::Func {
                params: vec![TypeSig::Int],
                ret: Box::new(TypeSig::Unit),
                captures: false,
            },
            Some("Sample function".into()),
        )
    }

    #[test]
    fn empty_table_round_trip() {
        let table = SymbolTable::new();
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = SymbolTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn single_entry_round_trip() {
        let mut table = SymbolTable::new();
        table.push(sample_entry(0, "main"));
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = SymbolTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn multiple_entries_with_various_kinds_round_trip() {
        let mut table = SymbolTable::new();
        table.push(SymbolEntry::new(
            0,
            "main",
            SymbolKind::Function,
            Visibility::Public,
            0,
            0,
            50,
            TypeSig::Func {
                params: vec![],
                ret: Box::new(TypeSig::Unit),
                captures: false,
            },
            None,
        ));
        table.push(SymbolEntry::new(
            1,
            "Calculator",
            SymbolKind::Class,
            Visibility::Public,
            0,
            100,
            200,
            TypeSig::Named {
                fully_qualified_name: "Calculator".into(),
                generic_args: vec![],
            },
            Some("Calculator class".into()),
        ));
        table.push(SymbolEntry::new(
            2,
            "add",
            SymbolKind::Method,
            Visibility::Public,
            1,
            30,
            80,
            TypeSig::Method {
                receiver: Box::new(TypeSig::Named {
                    fully_qualified_name: "Calculator".into(),
                    generic_args: vec![],
                }),
                params: vec![TypeSig::Int, TypeSig::Int],
                ret: Box::new(TypeSig::Int),
                is_virtual: false,
                vtable_slot: 0,
            },
            None,
        ));

        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = SymbolTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert_eq!(table2.entries.len(), 3);
        assert!(slice.is_empty());
    }

    #[test]
    fn find_by_symbol_id() {
        let mut table = SymbolTable::new();
        table.push(sample_entry(0, "a"));
        table.push(sample_entry(1, "b"));
        assert_eq!(table.find(0).unwrap().name, "a");
        assert_eq!(table.find(1).unwrap().name, "b");
        assert!(table.find(99).is_none());
    }

    #[test]
    fn find_by_name_returns_all_matches() {
        let mut table = SymbolTable::new();
        table.push(sample_entry(0, "add"));
        table.push(sample_entry(1, "add"));
        table.push(sample_entry(2, "sub"));
        let matches = table.find_by_name("add");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn invalid_symbol_kind_rejected() {
        let mut buf = Vec::new();
        // 构造最小有效条目然后篡改 kind 字节为 0xFF
        let entry = sample_entry(0, "x");
        entry.serialize(&mut buf);
        // kind 字节在第 6 字节（symbol_id=4 + name_len=2 + name=1 = 第 7 字节，索引 6）
        // 实际：symbol_id(4) + name_len(2) + name(1 for "x") = offset 7
        // kind 在 offset 7
        buf[7] = 0xFF;
        let mut slice = buf.as_slice();
        let err = SymbolEntry::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidSymbolKind(0xFF)));
    }

    #[test]
    fn intent_meta_default_is_none_placeholder() {
        // M2 阶段所有 SymbolEntry 的 intent_meta 必须是占位默认值
        let entry = sample_entry(0, "x");
        assert_eq!(entry.intent_meta.role, crate::intent_meta::IntentRole::None);
        assert!(entry.intent_meta.metadata.is_none());
    }

    #[test]
    fn intent_meta_with_hotness_round_trip() {
        use crate::intent_meta::{IntentMetadata, IntentRole};

        let entry = sample_entry(0, "hot_fn").with_intent_meta(IntentMeta::with_metadata(
            IntentRole::HotPath,
            IntentMetadata::Hotness {
                calls_per_sec: 5000,
                avg_latency_ns: 200,
            },
        ));
        let mut table = SymbolTable::new();
        table.push(entry);

        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = SymbolTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert_eq!(table2.entries[0].intent_meta.role, IntentRole::HotPath);
    }
}
