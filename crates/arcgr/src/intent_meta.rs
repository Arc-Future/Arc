//! `.arcgr` IntentMeta（RFC 034）。
//!
//! 符号意图元数据——通过 `[Facade]`/`[AbiBoundary]`/`[HotPath]`/`[Stable]`/`[Internal]`
//! attribute（RFC 012）声明，typeck 消费后填充到 SymbolTable.entry.intent_meta。
//!
//! ## 二进制布局（嵌入 SymbolTable 每条 entry 末尾）
//!
//! ```text
//! intent_meta:
//!   intent_role: 1 byte (IntentRole enum)
//!   has_metadata: 1 byte (0/1)
//!   if has_metadata == 1:
//!     metadata_tag: 1 byte (IntentMetadata enum)
//!     metadata_data: 变长（按 tag 分支读取）
//!       Hotness:        4 bytes u32 LE (calls_per_sec)
//!                       + 4 bytes u32 LE (avg_latency_ns)
//!       Boundary:       2 bytes u16 LE (abi_version)
//!                       + 2 bytes u16 LE (contract_name_len)
//!                       + contract_name_len bytes UTF-8
//!       Stability:      2 bytes u16 LE (since_major)
//!                       + 2 bytes u16 LE (since_minor)
//!                       + 1 byte (deprecated: 0/1)
//!                       + 2 bytes u16 LE (deprecation_msg_len)
//!                       + deprecation_msg_len bytes UTF-8
//!       FacadeLayer:    1 byte (layer_index)
//!                       + 1 byte (parent_count)
//!                       + 4 bytes × parent_count (parent_facade_symbol_ids u32 LE)
//!       InternalGroup:  1 byte (group_name_len)
//!                       + group_name_len bytes UTF-8
//! ```
//!
//! ## M2 占位策略
//!
//! M2 阶段所有 SymbolEntry 写入 `role=None + has_metadata=0`（2 字节占位），
//! M5 实施时填充真实 IntentMeta 数据。schema 完整定义先行——遵循 R1
//! 「前置 schema 先行」原则。

use crate::error::{ArcgrError, Result};
use crate::io::{read_u16, read_u32, read_u8, write_u16, write_u32, write_u8};

/// 符号角色枚举（1 字节）。
///
/// 通过 RFC 009 attribute 注册，5 种角色覆盖 Arc 设计哲学的符号分类：
/// - `Facade`：抽象 facade 层（如 std/Arc/ 提供的高级抽象）
/// - `AbiBoundary`：ABI 边界符号（如 extern 函数、rt_* ABI）
/// - `HotPath`：热路径性能关键符号（调度器、内存分配器）
/// - `Stable`：稳定 API（向后兼容承诺）
/// - `Internal`：内部实现细节（不对外暴露）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum IntentRole {
    /// 默认无角色（普通符号）。
    #[default]
    None = 0,
    /// `[Facade]`：抽象 facade 层。
    Facade = 1,
    /// `[AbiBoundary]`：ABI 边界符号。
    AbiBoundary = 2,
    /// `[HotPath]`：热路径性能关键符号。
    HotPath = 3,
    /// `[Stable]`：稳定 API。
    Stable = 4,
    /// `[Internal]`：内部实现细节。
    Internal = 5,
}

impl IntentRole {
    pub fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::None,
            1 => Self::Facade,
            2 => Self::AbiBoundary,
            3 => Self::HotPath,
            4 => Self::Stable,
            5 => Self::Internal,
            other => return Err(ArcgrError::InvalidIntentRole(other)),
        })
    }
}

/// 可选元数据变体（1 字节 tag + 变长数据）。
///
/// 每种 role 可选关联不同元数据，`has_metadata` 字段允许「仅有角色无元数据」
/// 的轻量场景，避免二进制体积膨胀。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntentMetadata {
    /// 热度值（用于 HotPath，记录调用频率/性能预算）。
    Hotness {
        calls_per_sec: u32,
        avg_latency_ns: u32,
    },
    /// 边界声明（用于 AbiBoundary，记录 ABI 版本/契约文件）。
    Boundary {
        abi_version: u16,
        contract_name: String,
    },
    /// 稳定性承诺（用于 Stable，记录 since 版本 + deprecation 状态）。
    Stability {
        since_major: u16,
        since_minor: u16,
        deprecated: bool,
        deprecation_msg: String,
    },
    /// facade 层级（用于 Facade，记录层号与上下层关系）。
    FacadeLayer {
        layer_index: u8,
        parent_facade_symbol_ids: Vec<u32>,
    },
    /// 内部分组（用于 Internal，记录模块归属与可见性边界）。
    InternalGroup { group_name: String },
}

