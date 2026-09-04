//! RFC 027: .resx → 强类型资源访问器源码生成器（ResX CodeGen）。
//!
//! 本模块位于 `crates/codegen/`（编译器工具链层）：编译期把 `.resx` XML
//! 解析后**直接生成 Arc 访问器源码**，经管线注入编译单元，随常规编译
//! 流水线下沉为字面量/静态属性——运行时零解析、零哈希、零 ABI 调用。
//!
//! ## 设计（对标 .NET ResXFileCodeGenerator，AOT 原生化）
//!
//! `Messages.resx` + `Messages.zh-CN.resx` 生成：
//!
//! ```as
//! public class Messages {
//!     public static string Greeting {
//!         get {
//!             string c = CultureInfo.CurrentUICulture.Name;
//!             if (c == "zh-CN" || (c.Length > 6 && c.Substring(0, 6) == "zh-CN-")) {
//!                 return "你好";
//!             }
//!             return "Hello";          // neutral 兜底
//!         }
//!     }
//! }
//! ```
//!
//! 文化回退用 BCP-47 前缀链（`zh-Hans-CN` 命中 `zh-Hans` 文件）静态展开为
//! if 链——可用文化集合编译期已知，无需运行时文化数据表。
//!
//! ## 生成纪律（硬约束）
//!
//! - **neutral 必须完备**：文化文件中的 key 若 neutral 缺失 → 编译错误
//!   （R054020，对齐 .NET Designer 行为，禁半拉子资源面）。
//! - **key 必须可净化为合法 PascalCase 标识符**（R054022）。
//! - **byte[] 后置**：数组字面量语法未定案前诚实报错（R054023），禁降级。
//!
//! ## Architecture red line
//!
//! 本模块零资源语义理解之外的职责：每条目仅 {name, type_tag, value} 三元组。
//! 文化感知回退逻辑在生成代码（std 消费）；此处只做文本产出。

// RFC 027 类型标签——与 .resx `type` 属性一一对应（内部映射用，非 ABI）
const RES_TAG_STRING: u32 = 0;
const RES_TAG_INT: u32 = 1;
const RES_TAG_LONG: u32 = 2;
const RES_TAG_FLOAT: u32 = 3;
const RES_TAG_DOUBLE: u32 = 4;
const RES_TAG_BOOL: u32 = 5;
const RES_TAG_BYTE_ARRAY: u32 = 6;

/// A single parsed resource entry from .resx XML.
#[derive(Debug, Clone)]
pub struct ResEntry {
    pub name: String,
    pub type_tag: u32,
    pub value_bytes: Vec<u8>,
}

/// A parsed .resx file's entry set.
#[derive(Debug, Clone)]
pub struct ResResourceSet {
    pub entries: Vec<ResEntry>,
}

/// A resource base name group: neutral set + per-culture sets.
///
/// `Messages.resx` → neutral; `Messages.zh-CN.resx` → culture "zh-CN".
/// `cultures` sorted by culture name specificity (longer first).
#[derive(Debug, Clone)]
pub struct ResxGroup {
    /// 资源基名（如 "Messages"，含多点基名的完整形式）。
    pub base_name: String,
    /// neutral（无文化后缀）资源集。
    pub neutral: ResResourceSet,
    /// (文化名, 资源集) 列表，按文化名长度降序（更具体优先）排列。
    pub cultures: Vec<(String, ResResourceSet)>,
}

impl ResxGroup {
    /// 生成的访问器类名 = 基名最后一个 `.` 段（"MyApp.Messages" → "Messages"）。
    pub fn class_name(&self) -> &str {
        self.base_name.rsplit('.').next().unwrap_or(&self.base_name)
    }
}

/// Error types for resx parsing and accessor generation.
#[derive(Debug)]
pub enum ResxError {
    /// XML parse failure
    Parse(String),
    /// Unknown or unsupported type attribute
    UnsupportedType(String),
    /// Expected XML element/attribute not found
    MissingElement(String),
    /// Accessor generation failure (R05402x)
    Generate(String),
}

