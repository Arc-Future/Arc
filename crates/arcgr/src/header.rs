//! `.arcgr` Header（76 字节，RFC 034）。
//!
//! Header 包含 8 个 section 的偏移/大小 + Header CRC32。M2 阶段仅前 4 个
//! section（FileTable/SymbolTable/ReferenceTable/ReferenceGraph）有数据；
//! 后 4 个 section（ContextManifest/TypeRelationGraph/CompletionTable/
//! DiagnosticCache）偏移量与大小均为 0（未实施），M3/M4/M5 各自里程碑填充。
//!
//! ## 二进制布局
//!
//! ```text
//! | Offset | Size | Field                          |
//! |--------|------|--------------------------------|
//! | 0      | 4    | Magic "AIDX"                   |
//! | 4      | 2    | Version (u16 LE)                |
//! | 6      | 2    | Flags (u16 LE)                  |
//! | 8      | 4    | file_table_off                  |
//! | 12     | 4    | file_table_size                 |
//! | 16     | 4    | symbol_table_off                |
//! | 20     | 4    | symbol_table_size               |
//! | 24     | 4    | reference_table_off             |
//! | 28     | 4    | reference_table_size            |
//! | 32     | 4    | reference_graph_off             |
//! | 36     | 4    | reference_graph_size            |
//! | 40     | 4    | context_manifest_off            |  // M4
//! | 44     | 4    | context_manifest_size           |  // M4
//! | 48     | 4    | type_relation_graph_off         |  // M3+
//! | 52     | 4    | type_relation_graph_size        |  // M3+
//! | 56     | 4    | completion_table_off            |  // M3+
//! | 60     | 4    | completion_table_size           |  // M3+
//! | 64     | 4    | diagnostic_cache_off            |  // M3+
//! | 68     | 4    | diagnostic_cache_size           |  // M3+
//! | 72     | 4    | header_crc32                    |  // 覆盖 0..72
//! ```
//!
//! 8 个 section 在 Header 中按 M2 推进顺序排列：前 4 个为 M2 必填，
//! 后 4 个为 M3+ 选填（M2 阶段偏移/大小均为 0 表示 section 不存在）。

use crc32fast::Hasher as Crc32;

use crate::error::{ArcgrError, Result};

/// Magic bytes：`"AIDX"`（Arc Index）。
pub const MAGIC: &[u8; 4] = b"AIDX";

/// `.arcgr` 格式版本。
///
/// - `1`：M2 起始版本（44 字节 Header，4 个 section 偏移）
/// - `2`：完整 schema 版本（76 字节 Header，8 个 section 偏移）——锁定 M3+
///   section 偏移字段，避免后续里程碑再次 bump 版本
pub const VERSION: u16 = 2;

/// Header 大小（字节）。
pub const HEADER_SIZE: u32 = 76;

/// Header 中 section 偏移字段数量。
pub const SECTION_COUNT: usize = 8;

/// Flags 位掩码（M2 阶段全部预留）。
#[allow(dead_code)]
pub const FLAG_RESERVED_1: u16 = 0x0001;

/// `.arcgr` Header（76 字节）。
///
/// 8 个 section 的偏移/大小 + Header CRC32。M2 阶段仅前 4 个 section 有数据；
/// 后 4 个 section 偏移/大小均为 0（M3/M4/M5 各自里程碑填充）。
///
/// **section 缺失判定**：`off == 0 && size == 0` 表示该 section 不存在
/// （未实施或当前编译单元无数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArcgrHeader {
    pub version: u16,
    pub flags: u16,
    // ---- M2 必填 section（4 个） ----
    pub file_table_off: u32,
    pub file_table_size: u32,
    pub symbol_table_off: u32,
    pub symbol_table_size: u32,
    pub reference_table_off: u32,
    pub reference_table_size: u32,
    pub reference_graph_off: u32,
    pub reference_graph_size: u32,
    // ---- M3+ 选填 section（4 个，M2 阶段 off=0, size=0） ----
    /// ContextManifest 子表（M4 实施）。
    pub context_manifest_off: u32,
    pub context_manifest_size: u32,
    /// TypeRelationGraph 子表（M3+ 实施，继承/实现/组合关系图）。
    pub type_relation_graph_off: u32,
    pub type_relation_graph_size: u32,
    /// CompletionTable 子表（M3+ 实施，补全候选）。
    pub completion_table_off: u32,
    pub completion_table_size: u32,
    /// DiagnosticCache 子表（M3+ 实施，编译期诊断快照）。
    pub diagnostic_cache_off: u32,
    pub diagnostic_cache_size: u32,
}

