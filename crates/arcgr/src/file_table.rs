//! `.arcgr` FileTable（RFC 034）。
//!
//! ## 二进制布局
//!
//! ```text
//! FileTable section:
//!   count: u32 LE
//!   entries[]:
//!     FileEntry[i]:
//!       file_id: 4 bytes u32 LE
//!       path_len: 2 bytes u16 LE
//!       path: path_len bytes UTF-8
//!       content_hash: 8 bytes u64 LE
//!       line_count: 4 bytes u32 LE
//! ```

use crate::error::Result;
use crate::io::{read_str, read_u32, read_u64, write_str, write_u32, write_u64};

/// 单个源码文件条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// FileId（包内唯一，0 索引）。
    pub file_id: u32,
    /// 源码文件绝对路径（UTF-8）。
    pub path: String,
    /// 内容 hash（M2 使用 CRC64；外部由调用方计算后传入）。
    pub content_hash: u64,
    /// 行数（用于 LSP 行号边界校验）。
    pub line_count: u32,
}

impl FileEntry {
    pub fn new(file_id: u32, path: String, content_hash: u64, line_count: u32) -> Self {
        Self {
            file_id,
            path,
            content_hash,
            line_count,
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u32(w, self.file_id);
        write_str(w, &self.path);
        write_u64(w, self.content_hash);
        write_u32(w, self.line_count);
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let file_id = read_u32(r)?;
        let path = read_str(r)?;
        let content_hash = read_u64(r)?;
        let line_count = read_u32(r)?;
        Ok(Self {
            file_id,
            path,
            content_hash,
            line_count,
        })
    }
}

/// FileTable——文件清单表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileTable {
    pub entries: Vec<FileEntry>,
}

impl FileTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, entry: FileEntry) {
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
            entries.push(FileEntry::deserialize(r)?);
        }
        Ok(Self { entries })
    }

    /// 按 file_id 查找条目。
    pub fn find(&self, file_id: u32) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.file_id == file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_round_trip() {
        let table = FileTable::new();
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        // count(4 bytes) = 0
        assert_eq!(buf.len(), 4);
        let mut slice = buf.as_slice();
        let table2 = FileTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn single_entry_round_trip() {
        let mut table = FileTable::new();
        table.push(FileEntry::new(
            0,
            "/proj/src/main.as".into(),
            0xDEADBEEFCAFEBABE,
            42,
        ));
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = FileTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert!(slice.is_empty());
    }

    #[test]
    fn multiple_entries_round_trip() {
        let mut table = FileTable::new();
        table.push(FileEntry::new(0, "/proj/a.as".into(), 1, 10));
        table.push(FileEntry::new(1, "/proj/b.as".into(), 2, 20));
        table.push(FileEntry::new(2, "/proj/c.as".into(), 3, 30));

        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = FileTable::deserialize(&mut slice).unwrap();
        assert_eq!(table, table2);
        assert_eq!(table2.entries.len(), 3);
        assert!(slice.is_empty());
    }

    #[test]
    fn find_by_file_id() {
        let mut table = FileTable::new();
        table.push(FileEntry::new(0, "/proj/a.as".into(), 1, 10));
        table.push(FileEntry::new(1, "/proj/b.as".into(), 2, 20));

        assert_eq!(table.find(0).unwrap().path, "/proj/a.as");
        assert_eq!(table.find(1).unwrap().path, "/proj/b.as");
        assert!(table.find(99).is_none());
    }

    #[test]
    fn unicode_path_round_trip() {
        let mut table = FileTable::new();
        table.push(FileEntry::new(0, "/proj/中文/文件.as".into(), 0, 1));
        let mut buf = Vec::new();
        table.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let table2 = FileTable::deserialize(&mut slice).unwrap();
        assert_eq!(table2.entries[0].path, "/proj/中文/文件.as");
    }

    #[test]
    fn truncated_count_rejected() {
        let buf = [0u8, 0]; // 不足 4 字节
        let mut slice = &buf[..];
        let err = FileTable::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, crate::error::ArcgrError::SectionTruncated(_)));
    }
}
