//! 编译期完整性门（RFC 036 语义级裁剪闭环验证 · `arc-prune-001`）。
//!
//! ## 动机
//!
//! 语义级裁剪（`filter_reachable_mir_fns`）以可达性分析为最小边界移除不可达 MIR 函数，
//! 同时维护约 20 条 "force-keep" 例外（itable/vtable 槽、字典用户键哈希/相等、静态
//! 初始化器等）。历史上对例外集合的疏漏会导致**过度裁剪**：ARC 函数被剪除、但 IR
//! 中仍有 `call`/`load` 引用其符号 → 链接期 `undefined symbol`，或在动态库角色下
//! 运行时符号缺位。此类缺陷常需等待链接/运行才暴露，闭环验证缺失。
//!
//! ## 判据
//!
//! 本门在 `emit_module` 汇集完整 LLVM IR 文本后运行：扫描所有 `@` 前缀符号名，
//! 判定每个被引用符号是否满足以下任一条件（否则判为**未定义**）：
//!
//! 1. 模块内有 `define`（用户函数 / 默认 ctor / vtable / 全局 / 字符串字面量等）；
//! 2. 模块内有 `declare`（`rt_*` ABI、libc、native 契约、跨 `.ao` 外部符号、`@llvm` 等）；
//! 3. 命中稳定放行名单（目前仅 `@llvm.*`——LLVM intrinsic 由后端自动解析，
//!    无需也不强制文本 `declare`，枚举逐个 declare 易漏且徒增噪声）。
//!
//! ## 失败语义
//!
//! 默认开启且为**硬错误**：任一未定义符号 → `CodegenError::Completeness`，诊断码
//! `arc-prune-001`。错误携带符号清单，供 CLI 渲染，引导定位过度裁剪的 force-keep 缺口。

use std::collections::{BTreeMap, HashSet};

/// 符号解析结果：`@name → 首次出现的行号（1 基）`，用于诊断定位。
struct SymbolStats {
    /// 被 `@` 提及的全部符号（含定义/声明位点与引用位点）。
    mentioned: BTreeMap<String, u32>,
    /// 模块内以 `define` 或 `@name =` 定义的符号。
    defined: HashSet<String>,
    /// 模块内以 `declare` 声明的符号。
    declared: HashSet<String>,
}

