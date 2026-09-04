//! TypeSig 类型签名（与 `.ao` TypeSig 共享 schema，独立实现）。
//!
//! ## schema 共享
//!
//! 本模块的 TypeSig 与 `crates/arc/src/aopkg_format.rs` 中的 TypeSig 共享同一二进制
//! schema（24 种 TypeSigTag），但实现独立。原因：arcgr crate 是基础库，不应反向依赖
//! arc crate（arc 是 CLI 顶层）。
//!
//! ## 二进制编码
//!
//! 每项以 1 字节 tag 起始，后跟 tag 特定的附加数据。递归读取必须按 tag 分支正确
//! 消费附加数据——TypeSig 不含总长度字段（外层包装时需附加长度前缀）。

use crate::error::{ArcgrError, Result};
use crate::io::{read_str, read_u16, read_u32, read_u8, write_str, write_u32, write_u8};

/// TypeSig 递归类型标签（1 字节）。
///
/// 24 种 tag 覆盖完整 Arc 类型系统：
/// - 基元（0-8）：Int/Long/Float/Double/Bool/String/Unit/Null/Object
/// - 复合（10-23）：Named/Func/Method/Property/GenericParam/Nullable/List/Array/
///   Tuple/Closure/Variant/TaskHandle/Span/Expression
///
/// `9` 保留未用（与 RFC schema 一致，留作未来扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeSigTag {
    Int = 0,
    Long = 1,
    Float = 2,
    Double = 3,
    Bool = 4,
    String = 5,
    Unit = 6,
    Null = 7,
    Object = 8,
    UInt = 24,
    ULong = 25,
    UShort = 26,
    SByte = 27,
    Named = 10,
    Func = 11,
    Method = 12,
    Property = 13,
    GenericParam = 14,
    Nullable = 15,
    List = 16,
    Array = 17,
    Tuple = 18,
    Closure = 19,
    Variant = 20,
    TaskHandle = 21,
    Span = 22,
    Expression = 23,
}

impl TypeSigTag {
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => Self::Int,
            1 => Self::Long,
            2 => Self::Float,
            3 => Self::Double,
            4 => Self::Bool,
            5 => Self::String,
            6 => Self::Unit,
            7 => Self::Null,
            8 => Self::Object,
            24 => Self::UInt,
            25 => Self::ULong,
            26 => Self::UShort,
            27 => Self::SByte,
            10 => Self::Named,
            11 => Self::Func,
            12 => Self::Method,
            13 => Self::Property,
            14 => Self::GenericParam,
            15 => Self::Nullable,
            16 => Self::List,
            17 => Self::Array,
            18 => Self::Tuple,
            19 => Self::Closure,
            20 => Self::Variant,
            21 => Self::TaskHandle,
            22 => Self::Span,
            23 => Self::Expression,
            other => return Err(ArcgrError::InvalidTypeSigTag(other)),
        })
    }
}

/// Arc 类型签名（递归二进制编码）。
///
/// 与 `.ao` `ExportEntry.type_sig` 共享同一二进制 schema。
#[derive(Debug, Clone, PartialEq)]
pub enum TypeSig {
    // 基元类型（无附加数据，仅 1 字节 tag）
    Int,
    Long,
    Float,
    Double,
    Bool,
    String,
    Unit,
    Null,
    Object,
    UInt,
    ULong,
    UShort,
    SByte,
    // 复合类型（带附加数据）
    Named {
        fully_qualified_name: String,
        generic_args: Vec<TypeSig>,
    },
    Func {
        params: Vec<TypeSig>,
        ret: Box<TypeSig>,
        captures: bool,
    },
    Method {
        receiver: Box<TypeSig>,
        params: Vec<TypeSig>,
        ret: Box<TypeSig>,
        is_virtual: bool,
        vtable_slot: u16,
    },
    Property {
        prop_type: Box<TypeSig>,
        has_getter: bool,
        has_setter: bool,
    },
    GenericParam {
        param_index: u8,
    },
    Nullable {
        inner: Box<TypeSig>,
    },
    List {
        element_type: Box<TypeSig>,
    },
    Array {
        element_type: Box<TypeSig>,
        length: u32,
    },
    Tuple {
        elements: Vec<TypeSig>,
    },
    Closure {
        fn_sig: Box<TypeSig>,
        env_type: Box<TypeSig>,
    },
    Variant {
        fully_qualified_name: String,
        cases: Vec<VariantCase>,
    },
    TaskHandle {
        result_type: Box<TypeSig>,
    },
    Span {
        element_type: Box<TypeSig>,
    },
    Expression {
        delegate_type: Box<TypeSig>,
    },
}

