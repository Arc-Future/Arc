//! Runtime stub generation for builtin collection methods.
//!
//! Contains LLVM IR stub emitters for Dictionary<K,V>, List<T>, and
//! ListEnumerator<T> builtin methods, plus type coercion utilities.

use super::*;

impl<'a> FnEmitter<'a> {
    /// Wraps a stub IR body with `linkonce_odr` linkage and COMDAT declaration.
    ///
    /// Replaces `define ` → `define linkonce_odr ` and prepends `$comdat_{mangled} = comdat any`.
    /// Required for Windows COFF where `linkonce_odr` without `comdat` still produces
    /// duplicate symbol errors from lld-link (RFC 017 M4-link Phase B §D2.1).
    fn stub_linkonce(&self, name: &str, body: String) -> String {
        let mangled = mangle_fn_name(name);
        let mut body = body;
        body = body.replace("define void @", "define linkonce_odr void @");
        body = body.replace("define i1 @", "define linkonce_odr i1 @");
        body = body.replace("define i32 @", "define linkonce_odr i32 @");
        body = body.replace("define i64 @", "define linkonce_odr i64 @");
        body = body.replace("define ptr @", "define linkonce_odr ptr @");
        body = body.replace(") {\n", ") comdat{\n");
        // Emit comdat declaration inline — stubs (including sub-stubs from
        // GetEnumerator) are not tracked by emit_comdat_decls.
        format!("${mangled} = comdat any\n{body}")
    }
    // ---- Builtin collection stubs ----
}

/// Returns true if the class name matches a builtin collection pattern
/// (Dictionary, List, HashSet, etc.) that would be handled by a stub.
pub fn class_is_stub_handled(name: &str) -> bool {
    let class = name.strip_prefix("__ctor::").unwrap_or(name);
    let class = class.split("::").next().unwrap_or(class);
    parse_dict_kv(class).is_some()
        || parse_concurrent_dict_kv(class).is_some()
        || parse_concurrent_single_elem(class).is_some()
        || parse_set_elem(class).is_some()
        || parse_queue_elem(class).is_some()
        || parse_stack_elem(class).is_some()
        || parse_enumerator_elem(class).is_some()
        || parse_dict_enumerator_kv(class).is_some()
        || parse_list_elem(class).is_some()
        || parse_tensor_elem(class).is_some()
        || parse_sorted_dict_kv(class).is_some()
        || parse_linked_list_elem(class).is_some()
        || parse_linked_list_node_elem(class).is_some()
        || parse_sorted_set_elem(class).is_some()
        || parse_weak_elem(class).is_some()
        || (class == "StringBuilder" && name.contains("__ctor"))
        || {
            // FileStream：仅 stub 路径的方法由 stub_linkonce 内联 comdat；
            // OpenRead/OpenWrite/Create 走真实 MIR，须进模块级 comdat 收集。
            if class == "FileStream" || class.starts_with("FileStream_") {
                let method = name.split("::").last().unwrap_or(name);
                let method = method.strip_prefix("FileStream_").unwrap_or(method);
                !matches!(method, "OpenRead" | "OpenWrite" | "Create")
            } else {
                class == "TextBuffer" || class.starts_with("TextBuffer_")
            }
        }
}

/// 把**裸 mangled 名**归一化为 stub 解析所需的 `Class::Method` 形态。
///
/// completeness 补发闭环（llvm_ir::mod）塞入 fns 的条目是调用点引用的裸
/// 符号名（`HashSet_string_get_Item`，无 `::`）——stub 各解析器以
/// `split("::")` 取方法名，bare 名 method="" → 全部落默认降级空体
/// （`void @HashSet_string_get_Item()`，与调用点签名不匹配 → clang 拒绝
/// IR）。此处按已知 stub 方法名后缀反向
/// 拆分；归一化后 `mangle_fn_name`（`::`→`_`）产出符号与裸名一致，引用
/// 不变。已含 `::` 的条目原样返回。
fn normalize_bare_stub_name(name: &str) -> String {
    if name.contains("::") {
        return name.to_string();
    }
    const STUB_METHODS: &[&str] = &[
        "get_Item",
        "set_Item",
        "get_Count",
        "get_IsEmpty",
        "get_Capacity",
        "get_Keys",
        "get_Values",
        "GetEnumerator",
        "TryGetValue",
        "TryAdd",
        "TryDequeue",
        "TryPeek",
        "TryPop",
        "TryTake",
        "GetValueOrDefault",
        "UnionWith",
        "IntersectWith",
        "ExceptWith",
        "SymmetricExceptWith",
        "Overlaps",
        "SetEquals",
        "CopyTo",
        "IndexOf",
        "RemoveAt",
        "Enqueue",
        "Dequeue",
        "Peek",
        "Push",
        "Pop",
        "Insert",
        "Clear",
        "Contains",
        "Remove",
        "Add",
    ];
    for m in STUB_METHODS {
        let sep = format!("_{m}");
        if let Some(cls) = name.strip_suffix(&sep) {
            return format!("{cls}::{m}");
        }
    }
    name.to_string()
}

/// [`iface_list_identity_scan_ir`] 的语义模式。
pub(super) enum IfaceScanMode {
    /// 返回命中下标（i32，未命中 -1）——IndexOf/Contains。
    Index,
    /// 命中即 `rt_list_remove_at`（返回 i1 是否删除）——Remove。
    Remove,
}

/// 接口元素 `List<Iface>` 的**对象身份扫描循环** IR 文本（stub 路径纯函数）。
///
/// 每次具体类→接口转换物化独立 fat 盒（`emit_make_iface` heap box），
/// `rt_list_index_of`/`rt_list_contains`/`rt_list_remove` 的指针相等对
/// 「同对象不同盒」恒判不等——C# 语义下接口引用相等 = 底层对象身份相等
/// （`emit_iface_equality` 同规则）。本循环逐元素解盒（fat[0] = obj）与
/// 查询方 obj 比对。`prefix` 隔离块/临时命名（同一 stub define 体内唯一）；
/// `item_addr` 为元素槽（槽内是 fat 盒地址，查询侧双重解盒与扫描侧对称）。
/// 文本以 `br label %{prefix}.hdr` 开头（拼入调用方的 entry 块尾部，phi 前驱
/// 为 stub 的 `entry` 块），以 `%{prefix}.res` phi 结尾。
pub(super) fn iface_list_identity_scan_ir(
    prefix: &str,
    handle: &str,
    item_addr: &str,
    mode: IfaceScanMode,
) -> String {
    let p = prefix;
    let (found_val, notfound_val, res_ty) = match mode {
        IfaceScanMode::Index => (format!("%{p}.iv"), "-1".to_string(), "i32"),
        IfaceScanMode::Remove => ("true".to_string(), "false".to_string(), "i1"),
    };
    let remove_ir = if matches!(mode, IfaceScanMode::Remove) {
        format!("  call void @rt_list_remove_at(ptr {handle}, i32 %{p}.iv)\n")
    } else {
        String::new()
    };
    format!(
        "  %{p}.out = alloca ptr, align 8\n\
         \x20 %{p}.qbox = load ptr, ptr {item_addr}\n\
         \x20 %{p}.q = load ptr, ptr %{p}.qbox\n\
         \x20 br label %{p}.hdr\n\
         {p}.hdr:\n\
         \x20 %{p}.iv = phi i32 [ 0, %entry ], [ %{p}.next, %{p}.adv ]\n\
         \x20 %{p}.cnt = call i32 @rt_list_size(ptr {handle})\n\
         \x20 %{p}.more = icmp slt i32 %{p}.iv, %{p}.cnt\n\
         \x20 br i1 %{p}.more, label %{p}.body, label %{p}.notfound\n\
         {p}.body:\n\
         \x20 call void @rt_list_get(ptr {handle}, i32 %{p}.iv, ptr %{p}.out)\n\
         \x20 %{p}.e = load ptr, ptr %{p}.out\n\
         \x20 %{p}.en = icmp eq ptr %{p}.e, null\n\
         \x20 br i1 %{p}.en, label %{p}.adv, label %{p}.ld\n\
         {p}.ld:\n\
         \x20 %{p}.eobj = load ptr, ptr %{p}.e\n\
         \x20 %{p}.hit = icmp eq ptr %{p}.eobj, %{p}.q\n\
         \x20 br i1 %{p}.hit, label %{p}.found, label %{p}.adv\n\
         {p}.found:\n\
         {remove_ir}\
         \x20 br label %{p}.join\n\
         {p}.adv:\n\
         \x20 %{p}.next = add i32 %{p}.iv, 1\n\
         \x20 br label %{p}.hdr\n\
         {p}.notfound:\n\
         \x20 br label %{p}.join\n\
         {p}.join:\n\
         \x20 %{p}.res = phi {res_ty} [ {found_val}, %{p}.found ], [ {notfound_val}, %{p}.notfound ]\n"
    )
}

