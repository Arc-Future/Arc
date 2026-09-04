//! DWARF 5 debug metadata emission (RFC 031 §2 / RFC 015 Phase B.2).
//!
//! Emits LLVM IR `!dbg` metadata nodes for source-level debugging:
//! - `DICompileUnit`: top-level compilation unit
//! - `DIFile`: source file
//! - `DISubprogram`: per-function metadata (name, linkage name, file, line)
//! - `DILocation`: per-instruction source location (line, col, scope)
//! - `DIBasicType` / `DISubroutineType`: function type metadata
//!
//! When debug info is enabled (`-g`), clang embeds DWARF 5 sections in the
//! object file, enabling lldb/gdb source-level debugging.
//!
//! ## Metadata node layout
//!
//! Reserved IDs (always emitted):
//! - `!0` = `DICompileUnit`
//! - `!1` = module flag: `Dwarf Version = 5`
//! - `!2` = module flag: `Debug Info Version = 3`
//! - `!3` = `llvm.ident`
//! - `!4` = `DIFile`
//! - `!5` = empty `!{}` (retainedNodes placeholder)
//!
//! Dynamic IDs (per function / per instruction):
//! - `!6+` = `DISubprogram`, `DISubroutineType`, `DIBasicType`, `DILocation`

use std::collections::HashMap;
use std::path::Path;

/// Debug metadata emitter (RFC 031 §2).
///
/// Collects metadata nodes during codegen and renders them at module end.
/// All node IDs are stable within a single compilation unit.
pub(crate) struct DbgMetadata {
    /// Metadata node bodies, indexed by ID. `nodes[i]` is the body of `!i`.
    nodes: Vec<String>,
    /// DICompileUnit node ID (always 0).
    cu_id: u32,
    /// DIFile node ID (always 4).
    file_id: u32,
    /// Empty `!{}` node ID for retainedNodes (always 5).
    empty_id: u32,
    /// Whether debug info emission is enabled.
    enabled: bool,
    /// 024 A1：StringBuilder 热路径 TBAA 节点（惰性分配，首个 FnEmitter
    /// 命中 `emit_sb_append_char_inline` 时创建，跨模块复用同一组 ID）。
    /// 无关 debug info 开关——LLVM 别名元数据与 DWARF 各自独立。
    sb_tbaa: Option<SbTbaaIds>,
    /// 045 M1：RtList 内联索引热路径 TBAA 节点（惰性分配一次，跨模块复用）。
    rt_list_tbaa: Option<RtListTbaaIds>,
    /// 045 M4：StringBuilder 缓冲 scoped-noalias 节点（惰性分配一次，跨模块复用）。
    /// 声明 `data` 缓冲（ensure 独立分配）与 `rt_sb_t` 头（data/len/cap 字段）
    /// 互不相交——缓冲访问挂 `!alias.scope`，头字段访问挂 `!noalias`（对齐
    /// [034 A1](038-native-load-model.md) scoped-noalias · restrict 语义）。
    sb_alias_scope: Option<SbAliasScopeIds>,
    /// 045 M5：用户 struct 类型 TBAA 节点缓存（按 struct 名惰性分配一次，跨模块复用）。
    /// 为热路径用户 struct 的字段 load/store 发射 struct-path TBAA，使 clang
    /// 能对用户类型做与 C 等价的别名消歧（对齐 `rt_list_tbaa` 先例）。
    struct_tbaa: HashMap<String, StructTbaaIds>,
}

/// 024 A1：StringBuilder 内联直降所需的 TBAA 节点 ID。
///
/// TBAA 树（对齐 clang struct-path 语法，根 → 互不 alias 的分枝）：
/// - `data`/`len`/`cap` 为 `rt_sb_t` 头各字段 tag（offset 0/8/16），
///   同 struct 不同 offset 的访问彼此不 alias。
/// - `buffer` 为 char 数据缓冲的标量访问类型，与头字段 tag 互不 alias。
///
/// 断言语义：`rt_sb_t` 头由 `rt_text_sb_new*` 独立 malloc，data 缓冲由
/// `rt_sb_ensure` 另行 malloc/realloc——两个对象永不相交（见 rt_text.c）。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SbTbaaIds {
    /// `rt_sb_t.data` 字段访问 tag（offset 0，char*）。
    pub data: u32,
    /// `rt_sb_t.len` 字段访问 tag（offset 8，size_t）。
    pub len: u32,
    /// `rt_sb_t.cap` 字段访问 tag（offset 16，size_t）。
    pub cap: u32,
    /// char 数据缓冲访问 tag（`!{!<buffer标量>, !<buffer标量>, i64 0}`）。
    pub buffer: u32,
}

