//! clang/LLVM 版本解析与支持基线校验（Phase 2 R-2 定案）。
//!
//! 支持基线 = LLVM 22（与 `.aopkg` metadata `llvm_version: "22"` 对齐，
//! 见 `aopkg_format.rs` / `main.rs publish`）。`arc doctor` 以此下限判定 FAIL；
//! `arc toolchain install llvm` 以此判定已装版本是否达标。

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// clang/LLVM 支持基线（R-2 定案：与 `.aopkg` metadata 一致）。
pub const LLVM_MIN_VERSION: &str = "22.0.0";

/// 解析后的 clang 版本（三项语义化版本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClangVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl ClangVersion {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl PartialOrd for ClangVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ClangVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

impl fmt::Display for ClangVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for ClangVersion {
    type Err = ClangVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() {
            return Err(ClangVersionError(t.to_string()));
        }
        let mut parts = t.split('.');
        let major = parse_part(parts.next(), t)?;
        let minor = match parts.next() {
            Some(p) => parse_part(Some(p), t)?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(p) => parse_part(Some(p), t)?,
            None => 0,
        };
        if parts.next().is_some() {
            return Err(ClangVersionError(t.to_string()));
        }
        Ok(ClangVersion::new(major, minor, patch))
    }
}

fn parse_part(raw: Option<&str>, full: &str) -> Result<u64, ClangVersionError> {
    let Some(p) = raw else {
        return Err(ClangVersionError(full.to_string()));
    };
    p.parse::<u64>()
        .map_err(|_| ClangVersionError(full.to_string()))
}

/// 版本解析错误（`major[.minor[.patch]]` 形式）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClangVersionError(pub String);

impl fmt::Display for ClangVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid clang version `{}`", self.0)
    }
}

/// 支持基线（`LLVM_MIN_VERSION`）解析后的版本。
pub fn min_supported_version() -> ClangVersion {
    LLVM_MIN_VERSION
        .parse()
        .expect("LLVM_MIN_VERSION must be a valid version")
}

/// 从 `clang --version` 输出提取版本。
///
/// 识别 `clang version X.Y.Z` 行与 `Apple clang version X.Y.Z` 行
///（LLVM 官方 / Apple 分发输出格式）。
pub fn version_from_clang_output(text: &str) -> Option<ClangVersion> {
    for line in text.lines() {
        let line = line.trim();
        let rest = line
            .strip_prefix("clang version ")
            .or_else(|| line.strip_prefix("Apple clang version "))?;
        // `18.1.8 (tags/RELEASE_181/final)` → 取首个空白前字段
        let token = rest.split_whitespace().next()?;
        if let Ok(v) = token.parse::<ClangVersion>() {
            return Some(v);
        }
    }
    None
}

/// 校验 clang 是否达到支持基线；返回错误信息（达标返回 `None`）。
pub fn ensure_clang_min_version(v: ClangVersion) -> Option<String> {
    let min = min_supported_version();
    if v >= min {
        None
    } else {
        Some(format!(
            "clang {v} is below the supported baseline {min} (LLVM 22+ required; \
             `arc toolchain install llvm` can provision a matching clang)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_various_clang_outputs() {
        assert_eq!(
            version_from_clang_output(
                "Apple clang version 16.0.0 (clang-1600.0.26.6)\nTarget: ..."
            ),
            Some(ClangVersion::new(16, 0, 0))
        );
        assert_eq!(
            version_from_clang_output(
                "clang version 22.1.8 (tags/RELEASE_221/final)\nTarget: x86_64"
            ),
            Some(ClangVersion::new(22, 1, 8))
        );
        assert_eq!(
            version_from_clang_output("clang version 15.0.7\n"),
            Some(ClangVersion::new(15, 0, 7))
        );
        assert_eq!(version_from_clang_output("not a clang\n"), None);
        assert_eq!(version_from_clang_output(""), None);
    }

    #[test]
    fn from_str_accepts_partial_versions() {
        assert_eq!(
            "22".parse::<ClangVersion>().unwrap(),
            ClangVersion::new(22, 0, 0)
        );
        assert_eq!(
            "22.1".parse::<ClangVersion>().unwrap(),
            ClangVersion::new(22, 1, 0)
        );
        assert_eq!("v22.1.8".parse::<ClangVersion>().unwrap_err().0, "v22.1.8");
    }

    #[test]
    fn version_comparison() {
        assert!(ClangVersion::new(22, 0, 0) > ClangVersion::new(15, 0, 0));
        assert!(ClangVersion::new(22, 1, 0) > ClangVersion::new(22, 0, 9));
        assert_eq!(ClangVersion::new(22, 0, 0), "22.0.0".parse().unwrap());
    }

    #[test]
    fn floor_enforcement() {
        assert_eq!(ensure_clang_min_version(ClangVersion::new(22, 0, 0)), None);
        assert_eq!(ensure_clang_min_version(ClangVersion::new(22, 1, 8)), None);
        assert!(ensure_clang_min_version(ClangVersion::new(21, 6, 0)).is_some());
        assert!(ensure_clang_min_version(ClangVersion::new(15, 0, 0)).is_some());
    }
}
