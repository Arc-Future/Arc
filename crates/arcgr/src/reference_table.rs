//! `.arcgr` ReferenceTable（RFC 034）。
//!
//! ## 二进制布局
//!
//! ```text
//! ReferenceTable section:
//!   count: u32 LE
//!   entries[]:
//!     ReferenceEntry[i]:
//!       ref_id: 4 bytes u32 LE
//!       symbol_id: 4 bytes u32 LE
//!       file_id: 4 bytes u32 LE
//!       span_start: 4 bytes u32 LE
//!       span_end: 4 bytes u32 LE
//!       context: 1 byte (ReferenceContext enum)
//! ```

use crate::error::{ArcgrError, Result};
use crate::io::{read_u32, write_u32};

/// 引用上下文（1 字节枚举，覆盖 Arc 全部引用语义）。
///
/// 为 RFC 038 M1 `textDocument/references` 提供语义上下文过滤能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReferenceContext {
    /// 读取（如变量引用、字段读取）。
    Read = 0,
    /// 写入（如赋值左侧、字段写入）。
    Write = 1,
    /// 调用（函数/方法调用）。
    Call = 2,
    /// 实现（class : Interface）。
    Implement = 3,
    /// 继承（class : BaseClass）。
    Inherit = 4,
    /// 导入（using 语句）。
    Import = 5,
    /// 类型标注（参数/变量类型）。
    TypeAnnotation = 6,
    /// 模式匹配（variant match）。
    PatternMatch = 7,
}

impl ReferenceContext {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Read,
            1 => Self::Write,
            2 => Self::Call,
            3 => Self::Implement,
            4 => Self::Inherit,
            5 => Self::Import,
            6 => Self::TypeAnnotation,
            7 => Self::PatternMatch,
            other => return Err(ArcgrError::InvalidReferenceContext(other)),
        })
    }
}

/// 单个引用条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEntry {
    /// 引用 ID（包内唯一）。
    pub ref_id: u32,
    /// 引用的目标符号 ID（关联 SymbolTable）。
    pub symbol_id: u32,
    /// 引用所在文件 ID（关联 FileTable）。
    pub file_id: u32,
    /// 引用 span start（字节偏移）。
    pub span_start: u32,
    /// 引用 span end（字节偏移）。
    pub span_end: u32,
    /// 引用上下文。
    pub context: ReferenceContext,
}

impl ReferenceEntry {
    pub fn new(
        ref_id: u32,
        symbol_id: u32,
        file_id: u32,
        span_start: u32,
        span_end: u32,
        context: ReferenceContext,
    ) -> Self {
        Self {
            ref_id,
            symbol_id,
            file_id,
            span_start,
            span_end,
            context,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.ref_id);
        write_u32(w, self.symbol_id);
        write_u32(w, self.file_id);
        write_u32(w, self.span_start);
        write_u32(w, self.span_end);
        w.push(self.context as u8);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let ref_id = read_u32(r)?;
        let symbol_id = read_u32(r)?;
        let file_id = read_u32(r)?;
        let span_start = read_u32(r)?;
        let span_end = read_u32(r)?;
        let context = ReferenceContext::from_u8(crate::io::read_u8(r)?)?;
        Ok(Self {
            ref_id,
            symbol_id,
            file_id,
            span_start,
            span_end,
            context,
        })
    }
}

/// ReferenceTable——引用清单表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceTable {
    pub entries: Vec<ReferenceEntry>,
}

impl ReferenceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: ReferenceEntry) {
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
            entries.push(ReferenceEntry::deserialize(r)?);
        }
        Ok(Self { entries })
    }

    /// 按目标 symbol_id 查找所有引用（用于 LSP `textDocument/references`）。
    pub fn find_by_symbol(&self, symbol_id: u32) -> Vec<&ReferenceEntry> {
        self.entries
            .iter()
            .filter(|e| e.symbol_id == symbol_id)
            .collect()
    }

    /// 按文件查找所有引用（用于增量索引修正）。
    pub fn find_by_file(&self, file_id: u32) -> Vec<&ReferenceEntry> {
        self.entries
            .iter()
            .filter(|e| e.file_id == file_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_round_trip() {
        let table = ReferenceTable::new();
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = ReferenceTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn single_entry_round_trip() {
        let mut table = ReferenceTable::new();
        table.push(ReferenceEntry::new(
            0,
            5,
            0,
            100,
            110,
            ReferenceContext::Call,
        ));
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = ReferenceTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn all_contexts_round_trip() {
        let contexts = [
            ReferenceContext::Read,
            ReferenceContext::Write,
            ReferenceContext::Call,
            ReferenceContext::Implement,
            ReferenceContext::Inherit,
            ReferenceContext::Import,
            ReferenceContext::TypeAnnotation,
            ReferenceContext::PatternMatch,
        ];
        let mut table = ReferenceTable::new();
        for (i, ctx) in contexts.iter().enumerate() {
            table.push(ReferenceEntry::new(i as u32, i as u32, 0, 0, 0, *ctx));
        }
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = ReferenceTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert_eq!(table2.entries.len(), 8);
        for (i, entry) in table2.entries.iter().enumerate() {
            assert_eq!(entry.context, contexts[i]);
        }
    }

    #[test]
    fn find_by_symbol_id() {
        let mut table = ReferenceTable::new();
        table.push(ReferenceEntry::new(0, 5, 0, 0, 0, ReferenceContext::Call));
        table.push(ReferenceEntry::new(1, 5, 1, 0, 0, ReferenceContext::Read));
        table.push(ReferenceEntry::new(2, 6, 0, 0, 0, ReferenceContext::Call));

        let refs = table.find_by_symbol(5);
        assert_eq!(refs.len(), 2);
        let refs = table.find_by_symbol(6);
        assert_eq!(refs.len(), 1);
        let refs = table.find_by_symbol(99);
        assert!(refs.is_empty());
    }

    #[test]
    fn find_by_file_id() {
        let mut table = ReferenceTable::new();
        table.push(ReferenceEntry::new(0, 1, 0, 0, 0, ReferenceContext::Call));
        table.push(ReferenceEntry::new(1, 2, 1, 0, 0, ReferenceContext::Call));
        table.push(ReferenceEntry::new(2, 3, 0, 0, 0, ReferenceContext::Call));

        let refs = table.find_by_file(0);
        assert_eq!(refs.len(), 2);
        let refs = table.find_by_file(1);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn invalid_context_rejected() {
        let mut buf = Vec::new();
        ReferenceEntry::new(0, 0, 0, 0, 0, ReferenceContext::Read).serialize(&mut buf);
        // context 字节在最后（21 字节，索引 20）
        buf[20] = 0xFF;
        let mut slice = buf.as_slice();
        let err = ReferenceEntry::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidReferenceContext(0xFF)));
    }
}