/// variant case：case 名称 + payload 类型（无 payload case 用 `TypeSig::Unit`）。
#[derive(Debug, Clone, PartialEq)]
pub struct VariantCase {
    pub case_name: String,
    pub payload_type: TypeSig,
    /// 判别值（枚举显式 `= N` / variant case 声明序 tag），与 `aopkg_format::VariantCase` 对齐。
    pub discriminant: u32,
}

impl TypeSig {
    /// 序列化为二进制字节（递归编码，无总长度前缀）。
    pub fn serialize(&self, w: &mut Vec<u8>) {
        match self {
            Self::Int => w.push(TypeSigTag::Int as u8),
            Self::Long => w.push(TypeSigTag::Long as u8),
            Self::Float => w.push(TypeSigTag::Float as u8),
            Self::Double => w.push(TypeSigTag::Double as u8),
            Self::Bool => w.push(TypeSigTag::Bool as u8),
            Self::String => w.push(TypeSigTag::String as u8),
            Self::Unit => w.push(TypeSigTag::Unit as u8),
            Self::Null => w.push(TypeSigTag::Null as u8),
            Self::Object => w.push(TypeSigTag::Object as u8),
            Self::UInt => w.push(TypeSigTag::UInt as u8),
            Self::ULong => w.push(TypeSigTag::ULong as u8),
            Self::UShort => w.push(TypeSigTag::UShort as u8),
            Self::SByte => w.push(TypeSigTag::SByte as u8),
            Self::Named {
                fully_qualified_name,
                generic_args,
            } => {
                w.push(TypeSigTag::Named as u8);
                write_str(w, fully_qualified_name);
                let n = generic_args.len().min(u8::MAX as usize) as u8;
                w.push(n);
                for arg in generic_args {
                    arg.serialize(w);
                }
            }
            Self::Func {
                params,
                ret,
                captures,
            } => {
                w.push(TypeSigTag::Func as u8);
                let n = params.len().min(u8::MAX as usize) as u8;
                w.push(n);
                for p in params {
                    p.serialize(w);
                }
                ret.serialize(w);
                w.push(if *captures { 1u8 } else { 0u8 });
            }
            Self::Method {
                receiver,
                params,
                ret,
                is_virtual,
                vtable_slot,
            } => {
                w.push(TypeSigTag::Method as u8);
                receiver.serialize(w);
                let n = params.len().min(u8::MAX as usize) as u8;
                w.push(n);
                for p in params {
                    p.serialize(w);
                }
                ret.serialize(w);
                w.push(if *is_virtual { 1u8 } else { 0u8 });
                w.extend_from_slice(&vtable_slot.to_le_bytes());
            }
            Self::Property {
                prop_type,
                has_getter,
                has_setter,
            } => {
                w.push(TypeSigTag::Property as u8);
                prop_type.serialize(w);
                w.push(if *has_getter { 1u8 } else { 0u8 });
                w.push(if *has_setter { 1u8 } else { 0u8 });
            }
            Self::GenericParam { param_index } => {
                w.push(TypeSigTag::GenericParam as u8);
                write_u8(w, *param_index);
            }
            Self::Nullable { inner } => {
                w.push(TypeSigTag::Nullable as u8);
                inner.serialize(w);
            }
            Self::List { element_type } => {
                w.push(TypeSigTag::List as u8);
                element_type.serialize(w);
            }
            Self::Array {
                element_type,
                length,
            } => {
                w.push(TypeSigTag::Array as u8);
                element_type.serialize(w);
                write_u32(w, *length);
            }
            Self::Tuple { elements } => {
                w.push(TypeSigTag::Tuple as u8);
                let n = elements.len().min(u8::MAX as usize) as u8;
                w.push(n);
                for e in elements {
                    e.serialize(w);
                }
            }
            Self::Closure { fn_sig, env_type } => {
                w.push(TypeSigTag::Closure as u8);
                fn_sig.serialize(w);
                env_type.serialize(w);
            }
            Self::Variant {
                fully_qualified_name,
                cases,
            } => {
                w.push(TypeSigTag::Variant as u8);
                write_str(w, fully_qualified_name);
                let n = cases.len().min(u8::MAX as usize) as u8;
                w.push(n);
                for case in cases {
                    write_str(w, &case.case_name);
                    w.extend_from_slice(&case.discriminant.to_le_bytes());
                    case.payload_type.serialize(w);
                }
            }
            Self::TaskHandle { result_type } => {
                w.push(TypeSigTag::TaskHandle as u8);
                result_type.serialize(w);
            }
            Self::Span { element_type } => {
                w.push(TypeSigTag::Span as u8);
                element_type.serialize(w);
            }
            Self::Expression { delegate_type } => {
                w.push(TypeSigTag::Expression as u8);
                delegate_type.serialize(w);
            }
        }
    }