impl ArcgrHeader {
    /// 序列化为 76 字节数组（含 CRC32）。
    pub fn serialize(&self) -> [u8; HEADER_SIZE as usize] {
        let mut buf = [0u8; HEADER_SIZE as usize];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.flags.to_le_bytes());
        buf[8..12].copy_from_slice(&self.file_table_off.to_le_bytes());
        buf[12..16].copy_from_slice(&self.file_table_size.to_le_bytes());
        buf[16..20].copy_from_slice(&self.symbol_table_off.to_le_bytes());
        buf[20..24].copy_from_slice(&self.symbol_table_size.to_le_bytes());
        buf[24..28].copy_from_slice(&self.reference_table_off.to_le_bytes());
        buf[28..32].copy_from_slice(&self.reference_table_size.to_le_bytes());
        buf[32..36].copy_from_slice(&self.reference_graph_off.to_le_bytes());
        buf[36..40].copy_from_slice(&self.reference_graph_size.to_le_bytes());
        buf[40..44].copy_from_slice(&self.context_manifest_off.to_le_bytes());
        buf[44..48].copy_from_slice(&self.context_manifest_size.to_le_bytes());
        buf[48..52].copy_from_slice(&self.type_relation_graph_off.to_le_bytes());
        buf[52..56].copy_from_slice(&self.type_relation_graph_size.to_le_bytes());
        buf[56..60].copy_from_slice(&self.completion_table_off.to_le_bytes());
        buf[60..64].copy_from_slice(&self.completion_table_size.to_le_bytes());
        buf[64..68].copy_from_slice(&self.diagnostic_cache_off.to_le_bytes());
        buf[68..72].copy_from_slice(&self.diagnostic_cache_size.to_le_bytes());

        // CRC32 覆盖 Magic..diagnostic_cache_size（offset 0..72），不含自身。
        let mut crc = Crc32::new();
        crc.update(&buf[0..72]);
        let crc_val = crc.finalize();
        buf[72..76].copy_from_slice(&crc_val.to_le_bytes());
        buf
    }

    /// 从字节切片反序列化（至少 76 字节）。
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE as usize {
            return Err(ArcgrError::TruncatedHeader(bytes.len()));
        }
        if &bytes[0..4] != MAGIC {
            return Err(ArcgrError::BadMagic([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]));
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(ArcgrError::UnsupportedVersion {
                expected: VERSION,
                actual: version,
            });
        }
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);

        // 校验 CRC32（覆盖 0..72）。
        let mut crc = Crc32::new();
        crc.update(&bytes[0..72]);
        let expected = crc.finalize();
        let actual = u32::from_le_bytes([bytes[72], bytes[73], bytes[74], bytes[75]]);
        if expected != actual {
            return Err(ArcgrError::HeaderCrcMismatch { expected, actual });
        }

        Ok(Self {
            version,
            flags,
            file_table_off: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            file_table_size: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            symbol_table_off: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            symbol_table_size: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            reference_table_off: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            reference_table_size: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            reference_graph_off: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
            reference_graph_size: u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
            context_manifest_off: u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            context_manifest_size: u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
            type_relation_graph_off: u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
            type_relation_graph_size: u32::from_le_bytes(bytes[52..56].try_into().unwrap()),
            completion_table_off: u32::from_le_bytes(bytes[56..60].try_into().unwrap()),
            completion_table_size: u32::from_le_bytes(bytes[60..64].try_into().unwrap()),
            diagnostic_cache_off: u32::from_le_bytes(bytes[64..68].try_into().unwrap()),
            diagnostic_cache_size: u32::from_le_bytes(bytes[68..72].try_into().unwrap()),
        })
    }

    /// 判定指定 section 是否存在（off != 0 || size != 0）。
    ///
    /// M2 阶段后 4 个 section 始终返回 false。
    pub fn has_section(&self, section: HeaderSection) -> bool {
        let (off, size) = self.section_bounds(section);
        off != 0 || size != 0
    }

    /// 获取指定 section 的 (offset, size)。
    pub fn section_bounds(&self, section: HeaderSection) -> (u32, u32) {
        match section {
            HeaderSection::FileTable => (self.file_table_off, self.file_table_size),
            HeaderSection::SymbolTable => (self.symbol_table_off, self.symbol_table_size),
            HeaderSection::ReferenceTable => (self.reference_table_off, self.reference_table_size),
            HeaderSection::ReferenceGraph => (self.reference_graph_off, self.reference_graph_size),
            HeaderSection::ContextManifest => {
                (self.context_manifest_off, self.context_manifest_size)
            }
            HeaderSection::TypeRelationGraph => {
                (self.type_relation_graph_off, self.type_relation_graph_size)
            }
            HeaderSection::CompletionTable => {
                (self.completion_table_off, self.completion_table_size)
            }
            HeaderSection::DiagnosticCache => {
                (self.diagnostic_cache_off, self.diagnostic_cache_size)
            }
        }
    }
}