/// 045 M1：RtList 内联索引热路径所需的 TBAA 节点 ID。
///
/// 对齐 `rt_list.c` 的 `RtList` 头布局（x64）：
/// - `data`@0（char*）、`size`@8（i32）、`capacity`@12（i32）、
///   `elem_size`@16（i32）、`eq`@24（ptr）、`arc_inc`@32（ptr）、`arc_dec`@40（ptr）。
///   索引槽路径仅触及 `data`@0 与 `size`@8——同 struct 不同 offset 的访问彼此
///   不 alias，使 `size` 越界检查与 `data` GEP 可独立调度，兑现 024 A1 别名消除。
#[derive(Clone, Copy, Debug)]
pub(crate) struct RtListTbaaIds {
    /// `RtList.data` 字段访问 tag（offset 0，char*）。
    pub data: u32,
    /// `RtList.size` 字段访问 tag（offset 8，int32_t）。
    pub size: u32,
}

/// 045 M4：StringBuilder 缓冲 scoped-noalias 节点 ID。
///
/// 对齐 LLVM scoped no-alias 元数据形态（clang 校验严格）：
///
/// ```text
/// !n = distinct !{!n, !{!"arc_sb_buffer_scope"}}   scope（distinct · 自引用）
/// ```
///
/// - 首操作数必须**自引用**（`!n`）——clang 校验「first scope operand must be
///   self-referential or string」；
/// - 次操作数（名称）必须是 **MDNode** `!{!"…"}`——clang 校验「second scope
///   operand must be MDNode」。
///
/// - 经 `data` 的缓冲访问挂 `!alias.scope !{!<scope>}`；
/// - `rt_sb_t` 头（data/len/cap）字段访问挂 `!noalias !{!<scope>}`。
///
/// 断言语义：`data` 指向 ensure 独立 malloc/realloc 的字符缓冲，与 `rt_sb_t`
/// 头结构体（独立 malloc，见 sb_tbaa 注释 / rt_text.c）永不相交。TBAA（M1）
/// 依类型区分，scoped-noalias（M4）在此基础上以「restrict 契约」显式声明
/// 缓冲不 alias 头——二者互补，兑现 [034 A1](034-native-load-model.md)。
#[derive(Clone, Copy, Debug)]
pub(crate) struct SbAliasScopeIds {
    /// scope 节点 `distinct !{!<self>, !{!"arc_sb_buffer_scope"}}`
    /// （挂于 `!alias.scope`/`!noalias`）。
    pub scope: u32,
}

/// 045 M5：用户 struct 类型的 struct-path TBAA 节点 ID。
///
/// 对齐 clang 对 C struct 的 struct-path TBAA 发射：`!tbaa` 挂于字段访问 tag
/// `!{!<struct>, !<access_scalar>, i64 <offset>}`，同一 struct 不同 offset 的
/// 字段访问彼此不 alias。`field_tag` 按字段名 → 字段访问 tag 索引。
#[derive(Clone, Debug)]
pub(crate) struct StructTbaaIds {
    /// 字段名 → 字段访问 tag ID（`alloc_tbaa_field`）。
    pub field_tag: HashMap<String, u32>,
}

impl DbgMetadata {
    /// Create a new debug metadata emitter.
    ///
    /// `file_path` is the source file path (used for DIFile).
    /// `enabled` controls whether metadata is actually emitted; when false,
    /// all methods are no-ops and `render()` returns an empty string.
    pub fn new(file_path: &str, enabled: bool) -> Self {
        if !enabled {
            return Self {
                nodes: Vec::new(),
                cu_id: 0,
                file_id: 0,
                empty_id: 0,
                enabled: false,
                sb_tbaa: None,
                rt_list_tbaa: None,
                sb_alias_scope: None,
                struct_tbaa: HashMap::new(),
            };
        }

        let path = Path::new(file_path);
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown.as".into());
        let file_dir = path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());

        // Reserve fixed nodes 0-5.
        let mut nodes = vec![String::new(); 6];
        // !0 = DICompileUnit (finalized in `finalize()`)
        // !1 = Dwarf Version flag
        nodes[1] = r#"!{i32 7, !"Dwarf Version", i32 5}"#.into();
        // !2 = Debug Info Version flag
        nodes[2] = r#"!{i32 2, !"Debug Info Version", i32 3}"#.into();
        // !3 = llvm.ident
        nodes[3] = r#"!{!"Arc 0.1.0"}"#.into();
        // !4 = DIFile
        nodes[4] = format!(
            r#"!DIFile(filename: "{}", directory: "{}")"#,
            escape(&file_name),
            escape(&file_dir)
        );
        // !5 = empty retainedNodes list
        nodes[5] = r#"!{}"#.into();

