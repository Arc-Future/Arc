//! `arcgr` 错误类型（RFC 034）。

use thiserror::Error;

/// `.arcgr` 编解码错误。
#[derive(Debug, Error)]
pub enum ArcgrError {
    #[error("truncated header (need 44 bytes, got {0})")]
    TruncatedHeader(usize),

    #[error("bad magic: expected AIDX, got {0:?}")]
    BadMagic([u8; 4]),

    #[error("unsupported version: expected {expected}, got {actual}")]
    UnsupportedVersion { expected: u16, actual: u16 },

    #[error("header CRC32 mismatch: expected {expected:#010x}, got {actual:#010x}")]
    HeaderCrcMismatch { expected: u32, actual: u32 },

    #[error("section truncated: {0}")]
    SectionTruncated(&'static str),

    #[error("invalid SymbolKind enum: {0}")]
    InvalidSymbolKind(u8),

    #[error("invalid Visibility enum: {0}")]
    InvalidVisibility(u8),

    #[error("invalid IntentRole enum: {0}")]
    InvalidIntentRole(u8),

    #[error("invalid IntentMetadata tag: {0}")]
    InvalidIntentMetadata(u8),

    #[error("invalid ReferenceContext enum: {0}")]
    InvalidReferenceContext(u8),

    #[error("invalid EdgeKind enum: {0}")]
    InvalidEdgeKind(u8),

    #[error("invalid EntryPointKind enum: {0}")]
    InvalidEntryPointKind(u8),

    #[error("invalid TypeSigTag: {0}")]
    InvalidTypeSigTag(u8),

    #[error("invalid ProjectKind enum: {0}")]
    InvalidProjectKind(u8),

    #[error("invalid DependencySource enum: {0}")]
    InvalidDependencySource(u8),

    #[error("invalid PublicApiKind enum: {0}")]
    InvalidPublicApiKind(u8),

    #[error("invalid DagEdgeKind enum: {0}")]
    InvalidDagEdgeKind(u8),

    #[error("UTF-8 decode error in {0}")]
    Utf8Error(&'static str),

    #[error("TypeSig trailing bytes: {0}")]
    TypeSigTrailingBytes(usize),

    #[error("section offset out of bounds: {section} at offset {offset}, file size {file_size}")]
    OffsetOutOfBounds {
        section: &'static str,
        offset: u32,
        file_size: usize,
    },
}

/// `arcgr` 操作结果。
pub type Result<T> = std::result::Result<T, ArcgrError>;
