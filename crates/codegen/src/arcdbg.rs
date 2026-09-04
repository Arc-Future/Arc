//! `.arcdbg` debug symbol package format (RFC 017 M2 / D5.2).
//!
//! Binary format for Arc-specific debug info, complementary to DWARF 5:
//! - `SymbolMap`: machine address → (symbol name, file, line, col)
//! - `ArcFrameInfo`: runtime-internal frame markers for ARC frame folding
//!
//! Generated post-link by invoking `llvm-nm` on the linked binary to resolve
//! symbol addresses, then merging with codegen's source-level symbol info.
//!
//! ## Binary layout
//!
//! ```text
//! [Header]            (32 bytes)
//! [SymbolMap entries] (sorted by address for binary search)
//! [ArcFrameInfo]      (runtime-internal frame markers)
//! [StringTable]       (NUL-terminated strings: symbol names + file paths)
//! ```
//!
//! ## Header (32 bytes)
//!
//! | Offset | Size | Field              |
//! |--------|------|--------------------|
//! | 0      | 4    | Magic "ADBG"       |
//! | 4      | 2    | Version (u16)      |
//! | 6      | 2    | Flags (u16)        |
//! | 8      | 4    | sym_map_off (u32)  |
//! | 12     | 4    | sym_map_count (u32)|
//! | 16     | 4    | str_tab_off (u32)  |
//! | 20     | 4    | str_tab_size (u32) |
//! | 24     | 4    | arc_frame_off (u32)|
//! | 28     | 4    | arc_frame_count(u32)|
//!
//! Note: CRC32 is deferred — the format is simple enough that correctness is
//! verified by the magic + version check. A checksum can be added later
//! without breaking compatibility (bump version).
//!
//! ## SymbolMap entry (24 bytes)
//!
//! | Offset | Size | Field              |
//! |--------|------|--------------------|
//! | 0      | 8    | address (u64)      |
//! | 8      | 4    | name_off (u32)     |
//! | 12     | 4    | file_off (u32)     |
//! | 16     | 4    | line (u32)         |
//! | 20     | 4    | col (u32)          |
//!
//! ## ArcFrameInfo entry (8 bytes)
//!
//! | Offset | Size | Field              |
//! |--------|------|--------------------|
//! | 0      | 4    | name_off (u32)     |
//! | 4      | 1    | frame_kind (u8)    |
//! | 5      | 3    | padding            |

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::CodegenError;

const MAGIC: &[u8; 4] = b"ADBG";
const VERSION: u16 = 1;
const HEADER_SIZE: u32 = 32;
const SYMBOL_ENTRY_SIZE: u32 = 24;
const ARC_FRAME_ENTRY_SIZE: u32 = 8;

/// Frame kind for ArcFrameInfo.
const FRAME_KIND_RUNTIME_INTERNAL: u8 = 0;

/// Source-level symbol info collected during codegen.
#[derive(Clone, Debug)]
pub struct SymbolInfo {
    /// Source-level name (e.g., "Main", "Rectangle::Area").
    pub source_name: String,
    /// Mangled LLVM symbol name (e.g., "main", "Rectangle_Area").
    pub mangled_name: String,
    /// Source file path.
    pub file: String,
    /// Source line (0 = unknown).
    pub line: u32,
    /// Source column (0 = unknown).
    pub col: u32,
}

