//! RFC 004 M1：基元类型 static abstract 方法/属性的内联 LLVM 指令发射。
//!
//! 识别 `func = "<type>.<method>"`（如 `int.Add`、`double.Multiply`、`int.Zero`）
//! 形式的 call，直接发射 LLVM 运算/比较指令，零运行时开销（无 ABI 调用）。
//!
//! 与 `builtin_math.rs`（Math facade）和 `builtin_vector.rs`（Vector SIMD）并列，
//! 共同构成 codegen 的「内联 builtin」拦截体系。
//!
//! ## 支持矩阵
//!
//! | 类型 | Add/Sub/Mul/Div | Negate | Zero/One | Equals | GetHashCode | Compare |
//! |------|-----------------|--------|----------|--------|-------------|---------|
//! | int/long/short/byte | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | float/double | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
//! | bool | — | — | — | ✓ | ✓ | ✓ |
//! | char | — | — | — | ✓ | ✓ | ✓ |
//! | string | — | — | — | ✓ | ✓ | ✓ |
//!
//! Compare 三值化：`sext i1 lt to i32` − `sext i1 gt to i32` → −1/0/1。

use super::*;
use mir::MirOperand;

/// 数值基元类型 → LLVM IR 类型字符串。
fn primitive_llvm_ty(type_name: &str) -> Option<&'static str> {
    Some(match type_name {
        "int" => "i32",
        "long" => "i64",
        "short" => "i16",
        "byte" => "i8",
        "float" => "float",
        "double" => "double",
        "bool" => "i1",
        "char" => "i32",
        "string" => "ptr",
        "uint" => "i32",
        "ulong" => "i64",
        "ushort" => "i16",
        "sbyte" => "i8",
        _ => return None,
    })
}

fn is_float_ty(llvm_ty: &str) -> bool {
    matches!(llvm_ty, "float" | "double")
}

impl<'a> FnEmitter<'a> {
    /// RFC 032 M1：基元类型 static abstract 调用拦截器。
    ///
    /// 由 `emit_call_typed` 调用。识别 `func = "<type>.<method>"` 形式，
    /// 直接发射 LLVM 指令（add/fadd/sub/mul/sdiv/fdiv/icmp/fcmp 等）。
    /// 返回 `None` 表示 func 不是基元类型 static abstract 调用，由调用方 fallback。
    pub(super) fn try_emit_primitive_static(
        &mut self,
        func: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (type_name, method) = func.split_once('.')?;
        let llvm_ty = primitive_llvm_ty(type_name)?;
        let result: TyVal = match method {
            "Add" => self.emit_prim_arith(args, llvm_ty, "add", "fadd"),
            "Subtract" => self.emit_prim_arith(args, llvm_ty, "sub", "fsub"),
            "Multiply" => self.emit_prim_arith(args, llvm_ty, "mul", "fmul"),
            "Divide" => {
                let div = if matches!(type_name, "byte" | "uint" | "ulong" | "ushort" | "char") {
                    "udiv"
                } else {
                    "sdiv"
                };
                self.emit_prim_arith(args, llvm_ty, div, "fdiv")
            }
            "Negate" => self.emit_prim_negate(args, llvm_ty),
            "Equals" => self.emit_prim_equals(args, llvm_ty, type_name),
            "GetHashCode" => self.emit_prim_get_hash_code(args, llvm_ty, type_name),
            "Compare" => self.emit_prim_compare(args, llvm_ty, type_name),
            "Zero" => self.emit_prim_zero(llvm_ty),
            "One" => self.emit_prim_one(llvm_ty),
            "Parse" => self.emit_prim_parse(args, type_name),
            "TryParse" => self.emit_prim_try_parse(args, type_name),
            "MinValue" | "MaxValue" | "Epsilon" | "NaN" | "PositiveInfinity"
            | "NegativeInfinity" => self.emit_prim_const(llvm_ty, type_name, method),
            "ToString" => self.emit_prim_to_string(args, type_name),
            "IsDigit" => self.emit_char_classify(args, "@rt_char_is_digit"),
            "IsLetter" => self.emit_char_classify(args, "@rt_char_is_letter"),
            "IsWhiteSpace" => self.emit_char_classify(args, "@rt_char_is_white_space"),
            "IsUpper" => self.emit_char_classify(args, "@rt_char_is_upper"),
            "IsLower" => self.emit_char_classify(args, "@rt_char_is_lower"),
            "ToUpper" => self.emit_char_convert(args, "@rt_char_to_upper"),
            "ToLower" => self.emit_char_convert(args, "@rt_char_to_lower"),
            _ => return None,
        };
        Some(result)
    }

