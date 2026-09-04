//! 源码位置标识：文件 ID、字节偏移 span 与节点包装。

/// 文件标识：索引到编译期 FileRegistry，映射到文件路径。
/// 0 保留给无效/合成节点（Span::DUMMY）；有效文件从 1 开始递增。
pub type FileId = u32;

/// 源码字节偏移 span，含文件标识以支持多文件项目。
/// RFC 024 M0-前置：补全 file_id，避免跨文件偏移冲突。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// 无效 span（file_id=0, start=0, end=0），用于合成节点或未知位置。
    pub const DUMMY: Self = Self {
        file_id: 0,
        start: 0,
        end: 0,
    };

    /// 创建 span，指定文件标识与字节范围。
    pub fn new(file_id: FileId, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    /// 合并两个 span，取最小 start 与最大 end。
    /// file_id 取 self 的（同文件内 merge）；跨文件 merge 不应发生。
    pub fn merge(self, other: Self) -> Self {
        Self {
            file_id: self.file_id,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::DUMMY
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned {
            node: f(self.node),
            span: self.span,
        }
    }
}