impl std::fmt::Display for ResxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResxError::Parse(msg) => write!(f, "R054001: XML parse error: {}", msg),
            ResxError::UnsupportedType(t) => write!(f, "R054002: unsupported resx type '{}'", t),
            ResxError::MissingElement(e) => write!(f, "R054013: missing element: {}", e),
            ResxError::Generate(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ResxError {}

/// Parse a .resx XML string into a ResResourceSet.
///
/// Supports the subset of .resx defined in RFC 027 §2.2:
///   - `<root>` / `<resheader>` / `<data name="key">` / `<value>` / `<value type="...">`
///   - Primitive types: string (default), int, long, float, double, bool
///   - byte[] parses (base64), but accessor generation defers it (R054023)
///
/// Does NOT support:
///   - `ResXFileRef` (external file references) — RFC 027 §9 excludes
///   - `ResXDataNode` (typed nodes) — RFC 027 §9 excludes
///   - `mimetype` attribute on `<data>` — RFC 027 §9 excludes
pub fn parse_resx(xml: &str) -> Result<ResResourceSet, ResxError> {
    let mut entries: Vec<ResEntry> = Vec::new();

    // Minimal XML parser for resx subset (no external crate dependency).
    // We scan for `<data name="key">...</data>` blocks.
    let mut pos = 0;
    let bytes = xml.as_bytes();

    while let Some(data_start) = find_tag_start(bytes, pos, b"data") {
        // Extract name attribute
        let name = extract_attr(bytes, data_start, b"name")
            .ok_or(ResxError::MissingElement("data/@name".into()))?;

        // Find `>` that closes the <data ...> opening tag
        let tag_close = find_byte(bytes, data_start, b'>')
            .ok_or(ResxError::Parse("unclosed <data> tag".into()))?;

        // Find closing </data>
        let data_end = find_closing_tag(bytes, tag_close + 1, b"data")
            .ok_or(ResxError::Parse("unclosed </data> tag".into()))?;

        let inner = &bytes[tag_close + 1..data_end];

        // Extract <value> content
        let value_start = find_tag_start(inner, 0, b"value").ok_or(ResxError::MissingElement(
            format!("data[@name='{}']/value", name),
        ))?;

        // Check for type attribute
        let type_attr = extract_attr(inner, value_start, b"type");

        let val_close = find_byte(inner, value_start, b'>').ok_or(ResxError::Parse(format!(
            "unclosed <value> for key '{}'",
            name
        )))?;

        // </value>
        let val_end = find_closing_tag(inner, val_close + 1, b"value").ok_or(ResxError::Parse(
            format!("unclosed </value> for key '{}'", name),
        ))?;

        let value_text = std::str::from_utf8(&inner[val_close + 1..val_end])
            .map_err(|_| ResxError::Parse(format!("invalid UTF-8 in value for key '{}'", name)))?;

        let (type_tag, value_bytes) = resolve_type(type_attr.as_deref(), value_text)?;

        entries.push(ResEntry {
            name,
            type_tag,
            value_bytes,
        });

        pos = data_end + 7; // skip past "</data>"
    }

    if entries.is_empty() {
        return Err(ResxError::Parse("no <data> entries found in .resx".into()));
    }

    Ok(ResResourceSet { entries })
}

// ─────────────────────────────────────────────────────────────────────────────
// 文件名解析：`<Base>.<Culture>.resx` → (base, culture?)
// ─────────────────────────────────────────────────────────────────────────────

/// 判定 stem 最后一个 `.` 段是否为 BCP-47 风格文化名
/// （`^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$`，如 `zh` / `zh-CN` / `zh-Hans`）。
fn is_culture_segment(seg: &str) -> bool {
    let mut parts = seg.split('-');
    let primary = parts.next().unwrap_or("");
    if !(primary.len() == 2 || primary.len() == 3)
        || !primary.chars().all(|c| c.is_ascii_lowercase())
    {
        return false;
    }
    parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// 拆分 .resx 文件名（不含扩展名）：`("Messages.zh-CN", …)` → `("Messages", Some("zh-CN"))`；
/// `("My.App.Messages", …)` → `("My.App.Messages", None)`（末段不匹配文化模式视为基名一部分）。
pub fn split_resx_stem(stem: &str) -> (String, Option<String>) {
    match stem.rsplit_once('.') {
        Some((base, last)) if is_culture_segment(last) => {
            (base.to_string(), Some(last.to_string()))
        }
        _ => (stem.to_string(), None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 访问器源码生成
// ─────────────────────────────────────────────────────────────────────────────

/// 生成强类型资源访问器 Arc 源码。
///
/// 生成形态见模块级文档。错误（R05402x，硬错误不降级）：
/// - R054020: 文化文件中的 key 缺失于 neutral
/// - R054021: 值含不支持的控控字符
/// - R054022: key 无法净化为合法 PascalCase 标识符 / 净化后碰撞
/// - R054023: byte[] 资源（数组字面量语法定案前后置）
/// - R054024: 同一 .resx 内 key 重复
pub fn generate_accessor_source(group: &ResxGroup) -> Result<String, ResxError> {
    let class_name = group.class_name();

    // 1. neutral 键全集 + 同文件去重
    let mut neutral_keys: Vec<&ResEntry> = Vec::new();
    for e in &group.neutral.entries {
        if neutral_keys.iter().any(|k| k.name == e.name) {
            return Err(gen(
                54_024,
                &format!(
                    "duplicate key '{}' in neutral '{}.resx'",
                    e.name, group.base_name
                ),
            ));
        }
        neutral_keys.push(e);
    }

    // 2. 文化集校验：key 必须存在于 neutral；同文件去重
    for (culture, set) in &group.cultures {
        let mut seen: Vec<&str> = Vec::new();
        for e in &set.entries {
            if seen.contains(&e.name.as_str()) {
                return Err(gen(
                    54_024,
                    &format!(
                        "duplicate key '{}' in '{}.{}.resx'",
                        e.name, group.base_name, culture
                    ),
                ));
            }
            seen.push(&e.name);
            if !neutral_keys.iter().any(|k| k.name == e.name) {
                return Err(gen(54_020, &format!(
                    "key '{}' exists in '{}.{}.resx' but missing from neutral '{}.resx' (neutral must be complete)",
                    e.name, group.base_name, culture, group.base_name
                )));
            }
        }
    }

    // 3. 属性名净化 + 碰撞检查
    let mut props: Vec<(String, &ResEntry)> = Vec::new();
    for e in &neutral_keys {
        let prop = to_pascal_case_ident(&e.name)?;
        if let Some((_, other)) = props.iter().find(|(p, _)| p == &prop) {
            return Err(gen(
                54_022,
                &format!(
                    "keys '{}' and '{}' both sanitize to property '{}' in '{}.resx'",
                    other.name, e.name, prop, group.base_name
                ),
            ));
        }
        props.push((prop, e));
    }

    // 4. 类型标签 → Arc 返回类型（byte[] 先行拒绝，带 key 上下文）
    let arc_type = |tag: u32| -> Result<&'static str, ResxError> {
        match tag {
            RES_TAG_STRING => Ok("string"),
            RES_TAG_INT => Ok("int"),
            RES_TAG_LONG => Ok("long"),
            RES_TAG_FLOAT => Ok("float"),
            RES_TAG_DOUBLE => Ok("double"),
            RES_TAG_BOOL => Ok("bool"),
            RES_TAG_BYTE_ARRAY => Err(gen(
                54_023,
                "byte[] resources are deferred until array literal syntax lands",
            )),
            _ => Err(gen(54_002, "unknown type tag")),
        }
    };

    // 5. 产出源码
    let mut src = String::with_capacity(4096);
    src.push_str("// 此文件由 arc 编译器生成（RFC 027 ResX CodeGen）—— 请勿手工修改。\n");
    src.push_str(&format!("// 源: {}.resx", group.base_name));
    for (culture, _) in &group.cultures {
        src.push_str(&format!(", {}.{}.resx", group.base_name, culture));
    }
    src.push_str("\n\n");
    src.push_str("using Arc.Globalization;\n\n");
    src.push_str(&format!(
        "/// <summary>强类型资源访问器，生成自 {}.resx。</summary>\n",
        group.base_name
    ));
    src.push_str(&format!("public class {} {{\n", class_name));
    src.push_str(&format!("    private {}() {{\n    }}\n\n", class_name));

    for (prop, entry) in &props {
        if entry.type_tag == RES_TAG_BYTE_ARRAY {
            return Err(gen(
                54_023,
                &format!(
                "byte[] resource '{}' in '{}.resx' is deferred until array literal syntax lands",
                entry.name, group.base_name
            ),
            ));
        }
        let ret_ty = arc_type(entry.type_tag)?;

        src.push_str(&format!(
            "    /// <summary>资源 \"{}\"。</summary>\n",
            entry.name
        ));
        src.push_str(&format!("    public static {} {} {{\n", ret_ty, prop));
        src.push_str("        get {\n");

        // 前缀链：按文化特异性降序（长名优先）；该 key 无任何文化变体 → 直通 neutral
        let mut cultures: Vec<&(String, ResResourceSet)> = group.cultures.iter().collect();
        cultures.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));

        let mut chain = String::new();
        for (culture, set) in &cultures {
            if let Some(e) = set.entries.iter().find(|e| e.name == entry.name) {
                let prefix_len = culture.len() + 1; // 含尾 '-'
                chain.push_str(&format!(
                    "            if (c == \"{}\" || (c.Length > {} && c.Substring(0, {}) == \"{}-\")) {{\n",
                    escape_literal(culture)?, prefix_len, prefix_len, culture
                ));
                chain.push_str(&format!(
                    "                return {};\n",
                    format_literal(e.type_tag, &e.value_bytes)?
                ));
                chain.push_str("            }\n");
            }
        }

        if chain.is_empty() {
            // 无文化变体：直接返回 neutral 值（编译器常量直通）
            src.push_str(&format!(
                "            return {};\n",
                format_literal(entry.type_tag, &entry.value_bytes)?
            ));
        } else {
            src.push_str("            string c = CultureInfo.CurrentUICulture.Name;\n");
            src.push_str(&chain);
            src.push_str(&format!(
                "            return {};\n",
                format_literal(entry.type_tag, &entry.value_bytes)?
            ));
        }

        src.push_str("        }\n");
        src.push_str("    }\n\n");
    }

    src.push_str("}\n");
    Ok(src)
}

// ── 生成辅助 ──

fn gen(code: u32, msg: &str) -> ResxError {
    ResxError::Generate(format!("R{:06}: {}", code, msg))
}

/// key → PascalCase 标识符。分段符 `.` / `-` / `_`；每段 `[A-Za-z0-9]+`，
/// 首段必须字母/下划线开头（"full_name" → "FullName"，"my.key" → "MyKey"）。
fn to_pascal_case_ident(key: &str) -> Result<String, ResxError> {
    let mut ident = String::new();
    for (i, seg) in key.split(['.', '-', '_']).enumerate() {
        if seg.is_empty() {
            continue;
        }
        if !seg.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(gen(
                54_022,
                &format!(
                    "resource key '{}' contains non-identifier characters (segment '{}')",
                    key, seg
                ),
            ));
        }
        let first = seg.chars().next().unwrap();
        if i == 0 && first.is_ascii_digit() {
            return Err(gen(
                54_022,
                &format!(
                    "resource key '{}' must start with a letter or underscore",
                    key
                ),
            ));
        }
        ident.extend(first.to_uppercase());
        ident.push_str(&seg[1..]);
    }
    if ident.is_empty() {
        return Err(gen(
            54_022,
            &format!("resource key '{}' has no identifier segments", key),
        ));
    }
    Ok(ident)
}

/// 值字节 → Arc 字面量。
fn format_literal(type_tag: u32, value_bytes: &[u8]) -> Result<String, ResxError> {
    match type_tag {
        RES_TAG_STRING => {
            let s = std::str::from_utf8(value_bytes)
                .map_err(|_| gen(54_021, "invalid UTF-8 in string resource"))?;
            Ok(format!("\"{}\"", escape_literal(s)?))
        }
        RES_TAG_INT => {
            let v = decode_i32(value_bytes)?;
            Ok(v.to_string())
        }
        RES_TAG_LONG => {
            let v = i64::from_le_bytes(
                value_bytes
                    .try_into()
                    .map_err(|_| gen(54_021, "bad long bytes"))?,
            );
            Ok(v.to_string())
        }
        RES_TAG_FLOAT => {
            let v = f32::from_le_bytes(
                value_bytes
                    .try_into()
                    .map_err(|_| gen(54_021, "bad float bytes"))?,
            );
            Ok(ensure_float_suffix(&format!("{}", v)))
        }
        RES_TAG_DOUBLE => {
            let v = f64::from_le_bytes(
                value_bytes
                    .try_into()
                    .map_err(|_| gen(54_021, "bad double bytes"))?,
            );
            Ok(ensure_float_suffix(&format!("{}", v)))
        }
        RES_TAG_BOOL => {
            if value_bytes.first() == Some(&1) {
                Ok("true".into())
            } else {
                Ok("false".into())
            }
        }
        RES_TAG_BYTE_ARRAY => Err(gen(
            54_023,
            "byte[] resources are deferred until array literal syntax lands",
        )),
        _ => Err(gen(54_002, "unknown type tag")),
    }
}

fn decode_i32(value_bytes: &[u8]) -> Result<i32, ResxError> {
    let bytes: [u8; 4] = value_bytes
        .try_into()
        .map_err(|_| gen(54_021, "bad int bytes"))?;
    Ok(i32::from_le_bytes(bytes))
}

/// 浮点字面量必须含 `.`/`e`，否则整数值会被推断为整型（"3" → "3.0"）。
fn ensure_float_suffix(s: &str) -> String {
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s.to_string()
    } else {
        format!("{}.0", s)
    }
}

