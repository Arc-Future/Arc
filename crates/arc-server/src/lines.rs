//! 共享的 UTF-16 行索引（统一 LSP 位置口径）。
//!
//! ## 定位
//!
//! LSP 的位置（行/列）与区间一律按 **UTF-16 code unit** 计数。本模块提供唯一的
//! 行索引实现，供 [`super::syntax`]（开放文档语法树）与 [`super::semantic`]
//! （`.arcgr` 语义索引）共用——避免两套位置换算口径并存导致的列偏移不一致。
//!
//! ## 设计
//!
//! 记录每行起始的**字节**偏移（`line_starts[0] == 0`，按 `\n` 切行），方法接收
//! `&str` 源码文本，在字节偏移 ↔ UTF-16 位置间换算（非 ASCII 字符按 UTF-16
//! 长度折算，与 LSP 规范一致）。
//!
//! 两套换算口径对比（[`super::semantic`] M1 曾用字节列，此处统一为 UTF-16）：
//!
//! | 方向 | API |
//! |------|-----|
//! | UTF-16 位置 → 字节偏移 | [`LineIndex::offset_of`] |
//! | 字节偏移 → UTF-16 位置 | [`LineIndex::position_of`] |
//! | 字节区间 → UTF-16 长度 | [`LineIndex::utf16_len`] |

/// UTF-16 感知的行索引。
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// 每行起始字节偏移（`line_starts[0] == 0`；按 `\n` 切行）。
    line_starts: Vec<u32>,
}

impl LineIndex {
    /// 从源码文本构建行索引。
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        Self {
            line_starts: starts,
        }
    }

    /// UTF-16 位置（0-based 行/列）→ 字节偏移；行越界返回 `None`。
    pub fn offset_of(&self, text: &str, line: u32, character: u32) -> Option<usize> {
        let line = line as usize;
        let &line_start = self.line_starts.get(line)?;
        let rest = text.get(line_start as usize..)?;
        let mut byte = 0usize;
        let mut units = 0u32;
        for ch in rest.chars() {
            if units >= character {
                break;
            }
            byte += ch.len_utf8();
            units += ch.len_utf16() as u32;
        }
        Some(line_start as usize + byte)
    }

    /// 字节偏移 → UTF-16 位置（0-based 行/列）。越界收敛到文档末尾。
    pub fn position_of(&self, text: &str, offset: usize) -> (u32, u32) {
        let offset = offset.min(text.len());
        // 最后一个 `line_starts[i] <= offset` 的 i 即行号
        let line = self.line_starts.partition_point(|&s| s as usize <= offset) - 1;
        let line_start = self.line_starts[line] as usize;
        let utf16 = text[line_start..offset]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum();
        (line as u32, utf16)
    }

    /// 字节区间 `[start, end)` 的 UTF-16 长度。
    pub fn utf16_len(&self, text: &str, start: usize, end: usize) -> u32 {
        text[start..end].chars().map(|c| c.len_utf16() as u32).sum()
    }

    /// 行数。
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// 指定行（0-based）的起始字节偏移。
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[line] as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_utf16_positions() {
        let text = "ab\n😀c\n\nz";
        let idx = LineIndex::new(text);
        // "😀" 占 4 字节 / 2 UTF-16 单位（列 0-1）；"c" 是第 3 个字符，UTF-16 列 = 2
        let (line, col) = idx.position_of(text, text.find('c').unwrap());
        assert_eq!((line, col), (1, 2));
        // 反向：UTF-16 列 2 → 'c' 的字节偏移
        let off = idx.offset_of(text, 1, 2).unwrap();
        assert_eq!(&text[off..off + 1], "c");
        // 越界列收敛到行尾
        assert!(idx.offset_of(text, 0, 100).is_some());
        // 越界行
        assert!(idx.offset_of(text, 99, 0).is_none());
        // 行数与行起始
        assert_eq!(idx.line_count(), 4);
        assert_eq!(idx.line_start(1), 3);
    }
}
