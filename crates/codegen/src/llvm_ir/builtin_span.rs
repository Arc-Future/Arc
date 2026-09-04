//! RFC 005：安全 `Span<T>` / `ReadOnlySpan<T>` codegen（胖指针，无用户裸指针）。
//!
//! 表示：栈上 `{ ptr data, i32 length }`，局部槽位存指向该结构的 `ptr`。

use super::*;
use ast::TypeId;
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    /// RFC 005 params@Span：`alloca [N x T]` + pack，**禁止** `rt_array_create`。
    pub(super) fn emit_span_from_stack(
        &mut self,
        elements: &[MirOperand],
        elem_type: &TypeId,
        _mutable: bool,
    ) -> TyVal {
        let n = elements.len();
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };
        let (data_val, len_val) = if n == 0 {
            ("null".into(), "0".into())
        } else {
            let buf = self.fresh_temp();
            self.emit(&format!("{buf} = alloca [{n} x {elem_ty}]"));
            for (i, el) in elements.iter().enumerate() {
                let (ety, eval) = self.emit_operand(el);
                let addr = self.fresh_temp();
                self.emit(&format!(
                    "{addr} = getelementptr inbounds [{n} x {elem_ty}], ptr {buf}, i32 0, i32 {i}"
                ));
                let (store_ty, store_val) = if ety == elem_ty {
                    (ety, eval)
                } else {
                    self.coerce_value(&ety, eval, &elem_ty)
                };
                self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
            }
            let data = self.fresh_temp();
            self.emit(&format!(
                "{data} = getelementptr inbounds [{n} x {elem_ty}], ptr {buf}, i32 0, i32 0"
            ));
            (data, n.to_string())
        };
        self.emit_pack_span(&data_val, &len_val)
    }

    /// `arr.AsSpan()` / `arr.AsSpan(start, len)` / `AsReadOnlySpan`。
    pub(super) fn emit_span_from_array(
        &mut self,
        array: &MirOperand,
        start: &Option<MirOperand>,
        length: &Option<MirOperand>,
        _mutable: bool,
    ) -> TyVal {
        let (_, arr) = self.emit_operand(array);
        let elem_ty = self.span_elem_llvm_of_array_operand(array);

        let (data_val, len_val) = match (start, length) {
            (Some(s), Some(l)) => {
                let (_, start_v) = self.emit_operand(s);
                let (_, len_v) = self.emit_operand(l);
                let arr_len = self.fresh_temp();
                self.emit(&format!("{arr_len} = call i32 @rt_array_length(ptr {arr})"));
                let start_neg = self.fresh_temp();
                self.emit(&format!("{start_neg} = icmp slt i32 {start_v}, 0"));
                let len_neg = self.fresh_temp();
                self.emit(&format!("{len_neg} = icmp slt i32 {len_v}, 0"));
                let end = self.fresh_temp();
                self.emit(&format!("{end} = add i32 {start_v}, {len_v}"));
                let end_bad = self.fresh_temp();
                self.emit(&format!("{end_bad} = icmp ugt i32 {end}, {arr_len}"));
                let bad1 = self.fresh_temp();
                self.emit(&format!("{bad1} = or i1 {start_neg}, {len_neg}"));
                let bad = self.fresh_temp();
                self.emit(&format!("{bad} = or i1 {bad1}, {end_bad}"));
                self.emit_span_bounds_panic(&bad);
                let data = self.fresh_temp();
                self.emit(&format!(
                    "{data} = getelementptr inbounds {elem_ty}, ptr {arr}, i32 {start_v}"
                ));
                (data, len_v)
            }
            _ => {
                let len = self.fresh_temp();
                self.emit(&format!("{len} = call i32 @rt_array_length(ptr {arr})"));
                (arr, len)
            }
        };

        self.emit_pack_span(&data_val, &len_val)
    }

    /// RFC 005 M2：已物化的 `char*` 上构造 `ReadOnlySpan<byte>`（供 `emit_string_method`）。
    pub(super) fn emit_span_from_string_ptr(
        &mut self,
        s: &str,
        start: Option<&MirOperand>,
        length: Option<&MirOperand>,
    ) -> TyVal {
        let str_len = self.fresh_temp();
        self.emit(&format!("{str_len} = call i32 @rt_str_length(ptr {s})"));

        let (data_val, len_val) = match (start, length) {
            (Some(st), Some(l)) => {
                let (_, start_v) = self.emit_operand(st);
                let (_, len_v) = self.emit_operand(l);
                let start_neg = self.fresh_temp();
                self.emit(&format!("{start_neg} = icmp slt i32 {start_v}, 0"));
                let len_neg = self.fresh_temp();
                self.emit(&format!("{len_neg} = icmp slt i32 {len_v}, 0"));
                let end = self.fresh_temp();
                self.emit(&format!("{end} = add i32 {start_v}, {len_v}"));
                let end_bad = self.fresh_temp();
                self.emit(&format!("{end_bad} = icmp ugt i32 {end}, {str_len}"));
                let bad1 = self.fresh_temp();
                self.emit(&format!("{bad1} = or i1 {start_neg}, {len_neg}"));
                let bad = self.fresh_temp();
                self.emit(&format!("{bad} = or i1 {bad1}, {end_bad}"));
                self.emit_span_bounds_panic(&bad);
                let data = self.fresh_temp();
                self.emit(&format!(
                    "{data} = getelementptr inbounds i8, ptr {s}, i32 {start_v}"
                ));
                (data, len_v)
            }
            _ => (s.to_string(), str_len),
        };

        self.emit_pack_span(&data_val, &len_val)
    }

    pub(super) fn emit_span_slice(
        &mut self,
        span: &MirOperand,
        start: &MirOperand,
        length: Option<&MirOperand>,
        _mutable: bool,
    ) -> TyVal {
        let (_, span_ptr) = self.emit_operand(span);
        let (_, start_v) = self.emit_operand(start);
        let (data0, len0) = self.emit_unpack_span(&span_ptr);
        let len_v = match length {
            Some(length) => {
                let (_, len_v) = self.emit_operand(length);
                let start_neg = self.fresh_temp();
                self.emit(&format!("{start_neg} = icmp slt i32 {start_v}, 0"));
                let len_neg = self.fresh_temp();
                self.emit(&format!("{len_neg} = icmp slt i32 {len_v}, 0"));
                let end = self.fresh_temp();
                self.emit(&format!("{end} = add i32 {start_v}, {len_v}"));
                let end_bad = self.fresh_temp();
                self.emit(&format!("{end_bad} = icmp ugt i32 {end}, {len0}"));
                let bad1 = self.fresh_temp();
                self.emit(&format!("{bad1} = or i1 {start_neg}, {len_neg}"));
                let bad = self.fresh_temp();
                self.emit(&format!("{bad} = or i1 {bad1}, {end_bad}"));
                self.emit_span_bounds_panic(&bad);
                len_v
            }
            None => {
                let start_neg = self.fresh_temp();
                self.emit(&format!("{start_neg} = icmp slt i32 {start_v}, 0"));
                let start_oob = self.fresh_temp();
                self.emit(&format!("{start_oob} = icmp ugt i32 {start_v}, {len0}"));
                let bad = self.fresh_temp();
                self.emit(&format!("{bad} = or i1 {start_neg}, {start_oob}"));
                self.emit_span_bounds_panic(&bad);
                let rest = self.fresh_temp();
                self.emit(&format!("{rest} = sub i32 {len0}, {start_v}"));
                rest
            }
        };
        let elem_ty = self.span_elem_llvm_of_span_operand(span);
        let data = self.fresh_temp();
        self.emit(&format!(
            "{data} = getelementptr inbounds {elem_ty}, ptr {data0}, i32 {start_v}"
        ));
        self.emit_pack_span(&data, &len_v)
    }

    pub(super) fn emit_span_fill(
        &mut self,
        span: &MirOperand,
        value: &MirOperand,
        elem_type: &TypeId,
    ) {
        let (_, span_ptr) = self.emit_operand(span);
        let (data, len) = self.emit_unpack_span(&span_ptr);
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };
        let (vty, vval) = self.emit_operand(value);
        let (store_ty, store_val) = if vty == elem_ty {
            (vty, vval)
        } else {
            self.coerce_value(&vty, vval, &elem_ty)
        };

        let i = self.fresh_temp();
        self.emit(&format!("{i} = alloca i32"));
        self.emit(&format!("store i32 0, ptr {i}"));
        let loop_hd = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_end = self.fresh_label();
        self.emit(&format!("br label %{loop_hd}"));
        self.emit(&format!("{loop_hd}:"));
        let i_val = self.fresh_temp();
        self.emit(&format!("{i_val} = load i32, ptr {i}"));
        let cont = self.fresh_temp();
        self.emit(&format!("{cont} = icmp ult i32 {i_val}, {len}"));
        self.emit(&format!(
            "br i1 {cont}, label %{loop_body}, label %{loop_end}"
        ));
        self.emit(&format!("{loop_body}:"));
        let addr = self.fresh_temp();
        self.emit(&format!(
            "{addr} = getelementptr inbounds {elem_ty}, ptr {data}, i32 {i_val}"
        ));
        self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
        let i_next = self.fresh_temp();
        self.emit(&format!("{i_next} = add i32 {i_val}, 1"));
        self.emit(&format!("store i32 {i_next}, ptr {i}"));
        self.emit(&format!("br label %{loop_hd}"));
        self.emit(&format!("{loop_end}:"));
    }

    pub(super) fn emit_span_clear(&mut self, span: &MirOperand, elem_type: &TypeId) {
        let zero = MirOperand::ConstInt(0);
        self.emit_span_fill(span, &zero, elem_type);
    }

    pub(super) fn emit_span_length(&mut self, span: &MirOperand) -> TyVal {
        let (_, span_ptr) = self.emit_operand(span);
        let (_, len) = self.emit_unpack_span(&span_ptr);
        ("i32".into(), len)
    }

    pub(super) fn emit_span_is_empty(&mut self, span: &MirOperand) -> TyVal {
        let (_, span_ptr) = self.emit_operand(span);
        let (_, len) = self.emit_unpack_span(&span_ptr);
        let cmp = self.fresh_temp();
        self.emit(&format!("{cmp} = icmp eq i32 {len}, 0"));
        ("i1".into(), cmp)
    }

    /// `src.CopyTo(dest)`：长度校验后按元素 memcpy 式循环（标量 load/store）。
    pub(super) fn emit_span_copy_to(
        &mut self,
        src: &MirOperand,
        dest: &MirOperand,
        elem_type: &TypeId,
    ) {
        let (_, src_ptr) = self.emit_operand(src);
        let (_, dest_ptr) = self.emit_operand(dest);
        let (src_data, src_len) = self.emit_unpack_span(&src_ptr);
        let (dest_data, dest_len) = self.emit_unpack_span(&dest_ptr);
        let too_short = self.fresh_temp();
        self.emit(&format!("{too_short} = icmp ult i32 {dest_len}, {src_len}"));
        self.emit_span_bounds_panic(&too_short);
        self.emit_span_elem_copy_loop(&src_data, &dest_data, &src_len, elem_type);
    }

    /// `src.TryCopyTo(dest)` → i1：目标过短返回 false；否则拷贝并返回 true。
    pub(super) fn emit_span_try_copy_to(
        &mut self,
        src: &MirOperand,
        dest: &MirOperand,
        elem_type: &TypeId,
    ) -> TyVal {
        let (_, src_ptr) = self.emit_operand(src);
        let (_, dest_ptr) = self.emit_operand(dest);
        let (src_data, src_len) = self.emit_unpack_span(&src_ptr);
        let (dest_data, dest_len) = self.emit_unpack_span(&dest_ptr);
        let too_short = self.fresh_temp();
        self.emit(&format!("{too_short} = icmp ult i32 {dest_len}, {src_len}"));
        let ok_bb = self.fresh_label();
        let fail_bb = self.fresh_label();
        let join_bb = self.fresh_label();
        self.emit(&format!(
            "br i1 {too_short}, label %{fail_bb}, label %{ok_bb}"
        ));
        self.emit(&format!("{ok_bb}:"));
        self.emit_span_elem_copy_loop(&src_data, &dest_data, &src_len, elem_type);
        self.emit(&format!("br label %{join_bb}"));
        self.emit(&format!("{fail_bb}:"));
        self.emit(&format!("br label %{join_bb}"));
        self.emit(&format!("{join_bb}:"));
        let result = self.fresh_temp();
        // too_short ? false : true  ≡  xor too_short, true  ≡  icmp eq too_short, false
        self.emit(&format!("{result} = xor i1 {too_short}, true"));
        ("i1".into(), result)
    }

    /// `span.ToArray()` → 新堆数组（`rt_array_create` + 元素拷贝）。
    pub(super) fn emit_span_to_array(&mut self, span: &MirOperand, elem_type: &TypeId) -> TyVal {
        let (_, span_ptr) = self.emit_operand(span);
        let (src_data, src_len) = self.emit_unpack_span(&span_ptr);
        let elem_size = llvm_size_of(elem_type) as i32;
        let arr = self.fresh_temp();
        self.emit(&format!(
            "{arr} = call ptr @rt_array_create(i32 {src_len}, i32 {elem_size})"
        ));
        self.emit_span_elem_copy_loop(&src_data, &arr, &src_len, elem_type);
        ("ptr".into(), arr)
    }

    fn emit_span_elem_copy_loop(
        &mut self,
        src_data: &str,
        dest_data: &str,
        src_len: &str,
        elem_type: &TypeId,
    ) {
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };

        let i = self.fresh_temp();
        self.emit(&format!("{i} = alloca i32"));
        self.emit(&format!("store i32 0, ptr {i}"));
        let loop_hd = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_end = self.fresh_label();
        self.emit(&format!("br label %{loop_hd}"));
        self.emit(&format!("{loop_hd}:"));
        let i_val = self.fresh_temp();
        self.emit(&format!("{i_val} = load i32, ptr {i}"));
        let cont = self.fresh_temp();
        self.emit(&format!("{cont} = icmp ult i32 {i_val}, {src_len}"));
        self.emit(&format!(
            "br i1 {cont}, label %{loop_body}, label %{loop_end}"
        ));
        self.emit(&format!("{loop_body}:"));
        let src_addr = self.fresh_temp();
        self.emit(&format!(
            "{src_addr} = getelementptr inbounds {elem_ty}, ptr {src_data}, i32 {i_val}"
        ));
        let val = self.fresh_temp();
        self.emit(&format!("{val} = load {elem_ty}, ptr {src_addr}"));
        let dest_addr = self.fresh_temp();
        self.emit(&format!(
            "{dest_addr} = getelementptr inbounds {elem_ty}, ptr {dest_data}, i32 {i_val}"
        ));
        self.emit(&format!("store {elem_ty} {val}, ptr {dest_addr}"));
        let i_next = self.fresh_temp();
        self.emit(&format!("{i_next} = add i32 {i_val}, 1"));
        self.emit(&format!("store i32 {i_next}, ptr {i}"));
        self.emit(&format!("br label %{loop_hd}"));
        self.emit(&format!("{loop_end}:"));
    }

    pub(super) fn emit_span_index_get(
        &mut self,
        span: &MirOperand,
        index: &MirOperand,
        elem_type: &TypeId,
    ) -> TyVal {
        let (_, span_ptr) = self.emit_operand(span);
        let (_, idx) = self.emit_operand(index);
        let (data, len) = self.emit_unpack_span(&span_ptr);
        self.emit_span_index_bounds(&idx, &len);
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };
        let addr = self.fresh_temp();
        self.emit(&format!(
            "{addr} = getelementptr inbounds {elem_ty}, ptr {data}, i32 {idx}"
        ));
        let result = self.fresh_temp();
        self.emit(&format!("{result} = load {elem_ty}, ptr {addr}"));
        (elem_ty, result)
    }

    pub(super) fn emit_span_index_set(
        &mut self,
        span: &MirOperand,
        index: &MirOperand,
        elem_type: &TypeId,
        value: &mir::MirRvalue,
    ) {
        let (_, span_ptr) = self.emit_operand(span);
        let (_, idx) = self.emit_operand(index);
        let (data, len) = self.emit_unpack_span(&span_ptr);
        self.emit_span_index_bounds(&idx, &len);
        let (vty, vval) = self.emit_rvalue(value);
        let elem_ty = match elem_type {
            TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
            other => llvm_type_of(other, self.layouts),
        };
        let addr = self.fresh_temp();
        self.emit(&format!(
            "{addr} = getelementptr inbounds {elem_ty}, ptr {data}, i32 {idx}"
        ));
        let (store_ty, store_val) = if vty == elem_ty {
            (vty, vval)
        } else {
            self.coerce_value(&vty, vval, &elem_ty)
        };
        self.emit(&format!("store {store_ty} {store_val}, ptr {addr}"));
    }

    pub(super) fn emit_pack_span(&mut self, data: &str, len: &str) -> TyVal {
        let fat = self.fresh_temp();
        self.emit(&format!("{fat} = alloca {{ ptr, i32 }}"));
        let dp = self.fresh_temp();
        self.emit(&format!(
            "{dp} = getelementptr inbounds {{ ptr, i32 }}, ptr {fat}, i32 0, i32 0"
        ));
        self.emit(&format!("store ptr {data}, ptr {dp}"));
        let lp = self.fresh_temp();
        self.emit(&format!(
            "{lp} = getelementptr inbounds {{ ptr, i32 }}, ptr {fat}, i32 0, i32 1"
        ));
        self.emit(&format!("store i32 {len}, ptr {lp}"));
        ("ptr".into(), fat)
    }

    pub(super) fn emit_unpack_span(&mut self, span_ptr: &str) -> (String, String) {
        let dp = self.fresh_temp();
        self.emit(&format!(
            "{dp} = getelementptr inbounds {{ ptr, i32 }}, ptr {span_ptr}, i32 0, i32 0"
        ));
        let data = self.fresh_temp();
        self.emit(&format!("{data} = load ptr, ptr {dp}"));
        let lp = self.fresh_temp();
        self.emit(&format!(
            "{lp} = getelementptr inbounds {{ ptr, i32 }}, ptr {span_ptr}, i32 0, i32 1"
        ));
        let len = self.fresh_temp();
        self.emit(&format!("{len} = load i32, ptr {lp}"));
        (data, len)
    }

    fn emit_span_index_bounds(&mut self, idx: &str, len: &str) {
        let neg = self.fresh_temp();
        self.emit(&format!("{neg} = icmp slt i32 {idx}, 0"));
        let oob = self.fresh_temp();
        self.emit(&format!("{oob} = icmp uge i32 {idx}, {len}"));
        let bad = self.fresh_temp();
        self.emit(&format!("{bad} = or i1 {neg}, {oob}"));
        self.emit_span_bounds_panic(&bad);
    }

    /// Hot-path correctness：`IndexGet`/`IndexSet` 识别 Span 胖指针（不止 Local）。
    pub(super) fn operand_is_span(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::Local(id) => matches!(self.local_type(*id), TypeId::Span { .. }),
            MirOperand::Field { class, .. } => class == "Span" || class == "ReadOnlySpan",
            _ => false,
        }
    }

    pub(super) fn operand_is_mutable_span(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::Local(id) => {
                matches!(self.local_type(*id), TypeId::Span { mutable: true, .. })
            }
            MirOperand::Field { class, .. } => class == "Span",
            _ => false,
        }
    }

    pub(super) fn emit_span_bounds_panic(&mut self, bad: &str) {
        let ok = self.fresh_label();
        let panic_bb = self.fresh_label();
        self.emit(&format!("br i1 {bad}, label %{panic_bb}, label %{ok}"));
        self.emit(&format!("{panic_bb}:"));
        self.emit("call void @rt_panic(ptr @__arc_span_oob)");
        self.emit("unreachable");
        self.emit(&format!("{ok}:"));
    }

    fn span_elem_llvm_of_array_operand(&self, array: &MirOperand) -> String {
        if let MirOperand::Local(id) = array {
            if let TypeId::Array { elem } = self.local_type(*id) {
                return match elem.as_ref() {
                    TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
                    other => llvm_type_of(other, self.layouts),
                };
            }
        }
        "i32".into()
    }

    fn span_elem_llvm_of_span_operand(&self, span: &MirOperand) -> String {
        if let MirOperand::Local(id) = span {
            if let TypeId::Span { elem, .. } = self.local_type(*id) {
                return match elem.as_ref() {
                    TypeId::Named(name) if self.layouts.structs.contains_key(name) => "ptr".into(),
                    other => llvm_type_of(other, self.layouts),
                };
            }
        }
        "i32".into()
    }
}
