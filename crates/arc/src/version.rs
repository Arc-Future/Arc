//! 最小语义化版本（发布清单版本选择 / self-update 版本标记共用）。
//!
//! 仅支持 `MAJOR.MINOR.PATCH` 三段十进制数字——这是发布协议（`release.rs`
//! manifest）与安装布局（`versions/current` 标记）的契约面。pre-release /
//! build 元数据与 semver/MVS 求解体系不在分发面内（RFC 017 禁止项）。

use std::fmt;
use std::str::FromStr;

/// 三段语义化版本，比较按 `(major, minor, patch)` 字典序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: [&str; 3] = s
            .trim()
            .split('.')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| format!("expected `MAJOR.MINOR.PATCH`, got `{s}`"))?;
        let mut out = [0u64; 3];
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(format!("invalid version component `{part}` in `{s}`"));
            }
            out[i] = part
                .parse::<u64>()
                .map_err(|e| format!("component `{part}` out of range in `{s}`: {e}"))?;
        }
        Ok(Self {
            major: out[0],
            minor: out[1],
            patch: out[2],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_components() {
        let v: Version = "1.0.0".parse().unwrap();
        assert_eq!(v, Version::new(1, 0, 0));
        let v: Version = " 0.2.13 ".parse().unwrap();
        assert_eq!(v, Version::new(0, 2, 13));
        assert_eq!(v.to_string(), "0.2.13");
    }

    #[test]
    fn rejects_malformed() {
        assert!("1.0".parse::<Version>().is_err());
        assert!("1.0.0.0".parse::<Version>().is_err());
        assert!("v1.0.0".parse::<Version>().is_err());
        assert!("1.0.0-rc.1".parse::<Version>().is_err());
        assert!("1..0".parse::<Version>().is_err());
        assert!("1.x.0".parse::<Version>().is_err());
        assert!("99999999999999999999.0.0".parse::<Version>().is_err());
    }

    #[test]
    fn orders_lexicographically() {
        let mut vs = ["0.2.0", "0.10.0", "1.0.0", "0.2.13", "0.2.3"];
        vs.sort_by_key(|s| s.parse::<Version>().unwrap());
        assert_eq!(vs, ["0.2.0", "0.2.3", "0.2.13", "0.10.0", "1.0.0"]);
    }
}