/// Header 中描述的 section 标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderSection {
    FileTable,
    SymbolTable,
    ReferenceTable,
    ReferenceGraph,
    ContextManifest,
    TypeRelationGraph,
    CompletionTable,
    DiagnosticCache,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> ArcgrHeader {
        ArcgrHeader {
            version: VERSION,
            flags: 0,
            file_table_off: 76,
            file_table_size: 100,
            symbol_table_off: 176,
            symbol_table_size: 200,
            reference_table_off: 376,
            reference_table_size: 80,
            reference_graph_off: 456,
            reference_graph_size: 60,
            context_manifest_off: 0,
            context_manifest_size: 0,
            type_relation_graph_off: 0,
            type_relation_graph_size: 0,
            completion_table_off: 0,
            completion_table_size: 0,
            diagnostic_cache_off: 0,
            diagnostic_cache_size: 0,
        }
    }

    #[test]
    fn header_round_trip() {
        let h = sample_header();
        let bytes = h.serialize();
        assert_eq!(bytes.len(), HEADER_SIZE as usize);
        let h2 = ArcgrHeader::deserialize(&bytes).unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = [0u8; 76];
        bytes[0..4].copy_from_slice(b"XXXX");
        let err = ArcgrHeader::deserialize(&bytes).unwrap_err();
        assert!(matches!(err, ArcgrError::BadMagic(_)));
    }

    #[test]
    fn truncated_header_rejected() {
        let bytes = [0u8; 75];
        let err = ArcgrHeader::deserialize(&bytes).unwrap_err();
        assert!(matches!(err, ArcgrError::TruncatedHeader(75)));
    }

    #[test]
    fn crc_tamper_rejected() {
        let h = sample_header();
        let mut bytes = h.serialize();
        bytes[8] ^= 0xFF; // 篡改 file_table_off
        let err = ArcgrHeader::deserialize(&bytes).unwrap_err();
        assert!(matches!(err, ArcgrError::HeaderCrcMismatch { .. }));
    }

    #[test]
    fn unsupported_version_rejected() {
        let mut bytes = sample_header().serialize();
        // 强制写入 version=99（serialize 会写 VERSION=2，这里覆盖）
        bytes[4..6].copy_from_slice(&99u16.to_le_bytes());
        // 重新计算 CRC32 以通过版本检查
        let mut crc = Crc32::new();
        crc.update(&bytes[0..72]);
        let crc_val = crc.finalize();
        bytes[72..76].copy_from_slice(&crc_val.to_le_bytes());
        let err = ArcgrHeader::deserialize(&bytes).unwrap_err();
        assert!(matches!(
            err,
            ArcgrError::UnsupportedVersion {
                expected: 2,
                actual: 99
            }
        ));
    }

    #[test]
    fn m2_optional_sections_default_zero() {
        // M2 阶段 4 个可选 section 偏移/大小必须为 0
        let h = sample_header();
        assert_eq!(h.context_manifest_off, 0);
        assert_eq!(h.context_manifest_size, 0);
        assert_eq!(h.type_relation_graph_off, 0);
        assert_eq!(h.type_relation_graph_size, 0);
        assert_eq!(h.completion_table_off, 0);
        assert_eq!(h.completion_table_size, 0);
        assert_eq!(h.diagnostic_cache_off, 0);
        assert_eq!(h.diagnostic_cache_size, 0);
    }

    #[test]
    fn has_section_correctly_reports_presence() {
        let mut h = sample_header();
        // M2 必填 section 存在
        assert!(h.has_section(HeaderSection::FileTable));
        assert!(h.has_section(HeaderSection::SymbolTable));
        assert!(h.has_section(HeaderSection::ReferenceTable));
        assert!(h.has_section(HeaderSection::ReferenceGraph));
        // M3+ 选填 section 不存在
        assert!(!h.has_section(HeaderSection::ContextManifest));
        assert!(!h.has_section(HeaderSection::TypeRelationGraph));
        assert!(!h.has_section(HeaderSection::CompletionTable));
        assert!(!h.has_section(HeaderSection::DiagnosticCache));

        // 模拟 M4 填充 ContextManifest
        h.context_manifest_off = 600;
        h.context_manifest_size = 200;
        assert!(h.has_section(HeaderSection::ContextManifest));
    }

    #[test]
    fn full_header_with_all_sections_round_trip() {
        // 模拟所有 section 都填充的情况（M5+ 完整状态）
        let h = ArcgrHeader {
            version: VERSION,
            flags: 0,
            file_table_off: 76,
            file_table_size: 100,
            symbol_table_off: 176,
            symbol_table_size: 200,
            reference_table_off: 376,
            reference_table_size: 80,
            reference_graph_off: 456,
            reference_graph_size: 60,
            context_manifest_off: 516,
            context_manifest_size: 500,
            type_relation_graph_off: 1016,
            type_relation_graph_size: 300,
            completion_table_off: 1316,
            completion_table_size: 400,
            diagnostic_cache_off: 1716,
            diagnostic_cache_size: 250,
        };
        let bytes = h.serialize();
        assert_eq!(bytes.len(), 76);
        let h2 = ArcgrHeader::deserialize(&bytes).unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn version_is_2() {
        // 完整 schema 版本必须是 2（76 字节 Header，8 个 section 偏移）
        assert_eq!(VERSION, 2);
        assert_eq!(HEADER_SIZE, 76);
    }
}
