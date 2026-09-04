//! Binary operation emission: arithmetic, comparison, and string operations.

use super::*;
use ast::{BinOp, TypeId};
use mir::MirOperand;

impl<'a> FnEmitter<'a> {
    // ---- Binary operations ----

    /// Check whether an operand represents an unsigned integer, using
    /// TypeId information for Locals and falling back to LLVM type for others.
    /// Correctly distinguishes byte (unsigned i8) from sbyte (signed i8),
    /// ushort (unsigned i16) from short (signed i16), etc.
    fn op_is_unsigned_int(&self, op: &MirOperand, llvm_ty: &str) -> bool {
        match op {
            MirOperand::Local(id) => {
                let ty = self.local_type(*id);
                matches!(
                    ty,
                    TypeId::Byte | TypeId::UInt | TypeId::ULong | TypeId::UShort | TypeId::Char
                )
            }
            _ => is_unsigned_int_ty(llvm_ty),
        }
    }

    pub(super) fn emit_binary(
        &mut self,
        op: BinOp,
        left: &MirOperand,
        right: &MirOperand,
    ) -> TyVal {
        // RFC 009 L2：可空值类型比较（仅 Eq/NotEq；其他比较语义模糊，C# 需显式 unwrap）。
        // - `T? == null` / `null == T?`：null 语义 = `!HasValue`（==）/ `HasValue`（!=），
        //   与 Value 无关。须在 `T? == T` 之前拦截——`ConstNull` 会被 `emit_nullable_value_eq`
        //   强转为 inner 零值参与 `Value == 0` 比较，`int? x = 0; x == null` 将误判为 true。
        // - `T? == T` / `T? != T`（T 为值类型）：拆箱后比较内值。
        // - `T? == T?` / `T? != T?`：两侧按 HasValue/Value 逐字段比较。
        if matches!(op, BinOp::Eq | BinOp::NotEq) {
            let l_inner = self.operand_nullable_value_inner(left);
            let r_inner = self.operand_nullable_value_inner(right);
            match (l_inner, r_inner) {
                (Some(_), None) if matches!(right, MirOperand::ConstNull) => {
                    return self.emit_nullable_null_eq(op, left);
                }
                (None, Some(_)) if matches!(left, MirOperand::ConstNull) => {
                    return self.emit_nullable_null_eq(op, right);
                }
                (Some(inner), None) => {
                    return self.emit_nullable_value_eq(op, left, right, &inner);
                }
                (None, Some(inner)) => {
                    return self.emit_nullable_value_eq(op, right, left, &inner);
                }
                (Some(inner), Some(_)) => {
                    return self.emit_nullable_nullable_eq(op, left, right, &inner);
                }
                _ => {} // fall through to regular path
            }
        }

        // Interface fat-pointer equality（RFC 037 texture-surface 暴露）：接口值
        // 为指向 `{ ptr obj, ptr itable }` 的胖指针；每次具体类→接口转换都新建
        // 独立 fat 槽（MakeIface alloca/box），地址比较会误判同对象不等（如
        // VideoSurface.Detach 的 `backend != _backend` 静默失效 → 纹理泄漏）。
        // 语义上按底层 obj 身份比较（含 null 安全：fat==null 或 obj==null 均视为
        // null）。类引用仍走下方通用 `icmp eq ptr`（对象地址即身份），不受影响。
        // 置于 string==null 之前：`iface == null` 的 null 语义须经
        // emit_iface_equality 统一处理（含 `(I)null` 装箱后 obj==null 的情形），
        // 而非退回 ptr 地址比较。
        if matches!(op, BinOp::Eq | BinOp::NotEq)
            && (self.operand_is_interface(left) || self.operand_is_interface(right))
        {
            return self.emit_iface_equality(op, left, right);
        }

        // String concat / compare
        if self.is_string_operand(left) && self.is_string_operand(right) {
            return self.emit_string_binary(op, left, right);
        }

        // string == null / != null：一侧 string、一侧 ConstNull 时走 ptr icmp，
        // 避免 MIR 误推 Int 后 `icmp eq i32, null`（clang 拒收）。
        if matches!(op, BinOp::Eq | BinOp::NotEq)
            && (matches!(left, MirOperand::ConstNull) || matches!(right, MirOperand::ConstNull))
            && (self.is_string_operand(left)
                || self.is_string_operand(right)
                || matches!(left, MirOperand::ConstNull)
                || matches!(right, MirOperand::ConstNull))
        {
            let (lty, lval) = self.emit_operand(left);
            let (rty, rval) = self.emit_operand(right);
            let lval = if lty == "ptr" {
                lval
            } else {
                let (_, v) = self.coerce_value(&lty, lval, "ptr");
                v
            };
            let rval = if rty == "ptr" {
                rval
            } else {
                let (_, v) = self.coerce_value(&rty, rval, "ptr");
                v
            };
            let tmp = self.fresh_temp();
            let pred = if op == BinOp::Eq { "eq" } else { "ne" };
            self.emit(&format!("{tmp} = icmp {pred} ptr {lval}, {rval}"));
            return ("i1".into(), tmp);
        }

        // String + non-string concat (e.g. "body: " + len).
        // Typeck already confirmed this is string concatenation.
        if op == BinOp::Add && (self.is_string_operand(left) || self.is_string_operand(right)) {
            return self.emit_string_primitive_concat(op, left, right);
        }

        let (lty, lval) = self.emit_operand(left);
        let (rty, rval) = self.emit_operand(right);

        // Defensive fallback: if either operand is ptr and op is Add,
        // treat as string concat. Converts non-ptr to string via rt_int_to_string.
        if op == BinOp::Add && (lty == "ptr" || rty == "ptr") {
            let lv = self.convert_to_string(Some(left), &lty, &lval);
            let rv = self.convert_to_string(Some(right), &rty, &rval);
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call ptr @rt_str_concat(ptr {lv}, ptr {rv})"
            ));
            return ("ptr".into(), tmp);
        }

        // Numeric promotion (RFC 007): align both operands to a common type.
        //   - float/double → promote to double if either is double, else float.
        //   - integer/integer → C# promotes byte/short to int (i32) minimum;
        //     wider of operands wins; zext byte (unsigned), sext others.
        //   - integer/floating → uitofp byte, sitofp others.
        let l_is_fp = lty == "float" || lty == "double";
        let r_is_fp = rty == "float" || rty == "double";
        let l_rank = int_rank(&lty);
        let r_rank = int_rank(&rty);
        let (lval, rval, operand_ty) = if l_is_fp && r_is_fp {
            let target = if lty == "double" || rty == "double" {
                "double"
            } else {
                "float"
            };
            let lval = if lty != target {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = fpext {lty} {lval} to {target}"));
                t
            } else {
                lval
            };
            let rval = if rty != target {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = fpext {rty} {rval} to {target}"));
                t
            } else {
                rval
            };
            (lval, rval, target.to_string())
        } else if l_is_fp && r_rank.is_some() {
            let cvt = if self.op_is_unsigned_int(right, &rty) {
                "uitofp"
            } else {
                "sitofp"
            };
            let t = self.fresh_temp();
            self.emit(&format!("{t} = {cvt} {rty} {rval} to {lty}"));
            (lval, t, lty.clone())
        } else if r_is_fp && l_rank.is_some() {
            let cvt = if self.op_is_unsigned_int(left, &lty) {
                "uitofp"
            } else {
                "sitofp"
            };
            let t = self.fresh_temp();
            self.emit(&format!("{t} = {cvt} {lty} {lval} to {rty}"));
            (t, rval, rty.clone())
        } else if let (Some(lr), Some(rr)) = (l_rank, r_rank) {
            // C# numeric promotion: byte/short arithmetic is done at int (i32) or wider.
            // INT_TYS ranks: i64=0, i32=1, i16=2, i8=3; smaller rank = wider type.
            // Target rank = min(wider_operand_rank, i32_rank=1) so i8/i16 promote to i32.
            let target_rank = lr.min(rr).min(1);
            let target = INT_TYS[target_rank];
            let lval = if lty != target {
                let ext = if self.op_is_unsigned_int(left, &lty) {
                    "zext"
                } else {
                    "sext"
                };
                let t = self.fresh_temp();
                self.emit(&format!("{t} = {ext} {lty} {lval} to {target}"));
                t
            } else {
                lval
            };
            let rval = if rty != target {
                let ext = if self.op_is_unsigned_int(right, &rty) {
                    "zext"
                } else {
                    "sext"
                };
                let t = self.fresh_temp();
                self.emit(&format!("{t} = {ext} {rty} {rval} to {target}"));
                t
            } else {
                rval
            };
            (lval, rval, target.to_string())
        } else {
            (lval, rval, lty.clone())
        };

        let is_float = operand_ty == "double" || operand_ty == "float";
        // After promotion, comparisons/division are unsigned only if BOTH
        // original operands are unsigned ints (byte, uint, ulong, ushort, char).
        let is_unsigned = !is_float && is_unsigned_int_ty(&lty) && is_unsigned_int_ty(&rty);

        // H1 / C ABI：class bool 字段存 i32，locals 为 i1。`&&`/`||` 须先统一到 i1，
        // 否则 `or i1 %field_i32, %cmp` → clang 拒收（AIToolSandbox._capDenied || …）。
        let (lval, rval, operand_ty) = if matches!(op, BinOp::And | BinOp::Or) {
            let (_, lv) = self.coerce_value(&lty, lval, "i1");
            let (_, rv) = self.coerce_value(&rty, rval, "i1");
            (lv, rv, "i1".to_string())
        } else {
            (lval, rval, operand_ty)
        };

        let (instr, result_ty) = match op {
            BinOp::Add => (if is_float { "fadd" } else { "add" }, operand_ty.clone()),
            BinOp::Sub => (if is_float { "fsub" } else { "sub" }, operand_ty.clone()),
            BinOp::Mul => (if is_float { "fmul" } else { "mul" }, operand_ty.clone()),
            BinOp::Div => (
                if is_float {
                    "fdiv"
                } else if is_unsigned {
                    "udiv"
                } else {
                    "sdiv"
                },
                operand_ty.clone(),
            ),
            BinOp::Mod => (
                if is_float {
                    "frem"
                } else if is_unsigned {
                    "urem"
                } else {
                    "srem"
                },
                operand_ty.clone(),
            ),
            BinOp::Eq => (if is_float { "fcmp oeq" } else { "icmp eq" }, "i1".into()),
            BinOp::NotEq => (if is_float { "fcmp one" } else { "icmp ne" }, "i1".into()),
            BinOp::Lt => (
                if is_float {
                    "fcmp olt"
                } else if is_unsigned {
                    "icmp ult"
                } else {
                    "icmp slt"
                },
                "i1".into(),
            ),
            BinOp::Le => (
                if is_float {
                    "fcmp ole"
                } else if is_unsigned {
                    "icmp ule"
                } else {
                    "icmp sle"
                },
                "i1".into(),
            ),
            BinOp::Gt => (
                if is_float {
                    "fcmp ogt"
                } else if is_unsigned {
                    "icmp ugt"
                } else {
                    "icmp sgt"
                },
                "i1".into(),
            ),
            BinOp::Ge => (
                if is_float {
                    "fcmp oge"
                } else if is_unsigned {
                    "icmp uge"
                } else {
                    "icmp sge"
                },
                "i1".into(),
            ),
            BinOp::And => ("and", "i1".into()),
            BinOp::Or => ("or", "i1".into()),
            BinOp::BitAnd => ("and", operand_ty.clone()),
            BinOp::BitOr => ("or", operand_ty.clone()),
            BinOp::BitXor => ("xor", operand_ty.clone()),
            BinOp::Shl => ("shl", operand_ty.clone()),
            BinOp::Shr => (
                if is_unsigned { "lshr" } else { "ashr" },
                operand_ty.clone(),
            ),
        };

        // For icmp/fcmp, the type parameter is the operand type, not the result type (i1).
        // For arithmetic ops, operand type == result type.
        let binop_ty = if instr.starts_with("icmp") || instr.starts_with("fcmp") {
            operand_ty.clone()
        } else {
            result_ty.clone()
        };

        let tmp = self.fresh_temp();
        self.emit(&format!("{tmp} = {instr} {binop_ty} {lval}, {rval}"));
        (result_ty, tmp)
    }

    fn emit_string_binary(&mut self, op: BinOp, left: &MirOperand, right: &MirOperand) -> TyVal {
        let (_, lval) = self.emit_operand(left);
        let (_, rval) = self.emit_operand(right);
        match op {
            BinOp::Add => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_concat(ptr {lval}, ptr {rval})"
                ));
                ("ptr".into(), tmp)
            }
            BinOp::Eq | BinOp::NotEq => {
                let cmp = self.fresh_temp();
                self.emit(&format!(
                    "{cmp} = call i32 @rt_str_equals(ptr {lval}, ptr {rval})"
                ));
                if matches!(op, BinOp::Eq) {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp ne i32 {cmp}, 0"));
                    ("i1".into(), tmp)
                } else {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = icmp eq i32 {cmp}, 0"));
                    ("i1".into(), tmp)
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let cmp = self.fresh_temp();
                self.emit(&format!(
                    "{cmp} = call i32 @rt_str_compare(ptr {lval}, ptr {rval})"
                ));
                let (cmp_op, _) = match op {
                    BinOp::Lt => ("slt", -1),
                    BinOp::Le => ("sle", 0),
                    BinOp::Gt => ("sgt", 0),
                    BinOp::Ge => ("sge", 0),
                    _ => unreachable!(),
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp {cmp_op} i32 {cmp}, 0"));
                ("i1".into(), tmp)
            }
            _ => ("i32".into(), "0".into()),
        }
    }

    /// Emit `string + primitive` concatenation.
    /// One operand is string (already ptr), the other is a non-string primitive
    /// (int, double, etc.) that must be converted to string first.
    fn emit_string_primitive_concat(
        &mut self,
        op: BinOp,
        left: &MirOperand,
        right: &MirOperand,
    ) -> TyVal {
        let (lty, lval) = self.emit_operand(left);
        let (rty, rval) = self.emit_operand(right);
        let lv = self.convert_to_string(Some(left), &lty, &lval);
        let rv = self.convert_to_string(Some(right), &rty, &rval);
        if op == BinOp::Add {
            let tmp = self.fresh_temp();
            self.emit(&format!(
                "{tmp} = call ptr @rt_str_concat(ptr {lv}, ptr {rv})"
            ));
            ("ptr".into(), tmp)
        } else {
            // Comparison: only string==string is meaningful; shouldn't occur.
            ("i32".into(), "0".into())
        }
    }

    /// Convert a non-string LLVM value to a string (ptr).
    /// If the value is already ptr, return it unchanged.
    fn convert_to_string(&mut self, op: Option<&MirOperand>, lty: &str, lval: &str) -> String {
        if lty == "ptr" {
            return lval.to_string();
        }
        // `char` 与 `byte`/`int` 在 LLVM 同为 i32，但拼接语义不同：
        // `"" + (char)52` → 单字符串 "4"（rt_str_from_codepoint）；
        // `"" + 52`       → 十进制串 "52"（rt_int_to_string）。
        // 此前 char 走 rt_int_to_string 把 ASCII 码当十进制数转串，
        // 产出 "52" 而非 "4"，污染 `IS(64)` → "5452" 等整串数字。
        if op.is_some_and(|o| self.operand_is_char(o)) {
            let c = self.fresh_temp();
            self.emit(&format!(
                "{c} = call ptr @rt_str_from_codepoint(i32 {lval})"
            ));
            return c;
        }
        // Cast to the expected input type and call the appropriate runtime function.
        let tmp = self.fresh_temp();
        match lty {
            "i8" | "i16" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = sext {lty} {lval} to i32"));
                self.emit(&format!("{tmp} = call ptr @rt_int_to_string(i32 {t})"));
            }
            "i32" => {
                self.emit(&format!("{tmp} = call ptr @rt_int_to_string(i32 {lval})"));
            }
            "i64" => {
                self.emit(&format!("{tmp} = call ptr @rt_long_to_string(i64 {lval})"));
            }
            "float" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = fpext float {lval} to double"));
                self.emit(&format!(
                    "{tmp} = call ptr @rt_double_to_string(double {t})"
                ));
            }
            "double" => {
                self.emit(&format!(
                    "{tmp} = call ptr @rt_double_to_string(double {lval})"
                ));
            }
            "i1" => {
                let t = self.fresh_temp();
                self.emit(&format!("{t} = zext i1 {lval} to i32"));
                self.emit(&format!("{tmp} = call ptr @rt_bool_to_string(i32 {t})"));
            }
            _ => {
                // Fallback: treat unknown type as i32
                self.emit(&format!("{tmp} = call ptr @rt_int_to_string(i32 {lval})"));
            }
        }
        tmp
    }

    /// 判断操作数是否为 `char` 类型（LLVM i32，需与 int/byte 区分的唯一依据是
    /// MIR 类型信息）。仅 Local 可精确判定；字面量 `'' + 'a'` 的 ConstInt 无法
    /// 与 int 字面量区分，由 typeck 保留类型上下文兜底（见 check_expr）。
    fn operand_is_char(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::Local(id) => matches!(self.local_type(*id), TypeId::Char),
            _ => false,
        }
    }

    /// 判断操作数静态类型是否为接口（fat-pointer 值）。
    ///
    /// 接口名遵循 `is_iface_name` 约定（`I` + 大写，含泛型 mangle 形如
    /// `IGetter_Dog`）。类引用/基元/字符串返回 false，保持各自既有比较路径。
    fn operand_is_interface(&self, op: &MirOperand) -> bool {
        match op {
            MirOperand::Local(id) | MirOperand::AddrOf(id) => {
                is_iface_name(&self.local_type(*id).display())
            }
            MirOperand::Field { class, field, .. } => {
                let (class, field) = (class.as_str(), field.as_str());
                let field_ty = if self.layouts.structs.contains_key(class) {
                    self.struct_field_info(class, field).1
                } else {
                    self.field_info(class, field).1
                };
                is_iface_name(field_ty.as_str())
            }
            // class → interface 装箱（fat-pointer 值）本身即接口。
            MirOperand::Iface { .. } => true,
            _ => false,
        }
    }

    /// 发射接口 fat-pointer 相等比较（`==`/`!=`）。
    ///
    /// 按底层 obj 身份比较而非 fat 槽地址（见 `emit_binary` 说明）。null 安全：
    /// 空等价归一化——fat 为 null（`ptr == null`）或底层 obj 为 null（`(I)null`
    /// 装箱）均视为空；统一处理两侧皆空、单侧空、及两侧非空按 obj 比较。
    ///
    /// LLVM IR 形态（空等价归一化）：
    /// ```text
    /// %lnull = icmp eq ptr %l, null
    /// %rnull = icmp eq ptr %r, null
    /// %dl    = alloca { ptr, ptr }            ; fat null 时 select 到 dummy 防 UB
    /// %sl    = select i1 %lnull, ptr %dl, ptr %l
    /// %olv   = load ptr, ptr getelementptr {ptr,ptr}, %sl, 0, 0
    /// ...同 r...
    /// %lnullish = or i1 %lnull, (icmp eq %olv, null)   ; 空等价（fat 或 obj 空）
    /// %rnullish = or i1 %rnull, (icmp eq %orv, null)
    /// %obj_eq   = icmp eq ptr %olv, %orv
    /// %neither  = xor i1 (or %lnullish, %rnullish), true
    /// %eq       = or i1 (and %lnullish, %rnullish), (and i1 %neither, %obj_eq)
    /// ```
    fn emit_iface_equality(&mut self, op: BinOp, left: &MirOperand, right: &MirOperand) -> TyVal {
        let (_, lval) = self.emit_operand(left); // ptr（fat 地址或 null）
        let (_, rval) = self.emit_operand(right);
        let lnull = self.fresh_temp();
        self.emit(&format!("{lnull} = icmp eq ptr {lval}, null"));
        let rnull = self.fresh_temp();
        self.emit(&format!("{rnull} = icmp eq ptr {rval}, null"));

        // 安全读 obj：fat 为 null 时 select 到 dummy alloca，避免对 null 解引用 UB。
        // 此时 obj 读自 dummy（undef），但其结果被下方 `lnull | …` 以 lnull 为准
        // 短路，不产生误判。
        let dummy_l = self.fresh_temp();
        self.emit(&format!("{dummy_l} = alloca {{ ptr, ptr }}"));
        let safe_l = self.fresh_temp();
        self.emit(&format!(
            "{safe_l} = select i1 {lnull}, ptr {dummy_l}, ptr {lval}"
        ));
        let ol_addr = self.fresh_temp();
        self.emit(&format!(
            "{ol_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {safe_l}, i32 0, i32 0"
        ));
        let olv = self.fresh_temp();
        self.emit(&format!("{olv} = load ptr, ptr {ol_addr}"));

        let dummy_r = self.fresh_temp();
        self.emit(&format!("{dummy_r} = alloca {{ ptr, ptr }}"));
        let safe_r = self.fresh_temp();
        self.emit(&format!(
            "{safe_r} = select i1 {rnull}, ptr {dummy_r}, ptr {rval}"
        ));
        let or_addr = self.fresh_temp();
        self.emit(&format!(
            "{or_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {safe_r}, i32 0, i32 0"
        ));
        let orv = self.fresh_temp();
        self.emit(&format!("{orv} = load ptr, ptr {or_addr}"));

        // 空等价归一化：fat 为 null 或底层 obj 为 null 均视为空。统一处理
        // `null`、`(I)null`（装箱 obj==null）以及两者混用的对称/非对称情形。
        let l_obj_null = self.fresh_temp();
        self.emit(&format!("{l_obj_null} = icmp eq ptr {olv}, null"));
        let l_nullish = self.fresh_temp();
        self.emit(&format!("{l_nullish} = or i1 {lnull}, {l_obj_null}"));
        let r_obj_null = self.fresh_temp();
        self.emit(&format!("{r_obj_null} = icmp eq ptr {orv}, null"));
        let r_nullish = self.fresh_temp();
        self.emit(&format!("{r_nullish} = or i1 {rnull}, {r_obj_null}"));

        // 相等 = 两侧皆空，或两侧皆非空且底层 obj 相同。
        let both_nullish = self.fresh_temp();
        self.emit(&format!("{both_nullish} = and i1 {l_nullish}, {r_nullish}"));
        let obj_eq = self.fresh_temp();
        self.emit(&format!("{obj_eq} = icmp eq ptr {olv}, {orv}"));
        let either_nullish = self.fresh_temp();
        self.emit(&format!(
            "{either_nullish} = or i1 {l_nullish}, {r_nullish}"
        ));
        let neither_nullish = self.fresh_temp();
        self.emit(&format!(
            "{neither_nullish} = xor i1 {either_nullish}, true"
        ));
        let obj_branch = self.fresh_temp();
        self.emit(&format!(
            "{obj_branch} = and i1 {neither_nullish}, {obj_eq}"
        ));
        let eq = self.fresh_temp();
        self.emit(&format!("{eq} = or i1 {both_nullish}, {obj_branch}"));

        if matches!(op, BinOp::NotEq) {
            let ne = self.fresh_temp();
            self.emit(&format!("{ne} = xor i1 {eq}, true"));
            return ("i1".into(), ne);
        }
        ("i1".into(), eq)
    }

    // ---- Nullable value-type comparison (RFC 009 L2) ----

    /// 若 `op` 为 `Local(id)` 且 `local_type(id)` 为 `Nullable<T>`（T 为基元值类型），
    /// 返回 `Some(T)`；否则 `None`。
    ///
    /// 用于 `T? == T` / `T? != T` 比较的拆箱决策。引用类型 nullable（`string?` 等）
    /// 不在此处理——其 `ptr` 直接比较即可，由 emit_binary 既有路径处理。
    pub(super) fn operand_nullable_value_inner(&self, op: &MirOperand) -> Option<TypeId> {
        if let MirOperand::Local(id) = op {
            if let TypeId::Nullable { inner } = self.local_type(*id) {
                if is_primitive_value_type(&inner) {
                    return Some((*inner).clone());
                }
            }
        }
        None
    }

    /// 发射 `T? == null` / `T? != null` 比较（T 为基元值类型，另一侧为 `ConstNull`）。
    ///
    /// C# 语义：`nullable == null` ⟺ `HasValue == false`（与 Value 无关）；
    /// `nullable != null` ⟺ `HasValue`。值类型 `T?` 内联 `{ i1, T }`，取字段 0
    /// （HasValue）即可，不触碰 Value 字段。
    fn emit_nullable_null_eq(&mut self, op: BinOp, nullable_op: &MirOperand) -> TyVal {
        let (nullable_ty, nullable_val) = self.emit_operand(nullable_op);
        let has = self.fresh_temp();
        self.emit(&format!(
            "{has} = extractvalue {nullable_ty} {nullable_val}, 0"
        ));
        match op {
            BinOp::Eq => {
                let result = self.fresh_temp();
                self.emit(&format!("{result} = icmp eq i1 {has}, false"));
                ("i1".into(), result)
            }
            BinOp::NotEq => ("i1".into(), has),
            _ => unreachable!("emit_nullable_null_eq only handles Eq/NotEq"),
        }
    }

    /// 发射 `T? == T` / `T? != T` 比较（T 为基元值类型）。
    ///
    /// 语义：
    /// - `==`：nullable 非 null **且** 内值等于 T
    /// - `!=`：nullable 为 null **或** 内值不等于 T
    ///
    /// 值类型 `T?` 内联 `{ i1, T }`：`extractvalue` 取 `HasValue`/`Value`，无指针
    /// 解引用（消除既有「指针装箱」表示下非空值的悬垂 load）。
    fn emit_nullable_value_eq(
        &mut self,
        op: BinOp,
        nullable_op: &MirOperand,
        other_op: &MirOperand,
        inner: &TypeId,
    ) -> TyVal {
        let (nullable_ty, nullable_val) = self.emit_operand(nullable_op);
        let (other_ty, other_val) = self.emit_operand(other_op);
        let inner_llvm_ty = primitive_value_storage_llvm_type(inner).to_string();

        // 对齐 other 到 inner_llvm_ty（如 ConstInt(2) 默认 i32，但 inner 可能是 i64）
        let (_, other_safe) = self.coerce_value(&other_ty, other_val, &inner_llvm_ty);

        let has = self.fresh_temp();
        self.emit(&format!(
            "{has} = extractvalue {nullable_ty} {nullable_val}, 0"
        ));
        let value = self.fresh_temp();
        self.emit(&format!(
            "{value} = extractvalue {nullable_ty} {nullable_val}, 1"
        ));

        let result = self.fresh_temp();
        match op {
            BinOp::Eq => {
                let val_eq = self.fresh_temp();
                self.emit(&format!(
                    "{val_eq} = icmp eq {inner_llvm_ty} {value}, {other_safe}"
                ));
                self.emit(&format!("{result} = and i1 {has}, {val_eq}"));
            }
            BinOp::NotEq => {
                let val_ne = self.fresh_temp();
                self.emit(&format!(
                    "{val_ne} = icmp ne {inner_llvm_ty} {value}, {other_safe}"
                ));
                let not_has = self.fresh_temp();
                self.emit(&format!("{not_has} = xor i1 {has}, true"));
                self.emit(&format!("{result} = or i1 {not_has}, {val_ne}"));
            }
            _ => unreachable!("emit_nullable_value_eq only handles Eq/NotEq"),
        }
        ("i1".into(), result)
    }

    /// 发射 `T? == T?` / `T? != T?` 比较（两侧均为基元值类型可空）。
    ///
    /// 语义对齐 .NET `Nullable<T>`：`==` = 两侧 `HasValue` 相同且（皆无值 或 值相等）；
    /// `!=` 取反。两侧均内联 `{ i1, T }`，逐字段 `extractvalue` 比较。
    fn emit_nullable_nullable_eq(
        &mut self,
        op: BinOp,
        left: &MirOperand,
        right: &MirOperand,
        inner: &TypeId,
    ) -> TyVal {
        let (lty, lval) = self.emit_operand(left);
        let (rty, rval) = self.emit_operand(right);
        let inner_llvm = primitive_value_storage_llvm_type(inner).to_string();

        let l_has = self.fresh_temp();
        self.emit(&format!("{l_has} = extractvalue {lty} {lval}, 0"));
        let l_val = self.fresh_temp();
        self.emit(&format!("{l_val} = extractvalue {lty} {lval}, 1"));
        let r_has = self.fresh_temp();
        self.emit(&format!("{r_has} = extractvalue {rty} {rval}, 0"));
        let r_val = self.fresh_temp();
        self.emit(&format!("{r_val} = extractvalue {rty} {rval}, 1"));

        let result = self.fresh_temp();
        match op {
            BinOp::Eq => {
                let has_eq = self.fresh_temp();
                self.emit(&format!("{has_eq} = icmp eq i1 {l_has}, {r_has}"));
                let val_eq = self.fresh_temp();
                self.emit(&format!("{val_eq} = icmp eq {inner_llvm} {l_val}, {r_val}"));
                let not_l_has = self.fresh_temp();
                self.emit(&format!("{not_l_has} = xor i1 {l_has}, true"));
                let either_null = self.fresh_temp();
                self.emit(&format!("{either_null} = or i1 {not_l_has}, {val_eq}"));
                self.emit(&format!("{result} = and i1 {has_eq}, {either_null}"));
            }
            BinOp::NotEq => {
                let has_ne = self.fresh_temp();
                self.emit(&format!("{has_ne} = icmp ne i1 {l_has}, {r_has}"));
                let val_ne = self.fresh_temp();
                self.emit(&format!("{val_ne} = icmp ne {inner_llvm} {l_val}, {r_val}"));
                let both_has = self.fresh_temp();
                self.emit(&format!("{both_has} = and i1 {l_has}, {r_has}"));
                let both_has_ne = self.fresh_temp();
                self.emit(&format!("{both_has_ne} = and i1 {both_has}, {val_ne}"));
                self.emit(&format!("{result} = or i1 {has_ne}, {both_has_ne}"));
            }
            _ => unreachable!("emit_nullable_nullable_eq only handles Eq/NotEq"),
        }
        ("i1".into(), result)
    }
}