        Self {
            nodes,
            cu_id: 0,
            file_id: 4,
            empty_id: 5,
            enabled: true,
            sb_tbaa: None,
            rt_list_tbaa: None,
            sb_alias_scope: None,
            struct_tbaa: HashMap::new(),
        }
    }

    /// Whether debug info emission is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Finalize the DICompileUnit. Must be called after all subprograms are added,
    /// before `render()`.
    pub fn finalize(&mut self) {
        if !self.enabled {
            return;
        }
        self.nodes[self.cu_id as usize] = format!(
            r#"distinct !DICompileUnit(language: DW_LANG_C_plus_plus, file: !{}, producer: "Arc 0.1.0", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)"#,
            self.file_id
        );
    }

    /// Create a `DISubprogram` metadata node.
    ///
    /// Returns the node ID to attach to the function definition via `!dbg !N`.
    pub fn add_subprogram(
        &mut self,
        name: &str,
        linkage_name: &str,
        line: u32,
        type_id: u32,
    ) -> u32 {
        if !self.enabled {
            return 0;
        }
        let id = self.next_id();
        self.nodes[id as usize] = format!(
            r#"distinct !DISubprogram(name: "{}", linkageName: "{}", scope: !{}, file: !{}, line: {}, type: !{}, scopeLine: {}, isLocal: false, isDefinition: true, unit: !{}, retainedNodes: !{})"#,
            escape(name),
            escape(linkage_name),
            self.file_id,
            self.file_id,
            line,
            type_id,
            line,
            self.cu_id,
            self.empty_id
        );
        id
    }

    /// Create a `DISubroutineType` metadata node.
    ///
    /// `ret_type_id`: return type DIBasicType ID, or `None` for void.
    /// `param_type_ids`: parameter type DIBasicType IDs.
    pub fn add_subroutine_type(&mut self, ret_type_id: Option<u32>, param_type_ids: &[u32]) -> u32 {
        if !self.enabled {
            return 0;
        }
        let mut types = Vec::with_capacity(param_type_ids.len() + 1);
        types.push(
            ret_type_id
                .map(|i| format!("!{i}"))
                .unwrap_or_else(|| "null".into()),
        );
        for pid in param_type_ids {
            types.push(format!("!{pid}"));
        }
        let types_list_id = self.add_node_list(&types);
        let id = self.next_id();
        self.nodes[id as usize] = format!("!DISubroutineType(types: !{types_list_id})");
        id
    }

    /// Create a `DIBasicType` metadata node.
    ///
    /// `encoding` should be a DWARF encoding constant like `DW_ATE_signed`.
    #[allow(dead_code)]
    pub fn add_basic_type(&mut self, name: &str, size: u32, encoding: &str) -> u32 {
        if !self.enabled {
            return 0;
        }
        let id = self.next_id();
        self.nodes[id as usize] = format!(
            r#"!DIBasicType(name: "{}", size: {}, encoding: {})"#,
            escape(name),
            size,
            encoding
        );
        id
    }

    /// Create a `DILocation` metadata node.
    ///
    /// Returns the node ID to attach to an instruction via `, !dbg !N`.
    /// `line`/`col` are 1-based; use 0 for unknown.
    pub fn add_location(&mut self, line: u32, col: u32, scope: u32) -> u32 {
        if !self.enabled {
            return 0;
        }
        let id = self.next_id();
        self.nodes[id as usize] = format!(
            r#"!DILocation(line: {}, column: {}, scope: !{})"#,
            line, col, scope
        );
        id
    }

    /// Render all metadata nodes as LLVM IR text.
    ///
    /// Emits the `!llvm.dbg.cu`, `!llvm.module.flags`, `!llvm.ident`
    /// named metadata declarations, followed by all numbered `!N = ...` nodes.
    ///
    /// RFC 009 M3：即使 debug info 被禁用，仍可能存在 loop vectorization
    /// metadata 节点（由 `alloc_loop_md` 推入）。此时仅发射这些节点，
    /// 不发射 dbg 相关闭包。
    pub fn render(&self) -> String {
        if !self.enabled {
            // Debug disabled: only emit loop vectorization metadata (if any).
            // When debug is disabled, `nodes` only contains loop MD entries
            // (alloc_loop_md pushes directly to `nodes`).
            if self.nodes.is_empty() {
                return String::new();
            }
            let mut out = String::new();
            for (i, node) in self.nodes.iter().enumerate() {
                out.push_str(&format!("!{i} = {node}\n"));
            }
            return out;
        }
        let mut out = String::new();
        out.push_str(&format!("!llvm.dbg.cu = !{{!{}}}\n", self.cu_id));
        out.push_str("!llvm.module.flags = !{!1, !2}\n");
        out.push_str("!llvm.ident = !{!3}\n");
        for (i, node) in self.nodes.iter().enumerate() {
            out.push_str(&format!("!{i} = {node}\n"));
        }
        out
    }

    /// RFC 039 M3：分配一个 `!llvm.loop` metadata 节点 ID，用于标记 while
    /// 循环 backedge 为强制向量化候选。
    ///
    /// 返回值为 metadata ID（即 `!N` 中的 N）。节点体为
    /// `!{!N, i32 1}`，其中 `i32 1` = `llvm.loop.vectorize.enable`。
    ///
    /// **平台无关**：此 metadata 仅提示 LLVM loop-vectorize pass 启用向量化；
    /// 实际指令集选择由 LLVM 据目标 CPU 特征决定（x86 SSE2/AVX2/AVX-512、
    /// ARM NEON、其他标量退化）。
    ///
    /// **ID 分配规则**：与 debug metadata 共享同一 `nodes` 命名空间，
    /// ID = `nodes.len()`（debug 启用时排在 dbg 节点之后；禁用时从 0 开始）。
    /// 这保证 IDs 全局唯一且与 `render()` 输出顺序一致。
    pub fn alloc_loop_md(&mut self) -> u32 {
        let id = self.nodes.len() as u32;
        // 自引用节点：`!{!N, i32 1}` —— LLVM loop metadata 的标准形式。
        // i32 1 对应 `llvm.loop.vectorize.enable = true`。
        let body = format!("!{{!{id}, i32 1}}");
        self.nodes.push(body);
        id
    }

    /// 024 A1：获取 StringBuilder 热路径 TBAA 节点 ID（惰性分配一次）。
    ///
    /// TBAA 树（对齐 clang 的 struct-path TBAA，旧格式）：
    ///
    /// ```text
    /// !0 = !{!"arc_tbaa_root"}                         根
    /// !1 = !{!"arc_ptr", !0}                           标量：char* 字段访问类型
    /// !2 = !{!"arc_i64", !0}                           标量：size_t 字段访问类型
    /// !3 = !{!"arc_sb_buffer", !0}                     标量：char 缓冲访问类型
    /// !4 = !{!"arc_sb_header", !1, i64 0, !2, i64 8, !2, i64 16}  struct 头
    /// !5 = !{!4, !1, i64 0}                            tag：data@0
    /// !6 = !{!4, !2, i64 8}                            tag：len@8
    /// !7 = !{!4, !2, i64 16}                           tag：cap@16
    /// !8 = !{!3, !3, i64 0}                            tag：char 缓冲 store
    /// ```
    ///
    /// 效果（LLVM TBAA 结构路径规则）：`!5`/`!6`/`!7` 同一 struct `!4` 下不同
    /// offset → 互不 alias；`!8` 与 `!5`/`!6`/`!7` 访问类型分属根的两枝 →
    /// 互不 alias；同 tag 自身仍 alias（保序）。这使追加循环中 data/cap 可被
    /// 提升出循环、len 保持寄存器变量。
    pub fn sb_tbaa(&mut self) -> SbTbaaIds {
        if let Some(ids) = self.sb_tbaa {
            return ids;
        }
        let root = self.alloc_tbaa_root("arc_tbaa_root");
        let ptr = self.alloc_tbaa_scalar("arc_ptr", root);
        let i64ty = self.alloc_tbaa_scalar("arc_i64", root);
        let buffer = self.alloc_tbaa_scalar("arc_sb_buffer", root);
        let sb = self.alloc_tbaa_struct("arc_sb_header", &[(ptr, 0), (i64ty, 8), (i64ty, 16)]);
        let data = self.alloc_tbaa_field(sb, ptr, 0);
        let len = self.alloc_tbaa_field(sb, i64ty, 8);
        let cap = self.alloc_tbaa_field(sb, i64ty, 16);
        let buf_tag = self.alloc_tbaa_scalar_tag(buffer);
        let ids = SbTbaaIds {
            data,
            len,
            cap,
            buffer: buf_tag,
        };
        self.sb_tbaa = Some(ids);
        ids
    }

    /// 045 M1：获取 RtList 内联索引热路径 TBAA 节点 ID（惰性分配一次）。
    ///
    /// TBAA 树（对齐 clang 的 struct-path TBAA，旧格式）：
    ///
    /// ```text
    /// !0 = !{!"arc_tbaa_root"}                         根
    /// !1 = !{!"arc_ptr", !0}                           标量：char* 字段访问类型
    /// !2 = !{!"arc_i32", !0}                           标量：int32_t 字段访问类型
    /// !3 = !{!"arc_rt_list", !1, i64 0, !2, i64 8}     struct 头
    /// !4 = !{!3, !1, i64 0}                            tag：data@0
    /// !5 = !{!3, !2, i64 8}                            tag：size@8
    /// ```
    ///
    /// 效果：`!4`/`!5` 同一 struct `!3` 下不同 offset → 互不 alias，使 `size`
    /// 越界检查与 `data` GEP 可独立调度（与 `sb_tbaa` 同构）。
    pub fn rt_list_tbaa(&mut self) -> RtListTbaaIds {
        if let Some(ids) = self.rt_list_tbaa {
            return ids;
        }
        let root = self.alloc_tbaa_root("arc_tbaa_root");
        let ptr = self.alloc_tbaa_scalar("arc_ptr", root);
        let i32ty = self.alloc_tbaa_scalar("arc_i32", root);
        let list = self.alloc_tbaa_struct("arc_rt_list", &[(ptr, 0), (i32ty, 8)]);
        let data = self.alloc_tbaa_field(list, ptr, 0);
        let size = self.alloc_tbaa_field(list, i32ty, 8);
        let ids = RtListTbaaIds { data, size };
        self.rt_list_tbaa = Some(ids);
        ids
    }

    /// 045 M5：获取用户 struct 类型的 struct-path TBAA 节点 ID（惰性分配一次）。
    ///
    /// `name` 为用户 struct 名；`fields` 为 `(字段名, 字段访问标量名, 字节偏移)`。
    ///
    /// TBAA 树（对齐 clang 对 C struct 的 struct-path 发射）：
    /// ```text
    /// !0 = !{!"arc_tbaa_root"}                       根
    /// !1 = !{!"arc_ptr", !0}                         标量：char* 字段访问类型
    /// !2 = !{!"arc_i64", !0}                         标量：size_t/int64 字段访问类型
    /// !3 = !{!"<struct>", !1, i64 0, !2, i64 8}      struct 类型（字段 @offset 对）
    /// !4 = !{!3, !1, i64 0}                          tag：field0@0
    /// !5 = !{!3, !2, i64 8}                          tag：field1@8
    /// ```
    /// 同 struct 不同 offset 的字段访问彼此不 alias（与 `rt_list_tbaa` 同构）。
    /// 跨模块惰性缓存于 `struct_tbaa`，复用同一组 ID。不受 `enabled` 门控。
    pub fn struct_tbaa(&mut self, name: &str, fields: &[(String, String, i64)]) -> StructTbaaIds {
        if let Some(ids) = self.struct_tbaa.get(name) {
            return ids.clone();
        }
        let root = self.alloc_tbaa_root("arc_tbaa_root");
        // 收集去重后的字段访问标量名。
        let mut unique: Vec<&str> = Vec::new();
        for (_, scalar, _) in fields {
            if !unique.iter().any(|s| *s == scalar) {
                unique.push(scalar);
            }
        }
        // 每个标量名分配一个标量节点（跨字段复用）。
        let mut scalars: HashMap<String, u32> = HashMap::new();
        for s in &unique {
            let id = self.alloc_tbaa_scalar(&format!("arc_{s}"), root);
            scalars.insert(s.to_string(), id);
        }
        // struct 类型节点：字段 (访问标量节点, 偏移) 对。
        let struct_fields: Vec<(u32, i64)> = fields
            .iter()
            .map(|(_, scalar, off)| (scalars[scalar], *off))
            .collect();
        let struct_node = self.alloc_tbaa_struct(name, &struct_fields);
        // 字段访问 tag。
        let mut field_tag = HashMap::new();
        for (fname, scalar, off) in fields {
            field_tag.insert(
                fname.clone(),
                self.alloc_tbaa_field(struct_node, scalars[scalar], *off),
            );
        }
        let ids = StructTbaaIds { field_tag };
        self.struct_tbaa.insert(name.to_string(), ids.clone());
        ids
    }

    /// 045 M4：获取 StringBuilder 缓冲 scoped-noalias 节点 ID（惰性分配一次）。
    ///
    /// 单个自引用 scope 节点 `distinct !{!<self>, !{!"arc_sb_buffer_scope"}}`。
    /// 缓冲访问挂 `!alias.scope !{!scope}`、头字段访问挂 `!noalias !{!scope}`，
    /// 声明 `data` 缓冲与 `rt_sb_t` 头互不相交（[034 A1](034-native-load-model.md)
    /// scoped-noalias · restrict）。与 `sb_tbaa` 一样不受 `enabled` 门控。
    pub fn sb_alias_scope(&mut self) -> SbAliasScopeIds {
        if let Some(ids) = self.sb_alias_scope {
            return ids;
        }
        let scope = self.alloc_alias_scope("arc_sb_buffer_scope");
        let ids = SbAliasScopeIds { scope };
        self.sb_alias_scope = Some(ids);
        ids
    }

    /// 分配 scoped-noalias scope 节点：`distinct !{!<self>, !{!"<name>"}}`。
    ///
    /// LLVM 校验（clang 实测）：
    /// - 首操作数必须**自引用**（`!<self>`）或字符串——「first scope operand must
    ///   be self-referential or string」；
    /// - 次操作数（名称）必须是 **MDNode** `!{!"…"}`——「second scope operand must
    ///   be MDNode」。
    ///
    /// 简单「缓冲不 alias 头」契约用单个自引用 scope 即可；同一 scope 可被
    /// `!alias.scope` 与 `!noalias` 双向引用（配对契约）。
    fn alloc_alias_scope(&mut self, name: &str) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes
            .push(format!(r#"distinct !{{!{id}, !{{!"{name}"}}}}"#));
        id
    }

    /// 分配 TBAA 根节点：`!{!"<name>"}`（单元素，见 LLVM LangRef 根节点形态）。
    ///
    /// 不受 `enabled` 门控（与 `alloc_loop_md` 一致）：别名元数据与 debug info
    /// 相互独立，必须始终发射。ID 分配规则同 `alloc_loop_md`（追加到 `nodes`）。
    fn alloc_tbaa_root(&mut self, name: &str) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(format!(r#"!{{!"{name}"}}"#));
        id
    }

    /// 分配 TBAA 标量类型节点：`!{!"<name>", !<parent>}`。
    fn alloc_tbaa_scalar(&mut self, name: &str, parent: u32) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes.push(format!(r#"!{{!"{name}", !{parent}}}"#));
        id
    }

    /// 分配 TBAA struct 类型节点（旧格式，字段以 `(类型节点, 字节偏移)` 对展开）：
    /// `!{!"<name>", !<f0>, i64 <o0>, !<f1>, i64 <o1>, …}`。
    ///
    /// 首字段对同时充当类型 DAG 的父链（旧格式 parent/field 不区分，见 LLVM
    /// TypeBasedAliasAnalysis.cpp）；字段 offset 必须递增，供路径规约按 offset
    /// 下钻到 access type。
    fn alloc_tbaa_struct(&mut self, name: &str, fields: &[(u32, i64)]) -> u32 {
        let id = self.nodes.len() as u32;
        let mut body = String::with_capacity(16 + name.len());
        body.push_str("!{!\"");
        body.push_str(name);
        body.push('"');
        for (ty, off) in fields {
            body.push_str(&format!(", !{ty}, i64 {off}"));
        }
        body.push('}');
        self.nodes.push(body);
        id
    }

    /// 分配 TBAA 字段访问 tag：`!{!<base>, !<access>, i64 <offset>}`。
    ///
    /// base 为 struct 类型节点、access 为标量访问类型节点；附于 `!tbaa` 的即此
    /// 节点。LLVM 据同 struct 不同 offset 判 NoAlias（clang 对 `S->data`/
    /// `S->len` 的惯用发射形式）。
    fn alloc_tbaa_field(&mut self, base: u32, access: u32, offset: i64) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes
            .push(format!(r#"!{{!{base}, !{access}, i64 {offset}}}"#));
        id
    }

    /// 分配标量访问 tag：`!{!<scalar>, !<scalar>, i64 0}`。
    ///
    /// 不经 struct 的裸标量访问（如 char 缓冲 store）使用此形态——base 与
    /// access 同为一个标量节点，offset 0。
    fn alloc_tbaa_scalar_tag(&mut self, scalar: u32) -> u32 {
        let id = self.nodes.len() as u32;
        self.nodes
            .push(format!(r#"!{{!{scalar}, !{scalar}, i64 0}}"#));
        id
    }

    // ---- Internal helpers ----

    fn next_id(&mut self) -> u32 {
        let id = self.nodes.len() as u32;
        // Push placeholder; caller will overwrite.
        self.nodes.push(String::new());
        id
    }

    fn add_node_list(&mut self, items: &[String]) -> u32 {
        let id = self.next_id();
        let body = format!("!{{ {} }}", items.join(", "));
        self.nodes[id as usize] = body;
        id
    }
}

/// Escape a string for LLVM IR metadata (backslash and double-quote).
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 024 A1：TBAA 与 debug info 开关相互独立——`enabled=false`（Release
    /// 构建、无 `-g`）时 `sb_tbaa()` 仍须产出合法 `!tbaa` 节点。
    #[test]
    fn sb_tbaa_emitted_when_debug_disabled() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let ids = dbg.sb_tbaa();
        let out = dbg.render();
        // 禁用 debug 时节点从 !0 开始。
        assert_eq!(ids.data, 5);
        assert_eq!(ids.len, 6);
        assert_eq!(ids.cap, 7);
        assert_eq!(ids.buffer, 8);
        assert!(
            out.contains(r#"!0 = !{!"arc_tbaa_root"}"#),
            "root missing:\n{out}"
        );
        assert!(
            out.contains(r#"!4 = !{!"arc_sb_header", !1, i64 0, !2, i64 8, !2, i64 16}"#),
            "struct type node missing:\n{out}"
        );
        assert!(
            out.contains("!5 = !{!4, !1, i64 0}"),
            "data tag missing:\n{out}"
        );
        assert!(
            out.contains("!6 = !{!4, !2, i64 8}"),
            "len tag missing:\n{out}"
        );
        assert!(
            out.contains("!7 = !{!4, !2, i64 16}"),
            "cap tag missing:\n{out}"
        );
        assert!(
            out.contains("!8 = !{!3, !3, i64 0}"),
            "buffer tag missing:\n{out}"
        );
        // 无 debug 前置（!llvm.dbg.cu 等）——仅编号节点。
        assert!(
            !out.contains("!llvm.dbg.cu"),
            "debug preamble leaked:\n{out}"
        );
    }

    /// 024 A1：`sb_tbaa()` 惰性分配只发生一次，重复调用返回同一组 ID。
    #[test]
    fn sb_tbaa_lazy_single_allocation() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let a = dbg.sb_tbaa();
        let b = dbg.sb_tbaa();
        assert_eq!(a.data, b.data);
        assert_eq!(a.len, b.len);
        assert_eq!(a.cap, b.cap);
        assert_eq!(a.buffer, b.buffer);
        let out = dbg.render();
        // 9 个节点：root/ptr/i64/buffer/sb + data/len/cap/buf_tag。
        assert_eq!(out.lines().filter(|l| l.starts_with('!')).count(), 9);
    }

    /// 024 A1：debug 启用时 TBAA 节点与 DWARF 节点共享同一 ID 空间且互不冲突。
    #[test]
    fn sb_tbaa_coexists_with_debug_metadata() {
        let mut dbg = DbgMetadata::new("test.as", true);
        dbg.finalize();
        let ids = dbg.sb_tbaa();
        let out = dbg.render();
        assert!(
            out.contains("!llvm.dbg.cu = !{!0}"),
            "debug cu missing:\n{out}"
        );
        assert!(
            out.contains(r#"!6 = !{!"arc_tbaa_root"}"#),
            "tbaa root missing:\n{out}"
        );
        assert!(
            out.contains("!llvm.dbg.cu = !{!0}") && out.contains("arc_sb_header"),
            "tbnaa should coexist with dwarf:\n{out}"
        );
        // 禁用分支的 ID 偏移：root=6 → data/len/cap/buffer = 11/12/13/14。
        assert_eq!(ids.data, 11);
        assert_eq!(ids.len, 12);
        assert_eq!(ids.cap, 13);
        assert_eq!(ids.buffer, 14);
        // 引用必须指向存在的节点体。
        for id in [ids.data, ids.len, ids.cap, ids.buffer] {
            assert!(
                out.contains(&format!("!{id} = !{{")),
                "id !{id} body missing:\n{out}"
            );
        }
    }

    /// 045 M1：`rt_list_tbaa()` 惰性分配一次且与 debug 开关无关（对齐 sb_tbaa）。
    #[test]
    fn rt_list_tbaa_emitted_when_debug_disabled() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let a = dbg.rt_list_tbaa();
        let b = dbg.rt_list_tbaa();
        assert_eq!(a.data, b.data);
        assert_eq!(a.size, b.size);
        let out = dbg.render();
        // 禁用 debug 时节点从 !0 开始：root/ptr/i32/list + data@0/size@8。
        assert_eq!(a.data, 4);
        assert_eq!(a.size, 5);
        assert!(
            out.contains(r#"!0 = !{!"arc_tbaa_root"}"#),
            "root missing:\n{out}"
        );
        assert!(
            out.contains(r#"!3 = !{!"arc_rt_list", !1, i64 0, !2, i64 8}"#),
            "struct type node missing:\n{out}"
        );
        assert!(
            out.contains("!4 = !{!3, !1, i64 0}"),
            "data tag missing:\n{out}"
        );
        assert!(
            out.contains("!5 = !{!3, !2, i64 8}"),
            "size tag missing:\n{out}"
        );
        assert!(
            !out.contains("!llvm.dbg.cu"),
            "debug preamble leaked:\n{out}"
        );
    }

    /// 045 M1：`rt_list_tbaa()` 与既有 `sb_tbaa()` 共享根节点、互不冲突。
    #[test]
    fn rt_list_tbaa_coexists_with_sb_tbaa() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let sb = dbg.sb_tbaa();
        let list = dbg.rt_list_tbaa();
        let out = dbg.render();
        assert!(out.contains("arc_sb_header"), "sb header missing:\n{out}");
        assert!(
            out.contains("arc_rt_list"),
            "rt_list header missing:\n{out}"
        );
        // 同一根 `arc_tbaa_root` 下两棵 TBAA 树并存，ID 各自唯一。
        assert_ne!(sb.data, list.data);
        assert_ne!(sb.len, list.size);
        for id in [list.data, list.size] {
            assert!(
                out.contains(&format!("!{id} = !{{")),
                "id !{id} body missing:\n{out}"
            );
        }
    }

    /// 045 M4：`sb_alias_scope()` 惰性分配一次，且与 debug 开关无关。
    /// 发射自引用 scope 节点 `distinct !{!<self>, !{!"arc_sb_buffer_scope"}}`。
    #[test]
    fn sb_alias_scope_emitted_when_debug_disabled() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let a = dbg.sb_alias_scope();
        let b = dbg.sb_alias_scope();
        assert_eq!(a.scope, b.scope);
        let out = dbg.render();
        // 禁用 debug 时节点从 !0 开始：scope=0。
        assert_eq!(a.scope, 0);
        assert!(
            out.contains(r#"!0 = distinct !{!0, !{!"arc_sb_buffer_scope"}}"#),
            "self-referential scope node missing:\n{out}"
        );
        assert!(
            !out.contains("!llvm.dbg.cu"),
            "debug preamble leaked:\n{out}"
        );
    }

    /// 045 M4：`sb_alias_scope()` 与 `sb_tbaa()` 共享同一 ID 空间、互不冲突。
    #[test]
    fn sb_alias_scope_coexists_with_sb_tbaa() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let sb = dbg.sb_tbaa();
        let scope = dbg.sb_alias_scope();
        let out = dbg.render();
        assert!(out.contains("arc_sb_header"), "tbaa header missing:\n{out}");
        assert!(
            out.contains("arc_sb_buffer_scope"),
            "alias.scope scope missing:\n{out}"
        );
        assert!(
            out.contains("distinct !{"),
            "distinct scope missing:\n{out}"
        );
        assert!(
            out.contains(&format!("!{} = distinct !{{", scope.scope)),
            "scope !{} body missing:\n{out}",
            scope.scope
        );
        // TBAA 节点 ID 与 scope 节点 ID 不重叠。
        for id in [sb.data, sb.len, sb.cap, sb.buffer] {
            assert!(
                out.contains(&format!("!{id} = !{{")),
                "id !{id} body missing:\n{out}"
            );
        }
        assert_ne!(
            scope.scope, sb.buffer,
            "alias.scope collides with tbaa buffer tag"
        );
    }

    /// 045 M5：`struct_tbaa()` 惰性分配一次，且与 debug 开关无关。
    /// 发射用户 struct 类型节点 + 字段 tag（X@0 / Y@4，同 struct 不同 offset）。
    #[test]
    fn struct_tbaa_emitted_when_debug_disabled() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let fields = vec![
            ("X".to_string(), "i64".to_string(), 0i64),
            ("Y".to_string(), "i64".to_string(), 4i64),
        ];
        let a = dbg.struct_tbaa("Point", &fields);
        let b = dbg.struct_tbaa("Point", &fields);
        assert_eq!(a.field_tag["X"], b.field_tag["X"]);
        assert_eq!(a.field_tag["Y"], b.field_tag["Y"]);
        let out = dbg.render();
        // 禁用 debug 时节点从 !0 开始：root/标量(i64)/Point struct/X@0/Y@4。
        assert!(
            out.contains(r#"!0 = !{!"arc_tbaa_root"}"#),
            "root missing:\n{out}"
        );
        assert!(
            out.contains(r#"!"Point""#),
            "struct type node missing:\n{out}"
        );
        assert_ne!(
            a.field_tag["X"], a.field_tag["Y"],
            "distinct field tags expected"
        );
        // 同 struct 不同 offset 的字段 tag 必须互不 alias（offset 0 vs 4）。
        assert!(out.contains(", i64 0}"), "X@0 tag missing:\n{out}");
        assert!(out.contains(", i64 4}"), "Y@4 tag missing:\n{out}");
        assert!(
            !out.contains("!llvm.dbg.cu"),
            "debug preamble leaked:\n{out}"
        );
    }

    /// 045 M5：`struct_tbaa()` 与既有 `rt_list_tbaa()` / `sb_tbaa()` 共享同一
    /// ID 空间、互不冲突（同一 root 下多棵 TBAA 树并存）。
    #[test]
    fn struct_tbaa_coexists_with_rt_list_tbaa() {
        let mut dbg = DbgMetadata::new("test.as", false);
        let list = dbg.rt_list_tbaa();
        let fields = vec![("X".to_string(), "i64".to_string(), 0i64)];
        let point = dbg.struct_tbaa("Point", &fields);
        let out = dbg.render();
        assert!(out.contains("arc_rt_list"), "rt_list missing:\n{out}");
        assert!(out.contains(r#"!"Point""#), "user struct missing:\n{out}");
        assert_ne!(list.size, point.field_tag["X"], "tag ID collision");
    }
}