/// Build the `.arcdbg` file next to the executable.
///
/// Post-link step: invokes `llvm-nm` on the linked binary to resolve symbol
/// addresses, then merges with codegen's source-level symbol info and writes
/// the `.arcdbg` binary.
///
/// Returns `Ok(())` on success. If `llvm-nm` is unavailable or fails, returns
/// an error — the caller may treat this as non-fatal (debug info is optional).
pub fn write_arcdbg(exe_path: &Path, symbols: &[SymbolInfo]) -> Result<(), CodegenError> {
    if symbols.is_empty() {
        return Ok(());
    }

    // Resolve symbol addresses via llvm-nm.
    let addr_map = nm_symbol_addresses(exe_path)?;

    // Build SymbolMap: merge source info with resolved addresses.
    let mut sym_entries: Vec<SymbolEntry> = Vec::new();
    for sym in symbols {
        if let Some(&addr) = addr_map.get(&sym.mangled_name) {
            sym_entries.push(SymbolEntry {
                address: addr,
                name: sym.source_name.clone(),
                file: sym.file.clone(),
                line: sym.line,
                col: sym.col,
            });
        }
    }
    // Sort by address for binary search at runtime.
    sym_entries.sort_by_key(|e| e.address);

    // Build ArcFrameInfo: mark all runtime-internal symbols.
    // Runtime functions follow the `rt_` prefix convention.
    let mut arc_frames: Vec<String> = Vec::new();
    for sym in symbols {
        let mangled = &sym.mangled_name;
        if (mangled.starts_with("rt_") || mangled.starts_with("__arc_"))
            && !arc_frames.contains(&sym.source_name)
        {
            arc_frames.push(sym.source_name.clone());
        }
    }
    // Also add the mangled names (runtime symbols are referenced by mangled name).
    for entry in &sym_entries {
        let name = &entry.name;
        let mangled = symbols
            .iter()
            .find(|s| s.source_name == *name)
            .map(|s| s.mangled_name.as_str())
            .unwrap_or("");
        if (mangled.starts_with("rt_") || mangled.starts_with("__arc_"))
            && !arc_frames.contains(&entry.name)
        {
            arc_frames.push(entry.name.clone());
        }
    }

    // Serialize to binary.
    let bytes = serialize(&sym_entries, &arc_frames);
    let arcdbg_path = arcdbg_path_for(exe_path);
    std::fs::write(&arcdbg_path, &bytes)
        .map_err(|e| CodegenError::Llvm(format!("write .arcdbg failed: {e}")))?;
    Ok(())
}

/// Resolve the `.arcdbg` file path for a given executable.
///
/// Placed next to the executable as `<exe>.arcdbg` so the runtime can find it
/// by appending `.arcdbg` to its own module path.
pub fn arcdbg_path_for(exe_path: &Path) -> PathBuf {
    let mut name = exe_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".into());
    name.push_str(".arcdbg");
    exe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

/// Internal: symbol entry with resolved address.
struct SymbolEntry {
    address: u64,
    name: String,
    file: String,
    line: u32,
    col: u32,
}

/// Serialize the `.arcdbg` binary format.
fn serialize(symbols: &[SymbolEntry], arc_frames: &[String]) -> Vec<u8> {
    // Build string table: all unique strings (symbol names + file paths).
    let mut str_tab = StringTableBuilder::new();
    let mut sym_records: Vec<(u64, u32, u32, u32, u32)> = Vec::new();
    for sym in symbols {
        let name_off = str_tab.intern(&sym.name);
        let file_off = if sym.file.is_empty() {
            0
        } else {
            str_tab.intern(&sym.file)
        };
        sym_records.push((sym.address, name_off, file_off, sym.line, sym.col));
    }

    let mut frame_records: Vec<(u32, u8)> = Vec::new();
    for name in arc_frames {
        let name_off = str_tab.intern(name);
        frame_records.push((name_off, FRAME_KIND_RUNTIME_INTERNAL));
    }

    let str_tab_bytes = str_tab.finish();

    // Compute offsets.
    let sym_map_off = HEADER_SIZE;
    let sym_map_count = sym_records.len() as u32;
    let sym_map_size = sym_map_count * SYMBOL_ENTRY_SIZE;
    let arc_frame_off = sym_map_off + sym_map_size;
    let arc_frame_count = frame_records.len() as u32;
    let arc_frame_size = arc_frame_count * ARC_FRAME_ENTRY_SIZE;
    let str_tab_off = arc_frame_off + arc_frame_size;
    let str_tab_size = str_tab_bytes.len() as u32;

    // Write binary.
    let mut out = Vec::with_capacity(str_tab_off as usize + str_tab_size as usize);

    // Header (32 bytes)
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&sym_map_off.to_le_bytes());
    out.extend_from_slice(&sym_map_count.to_le_bytes());
    out.extend_from_slice(&str_tab_off.to_le_bytes());
    out.extend_from_slice(&str_tab_size.to_le_bytes());
    out.extend_from_slice(&arc_frame_off.to_le_bytes());
    out.extend_from_slice(&arc_frame_count.to_le_bytes());

    // SymbolMap entries
    for (addr, name_off, file_off, line, col) in &sym_records {
        out.extend_from_slice(&addr.to_le_bytes());
        out.extend_from_slice(&name_off.to_le_bytes());
        out.extend_from_slice(&file_off.to_le_bytes());
        out.extend_from_slice(&line.to_le_bytes());
        out.extend_from_slice(&col.to_le_bytes());
    }

    // ArcFrameInfo entries
    for (name_off, kind) in &frame_records {
        out.extend_from_slice(&name_off.to_le_bytes());
        out.push(*kind);
        out.extend_from_slice(&[0u8, 0u8, 0u8]); // padding
    }

    // StringTable
    out.extend_from_slice(&str_tab_bytes);

    out
}

