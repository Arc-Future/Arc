//! WGSL 单一权威源一致性契约（rect 管线）。
//!
//! 维护正本为 `std/UI/Core/Rendering/wgpu/rect.wgsl`；运行时消费的是
//! `std/UI/Core/Rendering/wgpu/WgpuRender.Wgsl.as` 中 `RectWgslSource()` 的内嵌
//! 字符串副本。双份存在的历史风险是静默漂移：正本改了、内嵌副本没跟，
//! 测试（`wgsl_validate` 只校验 .wgsl 文件本身）仍绿，渲染行为却停在旧
//! shader。本契约把「内嵌副本 == 正本」变成机器判定；漂移即红，
//! `UPDATE_WGSL=1 cargo test -p arc-ui --test wgsl_source_sync` 以 .wgsl
//! 为正本再生 .as 内嵌块（模式对标 `design_tokens_contract.rs` 的
//! `UPDATE_BUILTIN_THEME`）。

use std::path::PathBuf;

const WGSL_REL: &str = "std/UI/Core/Rendering/wgpu/rect.wgsl";
const AS_REL: &str = "std/UI/Core/Rendering/wgpu/WgpuRender.Wgsl.as";
const FN_SIG: &str = "private string RectWgslSource()";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn read_repo_file(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

/// 归一化行尾（checkout autocrlf 差异不应导致契约假红/假绿）。
fn normalize_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// 正本主体：rect.wgsl 去文件级头注释（首个 `struct` 行起，含尾换行）。
fn rect_wgsl_body() -> String {
    let wgsl = normalize_lf(&read_repo_file(WGSL_REL));
    let idx = wgsl
        .find("struct RectUniform")
        .unwrap_or_else(|| panic!("{WGSL_REL} missing shader body (struct RectUniform)"));
    wgsl[idx..].to_string()
}

/// .as 内嵌副本：提取 `RectWgslSource()` return 语句的全部字符串字面量并拼接。
///
/// 按字符（而非字节）扫描：字面量内含中文注释行（UTF-8 多字节），字节级
/// 处理会拆坏编码。支持的转义与再生器输出对齐：`\n` / `\\` / `\"`。
fn extract_embedded_rect_wgsl(as_src: &str) -> String {
    let fn_at = as_src
        .find(FN_SIG)
        .unwrap_or_else(|| panic!("{AS_REL} missing `{FN_SIG}`"));
    let ret_at = as_src[fn_at..]
        .find("return")
        .unwrap_or_else(|| panic!("{AS_REL}: `{FN_SIG}` has no return"))
        + fn_at;
    let scan = &as_src[ret_at..];
    let mut out = String::new();
    let mut in_lit = false;
    let mut escaped = false;
    for c in scan.chars() {
        if escaped {
            match c {
                'n' => out.push('\n'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => panic!("{AS_REL}: unsupported escape \\{other} in RectWgslSource literal"),
            }
            escaped = false;
            continue;
        }
        if in_lit {
            match c {
                '\\' => escaped = true,
                '"' => in_lit = false,
                other => out.push(other),
            }
        } else {
            match c {
                '"' => in_lit = true,
                ';' => return out, // return 语句终止
                _ => {}            // 跳过空白 / `+` / `return`
            }
        }
    }
    panic!("{AS_REL}: RectWgslSource return statement not terminated before EOF");
}

/// return 语句终止分号的偏移（字面量感知：字符串内部的 `;` 不是语句终止）。
fn return_statement_end(from_ret: &str) -> usize {
    let mut in_lit = false;
    let mut escaped = false;
    for (i, c) in from_ret.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_lit {
            match c {
                '\\' => escaped = true,
                '"' => in_lit = false,
                _ => {}
            }
        } else if c == '"' {
            in_lit = true;
        } else if c == ';' {
            return i;
        }
    }
    panic!("{AS_REL}: RectWgslSource return statement not terminated before EOF");
}

/// 以 .wgsl 正本再生 .as 内嵌块（UPDATE_WGSL=1）。
fn regenerate_embedded_block(body: &str) {
    let as_src = read_repo_file(AS_REL);
    let fn_at = as_src
        .find(FN_SIG)
        .unwrap_or_else(|| panic!("{AS_REL} missing `{FN_SIG}`"));
    let ret_at = as_src[fn_at..].find("return").unwrap() + fn_at;
    let semicolon_at = ret_at + return_statement_end(&as_src[ret_at..]);
    // 与 .as 其余部分保持同一行尾风格，避免再生引入混合 EOL
    let eol = if as_src.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let indent = " ".repeat(15); // 对齐 `return ` 之后
    let mut block = String::from("return \"");
    let lines: Vec<&str> = body.trim_end_matches('\n').split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        let escaped = line.replace('\\', "\\\\").replace('"', "\\\"");
        block.push_str(&escaped);
        block.push_str("\\n\"");
        if i + 1 < lines.len() {
            block.push_str(" +");
            block.push_str(eol);
            block.push_str(&indent);
            block.push('"');
        }
    }
    block.push(';');
    let updated = format!(
        "{}{}{}",
        &as_src[..ret_at],
        block,
        &as_src[semicolon_at + 1..]
    );
    std::fs::write(repo_root().join(AS_REL), updated).expect("rewrite WgpuRender.Wgsl.as");
}

#[test]
fn rect_wgsl_embedded_copy_in_sync() {
    let body = rect_wgsl_body();
    let as_src = read_repo_file(AS_REL);
    let embedded = extract_embedded_rect_wgsl(&as_src);
    if std::env::var("UPDATE_WGSL").is_ok() {
        if embedded != body {
            regenerate_embedded_block(&body);
        }
        return;
    }
    assert_eq!(
        embedded, body,
        "WgpuRender.Wgsl.as RectWgslSource() embedded copy drifted from {WGSL_REL}; \
         regenerate with UPDATE_WGSL=1 cargo test -p arc-ui --test wgsl_source_sync"
    );
}

#[test]
fn embedded_rect_wgsl_compiles() {
    // 运行时消费的正是内嵌字符串；用 naga 在无 GPU 环境兜住语法回归
    //（wgsl_validate 只校验 .wgsl 文件本身，本测试覆盖内嵌副本）。
    let embedded = extract_embedded_rect_wgsl(&read_repo_file(AS_REL));
    let module = naga::front::wgsl::parse_str(&embedded).unwrap_or_else(|e| {
        panic!(
            "embedded rect WGSL failed to parse:\n{}",
            e.emit_to_string(&embedded)
        )
    });
    assert!(module.entry_points.iter().any(|ep| ep.name == "vs_main"));
    assert!(module.entry_points.iter().any(|ep| ep.name == "fs_main"));
}