impl IntentMetadata {
    /// 1 字节 tag（与 D5.1 schema 对齐）。
    fn tag(&self) -> u8 {
        match self {
            Self::Hotness { .. } => 1,
            Self::Boundary { .. } => 2,
            Self::Stability { .. } => 3,
            Self::FacadeLayer { .. } => 4,
            Self::InternalGroup { .. } => 5,
        }
    }

    fn from_tag(tag: u8, r: &mut &[u8]) -> Result<Self> {
        Ok(match tag {
            1 => {
                let calls_per_sec = read_u32(r)?;
                let avg_latency_ns = read_u32(r)?;
                Self::Hotness {
                    calls_per_sec,
                    avg_latency_ns,
                }
            }
            2 => {
                let abi_version = read_u16(r)?;
                let contract_name = read_str_u8(r)?;
                Self::Boundary {
                    abi_version,
                    contract_name,
                }
            }
            3 => {
                let since_major = read_u16(r)?;
                let since_minor = read_u16(r)?;
                let deprecated = read_u8(r)? != 0;
                let deprecation_msg = read_str_u16(r)?;
                Self::Stability {
                    since_major,
                    since_minor,
                    deprecated,
                    deprecation_msg,
                }
            }
            4 => {
                let layer_index = read_u8(r)?;
                let parent_count = read_u8(r)? as usize;
                let mut parent_facade_symbol_ids = Vec::with_capacity(parent_count);
                for _ in 0..parent_count {
                    parent_facade_symbol_ids.push(read_u32(r)?);
                }
                Self::FacadeLayer {
                    layer_index,
                    parent_facade_symbol_ids,
                }
            }
            5 => {
                let group_name = read_str_u8(r)?;
                Self::InternalGroup { group_name }
            }
            other => return Err(ArcgrError::InvalidIntentMetadata(other)),
        })
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u8(w, self.tag());
        match self {
            Self::Hotness {
                calls_per_sec,
                avg_latency_ns,
            } => {
                write_u32(w, *calls_per_sec);
                write_u32(w, *avg_latency_ns);
            }
            Self::Boundary {
                abi_version,
                contract_name,
            } => {
                write_u16(w, *abi_version);
                write_str_u8(w, contract_name);
            }
            Self::Stability {
                since_major,
                since_minor,
                deprecated,
                deprecation_msg,
            } => {
                write_u16(w, *since_major);
                write_u16(w, *since_minor);
                w.push(if *deprecated { 1u8 } else { 0u8 });
                write_str_u16(w, deprecation_msg);
            }
            Self::FacadeLayer {
                layer_index,
                parent_facade_symbol_ids,
            } => {
                w.push(*layer_index);
                let count = parent_facade_symbol_ids.len().min(u8::MAX as usize) as u8;
                w.push(count);
                for id in &parent_facade_symbol_ids[..count as usize] {
                    write_u32(w, *id);
                }
            }
            Self::InternalGroup { group_name } => {
                write_str_u8(w, group_name);
            }
        }
    }
}

/// 完整 IntentMeta——嵌入 SymbolEntry 末尾。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IntentMeta {
    pub role: IntentRole,
    pub metadata: Option<IntentMetadata>,
}

impl IntentMeta {
    /// M2 默认占位：role=None，无 metadata。
    pub fn none() -> Self {
        Self {
            role: IntentRole::None,
            metadata: None,
        }
    }

    /// 仅角色无元数据（轻量场景）。
    pub fn role_only(role: IntentRole) -> Self {
        Self {
            role,
            metadata: None,
        }
    }

    /// 角色 + 元数据。
    pub fn with_metadata(role: IntentRole, metadata: IntentMetadata) -> Self {
        Self {
            role,
            metadata: Some(metadata),
        }
    }

    pub fn serialize(&self, w: &mut Vec<u8>) {
        write_u8(w, self.role as u8);
        match &self.metadata {
            Some(m) => {
                w.push(1u8);
                m.serialize(w);
            }
            None => {
                w.push(0u8);
            }
        }
    }

    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        let role = IntentRole::from_u8(read_u8(r)?)?;
        let has_metadata = read_u8(r)? != 0;
        let metadata = if has_metadata {
            let tag = read_u8(r)?;
            Some(IntentMetadata::from_tag(tag, r)?)
        } else {
            None
        };
        Ok(Self { role, metadata })
    }
}

// ---- 内部 IO 辅助——长度前缀分别为 u8 / u16 的 UTF-8 字符串 ----
//
// D5.1 schema 中不同字段的长度前缀宽度不同（u8 用于短字符串如 contract_name，
// u16 用于较长字符串如 deprecation_msg）。这里与 schema 对齐，避免宽度歧义。