/// String table builder: interns strings and returns offsets.
struct StringTableBuilder {
    data: Vec<u8>,
    /// Maps string → offset. Offset 0 is reserved for "no string" (empty).
    offsets: HashMap<String, u32>,
}

impl StringTableBuilder {
    fn new() -> Self {
        // Offset 0 is a NUL byte representing "no string".
        let data = vec![0u8];
        let mut offsets = HashMap::new();
        offsets.insert(String::new(), 0);
        Self { data, offsets }
    }

    /// Intern a string, returning its offset in the string table.
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&off) = self.offsets.get(s) {
            return off;
        }
        let off = self.data.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        self.data.push(0); // NUL terminator
        self.offsets.insert(s.to_string(), off);
        off
    }

    fn finish(self) -> Vec<u8> {
        self.data
    }
}

/// Run `llvm-nm` on the binary to resolve symbol addresses.
///
/// Returns a map of mangled symbol name → address.
fn nm_symbol_addresses(exe_path: &Path) -> Result<HashMap<String, u64>, CodegenError> {
    let nm_path = llvm_nm_path();
    let output = Command::new(&nm_path)
        .args([
            "--print-address",
            "--defined-only",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| CodegenError::Llvm(format!("llvm-nm not found: {e}")))?;

    if !output.status.success() {
        // llvm-nm may fail on some platforms; treat as non-fatal.
        return Ok(HashMap::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in stdout.lines() {
        // llvm-nm output: "address T symbol_name" or "         U symbol_name"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        // Parse address (hex).
        let addr = match u64::from_str_radix(parts[0].trim_start_matches("0x"), 16) {
            Ok(a) => a,
            Err(_) => continue,
        };
        // parts[1] is the symbol type (T, t, D, d, etc.)
        // parts[2] is the symbol name
        let name = parts[2].to_string();
        map.insert(name, addr);
    }
    Ok(map)
}

/// Resolve the `llvm-nm` binary path.
///
/// On Windows, probes the same LLVM install directories as `clang_path()`.
/// On Unix, relies on PATH.
fn llvm_nm_path() -> String {
    if cfg!(windows) {
        for p in [
            r"C:\Program Files\LLVM\bin\llvm-nm.exe",
            r"C:\Program Files (x86)\LLVM\bin\llvm-nm.exe",
        ] {
            if std::path::Path::new(p).exists() {
                return p.into();
            }
        }
    }
    "llvm-nm".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_empty() {
        let bytes = serialize(&[], &[]);
        assert_eq!(&bytes[0..4], MAGIC);
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION);
    }

    #[test]
    fn serialize_with_symbols() {
        let symbols = vec![SymbolEntry {
            address: 0x401000,
            name: "Main".into(),
            file: "test.as".into(),
            line: 10,
            col: 5,
        }];
        let bytes = serialize(&symbols, &[]);
        assert_eq!(&bytes[0..4], MAGIC);

        // sym_map_count should be 1
        let sym_count = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        assert_eq!(sym_count, 1);

        // Parse symbol entry
        let sym_off = HEADER_SIZE as usize;
        let addr = u64::from_le_bytes(bytes[sym_off..sym_off + 8].try_into().unwrap());
        assert_eq!(addr, 0x401000);
    }

    #[test]
    fn arcdbg_path_for_exe() {
        let exe = Path::new("/tmp/hello.exe");
        let arcdbg = arcdbg_path_for(exe);
        assert_eq!(arcdbg.file_name().unwrap(), "hello.exe.arcdbg");
    }
}
