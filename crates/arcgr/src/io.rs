//! `.arcgr` 共享 IO 辅助函数。
//!
//! 提供 u8/u16/u32/u64/str 的 LE 序列化与反序列化。
//! 所有表模块（file_table/symbol_table/reference_table/reference_graph/typesig）
//! 共用此模块的辅助函数，保持二进制读写一致性。

use crate::error::{ArcgrError, Result};

pub(crate) fn write_u8(w: &mut Vec<u8>, v: u8) {
    w.push(v);
}

pub(crate) fn write_u16(w: &mut Vec<u8>, v: u16) {
    w.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_u32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_u64(w: &mut Vec<u8>, v: u64) {
    w.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn write_str(w: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    w.extend_from_slice(&len.to_le_bytes());
    w.extend_from_slice(&bytes[..len as usize]);
}

pub(crate) fn read_u8(r: &mut &[u8]) -> Result<u8> {
    if r.is_empty() {
        return Err(ArcgrError::SectionTruncated("u8"));
    }
    let v = r[0];
    *r = &r[1..];
    Ok(v)
}

pub(crate) fn read_u16(r: &mut &[u8]) -> Result<u16> {
    if r.len() < 2 {
        return Err(ArcgrError::SectionTruncated("u16"));
    }
    let v = u16::from_le_bytes([r[0], r[1]]);
    *r = &r[2..];
    Ok(v)
}

pub(crate) fn read_u32(r: &mut &[u8]) -> Result<u32> {
    if r.len() < 4 {
        return Err(ArcgrError::SectionTruncated("u32"));
    }
    let v = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
    *r = &r[4..];
    Ok(v)
}

pub(crate) fn read_u64(r: &mut &[u8]) -> Result<u64> {
    if r.len() < 8 {
        return Err(ArcgrError::SectionTruncated("u64"));
    }
    let v = u64::from_le_bytes([r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7]]);
    *r = &r[8..];
    Ok(v)
}

pub(crate) fn read_str(r: &mut &[u8]) -> Result<String> {
    let len = read_u16(r)? as usize;
    if r.len() < len {
        return Err(ArcgrError::SectionTruncated("string"));
    }
    let s = std::str::from_utf8(&r[..len])
        .map_err(|_| ArcgrError::Utf8Error("string"))?
        .to_string();
    *r = &r[len..];
    Ok(s)
}
