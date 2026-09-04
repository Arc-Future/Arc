//! `.arml` 错误类型。

use crate::ast::Span;

/// `.arml` 解析/类型检查错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArmlError {
    /// 词法错误（非法字符、未闭合字符串等）。
    #[error("lex error at {span:?}: {message}")]
    Lex { span: Span, message: String },

    /// 语法错误（意外 token、缺失闭合标签等）。
    #[error("parse error at {span:?}: {message}")]
    Parse { span: Span, message: String },

    /// 类型检查错误（未知组件、属性不匹配、绑定路径无效等）。
    #[error("type error at {span:?}: {message}")]
    Type { span: Span, message: String },

    /// IO 错误（文件读取失败）。存储消息字符串以支持 `Clone`。
    #[error("io error: {message}")]
    Io { message: String },
}

impl From<std::io::Error> for ArmlError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            message: e.to_string(),
        }
    }
}

impl ArmlError {
    pub fn lex(span: Span, message: impl Into<String>) -> Self {
        Self::Lex {
            span,
            message: message.into(),
        }
    }

    pub fn parse(span: Span, message: impl Into<String>) -> Self {
        Self::Parse {
            span,
            message: message.into(),
        }
    }

    pub fn type_error(span: Span, message: impl Into<String>) -> Self {
        Self::Type {
            span,
            message: message.into(),
        }
    }
}

pub type ArmlResult<T> = Result<T, ArmlError>;
