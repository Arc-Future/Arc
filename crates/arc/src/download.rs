//! HTTP(S) 下载工具（`arc toolchain` / `arc component` 分发共用）。
//!
//! 仅提供「GET 绝对 URL → 原始字节」一个动作；SHA256 完整性校验由调用方
//! （[crate::toolchain] / [crate::components]）按各自清单固定值执行。

use std::io::Read;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("cannot fetch {url}: {detail}")]
    Fetch { url: String, detail: String },
}

/// 下载绝对 URL 字节（HTTP(S) GET）。
pub fn http_get_bytes(url: &str) -> Result<Vec<u8>, DownloadError> {
    match ureq::get(url).call() {
        Ok(resp) => {
            let mut buf = Vec::new();
            resp.into_reader()
                .read_to_end(&mut buf)
                .map_err(|e| DownloadError::Fetch {
                    url: url.to_string(),
                    detail: e.to_string(),
                })?;
            Ok(buf)
        }
        Err(e) => Err(DownloadError::Fetch {
            url: url.to_string(),
            detail: e.to_string(),
        }),
    }
}