/// Arc 字符串字面量转义；不支持的控制字符 → R054021（诚实报错）。
fn escape_literal(s: &str) -> Result<String, ResxError> {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                return Err(gen(
                    54_021,
                    &format!(
                        "unsupported control character U+{:04X} in resource value",
                        c as u32
                    ),
                ))
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// XML helpers（既有实现，保持不变）
// ─────────────────────────────────────────────────────────────────────────────

fn find_byte(data: &[u8], start: usize, needle: u8) -> Option<usize> {
    data[start..]
        .iter()
        .position(|&b| b == needle)
        .map(|i| start + i)
}

fn find_tag_start(data: &[u8], start: usize, tag: &[u8]) -> Option<usize> {
    let mut pos = start;
    while let Some(lt) = find_byte(data, pos, b'<') {
        let after_lt = &data[lt + 1..];
        // May start with '/'
        let tag_start = if after_lt.first() == Some(&b'/') {
            lt + 2
        } else {
            lt + 1
        };
        if data[tag_start..].starts_with(tag) {
            // Make sure the next byte is a tag boundary (space, >, /, or end)
            let next = data.get(tag_start + tag.len()).copied().unwrap_or(b'>');
            if next == b' ' || next == b'>' || next == b'/' || next == b'\r' || next == b'\n' {
                return Some(lt);
            }
        }
        pos = lt + 1;
    }
    None
}

fn find_closing_tag(data: &[u8], start: usize, tag: &[u8]) -> Option<usize> {
    // Simple scan for </tag>
    let mut pos = start;
    while let Some(lt) = find_byte(data, pos, b'<') {
        if data.len() > lt + 2 && data[lt + 1] == b'/' && data[lt + 2..].starts_with(tag) {
            let expected = lt + 2 + tag.len();
            if expected < data.len() && data[expected] == b'>' {
                return Some(lt);
            }
        }
        pos = lt + 1;
    }
    None
}

fn extract_attr(data: &[u8], tag_start: usize, attr_name: &[u8]) -> Option<String> {
    // Find the attr_name="..." within the opening tag
    let tag_end = find_byte(data, tag_start, b'>')?;
    let tag_content = &data[tag_start..tag_end];

    let search = [attr_name, b"=\""].concat();
    let pos = tag_content
        .windows(search.len())
        .position(|w| w == search)?;

    let value_start = pos + search.len();
    let value_end = find_byte(tag_content, value_start, b'"')?;

    let raw = &tag_content[value_start..value_end];
    // Resolve XML entities
    Some(xml_unescape(raw))
}

fn xml_unescape(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn resolve_type(type_attr: Option<&str>, value_text: &str) -> Result<(u32, Vec<u8>), ResxError> {
    let raw = type_attr.map(|t| t.trim()).unwrap_or("");
    if raw.is_empty() {
        // Default: string — XML-unescape the value content
        return Ok((
            RES_TAG_STRING,
            xml_unescape(value_text.as_bytes()).as_bytes().to_vec(),
        ));
    }

    // Map .NET type names to Arc type tags
    match raw {
        "System.Int32" | "System.Int32, mscorlib" | "int" => {
            let v: i32 = value_text
                .trim()
                .parse()
                .map_err(|_| ResxError::Parse(format!("invalid int value: '{}'", value_text)))?;
            Ok((RES_TAG_INT, v.to_le_bytes().to_vec()))
        }
        "System.Int64" | "System.Int64, mscorlib" | "long" => {
            let v: i64 = value_text
                .trim()
                .parse()
                .map_err(|_| ResxError::Parse(format!("invalid long value: '{}'", value_text)))?;
            Ok((RES_TAG_LONG, v.to_le_bytes().to_vec()))
        }
        "System.Single" | "System.Single, mscorlib" | "float" => {
            let v: f32 = value_text
                .trim()
                .parse()
                .map_err(|_| ResxError::Parse(format!("invalid float value: '{}'", value_text)))?;
            Ok((RES_TAG_FLOAT, v.to_le_bytes().to_vec()))
        }
        "System.Double" | "System.Double, mscorlib" | "double" => {
            let v: f64 = value_text
                .trim()
                .parse()
                .map_err(|_| ResxError::Parse(format!("invalid double value: '{}'", value_text)))?;
            Ok((RES_TAG_DOUBLE, v.to_le_bytes().to_vec()))
        }
        "System.Boolean" | "System.Boolean, mscorlib" | "bool" => {
            let v: u8 = match value_text.trim().to_lowercase().as_str() {
                "true" => 1,
                "false" => 0,
                _ => {
                    return Err(ResxError::Parse(format!(
                        "invalid bool value: '{}'",
                        value_text
                    )));
                }
            };
            Ok((RES_TAG_BOOL, vec![v]))
        }
        "System.Byte[]" | "System.Byte[], mscorlib" => {
            // Base64-decode the value
            let decoded = base64_decode(value_text.trim()).map_err(|_| {
                ResxError::Parse(format!("invalid base64 byte[]: '{}'", value_text))
            })?;
            Ok((RES_TAG_BYTE_ARRAY, decoded))
        }
        other => Err(ResxError::UnsupportedType(other.to_string())),
    }
}

/// Minimal base64 decoder (no external crate dependency).
fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    let chars: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ' && b != b'=')
        .collect();

    if chars.is_empty() {
        return Ok(Vec::new());
    }

    let decode_char = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    let mut out = Vec::with_capacity(chars.len() * 3 / 4 + 1);
    let mut i = 0;
    while i < chars.len() {
        let v0 = decode_char(chars[i]).ok_or(())?;
        let v1 = decode_char(*chars.get(i + 1).unwrap_or(&b'A')).ok_or(())?;

        out.push((v0 << 2) | (v1 >> 4));

        if i + 2 < chars.len() {
            let v2 = decode_char(chars[i + 2]).ok_or(())?;
            out.push((v1 << 4) | (v2 >> 2));
            if i + 3 < chars.len() {
                let v3 = decode_char(chars[i + 3]).ok_or(())?;
                out.push((v2 << 6) | v3);
            }
        }

        i += 4;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML_NEUTRAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
  <data name="greeting" xml:space="preserve"><value>Hello, World!</value></data>
  <data name="answer" xml:space="preserve"><value type="System.Int32">42</value></data>
  <data name="ratio" xml:space="preserve"><value type="System.Double">3.0</value></data>
  <data name="enabled" xml:space="preserve"><value type="System.Boolean">true</value></data>
</root>"#;

    fn group_with_cultures(xml_neutral: &str, cultures: Vec<(&str, &str)>) -> ResxGroup {
        ResxGroup {
            base_name: "Messages".into(),
            neutral: parse_resx(xml_neutral).unwrap(),
            cultures: cultures
                .into_iter()
                .map(|(c, xml)| (c.to_string(), parse_resx(xml).unwrap()))
                .collect(),
        }
    }

    #[test]
    fn test_parse_simple_resx() {
        let set = parse_resx(XML_NEUTRAL).unwrap();
        assert_eq!(set.entries.len(), 4);

        let e0 = &set.entries[0];
        assert_eq!(e0.name, "greeting");
        assert_eq!(e0.type_tag, RES_TAG_STRING);
        assert_eq!(e0.value_bytes, b"Hello, World!");

        let e1 = &set.entries[1];
        assert_eq!(e1.type_tag, RES_TAG_INT);
        assert_eq!(decode_i32(&e1.value_bytes).unwrap(), 42);
    }

    #[test]
    fn test_parse_resx_with_comment() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
  <data name="hello" xml:space="preserve"><value>world</value></data>
</root>"#;
        let set = parse_resx(xml).unwrap();
        assert_eq!(set.entries.len(), 1);
    }

    #[test]
    fn test_parse_xml_entities() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <data name="a&amp;b" xml:space="preserve">
    <value>value with &lt;tags&gt; &amp; &apos;quotes&apos;</value>
  </data>
</root>"#;
        let set = parse_resx(xml).unwrap();
        assert_eq!(set.entries[0].name, "a&b");
        assert_eq!(set.entries[0].value_bytes, b"value with <tags> & 'quotes'");
    }

    #[test]
    fn test_parse_empty_resx_fails() {
        let xml = r#"<root>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
</root>"#;
        let err = parse_resx(xml).unwrap_err();
        assert!(format!("{err}").contains("no <data> entries"));
    }

    #[test]
    fn test_parse_unsupported_type() {
        let xml = r#"<root>
  <data name="x" xml:space="preserve"><value type="System.Decimal">123.45</value></data>
</root>"#;
        let err = parse_resx(xml).unwrap_err();
        assert!(format!("{err}").contains("R054002"));
    }

    // ── 文件名拆分 ──

    #[test]
    fn test_split_resx_stem() {
        assert_eq!(split_resx_stem("Messages"), ("Messages".into(), None));
        assert_eq!(
            split_resx_stem("Messages.zh-CN"),
            ("Messages".into(), Some("zh-CN".into()))
        );
        assert_eq!(
            split_resx_stem("Messages.zh-Hans"),
            ("Messages".into(), Some("zh-Hans".into()))
        );
        assert_eq!(
            split_resx_stem("My.App.Messages"),
            ("My.App.Messages".into(), None)
        );
        assert_eq!(
            split_resx_stem("My.App.Messages.fr"),
            ("My.App.Messages".into(), Some("fr".into()))
        );
        // "strings" 长度 7 不匹配文化模式（2-3 字母主子标签）→ 视为基名
        assert_eq!(split_resx_stem("strings"), ("strings".into(), None));
    }

    // ── 访问器生成 ──

    #[test]
    fn test_generate_neutral_only() {
        let group = group_with_cultures(XML_NEUTRAL, vec![]);
        let src = generate_accessor_source(&group).unwrap();
        assert!(
            src.contains("public class Messages {"),
            "class decl:\n{src}"
        );
        assert!(
            src.contains("public static string Greeting {"),
            "string prop:\n{src}"
        );
        assert!(
            src.contains("return \"Hello, World!\";"),
            "neutral literal:\n{src}"
        );
        assert!(
            src.contains("public static int Answer {"),
            "int prop:\n{src}"
        );
        assert!(src.contains("return 42;"), "int literal:\n{src}");
        assert!(
            src.contains("public static double Ratio {"),
            "double prop:\n{src}"
        );
        assert!(
            src.contains("return 3.0;"),
            "double literal keeps .0:\n{src}"
        );
        assert!(
            src.contains("public static bool Enabled {"),
            "bool prop:\n{src}"
        );
        assert!(src.contains("return true;"), "bool literal:\n{src}");
        // 无文化文件 → 不生成 CurrentUICulture 读取
        assert!(
            !src.contains("CurrentUICulture"),
            "no culture chain:\n{src}"
        );
        assert!(src.contains("private Messages() {"), "private ctor:\n{src}");
    }

    #[test]
    fn test_generate_with_culture_chain() {
        let zh_cn = r#"<root>
  <data name="greeting" xml:space="preserve"><value>你好，Arc！</value></data>
</root>"#;
        let group = group_with_cultures(XML_NEUTRAL, vec![("zh-CN", zh_cn)]);
        let src = generate_accessor_source(&group).unwrap();

        assert!(
            src.contains("string c = CultureInfo.CurrentUICulture.Name;"),
            "culture read:\n{src}"
        );
        assert!(
            src.contains(
                "if (c == \"zh-CN\" || (c.Length > 6 && c.Substring(0, 6) == \"zh-CN-\")) {"
            ),
            "prefix chain:\n{src}"
        );
        assert!(
            src.contains("return \"你好，Arc！\";"),
            "culture literal:\n{src}"
        );
        // Greeting 有文化变体 → 链存在；Answer 无 → 直通 neutral（取 Answer 块自身，不含后续属性）
        let answer_block = src
            .split("public static int Answer")
            .nth(1)
            .unwrap()
            .split("public static ")
            .next()
            .unwrap();
        assert!(
            !answer_block.contains("CurrentUICulture"),
            "key without culture variant stays direct:\n{src}"
        );
    }

    #[test]
    fn test_generate_specificity_order() {
        // zh-Hans 与 zh 同时存在：更长（更具体）的 zh-Hans 先判
        let zh_hans = r#"<root><data name="greeting"><value>你好（简体）</value></data></root>"#;
        let zh = r#"<root><data name="greeting"><value>你好</value></data></root>"#;
        let group = group_with_cultures(XML_NEUTRAL, vec![("zh", zh), ("zh-Hans", zh_hans)]);
        let src = generate_accessor_source(&group).unwrap();
        let pos_hans = src.find("zh-Hans").expect("zh-Hans branch");
        let pos_zh = src
            .find("\"zh\" ||")
            .or_else(|| src.find("== \"zh\""))
            .expect("zh branch");
        assert!(
            pos_hans < pos_zh,
            "zh-Hans (more specific) checked first:\n{src}"
        );
    }

    #[test]
    fn test_key_sanitization() {
        let xml = r#"<root>
  <data name="full_name" xml:space="preserve"><value>Full Name</value></data>
  <data name="my.key" xml:space="preserve"><value>dotted</value></data>
</root>"#;
        let group = group_with_cultures(xml, vec![]);
        let src = generate_accessor_source(&group).unwrap();
        assert!(
            src.contains("public static string FullName {"),
            "snake_case → PascalCase:\n{src}"
        );
        assert!(
            src.contains("public static string MyKey {"),
            "dot key → PascalCase:\n{src}"
        );
    }

    #[test]
    fn test_generate_missing_neutral_key_errors() {
        let zh = r#"<root><data name="only_in_culture"><value>x</value></data></root>"#;
        let group = group_with_cultures(XML_NEUTRAL, vec![("zh-CN", zh)]);
        let err = generate_accessor_source(&group).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("R054020"), "expected R054020, got: {msg}");
        assert!(msg.contains("only_in_culture"));
    }

    #[test]
    fn test_generate_duplicate_key_errors() {
        // parser 允许重复键（纯解析层）；去重是生成器职责
        let dup = r#"<root>
  <data name="greeting"><value>a</value></data>
  <data name="greeting"><value>b</value></data>
</root>"#;
        let set = parse_resx(dup).unwrap();
        let group = ResxGroup {
            base_name: "Messages".into(),
            neutral: set,
            cultures: vec![],
        };
        let err = generate_accessor_source(&group).unwrap_err();
        assert!(format!("{err}").contains("R054024"), "dup key: {err}");
    }

    #[test]
    fn test_generate_sanitize_collision_errors() {
        let xml = r#"<root>
  <data name="full_name"><value>a</value></data>
  <data name="full.name"><value>b</value></data>
</root>"#;
        let group = group_with_cultures(xml, vec![]);
        let err = generate_accessor_source(&group).unwrap_err();
        assert!(format!("{err}").contains("R054022"), "collision: {err}");
    }

    #[test]
    fn test_generate_invalid_key_errors() {
        for key in ["2fast", "a b", ""] {
            let xml = format!(
                r#"<root><data name="{}"><value>x</value></data></root>"#,
                key
            );
            let set = match parse_resx(&xml) {
                Ok(s) => s,
                Err(_) => continue, // 空 name 等在解析层已拒
            };
            let group = ResxGroup {
                base_name: "M".into(),
                neutral: set,
                cultures: vec![],
            };
            let r = generate_accessor_source(&group);
            assert!(r.is_err(), "key '{key}' should be rejected");
        }
    }

    #[test]
    fn test_generate_byte_array_deferred() {
        let xml =
            r#"<root><data name="raw"><value type="System.Byte[]">AAECAwQ=</value></data></root>"#;
        let group = group_with_cultures(xml, vec![]);
        let err = generate_accessor_source(&group).unwrap_err();
        assert!(
            format!("{err}").contains("R054023"),
            "byte[] deferred: {err}"
        );
    }

    #[test]
    fn test_generate_control_char_rejected() {
        let group = ResxGroup {
            base_name: "M".into(),
            neutral: ResResourceSet {
                entries: vec![ResEntry {
                    name: "k".into(),
                    type_tag: RES_TAG_STRING,
                    value_bytes: b"bad\x01char".to_vec(),
                }],
            },
            cultures: vec![],
        };
        let err = generate_accessor_source(&group).unwrap_err();
        assert!(format!("{err}").contains("R054021"), "control char: {err}");
    }

    #[test]
    fn test_class_name_multi_dot_base() {
        let group = ResxGroup {
            base_name: "My.App.Messages".into(),
            neutral: parse_resx(r#"<root><data name="k"><value>v</value></data></root>"#).unwrap(),
            cultures: vec![],
        };
        assert_eq!(group.class_name(), "Messages");
        let src = generate_accessor_source(&group).unwrap();
        assert!(src.contains("public class Messages {"));
    }

    #[test]
    fn test_escape_literal() {
        assert_eq!(escape_literal("a\"b\\c\nd").unwrap(), "a\\\"b\\\\c\\nd");
        assert!(escape_literal("x\u{0001}y").is_err());
    }

    #[test]
    fn test_ensure_float_suffix() {
        assert_eq!(ensure_float_suffix("3"), "3.0");
        assert_eq!(ensure_float_suffix("3.14"), "3.14");
        assert_eq!(ensure_float_suffix("1e10"), "1e10");
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("AAEC").unwrap(), &[0, 1, 2]);
    }
}
