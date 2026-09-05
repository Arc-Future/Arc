//! Inline builtin collection method calls (Dictionary/List) that map to direct
//! `rt_*` runtime ABI calls. Extracted from `emit_call` for single-responsibility.

use super::*;
use mir::MirOperand;

/// MIR-rewritten `string.Split` overload kind (see `rewrite_string_split_method`).
enum SplitEmitKind {
    Basic,
    Multi,
    MultiOpts,
    Count,
    MultiCount,
}

impl<'a> FnEmitter<'a> {
    /// 接口元素 `List<Iface>` 的**对象身份扫描删除**。
    ///
    /// 每次具体类→接口转换物化独立 fat 盒（`emit_make_iface` heap box），
    /// `rt_list_remove` 的指针相等对「同对象不同盒」恒判不等（C# 语义：接口
    /// 引用相等 = 底层对象身份相等，`emit_iface_equality` 同规则）。本 helper
    /// 逐元素解盒（fat[0] = obj）与查询方 obj 比对，命中即 `rt_list_remove_at`。
    /// 元素恒为盒地址（MakeIface 对 null obj 也产盒，fat[0]=null），仍按 null
    /// 防御跳过，避免悬空 load。返回 i1 temp（是否删除）。
    fn emit_iface_list_identity_remove(&mut self, handle: &str, item_ptr: &str) -> String {
        let entry = self.fresh_label();
        // 调用方当前块收尾跳转（phi 前驱需要显式标签）。
        self.emit(&format!("br label %{entry}"));
        self.emit_label(&entry);
        let out = self.fresh_temp();
        self.emit(&format!("{out} = alloca ptr, align 8"));
        // 查询侧双重解盒：`item_ptr` 是**元素槽**（rt_list_remove ABI 约定），
        // 槽内是 fat 盒地址；接口引用相等须比较**底层对象**（fat[0]）——与
        // 扫描侧的元素解盒对称（盒地址 ≠ 对象，少一层即永不相等）。
        let q_box = self.fresh_temp();
        self.emit(&format!("{q_box} = load ptr, ptr {item_ptr}"));
        let q = self.fresh_temp();
        self.emit(&format!("{q} = load ptr, ptr {q_box}"));
        let hdr = self.fresh_label();
        let body = self.fresh_label();
        let ld = self.fresh_label();
        let adv = self.fresh_label();
        let found = self.fresh_label();
        let notfound = self.fresh_label();
        let join = self.fresh_label();
        self.emit(&format!("br label %{hdr}"));
        self.emit_label(&hdr);
        let iv = self.fresh_temp();
        let iv_next = self.fresh_temp();
        self.emit(&format!(
            "{iv} = phi i32 [ 0, %{entry} ], [ {iv_next}, %{adv} ]"
        ));
        let cnt = self.fresh_temp();
        self.emit(&format!("{cnt} = call i32 @rt_list_size(ptr {handle})"));
        let more = self.fresh_temp();
        self.emit(&format!("{more} = icmp slt i32 {iv}, {cnt}"));
        self.emit(&format!("br i1 {more}, label %{body}, label %{notfound}"));
        self.emit_label(&body);
        self.emit(&format!(
            "call void @rt_list_get(ptr {handle}, i32 {iv}, ptr {out})"
        ));
        let e = self.fresh_temp();
        self.emit(&format!("{e} = load ptr, ptr {out}"));
        let en = self.fresh_temp();
        self.emit(&format!("{en} = icmp eq ptr {e}, null"));
        self.emit(&format!("br i1 {en}, label %{adv}, label %{ld}"));
        self.emit_label(&ld);
        let eobj = self.fresh_temp();
        self.emit(&format!("{eobj} = load ptr, ptr {e}"));
        let hit = self.fresh_temp();
        self.emit(&format!("{hit} = icmp eq ptr {eobj}, {q}"));
        self.emit(&format!("br i1 {hit}, label %{found}, label %{adv}"));
        self.emit_label(&found);
        self.emit(&format!(
            "call void @rt_list_remove_at(ptr {handle}, i32 {iv})"
        ));
        self.emit(&format!("br label %{join}"));
        self.emit_label(&adv);
        self.emit(&format!("{iv_next} = add i32 {iv}, 1"));
        self.emit(&format!("br label %{hdr}"));
        self.emit_label(&notfound);
        self.emit(&format!("br label %{join}"));
        self.emit_label(&join);
        let res = self.fresh_temp();
        self.emit(&format!(
            "{res} = phi i1 [ true, %{found} ], [ false, %{notfound} ]"
        ));
        res
    }

    /// 接口元素 `List<Iface>` 的**对象身份扫描查找**（IndexOf/Contains 共用）。
    ///
    /// 与 [`Self::emit_iface_list_identity_remove`] 同族：fat 盒每次转换新建，
    /// `rt_list_index_of`/`rt_list_contains` 的指针相等对「同对象不同盒」恒判
    /// 不等——按解盒 obj 扫描比对。返回 i32 temp（命中下标，未命中 -1）。
    fn emit_iface_list_identity_index(&mut self, handle: &str, item_ptr: &str) -> String {
        let entry = self.fresh_label();
        self.emit(&format!("br label %{entry}"));
        self.emit_label(&entry);
        let out = self.fresh_temp();
        self.emit(&format!("{out} = alloca ptr, align 8"));
        let q_box = self.fresh_temp();
        self.emit(&format!("{q_box} = load ptr, ptr {item_ptr}"));
        let q = self.fresh_temp();
        self.emit(&format!("{q} = load ptr, ptr {q_box}"));
        let hdr = self.fresh_label();
        let body = self.fresh_label();
        let ld = self.fresh_label();
        let adv = self.fresh_label();
        let found = self.fresh_label();
        let notfound = self.fresh_label();
        let join = self.fresh_label();
        self.emit(&format!("br label %{hdr}"));
        self.emit_label(&hdr);
        let iv = self.fresh_temp();
        let iv_next = self.fresh_temp();
        self.emit(&format!(
            "{iv} = phi i32 [ 0, %{entry} ], [ {iv_next}, %{adv} ]"
        ));
        let cnt = self.fresh_temp();
        self.emit(&format!("{cnt} = call i32 @rt_list_size(ptr {handle})"));
        let more = self.fresh_temp();
        self.emit(&format!("{more} = icmp slt i32 {iv}, {cnt}"));
        self.emit(&format!("br i1 {more}, label %{body}, label %{notfound}"));
        self.emit_label(&body);
        self.emit(&format!(
            "call void @rt_list_get(ptr {handle}, i32 {iv}, ptr {out})"
        ));
        let e = self.fresh_temp();
        self.emit(&format!("{e} = load ptr, ptr {out}"));
        let en = self.fresh_temp();
        self.emit(&format!("{en} = icmp eq ptr {e}, null"));
        self.emit(&format!("br i1 {en}, label %{adv}, label %{ld}"));
        self.emit_label(&ld);
        let eobj = self.fresh_temp();
        self.emit(&format!("{eobj} = load ptr, ptr {e}"));
        let hit = self.fresh_temp();
        self.emit(&format!("{hit} = icmp eq ptr {eobj}, {q}"));
        self.emit(&format!("br i1 {hit}, label %{found}, label %{adv}"));
        self.emit_label(&found);
        self.emit(&format!("br label %{join}"));
        self.emit_label(&adv);
        self.emit(&format!("{iv_next} = add i32 {iv}, 1"));
        self.emit(&format!("br label %{hdr}"));
        self.emit_label(&notfound);
        self.emit(&format!("br label %{join}"));
        self.emit_label(&join);
        let res = self.fresh_temp();
        self.emit(&format!(
            "{res} = phi i32 [ {iv}, %{found} ], [ -1, %{notfound} ]"
        ));
        res
    }

