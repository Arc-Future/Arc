//! Target triple parsing, family classification, and build gates.
//!
//! Cross-platform matrix and maturity gates: `docs/rfc/031-compiler-cli.md` (WASM gate).

/// Target family for the unified platform matrix (RFC 037 §D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFamily {
    DesktopWindows,
    DesktopLinux,
    DesktopMac,
    Ohos,
    WebAssembly,
    Wasi,
    Unknown,
}

impl TargetFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DesktopWindows => "desktop-windows",
            Self::DesktopLinux => "desktop-linux",
            Self::DesktopMac => "desktop-mac",
            Self::Ohos => "ohos",
            Self::WebAssembly => "webassembly",
            Self::Wasi => "wasi",
            Self::Unknown => "unknown",
        }
    }

    /// Whether `arc build` may proceed for this family (RFC 037 M-W1a gate).
    pub fn is_build_supported(self) -> bool {
        !matches!(self, Self::WebAssembly | Self::Wasi)
    }
}

/// Known target triple for cross-compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTriple {
    pub triple: String,
}

impl TargetTriple {
    /// Host triple for the machine running the compiler.
    pub fn host() -> Self {
        Self {
            triple: host_triple().to_string(),
        }
    }

    /// Expand CLI aliases (`web`, `ohos`) to canonical triples (RFC 037 §D7).
    pub fn expand_alias(s: &str) -> String {
        match s.trim() {
            "web" => "wasm32-unknown-unknown".into(),
            "ohos" => "aarch64-unknown-linux-ohos".into(),
            other => other.to_string(),
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let triple = Self::expand_alias(s);
        if triple.is_empty() {
            return Err("target triple must not be empty".into());
        }
        let parts: Vec<&str> = triple.split('-').collect();
        if parts.len() < 3 && !is_known_wasi_short_triple(&triple) {
            return Err(format!(
                "invalid target triple '{triple}': expected arch-vendor-os form"
            ));
        }
        Ok(Self { triple })
    }

    /// Parse a triple and reject families not yet implemented for build (RFC 037 M-W1a).
    pub fn parse_for_build(s: &str) -> Result<Self, String> {
        let t = Self::parse(s)?;
        t.ensure_build_supported()?;
        Ok(t)
    }

    /// Classify this triple into a [`TargetFamily`].
    pub fn family(&self) -> TargetFamily {
        classify_triple(&self.triple)
    }

    fn ensure_build_supported(&self) -> Result<(), String> {
        let family = self.family();
        if family.is_build_supported() {
            return Ok(());
        }
        Err(build_unsupported_message(&self.triple, family))
    }

    pub fn as_str(&self) -> &str {
        &self.triple
    }
    pub fn is_wasm_family(&self) -> bool {
        matches!(
            self.family(),
            TargetFamily::WebAssembly | TargetFamily::Wasi
        )
    }

    pub fn parse_for_experimental_wasm_emit(s: &str) -> Result<Self, String> {
        let t = Self::parse(s)?;
        if !t.is_wasm_family() {
            return Err(format!(
                "--experimental-wasm-emit only applies to wasm targets (wasm32-unknown-unknown / web / wasm32-wasip*); got '{}'",
                t.as_str()
            ));
        }
        Ok(t)
    }
}

/// One line of the `--list-targets` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetListing {
    pub triple: &'static str,
    pub alias: Option<&'static str>,
    pub family: TargetFamily,
    pub build_status: &'static str,
}

/// Canonical target matrix for CLI `--list-targets` (RFC 037 §D1).
pub fn target_listings() -> &'static [TargetListing] {
    const LIST: &[TargetListing] = &[
        TargetListing {
            triple: "x86_64-pc-windows-msvc",
            alias: None,
            family: TargetFamily::DesktopWindows,
            build_status: "supported",
        },
        TargetListing {
            triple: "aarch64-pc-windows-msvc",
            alias: None,
            family: TargetFamily::DesktopWindows,
            build_status: "supported",
        },
        TargetListing {
            triple: "x86_64-unknown-linux-gnu",
            alias: None,
            family: TargetFamily::DesktopLinux,
            build_status: "supported",
        },
        TargetListing {
            triple: "aarch64-unknown-linux-gnu",
            alias: None,
            family: TargetFamily::DesktopLinux,
            build_status: "supported",
        },
        TargetListing {
            triple: "aarch64-apple-darwin",
            alias: None,
            family: TargetFamily::DesktopMac,
            build_status: "supported",
        },
        TargetListing {
            triple: "x86_64-apple-darwin",
            alias: None,
            family: TargetFamily::DesktopMac,
            build_status: "supported",
        },
        TargetListing {
            triple: "aarch64-unknown-linux-ohos",
            alias: Some("ohos"),
            family: TargetFamily::Ohos,
            build_status: "supported (native link only; HAP packaging planned)",
        },
        TargetListing {
            triple: "wasm32-unknown-unknown",
            alias: Some("web"),
            family: TargetFamily::WebAssembly,
            build_status: "未实现 (RFC 037 M-W3)",
        },
        TargetListing {
            triple: "wasm32-wasip1",
            alias: None,
            family: TargetFamily::Wasi,
            build_status: "未实现 (RFC 037)",
        },
        TargetListing {
            triple: "wasm32-wasip2",
            alias: None,
            family: TargetFamily::Wasi,
            build_status: "未实现 (RFC 037)",
        },
    ];
    LIST
}

