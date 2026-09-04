use ast::*;
use thiserror::Error;

pub(crate) enum FieldOrProperty {
    Field(FieldDef),
    Property,
}

pub(crate) enum ClassBodyMember {
    Field(FieldDef),
    MultiField(Vec<FieldDef>),
    Property(PropertyDef),
    Method(MethodDef),
    Constructor(ConstructorDef),
}

pub(crate) enum InterfaceBodyMember {
    Method(MethodSig),
    Property(PropertyDef),
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("unexpected token at {span:?}: expected {expected}, found {found}")]
    Unexpected {
        span: Span,
        expected: String,
        found: String,
    },
    #[error("unexpected end of file")]
    Eof,
}