    /// Inline builtin collection method calls that can be expressed as direct rt_* calls.
    pub(super) fn emit_builtin_method_call(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        target_fn: Option<&str>,
    ) -> Option<TyVal> {
        // String instance methods (P2): direct rt_str_* dispatch.
        if receiver_type == "string" {
            return self.emit_string_method(receiver, method, args);
        }

        // Primitive instance methods: ToString() → rt_*_to_string ABI.
        // 非基元（如 StringBuilder）不得在此 `return None`——须落入下方 facade 分支。
        if matches!(method, "ToString" | "ToString_") {
            let (abi_ty, abi_fn) = match receiver_type {
                "int" => ("i32", "@rt_int_to_string"),
                "long" => ("i64", "@rt_long_to_string"),
                "short" => ("i16", "@rt_short_to_string"),
                "byte" => ("i8", "@rt_byte_to_string"),
                "float" => ("float", "@rt_float_to_string"),
                "double" => ("double", "@rt_double_to_string"),
                "bool" => ("i32", "@rt_bool_to_string"),
                "char" => ("i32", "@rt_char_to_string"),
                "uint" => ("i32", "@rt_uint_to_string"),
                "ulong" => ("i64", "@rt_ulong_to_string"),
                "ushort" => ("i16", "@rt_ushort_to_string"),
                "sbyte" => ("i8", "@rt_sbyte_to_string"),
                _ => ("", ""),
            };
            if !abi_fn.is_empty() {
                let (recv_ty, val) = self.emit_operand(receiver);
                let coerced = if recv_ty != abi_ty {
                    let c = self.fresh_temp();
                    let conv = if recv_ty.starts_with('i') && abi_ty.starts_with('i') {
                        "zext"
                    } else if recv_ty == "float" && abi_ty == "double" {
                        "fpext"
                    } else {
                        "bitcast"
                    };
                    self.emit(&format!("{c} = {conv} {recv_ty} {val} to {abi_ty}"));
                    c
                } else {
                    val
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi_fn}({abi_ty} {coerced})"));
                return Some(("ptr".into(), tmp));
            }
        }

        // Primitive instance Equals/GetHashCode/CompareTo: direct LLVM ops.
        let prim_ty = match receiver_type {
            "int" => "i32",
            "long" => "i64",
            "short" => "i16",
            "byte" => "i8",
            "float" => "float",
            "double" => "double",
            "bool" => "i1",
            "char" => "i32",
            "uint" => "i32",
            "ulong" => "i64",
            "ushort" => "i16",
            "sbyte" => "i8",
            _ => "",
        };
        if !prim_ty.is_empty() {
            let (_, recv_val) = self.emit_operand(receiver);
            return Some(match method {
                "GetHashCode" => {
                    self.emit_prim_instance_get_hash_code(prim_ty, receiver_type, &recv_val)
                }
                "Equals" => {
                    let (_, arg_val) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit_prim_instance_equals(prim_ty, &recv_val, &arg_val)
                }
                "CompareTo" => {
                    let (_, arg_val) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit_prim_instance_compare(prim_ty, receiver_type, &recv_val, &arg_val)
                }
                _ => return None,
            });
        }

        // StringBuilder (Arc.Text facade, RFC 021 §4.3 M4): load the runtime
        // handle from offset 16 and dispatch to rt_text_sb_*. Append/AppendLine
        // return the receiver object ptr unchanged so call sites can chain
        // (`sb.Append(a).Append(b)`); ToString returns a fresh string ptr.
        if receiver_type == "StringBuilder" {
            let (_, recv) = self.emit_operand(receiver);
            let hp = self.fresh_temp();
            self.emit(&format!(
                "{hp} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {hp}"));
            return Some(match method {
                "Append" => {
                    let (arg_ty, arg_val) = self.emit_operand(
                        &args
                            .first()
                            .cloned()
                            .unwrap_or(MirOperand::ConstString(String::new())),
                    );
                    match arg_ty.as_str() {
                        "ptr" => {
                            self.emit(&format!(
                                "call ptr @rt_text_sb_append(ptr {handle}, ptr {arg_val})"
                            ));
                        }
                        "i32" => {
                            // char and int are both i32 in LLVM; use target_fn
                            // suffix to disambiguate ("_char" vs "_int").
                            let is_char = target_fn.is_some_and(|t| t.ends_with("_char"));
                            if is_char {
                                // RFC 005 式直降：`Append(char)` 内联为 rt_sb_t
                                // 字段直接读写（冷路径回落 C 函数），免 ABI 调用边界。
                                self.emit_sb_append_char_inline(&handle, &arg_val);
                            } else {
                                self.emit(&format!(
                                    "call ptr @rt_text_sb_append_int(ptr {handle}, i32 {arg_val})"
                                ));
                            }
                        }
                        "i64" => {
                            self.emit(&format!(
                                "call ptr @rt_text_sb_append_long(ptr {handle}, i64 {arg_val})"
                            ));
                        }
                        "i1" => {
                            let ext = self.fresh_temp();
                            self.emit(&format!("{ext} = zext i1 {arg_val} to i8"));
                            self.emit(&format!(
                                "call ptr @rt_text_sb_append_bool(ptr {handle}, i8 {ext})"
                            ));
                        }
                        "float" => {
                            self.emit(&format!(
                                "call ptr @rt_text_sb_append_float(ptr {handle}, float {arg_val})"
                            ));
                        }
                        "double" => {
                            self.emit(&format!(
                                "call ptr @rt_text_sb_append_double(ptr {handle}, double {arg_val})"
                            ));
                        }
                        _ => return None,
                    }
                    ("ptr".into(), recv)
                }
                "AppendLine" => {
                    let arg = if args.is_empty() {
                        let (_, a) = self.emit_operand(&MirOperand::ConstString(String::new()));
                        a
                    } else {
                        let (_, a) = self.emit_operand(&args[0]);
                        a
                    };
                    self.emit(&format!(
                        "call ptr @rt_text_sb_append_line(ptr {handle}, ptr {arg})"
                    ));
                    ("ptr".into(), recv)
                }
                // MIR 零参重载 mangle 为 `ToString_`；两参保留 `ToString`。
                "ToString" | "ToString_" => {
                    if args.len() >= 2 {
                        let (s_ty, s_val) = self.emit_operand(&args[0]);
                        let (l_ty, l_val) = self.emit_operand(&args[1]);
                        let start = self.sb_coerce_i32(&s_ty, &s_val);
                        let len = self.sb_coerce_i32(&l_ty, &l_val);
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_text_sb_to_string_range(ptr {handle}, i32 {start}, i32 {len})"
                        ));
                        ("ptr".into(), tmp)
                    } else {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_text_sb_to_string(ptr {handle})"
                        ));
                        ("ptr".into(), tmp)
                    }
                }
                "get_Length" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_text_sb_length(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                "get_Capacity" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_text_sb_get_capacity(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_text_sb_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "EnsureCapacity" => {
                    let (c_ty, c_val) = self.emit_operand(&args[0]);
                    let cap = self.sb_coerce_i32(&c_ty, &c_val);
                    self.emit(&format!(
                        "call void @rt_text_sb_ensure_capacity(ptr {handle}, i32 {cap})"
                    ));
                    ("void".into(), String::new())
                }
                "Insert" => {
                    let (idx_ty, idx_val) = self.emit_operand(&args[0]);
                    let (_, str_val) = self.emit_operand(&args[1]);
                    let idx = self.sb_coerce_i32(&idx_ty, &idx_val);
                    self.emit(&format!(
                        "call ptr @rt_text_sb_insert(ptr {handle}, i32 {idx}, ptr {str_val})"
                    ));
                    ("ptr".into(), recv)
                }
                "Remove" => {
                    let (s_ty, s_val) = self.emit_operand(&args[0]);
                    let (l_ty, l_val) = self.emit_operand(&args[1]);
                    let start = self.sb_coerce_i32(&s_ty, &s_val);
                    let len = self.sb_coerce_i32(&l_ty, &l_val);
                    self.emit(&format!(
                        "call ptr @rt_text_sb_remove(ptr {handle}, i32 {start}, i32 {len})"
                    ));
                    ("ptr".into(), recv)
                }
                "Replace" => {
                    let (_, old_val) = self.emit_operand(&args[0]);
                    let (_, new_val) = self.emit_operand(&args[1]);
                    self.emit(&format!(
                        "call ptr @rt_text_sb_replace(ptr {handle}, ptr {old_val}, ptr {new_val})"
                    ));
                    ("ptr".into(), recv)
                }
                "get_Item" => {
                    let (idx_ty, idx_val) = self.emit_operand(&args[0]);
                    let idx = self.sb_coerce_i32(&idx_ty, &idx_val);
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_text_sb_get_char(ptr {handle}, i32 {idx})"
                    ));
                    ("i32".into(), tmp)
                }
                "set_Item" => {
                    let (idx_ty, idx_val) = self.emit_operand(&args[0]);
                    let (val_ty, val_val) = self.emit_operand(&args[1]);
                    let idx = self.sb_coerce_i32(&idx_ty, &idx_val);
                    self.emit(&format!(
                        "call void @rt_text_sb_set_char(ptr {handle}, i32 {idx}, {val_ty} {val_val})"
                    ));
                    ("void".into(), String::new())
                }
                _ => return None,
            });
        }

        if let Some((k_suf, v_suf)) = parse_dict_kv(receiver_type) {
            let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
            let k_is_scalar = dict_kv_is_scalar(&k_suf, self.layouts);
            let v_is_scalar = dict_kv_is_scalar(&v_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            // _handle field is at offset 16 (HEADER_SIZE)
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            // Lazy key_arg: only emit boxing IR when method actually uses a key.
            // Methods like get_Count / Clear / get_Keys / get_Values / GetEnumerator /
            // ContainsValue have no key argument; pre-computing a dummy key_arg for
            // scalar key types produces invalid inttoptr that clang rejects.
            let needs_key = matches!(
                method,
                "set_Item" | "get_Item" | "ContainsKey" | "Add" | "TryGetValue" | "Remove"
            );
            let key_arg = if needs_key {
                if let Some(first) = args.first() {
                    let (key_op_ty, key_op_val) = self.emit_operand(first);
                    if k_is_scalar {
                        self.box_scalar_to_ptr(&k_suf, &key_op_ty, &key_op_val)
                    } else if self.layouts.structs.contains_key(k_suf.as_str())
                        && matches!(method, "Add" | "set_Item")
                    {
                        // rt_dict 键持久语义（struct 键）：int_keys=0 模式下 keys
                        // 槽按引用存键（rt_dict_insert_at 原样存 key 指针），调用
                        // 方的栈存储随帧失效——hash/eq trampoline 之后读悬垂地址，
                        // 查找随机失败（注册后随调用帧消亡即 miss）。
                        // 值语义 struct 键在存入时堆拷贝；rt_dict 无键释放回调
                        // （rt_* ABI 冻结，RFC 036），readonly 注册表场景每键一次
                        // 规模有界，泄漏可接受。查询类方法（ContainsKey/TryGetValue/
                        // Remove/get_Item）仅在调用期比对，栈地址有效即可，不拷贝。
                        let agg = format!("%struct.{k_suf}");
                        let size = self.fresh_temp();
                        self.emit(&format!(
                            "{size} = ptrtoint ptr getelementptr ({agg}, ptr null, i32 1) to i64"
                        ));
                        let box_ptr = self.fresh_temp();
                        self.emit(&format!("{box_ptr} = call ptr @calloc(i64 1, i64 {size})"));
                        let kv = self.fresh_temp();
                        self.emit(&format!("{kv} = load {agg}, ptr {key_op_val}"));
                        self.emit(&format!("store {agg} {kv}, ptr {box_ptr}"));
                        box_ptr
                    } else {
                        key_op_val
                    }
                } else {
                    "null".to_string()
                }
            } else {
                "null".to_string()
            };

            return Some(match method {
                "set_Item" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    // H1: class 值须 retain，否则 Add 后 local drop 释对象而 Dict 仍持指针。
                    if list_elem_is_ref(&v_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {val_arg})"));
                    }
                    self.emit(&format!(
                        "call void @rt_dict_set(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    ("void".into(), String::new())
                }
                "get_Item" => {
                    let rp = self.fresh_temp();
                    self.emit(&format!(
                        "{rp} = call ptr @rt_dict_get(ptr {handle}, ptr {key_arg})"
                    ));
                    if v_is_scalar {
                        let r = self.unbox_ptr_to_scalar(&v_suf, &rp);
                        (v_ty.into(), r)
                    } else {
                        if list_elem_is_ref(&v_suf, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {rp})"));
                        }
                        (v_ty.into(), rp)
                    }
                }
                "ContainsKey" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_dict_contains(ptr {handle}, ptr {key_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "ContainsValue" => {
                    let (val_op_ty, val_op_val) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    // RFC 038 M2：ContainsValue 的 user-type value 相等性对齐
                    // C# EqualityComparer<TValue>.Default 语义——仅当 value 类型
                    // 实现了 Equals 时才引用 @__dict_eq_{V}（值相等）；否则传 null
                    // 走 runtime 引用相等（rt_dict_contains_value 的 eq==null 分支）。
                    // 避免对未实现 Equals 的类型（如 List<T>）引用未定义 trampoline。
                    let eq_fn = if dict_kv_is_user_type(&v_suf, self.layouts) {
                        if dict_value_has_equals(&v_suf, self.layouts) {
                            dict_user_eq_fn(&v_suf)
                        } else {
                            "null".to_string()
                        }
                    } else {
                        dict_eq_fn(&v_suf).to_string()
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_dict_contains_value(ptr {handle}, ptr {val_arg}, ptr {eq_fn})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Add" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    if list_elem_is_ref(&v_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {val_arg})"));
                    }
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_dict_try_add(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "TryGetValue" => {
                    // Single hash lookup: out V value via rt_dict_try_get_value
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca ptr, align 8"));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_dict_try_get_value(ptr {handle}, ptr {key_arg}, ptr {slot})"
                    ));
                    if args.len() > 1 {
                        let out_ptr = match &args[1] {
                            MirOperand::Local(id) | MirOperand::AddrOf(id) => {
                                self.byref_arg_ptr(*id)
                            }
                            _ => return None,
                        };
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {slot}"));
                        let result = if v_is_scalar {
                            self.unbox_ptr_to_scalar(&v_suf, &out_val)
                        } else {
                            if list_elem_is_ref(&v_suf, self.layouts) {
                                self.emit(&format!("call void @rt_arc_inc(ptr {out_val})"));
                            }
                            out_val
                        };
                        self.emit(&format!("store {v_ty} {result}, ptr {out_ptr}"));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Remove" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_dict_remove(ptr {handle}, ptr {key_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_dict_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!("{result} = call i32 @rt_dict_count(ptr {handle})"));
                    ("i32".into(), result)
                }
                "get_Keys" => {
                    let result = self.fresh_temp();
                    self.emit(&format!("{result} = call ptr @rt_dict_keys(ptr {handle})"));
                    ("ptr".into(), result)
                }
                "get_Values" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_dict_values(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                "GetEnumerator" => {
                    let enum_handle = self.fresh_temp();
                    self.emit(&format!(
                        "{enum_handle} = call ptr @rt_dict_get_enumerator(ptr {handle})"
                    ));
                    // 幻影类 DictEnumerator<K,V> 的 itable + MoveNext/get_Current 实现：
                    // 模块级定义，按 (K,V) 去重一次发射（FnEmitter.dict_enum_artifacts，
                    // emit_module 末尾统一输出——define 不可嵌套在函数体内）。
                    // 槽序 [MoveNext, get_Current] 与 IEnumerator layout
                    // （方法槽在前、属性 getter 紧随）一致。
                    let movenext_name = format!("DictEnumerator_{k_suf}_{v_suf}::MoveNext");
                    let current_name = format!("DictEnumerator_{k_suf}_{v_suf}::get_Current");
                    let movenext_mangled = mangle_fn_name(&movenext_name);
                    let current_mangled = mangle_fn_name(&current_name);
                    let artifact = format!(
                        "@.itable.DictEnumerator_{k_suf}_{v_suf}_IEnumerator_KVP_{k_suf}_{v_suf} = \
                         private constant [2 x ptr] [ptr @{movenext_mangled}, ptr @{current_mangled}]\n\
                         {movenext}\n\
                         {current}\n",
                        movenext = self.dict_enumerator_stub(&movenext_name),
                        current = self.dict_enumerator_stub(&current_name),
                    );
                    self.dict_enum_artifacts.insert(artifact);
                    let obj = self.fresh_temp();
                    self.emit(&format!("{obj} = call ptr @malloc(i64 24)"));
                    self.emit(&format!("store i32 1, ptr {obj}"));
                    let vt = self.fresh_temp();
                    self.emit(&format!(
                        "{vt} = getelementptr inbounds i8, ptr {obj}, i32 8"
                    ));
                    self.emit(&format!(
                        "store ptr @.itable.DictEnumerator_{k_suf}_{v_suf}_IEnumerator_KVP_{k_suf}_{v_suf}, ptr {vt}"
                    ));
                    let hp = self.fresh_temp();
                    self.emit(&format!(
                        "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
                    ));
                    self.emit(&format!("store ptr {enum_handle}, ptr {hp}"));
                    let fat = self.fresh_temp();
                    self.emit(&format!("{fat} = alloca {{ ptr, ptr }}"));
                    let fat_obj = self.fresh_temp();
                    self.emit(&format!("{fat_obj} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 0"));
                    self.emit(&format!("store ptr {obj}, ptr {fat_obj}"));
                    let fat_vt = self.fresh_temp();
                    self.emit(&format!(
                        "{fat_vt} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 1"
                    ));
                    self.emit(&format!(
                        "store ptr @.itable.DictEnumerator_{k_suf}_{v_suf}_IEnumerator_KVP_{k_suf}_{v_suf}, ptr {fat_vt}"
                    ));
                    ("ptr".into(), fat)
                }
                _ => return None,
            });
        }

        // ConcurrentDictionary<K,V> — RFC 024 M1 per-bucket lock + lock-free read
        if let Some((k_suf, v_suf)) = parse_concurrent_dict_kv(receiver_type) {
            let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
            let k_is_scalar = dict_kv_is_scalar(&k_suf, self.layouts);
            let v_is_scalar = dict_kv_is_scalar(&v_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            // Lazy key_arg: avoid emitting IR for methods without key arg.
            let key_arg = if let Some(first) = args.first() {
                let (key_op_ty, key_op_val) = self.emit_operand(first);
                if k_is_scalar {
                    self.box_scalar_to_ptr(&k_suf, &key_op_ty, &key_op_val)
                } else {
                    key_op_val
                }
            } else {
                "null".to_string()
            };

            return Some(match method {
                "TryAdd" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    // H1: class 值 retain（与 set_Item / Dict.TryAdd 同构）。
                    if list_elem_is_ref(&v_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {val_arg})"));
                    }
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_try_add(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "TryGetValue" => {
                    // out V value — allocate stack slot, pass ptr, load result
                    let alloca_name = self.fresh_temp();
                    self.emit(&format!("{alloca_name} = alloca ptr, align 8"));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_try_get(ptr {handle}, ptr {key_arg}, ptr {alloca_name})"
                    ));
                    if let Some(out_id) = args.get(1).and_then(|a| match a {
                        MirOperand::Local(id) | MirOperand::AddrOf(id) => Some(*id),
                        _ => None,
                    }) {
                        let out_ptr = self.byref_arg_ptr(out_id);
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {alloca_name}"));
                        let result = if v_is_scalar {
                            self.unbox_ptr_to_scalar(&v_suf, &out_val)
                        } else {
                            out_val
                        };
                        self.emit(&format!("store {v_ty} {result}, ptr {out_ptr}"));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "set_Item" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    if list_elem_is_ref(&v_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {val_arg})"));
                    }
                    self.emit(&format!(
                        "call void @rt_concurrent_dict_set(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    ("void".into(), String::new())
                }
                "GetValueOrDefault" | "get_Item" => {
                    let rp = self.fresh_temp();
                    self.emit(&format!(
                        "{rp} = call ptr @rt_concurrent_dict_get_or_default(ptr {handle}, ptr {key_arg})"
                    ));
                    if v_is_scalar {
                        let r = self.unbox_ptr_to_scalar(&v_suf, &rp);
                        (v_ty.into(), r)
                    } else {
                        if list_elem_is_ref(&v_suf, self.layouts) {
                            self.emit(&format!("call void @rt_arc_inc(ptr {rp})"));
                        }
                        (v_ty.into(), rp)
                    }
                }
                "TryRemove" => {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca ptr, align 8"));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_try_remove(ptr {handle}, ptr {key_arg}, ptr {slot})"
                    ));
                    // out 实参经 MIR 为 AddrOf/Local；形状不符时仍返回 ABI 结果（禁 stub 假 0）。
                    if let Some(out_id) = args.get(1).and_then(|a| match a {
                        MirOperand::Local(id) | MirOperand::AddrOf(id) => Some(*id),
                        _ => None,
                    }) {
                        let out_ptr = self.byref_arg_ptr(out_id);
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {slot}"));
                        let result = if v_is_scalar {
                            self.unbox_ptr_to_scalar(&v_suf, &out_val)
                        } else {
                            out_val
                        };
                        self.emit(&format!("store {v_ty} {result}, ptr {out_ptr}"));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "TryUpdate" => {
                    let (nv_ty, nv_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let (cv_ty, cv_val) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let new_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &nv_ty, &nv_val)
                    } else {
                        nv_val
                    };
                    let cmp_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &cv_ty, &cv_val)
                    } else {
                        cv_val
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_try_update(ptr {handle}, ptr {key_arg}, ptr {new_arg}, ptr {cmp_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "GetOrAdd" => {
                    // Value overload：第二参 LLVM 类型为标量 → get_or_add_val。
                    // Factory 重载：C 函数指针路径；Arc Func trampoline 后置。
                    let (arg_ty, arg_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                    let use_val = v_is_scalar
                        && matches!(
                            arg_ty.as_str(),
                            "i8" | "i16" | "i32" | "i64" | "float" | "double"
                        );
                    let rp = self.fresh_temp();
                    if use_val {
                        let val_arg = self.box_scalar_to_ptr(&v_suf, &arg_ty, &arg_val);
                        self.emit(&format!(
                            "{rp} = call ptr @rt_concurrent_dict_get_or_add_val(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                        ));
                    } else {
                        self.emit(&format!(
                            "{rp} = call ptr @rt_concurrent_dict_get_or_add(ptr {handle}, ptr {key_arg}, ptr {arg_val})"
                        ));
                    }
                    if v_is_scalar {
                        let r = self.unbox_ptr_to_scalar(&v_suf, &rp);
                        (v_ty.into(), r)
                    } else {
                        (v_ty.into(), rp)
                    }
                }
                "get_Keys" | "Keys" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_concurrent_dict_keys(ptr {handle})"
                    ));
                    ("ptr".into(), tmp)
                }
                "ContainsKey" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_contains(ptr {handle}, ptr {key_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "get_Count" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_concurrent_dict_count(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                "get_IsEmpty" | "IsEmpty" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_concurrent_dict_count(ptr {handle})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp eq i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Clear" => {
                    self.emit(&format!(
                        "call void @rt_concurrent_dict_clear(ptr {handle})"
                    ));
                    ("void".into(), String::new())
                }
                _ => return None,
            });
        }

        // ConcurrentQueue/ConcurrentBag/ConcurrentStack (RFC 024 M2-M4) — single-generic
        if let Some(elem_suf) = parse_concurrent_single_elem(receiver_type) {
            let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
            let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);
            let abi_prefix = if receiver_type.starts_with("ConcurrentQueue_") {
                "rt_concurrent_queue"
            } else if receiver_type.starts_with("ConcurrentBag_") {
                "rt_concurrent_bag"
            } else if receiver_type.starts_with("ConcurrentStack_") {
                "rt_concurrent_stack"
            } else if receiver_type.starts_with("BlockingCollection_") {
                "rt_blocking_collection"
            } else {
                return None;
            };

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            // BlockingCollection has different method set
            let is_blocking = receiver_type.starts_with("BlockingCollection_");

            return Some(match method {
                // Enqueue/Push/Add — one value arg
                "Add" if is_blocking => {
                    let val = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if is_scalar {
                        self.box_scalar_to_ptr(elem_suf, &val.0, &val.1)
                    } else {
                        val.1
                    };
                    self.emit(&format!(
                        "call void @rt_blocking_collection_add(ptr {handle}, ptr {val_arg})"
                    ));
                    ("void".into(), String::new())
                }
                "Take" if is_blocking => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_blocking_collection_take(ptr {handle})"
                    ));
                    if is_scalar {
                        let r = self.unbox_ptr_to_scalar(elem_suf, &tmp);
                        (elem_ty.into(), r)
                    } else {
                        (elem_ty.into(), tmp)
                    }
                }
                "CompleteAdding" if is_blocking => {
                    self.emit(&format!(
                        "call void @rt_blocking_collection_complete(ptr {handle})"
                    ));
                    ("void".into(), String::new())
                }
                "get_IsCompleted" | "IsCompleted" if is_blocking => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_blocking_collection_is_completed(ptr {handle})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "get_IsAddingCompleted" | "IsAddingCompleted" if is_blocking => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_blocking_collection_is_adding_completed(ptr {handle})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                // RFC 024 M7: IConcurrentCollection.TryAdd on Queue/Bag/Stack;
                // BlockingCollection.TryAdd stays on blocking ABI.
                "TryAdd" if is_blocking => {
                    let val = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if is_scalar {
                        self.box_scalar_to_ptr(elem_suf, &val.0, &val.1)
                    } else {
                        val.1
                    };
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_blocking_collection_try_add(ptr {handle}, ptr {val_arg})"
                    ));
                    ("i32".into(), tmp)
                }
                "TryAdd" => {
                    let val = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if is_scalar {
                        self.box_scalar_to_ptr(elem_suf, &val.0, &val.1)
                    } else {
                        val.1
                    };
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @{abi_prefix}_try_add(ptr {handle}, ptr {val_arg})"
                    ));
                    ("i32".into(), tmp)
                }
                // Enqueue/Push/Add — one value arg
                m @ ("Enqueue" | "Add" | "Push") => {
                    let val = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if is_scalar {
                        self.box_scalar_to_ptr(elem_suf, &val.0, &val.1)
                    } else {
                        val.1
                    };
                    let abi = match m {
                        "Enqueue" => format!("{abi_prefix}_enqueue"),
                        "Add" => format!("{abi_prefix}_add"),
                        "Push" => format!("{abi_prefix}_push"),
                        _ => unreachable!(),
                    };
                    self.emit(&format!("call void @{abi}(ptr {handle}, ptr {val_arg})"));
                    ("void".into(), String::new())
                }
                // TryDequeue/TryTake/TryPop — returns bool + out ptr
                // out 实参经 MIR lower 为 AddrOf(local)（见 RefArg），兼容 Local。
                // M7: Queue/Stack also expose TryTake via IConcurrentCollection.
                m @ ("TryDequeue" | "TryTake" | "TryPop") => {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca ptr, align 8"));
                    let tmp = self.fresh_temp();
                    let abi = if is_blocking {
                        "rt_blocking_collection_try_take".to_string()
                    } else {
                        match m {
                            "TryDequeue" => format!("{abi_prefix}_try_dequeue"),
                            "TryTake" => format!("{abi_prefix}_try_take"),
                            "TryPop" => format!("{abi_prefix}_try_pop"),
                            _ => unreachable!(),
                        }
                    };
                    self.emit(&format!(
                        "{tmp} = call i32 @{abi}(ptr {handle}, ptr {slot})"
                    ));
                    if let Some(out_id) = match &args.first() {
                        Some(MirOperand::Local(id) | MirOperand::AddrOf(id)) => Some(*id),
                        _ => None,
                    } {
                        let out_ptr = self.byref_arg_ptr(out_id);
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {slot}"));
                        let result = if is_scalar {
                            self.unbox_ptr_to_scalar(elem_suf, &out_val)
                        } else {
                            out_val
                        };
                        self.emit(&format!("store {elem_ty} {result}, ptr {out_ptr}"));
                    }
                    ("i32".into(), tmp)
                }
                // TryPeek
                "TryPeek" => {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca ptr, align 8"));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @{abi_prefix}_try_peek(ptr {handle}, ptr {slot})"
                    ));
                    if let Some(out_id) = match &args.first() {
                        Some(MirOperand::Local(id) | MirOperand::AddrOf(id)) => Some(*id),
                        _ => None,
                    } {
                        let out_ptr = self.byref_arg_ptr(out_id);
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {slot}"));
                        let result = if is_scalar {
                            self.unbox_ptr_to_scalar(elem_suf, &out_val)
                        } else {
                            out_val
                        };
                        self.emit(&format!("store {elem_ty} {result}, ptr {out_ptr}"));
                    }
                    ("i32".into(), tmp)
                }
                "get_Count" | "Count" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @{abi_prefix}_count(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                // BlockingCollection.BoundedCapacity — 有效容量上限（无界 = int.MaxValue）。
                "get_BoundedCapacity" | "BoundedCapacity" if is_blocking => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_blocking_collection_bounded_capacity(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                "get_IsEmpty" | "IsEmpty" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @{abi_prefix}_is_empty(ptr {handle})"
                    ));
                    ("i32".into(), tmp)
                }
                "Clear" => {
                    self.emit(&format!("call void @{abi_prefix}_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                // RFC 024 M7: ToArray / CopyTo on Queue/Bag/Stack/BlockingCollection
                "ToArray" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call ptr @{abi_prefix}_to_array(ptr {handle})"
                    ));
                    ("ptr".into(), tmp)
                }
                "CopyTo" => {
                    let (_, array) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let (_, idx) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "call void @{abi_prefix}_copy_to(ptr {handle}, ptr {array}, i32 {idx})"
                    ));
                    ("void".into(), String::new())
                }
                _ => return None,
            });
        }

        // HashSet<T> — RFC Phase 5
        if let Some(elem_suf) = parse_set_elem(receiver_type) {
            let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "Add" | "Contains" | "Remove" => {
                    // rt_set_* ABI：`elem_ptr` 为「指向元素槽」的指针，C 侧固定读
                    // 8 字节（`key = *(void**)elem_ptr`）。因此元素槽必须是 ptr 宽度：
                    //   - 标量（int/long/short/byte/bool/float/double/…）先经
                    //     `box_scalar_to_ptr` 规范化装箱（与 Dictionary 同一契约），
                    //     保证低 N 位承载位模式、高位置零——存储/查询 key 自洽；
                    //   - string / 引用 / 用户类型本身即 ptr，直接入槽。
                    // 旧实现 `alloca {elem_ty}` + store 位型，1/2/4 字节槽会被 C 侧
                    // 读入栈垃圾 → HashSet<int>/<short>/<byte>/<bool>/<float> 等
                    // 全部失效（`HashSetTests` / `std_collections_e2e` 失败根因）。
                    let (_, arg_rval) = self.emit_operand(&args[0]);
                    let key_ptr = if dict_kv_is_scalar(elem_suf, self.layouts) {
                        self.box_scalar_to_ptr(elem_suf, elem_ty, &arg_rval)
                    } else {
                        arg_rval
                    };
                    let boxed = self.fresh_temp();
                    self.emit(&format!("{boxed} = alloca ptr"));
                    self.emit(&format!("store ptr {key_ptr}, ptr {boxed}"));
                    let result = self.fresh_temp();
                    let abi = match method {
                        "Add" => "rt_set_add",
                        "Contains" => "rt_set_contains",
                        "Remove" => "rt_set_remove",
                        _ => unreachable!(),
                    };
                    self.emit(&format!(
                        "{result} = call i32 @{abi}(ptr {handle}, ptr {boxed})"
                    ));
                    let truncated = self.fresh_temp();
                    self.emit(&format!("{truncated} = trunc i32 {result} to i1"));
                    ("i1".into(), truncated)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_set_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!("{result} = call i32 @rt_set_count(ptr {handle})"));
                    ("i32".into(), result)
                }
                "UnionWith" | "IntersectWith" | "ExceptWith" | "SymmetricExceptWith" => {
                    let (_, other_recv) = self.emit_operand(&args[0]);
                    let ohp = self.fresh_temp();
                    self.emit(&format!(
                        "{ohp} = getelementptr inbounds i8, ptr {other_recv}, i32 16"
                    ));
                    let ohandle = self.fresh_temp();
                    self.emit(&format!("{ohandle} = load ptr, ptr {ohp}"));
                    let abi = match method {
                        "UnionWith" => "rt_set_union_with",
                        "IntersectWith" => "rt_set_intersect_with",
                        "ExceptWith" => "rt_set_except_with",
                        "SymmetricExceptWith" => "rt_set_symmetric_except_with",
                        _ => unreachable!(),
                    };
                    self.emit(&format!("call void @{abi}(ptr {handle}, ptr {ohandle})"));
                    ("void".into(), String::new())
                }
                "IsSubsetOf" | "IsSupersetOf" | "IsProperSubsetOf" | "IsProperSupersetOf"
                | "Overlaps" | "SetEquals" => {
                    let (_, other_recv) = self.emit_operand(&args[0]);
                    let ohp = self.fresh_temp();
                    self.emit(&format!(
                        "{ohp} = getelementptr inbounds i8, ptr {other_recv}, i32 16"
                    ));
                    let ohandle = self.fresh_temp();
                    self.emit(&format!("{ohandle} = load ptr, ptr {ohp}"));
                    let abi = match method {
                        "IsSubsetOf" => "rt_set_is_subset_of",
                        "IsSupersetOf" => "rt_set_is_superset_of",
                        "IsProperSubsetOf" => "rt_set_is_proper_subset_of",
                        "IsProperSupersetOf" => "rt_set_is_proper_superset_of",
                        "Overlaps" => "rt_set_overlaps",
                        "SetEquals" => "rt_set_set_equals",
                        _ => unreachable!(),
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @{abi}(ptr {handle}, ptr {ohandle})"
                    ));
                    let truncated = self.fresh_temp();
                    self.emit(&format!("{truncated} = trunc i32 {raw} to i1"));
                    ("i1".into(), truncated)
                }
                "ToArray" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_set_to_array(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                "GetEnumerator" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_set_get_enumerator(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                _ => return None,
            });
        }

        // Queue<T> — RFC Phase 5
        if let Some(elem_suf) = parse_queue_elem(receiver_type) {
            let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
            let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "Enqueue" => {
                    // rt_queue ABI：`elem_ptr` 是「元素值槽地址」（C 侧按
                    // elem_size memcpy）——标量与引用类型一律先落槽再传槽
                    // 地址。旧实现非标量分支直传对象指针，C 侧把对象头前
                    // 8 字节（refcount 快照）当元素值存入（channels
                    // backpressure waiter=0x2 → 0xC0000005 实证）。
                    //
                    // 所有权移交（对齐 Dictionary.Add 的家族约定）：类元素
                    // 入队 +1 加在**元素值**上；若误加在槽地址上，D1 守卫
                    // 会把槽前 4 字节（元素指针低半）当 refcount 改写——
                    // 队列存入撕裂指针（waiter=real+1 实证）。ARC 维护值
                    // 与 ABI 传参槽位必须分离。
                    let arg_rval = self.emit_operand(&args[0]).1;
                    let boxed = self.fresh_temp();
                    self.emit(&format!("{boxed} = alloca {elem_ty}"));
                    self.emit(&format!("store {elem_ty} {arg_rval}, ptr {boxed}"));
                    if !is_scalar && list_elem_is_ref(elem_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {arg_rval})"));
                    }
                    self.emit(&format!(
                        "call void @rt_queue_enqueue(ptr {handle}, ptr {boxed})"
                    ));
                    ("void".into(), String::new())
                }
                "Dequeue" | "Peek" => {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca {elem_ty}"));
                    let abi = if method == "Dequeue" {
                        "rt_queue_dequeue"
                    } else {
                        "rt_queue_peek"
                    };
                    self.emit(&format!("call void @{abi}(ptr {handle}, ptr {slot})"));
                    let result = self.fresh_temp();
                    self.emit(&format!("{result} = load {elem_ty}, ptr {slot}"));
                    // 出队/窥视转移队列持有的 +1 给消费局部（与 Enqueue 的 +1 对称）。
                    if !is_scalar && list_elem_is_ref(elem_suf, self.layouts) {
                        self.emit(&format!("call void @rt_arc_inc(ptr {result})"));
                    }
                    (elem_ty.to_string(), result)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_queue_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_queue_count(ptr {handle})"
                    ));
                    ("i32".into(), result)
                }
                _ => return None,
            });
        }

        // Stack<T> — Phase 3-B: LIFO stack (C ABI backed)
        if let Some(elem_suf) = parse_stack_elem(receiver_type) {
            let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
            let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "Push" => {
                    // 值槽 ABI（对齐 Set 臂范式 / rt_queue 同源教训）：指针
                    // 值落 8B 槽后传槽地址——直传对象指针会把对象头 refcount
                    // 快照当元素拷入 rt_list。
                    let arg_val = {
                        let arg_rval = self.emit_operand(&args[0]).1;
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {arg_rval}, ptr {boxed}"));
                        boxed
                    };
                    self.emit(&format!(
                        "call void @rt_stack_push(ptr {handle}, ptr {arg_val})"
                    ));
                    ("void".into(), String::new())
                }
                "Pop" | "Peek" => {
                    let abi = if method == "Pop" {
                        "rt_stack_pop"
                    } else {
                        "rt_stack_peek"
                    };
                    let out = self.fresh_temp();
                    self.emit(&format!("{out} = alloca {elem_ty}"));
                    self.emit(&format!("call i32 @{abi}(ptr {handle}, ptr {out})"));
                    if is_scalar {
                        let v = self.fresh_temp();
                        self.emit(&format!("{v} = load {elem_ty}, ptr {out}"));
                        (elem_ty.into(), v)
                    } else {
                        let v = self.fresh_temp();
                        self.emit(&format!("{v} = load ptr, ptr {out}"));
                        ("ptr".into(), v)
                    }
                }
                "TryPop" | "TryPeek" => {
                    let abi = if method == "TryPop" {
                        "rt_stack_try_pop"
                    } else {
                        "rt_stack_try_peek"
                    };
                    let out = self.fresh_temp();
                    self.emit(&format!("{out} = alloca {elem_ty}"));
                    let raw = self.fresh_temp();
                    self.emit(&format!("{raw} = call i32 @{abi}(ptr {handle}, ptr {out})"));
                    self.emit(&format!("{raw} = trunc i32 {raw} to i1"));
                    ("i1".into(), raw)
                }
                "Contains" => {
                    let arg_val = {
                        let arg_rval = self.emit_operand(&args[0]).1;
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {arg_rval}, ptr {boxed}"));
                        boxed
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_stack_contains(ptr {handle}, ptr {arg_val})"
                    ));
                    self.emit(&format!("{raw} = trunc i32 {raw} to i1"));
                    ("i1".into(), raw)
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_stack_count(ptr {handle})"
                    ));
                    ("i32".into(), result)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_stack_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "ToArray" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_stack_to_array(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                _ => return None,
            });
        }

        // SortedDictionary<K,V> — Stable 最小面：标量键/值 inttoptr 装箱（rt_cmp_int）
        // Keys/Values / 比较器 ctor 已从公开面移除——禁止静默 stub。
        if let Some((k_suf, v_suf)) = parse_sorted_dict_kv(receiver_type) {
            let v_ty = dict_kv_llvm_ty(&v_suf, self.layouts);
            let k_is_scalar = dict_kv_is_scalar(&k_suf, self.layouts);
            let v_is_scalar = dict_kv_is_scalar(&v_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            let key_arg = if let Some(first) = args.first() {
                let (key_op_ty, key_op_val) = self.emit_operand(first);
                if k_is_scalar {
                    self.box_scalar_to_ptr(&k_suf, &key_op_ty, &key_op_val)
                } else {
                    key_op_val
                }
            } else {
                "null".to_string()
            };

            return Some(match method {
                "set_Item" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    self.emit(&format!(
                        "call void @rt_sorted_dict_set(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    ("void".into(), String::new())
                }
                "get_Item" => {
                    let rp = self.fresh_temp();
                    self.emit(&format!(
                        "{rp} = call ptr @rt_sorted_dict_get(ptr {handle}, ptr {key_arg})"
                    ));
                    if v_is_scalar {
                        let r = self.unbox_ptr_to_scalar(&v_suf, &rp);
                        (v_ty.into(), r)
                    } else {
                        (v_ty.into(), rp)
                    }
                }
                "ContainsKey" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_sorted_dict_contains(ptr {handle}, ptr {key_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Add" => {
                    let (val_op_ty, val_op_val) =
                        self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let val_arg = if v_is_scalar {
                        self.box_scalar_to_ptr(&v_suf, &val_op_ty, &val_op_val)
                    } else {
                        val_op_val
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_sorted_dict_add(ptr {handle}, ptr {key_arg}, ptr {val_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "TryGetValue" => {
                    let slot = self.fresh_temp();
                    self.emit(&format!("{slot} = alloca ptr, align 8"));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_sorted_dict_try_get(ptr {handle}, ptr {key_arg}, ptr {slot})"
                    ));
                    if args.len() > 1 {
                        let out_ptr = match &args[1] {
                            MirOperand::Local(id) | MirOperand::AddrOf(id) => {
                                self.byref_arg_ptr(*id)
                            }
                            _ => return None,
                        };
                        let out_val = self.fresh_temp();
                        self.emit(&format!("{out_val} = load ptr, ptr {slot}"));
                        let result = if v_is_scalar {
                            self.unbox_ptr_to_scalar(&v_suf, &out_val)
                        } else {
                            out_val
                        };
                        self.emit(&format!("store {v_ty} {result}, ptr {out_ptr}"));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Remove" => {
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_sorted_dict_remove(ptr {handle}, ptr {key_arg})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_sorted_dict_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_sorted_dict_count(ptr {handle})"
                    ));
                    ("i32".into(), result)
                }
                // get_Keys / get_Values 已从 SortedDictionary.as Stable 面移除
                _ => return None,
            });
        }

        // LinkedList<T> — Phase 3-B: doubly-linked list
        if let Some(elem_suf) = parse_linked_list_elem(receiver_type) {
            let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
            let is_scalar = !list_elem_is_ref(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "AddLast" | "AddFirst" => {
                    let (_arg_ty, arg_val) = self.emit_operand(&args[0]);
                    let item_ptr = if is_scalar {
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {arg_val}, ptr {boxed}"));
                        boxed
                    } else {
                        arg_val
                    };
                    let result = self.fresh_temp();
                    let abi = if method == "AddLast" {
                        "rt_linked_list_add_last"
                    } else {
                        "rt_linked_list_add_first"
                    };
                    self.emit(&format!(
                        "{result} = call ptr @{abi}(ptr {handle}, ptr {item_ptr})"
                    ));
                    ("ptr".into(), result)
                }
                "AddAfter" | "AddBefore" => {
                    let (_node_ty, node_val) = self.emit_operand(&args[0]);
                    let (_item_ty, item_val) = self.emit_operand(args.get(1).unwrap_or(&args[0]));
                    let item_ptr = if is_scalar {
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {item_val}, ptr {boxed}"));
                        boxed
                    } else {
                        item_val
                    };
                    let result = self.fresh_temp();
                    let abi = if method == "AddAfter" {
                        "rt_linked_list_add_after"
                    } else {
                        "rt_linked_list_add_before"
                    };
                    self.emit(&format!(
                        "{result} = call ptr @{abi}(ptr {handle}, ptr {node_val}, ptr {item_ptr})"
                    ));
                    ("ptr".into(), result)
                }
                "Find" | "FindLast" => {
                    let (_arg_ty, arg_val) = self.emit_operand(&args[0]);
                    let item_ptr = if is_scalar {
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {arg_val}, ptr {boxed}"));
                        boxed
                    } else {
                        arg_val
                    };
                    let result = self.fresh_temp();
                    let abi = if method == "Find" {
                        "rt_linked_list_find"
                    } else {
                        "rt_linked_list_find_last"
                    };
                    self.emit(&format!(
                        "{result} = call ptr @{abi}(ptr {handle}, ptr {item_ptr})"
                    ));
                    ("ptr".into(), result)
                }
                "Contains" => {
                    let (_arg_ty, arg_val) = self.emit_operand(&args[0]);
                    let item_ptr = if is_scalar {
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {arg_val}, ptr {boxed}"));
                        boxed
                    } else {
                        arg_val
                    };
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_linked_list_contains(ptr {handle}, ptr {item_ptr})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "get_First" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_linked_list_first(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                "get_Last" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_linked_list_last(ptr {handle})"
                    ));
                    ("ptr".into(), result)
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_linked_list_count(ptr {handle})"
                    ));
                    ("i32".into(), result)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_linked_list_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "Remove" => {
                    // 值重载 vs 节点重载：靠 target_fn 后缀（Remove_LinkedListNode_*）。
                    let is_node = target_fn
                        .map(|t| t.contains("LinkedListNode"))
                        .unwrap_or(false);
                    if is_node {
                        let (_nty, node_val) = self.emit_operand(&args[0]);
                        self.emit(&format!(
                            "call void @rt_linked_list_remove_node(ptr {handle}, ptr {node_val})"
                        ));
                        ("void".into(), String::new())
                    } else {
                        let (_arg_ty, arg_val) = self.emit_operand(&args[0]);
                        let item_ptr = if is_scalar {
                            let boxed = self.fresh_temp();
                            self.emit(&format!("{boxed} = alloca {elem_ty}"));
                            self.emit(&format!("store {elem_ty} {arg_val}, ptr {boxed}"));
                            boxed
                        } else {
                            arg_val
                        };
                        let raw = self.fresh_temp();
                        self.emit(&format!(
                            "{raw} = call i32 @rt_linked_list_remove(ptr {handle}, ptr {item_ptr})"
                        ));
                        let tmp = self.fresh_temp();
                        self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                        ("i1".into(), tmp)
                    }
                }
                _ => return None,
            });
        }

        // LinkedListNode<T> — 不透明 RtLinkedListNode* 透传；%recv 即 node handle。
        if let Some(elem_suf) = parse_linked_list_node_elem(receiver_type) {
            let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);
            let is_scalar = !list_elem_is_ref(elem_suf, self.layouts);
            let (_, recv) = self.emit_operand(receiver);

            return Some(match method {
                "get_Value" => {
                    let out = self.fresh_temp();
                    let result = self.fresh_temp();
                    if is_scalar {
                        self.emit(&format!("{out} = alloca {elem_ty}"));
                        self.emit(&format!(
                            "call void @rt_linked_list_node_value(ptr {recv}, ptr {out})"
                        ));
                        self.emit(&format!("{result} = load {elem_ty}, ptr {out}"));
                        (elem_ty.into(), result)
                    } else {
                        self.emit(&format!("{out} = alloca ptr"));
                        self.emit(&format!(
                            "call void @rt_linked_list_node_value(ptr {recv}, ptr {out})"
                        ));
                        self.emit(&format!("{result} = load ptr, ptr {out}"));
                        ("ptr".into(), result)
                    }
                }
                "set_Value" => {
                    let (_aty, aval) = self.emit_operand(&args[0]);
                    if is_scalar {
                        let boxed = self.fresh_temp();
                        self.emit(&format!("{boxed} = alloca {elem_ty}"));
                        self.emit(&format!("store {elem_ty} {aval}, ptr {boxed}"));
                        self.emit(&format!(
                            "call void @rt_linked_list_node_set_value(ptr {recv}, ptr {boxed})"
                        ));
                    } else {
                        self.emit(&format!(
                            "call void @rt_linked_list_node_set_value(ptr {recv}, ptr {aval})"
                        ));
                    }
                    ("void".into(), String::new())
                }
                "get_Previous" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_linked_list_node_prev(ptr {recv})"
                    ));
                    ("ptr".into(), result)
                }
                "get_Next" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_linked_list_node_next(ptr {recv})"
                    ));
                    ("ptr".into(), result)
                }
                "get_List" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call ptr @rt_linked_list_node_list(ptr {recv})"
                    ));
                    ("ptr".into(), result)
                }
                _ => return None,
            });
        }

        // SortedSet<T> — Stable 最小面：标量键与 SortedDictionary 同为 inttoptr 装箱
        // （rt_cmp_int 比较指针位）；禁止 alloca 假指针（栈槽在 Add 返回后失效）。
        if let Some(elem_suf) = parse_sorted_set_elem(receiver_type) {
            let elem_ty = dict_kv_llvm_ty(elem_suf, self.layouts);
            let is_scalar = dict_kv_is_scalar(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "Add" | "Contains" | "Remove" => {
                    let (arg_ty, arg_rval) = self.emit_operand(&args[0]);
                    let arg_val = if is_scalar {
                        self.box_scalar_to_ptr(elem_suf, &arg_ty, &arg_rval)
                    } else {
                        arg_rval
                    };
                    let result = self.fresh_temp();
                    let abi = match method {
                        "Add" => "rt_sorted_set_add",
                        "Contains" => "rt_sorted_set_contains",
                        "Remove" => "rt_sorted_set_remove",
                        _ => unreachable!(),
                    };
                    self.emit(&format!(
                        "{result} = call i32 @{abi}(ptr {handle}, ptr {arg_val})"
                    ));
                    let truncated = self.fresh_temp();
                    self.emit(&format!("{truncated} = trunc i32 {result} to i1"));
                    ("i1".into(), truncated)
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_sorted_set_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "get_Count" => {
                    let result = self.fresh_temp();
                    self.emit(&format!(
                        "{result} = call i32 @rt_sorted_set_count(ptr {handle})"
                    ));
                    ("i32".into(), result)
                }
                "get_Min" | "get_Max" => {
                    let abi = if method == "get_Min" {
                        "rt_sorted_set_min"
                    } else {
                        "rt_sorted_set_max"
                    };
                    let out = self.fresh_temp();
                    self.emit(&format!("{out} = alloca ptr"));
                    let ok = self.fresh_temp();
                    self.emit(&format!("{ok} = call i32 @{abi}(ptr {handle}, ptr {out})"));
                    let loaded = self.fresh_temp();
                    self.emit(&format!("{loaded} = load ptr, ptr {out}"));
                    if is_scalar {
                        let r = self.unbox_ptr_to_scalar(elem_suf, &loaded);
                        (elem_ty.into(), r)
                    } else {
                        ("ptr".into(), loaded)
                    }
                }
                _ => return None,
            });
        }

        if let Some(elem_suf) = parse_list_elem(receiver_type) {
            let elem_ty = list_elem_llvm_ty(elem_suf, self.layouts);

            let (_, recv) = self.emit_operand(receiver);
            let handle_addr = self.fresh_temp();
            self.emit(&format!(
                "{handle_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
            ));
            let handle = self.fresh_temp();
            self.emit(&format!("{handle} = load ptr, ptr {handle_addr}"));

            return Some(match method {
                "get_Count" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = call i32 @rt_list_size(ptr {handle})"));
                    ("i32".into(), tmp)
                }
                "Add" => {
                    if list_elem_is_ref(elem_suf, self.layouts) {
                        let item_ptr = self.list_item_to_ptr(
                            &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                            elem_ty,
                        );
                        self.emit(&format!(
                            "call void @rt_list_push(ptr {handle}, ptr {item_ptr})"
                        ));
                    } else {
                        // RFC 005：值类型 Add 直降 RtList（冷路径 ensure_capacity；无 alloca/memcpy）。
                        let (op_ty, op_val) = self.emit_operand(
                            &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        );
                        let (store_ty, store_val) = if op_ty == elem_ty {
                            (op_ty, op_val)
                        } else {
                            self.coerce_value(&op_ty, op_val, elem_ty)
                        };
                        // RFC 004 生命周期（D3）：variant 元素值深拷贝到堆（同 FieldSet）——
                        // 否则 list 缓冲存创建帧 alloca 指针，帧消亡后悬垂。
                        let store_val =
                            if self.layouts.variants.contains_key(elem_suf) && store_ty == "ptr" {
                                self.emit_variant_deep_copy(elem_suf, &store_val)
                            } else {
                                store_val
                            };
                        let elem_size = list_elem_size(elem_suf, self.layouts);
                        self.emit_list_add_value(
                            &handle, elem_ty, elem_size, &store_ty, &store_val,
                        );
                    }
                    ("void".into(), String::new())
                }
                // 索引器快路径：C# 式直访 buffer（bounds + GEP + load/store）。
                // 引用元素 set 仍走 rt_list_set（ARC）。
                "get_Item" => {
                    let idx_val = self
                        .list_index_val(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let elem_size = list_elem_size(elem_suf, self.layouts);
                    // 引用元素必须 retain：调用方 local drop 会 dec；否则与 List 共享
                    // 且 count==1 时 emit_class_drop 会误释字段（QIF WriteResults AV）。
                    self.emit_list_index_get(
                        &handle,
                        &idx_val,
                        elem_ty,
                        elem_size,
                        list_elem_is_ref(elem_suf, self.layouts),
                    )
                }
                // RFC 005 M2：List → Span（直访 data/size；扩容后视图失效）。
                "AsSpan" | "AsReadOnlySpan" => {
                    let (start, length) = match args.len() {
                        0 => (None, None),
                        2 => (Some(args[0].clone()), Some(args[1].clone())),
                        _ => return None,
                    };
                    // 复用已加载的 handle：构造临时伪路径会二次求值 receiver。
                    // 这里直接基于 handle 打包（与 emit_span_from_list 同形）。
                    let data_addr = self.fresh_temp();
                    self.emit(&format!(
                        "{data_addr} = getelementptr inbounds i8, ptr {handle}, i32 0"
                    ));
                    let data0 = self.fresh_temp();
                    self.emit(&format!("{data0} = load ptr, ptr {data_addr}"));
                    let size_addr = self.fresh_temp();
                    self.emit(&format!(
                        "{size_addr} = getelementptr inbounds i8, ptr {handle}, i32 8"
                    ));
                    let list_len = self.fresh_temp();
                    self.emit(&format!("{list_len} = load i32, ptr {size_addr}"));
                    let (data_val, len_val) = match (&start, &length) {
                        (Some(s), Some(l)) => {
                            let (_, start_v) = self.emit_operand(s);
                            let (_, len_v) = self.emit_operand(l);
                            let start_neg = self.fresh_temp();
                            self.emit(&format!("{start_neg} = icmp slt i32 {start_v}, 0"));
                            let len_neg = self.fresh_temp();
                            self.emit(&format!("{len_neg} = icmp slt i32 {len_v}, 0"));
                            let end = self.fresh_temp();
                            self.emit(&format!("{end} = add i32 {start_v}, {len_v}"));
                            let end_bad = self.fresh_temp();
                            self.emit(&format!("{end_bad} = icmp ugt i32 {end}, {list_len}"));
                            let bad1 = self.fresh_temp();
                            self.emit(&format!("{bad1} = or i1 {start_neg}, {len_neg}"));
                            let bad = self.fresh_temp();
                            self.emit(&format!("{bad} = or i1 {bad1}, {end_bad}"));
                            self.emit_span_bounds_panic(&bad);
                            let data = self.fresh_temp();
                            self.emit(&format!(
                                "{data} = getelementptr inbounds {elem_ty}, ptr {data0}, i32 {start_v}"
                            ));
                            (data, len_v)
                        }
                        _ => (data0, list_len),
                    };
                    self.emit_pack_span(&data_val, &len_val)
                }
                "set_Item" => {
                    let idx_val = self
                        .list_index_val(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    if list_elem_is_ref(elem_suf, self.layouts) {
                        let idx = format!("i32 {idx_val}");
                        let item_ptr = self.list_item_to_ptr(
                            &args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)),
                            elem_ty,
                        );
                        self.emit(&format!(
                            "call void @rt_list_set(ptr {handle}, {idx}, ptr {item_ptr})"
                        ));
                    } else {
                        let elem_size = list_elem_size(elem_suf, self.layouts);
                        let (op_ty, op_val) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let (store_ty, store_val) = if op_ty == elem_ty {
                            (op_ty, op_val)
                        } else {
                            self.coerce_value(&op_ty, op_val, elem_ty)
                        };
                        self.emit_list_index_set(
                            &handle, &idx_val, elem_ty, elem_size, &store_ty, &store_val,
                        );
                    }
                    ("void".into(), String::new())
                }
                "Contains" => {
                    let item_ptr = self.list_item_to_ptr(
                        &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        elem_ty,
                    );
                    // 接口元素特化（同 Remove）：对象身份比较（见
                    // emit_iface_list_identity_index 注记）。
                    if self.layouts.interfaces.contains_key(elem_suf) {
                        let idx = self.emit_iface_list_identity_index(&handle, &item_ptr);
                        let tmp = self.fresh_temp();
                        self.emit(&format!("{tmp} = icmp ne i32 {idx}, -1"));
                        return Some(("i1".into(), tmp));
                    }
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_list_contains(ptr {handle}, ptr {item_ptr})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "IndexOf" => {
                    let item_ptr = self.list_item_to_ptr(
                        &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        elem_ty,
                    );
                    // 接口元素特化（同 Remove）：对象身份比较。
                    if self.layouts.interfaces.contains_key(elem_suf) {
                        let idx = self.emit_iface_list_identity_index(&handle, &item_ptr);
                        return Some(("i32".into(), idx));
                    }
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_index_of(ptr {handle}, ptr {item_ptr})"
                    ));
                    ("i32".into(), tmp)
                }
                "Insert" => {
                    let idx = self
                        .list_index_arg(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let item_ptr = self.list_item_to_ptr(
                        &args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)),
                        elem_ty,
                    );
                    self.emit(&format!(
                        "call void @rt_list_insert(ptr {handle}, {idx}, ptr {item_ptr})"
                    ));
                    ("void".into(), String::new())
                }
                "RemoveAt" => {
                    let idx = self
                        .list_index_arg(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "call void @rt_list_remove_at(ptr {handle}, {idx})"
                    ));
                    ("void".into(), String::new())
                }
                "Clear" => {
                    self.emit(&format!("call void @rt_list_clear(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "Remove" => {
                    let item_ptr = self.list_item_to_ptr(
                        &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        elem_ty,
                    );
                    // 接口元素特化：每次 class→iface 转换物化**新** fat 盒
                    //（emit_make_iface heap box），rt_list_remove 的指针相等必判
                    // 「不同」——`List<IComponent>.Remove(hp)` 永不命中（l3
                    // component_store 实测）。C# 语义下接口引用相等 = 底层对象
                    // 身份相等（emit_iface_equality 同规则），此处按解盒 obj
                    // 扫描删除。同族 IndexOf/Contains 与 stub 路径同缺陷，
                    // 待同 helper 推广（案注）。
                    if self.layouts.interfaces.contains_key(elem_suf) {
                        let hit = self.emit_iface_list_identity_remove(&handle, &item_ptr);
                        return Some(("i1".into(), hit));
                    }
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_list_remove(ptr {handle}, ptr {item_ptr})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "Reverse" => {
                    self.emit(&format!("call void @rt_list_reverse(ptr {handle})"));
                    ("void".into(), String::new())
                }
                "Find" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let out = self.fresh_temp();
                    self.emit(&format!("{out} = alloca {elem_ty}"));
                    self.emit(&format!(
                        "call i32 @rt_list_find_get(ptr {handle}, ptr {pred}, ptr {out})"
                    ));
                    let r = self.fresh_temp();
                    self.emit(&format!("{r} = load {elem_ty}, ptr {out}"));
                    (elem_ty.into(), r)
                }
                "FindAll" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let new_handle = self.fresh_temp();
                    self.emit(&format!(
                        "{new_handle} = call ptr @rt_list_find_all(ptr {handle}, ptr {pred})"
                    ));
                    let obj = self.fresh_temp();
                    self.emit(&format!("{obj} = call ptr @malloc(i64 24)"));
                    self.emit(&format!("store i32 1, ptr {obj}"));
                    let vtbl = self.fresh_temp();
                    self.emit(&format!(
                        "{vtbl} = getelementptr inbounds i8, ptr {obj}, i32 8"
                    ));
                    self.emit(&format!("store ptr null, ptr {vtbl}"));
                    let hp = self.fresh_temp();
                    self.emit(&format!(
                        "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
                    ));
                    self.emit(&format!("store ptr {new_handle}, ptr {hp}"));
                    ("ptr".into(), obj)
                }
                "Exists" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_list_exists(ptr {handle}, ptr {pred})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "FindIndex" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_find_index(ptr {handle}, ptr {pred})"
                    ));
                    ("i32".into(), tmp)
                }
                "FindLastIndex" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_find_last_index(ptr {handle}, ptr {pred})"
                    ));
                    ("i32".into(), tmp)
                }
                "TrueForAll" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let raw = self.fresh_temp();
                    self.emit(&format!(
                        "{raw} = call i32 @rt_list_true_for_all(ptr {handle}, ptr {pred})"
                    ));
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                    ("i1".into(), tmp)
                }
                "LastIndexOf" => {
                    let item_ptr = self.list_item_to_ptr(
                        &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        elem_ty,
                    );
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_last_index_of(ptr {handle}, ptr {item_ptr})"
                    ));
                    ("i32".into(), tmp)
                }
                "ForEach" => {
                    // 捕获 lambda 直降（ForEach_AppliesAction 根因修复）：rt_list_for_each
                    // 的 action ABI 是单参 C 函数指针 `action(elem_slot)`（rt_list.c：
                    // `data + i*elem_size` 逐元素调用）。捕获 lambda 的值是
                    // arc_closure{fn, env} 结构指针，直传会被当代码地址调用 → NX。
                    // 裸 FnPtr（无捕获 lambda，ABI 与 action 完全一致）保留 rt 快路径；
                    // 其余（MirOperand::Closure 捕获 lambda、CD-23 统一存 arc_closure
                    // 的委托局部/字段）内联展开循环：解包 fn/env 后按 env==null 运行时
                    // 分支（RFC 008 双路径，同 emit_closure_indirect_call）——bare
                    // `fn(elem_slot)` / capturing `fn(env, elem_slot)`；元素一律以
                    // ptr 槽位传递（lambda 形参按槽 load 取值，值/引用元素同构）。
                    let arg = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                    let inline = matches!(
                        arg,
                        MirOperand::Closure { .. }
                            | MirOperand::Local(_)
                            | MirOperand::Field { .. }
                    );
                    if !inline {
                        let (_, pred) = self.emit_operand(&arg);
                        self.emit(&format!(
                            "call void @rt_list_for_each(ptr {handle}, ptr {pred})"
                        ));
                    } else {
                        let elem_size = list_elem_size(elem_suf, self.layouts);
                        let (_, closure_val) = self.emit_operand_as_closure(&arg);
                        let fn_field = self.fresh_temp();
                        self.emit(&format!(
                            "{fn_field} = getelementptr %arc_closure, ptr {closure_val}, i32 0, i32 0"
                        ));
                        let fn_ptr = self.fresh_temp();
                        self.emit(&format!("{fn_ptr} = load ptr, ptr {fn_field}"));
                        let env_field = self.fresh_temp();
                        self.emit(&format!(
                            "{env_field} = getelementptr %arc_closure, ptr {closure_val}, i32 0, i32 1"
                        ));
                        let env_ptr = self.fresh_temp();
                        self.emit(&format!("{env_ptr} = load ptr, ptr {env_field}"));
                        let env_is_null = self.fresh_temp();
                        self.emit(&format!("{env_is_null} = icmp eq ptr {env_ptr}, null"));
                        let idx_slot = self.fresh_temp();
                        self.emit(&format!("{idx_slot} = alloca i32"));
                        self.emit(&format!("store i32 0, ptr {idx_slot}"));
                        let lbl_bare = self.fresh_label();
                        let lbl_bare_body = self.fresh_label();
                        let lbl_cap = self.fresh_label();
                        let lbl_cap_body = self.fresh_label();
                        let lbl_done = self.fresh_label();
                        self.emit(&format!(
                            "br i1 {env_is_null}, label %{lbl_bare}, label %{lbl_cap}"
                        ));

                        // 裸 lambda 路径：fn(elem_slot)——与 rt action ABI 相同。
                        // count/data 每迭代重读（lambda 内 Add 扩容后仍一致）。
                        self.emit_label(&lbl_bare);
                        let size_addr_b = self.fresh_temp();
                        self.emit(&format!(
                            "{size_addr_b} = getelementptr inbounds i8, ptr {handle}, i32 8"
                        ));
                        let n_b = self.fresh_temp();
                        self.emit(&format!("{n_b} = load i32, ptr {size_addr_b}"));
                        let i_b = self.fresh_temp();
                        self.emit(&format!("{i_b} = load i32, ptr {idx_slot}"));
                        let cond_b = self.fresh_temp();
                        self.emit(&format!("{cond_b} = icmp slt i32 {i_b}, {n_b}"));
                        self.emit(&format!(
                            "br i1 {cond_b}, label %{lbl_bare_body}, label %{lbl_done}"
                        ));
                        self.emit_label(&lbl_bare_body);
                        let data_addr_b = self.fresh_temp();
                        self.emit(&format!(
                            "{data_addr_b} = getelementptr inbounds i8, ptr {handle}, i32 0"
                        ));
                        let data_b = self.fresh_temp();
                        self.emit(&format!("{data_b} = load ptr, ptr {data_addr_b}"));
                        let off_b = self.fresh_temp();
                        self.emit(&format!("{off_b} = mul i32 {i_b}, {elem_size}"));
                        let eslot_b = self.fresh_temp();
                        self.emit(&format!(
                            "{eslot_b} = getelementptr inbounds i8, ptr {data_b}, i32 {off_b}"
                        ));
                        self.emit_call_may_throw(
                            "void",
                            &fn_ptr,
                            &format!("ptr {eslot_b}"),
                            true,
                            None,
                        );
                        let i2_b = self.fresh_temp();
                        self.emit(&format!("{i2_b} = add i32 {i_b}, 1"));
                        self.emit(&format!("store i32 {i2_b}, ptr {idx_slot}"));
                        self.emit(&format!("br label %{lbl_bare}"));

                        // 捕获 lambda 路径：fn(env, elem_slot)——__env__ 首参
                        // （RFC 008），捕获读写在 lambda 体内经 env 字段解引用；
                        // ByRef 捕获直接写回外层权威槽（sum += x 语义成立）。
                        self.emit_label(&lbl_cap);
                        let size_addr_c = self.fresh_temp();
                        self.emit(&format!(
                            "{size_addr_c} = getelementptr inbounds i8, ptr {handle}, i32 8"
                        ));
                        let n_c = self.fresh_temp();
                        self.emit(&format!("{n_c} = load i32, ptr {size_addr_c}"));
                        let i_c = self.fresh_temp();
                        self.emit(&format!("{i_c} = load i32, ptr {idx_slot}"));
                        let cond_c = self.fresh_temp();
                        self.emit(&format!("{cond_c} = icmp slt i32 {i_c}, {n_c}"));
                        self.emit(&format!(
                            "br i1 {cond_c}, label %{lbl_cap_body}, label %{lbl_done}"
                        ));
                        self.emit_label(&lbl_cap_body);
                        let data_addr_c = self.fresh_temp();
                        self.emit(&format!(
                            "{data_addr_c} = getelementptr inbounds i8, ptr {handle}, i32 0"
                        ));
                        let data_c = self.fresh_temp();
                        self.emit(&format!("{data_c} = load ptr, ptr {data_addr_c}"));
                        let off_c = self.fresh_temp();
                        self.emit(&format!("{off_c} = mul i32 {i_c}, {elem_size}"));
                        let eslot_c = self.fresh_temp();
                        self.emit(&format!(
                            "{eslot_c} = getelementptr inbounds i8, ptr {data_c}, i32 {off_c}"
                        ));
                        self.emit_call_may_throw(
                            "void",
                            &fn_ptr,
                            &format!("ptr {env_ptr}, ptr {eslot_c}"),
                            true,
                            None,
                        );
                        let i2_c = self.fresh_temp();
                        self.emit(&format!("{i2_c} = add i32 {i_c}, 1"));
                        self.emit(&format!("store i32 {i2_c}, ptr {idx_slot}"));
                        self.emit(&format!("br label %{lbl_cap}"));

                        self.emit_label(&lbl_done);
                    }
                    ("void".into(), String::new())
                }
                "RemoveAll" => {
                    let (_, pred) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_remove_all(ptr {handle}, ptr {pred})"
                    ));
                    ("i32".into(), tmp)
                }
                "Sort" => {
                    if args.is_empty() {
                        self.emit(&format!("call void @rt_list_sort_default(ptr {handle})"));
                    } else {
                        let (_, cmp) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        self.emit(&format!("call void @rt_list_sort(ptr {handle}, ptr {cmp})"));
                    }
                    ("void".into(), String::new())
                }
                "ToArray" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = call ptr @rt_list_to_array(ptr {handle})"));
                    ("ptr".into(), tmp)
                }
                "CopyTo" => {
                    let (_, array) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let start = self
                        .list_index_arg(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "call void @rt_list_copy_to(ptr {handle}, ptr {array}, {start})"
                    ));
                    ("void".into(), String::new())
                }
                "GetEnumerator" => {
                    let size = self.fresh_temp();
                    self.emit(&format!("{size} = call i32 @rt_list_size(ptr {handle})"));
                    let obj = self.fresh_temp();
                    self.emit(&format!("{obj} = call ptr @malloc(i64 32)"));
                    self.emit(&format!("store i32 1, ptr {obj}"));
                    let vt = self.fresh_temp();
                    self.emit(&format!(
                        "{vt} = getelementptr inbounds i8, ptr {obj}, i32 8"
                    ));
                    self.emit(&format!(
                        "store ptr @.itable.ListEnumerator_{elem_suf}_IEnumerator_{elem_suf}, ptr {vt}"
                    ));
                    let hp = self.fresh_temp();
                    self.emit(&format!(
                        "{hp} = getelementptr inbounds i8, ptr {obj}, i32 16"
                    ));
                    self.emit(&format!("store ptr {handle}, ptr {hp}"));
                    let ip = self.fresh_temp();
                    self.emit(&format!(
                        "{ip} = getelementptr inbounds i8, ptr {obj}, i32 24"
                    ));
                    self.emit(&format!("store i32 -1, ptr {ip}"));
                    let cp = self.fresh_temp();
                    self.emit(&format!(
                        "{cp} = getelementptr inbounds i8, ptr {obj}, i32 28"
                    ));
                    self.emit(&format!("store i32 {size}, ptr {cp}"));
                    let fat = self.fresh_temp();
                    self.emit(&format!("{fat} = alloca {{ ptr, ptr }}"));
                    let fat_obj = self.fresh_temp();
                    self.emit(&format!(
                        "{fat_obj} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 0"
                    ));
                    self.emit(&format!("store ptr {obj}, ptr {fat_obj}"));
                    let fat_vt = self.fresh_temp();
                    self.emit(&format!(
                        "{fat_vt} = getelementptr inbounds {{ ptr, ptr }}, ptr {fat}, i32 0, i32 1"
                    ));
                    self.emit(&format!(
                        "store ptr @.itable.ListEnumerator_{elem_suf}_IEnumerator_{elem_suf}, ptr {fat_vt}"
                    ));
                    ("ptr".into(), fat)
                }
                "AddRange" => {
                    // The formal parameter is `IEnumerable<T>` (interface). MIR
                    // lowering (`maybe_box_iface`) boxes a `List<T>` argument
                    // into `MirOperand::Iface` — a fat pointer `{ ptr obj, ptr
                    // vtable }` (16 bytes). The builtin path needs the bare
                    // List object pointer to read the `_handle` field at offset
                    // 16, so unwrap the iface and emit the inner object operand
                    // directly. Without this, `getelementptr ... i32 16` on the
                    // 16-byte fat struct reads past its end (UB) and crashes.
                    let src_op = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                    let src_obj_op = match &src_op {
                        MirOperand::Iface { object, .. } => (**object).clone(),
                        _ => src_op,
                    };
                    let (_, src) = self.emit_operand(&src_obj_op);
                    let src_hp = self.fresh_temp();
                    self.emit(&format!(
                        "{src_hp} = getelementptr inbounds i8, ptr {src}, i32 16"
                    ));
                    let src_handle = self.fresh_temp();
                    self.emit(&format!("{src_handle} = load ptr, ptr {src_hp}"));
                    self.emit(&format!(
                        "call void @rt_list_add_range_list(ptr {handle}, ptr {src_handle})"
                    ));
                    ("void".into(), String::new())
                }
                // P5-H: Capacity / IsReadOnly / RemoveRange / TrimExcess
                "get_Capacity" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = call i32 @rt_list_capacity(ptr {handle})"));
                    ("i32".into(), tmp)
                }
                "set_Capacity" => {
                    let (_, cap) = self
                        .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "call void @rt_list_set_capacity(ptr {handle}, i32 {cap})"
                    ));
                    ("void".into(), String::new())
                }
                "get_IsReadOnly" => {
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_list_is_read_only(ptr {handle})"
                    ));
                    let b = self.fresh_temp();
                    self.emit(&format!("{b} = icmp ne i32 {tmp}, 0"));
                    ("i1".into(), b)
                }
                "RemoveRange" => {
                    let idx = self
                        .list_index_arg(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                    let cnt = self
                        .list_index_arg(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "call void @rt_list_remove_range(ptr {handle}, {idx}, {cnt})"
                    ));
                    ("void".into(), String::new())
                }
                "TrimExcess" => {
                    self.emit(&format!("call void @rt_list_trim_excess(ptr {handle})"));
                    ("void".into(), String::new())
                }
                // C# 二分查找：key 必须以指针进入 rt（memcmp 按元素字节比较）。
                // 旧路径穿透到普通 fallback 后按值传 i32，与 stub ABI 错位——x64 下
                // RDX=值被当 %cmp/%key 指针解引用 → 0xc0000005（UnitTest BinarySearch
                // 崩溃根因）。仿 Contains：list_item_to_ptr（entry hoisted alloca）。
                m if m == "BinarySearch" || m.starts_with("BinarySearch_") => {
                    if args.len() >= 2 {
                        // comparer 重载：rt_list_binary_search_cmp 需要 C 函数指针，
                        // Arc 侧 IComparer<T> 是对象——无回调桥接，确定性 panic 优于
                        // 把对象指针当函数指针调用。
                        self.emit("call void @rt_panic(ptr @__arc_list_cmp_unsupported)");
                        ("i32".into(), "0".into())
                    } else {
                        let item_ptr = self.list_item_to_ptr(
                            &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                            elem_ty,
                        );
                        let raw = self.fresh_temp();
                        self.emit(&format!(
                            "{raw} = call i32 @rt_list_binary_search(ptr {handle}, ptr {item_ptr})"
                        ));
                        ("i32".into(), raw)
                    }
                }
                _ => return None,
            });
        }

        None
    }

    /// Store a List<T> item operand into an alloca buffer and return the buffer ptr.
    /// Coerces the operand to `elem_ty` if the MIR-level type differs (e.g. i32 literal → i64 slot).
    ///
    /// The alloca is hoisted to the function entry block (`entry_allocas`):
    /// emitting it at the current insertion point inside a loop body would
    /// allocate a fresh stack slot on every iteration, never reclaimed until
    /// function exit — `List<Class>.Add` in a loop leaks ~16B/Add and
    /// overflows the default 1MB stack after ~64k adds (0xC00000FD, RFC 005 ③
    /// churn harness). The store stays at the call site; the slot is reused.
    fn list_item_to_ptr(&mut self, operand: &MirOperand, elem_ty: &str) -> String {
        let (op_ty, op_val) = self.emit_operand(operand);
        let buf = self.fresh_temp();
        self.entry_allocas
            .push_str(&format!("  {buf} = alloca {elem_ty}\n"));
        let (store_ty, store_val) = if op_ty == elem_ty {
            (op_ty, op_val)
        } else {
            self.coerce_value(&op_ty, op_val, elem_ty)
        };
        self.emit(&format!("store {store_ty} {store_val}, ptr {buf}"));
        buf
    }

    /// Emit a List index argument, coercing to `i32` if needed (runtime expects i32).
    fn list_index_arg(&mut self, operand: &MirOperand) -> String {
        format!("i32 {}", self.list_index_val(operand))
    }

    /// Index value (i32 SSA) for List indexer / Get / Set.
    fn list_index_val(&mut self, operand: &MirOperand) -> String {
        let (idx_ty, idx_val) = self.emit_operand(operand);
        if idx_ty == "i32" {
            idx_val
        } else {
            self.coerce_value(&idx_ty, idx_val, "i32").1
        }
    }

    /// C# 式 List 索引读：load data@0 / size@8 → unsigned 越界检查 → GEP → load。
    /// 无 `rt_list_get` 调用、无 alloca/memcpy。
    fn emit_list_index_get(
        &mut self,
        handle: &str,
        idx_val: &str,
        elem_ty: &str,
        elem_size: i32,
        retain_ref: bool,
    ) -> TyVal {
        let slot = self.emit_list_index_slot(handle, idx_val, elem_size);
        let r = self.fresh_temp();
        self.emit(&format!("{r} = load {elem_ty}, ptr {slot}"));
        if retain_ref {
            // elem_ty is ptr for class elements.
            self.emit(&format!("call void @rt_arc_inc(ptr {r})"));
        }
        (elem_ty.into(), r)
    }

    /// C# 式 List 索引写（值类型）：直访 store；引用类型请走 `rt_list_set`。
    fn emit_list_index_set(
        &mut self,
        handle: &str,
        idx_val: &str,
        elem_ty: &str,
        elem_size: i32,
        store_ty: &str,
        store_val: &str,
    ) {
        let _ = elem_ty;
        let slot = self.emit_list_index_slot(handle, idx_val, elem_size);
        self.emit(&format!("store {store_ty} {store_val}, ptr {slot}"));
    }

    /// RFC 005：值类型 `Add` 直降 — size/capacity 检查；满则 `rt_list_ensure_capacity`；
    /// 再 GEP+store+size++。无 `rt_list_push`、无 alloca/memcpy。
    fn emit_list_add_value(
        &mut self,
        handle: &str,
        elem_ty: &str,
        elem_size: i32,
        store_ty: &str,
        store_val: &str,
    ) {
        let _ = elem_ty;
        let size_addr = self.fresh_temp();
        self.emit(&format!(
            "{size_addr} = getelementptr inbounds i8, ptr {handle}, i32 8"
        ));
        let size = self.fresh_temp();
        self.emit(&format!("{size} = load i32, ptr {size_addr}"));
        let cap_addr = self.fresh_temp();
        self.emit(&format!(
            "{cap_addr} = getelementptr inbounds i8, ptr {handle}, i32 12"
        ));
        let cap = self.fresh_temp();
        self.emit(&format!("{cap} = load i32, ptr {cap_addr}"));
        let need_grow = self.fresh_temp();
        self.emit(&format!("{need_grow} = icmp uge i32 {size}, {cap}"));
        let grow_bb = self.fresh_label();
        let ready_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {need_grow}, label %{grow_bb}, label %{ready_bb}"
        ));
        self.emit_label(&grow_bb);
        let needed = self.fresh_temp();
        self.emit(&format!("{needed} = add i32 {size}, 1"));
        self.emit(&format!(
            "call void @rt_list_ensure_capacity(ptr {handle}, i32 {needed})"
        ));
        self.emit(&format!("br label %{ready_bb}"));
        self.emit_label(&ready_bb);
        // 扩容后 data 可能已 realloc；必须在 merge 后重载。
        let data_addr = self.fresh_temp();
        self.emit(&format!(
            "{data_addr} = getelementptr inbounds i8, ptr {handle}, i32 0"
        ));
        let data = self.fresh_temp();
        self.emit(&format!("{data} = load ptr, ptr {data_addr}"));
        let size2 = self.fresh_temp();
        self.emit(&format!("{size2} = load i32, ptr {size_addr}"));
        let byte_off = self.fresh_temp();
        self.emit(&format!("{byte_off} = mul i32 {size2}, {elem_size}"));
        let slot = self.fresh_temp();
        self.emit(&format!(
            "{slot} = getelementptr inbounds i8, ptr {data}, i32 {byte_off}"
        ));
        self.emit(&format!("store {store_ty} {store_val}, ptr {slot}"));
        let new_size = self.fresh_temp();
        self.emit(&format!("{new_size} = add i32 {size2}, 1"));
        self.emit(&format!("store i32 {new_size}, ptr {size_addr}"));
    }

    /// RFC 005 式直降：`StringBuilder.Append(char)` 内联为 `rt_sb_t` 直接字段写
    /// （布局契约：data@0 / len@8 / cap@16，len/cap 为 size_t → i64，勿改字段序）。
    /// 容量不足走冷路径回调 `rt_text_sb_append_char`（C 侧扩容 + len 维护），
    /// 与 std_hotpath_bench_e2e `cbench_sb_append` 的直降镜像一致。不写 NUL：
    /// `rt_text_sb_to_string` 自落 NUL（懒终止契约，见 rt_text.c）。
    fn emit_sb_append_char_inline(&mut self, handle: &str, char_val: &str) {
        // 024 A1：sb 头字段与 data 缓冲注入互不 alias 的 TBAA 元数据。
        // 头（data@0/len@8/cap@16）与缓冲（ensure 独立分配）是不同对象，
        // 字段级 offset 还使 len store 不冲掉 data/cap——LLVM 遂可把 data/cap
        // 提升出追加循环、len 保持寄存器变量（详见 debug_info.rs sb_tbaa）。
        let tbaa = self.dbg.sb_tbaa();
        // 045 M4：缓冲 scoped-noalias。`data` 指向 ensure 独立分配的字符缓冲，
        // 与 `rt_sb_t` 头（独立 malloc）永不相交——缓冲 store 挂 `!alias.scope`，
        // 头字段访问挂 `!noalias`，以 restrict 契约显式声明（[034 A1] 互补 TBAA）。
        let scope = self.dbg.sb_alias_scope();
        // RFC 005 游标提升：纯追加循环内，头字段已提升为 shadow alloca
        // （`emit_nested_while` 判定），热路径改读写 shadow，冷路径 flush/reload。
        if let Some(sh) = self.sb_shadow.clone() {
            let data_addr = self.fresh_temp();
            self.emit(&format!(
                "{data_addr} = getelementptr inbounds i8, ptr {}, i32 0",
                sh.handle
            ));
            let len_addr = self.fresh_temp();
            self.emit(&format!(
                "{len_addr} = getelementptr inbounds i8, ptr {}, i32 8",
                sh.handle
            ));
            let cap_addr = self.fresh_temp();
            self.emit(&format!(
                "{cap_addr} = getelementptr inbounds i8, ptr {}, i32 16",
                sh.handle
            ));
            let data = self.fresh_temp();
            self.emit(&format!(
                "{data} = load ptr, ptr {}, !tbaa !{}",
                sh.data, tbaa.data
            ));
            let len = self.fresh_temp();
            self.emit(&format!(
                "{len} = load i64, ptr {}, !tbaa !{}",
                sh.len, tbaa.len
            ));
            let cap = self.fresh_temp();
            self.emit(&format!(
                "{cap} = load i64, ptr {}, !tbaa !{}",
                sh.cap, tbaa.cap
            ));
            let needed = self.fresh_temp();
            self.emit(&format!("{needed} = add i64 {len}, 2"));
            let grow = self.fresh_temp();
            self.emit(&format!("{grow} = icmp ugt i64 {needed}, {cap}"));
            let grow_bb = self.fresh_label();
            let inline_bb = self.fresh_label();
            let done_bb = self.fresh_label();
            self.emit(&format!(
                "br i1 {grow}, label %{grow_bb}, label %{inline_bb}"
            ));
            // 冷路径：shadow 尚未写回头，先 flush 三字段，再调用（读 header.len
            // 决定追加位置，capacity 不足则 realloc），调用后 re-sync reload。
            self.emit_label(&grow_bb);
            self.emit(&format!(
                "store ptr {data}, ptr {data_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.data, scope.scope
            ));
            self.emit(&format!(
                "store i64 {len}, ptr {len_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.len, scope.scope
            ));
            self.emit(&format!(
                "store i64 {cap}, ptr {cap_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.cap, scope.scope
            ));
            self.emit(&format!(
                "call ptr @rt_text_sb_append_char(ptr {}, i32 {char_val})",
                sh.handle
            ));
            let ndata = self.fresh_temp();
            self.emit(&format!(
                "{ndata} = load ptr, ptr {data_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.data, scope.scope
            ));
            let nlen = self.fresh_temp();
            self.emit(&format!(
                "{nlen} = load i64, ptr {len_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.len, scope.scope
            ));
            let ncap = self.fresh_temp();
            self.emit(&format!(
                "{ncap} = load i64, ptr {cap_addr}, !tbaa !{}, !noalias !{{!{}}}",
                tbaa.cap, scope.scope
            ));
            self.emit(&format!("store ptr {ndata}, ptr {}", sh.data));
            self.emit(&format!("store i64 {nlen}, ptr {}", sh.len));
            self.emit(&format!("store i64 {ncap}, ptr {}", sh.cap));
            self.emit(&format!("br label %{done_bb}"));
            // 热路径：纯寄存器追加，仅写缓冲 + 更新 shadow.len。
            self.emit_label(&inline_bb);
            let char8 = self.fresh_temp();
            self.emit(&format!("{char8} = trunc i32 {char_val} to i8"));
            let slot = self.fresh_temp();
            self.emit(&format!(
                "{slot} = getelementptr inbounds i8, ptr {data}, i64 {len}"
            ));
            self.emit(&format!(
                "store i8 {char8}, ptr {slot}, !tbaa !{}, !alias.scope !{{!{}}}",
                tbaa.buffer, scope.scope
            ));
            let len2 = self.fresh_temp();
            self.emit(&format!("{len2} = add i64 {len}, 1"));
            self.emit(&format!("store i64 {len2}, ptr {}", sh.len));
            self.emit(&format!("br label %{done_bb}"));
            self.emit_label(&done_bb);
            return;
        }
        let data_addr = self.fresh_temp();
        self.emit(&format!(
            "{data_addr} = getelementptr inbounds i8, ptr {handle}, i32 0"
        ));
        let data = self.fresh_temp();
        self.emit(&format!(
            "{data} = load ptr, ptr {data_addr}, !tbaa !{}, !noalias !{{!{}}}",
            tbaa.data, scope.scope
        ));
        let len_addr = self.fresh_temp();
        self.emit(&format!(
            "{len_addr} = getelementptr inbounds i8, ptr {handle}, i32 8"
        ));
        let len = self.fresh_temp();
        self.emit(&format!(
            "{len} = load i64, ptr {len_addr}, !tbaa !{}, !noalias !{{!{}}}",
            tbaa.len, scope.scope
        ));
        let cap_addr = self.fresh_temp();
        self.emit(&format!(
            "{cap_addr} = getelementptr inbounds i8, ptr {handle}, i32 16"
        ));
        let cap = self.fresh_temp();
        self.emit(&format!(
            "{cap} = load i64, ptr {cap_addr}, !tbaa !{}, !noalias !{{!{}}}",
            tbaa.cap, scope.scope
        ));
        let needed = self.fresh_temp();
        self.emit(&format!("{needed} = add i64 {len}, 2"));
        let grow = self.fresh_temp();
        self.emit(&format!("{grow} = icmp ugt i64 {needed}, {cap}"));
        let grow_bb = self.fresh_label();
        let inline_bb = self.fresh_label();
        let done_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {grow}, label %{grow_bb}, label %{inline_bb}"
        ));
        self.emit_label(&grow_bb);
        self.emit(&format!(
            "call ptr @rt_text_sb_append_char(ptr {handle}, i32 {char_val})"
        ));
        self.emit(&format!("br label %{done_bb}"));
        self.emit_label(&inline_bb);
        let char8 = self.fresh_temp();
        self.emit(&format!("{char8} = trunc i32 {char_val} to i8"));
        let slot = self.fresh_temp();
        self.emit(&format!(
            "{slot} = getelementptr inbounds i8, ptr {data}, i64 {len}"
        ));
        self.emit(&format!(
            "store i8 {char8}, ptr {slot}, !tbaa !{}, !alias.scope !{{!{}}}",
            tbaa.buffer, scope.scope
        ));
        let len2 = self.fresh_temp();
        self.emit(&format!("{len2} = add i64 {len}, 1"));
        self.emit(&format!(
            "store i64 {len2}, ptr {len_addr}, !tbaa !{}, !noalias !{{!{}}}",
            tbaa.len, scope.scope
        ));
        self.emit(&format!("br label %{done_bb}"));
        self.emit_label(&done_bb);
    }

    /// RtList 直访槽指针：`data + idx * elem_size`（含 OOB → `rt_panic`）。
    fn emit_list_index_slot(&mut self, handle: &str, idx_val: &str, elem_size: i32) -> String {
        // 045 M1：RtList 头 data@0/size@8 注入互不 alias 的 TBAA 元数据，
        // 使越界检查的 size 读与 data GEP 可独立调度（对齐 sb_tbaa 先例）。
        let tbaa = self.dbg.rt_list_tbaa();
        let data_addr = self.fresh_temp();
        self.emit(&format!(
            "{data_addr} = getelementptr inbounds i8, ptr {handle}, i32 0"
        ));
        let data = self.fresh_temp();
        self.emit(&format!(
            "{data} = load ptr, ptr {data_addr}, !tbaa !{}",
            tbaa.data
        ));
        let size_addr = self.fresh_temp();
        self.emit(&format!(
            "{size_addr} = getelementptr inbounds i8, ptr {handle}, i32 8"
        ));
        let size = self.fresh_temp();
        self.emit(&format!(
            "{size} = load i32, ptr {size_addr}, !tbaa !{}",
            tbaa.size
        ));
        // unsigned 比较：负下标变为大正数，一并越界。
        let in_bounds = self.fresh_temp();
        self.emit(&format!("{in_bounds} = icmp ult i32 {idx_val}, {size}"));
        let ok_bb = self.fresh_label();
        let oob_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {in_bounds}, label %{ok_bb}, label %{oob_bb}"
        ));
        self.emit_label(&oob_bb);
        self.emit("call void @rt_panic(ptr @__arc_list_oob)");
        self.emit("unreachable");
        self.emit_label(&ok_bb);
        let byte_off = self.fresh_temp();
        self.emit(&format!("{byte_off} = mul i32 {idx_val}, {elem_size}"));
        let slot = self.fresh_temp();
        self.emit(&format!(
            "{slot} = getelementptr inbounds i8, ptr {data}, i32 {byte_off}"
        ));
        slot
    }

    /// Coerce a value to i32 for the StringBuilder ABI (insert/remove/range indices).
    fn sb_coerce_i32(&mut self, ty: &str, val: &str) -> String {
        if ty == "i32" {
            val.to_string()
        } else {
            self.coerce_value(ty, val.to_string(), "i32").1
        }
    }

    /// Emit a string method call as a direct `rt_str_*` ABI invocation.
    ///
    /// Returns `None` for unknown method names so the caller can fall through
    /// to the normal method-call path. All string methods return either a
    /// freshly allocated `ptr` (new string / array) or an `i1`/`i32` scalar.
    fn emit_string_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let str_ret = |tmp: String| -> TyVal { ("ptr".into(), tmp) };
        Some(match method {
            "Split" => self.emit_string_split(&recv, args, SplitEmitKind::Basic),
            "SplitMulti" => self.emit_string_split(&recv, args, SplitEmitKind::Multi),
            "SplitMultiOpts" => self.emit_string_split(&recv, args, SplitEmitKind::MultiOpts),
            "SplitCount" => self.emit_string_split(&recv, args, SplitEmitKind::Count),
            "SplitMultiCount" => self.emit_string_split(&recv, args, SplitEmitKind::MultiCount),
            "Replace" => {
                let (_, old) = self.emit_operand(&args[0]);
                let (_, neu) = self.emit_operand(&args[1]);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_replace(ptr {recv}, ptr {old}, ptr {neu})"
                ));
                str_ret(tmp)
            }
            "Substring" => {
                let start = self.string_int_arg(&args[0]);
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let len = self.string_int_arg(&args[1]);
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_substring(ptr {recv}, {start}, {len})"
                    ));
                } else {
                    // length = -1 → to end of string.
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_substring(ptr {recv}, {start}, i32 -1)"
                    ));
                }
                str_ret(tmp)
            }
            "Contains" => {
                let (_, sub) = self.emit_operand(&args[0]);
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_str_contains(ptr {recv}, ptr {sub})"
                ));
                self.bool_from_i32(&raw)
            }
            "IndexOf" => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                if args.len() >= 2 {
                    let start = self.string_int_arg(&args[1]);
                    match arg_ty.as_str() {
                        "i32" => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_index_of_char_from(ptr {recv}, i32 {arg_val}, {start})"
                            ));
                            ("i32".into(), tmp)
                        }
                        _ => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_index_of_from(ptr {recv}, ptr {arg_val}, {start})"
                            ));
                            ("i32".into(), tmp)
                        }
                    }
                } else {
                    match arg_ty.as_str() {
                        "i32" => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_index_of_char(ptr {recv}, i32 {arg_val})"
                            ));
                            ("i32".into(), tmp)
                        }
                        _ => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_index_of(ptr {recv}, ptr {arg_val})"
                            ));
                            ("i32".into(), tmp)
                        }
                    }
                }
            }
            "LastIndexOf" => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                if args.len() >= 2 {
                    let start = self.string_int_arg(&args[1]);
                    match arg_ty.as_str() {
                        "i32" => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_last_index_of_char_from(ptr {recv}, i32 {arg_val}, {start})"
                            ));
                            ("i32".into(), tmp)
                        }
                        _ => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_last_index_of_from(ptr {recv}, ptr {arg_val}, {start})"
                            ));
                            ("i32".into(), tmp)
                        }
                    }
                } else {
                    match arg_ty.as_str() {
                        "i32" => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_last_index_of_char(ptr {recv}, i32 {arg_val})"
                            ));
                            ("i32".into(), tmp)
                        }
                        _ => {
                            let tmp = self.fresh_temp();
                            self.emit(&format!(
                                "{tmp} = call i32 @rt_str_last_index_of(ptr {recv}, ptr {arg_val})"
                            ));
                            ("i32".into(), tmp)
                        }
                    }
                }
            }
            "Insert" => {
                let idx = self.string_int_arg(&args[0]);
                let (_, v) = self.emit_operand(&args[1]);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_insert(ptr {recv}, {idx}, ptr {v})"
                ));
                str_ret(tmp)
            }
            "Remove" => {
                let start = self.string_int_arg(&args[0]);
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let count = self.string_int_arg(&args[1]);
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_remove(ptr {recv}, {start}, {count})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_remove(ptr {recv}, {start}, i32 -1)"
                    ));
                }
                str_ret(tmp)
            }
            "StartsWith" => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                let raw = self.fresh_temp();
                if arg_ty == "i32" {
                    self.emit(&format!(
                        "{raw} = call i32 @rt_str_starts_with_char(ptr {recv}, i32 {arg_val})"
                    ));
                } else {
                    self.emit(&format!(
                        "{raw} = call i32 @rt_str_starts_with(ptr {recv}, ptr {arg_val})"
                    ));
                }
                self.bool_from_i32(&raw)
            }
            "EndsWith" => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                let raw = self.fresh_temp();
                if arg_ty == "i32" {
                    self.emit(&format!(
                        "{raw} = call i32 @rt_str_ends_with_char(ptr {recv}, i32 {arg_val})"
                    ));
                } else {
                    self.emit(&format!(
                        "{raw} = call i32 @rt_str_ends_with(ptr {recv}, ptr {arg_val})"
                    ));
                }
                self.bool_from_i32(&raw)
            }
            "Trim" => self.emit_string_trim(
                &recv,
                args,
                "rt_str_trim",
                "rt_str_trim_char",
                "rt_str_trim_chars",
            ),
            "TrimStart" => self.emit_string_trim(
                &recv,
                args,
                "rt_str_trim_start",
                "rt_str_trim_start_char",
                "rt_str_trim_start_chars",
            ),
            "TrimEnd" => self.emit_string_trim(
                &recv,
                args,
                "rt_str_trim_end",
                "rt_str_trim_end_char",
                "rt_str_trim_end_chars",
            ),
            "PadLeft" => {
                let width = self.string_int_arg(&args[0]);
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let pad = self.string_int_arg(&args[1]);
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_pad_left_char(ptr {recv}, {width}, {pad})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_pad_left(ptr {recv}, {width})"
                    ));
                }
                str_ret(tmp)
            }
            "PadRight" => {
                let width = self.string_int_arg(&args[0]);
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let pad = self.string_int_arg(&args[1]);
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_pad_right_char(ptr {recv}, {width}, {pad})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_pad_right(ptr {recv}, {width})"
                    ));
                }
                str_ret(tmp)
            }
            "ToUpper" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_str_to_upper(ptr {recv})"));
                str_ret(tmp)
            }
            "ToLower" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_str_to_lower(ptr {recv})"));
                str_ret(tmp)
            }
            // 与静态 `string.GetHashCode(s)` / emit_prim_get_hash_code 对齐（DJB2 内容哈希）。
            "GetHashCode" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_hash_str(ptr {recv})"));
                ("i32".into(), tmp)
            }
            // IComparable<string>：实例 CompareTo(other) 与 static Compare(a,b) 同源，
            // 均三值化 strcmp → rt_str_compare（-1/0/1）。基元实例 CompareTo 走
            // emit_builtin_method_call 的 prim_ty 内联（icmp/fcmp）；string 为 ptr，
            // 无法内联整数/浮点比较，须在此统一落到运行时 ABI。
            "Compare" | "CompareTo" => {
                let (_, other) = self.emit_operand(&args[0]);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_str_compare(ptr {recv}, ptr {other})"
                ));
                ("i32".into(), tmp)
            }
            "ToCharArray" => {
                // ToCharArray() | ToCharArray(start, length); range clamp = Substring.
                let tmp = self.fresh_temp();
                if args.len() >= 2 {
                    let start = self.string_int_arg(&args[0]);
                    let len = self.string_int_arg(&args[1]);
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_to_char_array_range(ptr {recv}, {start}, {len})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_to_char_array(ptr {recv})"
                    ));
                }
                str_ret(tmp)
            }
            // RFC 005 M2：UTF-8 诚实 → ReadOnlySpan<byte>（零拷贝码元视图）。
            // `recv` 已由上方 `emit_operand` 物化，避免二次求值。
            "AsSpan" => {
                let (start, length) = match args.len() {
                    0 => (None, None),
                    2 => (Some(&args[0]), Some(&args[1])),
                    _ => return None,
                };
                self.emit_span_from_string_ptr(&recv, start, length)
            }
            // C# `string.Chars` / `s[i]` → UTF-8 code unit as char（与 Length 对齐）。
            "get_Chars" => {
                let idx = self.string_int_arg(&args[0]);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_str_char_at(ptr {recv}, {idx})"
                ));
                ("i32".into(), tmp)
            }
            // string.ToString() → 返回自身（字符串已是字符串，无需复制）。
            "ToString" | "ToString_" => ("ptr".into(), recv.to_string()),
            _ => return None,
        })
    }

    /// Emit a string int argument, coercing to `i32` (runtime ABI requires i32).
    fn string_int_arg(&mut self, operand: &MirOperand) -> String {
        let (ty, val) = self.emit_operand(operand);
        if ty == "i32" {
            format!("i32 {val}")
        } else {
            let (cty, cval) = self.coerce_value(&ty, val, "i32");
            format!("{cty} {cval}")
        }
    }

    /// Pack N char operands into a temporary `int32_t[]` (same as Trim params).
    fn emit_char_params_array(&mut self, args: &[MirOperand]) -> String {
        let n = args.len();
        let arr = self.fresh_temp();
        self.emit(&format!(
            "{arr} = call ptr @rt_array_create(i32 {n}, i32 4)"
        ));
        for (i, arg) in args.iter().enumerate() {
            let c = self.string_int_arg(arg);
            let slot = self.fresh_temp();
            self.emit(&format!(
                "{slot} = getelementptr inbounds i32, ptr {arr}, i32 {i}"
            ));
            self.emit(&format!("store {c}, ptr {slot}"));
        }
        arr
    }

    fn emit_string_split(&mut self, recv: &str, args: &[MirOperand], kind: SplitEmitKind) -> TyVal {
        let tmp = self.fresh_temp();
        match kind {
            SplitEmitKind::Basic => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                if args.len() >= 2 {
                    let opts = self.string_int_arg(&args[1]);
                    match arg_ty.as_str() {
                        "i32" => self.emit(&format!(
                            "{tmp} = call ptr @rt_str_split_char_opts(ptr {recv}, i32 {arg_val}, {opts})"
                        )),
                        _ => self.emit(&format!(
                            "{tmp} = call ptr @rt_str_split_opts(ptr {recv}, ptr {arg_val}, {opts})"
                        )),
                    }
                } else {
                    match arg_ty.as_str() {
                        "i32" => self.emit(&format!(
                            "{tmp} = call ptr @rt_str_split_char(ptr {recv}, i32 {arg_val})"
                        )),
                        _ => self.emit(&format!(
                            "{tmp} = call ptr @rt_str_split(ptr {recv}, ptr {arg_val})"
                        )),
                    }
                }
            }
            SplitEmitKind::Multi => {
                let seps = if args.len() == 1 {
                    let (_, v) = self.emit_operand(&args[0]);
                    v
                } else {
                    self.emit_char_params_array(args)
                };
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_split_chars(ptr {recv}, ptr {seps})"
                ));
            }
            SplitEmitKind::MultiOpts => {
                let opts = self.string_int_arg(args.last().unwrap());
                let seps_args = &args[..args.len() - 1];
                let seps = if seps_args.len() == 1 {
                    let (ty, v) = self.emit_operand(&seps_args[0]);
                    if ty == "i32" {
                        self.emit_char_params_array(seps_args)
                    } else {
                        v
                    }
                } else {
                    self.emit_char_params_array(seps_args)
                };
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_split_chars_opts(ptr {recv}, ptr {seps}, {opts})"
                ));
            }
            SplitEmitKind::Count => {
                let (arg_ty, arg_val) = self.emit_operand(&args[0]);
                let count = self.string_int_arg(&args[1]);
                let opts = self.string_int_arg(&args[2]);
                match arg_ty.as_str() {
                    "i32" => self.emit(&format!(
                        "{tmp} = call ptr @rt_str_split_char_opts_count(ptr {recv}, i32 {arg_val}, {count}, {opts})"
                    )),
                    _ => self.emit(&format!(
                        "{tmp} = call ptr @rt_str_split_opts_count(ptr {recv}, ptr {arg_val}, {count}, {opts})"
                    )),
                }
            }
            SplitEmitKind::MultiCount => {
                let (_, seps) = self.emit_operand(&args[0]);
                let count = self.string_int_arg(&args[1]);
                let opts = self.string_int_arg(&args[2]);
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_split_chars_opts_count(ptr {recv}, ptr {seps}, {count}, {opts})"
                ));
            }
        }
        ("ptr".into(), tmp)
    }

    /// Trim / TrimStart / TrimEnd：0 参空白；1 参 char→*_char；1 参 char[] 或 N 参 chars→*_chars。
    fn emit_string_trim(
        &mut self,
        recv: &str,
        args: &[MirOperand],
        whitespace_fn: &str,
        single_char_fn: &str,
        multi_chars_fn: &str,
    ) -> TyVal {
        let tmp = self.fresh_temp();
        if args.is_empty() {
            self.emit(&format!("{tmp} = call ptr @{whitespace_fn}(ptr {recv})"));
        } else if args.len() == 1 {
            let (arg_ty, arg_val) = self.emit_operand(&args[0]);
            if arg_ty == "i32" {
                self.emit(&format!(
                    "{tmp} = call ptr @{single_char_fn}(ptr {recv}, i32 {arg_val})"
                ));
            } else {
                self.emit(&format!(
                    "{tmp} = call ptr @{multi_chars_fn}(ptr {recv}, ptr {arg_val})"
                ));
            }
        } else {
            let n = args.len();
            let arr = self.fresh_temp();
            self.emit(&format!(
                "{arr} = call ptr @rt_array_create(i32 {n}, i32 4)"
            ));
            for (i, arg) in args.iter().enumerate() {
                let c = self.string_int_arg(arg);
                let slot = self.fresh_temp();
                self.emit(&format!(
                    "{slot} = getelementptr inbounds i32, ptr {arr}, i32 {i}"
                ));
                self.emit(&format!("store {c}, ptr {slot}"));
            }
            self.emit(&format!(
                "{tmp} = call ptr @{multi_chars_fn}(ptr {recv}, ptr {arr})"
            ));
        }
        ("ptr".into(), tmp)
    }

    /// Convert an i32 (0/1) runtime result into an i1 LLVM value.
    fn bool_from_i32(&mut self, raw: &str) -> TyVal {
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
        ("i1".into(), tmp)
    }

    /// Box a scalar value into `ptr` for the Dictionary runtime ABI (inline path).
    ///
    /// - int-family: direct `inttoptr`
    /// - `float`:  `bitcast float → i32`, then `inttoptr i32 → ptr`
    /// - `double`: `bitcast double → i64`, then `inttoptr i64 → ptr`
    /// - `bool`:   `zext i1 → i32`, then `inttoptr i32 → ptr`
    fn box_scalar_to_ptr(&mut self, suffix: &str, op_ty: &str, op_val: &str) -> String {
        match suffix {
            "float" => {
                let bc = self.fresh_temp();
                self.emit(&format!("{bc} = bitcast {op_ty} {op_val} to i32"));
                let kp = self.fresh_temp();
                self.emit(&format!("{kp} = inttoptr i32 {bc} to ptr"));
                kp
            }
            "double" => {
                let bc = self.fresh_temp();
                self.emit(&format!("{bc} = bitcast {op_ty} {op_val} to i64"));
                let kp = self.fresh_temp();
                self.emit(&format!("{kp} = inttoptr i64 {bc} to ptr"));
                kp
            }
            "bool" => {
                let z = self.fresh_temp();
                self.emit(&format!("{z} = zext {op_ty} {op_val} to i32"));
                let kp = self.fresh_temp();
                self.emit(&format!("{kp} = inttoptr i32 {z} to ptr"));
                kp
            }
            _ => {
                let kp = self.fresh_temp();
                self.emit(&format!("{kp} = inttoptr {op_ty} {op_val} to ptr"));
                kp
            }
        }
    }

    /// Unbox a `ptr` back to a scalar type (inverse of `box_scalar_to_ptr`).
    fn unbox_ptr_to_scalar(&mut self, suffix: &str, ptr_val: &str) -> String {
        let ty = dict_kv_llvm_ty(suffix, self.layouts);
        match suffix {
            "float" => {
                let pi = self.fresh_temp();
                self.emit(&format!("{pi} = ptrtoint ptr {ptr_val} to i32"));
                let r = self.fresh_temp();
                self.emit(&format!("{r} = bitcast i32 {pi} to {ty}"));
                r
            }
            "double" => {
                let pi = self.fresh_temp();
                self.emit(&format!("{pi} = ptrtoint ptr {ptr_val} to i64"));
                let r = self.fresh_temp();
                self.emit(&format!("{r} = bitcast i64 {pi} to {ty}"));
                r
            }
            "bool" => {
                let pi = self.fresh_temp();
                self.emit(&format!("{pi} = ptrtoint ptr {ptr_val} to i32"));
                let r = self.fresh_temp();
                self.emit(&format!("{r} = trunc i32 {pi} to {ty}"));
                r
            }
            _ => {
                let r = self.fresh_temp();
                self.emit(&format!("{r} = ptrtoint ptr {ptr_val} to {ty}"));
                r
            }
        }
    }

    /// Instance GetHashCode() → i32 for primitives.
    /// Mirrors the static form in builtin_primitive.rs.
    fn emit_prim_instance_get_hash_code(
        &mut self,
        llvm_ty: &str,
        type_name: &str,
        val: &str,
    ) -> TyVal {
        match llvm_ty {
            "i32" => ("i32".into(), val.to_string()),
            "i16" | "i8" => {
                let ext = if llvm_ty == "i8" && type_name != "sbyte" {
                    "zext"
                } else {
                    "sext"
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = {ext} {llvm_ty} {val} to i32"));
                ("i32".into(), tmp)
            }
            "i1" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = zext i1 {val} to i32"));
                ("i32".into(), tmp)
            }
            "i64" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc i64 {val} to i32"));
                ("i32".into(), tmp)
            }
            "float" => {
                let bc = self.fresh_temp();
                self.emit(&format!("{bc} = bitcast float {val} to i32"));
                ("i32".into(), bc)
            }
            "double" => {
                let bc = self.fresh_temp();
                self.emit(&format!("{bc} = bitcast double {val} to i64"));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc i64 {bc} to i32"));
                ("i32".into(), tmp)
            }
            _ => ("i32".into(), "0".into()),
        }
    }

    /// Instance Equals(other) → i1 for primitives.
    fn emit_prim_instance_equals(&mut self, llvm_ty: &str, a: &str, b: &str) -> TyVal {
        let op = if llvm_ty == "float" || llvm_ty == "double" {
            "fcmp oeq"
        } else {
            "icmp eq"
        };
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = {op} {llvm_ty} {a}, {b}"));
        ("i1".into(), tmp)
    }

    /// Instance CompareTo(other) → i32 for primitives.
    fn emit_prim_instance_compare(
        &mut self,
        llvm_ty: &str,
        type_name: &str,
        a: &str,
        b: &str,
    ) -> TyVal {
        let is_unsigned = matches!(type_name, "byte" | "uint" | "ulong" | "ushort" | "char");
        let (lt_op, gt_op) = if llvm_ty == "float" || llvm_ty == "double" {
            ("fcmp olt", "fcmp ogt")
        } else if is_unsigned {
            ("icmp ult", "icmp ugt")
        } else {
            ("icmp slt", "icmp sgt")
        };
        let lt = self.fresh_temp();
        self.emit(&format!("{lt} = {lt_op} {llvm_ty} {a}, {b}"));
        let gt = self.fresh_temp();
        self.emit(&format!("{gt} = {gt_op} {llvm_ty} {a}, {b}"));
        let ext_lt = self.fresh_temp();
        self.emit(&format!("{ext_lt} = sext i1 {lt} to i32"));
        let ext_gt = self.fresh_temp();
        self.emit(&format!("{ext_gt} = sext i1 {gt} to i32"));
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = sub i32 {ext_lt}, {ext_gt}"));
        ("i32".into(), tmp)
    }
}