    /// 二元算术：Add/Subtract/Multiply/Divide。
    /// 整数用 `add`/`sub`/`mul`/`sdiv`，浮点用 `fadd`/`fsub`/`fmul`/`fdiv`。
    fn emit_prim_arith(
        &mut self,
        args: &[MirOperand],
        llvm_ty: &str,
        int_op: &str,
        float_op: &str,
    ) -> TyVal {
        let l = self.prim_operand(args.first(), llvm_ty);
        let r = self.prim_operand(args.get(1), llvm_ty);
        let op = if is_float_ty(llvm_ty) {
            float_op
        } else {
            int_op
        };
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = {op} {llvm_ty} {l}, {r}"));
        (llvm_ty.into(), tmp)
    }

    /// 一元取负：整数 `sub 0, x`；浮点 `fneg x`。
    fn emit_prim_negate(&mut self, args: &[MirOperand], llvm_ty: &str) -> TyVal {
        let v = self.prim_operand(args.first(), llvm_ty);
        let tmp = self.fresh_temp();
        if is_float_ty(llvm_ty) {
            self.emit(&format!("{tmp} = fneg {llvm_ty} {v}"));
        } else {
            self.emit(&format!("{tmp} = sub {llvm_ty} 0, {v}"));
        }
        (llvm_ty.into(), tmp)
    }

    /// 相等比较 → i1。整数 `icmp eq`，浮点 `fcmp oeq`，string 走 `rt_str_equals`。
    fn emit_prim_equals(&mut self, args: &[MirOperand], llvm_ty: &str, type_name: &str) -> TyVal {
        if type_name == "string" {
            let (_, l) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
            let (_, r) = self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
            let cmp = self.fresh_temp();
            self.emit(&format!(
                "{cmp} = call i32 @rt_str_equals(ptr {l}, ptr {r})"
            ));
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = icmp ne i32 {cmp}, 0"));
            return ("i1".into(), tmp);
        }
        let l = self.prim_operand(args.first(), llvm_ty);
        let r = self.prim_operand(args.get(1), llvm_ty);
        let op = if is_float_ty(llvm_ty) {
            "fcmp oeq"
        } else {
            "icmp eq"
        };
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = {op} {llvm_ty} {l}, {r}"));
        ("i1".into(), tmp)
    }

