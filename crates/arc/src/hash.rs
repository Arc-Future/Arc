//! 内容哈希工具（增量指纹 / 组件与工具链下载校验共用）。
//!
//! 以字节流为内容键（而非名称/版本），支撑「未变更对象跳过重编」的增量
//! 指纹（[crate::incremental]）与下载分发的完整性校验（[crate::components] /
//! [crate::toolchain]）。

use sha2::{Digest, Sha256};

/// SHA-256 → 小写 hex。
pub fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 内容寻址 SHA-256（内容哈希收敛别名；与 [`hex_sha256`] 同实现）。
pub fn content_sha256(bytes: &[u8]) -> String {
    hex_sha256(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        // SHA-256("abc") 标准测试向量。
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(content_sha256(b""), hex_sha256(b""));
    }
}