/// Human-readable `--list-targets` output.
pub fn format_target_list() -> String {
    let mut out = String::from("Known targets (RFC 037 target matrix):\n");
    out.push_str(&format!(
        "  {:<32} {:<18} {:<6} {}\n",
        "TRIPLE", "FAMILY", "ALIAS", "BUILD"
    ));
    for entry in target_listings() {
        out.push_str(&format!(
            "  {:<32} {:<18} {:<6} {}\n",
            entry.triple,
            entry.family.as_str(),
            entry.alias.unwrap_or("-"),
            entry.build_status,
        ));
    }
    out.push_str("\nUse -r/--target <TRIPLE> or alias (web, ohos). See docs/rfc/031-compiler-cli.md (WASM gate)");
    out
}

fn is_known_wasi_short_triple(triple: &str) -> bool {
    matches!(triple, "wasm32-wasip1" | "wasm32-wasip2")
}

fn classify_triple(triple: &str) -> TargetFamily {
    let lower = triple.to_ascii_lowercase();
    if lower == "wasm32-unknown-unknown" {
        return TargetFamily::WebAssembly;
    }
    if is_known_wasi_short_triple(&lower) || lower.starts_with("wasm32-wasi") {
        return TargetFamily::Wasi;
    }
    if lower.ends_with("-linux-ohos") {
        return TargetFamily::Ohos;
    }
    if lower.contains("-pc-windows-") {
        return TargetFamily::DesktopWindows;
    }
    if lower.contains("-apple-darwin") {
        return TargetFamily::DesktopMac;
    }
    if lower.contains("-unknown-linux-")
        || lower.contains("-linux-gnu")
        || lower.contains("-linux-musl")
    {
        return TargetFamily::DesktopLinux;
    }
    TargetFamily::Unknown
}

fn build_unsupported_message(triple: &str, family: TargetFamily) -> String {
    let milestone = match family {
        TargetFamily::WebAssembly => "M-W3 浏览器垂直切片",
        TargetFamily::Wasi => "M-W3 之后 headless WASI",
        _ => "后续里程碑",
    };
    format!(
        "target '{triple}' ({}) 未实现 (RFC 037 {milestone}); \
         禁止 silent 当 native 编译；见 `arc build --list-targets`",
        family.as_str()
    )
}

fn host_triple() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_triple() {
        let t = TargetTriple::parse("x86_64-unknown-linux-gnu").unwrap();
        assert_eq!(t.as_str(), "x86_64-unknown-linux-gnu");
        assert_eq!(t.family(), TargetFamily::DesktopLinux);
    }

    #[test]
    fn parse_ohos_triple() {
        let t = TargetTriple::parse("aarch64-unknown-linux-ohos").unwrap();
        assert_eq!(t.as_str(), "aarch64-unknown-linux-ohos");
        assert_eq!(t.family(), TargetFamily::Ohos);
    }

    #[test]
    fn reject_invalid_triple() {
        assert!(TargetTriple::parse("invalid").is_err());
    }

    #[test]
    fn host_triple_is_valid() {
        let t = TargetTriple::host();
        assert!(TargetTriple::parse(t.as_str()).is_ok());
    }

    #[test]
    fn web_alias_expands_to_wasm() {
        let t = TargetTriple::parse("web").unwrap();
        assert_eq!(t.as_str(), "wasm32-unknown-unknown");
        assert_eq!(t.family(), TargetFamily::WebAssembly);
    }

    #[test]
    fn ohos_alias_expands() {
        let t = TargetTriple::parse("ohos").unwrap();
        assert_eq!(t.as_str(), "aarch64-unknown-linux-ohos");
    }

    #[test]
    fn wasm_build_hard_error() {
        let err = TargetTriple::parse_for_build("wasm32-unknown-unknown").unwrap_err();
        assert!(err.contains("未实现"));
        assert!(err.contains("wasm32-unknown-unknown"));
        assert!(err.contains("RFC 037"));
    }

    #[test]
    fn web_alias_build_hard_error() {
        let err = TargetTriple::parse_for_build("web").unwrap_err();
        assert!(err.contains("未实现"));
        assert!(err.contains("wasm32-unknown-unknown"));
    }

    #[test]
    fn wasi_short_triple_parses() {
        let t = TargetTriple::parse("wasm32-wasip1").unwrap();
        assert_eq!(t.family(), TargetFamily::Wasi);
    }

    #[test]
    fn wasi_build_hard_error() {
        let err = TargetTriple::parse_for_build("wasm32-wasip1").unwrap_err();
        assert!(err.contains("未实现"));
        assert!(err.contains("wasi"));
    }

    #[test]
    fn desktop_build_allowed() {
        TargetTriple::parse_for_build("x86_64-unknown-linux-gnu").unwrap();
        TargetTriple::parse_for_build("x86_64-pc-windows-msvc").unwrap();
    }

    #[test]
    fn list_targets_includes_wasm_row() {
        let rows: Vec<_> = target_listings()
            .iter()
            .filter(|e| e.family == TargetFamily::WebAssembly)
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].alias, Some("web"));
    }

    #[test]
    fn format_target_list_mentions_rfc084() {
        let text = format_target_list();
        assert!(text.contains("RFC 037"));
        assert!(text.contains("wasm32-unknown-unknown"));
    }
}