impl<'a> FnEmitter<'a> {
    /// Returns LLVM IR stub for builtin collection methods, or None.
    /// Each function definition is wrapped with `$mangled = comdat any` so
    /// that `linkonce_odr` deduplication works on Windows COFF.
    pub(super) fn try_emit_stub(&self, name: &str) -> Option<String> {
        let name = &normalize_bare_stub_name(name);
        // Dictionary<K,V> stubs — match any Dictionary_* monomorphization
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_dict_kv(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.dict_stub(name)));
        }

        // ConcurrentDictionary<K,V> (RFC 024 M1)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_concurrent_dict_kv(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.concurrent_dict_stub(name)));
        }

        // ConcurrentQueue/ConcurrentBag/ConcurrentStack<T> (RFC 024 M2-M4)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_concurrent_single_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.concurrent_single_stub(name)));
        }

        // HashSet<T> (RFC Phase 5)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_set_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.set_stub(name)));
        }

        // Queue<T> (RFC Phase 5)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_queue_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.queue_stub(name)));
        }

        // Stack<T> — Phase 3-B: LIFO stack (C ABI backed)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_stack_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.stack_stub(name)));
        }

        // ListEnumerator<T> (any monomorphization)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_enumerator_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.enumerator_stub(name)));
        }

        // DictEnumerator<K,V> (RFC 025 M5: Dictionary traversal)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_dict_enumerator_kv(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.dict_enumerator_stub(name)));
        }

        // List<T> (any monomorphization)
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_list_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.list_stub(name)));
        }

        // Tensor<T> (any monomorphization) — RFC 021 Phase 1
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_tensor_elem(class_name).is_some() {
            return Some(
                self.stub_linkonce(name, super::builtin_tensor::try_emit_tensor_stub(name)?),
            );
        }

        // SortedDictionary<K,V> stubs — Phase 3-B: red-black tree ordered map
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_sorted_dict_kv(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.sorted_dict_stub(name)));
        }

        // LinkedList<T> stubs — Phase 3-B: doubly-linked list
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_linked_list_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.linked_list_stub(name)));
        }

        // LinkedListNode<T> stubs — Phase 3-B: node accessors
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_linked_list_node_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.linked_list_node_stub(name)));
        }

        // SortedSet<T> stubs — Phase 3-B: red-black tree ordered set
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_sorted_set_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.sorted_set_stub(name)));
        }

        // RFC 005 §2.2: Weak<T> facade — ctor + TryGet 均走 stub。
        // ctor 形如 `__ctor::Weak_T_1`（self + target）；stub 调用
        // rt_arc_weak_create(target) 返回 RtWeak* 槽位，store 到 offset 16
        // 处的 _target 字段（直接 store ptr，绕过 FieldSet ARC 维护——
        // 槽位不是 ArcHeader 对象，rt_arc_inc 会误读其首字段为 refcount）。
        // TryGet 形如 `Weak_T::TryGet` 或 `Weak_T_TryGet`；stub load 槽位后
        // 调 rt_arc_weak_try_get 返回目标指针（已 strong-retained）或 NULL。
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if parse_weak_elem(class_name).is_some() {
            return Some(self.stub_linkonce(name, self.weak_stub(name)));
        }

        // StringBuilder (Arc.Text facade, RFC 021 §4.3 M4) — non-generic.
        // Constructor intercept: allocate the runtime handle and store it at
        // offset 16 (the `_handle` slot). Method bodies are never emitted as
        // standalone functions — `emit_builtin_method_call` inlines them at
        // call sites. The ctor runs through the normal `emit_new` calloc +
        // `__ctor__StringBuilder` path, so this stub initializes the handle.
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        if class_name == "StringBuilder" && name.contains("__ctor") {
            let mangled = mangle_fn_name(name);
            return Some(format!(
                "${mangled} = comdat any\n\
                 define linkonce_odr void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_text_sb_new()\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            ));
        }

        // FileStream（标准库就绪 P0）：ctor + 虚表方法体均走 stub。
        // 必须用 class 名前缀匹配，禁止 `name.contains("FileStream")`——会误伤
        // 测试方法名如 `Tests::FileStream_WriteReadRoundtrip`。
        // 静态工厂 OpenRead/OpenWrite/Create 不 stub（保留真实 `new FileStream` 体）。
        let fs_class = name.strip_prefix("__ctor::").unwrap_or(name);
        let fs_class = fs_class.split("::").next().unwrap_or(fs_class);
        if fs_class == "FileStream" || fs_class.starts_with("FileStream_") {
            let method = name.split("::").last().unwrap_or(name);
            let method = method.strip_prefix("FileStream_").unwrap_or(method);
            if matches!(method, "OpenRead" | "OpenWrite" | "Create") {
                return None;
            }
            return Some(self.stub_linkonce(name, self.file_stream_stub(name)));
        }

        // TextBuffer（RFC 037 M-CE1）：ctor + 实例/静态方法走 rt_editor_* stub。
        let tb_class = name.strip_prefix("__ctor::").unwrap_or(name);
        let tb_class = tb_class.split("::").next().unwrap_or(tb_class);
        if tb_class == "TextBuffer" || tb_class.starts_with("TextBuffer_") {
            return Some(self.stub_linkonce(name, self.text_buffer_stub(name)));
        }

        None
    }

    /// TextBuffer ctor / 实例·静态方法 IR stub（`_handle` @ offset 16 为 i64）。
    fn text_buffer_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);

        if name.contains("OpenPath") && !name.contains("__ctor") {
            return format!(
                "define ptr @{mangled}(ptr %path) {{\n\
                 entry:\n\
                 \x20 %raw = call ptr @rt_editor_open_path(ptr %path)\n\
                 \x20 %bad = icmp eq ptr %raw, null\n\
                 \x20 br i1 %bad, label %fail, label %ok\n\
                 fail:\n\
                 \x20 ret ptr null\n\
                 ok:\n\
                 \x20 %obj = call ptr @calloc(i64 1, i64 24)\n\
                 \x20 store i32 1, ptr %obj\n\
                 \x20 %hi = ptrtoint ptr %raw to i64\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %obj, i32 16\n\
                 \x20 store i64 %hi, ptr %hp\n\
                 \x20 ret ptr %obj\n\
                 }}\n"
            );
        }

        if name.contains("__ctor") {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %raw = call ptr @rt_editor_create_empty()\n\
                 \x20 %hi = ptrtoint ptr %raw to i64\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store i64 %hi, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }

        let method = name
            .strip_prefix("TextBuffer::")
            .or_else(|| name.strip_prefix("TextBuffer_"))
            .unwrap_or(name);
        let method = method.strip_prefix("get_").unwrap_or(method);

        match method {
            "Length" => format!(
                "define i64 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %n = call i64 @rt_editor_length(ptr %handle)\n\
                 \x20 ret i64 %n\n\
                 }}\n"
            ),
            "LineCount" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %n = call i32 @rt_editor_line_count(ptr %handle)\n\
                 \x20 ret i32 %n\n\
                 }}\n"
            ),
            "IsMmapBacked" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %raw = call i32 @rt_editor_is_mmap_backed(ptr %handle)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "SetText" => format!(
                "define i1 @{mangled}(ptr %self, ptr %text) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %raw = call i32 @rt_editor_set_text(ptr %handle, ptr %text)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "LineText" => format!(
                "define ptr @{mangled}(ptr %self, i32 %line) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %s = call ptr @rt_editor_line_text(ptr %handle, i32 %line)\n\
                 \x20 ret ptr %s\n\
                 }}\n"
            ),
            "EnsureLines" => format!(
                "define i1 @{mangled}(ptr %self, i32 %first, i32 %last) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %raw = call i32 @rt_editor_ensure_lines(ptr %handle, i32 %first, i32 %last)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "Insert" => format!(
                "define i1 @{mangled}(ptr %self, i64 %offset, ptr %text) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %raw = call i32 @rt_editor_insert(ptr %handle, i64 %offset, ptr %text)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "Delete" => format!(
                "define i1 @{mangled}(ptr %self, i64 %offset, i64 %len) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 %raw = call i32 @rt_editor_delete(ptr %handle, i64 %offset, i64 %len)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "Dispose" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %hi = load i64, ptr %hp\n\
                 \x20 %handle = inttoptr i64 %hi to ptr\n\
                 \x20 call void @rt_editor_destroy(ptr %handle)\n\
                 \x20 store i64 0, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    /// RFC 005 §2.2: Weak<T> ctor + TryGet IR stub（直调与虚表共用）。
    /// `name` 形如 `__ctor::Weak_T_1`（ctor，self + target）或
    /// `Weak_T::TryGet` / `Weak_T_TryGet`（实例方法），或
    /// `Weak_T::GetWeakSlot` / `Weak_T_GetWeakSlot`（RFC 017 §2.6 边界登记
    /// 的槽位读取）。各分支均通过 offset 16 的 _target 字段读写 RtWeak* 槽位。
    fn weak_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);

        // ctor：`__ctor::Weak_T_1` → rt_arc_weak_create(target) 存入 offset 16
        if name.contains("__ctor") {
            // 1-arg ctor (self + target)
            return format!(
                "define void @{mangled}(ptr %self, ptr %target) {{\n\
                 entry:\n\
                 \x20 %slot = call ptr @rt_arc_weak_create(ptr %target)\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %slot, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }

        // TryGet / GetWeakSlot：`Weak_T::<m>` / `Weak_T_<m>`
        let method = name
            .strip_prefix("Weak_")
            .and_then(|rest| {
                // Strip element suffix and separator. Suffix may itself contain
                // underscores (e.g. user `Foo_Bar` class → `Weak_Foo_Bar`);
                // we only need to detect the method tail.
                rest.rsplit_once("::")
                    .or_else(|| rest.rsplit_once("_TryGet"))
                    .or_else(|| rest.rsplit_once("_GetWeakSlot"))
                    .map(|(_, m)| m)
            })
            .unwrap_or(name);
        if method == "TryGet" {
            return format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %slot = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_arc_weak_try_get(ptr %slot)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            );
        }
        if method == "GetWeakSlot" {
            // RFC 017 §2.6: 返回不透明 RtWeak* 槽位（供 ALC 边界登记；
            // 用户面不暴露）。slot 已由 ctor 写入 offset 16。
            return format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %slot = load ptr, ptr %hp\n\
                 \x20 ret ptr %slot\n\
                 }}\n"
            );
        }

        // 未知方法：空 stub（不应触达；typeck 已限定 Weak<T> 公开面只有 TryGet）
        format!("define void @{mangled}(ptr %self) {{\nentry:\n  ret void\n}}\n")
    }

    /// FileStream ctor / 实例方法 IR stub（vtable 与直调共用）。
    fn file_stream_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);

        // ctor：`__ctor::FileStream_2` / `__ctor_FileStream_2`（path + mode）
        if name.contains("__ctor") {
            if name.contains("FileStream_2") {
                return format!(
                    "define void @{mangled}(ptr %self, ptr %path, i32 %mode) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_file_stream_open(ptr %path, i32 %mode)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                );
            }
            return format!("define void @{mangled}(ptr %self) {{\nentry:\n  ret void\n}}\n");
        }

        // 方法名：FileStream::Read / FileStream_Read / FileStream::get_CanRead
        let method = name
            .strip_prefix("FileStream::")
            .or_else(|| name.strip_prefix("FileStream_"))
            .unwrap_or(name);
        let method = method.strip_prefix("get_").unwrap_or(method);

        match method {
            "CanRead" | "can_read" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %raw = call i32 @rt_file_stream_can_read(ptr %handle)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "CanWrite" | "can_write" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %raw = call i32 @rt_file_stream_can_write(ptr %handle)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "CanSeek" | "can_seek" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %raw = call i32 @rt_file_stream_can_seek(ptr %handle)\n\
                 \x20 %b = icmp ne i32 %raw, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "Length" => format!(
                "define i64 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %n = call i64 @rt_file_stream_get_length(ptr %handle)\n\
                 \x20 ret i64 %n\n\
                 }}\n"
            ),
            "Position" | "_getPosition" => format!(
                "define i64 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %n = call i64 @rt_file_stream_get_position(ptr %handle)\n\
                 \x20 ret i64 %n\n\
                 }}\n"
            ),
            "set_Position" | "_setPosition" => format!(
                "define void @{mangled}(ptr %self, i64 %value) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_file_stream_set_position(ptr %handle, i64 %value)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Read" => format!(
                "define i32 @{mangled}(ptr %self, ptr %buffer, i32 %offset, i32 %count) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %n = call i32 @rt_file_stream_read(ptr %handle, ptr %buffer, i32 %offset, i32 %count)\n\
                 \x20 ret i32 %n\n\
                 }}\n"
            ),
            "Write" => format!(
                "define void @{mangled}(ptr %self, ptr %buffer, i32 %offset, i32 %count) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_file_stream_write(ptr %handle, ptr %buffer, i32 %offset, i32 %count)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Seek" => format!(
                "define i64 @{mangled}(ptr %self, i64 %offset, i32 %origin) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %n = call i64 @rt_file_stream_seek(ptr %handle, i64 %offset, i32 %origin)\n\
                 \x20 ret i64 %n\n\
                 }}\n"
            ),
            "SetLength" => format!(
                "define void @{mangled}(ptr %self, i64 %value) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_file_stream_set_length(ptr %handle, i64 %value)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Flush" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_file_stream_flush(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            // 真异步 stub（调用点经 try_emit_file_stream_method 拦截直射 ABI；
            // 此定义保持签名正确性：返回 Pending Task*，CT 省略为 null）。
            "ReadAsync" => format!(
                "define ptr @{mangled}(ptr %self, ptr %buffer, i32 %offset, i32 %count, ptr %ct) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %task = call ptr @rt_file_stream_read_async(ptr %handle, ptr %buffer, i32 %offset, i32 %count, ptr %ct)\n\
                 \x20 ret ptr %task\n\
                 }}\n"
            ),
            "WriteAsync" => format!(
                "define ptr @{mangled}(ptr %self, ptr %buffer, i32 %offset, i32 %count, ptr %ct) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %task = call ptr @rt_file_stream_write_async(ptr %handle, ptr %buffer, i32 %offset, i32 %count, ptr %ct)\n\
                 \x20 ret ptr %task\n\
                 }}\n"
            ),
            "FlushAsync" => format!(
                "define ptr @{mangled}(ptr %self, ptr %ct) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %task = call ptr @rt_file_stream_flush_async(ptr %handle, ptr %ct)\n\
                 \x20 ret ptr %task\n\
                 }}\n"
            ),
            "Dispose" | "_closeHandle" | "Close" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_file_stream_close(ptr %handle)\n\
                 \x20 store ptr null, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    fn dict_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let (k_suf, v_suf) = parse_dict_kv(class_name).unwrap_or(("string".into(), "int".into()));
        let k_ty = dict_kv_llvm_ty(&k_suf, self.layouts);
        let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
        // RFC 004 M2：用户类型键使用 trampoline 调用 `K_GetHashCode`/`K_Equals`
        // 静态方法（零装箱），基元/string 键沿用 runtime 内置 hash/eq 函数。
        let (hash_fn, eq_fn) = if dict_kv_is_user_type(&k_suf, self.layouts) {
            (dict_user_hash_fn(&k_suf), dict_user_eq_fn(&k_suf))
        } else {
            (
                dict_hash_fn(&k_suf).to_string(),
                dict_eq_fn(&k_suf).to_string(),
            )
        };

        if name.contains("__ctor") {
            // capacity ctor → create + rt_dict_ensure_capacity（H2 facade 预分配）。
            let rest = name.strip_prefix("__ctor::").unwrap_or(name);
            let arity = rest.rsplit_once('_').and_then(|(_, suf)| {
                if !suf.is_empty() && suf.chars().all(|c| c.is_ascii_digit()) {
                    suf.parse::<u32>().ok()
                } else {
                    None
                }
            });
            return match arity {
                Some(1) => format!(
                    "define void @{mangled}(ptr %self, i32 %capacity) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_dict_create(ptr {hash_fn}, ptr {eq_fn})\n\
                     \x20 call void @rt_dict_ensure_capacity(ptr %handle, i32 %capacity)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
                _ => format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_dict_create(ptr {hash_fn}, ptr {eq_fn})\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
            };
        }
        // `Class::Method` / `Class::Method_ty`（重载后缀）→ 取方法名。
        let method_raw = name.split("::").last().unwrap_or(name);
        let method = method_raw
            .split_once('_')
            .filter(|(base, _)| {
                // 保留 get_Item / set_Item；其余 Method_ty 取 Method。
                *base != "get" && *base != "set"
            })
            .map(|(base, _)| base)
            .unwrap_or(method_raw);
        match method {
            "set_Item" => {
                // Box key/value scalars into ptr; pass through string/ptr unchanged.
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                let (val_ir, val_arg) = if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&v_suf, self.layouts, "%value", "v");
                    (ir, reg)
                } else {
                    (String::new(), "%value".to_string())
                };
                let retain = if list_elem_is_ref(&v_suf, self.layouts) {
                    format!("  call void @rt_arc_inc(ptr {val_arg})\n")
                } else {
                    String::new()
                };
                format!(
                    "define void @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     {retain}\
                     \x20 call void @rt_dict_set(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "get_Item" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                // Unbox the returned ptr back to the value type when scalar.
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %rp = call ptr @rt_dict_get(ptr %handle, ptr {key_arg})\n\
                         {unbox_ir}\
                         \x20 ret {v_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    // H1: class 值 get_Item 与 List 同构 retain（string/标量不加）。
                    let retain = if list_elem_is_ref(&v_suf, self.layouts) {
                        "  call void @rt_arc_inc(ptr %r)\n"
                    } else {
                        ""
                    };
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %r = call ptr @rt_dict_get(ptr %handle, ptr {key_arg})\n\
                         {retain}\
                         \x20 ret {v_ty} %r\n\
                         }}\n"
                    )
                }
            }
            "ContainsKey" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key) {{\n\
                     entry:\n\
                     {key_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_dict_contains(ptr %handle, ptr {key_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "ContainsValue" => {
                let (val_ir, val_arg) = if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&v_suf, self.layouts, "%value", "v");
                    (ir, reg)
                } else {
                    (String::new(), "%value".to_string())
                };
                // RFC 038 M2：ContainsValue 的 user-type value 相等性对齐
                // C# EqualityComparer<TValue>.Default 语义——仅当 value 类型实现了
                // Equals 时才引用 @__dict_eq_{V}（值相等）；否则传 null 走 runtime
                // 引用相等，避免对未实现 Equals 的类型引用未定义 trampoline。
                let eq_fn = if dict_kv_is_user_type(&v_suf, self.layouts) {
                    if dict_value_has_equals(&v_suf, self.layouts) {
                        dict_user_eq_fn(&v_suf)
                    } else {
                        "null".to_string()
                    }
                } else {
                    dict_eq_fn(&v_suf).to_string()
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {v_ty} %value) {{\n\
                     entry:\n\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_dict_contains_value(ptr %handle, ptr {val_arg}, ptr {eq_fn})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Remove" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key) {{\n\
                     entry:\n\
                     {key_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_dict_remove(ptr %handle, ptr {key_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Clear" => {
                format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 call void @rt_dict_clear(ptr %handle)\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "Count" | "get_Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_dict_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            // 禁静默 `ret i1 false`：缺 out 槽签名会在 IndirectCall/未 inline 路径
            // 造成 ABI UB（0xc0000005）或假绿。与 emit_builtin 同源 rt_dict_try_get_value。
            "TryGetValue" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_dict_try_get_value(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         {unbox_ir}\
                         \x20 store {v_ty} {v_result}, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_dict_try_get_value(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         \x20 store ptr %rp, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                }
            }
            // Add = rt_dict_try_add（bool）；与 emit_builtin 同源，禁缺臂假绿。
            "Add" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                let (val_ir, val_arg) = if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&v_suf, self.layouts, "%value", "v");
                    (ir, reg)
                } else {
                    (String::new(), "%value".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_dict_try_add(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Keys" | "get_Keys" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_dict_keys(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "Values" | "get_Values" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_dict_values(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            // GetEnumerator 需 itable 装配（emit_builtin 热路径）；链接兜底禁静默假绿。
            _ => format!(
                "define void @{mangled}() {{\n\
                 entry:\n\
                 \x20 call void @rt_panic(ptr @__arc_stub_unimplemented)\n\
                 \x20 unreachable\n\
                 }}\n"
            ),
        }
    }

    /// RFC 024 M1: ConcurrentDictionary<K,V> stub generation.
    /// Constructor + Stable 方法链接 stub 真转发 `rt_concurrent_dict_*`
    ///（与 Dictionary / emit_builtin 同构；禁 Stable 面 panic 半物化）。
    fn concurrent_dict_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let (k_suf, v_suf) =
            parse_concurrent_dict_kv(class_name).unwrap_or(("string".into(), "int".into()));
        let k_ty = dict_kv_llvm_ty(&k_suf, self.layouts);
        let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
        let (hash_fn, eq_fn) = if dict_kv_is_user_type(&k_suf, self.layouts) {
            (dict_user_hash_fn(&k_suf), dict_user_eq_fn(&k_suf))
        } else {
            (
                dict_hash_fn(&k_suf).to_string(),
                dict_eq_fn(&k_suf).to_string(),
            )
        };

        if name.contains("__ctor") {
            // emit_new：无参 → `__ctor::Class`；有参 → `__ctor::Class_<arity>`。
            // 旧 stub 一律 `(ptr, i32)`，导致 `new ConcurrentDictionary()` 少传
            // concurrencyLevel（ABI UB）——UnitTest TryRemove 等在 QIF 宿主下红。
            let rest = name.strip_prefix("__ctor::").unwrap_or(name);
            let arity = rest.rsplit_once('_').and_then(|(_, suf)| {
                if !suf.is_empty() && suf.chars().all(|c| c.is_ascii_digit()) {
                    suf.parse::<u32>().ok()
                } else {
                    None
                }
            });
            return match arity {
                None => format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_concurrent_dict_create(ptr {hash_fn}, ptr {eq_fn}, i32 31)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
                Some(1) => format!(
                    "define void @{mangled}(ptr %self, i32 %concurrencyLevel) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_concurrent_dict_create_level(ptr {hash_fn}, ptr {eq_fn}, i32 %concurrencyLevel)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
                Some(2) => format!(
                    "define void @{mangled}(ptr %self, i32 %concurrencyLevel, i32 %capacity) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_concurrent_dict_create_level_cap(ptr {hash_fn}, ptr {eq_fn}, i32 %concurrencyLevel, i32 %capacity)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
                Some(_) => format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_concurrent_dict_create(ptr {hash_fn}, ptr {eq_fn}, i32 31)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
            };
        }
        // Stable 面链接 stub = 真 rt_* 转发（与 emit_builtin / Dictionary TryGetValue 刀同构）。
        // 禁静默 0/null；未挂 Stable 的 Values/ToArray/Func 重载 → panic。
        let method_raw = name.split("::").last().unwrap_or(name);
        let method = method_raw
            .split_once('_')
            .filter(|(base, _)| *base != "get" && *base != "set")
            .map(|(base, _)| base)
            .unwrap_or(method_raw);

        let key_box = |param: &str, prefix: &str| -> (String, String) {
            if dict_kv_is_scalar(&k_suf, self.layouts) {
                dict_kv_scalar_to_ptr(&k_suf, self.layouts, param, prefix)
            } else {
                (String::new(), param.to_string())
            }
        };
        let val_box = |param: &str, prefix: &str| -> (String, String) {
            if dict_kv_is_scalar(&v_suf, self.layouts) {
                dict_kv_scalar_to_ptr(&v_suf, self.layouts, param, prefix)
            } else {
                (String::new(), param.to_string())
            }
        };
        let panic_void = |sig: &str| {
            format!(
                "define void @{mangled}{sig} {{\n\
                 entry:\n\
                 \x20 call void @rt_panic(ptr @__arc_stub_unimplemented)\n\
                 \x20 unreachable\n\
                 }}\n"
            )
        };

        match method {
            "TryAdd" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                let (val_ir, val_arg) = val_box("%value", "v");
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_concurrent_dict_try_add(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "ContainsKey" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key) {{\n\
                     entry:\n\
                     {key_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_concurrent_dict_contains(ptr %handle, ptr {key_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "TryGetValue" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_concurrent_dict_try_get(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         {unbox_ir}\
                         \x20 store {v_ty} {v_result}, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_concurrent_dict_try_get(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         \x20 store ptr %rp, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                }
            }
            "set_Item" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                let (val_ir, val_arg) = val_box("%value", "v");
                let retain = if list_elem_is_ref(&v_suf, self.layouts) {
                    format!("  call void @rt_arc_inc(ptr {val_arg})\n")
                } else {
                    String::new()
                };
                format!(
                    "define void @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     {retain}\
                     \x20 call void @rt_concurrent_dict_set(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "Clear" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_concurrent_dict_clear(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "GetValueOrDefault" | "get_Item" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %rp = call ptr @rt_concurrent_dict_get_or_default(ptr %handle, ptr {key_arg})\n\
                         {unbox_ir}\
                         \x20 ret {v_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    let retain = if list_elem_is_ref(&v_suf, self.layouts) {
                        "  call void @rt_arc_inc(ptr %r)\n"
                    } else {
                        ""
                    };
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %r = call ptr @rt_concurrent_dict_get_or_default(ptr %handle, ptr {key_arg})\n\
                         {retain}\
                         \x20 ret {v_ty} %r\n\
                         }}\n"
                    )
                }
            }
            "TryRemove" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_concurrent_dict_try_remove(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         {unbox_ir}\
                         \x20 store {v_ty} {v_result}, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define i1 @{mangled}(ptr %self, {k_ty} %key, ptr %out) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = alloca ptr, align 8\n\
                         \x20 %r = call i32 @rt_concurrent_dict_try_remove(ptr %handle, ptr {key_arg}, ptr %slot)\n\
                         \x20 %rp = load ptr, ptr %slot\n\
                         \x20 store ptr %rp, ptr %out\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                }
            }
            "GetOrAdd" => {
                // Value 重载（Stable）；Func trampoline 已撤面。
                let (key_ir, key_arg) = key_box("%key", "k");
                let (val_ir, val_arg) = val_box("%value", "vin");
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "vout");
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                         entry:\n\
                         {key_ir}\
                         {val_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %rp = call ptr @rt_concurrent_dict_get_or_add_val(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                         {unbox_ir}\
                         \x20 ret {v_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                         entry:\n\
                         {key_ir}\
                         {val_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %r = call ptr @rt_concurrent_dict_get_or_add_val(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                         \x20 ret {v_ty} %r\n\
                         }}\n"
                    )
                }
            }
            "get_Count" | "Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_concurrent_dict_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "get_IsEmpty" | "IsEmpty" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %h = load ptr, ptr %hp\n\
                 \x20 %c = call i32 @rt_concurrent_dict_count(ptr %h)\n\
                 \x20 %z = icmp eq i32 %c, 0\n\
                 \x20 ret i1 %z\n\
                 }}\n"
            ),
            "TryUpdate" => {
                let (key_ir, key_arg) = key_box("%key", "k");
                let (new_ir, new_arg) = val_box("%newValue", "n");
                let (cmp_ir, cmp_arg) = val_box("%comparisonValue", "c");
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key, {v_ty} %newValue, {v_ty} %comparisonValue) {{\n\
                     entry:\n\
                     {key_ir}\
                     {new_ir}\
                     {cmp_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_concurrent_dict_try_update(ptr %handle, ptr {key_arg}, ptr {new_arg}, ptr {cmp_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Keys" | "get_Keys" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_concurrent_dict_keys(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            // Values / ToArray 未挂 Arc Stable（标量布局）；禁静默 null。
            "Values" | "get_Values" | "ToArray" | "AddOrUpdate" => panic_void("(ptr %self)"),
            _ => panic_void("(ptr %self)"),
        }
    }

    /// RFC 024 M2-M4: Single-generic concurrent collection stubs (Queue/Bag/Stack).
    fn concurrent_single_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_concurrent_single_elem(class_name).unwrap_or("int");
        let _elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);

        let abi_prefix = if class_name.starts_with("ConcurrentQueue_") {
            "rt_concurrent_queue"
        } else if class_name.starts_with("ConcurrentBag_") {
            "rt_concurrent_bag"
        } else if class_name.starts_with("ConcurrentStack_") {
            "rt_concurrent_stack"
        } else if class_name.starts_with("BlockingCollection_") {
            "rt_blocking_collection"
        } else {
            return format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n");
        };

        if name.contains("__ctor") {
            if class_name.starts_with("BlockingCollection_") {
                // arity 由 mangle 后缀区分：`BlockingCollection_int_1` vs `_2`。
                let is_arity2 = {
                    let rest = name.strip_prefix("__ctor::").unwrap_or(name);
                    rest.ends_with("_2")
                };
                if is_arity2 {
                    // emit_new 拦截主路径；本 stub 仅保证符号可链（默认 Queue kind）。
                    return format!(
                        "define void @{mangled}(ptr %self, ptr %collection, i32 %boundedCapacity) {{\n\
                         entry:\n\
                         \x20 %inner_addr = getelementptr inbounds i8, ptr %collection, i32 16\n\
                         \x20 %inner = load ptr, ptr %inner_addr\n\
                         \x20 %handle = call ptr @rt_blocking_collection_create_with(ptr %inner, i32 0, i32 %boundedCapacity, i32 0)\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 store ptr %handle, ptr %hp\n\
                         \x20 ret void\n\
                         }}\n"
                    );
                }
                // facade：BlockingCollection(int boundedCapacity)；strategy 固定 0（Block）。
                return format!(
                    "define void @{mangled}(ptr %self, i32 %boundedCapacity) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_blocking_collection_create(i32 %boundedCapacity, i32 0)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                );
            }
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @{abi_prefix}_create()\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n")
    }

    fn list_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_list_elem(class_name).unwrap_or("int");
        let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
        let elem_size = list_elem_size(elem_suf, self.layouts);
        let eq_fn = match list_eq_fn(elem_suf) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_inc = match list_arc_inc_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_dec = match list_arc_dec_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let is_ctor = name.contains("__ctor");
        let method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_list_create(i32 {elem_size}, {eq_fn}, {arc_inc}, {arc_dec})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        match method {
            "Add" => {
                // 引用元素走 rt_list_push（ARC）；值类型直降 RtList（RFC 005）。
                if list_arc_inc_fn(elem_suf, self.layouts).is_some() {
                    format!(
                        "define void @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         \x20 call void @rt_list_push(ptr %handle, ptr %item_addr)\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define void @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %size_addr = getelementptr inbounds i8, ptr %handle, i32 8\n\
                         \x20 %size = load i32, ptr %size_addr\n\
                         \x20 %cap_addr = getelementptr inbounds i8, ptr %handle, i32 12\n\
                         \x20 %cap = load i32, ptr %cap_addr\n\
                         \x20 %need_grow = icmp uge i32 %size, %cap\n\
                         \x20 br i1 %need_grow, label %grow, label %ready\n\
                         grow:\n\
                         \x20 %needed = add i32 %size, 1\n\
                         \x20 call void @rt_list_ensure_capacity(ptr %handle, i32 %needed)\n\
                         \x20 br label %ready\n\
                         ready:\n\
                         \x20 %data_addr = getelementptr inbounds i8, ptr %handle, i32 0\n\
                         \x20 %data = load ptr, ptr %data_addr\n\
                         \x20 %size2 = load i32, ptr %size_addr\n\
                         \x20 %byte_off = mul i32 %size2, {elem_size}\n\
                         \x20 %slot = getelementptr inbounds i8, ptr %data, i32 %byte_off\n\
                         \x20 store {elem_ty} %item, ptr %slot\n\
                         \x20 %new_size = add i32 %size2, 1\n\
                         \x20 store i32 %new_size, ptr %size_addr\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                }
            }
            "get_Item" => {
                let retain = if list_arc_inc_fn(elem_suf, self.layouts).is_some() {
                    "  call void @rt_arc_inc(ptr %r)\n"
                } else {
                    ""
                };
                format!(
                    "define {elem_ty} @{mangled}(ptr %self, i32 %index) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %data_addr = getelementptr inbounds i8, ptr %handle, i32 0\n\
                     \x20 %data = load ptr, ptr %data_addr\n\
                     \x20 %size_addr = getelementptr inbounds i8, ptr %handle, i32 8\n\
                     \x20 %size = load i32, ptr %size_addr\n\
                     \x20 %in_bounds = icmp ult i32 %index, %size\n\
                     \x20 br i1 %in_bounds, label %ok, label %oob\n\
                     oob:\n\
                     \x20 call void @rt_panic(ptr @__arc_list_oob)\n\
                     \x20 unreachable\n\
                     ok:\n\
                     \x20 %byte_off = mul i32 %index, {elem_size}\n\
                     \x20 %slot = getelementptr inbounds i8, ptr %data, i32 %byte_off\n\
                     \x20 %r = load {elem_ty}, ptr %slot\n\
                     {retain}\
                     \x20 ret {elem_ty} %r\n\
                     }}\n"
                )
            }
            "set_Item" => {
                // 引用元素保留 rt_list_set（ARC）；值类型直访 store。
                if list_arc_inc_fn(elem_suf, self.layouts).is_some() {
                    format!(
                        "define void @{mangled}(ptr %self, i32 %index, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         \x20 call void @rt_list_set(ptr %handle, i32 %index, ptr %item_addr)\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define void @{mangled}(ptr %self, i32 %index, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %slot = call ptr @rt_list_at(ptr %handle, i32 %index)\n\
                         \x20 store {elem_ty} %item, ptr %slot\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                }
            }
            "get_Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_size(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "Contains" => {
                // 接口元素特化：对象身份扫描（fat 盒每次转换新建，指针相等
                // 恒判不等——与内联路径 emit_iface_list_identity_index 同语义）。
                if self.layouts.interfaces.contains_key(elem_suf) {
                    let scan = iface_list_identity_scan_ir(
                        "ctn",
                        "%handle",
                        "%item_addr",
                        IfaceScanMode::Index,
                    );
                    return format!(
                        "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         {scan}\
                         \x20 %b = icmp ne i32 %ctn.res, -1\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    );
                }
                format!(
                    "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %item_addr = alloca {elem_ty}\n\
                     \x20 store {elem_ty} %item, ptr %item_addr\n\
                     \x20 %r = call i32 @rt_list_contains(ptr %handle, ptr %item_addr)\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "IndexOf" => {
                // 接口元素特化：对象身份扫描（同 Contains）。
                if self.layouts.interfaces.contains_key(elem_suf) {
                    let scan = iface_list_identity_scan_ir(
                        "iox",
                        "%handle",
                        "%item_addr",
                        IfaceScanMode::Index,
                    );
                    return format!(
                        "define i32 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         {scan}\
                         \x20 ret i32 %iox.res\n\
                         }}\n"
                    );
                }
                format!(
                    "define i32 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %item_addr = alloca {elem_ty}\n\
                     \x20 store {elem_ty} %item, ptr %item_addr\n\
                     \x20 %r = call i32 @rt_list_index_of(ptr %handle, ptr %item_addr)\n\
                     \x20 ret i32 %r\n\
                     }}\n"
                )
            }
            "Insert" => format!(
                "define void @{mangled}(ptr %self, i32 %index, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 call void @rt_list_insert(ptr %handle, i32 %index, ptr %item_addr)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "RemoveAt" => format!(
                "define void @{mangled}(ptr %self, i32 %index) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_remove_at(ptr %handle, i32 %index)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Clear" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_clear(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Remove" => {
                // 接口元素特化：对象身份扫描，命中即 remove_at（同 Contains/）。
                if self.layouts.interfaces.contains_key(elem_suf) {
                    let scan = iface_list_identity_scan_ir(
                        "rmv",
                        "%handle",
                        "%item_addr",
                        IfaceScanMode::Remove,
                    );
                    return format!(
                        "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         {scan}\
                         \x20 ret i1 %rmv.res\n\
                         }}\n"
                    );
                }
                format!(
                    "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %item_addr = alloca {elem_ty}\n\
                     \x20 store {elem_ty} %item, ptr %item_addr\n\
                     \x20 %r = call i32 @rt_list_remove(ptr %handle, ptr %item_addr)\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Reverse" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_reverse(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Find" => format!(
                "define {elem_ty} @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %out = alloca {elem_ty}\n\
                 \x20 %found = call i32 @rt_list_find_get(ptr %handle, ptr %pred, ptr %out)\n\
                 \x20 %r = load {elem_ty}, ptr %out\n\
                 \x20 ret {elem_ty} %r\n\
                 }}\n"
            ),
            "FindAll" => format!(
                "define ptr @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %new_handle = call ptr @rt_list_find_all(ptr %handle, ptr %pred)\n\
                 \x20 %obj = call ptr @malloc(i64 24)\n\
                 \x20 store i32 1, ptr %obj\n\
                 \x20 %vp = getelementptr inbounds i8, ptr %obj, i32 8\n\
                 \x20 store ptr null, ptr %vp\n\
                 \x20 %hp2 = getelementptr inbounds i8, ptr %obj, i32 16\n\
                 \x20 store ptr %new_handle, ptr %hp2\n\
                 \x20 ret ptr %obj\n\
                 }}\n"
            ),
            "Exists" => format!(
                "define i1 @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_exists(ptr %handle, ptr %pred)\n\
                 \x20 %b = icmp ne i32 %r, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "FindIndex" => format!(
                "define i32 @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_find_index(ptr %handle, ptr %pred)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "FindLastIndex" => format!(
                "define i32 @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_find_last_index(ptr %handle, ptr %pred)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "TrueForAll" => format!(
                "define i1 @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_true_for_all(ptr %handle, ptr %pred)\n\
                 \x20 %b = icmp ne i32 %r, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "LastIndexOf" => format!(
                "define i32 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call i32 @rt_list_last_index_of(ptr %handle, ptr %item_addr)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "ForEach" => format!(
                "define void @{mangled}(ptr %self, ptr %action) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_for_each(ptr %handle, ptr %action)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "RemoveAll" => format!(
                "define i32 @{mangled}(ptr %self, ptr %pred) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_list_remove_all(ptr %handle, ptr %pred)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "Sort_" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_sort_default(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            m if m.starts_with("Sort_") => format!(
                "define void @{mangled}(ptr %self, ptr %cmp) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_sort(ptr %handle, ptr %cmp)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "ToArray" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_list_to_array(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "CopyTo" => format!(
                "define void @{mangled}(ptr %self, ptr %array, i32 %start) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_list_copy_to(ptr %handle, ptr %array, i32 %start)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "AddRange" => format!(
                "define void @{mangled}(ptr %self, ptr %items) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %obj = load ptr, ptr %items\n\
                 \x20 %ihp = getelementptr inbounds i8, ptr %obj, i32 16\n\
                 \x20 %ihandle = load ptr, ptr %ihp\n\
                 \x20 call void @rt_list_add_range_list(ptr %handle, ptr %ihandle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "GetEnumerator" => {
                // Emit itable + ListEnumerator method stubs inline.
                // ListEnumerator_<suf> is not instantiated by typeck when only
                // IEnumerator<T> is used in user code, so emit the itable and
                // method implementations here to make the enumerator self-contained.
                let movenext_name = format!("ListEnumerator_{elem_suf}::MoveNext");
                let current_name = format!("ListEnumerator_{elem_suf}::get_Current");
                let movenext_stub =
                    self.stub_linkonce(&movenext_name, self.enumerator_stub(&movenext_name));
                let current_stub =
                    self.stub_linkonce(&current_name, self.enumerator_stub(&current_name));
                let movenext_mangled = mangle_fn_name(&movenext_name);
                let current_mangled = mangle_fn_name(&current_name);
                let itable_decl = format!(
                    "@.itable.ListEnumerator_{elem_suf}_IEnumerator_{elem_suf} = \
                     private constant [2 x ptr] [ptr @{movenext_mangled}, ptr @{current_mangled}]\n\n"
                );
                format!(
                    "{itable_decl}\
                     {movenext_stub}\n\
                     {current_stub}\n\
                     define ptr @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %cnt = call i32 @rt_list_size(ptr %handle)\n\
                     \x20 %obj = call ptr @malloc(i64 32)\n\
                     \x20 store i32 1, ptr %obj\n\
                     \x20 %vp = getelementptr inbounds i8, ptr %obj, i32 8\n\
                     \x20 store ptr @.itable.ListEnumerator_{elem_suf}_IEnumerator_{elem_suf}, ptr %vp\n\
                     \x20 %shp = getelementptr inbounds i8, ptr %obj, i32 16\n\
                     \x20 store ptr %handle, ptr %shp\n\
                     \x20 %ip = getelementptr inbounds i8, ptr %obj, i32 24\n\
                     \x20 store i32 -1, ptr %ip\n\
                     \x20 %cp = getelementptr inbounds i8, ptr %obj, i32 28\n\
                     \x20 store i32 %cnt, ptr %cp\n\
                     \x20 %fat = call ptr @malloc(i64 16)\n\
                     \x20 store ptr %obj, ptr %fat\n\
                     \x20 %fatvt = getelementptr inbounds i8, ptr %fat, i32 8\n\
                     \x20 store ptr @.itable.ListEnumerator_{elem_suf}_IEnumerator_{elem_suf}, ptr %fatvt\n\
                     \x20 ret ptr %fat\n\
                     }}\n"
                )
            }
            "InsertRange" => format!(
                // 源 `items` 为 IEnumerable<T> 胖指针（{ obj, vtable }）。解包 obj 后
                // 读 _handle(offset 16) 得源 List handle。rt_list_buffer_and_size 在任一
                // out 参数为 NULL 时提前返回（不写其它 out），故必须一次性传入两个有效
                // out 参数，否则 %buf/%n 为未初始化垃圾，rt_list_insert_range 会提前返回。
                "define void @{mangled}(ptr %self, i32 %index, ptr %items) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %obj = load ptr, ptr %items\n\
                 \x20 %ihp = getelementptr inbounds i8, ptr %obj, i32 16\n\
                 \x20 %ihandle = load ptr, ptr %ihp\n\
                 \x20 %bufp = alloca ptr\n\
                 \x20 %cntp = alloca i32\n\
                 \x20 call void @rt_list_buffer_and_size(ptr %ihandle, ptr %bufp, ptr %cntp)\n\
                 \x20 %buf = load ptr, ptr %bufp\n\
                 \x20 %n = load i32, ptr %cntp\n\
                 \x20 call void @rt_list_insert_range(ptr %handle, i32 %index, ptr %buf, i32 %n)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "GetRange" => format!(
                "define ptr @{mangled}(ptr %self, i32 %index, i32 %count) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %rh = call ptr @rt_list_get_range(ptr %handle, i32 %index, i32 %count)\n\
                 \x20 %obj = call ptr @malloc(i64 24)\n\
                 \x20 store i32 1, ptr %obj\n\
                 \x20 %vp = getelementptr inbounds i8, ptr %obj, i32 8\n\
                 \x20 store ptr null, ptr %vp\n\
                 \x20 %hp2 = getelementptr inbounds i8, ptr %obj, i32 16\n\
                 \x20 store ptr %rh, ptr %hp2\n\
                 \x20 ret ptr %obj\n\
                 }}\n"
            ),
            m if m == "BinarySearch" || m.starts_with("BinarySearch_") => {
                // 重载判别：单态化名为 `BinarySearch_{elem_suf}`（单参）vs
                // `BinarySearch_{elem_suf}_IComparer_{elem_suf}`（comparer）。
                // 旧判定 starts_with("BinarySearch_") 把单参也归入 comparer → 3 参
                // stub 与调用点 ABI 错位（UnitTest BinarySearch 0xc0000005 根因之一）。
                // key 形状与普通 fallback 一致（值类型按值 + stub 内 alloca），
                // 保证两条路径共享同一 ABI。
                let has_cmp = match m.strip_prefix("BinarySearch_") {
                    Some(rest) => rest.len() > elem_suf.len(),
                    None => false,
                };
                if has_cmp {
                    format!(
                        "define i32 @{mangled}(ptr %self, {elem_ty} %key, ptr %cmp) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %key_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %key, ptr %key_addr\n\
                         \x20 %r = call i32 @rt_list_binary_search_cmp(ptr %handle, ptr %key_addr, ptr %cmp)\n\
                         \x20 ret i32 %r\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define i32 @{mangled}(ptr %self, {elem_ty} %key) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %key_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %key, ptr %key_addr\n\
                         \x20 %r = call i32 @rt_list_binary_search(ptr %handle, ptr %key_addr)\n\
                         \x20 ret i32 %r\n\
                         }}\n"
                    )
                }
            }
            // Do not `ret void` here: call sites may expect i32/i1 and would read
            // garbage / false when a new List facade method lacks a stub arm.
            _ => format!(
                "define void @{mangled}() {{\n\
                 entry:\n\
                 \x20 call void @rt_panic(ptr @__arc_stub_unimplemented)\n\
                 \x20 unreachable\n\
                 }}\n"
            ),
        }
    }

    /// Generate LLVM IR stub for ListEnumerator<T> methods (MoveNext, Current).
    /// Object layout: [0..8) refcount | [8..16) vtable | [16..24) _sourceHandle (ptr)
    ///                 [24..28) _index (i32) | [28..32) _count (i32)
    fn enumerator_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_enumerator_elem(class_name).unwrap_or("int");
        let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
        let method = name.split("::").nth(1).unwrap_or("");

        match method {
            "MoveNext" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %ip = getelementptr inbounds i8, ptr %self, i32 24\n\
                 \x20 %idx = load i32, ptr %ip\n\
                 \x20 %next = add i32 %idx, 1\n\
                 \x20 store i32 %next, ptr %ip\n\
                 \x20 %cp = getelementptr inbounds i8, ptr %self, i32 28\n\
                 \x20 %cnt = load i32, ptr %cp\n\
                 \x20 %r = icmp slt i32 %next, %cnt\n\
                 \x20 ret i1 %r\n\
                 }}\n"
            ),
            "get_Current" => {
                let retain = if list_arc_inc_fn(elem_suf, self.layouts).is_some() {
                    "  call void @rt_arc_inc(ptr %r)\n"
                } else {
                    ""
                };
                format!(
                    "define {elem_ty} @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %ip = getelementptr inbounds i8, ptr %self, i32 24\n\
                     \x20 %idx = load i32, ptr %ip\n\
                     \x20 %out = alloca {elem_ty}\n\
                     \x20 call void @rt_list_get(ptr %handle, i32 %idx, ptr %out)\n\
                     \x20 %r = load {elem_ty}, ptr %out\n\
                     {retain}\
                     \x20 ret {elem_ty} %r\n\
                     }}\n"
                )
            }
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    /// Generate LLVM IR stub for DictEnumerator<K,V> methods (MoveNext, Current).
    /// Object layout: [0..8) refcount | [8..16) itable | [16..24) _handle (ptr to RtDictEnumerator)
    pub(crate) fn dict_enumerator_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let (k_suf, v_suf) =
            parse_dict_enumerator_kv(class_name).unwrap_or(("string".into(), "int".into()));
        let k_ty = dict_kv_llvm_ty(&k_suf, self.layouts);
        let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
        let k_is_scalar = dict_kv_is_scalar(&k_suf, self.layouts);
        let v_is_scalar = dict_kv_is_scalar(&v_suf, self.layouts);
        let method = name.split("::").nth(1).unwrap_or("");

        match method {
            "MoveNext" => format!(
                "define i1 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_dict_enumerator_move_next(ptr %handle)\n\
                 \x20 %b = icmp ne i32 %r, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            "get_Current" => {
                let kv_ty = format!("{{{k_ty}, {v_ty}}}");
                // Key: call runtime, optionally unbox scalar
                let (key_ir, key_field) = if k_is_scalar {
                    dict_kv_ptr_to_scalar(&k_suf, self.layouts, "%kp", "k")
                } else {
                    (String::new(), "%kp".to_string())
                };
                // Value: call runtime, optionally unbox scalar
                let (val_ir, val_field) = if v_is_scalar {
                    dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%vp", "v")
                } else {
                    (String::new(), "%vp".to_string())
                };
                format!(
                    "define {kv_ty} @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %kp = call ptr @rt_dict_enumerator_get_key(ptr %handle)\n\
                     {key_ir}\
                     \x20 %vp = call ptr @rt_dict_enumerator_get_value(ptr %handle)\n\
                     {val_ir}\
                     \x20 %s = alloca {kv_ty}\n\
                     \x20 %f0 = getelementptr inbounds {kv_ty}, ptr %s, i32 0, i32 0\n\
                     \x20 store {k_ty} {key_field}, ptr %f0\n\
                     \x20 %f1 = getelementptr inbounds {kv_ty}, ptr %s, i32 0, i32 1\n\
                     \x20 store {v_ty} {val_field}, ptr %f1\n\
                     \x20 %r = load {kv_ty}, ptr %s\n\
                     \x20 ret {kv_ty} %r\n\
                     }}\n"
                )
            }
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ── HashSet<T> stub (RFC Phase 5) ──

    fn set_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_set_elem(class_name).unwrap_or("int");
        let is_ctor = name.contains("__ctor");
        let method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            let hash_fn = dict_hash_fn(elem_suf);
            let eq_fn = dict_eq_fn(elem_suf);
            let rest = name.strip_prefix("__ctor::").unwrap_or(name);
            let arity = rest.rsplit_once('_').and_then(|(_, suf)| {
                if !suf.is_empty() && suf.chars().all(|c| c.is_ascii_digit()) {
                    suf.parse::<u32>().ok()
                } else {
                    None
                }
            });
            return match arity {
                Some(1) => format!(
                    "define void @{mangled}(ptr %self, i32 %capacity) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_set_create(ptr {hash_fn}, ptr {eq_fn})\n\
                     \x20 call void @rt_set_ensure_capacity(ptr %handle, i32 %capacity)\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
                _ => format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %handle = call ptr @rt_set_create(ptr {hash_fn}, ptr {eq_fn})\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 store ptr %handle, ptr %hp\n\
                     \x20 ret void\n\
                     }}\n"
                ),
            };
        }
        match method {
            "UnionWith" | "IntersectWith" | "ExceptWith" | "SymmetricExceptWith" => {
                let abi = match method {
                    "UnionWith" => "rt_set_union_with",
                    "IntersectWith" => "rt_set_intersect_with",
                    "ExceptWith" => "rt_set_except_with",
                    "SymmetricExceptWith" => "rt_set_symmetric_except_with",
                    _ => unreachable!(),
                };
                format!(
                    "define void @{mangled}(ptr %self, ptr %other) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %ohp = getelementptr inbounds i8, ptr %other, i32 16\n\
                     \x20 %ohandle = load ptr, ptr %ohp\n\
                     \x20 call void @{abi}(ptr %handle, ptr %ohandle)\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "IsSubsetOf" | "IsSupersetOf" | "IsProperSubsetOf" | "IsProperSupersetOf"
            | "Overlaps" | "SetEquals" => {
                let abi = match method {
                    "IsSubsetOf" => "rt_set_is_subset_of",
                    "IsSupersetOf" => "rt_set_is_superset_of",
                    "IsProperSubsetOf" => "rt_set_is_proper_subset_of",
                    "IsProperSupersetOf" => "rt_set_is_proper_superset_of",
                    "Overlaps" => "rt_set_overlaps",
                    "SetEquals" => "rt_set_set_equals",
                    _ => unreachable!(),
                };
                format!(
                    "define i1 @{mangled}(ptr %self, ptr %other) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %ohp = getelementptr inbounds i8, ptr %other, i32 16\n\
                     \x20 %ohandle = load ptr, ptr %ohp\n\
                     \x20 %r = call i32 @{abi}(ptr %handle, ptr %ohandle)\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "ToArray" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_set_to_array(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "GetEnumerator" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_set_get_enumerator(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            // 索引读（foreach 索引脱糖的 `v[i]`）：HashSet 无语义序，
            // rt_set_get 按内部桶序稳定枚举（与 rt_set_to_array 同序）。
            "get_Item" => {
                let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
                let retain = if list_arc_inc_fn(elem_suf, self.layouts).is_some() {
                    "  call void @rt_arc_inc(ptr %r)\n"
                } else {
                    ""
                };
                format!(
                    "define {elem_ty} @{mangled}(ptr %self, i32 %index) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %out = alloca {elem_ty}\n\
                     \x20 %ok = call i32 @rt_set_get(ptr %handle, i32 %index, ptr %out)\n\
                     \x20 %r = load {elem_ty}, ptr %out\n\
                     {retain}\
                     \x20 ret {elem_ty} %r\n\
                     }}\n"
                )
            }
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ── Queue<T> stub (RFC Phase 5) ──

    fn queue_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_queue_elem(class_name).unwrap_or("int");
        let elem_size = list_elem_size(elem_suf, self.layouts);
        let is_ctor = name.contains("__ctor");
        let _method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_queue_create(i32 {elem_size})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n")
    }

    // ── Stack<T> stub (Phase 3-B) ──

    fn stack_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_stack_elem(class_name).unwrap_or("int");
        let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
        let elem_size = list_elem_size(elem_suf, self.layouts);
        let eq_fn = match list_eq_fn(elem_suf) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_inc = match list_arc_inc_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_dec = match list_arc_dec_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);
        let is_ctor = name.contains("__ctor");
        let method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_stack_create(i32 {elem_size}, {eq_fn}, {arc_inc}, {arc_dec})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        match method {
            "Push" => {
                let item_addr = if is_scalar {
                    format!(
                        "  %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n"
                    )
                } else {
                    String::new()
                };
                let arg = if is_scalar { "%item_addr" } else { "%item" };
                format!(
                    "define void @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     {item_addr}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 call void @rt_stack_push(ptr %handle, ptr {arg})\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "Pop" | "Peek" => {
                let abi = if method == "Pop" {
                    "rt_stack_pop"
                } else {
                    "rt_stack_peek"
                };
                if is_scalar {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(elem_suf, self.layouts, "%out", "v");
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca {elem_ty}\n\
                         \x20 %r = call i32 @{abi}(ptr %handle, ptr %out)\n\
                         {unbox_ir}\
                         \x20 ret {elem_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca ptr\n\
                         \x20 %r = call i32 @{abi}(ptr %handle, ptr %out)\n\
                         \x20 %v = load ptr, ptr %out\n\
                         \x20 ret ptr %v\n\
                         }}\n"
                    )
                }
            }
            "TryPop" | "TryPeek" => {
                let abi = if method == "TryPop" {
                    "rt_stack_try_pop"
                } else {
                    "rt_stack_try_peek"
                };
                format!(
                    "define i1 @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %out = alloca {elem_ty}\n\
                     \x20 %r = call i32 @{abi}(ptr %handle, ptr %out)\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "get_Count" | "Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_stack_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "Clear" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_stack_clear(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Contains" => {
                let item_addr = if is_scalar {
                    format!(
                        "  %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n"
                    )
                } else {
                    String::new()
                };
                let arg = if is_scalar { "%item_addr" } else { "%item" };
                format!(
                    "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     {item_addr}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_stack_contains(ptr %handle, ptr {arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "ToArray" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_stack_to_array(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ── LinkedListNode<T> stub (Phase 3-B) ──
    //
    // add_*/first/last/find 返回不透明 RtLinkedListNode*；Arc 侧将
    // LinkedListNode<T> 视为该指针的透传（identity），%self 即 node handle。
    // 禁止再 GEP 包装对象字段——那是静默错误路径。

    fn linked_list_node_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_linked_list_node_elem(class_name).unwrap_or("int");
        let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
        let is_scalar = !list_elem_is_ref(elem_suf, self.layouts);
        let method = name.split("::").nth(1).unwrap_or("");

        match method {
            "get_Value" => {
                if is_scalar {
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %out = alloca {elem_ty}\n\
                         \x20 call void @rt_linked_list_node_value(ptr %self, ptr %out)\n\
                         \x20 %r = load {elem_ty}, ptr %out\n\
                         \x20 ret {elem_ty} %r\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define ptr @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %out = alloca ptr\n\
                         \x20 call void @rt_linked_list_node_value(ptr %self, ptr %out)\n\
                         \x20 %r = load ptr, ptr %out\n\
                         \x20 ret ptr %r\n\
                         }}\n"
                    )
                }
            }
            "set_Value" => {
                if is_scalar {
                    format!(
                        "define void @{mangled}(ptr %self, {elem_ty} %value) {{\n\
                         entry:\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %value, ptr %item_addr\n\
                         \x20 call void @rt_linked_list_node_set_value(ptr %self, ptr %item_addr)\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define void @{mangled}(ptr %self, ptr %value) {{\n\
                         entry:\n\
                         \x20 call void @rt_linked_list_node_set_value(ptr %self, ptr %value)\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                }
            }
            "get_Previous" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %r = call ptr @rt_linked_list_node_prev(ptr %self)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "get_Next" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %r = call ptr @rt_linked_list_node_next(ptr %self)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            // 返回 runtime list handle（RtLinkedList*），非 Arc LinkedList 包装对象；
            // 不得冒充可再调 AddLast 的 facade——Stable 面不测 List 属性。
            "get_List" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %r = call ptr @rt_linked_list_node_list(ptr %self)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ── SortedDictionary<K,V> stub (Phase 3-B) ──

    fn sorted_dict_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let (k_suf, v_suf) =
            parse_sorted_dict_kv(class_name).unwrap_or(("string".into(), "int".into()));
        let k_ty = dict_kv_llvm_ty(&k_suf, self.layouts);
        let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
        let cmp_fn = dict_cmp_fn(&k_suf);

        if name.contains("__ctor") {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_sorted_dict_create(ptr {cmp_fn})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        let method_raw = name.split("::").last().unwrap_or(name);
        let method = method_raw
            .split_once('_')
            .filter(|(base, _)| *base != "get" && *base != "set")
            .map(|(base, _)| base)
            .unwrap_or(method_raw);
        match method {
            "set_Item" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                let (val_ir, val_arg) = if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&v_suf, self.layouts, "%value", "v");
                    (ir, reg)
                } else {
                    (String::new(), "%value".to_string())
                };
                format!(
                    "define void @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 call void @rt_sorted_dict_set(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "get_Item" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(&v_suf, self.layouts, "%rp", "v");
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %rp = call ptr @rt_sorted_dict_get(ptr %handle, ptr {key_arg})\n\
                         {unbox_ir}\
                         \x20 ret {v_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define {v_ty} @{mangled}(ptr %self, {k_ty} %key) {{\n\
                         entry:\n\
                         {key_ir}\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %r = call ptr @rt_sorted_dict_get(ptr %handle, ptr {key_arg})\n\
                         \x20 ret {v_ty} %r\n\
                         }}\n"
                    )
                }
            }
            "ContainsKey" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key) {{\n\
                     entry:\n\
                     {key_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_sorted_dict_contains(ptr %handle, ptr {key_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Remove" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key) {{\n\
                     entry:\n\
                     {key_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_sorted_dict_remove(ptr %handle, ptr {key_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Add" => {
                let (key_ir, key_arg) = if dict_kv_is_scalar(&k_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&k_suf, self.layouts, "%key", "k");
                    (ir, reg)
                } else {
                    (String::new(), "%key".to_string())
                };
                let (val_ir, val_arg) = if dict_kv_is_scalar(&v_suf, self.layouts) {
                    let (ir, reg) = dict_kv_scalar_to_ptr(&v_suf, self.layouts, "%value", "v");
                    (ir, reg)
                } else {
                    (String::new(), "%value".to_string())
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {k_ty} %key, {v_ty} %value) {{\n\
                     entry:\n\
                     {key_ir}\
                     {val_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @rt_sorted_dict_add(ptr %handle, ptr {key_arg}, ptr {val_arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Clear" => {
                format!(
                    "define void @{mangled}(ptr %self) {{\n\
                     entry:\n\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 call void @rt_sorted_dict_clear(ptr %handle)\n\
                     \x20 ret void\n\
                     }}\n"
                )
            }
            "Count" | "get_Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_sorted_dict_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            // TryGetValue 热路径走 emit_builtin（含 out 槽）；Keys/Values 已从 Stable 面移除。
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ── LinkedList<T> stub (Phase 3-B) ──

    fn linked_list_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_linked_list_elem(class_name).unwrap_or("int");
        let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
        let elem_size = list_elem_size(elem_suf, self.layouts);
        let eq_fn = match list_eq_fn(elem_suf) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_inc = match list_arc_inc_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let arc_dec = match list_arc_dec_fn(elem_suf, self.layouts) {
            Some(f) => format!("ptr {f}"),
            None => "ptr null".to_string(),
        };
        let is_ctor = name.contains("__ctor");
        let method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_linked_list_create(i32 {elem_size}, {eq_fn}, {arc_inc}, {arc_dec})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        match method {
            "AddLast" => format!(
                "define ptr @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_add_last(ptr %handle, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "AddFirst" => format!(
                "define ptr @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_add_first(ptr %handle, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "AddAfter" => format!(
                "define ptr @{mangled}(ptr %self, ptr %node, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_add_after(ptr %handle, ptr %node, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "AddBefore" => format!(
                "define ptr @{mangled}(ptr %self, ptr %node, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_add_before(ptr %handle, ptr %node, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            // 重载消歧后 link name 为 Remove_int / Remove_LinkedListNode_int。
            m if m == "Remove" || m.starts_with("Remove_") => {
                if name.contains("LinkedListNode") {
                    format!(
                        "define void @{mangled}(ptr %self, ptr %node) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 call void @rt_linked_list_remove_node(ptr %handle, ptr %node)\n\
                         \x20 ret void\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %item_addr = alloca {elem_ty}\n\
                         \x20 store {elem_ty} %item, ptr %item_addr\n\
                         \x20 %r = call i32 @rt_linked_list_remove(ptr %handle, ptr %item_addr)\n\
                         \x20 %b = icmp ne i32 %r, 0\n\
                         \x20 ret i1 %b\n\
                         }}\n"
                    )
                }
            }
            "get_First" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_linked_list_first(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "get_Last" => format!(
                "define ptr @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call ptr @rt_linked_list_last(ptr %handle)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "get_Count" | "Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_linked_list_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "Clear" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_linked_list_clear(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "Find" => format!(
                "define ptr @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_find(ptr %handle, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "FindLast" => format!(
                "define ptr @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call ptr @rt_linked_list_find_last(ptr %handle, ptr %item_addr)\n\
                 \x20 ret ptr %r\n\
                 }}\n"
            ),
            "Contains" => format!(
                "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %item_addr = alloca {elem_ty}\n\
                 \x20 store {elem_ty} %item, ptr %item_addr\n\
                 \x20 %r = call i32 @rt_linked_list_contains(ptr %handle, ptr %item_addr)\n\
                 \x20 %b = icmp ne i32 %r, 0\n\
                 \x20 ret i1 %b\n\
                 }}\n"
            ),
            _ => format!(
                "define void @{mangled}() {{\nentry:\n  ret void\n}}\n"
            ),
        }
    }

    // ── SortedSet<T> stub (Phase 3-B) ──

    fn sorted_set_stub(&self, name: &str) -> String {
        let mangled = mangle_fn_name(name);
        let class_name = name.strip_prefix("__ctor::").unwrap_or(name);
        let class_name = class_name.split("::").next().unwrap_or(class_name);
        let elem_suf = parse_sorted_set_elem(class_name).unwrap_or("int");
        let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
        let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);
        let cmp_fn = dict_cmp_fn(elem_suf);
        let is_ctor = name.contains("__ctor");
        let method = name.split("::").nth(1).unwrap_or("");

        if is_ctor {
            return format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %handle = call ptr @rt_sorted_set_create(ptr {cmp_fn})\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 store ptr %handle, ptr %hp\n\
                 \x20 ret void\n\
                 }}\n"
            );
        }
        match method {
            "Add" | "Contains" | "Remove" => {
                // 标量：inttoptr 装箱（同 SortedDictionary / rt_cmp_int），禁止 alloca。
                let (box_ir, arg) = if is_scalar {
                    let (ir, reg) = dict_kv_scalar_to_ptr(elem_suf, self.layouts, "%item", "item");
                    (ir, reg)
                } else {
                    (String::new(), "%item".to_string())
                };
                let abi = match method {
                    "Add" => "rt_sorted_set_add",
                    "Contains" => "rt_sorted_set_contains",
                    "Remove" => "rt_sorted_set_remove",
                    _ => unreachable!(),
                };
                format!(
                    "define i1 @{mangled}(ptr %self, {elem_ty} %item) {{\n\
                     entry:\n\
                     {box_ir}\
                     \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                     \x20 %handle = load ptr, ptr %hp\n\
                     \x20 %r = call i32 @{abi}(ptr %handle, ptr {arg})\n\
                     \x20 %b = icmp ne i32 %r, 0\n\
                     \x20 ret i1 %b\n\
                     }}\n"
                )
            }
            "Clear" => format!(
                "define void @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 call void @rt_sorted_set_clear(ptr %handle)\n\
                 \x20 ret void\n\
                 }}\n"
            ),
            "get_Count" | "Count" => format!(
                "define i32 @{mangled}(ptr %self) {{\n\
                 entry:\n\
                 \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                 \x20 %handle = load ptr, ptr %hp\n\
                 \x20 %r = call i32 @rt_sorted_set_count(ptr %handle)\n\
                 \x20 ret i32 %r\n\
                 }}\n"
            ),
            "get_Min" => {
                if is_scalar {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(elem_suf, self.layouts, "%raw", "v");
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca ptr\n\
                         \x20 %r = call i32 @rt_sorted_set_min(ptr %handle, ptr %out)\n\
                         \x20 %raw = load ptr, ptr %out\n\
                         {unbox_ir}\
                         \x20 ret {elem_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca ptr\n\
                         \x20 %r = call i32 @rt_sorted_set_min(ptr %handle, ptr %out)\n\
                         \x20 %v = load ptr, ptr %out\n\
                         \x20 ret ptr %v\n\
                         }}\n"
                    )
                }
            }
            "get_Max" => {
                if is_scalar {
                    let (unbox_ir, v_result) =
                        dict_kv_ptr_to_scalar(elem_suf, self.layouts, "%raw", "v");
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca ptr\n\
                         \x20 %r = call i32 @rt_sorted_set_max(ptr %handle, ptr %out)\n\
                         \x20 %raw = load ptr, ptr %out\n\
                         {unbox_ir}\
                         \x20 ret {elem_ty} {v_result}\n\
                         }}\n"
                    )
                } else {
                    format!(
                        "define {elem_ty} @{mangled}(ptr %self) {{\n\
                         entry:\n\
                         \x20 %hp = getelementptr inbounds i8, ptr %self, i32 16\n\
                         \x20 %handle = load ptr, ptr %hp\n\
                         \x20 %out = alloca ptr\n\
                         \x20 %r = call i32 @rt_sorted_set_max(ptr %handle, ptr %out)\n\
                         \x20 %v = load ptr, ptr %out\n\
                         \x20 ret ptr %v\n\
                         }}\n"
                    )
                }
            }
            // Reverse / GetViewBetween / Union* 已从 SortedSet.as Stable 面移除——无 stub。
            _ => format!("define void @{mangled}() {{\nentry:\n  ret void\n}}\n"),
        }
    }

    // ---- Type coercion ----

    /// Coerce a value from one LLVM IR type to another.
    ///
    /// Handles mismatches between rvalue emission and local alloca types:
    /// - `i32` → `i1`: `trunc` (e.g. `rt_dict_contains` returns `i32`, local is `bool`/`i1`)
    /// - `i1` → `i32`: `zext` (e.g. storing a comparison result into an `i32` local)
    /// - integer ↔ integer (i8/i16/i32/i64): `trunc` (narrower) or `sext` (wider)
    /// - integer → float/double: `sitofp`
    /// - float/double → integer: `fptosi`
    /// - float ↔ double: `fpext` / `fptrunc`
    /// - integer ↔ ptr (RFC 016 M3 §3.3 FFI): `inttoptr` / `ptrtoint`
    ///   用于 NativePtr 字段存储 / `(NativePtr)long_value` / `(long)native_ptr_value`
    ///   等 FFI 场景——wgpu 句柄在 C 侧为 `void*`，Arc 侧可存为 `NativePtr` 或 `long`。
    /// - same type: no-op
    /// - other mismatches: return as-is (caller's responsibility)
    pub(super) fn coerce_value(
        &mut self,
        from_ty: &str,
        val: String,
        to_ty: &str,
    ) -> (String, String) {
        if from_ty == to_ty {
            return (from_ty.to_string(), val);
        }
        // i32 → i1: truncate (bool storage)
        if from_ty == "i32" && to_ty == "i1" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = trunc i32 {val} to i1"));
            return ("i1".into(), tmp);
        }
        // i1 → i32: zero-extend
        if from_ty == "i1" && to_ty == "i32" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = zext i1 {val} to i32"));
            return ("i32".into(), tmp);
        }
        // RFC 016 M3 §3.3：integer → ptr（FFI 场景）
        // - i64 → ptr：`inttoptr i64 %val to ptr`（long → NativePtr，wgpu surface handle）
        // - i32 → ptr：`inttoptr i32 %val to ptr`（int 字面量 0 → NativePtr null）
        // 与 emit_call.rs::emit_handle_as_ptr 一致，但更通用——支持任意
        // integer-to-pointer FFI 转换（如 (NativePtr)long_value 强转）。
        if (from_ty == "i64" || from_ty == "i32") && to_ty == "ptr" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = inttoptr {from_ty} {val} to ptr"));
            return ("ptr".into(), tmp);
        }
        // RFC 016 M3 §3.3：ptr → integer（FFI 场景）
        // - ptr → i64：`ptrtoint ptr %val to i64`（NativePtr → long，存储到 long 字段）
        // - ptr → i32：`ptrtoint ptr %val to i32`（truncating，仅在调用方明确要求时触发）
        //
        // NOTE: when the destination is i32 (e.g. MIR type inference fell back to Int
        // for a string value), zero-extending back via inttoptr would corrupt 64-bit
        // pointers. We still emit the ptrtoint to satisfy the alloca store type, and
        // rely on the defensive fallback in emit_binary (inttoptr i32 → ptr) to
        // recover. This is an ABI quirk of the i32 alloca — the runtime concat path
        // only looks at the lower 32 bits, which luckily works on most 64-bit systems
        // since user-space pointers fit in 32 bits after truncation.
        if from_ty == "ptr" && (to_ty == "i64" || to_ty == "i32") {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = ptrtoint ptr {val} to {to_ty}"));
            return (to_ty.to_string(), tmp);
        }
        // integer ↔ integer (i8/i16/i32/i64): RFC 019 Phase 2.
        // `byte` (i8) is unsigned → use `zext`; other ints are signed → `sext`.
        if let (Some(from_rank), Some(to_rank)) = (int_rank(from_ty), int_rank(to_ty)) {
            if from_rank < to_rank {
                // from is wider → truncate
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc {from_ty} {val} to {to_ty}"));
                return (to_ty.to_string(), tmp);
            } else {
                // from is narrower → extend (zext for unsigned byte, sext for signed)
                let ext = if is_unsigned_int_ty(from_ty) {
                    "zext"
                } else {
                    "sext"
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = {ext} {from_ty} {val} to {to_ty}"));
                return (to_ty.to_string(), tmp);
            }
        }
        // integer → float/double: `uitofp` for unsigned byte, `sitofp` otherwise
        if int_rank(from_ty).is_some() && (to_ty == "float" || to_ty == "double") {
            let cvt = if is_unsigned_int_ty(from_ty) {
                "uitofp"
            } else {
                "sitofp"
            };
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = {cvt} {from_ty} {val} to {to_ty}"));
            return (to_ty.to_string(), tmp);
        }
        // float/double → integer: `fptoui` for unsigned byte, `fptosi` otherwise
        if (from_ty == "float" || from_ty == "double") && int_rank(to_ty).is_some() {
            let cvt = if is_unsigned_int_ty(to_ty) {
                "fptoui"
            } else {
                "fptosi"
            };
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = {cvt} {from_ty} {val} to {to_ty}"));
            return (to_ty.to_string(), tmp);
        }
        // double → float: truncate (loses precision, matches C# explicit cast)
        if from_ty == "double" && to_ty == "float" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = fptrunc double {val} to float"));
            return ("float".into(), tmp);
        }
        // float → double: extend (safe widening)
        if from_ty == "float" && to_ty == "double" {
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = fpext float {val} to double"));
            return ("double".into(), tmp);
        }
        // Default: no coercion
        (from_ty.to_string(), val)
    }
}