    /// GetHashCode → i32。
    /// - int/i16/i8：sext/zext 到 i32（int 直接返回）
    /// - i64：trunc 到 i32
    /// - i1：zext 到 i32
    /// - float：bitcast 到 i32
    /// - double：bitcast 到 i64 再 trunc 到 i32
    /// - string：DJB2 内容哈希（`rt_hash_str`），区分不同内容的字符串。
    fn emit_prim_get_hash_code(
        &mut self,
        args: &[MirOperand],
        llvm_ty: &str,
        type_name: &str,
    ) -> TyVal {
        if type_name == "string" {
            let (_, v) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
            let tmp = self.fresh_temp();
            self.emit(&format!("{tmp} = call i32 @rt_hash_str(ptr {v})"));
            return ("i32".into(), tmp);
        }
        let (src_ty, v) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        match llvm_ty {
            "i32" => ("i32".into(), v),
            "i16" | "i8" => {
                let ext = if llvm_ty == "i8" && type_name != "sbyte" {
                    "zext"
                } else {
                    "sext"
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = {ext} {src_ty} {v} to i32"));
                ("i32".into(), tmp)
            }
            "i1" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = zext i1 {v} to i32"));
                ("i32".into(), tmp)
            }
            "i64" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc i64 {v} to i32"));
                ("i32".into(), tmp)
            }
            "float" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast float {v} to i32"));
                ("i32".into(), tmp)
            }
            "double" => {
                let bc = self.fresh_temp();
                self.emit(&format!("{bc} = bitcast double {v} to i64"));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc i64 {bc} to i32"));
                ("i32".into(), tmp)
            }
            _ => ("i32".into(), "0".into()),
        }
    }

    /// 三值比较 → i32（-1/0/1）。
    /// `sext i1 (a<b) to i32` − `sext i1 (a>b) to i32`：
    ///   - a<b：−1 − 0 = −1
    ///   - a>b：0 − −1 = 1
    ///   - a=b：0 − 0 = 0
    ///     string 走 `rt_str_compare`。
    fn emit_prim_compare(&mut self, args: &[MirOperand], llvm_ty: &str, type_name: &str) -> TyVal {
        if type_name == "string" {
            let (_, l) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
            let (_, r) = self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call i32 @rt_str_compare(ptr {l}, ptr {r})"
            ));
            return ("i32".into(), tmp);
        }
        let l = self.prim_operand(args.first(), llvm_ty);
        let r = self.prim_operand(args.get(1), llvm_ty);
        let is_unsigned = matches!(type_name, "byte" | "uint" | "ulong" | "ushort" | "char");
        let lt_op = if is_float_ty(llvm_ty) {
            "fcmp olt"
        } else if is_unsigned {
            "icmp ult"
        } else {
            "icmp slt"
        };
        let gt_op = if is_float_ty(llvm_ty) {
            "fcmp ogt"
        } else if is_unsigned {
            "icmp ugt"
        } else {
            "icmp sgt"
        };
        let lt = self.fresh_temp();
        self.emit(&format!("{lt} = {lt_op} {llvm_ty} {l}, {r}"));
        let gt = self.fresh_temp();
        self.emit(&format!("{gt} = {gt_op} {llvm_ty} {l}, {r}"));
        let ext_lt = self.fresh_temp();
        self.emit(&format!("{ext_lt} = sext i1 {lt} to i32"));
        let ext_gt = self.fresh_temp();
        self.emit(&format!("{ext_gt} = sext i1 {gt} to i32"));
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = sub i32 {ext_lt}, {ext_gt}"));
        ("i32".into(), tmp)
    }

    /// Zero 常量：整数 `0`，浮点 `0.0`。
    fn emit_prim_zero(&mut self, llvm_ty: &str) -> TyVal {
        let v = if is_float_ty(llvm_ty) { "0.0" } else { "0" };
        (llvm_ty.into(), v.into())
    }

    /// One 常量：整数 `1`，浮点 `1.0`。
    fn emit_prim_one(&mut self, llvm_ty: &str) -> TyVal {
        let v = if is_float_ty(llvm_ty) { "1.0" } else { "1" };
        (llvm_ty.into(), v.into())
    }

    /// Parse(string) → 调 rt_parse_* ABI。
    fn emit_prim_parse(&mut self, args: &[MirOperand], type_name: &str) -> TyVal {
        let (_, s) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
        let abi = match type_name {
            "int" => ("i32", "@rt_parse_int32"),
            "long" => ("i64", "@rt_parse_int64"),
            "float" => ("float", "@rt_parse_float"),
            "double" => ("double", "@rt_parse_double"),
            "bool" => ("i32", "@rt_parse_bool"),
            "char" => ("i32", "@rt_parse_char"),
            "short" => ("i32", "@rt_parse_int32"),
            "byte" => ("i32", "@rt_parse_int32"),
            "uint" => ("i32", "@rt_parse_uint32"),
            "ulong" => ("i64", "@rt_parse_uint64"),
            "ushort" => ("i32", "@rt_parse_uint32"),
            "sbyte" => ("i32", "@rt_parse_int32"),
            _ => panic!("unsupported Parse for {type_name}"),
        };
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = call {} {}(ptr {})", abi.0, abi.1, s));
        // Truncate for narrow types
        match type_name {
            "short" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = trunc i32 {tmp} to i16"));
                return ("i16".into(), t);
            }
            "byte" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = trunc i32 {tmp} to i8"));
                return ("i8".into(), t);
            }
            "ushort" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = trunc i32 {tmp} to i16"));
                return ("i16".into(), t);
            }
            "sbyte" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = trunc i32 {tmp} to i8"));
                return ("i8".into(), t);
            }
            _ => {}
        }
        (abi.0.into(), tmp)
    }

    /// TryParse(string, ptr out) → i32 (1=success)。
    fn emit_prim_try_parse(&mut self, args: &[MirOperand], type_name: &str) -> TyVal {
        let (_, s) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
        let (_, out_ptr) =
            self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
        // Narrow types need an intermediate i32 slot, then truncate on success.
        let needs_trunc = matches!(type_name, "short" | "byte" | "ushort" | "sbyte");
        let (abi, llvm_out_ty) = if needs_trunc {
            ("@rt_parse_int32_try", "i32")
        } else {
            match type_name {
                "int" => ("@rt_parse_int32_try", "i32"),
                "long" => ("@rt_parse_int64_try", "i64"),
                "float" => ("@rt_parse_float_try", "float"),
                "double" => ("@rt_parse_double_try", "double"),
                "bool" => ("@rt_parse_bool_try", "i32"),
                "char" => ("@rt_parse_char_try", "i32"),
                "uint" => ("@rt_parse_uint32_try", "i32"),
                "ulong" => ("@rt_parse_uint64_try", "i64"),
                _ => panic!("unsupported TryParse for {type_name}"),
            }
        };
        let out_slot = if needs_trunc {
            let slot = self.fresh_temp();
            self.emit(&format!("{slot} = alloca {llvm_out_ty}, align 4"));
            slot
        } else {
            out_ptr.clone()
        };
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = call i32 {abi}(ptr {s}, ptr {out_slot})"));
        // Truncate narrow types on success
        if needs_trunc {
            let trunc_ty = match type_name {
                "short" | "ushort" => "i16",
                "byte" | "sbyte" => "i8",
                _ => unreachable!(),
            };
            let val32 = self.fresh_temp();
            self.emit(&format!("{val32} = load {llvm_out_ty}, ptr {out_slot}"));
            let val_trunc = self.fresh_temp();
            self.emit(&format!(
                "{val_trunc} = trunc {llvm_out_ty} {val32} to {trunc_ty}"
            ));
            self.emit(&format!("store {trunc_ty} {val_trunc}, ptr {out_ptr}"));
        }
        // 返回 bool：icmp ne 0
        let bool_tmp = self.fresh_temp();
        self.emit(&format!("{bool_tmp} = icmp ne i32 {tmp}, 0"));
        ("i1".into(), bool_tmp)
    }

    /// 内置常量：MinValue / MaxValue / Epsilon / NaN / Infinity。
    fn emit_prim_const(&mut self, llvm_ty: &str, type_name: &str, field: &str) -> TyVal {
        let v: &str = match (type_name, field) {
            ("int", "MinValue") => "-2147483648",
            ("int", "MaxValue") => "2147483647",
            ("long", "MinValue") => "-9223372036854775808",
            ("long", "MaxValue") => "9223372036854775807",
            ("short", "MinValue") => "-32768",
            ("short", "MaxValue") => "32767",
            ("byte", "MinValue") => "0",
            ("byte", "MaxValue") => "255",
            ("uint", "MinValue") => "0",
            ("uint", "MaxValue") => "4294967295",
            ("ulong", "MinValue") => "0",
            ("ulong", "MaxValue") => "18446744073709551615",
            ("ushort", "MinValue") => "0",
            ("ushort", "MaxValue") => "65535",
            ("sbyte", "MinValue") => "-128",
            ("sbyte", "MaxValue") => "127",
            ("float", "MinValue") => "-340282346000000000000000000000000000000.0",
            ("float", "MaxValue") => "340282346000000000000000000000000000000.0",
            ("float", "Epsilon") => "0x1.0p-149",
            ("float", "NaN") => "0x7FF8000000000000",
            ("float", "PositiveInfinity") => "0x7F800000",
            ("float", "NegativeInfinity") => "0xFF800000",
            ("double", "MinValue") => "-179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0",
            ("double", "MaxValue") => "179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000.0",
            ("double", "Epsilon") => "0x1.0p-1074",
            ("double", "NaN") => "0x7FF8000000000000",
            ("double", "PositiveInfinity") => "0x7FF0000000000000",
            ("double", "NegativeInfinity") => "0xFFF0000000000000",
            _ => panic!("unsupported const {field} for {type_name}"),
        };
        (llvm_ty.into(), v.into())
    }

    /// ToString(value) → rt_*_to_string；ToString(value, format) → rt_*_to_string_fmt（RFC 007 M2a）；
    /// ToString(value, format, provider) → rt_*_to_string_fmt_p（RFC 033 M5 文化感知）。
    fn emit_prim_to_string(&mut self, args: &[MirOperand], type_name: &str) -> TyVal {
        let (abi_ty, abi_fn, abi_fmt, abi_fmt_p) = match type_name {
            "int" => (
                "i32",
                "@rt_int_to_string",
                "@rt_int_to_string_fmt",
                "@rt_int_to_string_fmt_p",
            ),
            "long" => (
                "i64",
                "@rt_long_to_string",
                "@rt_long_to_string_fmt",
                "@rt_long_to_string_fmt_p",
            ),
            "short" => (
                "i16",
                "@rt_short_to_string",
                "@rt_short_to_string_fmt",
                "@rt_short_to_string_fmt_p",
            ),
            "byte" => (
                "i8",
                "@rt_byte_to_string",
                "@rt_byte_to_string_fmt",
                "@rt_byte_to_string_fmt_p",
            ),
            "float" => (
                "float",
                "@rt_float_to_string",
                "@rt_float_to_string_fmt",
                "@rt_float_to_string_fmt_p",
            ),
            "double" => (
                "double",
                "@rt_double_to_string",
                "@rt_double_to_string_fmt",
                "@rt_double_to_string_fmt_p",
            ),
            "bool" => ("i32", "@rt_bool_to_string", "", ""),
            "char" => ("i32", "@rt_char_to_string", "", ""),
            "uint" => (
                "i32",
                "@rt_uint_to_string",
                "@rt_uint_to_string_fmt",
                "@rt_uint_to_string_fmt_p",
            ),
            "ulong" => (
                "i64",
                "@rt_ulong_to_string",
                "@rt_ulong_to_string_fmt",
                "@rt_ulong_to_string_fmt_p",
            ),
            "ushort" => (
                "i16",
                "@rt_ushort_to_string",
                "@rt_ushort_to_string_fmt",
                "@rt_ushort_to_string_fmt_p",
            ),
            "sbyte" => (
                "i8",
                "@rt_sbyte_to_string",
                "@rt_sbyte_to_string_fmt",
                "@rt_sbyte_to_string_fmt_p",
            ),
            _ => panic!("unsupported ToString for {type_name}"),
        };
        let (recv_ty, v) =
            self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let v = if recv_ty != abi_ty {
            let c = self.fresh_temp();
            let conv = if recv_ty.starts_with('i') && abi_ty.starts_with('i') {
                "zext"
            } else if recv_ty == "float" && abi_ty == "double" {
                "fpext"
            } else {
                "bitcast"
            };
            self.emit(&format!("{c} = {conv} {recv_ty} {v} to {abi_ty}"));
            c
        } else {
            v
        };
        let tmp = self.fresh_temp();
        match args.len() {
            0 | 1 => self.emit(&format!("{tmp} = call ptr {abi_fn}({abi_ty} {v})")),
            2 => {
                if abi_fmt.is_empty() {
                    panic!("ToString(format) unsupported for {type_name}");
                }
                let (_, fmt) = self.emit_operand(&args[1]);
                self.emit(&format!(
                    "{tmp} = call ptr {abi_fmt}({abi_ty} {v}, ptr {fmt})"
                ));
            }
            3 => {
                if abi_fmt_p.is_empty() {
                    panic!("ToString(format, provider) unsupported for {type_name}");
                }
                let (_, fmt) = self.emit_operand(&args[1]);
                let (_, provider) = self.emit_operand(&args[2]);
                self.emit(&format!(
                    "{tmp} = call ptr {abi_fmt_p}({abi_ty} {v}, ptr {fmt}, ptr {provider})"
                ));
            }
            _ => panic!("ToString with {} args unsupported", args.len()),
        }
        ("ptr".into(), tmp)
    }

    /// Char 分类方法：IsDigit/IsLetter/IsWhiteSpace/IsUpper/IsLower。
    /// 调用 rt_char_is_* ABI，返回 i1（icmp ne 0）。
    fn emit_char_classify(&mut self, args: &[MirOperand], abi: &str) -> TyVal {
        let (_, v) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let int_tmp = self.fresh_temp();
        self.emit(&format!("{int_tmp} = call i32 {abi}(i32 {v})"));
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = icmp ne i32 {int_tmp}, 0"));
        ("i1".into(), tmp)
    }

    /// Char 转换方法：ToUpper/ToLower。
    /// 调用 rt_char_to_* ABI，返回 i32。
    fn emit_char_convert(&mut self, args: &[MirOperand], abi: &str) -> TyVal {
        let (_, v) = self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = call i32 {abi}(i32 {v})"));
        ("i32".into(), tmp)
    }

    /// 取一个 operand 并强制转换为 `llvm_ty`。
    /// 处理 ConstInt(0/1) 默认为 i32 与 bool（i1）/short（i16）/byte（i8）/long（i64）
    /// 类型不匹配问题。同类型直接返回，异类型走 `coerce_value`。
    fn prim_operand(&mut self, arg: Option<&MirOperand>, llvm_ty: &str) -> String {
        let default = if is_float_ty(llvm_ty) {
            MirOperand::ConstFloat(0.0)
        } else {
            MirOperand::ConstInt(0)
        };
        let (src_ty, val) = self.emit_operand(&arg.cloned().unwrap_or(default));
        if src_ty == llvm_ty {
            return val;
        }
        let (_, coerced) = self.coerce_value(&src_ty, val, llvm_ty);
        coerced
    }
}