fn write_str_u8(w: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u8::MAX as usize) as u8;
    w.push(len);
    w.extend_from_slice(&bytes[..len as usize]);
}

fn read_str_u8(r: &mut &[u8]) -> Result<String> {
    let len = read_u8(r)? as usize;
    if r.len() < len {
        return Err(ArcgrError::SectionTruncated("IntentMetadata str_u8"));
    }
    let s = std::str::from_utf8(&r[..len])
        .map_err(|_| ArcgrError::Utf8Error("IntentMetadata str_u8"))?
        .to_string();
    *r = &r[len..];
    Ok(s)
}

fn write_str_u16(w: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    write_u16(w, len);
    w.extend_from_slice(&bytes[..len as usize]);
}

fn read_str_u16(r: &mut &[u8]) -> Result<String> {
    let len = read_u16(r)? as usize;
    if r.len() < len {
        return Err(ArcgrError::SectionTruncated("IntentMetadata str_u16"));
    }
    let s = std::str::from_utf8(&r[..len])
        .map_err(|_| ArcgrError::Utf8Error("IntentMetadata str_u16"))?
        .to_string();
    *r = &r[len..];
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_round_trip() {
        let m = IntentMeta::none();
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        // role(1) + has_metadata(1) = 2 bytes
        assert_eq!(buf.len(), 2);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
        assert!(slice.is_empty());
    }

    #[test]
    fn role_only_round_trip() {
        let m = IntentMeta::role_only(IntentRole::Stable);
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        assert_eq!(buf.len(), 2);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn hotness_metadata_round_trip() {
        let m = IntentMeta::with_metadata(
            IntentRole::HotPath,
            IntentMetadata::Hotness {
                calls_per_sec: 10_000,
                avg_latency_ns: 500,
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        // role(1) + has_metadata(1) + tag(1) + u32 + u32 = 11 bytes
        assert_eq!(buf.len(), 11);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn boundary_metadata_round_trip() {
        let m = IntentMeta::with_metadata(
            IntentRole::AbiBoundary,
            IntentMetadata::Boundary {
                abi_version: 3,
                contract_name: "Arc.Runtime".into(),
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn stability_metadata_round_trip() {
        let m = IntentMeta::with_metadata(
            IntentRole::Stable,
            IntentMetadata::Stability {
                since_major: 1,
                since_minor: 5,
                deprecated: true,
                deprecation_msg: "use NewApi instead".into(),
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn facade_layer_metadata_round_trip() {
        let m = IntentMeta::with_metadata(
            IntentRole::Facade,
            IntentMetadata::FacadeLayer {
                layer_index: 2,
                parent_facade_symbol_ids: vec![10, 20, 30],
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn internal_group_metadata_round_trip() {
        let m = IntentMeta::with_metadata(
            IntentRole::Internal,
            IntentMetadata::InternalGroup {
                group_name: "codegen::lower".into(),
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn all_roles_serialized_correctly() {
        let roles = [
            IntentRole::None,
            IntentRole::Facade,
            IntentRole::AbiBoundary,
            IntentRole::HotPath,
            IntentRole::Stable,
            IntentRole::Internal,
        ];
        for role in roles {
            let m = IntentMeta::role_only(role);
            let mut buf = Vec::new();
            m.serialize(&mut buf);
            let mut slice = buf.as_slice();
            let m2 = IntentMeta::deserialize(&mut slice).unwrap();
            assert_eq!(m, m2);
        }
    }

    #[test]
    fn invalid_role_rejected() {
        let err = IntentRole::from_u8(0xFF).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidIntentRole(0xFF)));
    }

    #[test]
    fn invalid_metadata_tag_rejected() {
        // 构造：role=HotPath + has_metadata=1 + tag=0xFF
        let buf = vec![3, 1, 0xFF];
        let mut slice = buf.as_slice();
        let err = IntentMeta::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidIntentMetadata(0xFF)));
    }

    #[test]
    fn facade_layer_with_max_parents_round_trip() {
        // 验证 parent_count 上限为 u8::MAX
        let ids: Vec<u32> = (0..u8::MAX as u32).collect();
        let m = IntentMeta::with_metadata(
            IntentRole::Facade,
            IntentMetadata::FacadeLayer {
                layer_index: 0,
                parent_facade_symbol_ids: ids,
            },
        );
        let mut buf = Vec::new();
        m.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let m2 = IntentMeta::deserialize(&mut slice).unwrap();
        assert_eq!(m, m2);
    }
}