/// 解析单行，将其中所有 `@` 符号名加入 `mentioned`，并据行首关键字归入
/// `defined` / `declared`。
fn scan_line(line: &str, line_no: u32, stats: &mut SymbolStats) {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return;
    }

    // 逐字符提取 `@` 前缀符号。LLVM 标识符：命名符或 `"..."` 引用符。
    let mut tokens: Vec<String> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'@' {
            let mut end = i + 1;
            if end < bytes.len() && bytes[end] == b'"' {
                // 引用标识符：读到下一个未转义引号。
                end += 1;
                while end < bytes.len() {
                    if bytes[end] == b'\\' {
                        end += 2;
                        continue;
                    }
                    if bytes[end] == b'"' {
                        end += 1;
                        break;
                    }
                    end += 1;
                }
            } else {
                // 命名标识符：`[A-Za-z0-9$._]`。
                while end < bytes.len() && bytes[end].is_ascii_alphanumeric()
                    || (end < bytes.len() && matches!(bytes[end], b'$' | b'_' | b'.'))
                {
                    end += 1;
                }
            }
            if end > i + 1 {
                tokens.push(line[i..end].to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }

    for t in &tokens {
        stats.mentioned.entry(t.clone()).or_insert(line_no);
    }

    if tokens.is_empty() {
        return;
    }
    if trimmed.starts_with("declare") {
        stats.declared.extend(tokens);
    } else if trimmed.starts_with("define") {
        // `define` 行首个 `@` 为被定义函数本体；其后可能含 personality 等 `@` 引用，
        // 但它们已 `declare`（归入 declared），统一并入 defined 不影响判定。
        stats.defined.extend(tokens);
    } else if trimmed.starts_with('@') {
        // 顶层全局定义（vtable / static 字段全局 / 字符串字面量 / 元数据常量等）。
        stats.defined.extend(tokens);
    }
}

/// 遍历 IR 文本，返回符号解析统计结果。
fn analyze(ir: &str) -> SymbolStats {
    let mut stats = SymbolStats {
        mentioned: BTreeMap::new(),
        defined: HashSet::new(),
        declared: HashSet::new(),
    };
    for (idx, line) in ir.lines().enumerate() {
        scan_line(line, (idx + 1) as u32, &mut stats);
    }
    stats
}

/// 判定符号是否放行（`@llvm.*` intrinsic 由 LLVM 后端自动解析，无需文本 declare）。
fn is_allowlisted(name: &str) -> bool {
    name.starts_with("@llvm.")
}

/// 运行编译期完整性门。任一被引用符号既未定义、未声明、也不在放行名单 → 报错。
///
/// 返回 `Err(CodegenError::Completeness)`（携带 `arc-prune-001` 诊断码），
/// 附带未定义符号清单及其首次引用行号，引导定位 reachability 过度裁剪缺口。
pub fn check_ir_complete(ir: &str) -> Result<(), crate::CodegenError> {
    let missing = check_ir_complete_missing(ir);
    if missing.is_empty() {
        return Ok(());
    }

    let preview = missing
        .iter()
        .map(|(name, line)| format!("{name}@{line}"))
        .collect::<Vec<_>>();
    let preview = if preview.len() > 10 {
        let mut v = preview;
        v.truncate(10);
        format!("{}（等 {} 个）", v.join(", "), preview_len_hint(&missing))
    } else {
        preview.join(", ")
    };

    Err(crate::CodegenError::Completeness(format!(
        "arc-prune-001: 发射出的 IR 引用了 {} 个既未定义也未声明的符号，\
         疑似语义级裁剪（reachability）过度裁剪——请核查 `filter_reachable_mir_fns` \
         的 force-keep 例外：{preview}",
        missing.len()
    )))
}

fn preview_len_hint(missing: &[(String, u32)]) -> usize {
    missing.len()
}

/// 完整性分析（返回 bare 未定义符号名单，供调用方做 stub 补发闭环）。
/// 名单已按首次引用行号排序；`@` 前缀已剥离。
pub fn check_ir_complete_missing(ir: &str) -> Vec<(String, u32)> {
    let stats = analyze(ir);
    let mut undefined: Vec<(String, u32)> = stats
        .mentioned
        .iter()
        .filter(|(name, _)| {
            !is_allowlisted(name)
                && !stats.defined.contains(*name)
                && !stats.declared.contains(*name)
        })
        .map(|(name, line)| (name.trim_start_matches('@').to_string(), *line))
        .collect();
    undefined.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    undefined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat_refs(ir: &str) -> HashSet<String> {
        analyze(ir).mentioned.into_keys().collect()
    }

    #[test]
    fn ok_when_all_resolved() {
        let ir = "\
declare void @rt_env_init(i32, ptr)\n\
@__arc_file = private constant [2 x i8] c\"x\\00\"\n\
define void @main() {\nentry:\n  call void @rt_env_init(i32 0, ptr @__arc_file)\n\
  call double @llvm.sqrt.f64(double 1.0)\n  ret void\n}\n";
        assert!(check_ir_complete(ir).is_ok());
    }

    #[test]
    fn llvm_intrinsics_are_allowlisted_even_without_declare() {
        let ir = "\
define double @f(double %x) {\nentry:\n  %r = call double @llvm.memset.p0.i64(double %x)\n  ret double %r\n}\n";
        // 未 declare 的 `@llvm.*` 应收敛为放行（无报错）。
        let stats = analyze(ir);
        assert!(!stats.declared.contains("@llvm.memset.p0.i64"));
        assert!(check_ir_complete(ir).is_ok());
    }

    #[test]
    fn reports_pruned_symbol_with_location() {
        let ir = "\
define void @caller() {\nentry:\n  call void @pruned_fn()\n  ret void\n}\n";
        let err = check_ir_complete(ir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("arc-prune-001"), "unexpected msg: {msg}");
        assert!(msg.contains("pruned_fn@3"), "unexpected msg: {msg}");
    }

    #[test]
    fn distinguishes_define_declare_from_reference() {
        let ir = "\
declare void @declared_fn(void)\n\
define void @defined_fn() {\nentry:\n  ret void\n}\n\
define void @uses() {\nentry:\n  call void @declared_fn()\n  call void @defined_fn()\n  ret void\n}\n";
        assert!(check_ir_complete(ir).is_ok());
    }

    #[test]
    fn quoted_identifier_is_parsed() {
        let ir = "\
declare void @\"weird name\"(void)\n\
define void @f() {\nentry:\n  call void @\"weird name\"()\n  ret void\n}\n";
        assert!(check_ir_complete(ir).is_ok());
        let refs = stat_refs(ir);
        assert!(refs.contains("@\"weird name\""));
    }
}