    /// 从字节切片反序列化（递归读取，不消费长度前缀）。
    pub fn deserialize(r: &mut &[u8]) -> Result<Self> {
        if r.is_empty() {
            return Err(ArcgrError::SectionTruncated("typesig tag"));
        }
        let tag = TypeSigTag::from_u8(r[0])?;
        *r = &r[1..];
        Ok(match tag {
            TypeSigTag::Int => Self::Int,
            TypeSigTag::Long => Self::Long,
            TypeSigTag::Float => Self::Float,
            TypeSigTag::Double => Self::Double,
            TypeSigTag::Bool => Self::Bool,
            TypeSigTag::String => Self::String,
            TypeSigTag::Unit => Self::Unit,
            TypeSigTag::Null => Self::Null,
            TypeSigTag::Object => Self::Object,
            TypeSigTag::UInt => Self::UInt,
            TypeSigTag::ULong => Self::ULong,
            TypeSigTag::UShort => Self::UShort,
            TypeSigTag::SByte => Self::SByte,
            TypeSigTag::Named => {
                let fully_qualified_name = read_str(r)?;
                let n = read_u8(r)?;
                let mut generic_args = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    generic_args.push(Self::deserialize(r)?);
                }
                Self::Named {
                    fully_qualified_name,
                    generic_args,
                }
            }
            TypeSigTag::Func => {
                let n = read_u8(r)?;
                let mut params = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    params.push(Self::deserialize(r)?);
                }
                let ret = Box::new(Self::deserialize(r)?);
                let captures = read_u8(r)? != 0;
                Self::Func {
                    params,
                    ret,
                    captures,
                }
            }
            TypeSigTag::Method => {
                let receiver = Box::new(Self::deserialize(r)?);
                let n = read_u8(r)?;
                let mut params = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    params.push(Self::deserialize(r)?);
                }
                let ret = Box::new(Self::deserialize(r)?);
                let is_virtual = read_u8(r)? != 0;
                let vtable_slot = read_u16(r)?;
                Self::Method {
                    receiver,
                    params,
                    ret,
                    is_virtual,
                    vtable_slot,
                }
            }
            TypeSigTag::Property => {
                let prop_type = Box::new(Self::deserialize(r)?);
                let has_getter = read_u8(r)? != 0;
                let has_setter = read_u8(r)? != 0;
                Self::Property {
                    prop_type,
                    has_getter,
                    has_setter,
                }
            }
            TypeSigTag::GenericParam => {
                let param_index = read_u8(r)?;
                Self::GenericParam { param_index }
            }
            TypeSigTag::Nullable => {
                let inner = Box::new(Self::deserialize(r)?);
                Self::Nullable { inner }
            }
            TypeSigTag::List => {
                let element_type = Box::new(Self::deserialize(r)?);
                Self::List { element_type }
            }
            TypeSigTag::Array => {
                let element_type = Box::new(Self::deserialize(r)?);
                let length = read_u32(r)?;
                Self::Array {
                    element_type,
                    length,
                }
            }
            TypeSigTag::Tuple => {
                let n = read_u8(r)?;
                let mut elements = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    elements.push(Self::deserialize(r)?);
                }
                Self::Tuple { elements }
            }
            TypeSigTag::Closure => {
                let fn_sig = Box::new(Self::deserialize(r)?);
                let env_type = Box::new(Self::deserialize(r)?);
                Self::Closure { fn_sig, env_type }
            }
            TypeSigTag::Variant => {
                let fully_qualified_name = read_str(r)?;
                let n = read_u8(r)?;
                let mut cases = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let case_name = read_str(r)?;
                    let discriminant = read_u32(r)?;
                    let payload_type = Self::deserialize(r)?;
                    cases.push(VariantCase {
                        case_name,
                        payload_type,
                        discriminant,
                    });
                }
                Self::Variant {
                    fully_qualified_name,
                    cases,
                }
            }
            TypeSigTag::TaskHandle => {
                let result_type = Box::new(Self::deserialize(r)?);
                Self::TaskHandle { result_type }
            }
            TypeSigTag::Span => {
                let element_type = Box::new(Self::deserialize(r)?);
                Self::Span { element_type }
            }
            TypeSigTag::Expression => {
                let delegate_type = Box::new(Self::deserialize(r)?);
                Self::Expression { delegate_type }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_round_trip() {
        for sig in [
            TypeSig::Int,
            TypeSig::Long,
            TypeSig::Float,
            TypeSig::Double,
            TypeSig::Bool,
            TypeSig::String,
            TypeSig::Unit,
            TypeSig::Null,
            TypeSig::Object,
        ] {
            let mut buf = Vec::new();
            sig.serialize(&mut buf);
            assert_eq!(buf.len(), 1, "primitive should be 1 byte");
            let mut slice = buf.as_slice();
            let sig2 = TypeSig::deserialize(&mut slice).unwrap();
            assert_eq!(sig, sig2);
            assert!(slice.is_empty());
        }
    }

    #[test]
    fn named_type_round_trip() {
        let sig = TypeSig::Named {
            fully_qualified_name: "Arc.Collections.List".into(),
            generic_args: vec![TypeSig::Int, TypeSig::String],
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
        assert!(slice.is_empty());
    }

    #[test]
    fn func_type_round_trip() {
        let sig = TypeSig::Func {
            params: vec![TypeSig::Int, TypeSig::String],
            ret: Box::new(TypeSig::Bool),
            captures: true,
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
        assert!(slice.is_empty());
    }

    #[test]
    fn method_type_round_trip() {
        let sig = TypeSig::Method {
            receiver: Box::new(TypeSig::Named {
                fully_qualified_name: "Foo".into(),
                generic_args: vec![],
            }),
            params: vec![TypeSig::Int],
            ret: Box::new(TypeSig::Unit),
            is_virtual: true,
            vtable_slot: 42,
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
        assert!(slice.is_empty());
    }

    #[test]
    fn variant_type_round_trip() {
        let sig = TypeSig::Variant {
            fully_qualified_name: "Result".into(),
            cases: vec![
                VariantCase {
                    case_name: "Ok".into(),
                    payload_type: TypeSig::Int,
                    discriminant: 0,
                },
                VariantCase {
                    case_name: "Err".into(),
                    payload_type: TypeSig::String,
                    discriminant: 1,
                },
            ],
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
        assert!(slice.is_empty());
    }

    #[test]
    fn invalid_tag_rejected() {
        let buf = [9u8]; // 9 是保留 tag
        let mut slice = &buf[..];
        let err = TypeSig::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::InvalidTypeSigTag(9)));
    }

    #[test]
    fn empty_input_rejected() {
        let buf: [u8; 0] = [];
        let mut slice = &buf[..];
        let err = TypeSig::deserialize(&mut slice).unwrap_err();
        assert!(matches!(err, ArcgrError::SectionTruncated("typesig tag")));
    }

    // 测试 write_u32/write_u16 调用以消除未使用警告
    #[test]
    fn array_type_uses_write_u32() {
        let sig = TypeSig::Array {
            element_type: Box::new(TypeSig::Int),
            length: 0x12345678,
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
    }

    #[test]
    fn method_type_uses_write_u16() {
        let sig = TypeSig::Method {
            receiver: Box::new(TypeSig::Object),
            params: vec![],
            ret: Box::new(TypeSig::Unit),
            is_virtual: false,
            vtable_slot: 0xBEEF,
        };
        let mut buf = Vec::new();
        sig.serialize(&mut buf);
        let mut slice = buf.as_slice();
        let sig2 = TypeSig::deserialize(&mut slice).unwrap();
        assert_eq!(sig, sig2);
    }
}
