//! Builtin method dispatch registry.
//!
//! 类名是否为 stub facade 以 `typeck::classify_builtin_facade` /
//! `is_builtin_facade` 为唯一清单；本模块只负责发射 `rt_*` ABI。
//! 新增 facade：先改 `crates/typeck/src/builtin_facade.rs`，再在此补 handler。
//!
//! Design:
//!   - func-form: `try_emit_builtin_static` — matches `"Class.Method"` from MIR Call
//!   - method-form: `try_emit_builtin_method` — `classify_builtin_facade` + handler

use super::*;
use ast::TypeId;
use mir::MirOperand;
use typeck::{classify_builtin_facade, BuiltinFacadeKind};

impl<'a> FnEmitter<'a> {
    // ---- Static func-form dispatch ----

    /// Try to handle `Class.Method(...)` via the builtin dispatch table.
    /// Returns `Some(result)` if matched and emitted; `None` otherwise.
    pub(super) fn try_emit_builtin_static(
        &mut self,
        func: &str,
        args: &[MirOperand],
        expected: &TypeId,
    ) -> Option<TyVal> {
        match func {
            // ── Environment (Phase 1 + Phase 2) ──
            func if func.starts_with("Environment.") => {
                let method = func.strip_prefix("Environment.").unwrap();
                self.try_emit_environment_static(method, args)
            }

            // ── Console ──
            "Console.WriteLine"
            | "Console.Write"
            | "Console.ReadLine"
            | "Console.Read"
            | "Console.SetForegroundColor"
            | "Console.SetBackgroundColor"
            | "Console.GetForegroundColor"
            | "Console.GetBackgroundColor"
            | "Console.ResetColor"
            | "Console.ErrorWriteLine"
            | "Console.ErrorWrite" => {
                let method = func.strip_prefix("Console.").unwrap();
                self.try_emit_console_static(method, args)
            }

            // ── Window ──
            "Window.Run" => {
                let (_, title) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString("Arc".into())),
                );
                let (_, width) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(640)));
                let (_, height) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(480)));
                self.emit(&format!(
                    "call void @__arc_window_run(ptr {title}, i32 {width}, i32 {height})"
                ));
                Some(("void".into(), String::new()))
            }
            "Window.RunWithText" => {
                let (_, title) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString("Arc".into())),
                );
                let (_, width) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(640)));
                let (_, height) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(480)));
                let (_, text) = self.emit_operand(
                    &args
                        .get(3)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                self.emit(&format!("call void @__arc_window_run_with_text(ptr {title}, i32 {width}, i32 {height}, ptr {text})"));
                Some(("void".into(), String::new()))
            }

            // ── File I/O ──
            "File.ReadAllText" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_read_file(ptr {path})"));
                Some(("ptr".into(), tmp))
            }
            "File.WriteAllText" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, content) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_write_file(ptr {path}, ptr {content})"
                ));
                Some(("i32".into(), tmp))
            }
            "File.ReadAllBytes" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_file_read_all_bytes(ptr {path})"
                ));
                Some(("ptr".into(), tmp))
            }
            "File.WriteAllBytes" => {
                let (_, path) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, bytes) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_file_write_all_bytes(ptr {path}, ptr {bytes})"
                ));
                Some(("i32".into(), tmp))
            }

            // ── Task static ──
            "Task.FromResult" | "Task.WhenAll" | "Task.WhenAny" | "Task.CompletedTask"
            | "Task.FromCanceled" | "Task.FromException" | "Task.WaitAll" | "Task.WaitAny"
            | "Task.Delay" | "Task.Run" => {
                let method = func.strip_prefix("Task.").unwrap();
                self.try_emit_task_static(method, args, expected)
            }

            // ── Thread static ──
            "Thread.Sleep" => {
                let (ty, ms) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                // C# 签名 `Thread.Sleep(int)` 实参为 i32，ABI 参数为 i64（毫秒）——
                // 常量实参侥幸字面兼容，变量实参须显式 sext 加宽。
                let ms64 = if ty == "i32" {
                    let tmp = self.fresh_temp();
                    self.emit(&format!("{tmp} = sext i32 {ms} to i64"));
                    tmp
                } else {
                    ms
                };
                self.emit(&format!("call void @rt_thread_sleep(i64 {ms64})"));
                Some(("void".into(), String::new()))
            }
            "Thread.CurrentThread" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_thread_current()"));
                Some(("ptr".into(), tmp))
            }
            // ManagedThreadId 表面类型为 int；ABI 返回 i64，截断为 i32。
            "Thread.ManagedThreadId" => {
                let tmp64 = self.fresh_temp();
                self.emit(&format!("{tmp64} = call i64 @rt_thread_current_id()"));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = trunc i64 {tmp64} to i32"));
                Some(("i32".into(), tmp))
            }

            // ── CancellationToken static ──
            // CancellationToken.None（RFC 009 M4）：语义对齐 .NET，None 是永不可取消
            // 的空令牌。与 `new CancellationToken()` 同路径（rt_cts_create 创建
            // canceled=0 的 RtCts 指针）。
            "CancellationToken.None" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_cts_create()"));
                Some(("ptr".into(), tmp))
            }

            // 默认 Task.Run 池：报告前 join（atexit 太晚，WriteResults 仍与 worker 争堆）。
            "ThreadPoolScheduler.ShutdownDefaultPool" => {
                self.emit("call void @rt_default_pool_shutdown()");
                Some(("void".into(), String::new()))
            }

            // ── Monitor static ──
            "Monitor.Enter" | "Monitor.Exit" | "Monitor.TryEnter" | "Monitor.Wait"
            | "Monitor.Pulse" | "Monitor.PulseAll" => {
                let method = func.strip_prefix("Monitor.").unwrap();
                self.try_emit_monitor_static(method, args)
            }

            // ── Interlocked static（RFC 009 §7.5；LLVM atomicrmw/cmpxchg）──
            "Interlocked.Increment"
            | "Interlocked.Decrement"
            | "Interlocked.Exchange"
            | "Interlocked.CompareExchange" => {
                let method = func.strip_prefix("Interlocked.").unwrap();
                self.try_emit_interlocked_static(method, args)
            }

            // ── Parallel ──
            // RFC 009 M2: Parallel.For 已实现（emit_parallel_for）。
            // RFC 009 M6: Parallel.ForEach 数组并行遍历（emit_parallel_foreach）。
            "Parallel.For" => Some(self.emit_parallel_for(args)),
            "Parallel.ForEach" => Some(self.emit_parallel_foreach(args)),

            // ── Base64 / Hex / Url ──
            "Base64.Encode"
            | "Base64.Decode"
            | "Base64.ToBase64String"
            | "Base64.FromBase64String"
            | "Hex.Encode"
            | "Hex.Decode"
            | "Hex.ToHexString"
            | "Hex.FromHexString"
            | "Url.Encode"
            | "Url.Decode" => {
                let (prefix, rest) = if func.starts_with("Base64.") {
                    ("Base64", func.strip_prefix("Base64.").unwrap())
                } else if func.starts_with("Url.") {
                    ("Url", func.strip_prefix("Url.").unwrap())
                } else {
                    ("Hex", func.strip_prefix("Hex.").unwrap())
                };
                let abi = match (prefix, rest) {
                    ("Base64", "Encode") => "@rt_text_base64_encode",
                    ("Base64", "Decode") => "@rt_text_base64_decode",
                    // RFC 037 M1 §1.2 ⑥: byte[] ↔ base64（ToBase64String / FromBase64String）
                    ("Base64", "ToBase64String") => "@rt_text_base64_bytes_encode",
                    ("Base64", "FromBase64String") => "@rt_text_base64_bytes_decode",
                    ("Hex", "Encode") => "@rt_text_hex_encode",
                    ("Hex", "Decode") => "@rt_text_hex_decode",
                    // RFC 026 M1 §1.2 ⑥: byte[] ↔ hex（ToHexString / FromHexString）
                    ("Hex", "ToHexString") => "@rt_text_hex_bytes_encode",
                    ("Hex", "FromHexString") => "@rt_text_hex_bytes_decode",
                    // Arc.Text.Url percent-encoding（WebUtility 对齐）
                    ("Url", "Encode") => "@rt_text_url_encode",
                    ("Url", "Decode") => "@rt_text_url_decode",
                    _ => return None,
                };
                let (_, arg) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }

            // ── Encoding UTF-8 ──
            "Encoding.GetBytes" | "Encoding.GetString" => {
                let abi = if func.ends_with("GetBytes") {
                    "@rt_text_utf8_get_bytes"
                } else {
                    "@rt_text_utf8_get_string"
                };
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }
            "Encoding.GetByteCount" => {
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_text_utf8_get_byte_count(ptr {arg})"
                ));
                Some(("i32".into(), tmp))
            }
            // ── Encoding 变体：UTF-16LE / Latin-1（byte[] 往返）──
            "Encoding.GetBytesUtf16" | "Encoding.GetBytesLatin1" => {
                let abi = if func.ends_with("Utf16") {
                    "@rt_text_utf16_get_bytes"
                } else {
                    "@rt_text_latin1_get_bytes"
                };
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }
            "Encoding.GetStringUtf16" | "Encoding.GetStringLatin1" => {
                let abi = if func.ends_with("Utf16") {
                    "@rt_text_utf16_get_string"
                } else {
                    "@rt_text_latin1_get_string"
                };
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }

            // ── Regex：Arc.Text.Regex facade（rt_regex_* ABI）──
            // 带 RegexOptions 的重载（末参 int32 flags）与无 options 旧版共用 dotted key，
            // 按 args.len() 分派。flags: IgnoreCase=1 Multiline=2 Singleline=4 ExplicitCapture=8。
            "Regex.IsMatch" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 3 {
                    let (_, oa) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_regex_is_match_opt(ptr {pa}, ptr {ina}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_regex_is_match(ptr {pa}, ptr {ina})"
                    ));
                }
                Some(("i32".into(), tmp))
            }
            "Regex.Match" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 3 {
                    let (_, oa) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_match_opt(ptr {pa}, ptr {ina}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_match(ptr {pa}, ptr {ina})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }
            "Regex.MatchGroup" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ga) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                if args.len() >= 4 {
                    let (_, oa) =
                        self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_match_group_opt(ptr {pa}, ptr {ina}, i32 {ga}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_match_group(ptr {pa}, ptr {ina}, i32 {ga})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }
            "Regex.Matches" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 3 {
                    let (_, oa) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_matches_opt(ptr {pa}, ptr {ina}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_matches(ptr {pa}, ptr {ina})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }
            "Regex.Replace" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ra) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 4 {
                    let (_, oa) =
                        self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_replace_opt(ptr {pa}, ptr {ina}, ptr {ra}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_replace(ptr {pa}, ptr {ina}, ptr {ra})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }
            "Regex.Split" => {
                let (_, pa) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, ina) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if args.len() >= 3 {
                    let (_, oa) =
                        self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_split_opt(ptr {pa}, ptr {ina}, i32 {oa})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_regex_split(ptr {pa}, ptr {ina})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }

            // ── JsonReader codepoint → UTF-8 ──
            "JsonReader._codePointToString" => {
                let (_, code) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_from_codepoint(i32 {code})"
                ));
                Some(("ptr".into(), tmp))
            }

            // ── string static ──
            "string.IsNullOrEmpty" => {
                let (_, s) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                /* null check: ptr == null */
                let null_chk = self.fresh_temp();
                self.emit(&format!("{null_chk} = icmp eq ptr {s}, null"));
                /* empty check: strlen == 0 */
                let len = self.fresh_temp();
                self.emit(&format!("{len} = call i32 @rt_str_length(ptr {s})"));
                let empty_chk = self.fresh_temp();
                self.emit(&format!("{empty_chk} = icmp eq i32 {len}, 0"));
                /* result = null || empty */
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = or i1 {null_chk}, {empty_chk}"));
                Some(("i1".into(), tmp))
            }
            "string.IsNullOrWhiteSpace" => {
                let (_, s) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let int_tmp = self.fresh_temp();
                self.emit(&format!(
                    "{int_tmp} = call i32 @rt_str_is_null_or_white_space(ptr {s})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {int_tmp}, 0"));
                Some(("i1".into(), tmp))
            }
            "string.FromCharCount" => {
                let (_, c) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, cnt) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_from_char_count(i32 {c}, i32 {cnt})"
                ));
                Some(("ptr".into(), tmp))
            }
            func if func == "string.Format" || func.starts_with("string.Format_") => {
                let (_, fmt) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let mut argv: Vec<String> = Vec::with_capacity(4);
                for (i, a) in args.iter().enumerate().skip(1) {
                    if i > 4 {
                        break;
                    }
                    let (_, v) = self.emit_operand(a);
                    argv.push(format!("ptr {v}"));
                }
                while argv.len() < 4 {
                    argv.push("ptr null".into());
                }
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_format(ptr {fmt}, {})",
                    argv.join(", ")
                ));
                Some(("ptr".into(), tmp))
            }
            "string.Concat" => {
                let (_, a) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, b) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_str_concat(ptr {a}, ptr {b})"
                ));
                Some(("ptr".into(), tmp))
            }
            "string.Compare" | "string.CompareOrdinal" => {
                let (_, left) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, right) = self.emit_operand(
                    &args
                        .get(1)
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_str_compare(ptr {left}, ptr {right})"
                ));
                Some(("i32".into(), tmp))
            }
            "string.Join" => {
                let (sep_ty, sep) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, arr) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                if sep_ty == "i32" {
                    // Join(char, string[]) — UTF-8 码元分隔符
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_join_char(i32 {sep}, ptr {arr})"
                    ));
                } else {
                    self.emit(&format!(
                        "{tmp} = call ptr @rt_str_join(ptr {sep}, ptr {arr})"
                    ));
                }
                Some(("ptr".into(), tmp))
            }

            // ── RFC 026 M3 P0-1：Security 哈希/HMAC/CSPRNG 门面已改 AesGcm 模式，
            // 私有静态 ABI 拦截（`Class::_Method` 形态）见下方 Security 区块 ──

            // ── Dns static (RFC 026 M4) ──
            "Dns.Resolve" => {
                let (_, host) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_dns_resolve(ptr {host})"));
                Some(("ptr".into(), tmp))
            }
            "Dns.GetHostAddresses" => {
                let (_, host) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_dns_resolve_all(ptr {host})"));
                Some(("ptr".into(), tmp))
            }
            "Dns.GetHostEntry" => {
                let (_, host) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let addr_tmp = self.fresh_temp();
                self.emit(&format!(
                    "{addr_tmp} = call ptr @rt_dns_resolve_all(ptr {host})"
                ));
                let entry_tmp = self.fresh_temp();
                self.emit(&format!("{entry_tmp} = alloca {{ ptr, ptr }}"));
                let f0_addr = self.fresh_temp();
                self.emit(&format!("{f0_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {entry_tmp}, i32 0, i32 0"));
                self.emit(&format!("store ptr {host}, ptr {f0_addr}"));
                let f1_addr = self.fresh_temp();
                self.emit(&format!("{f1_addr} = getelementptr inbounds {{ ptr, ptr }}, ptr {entry_tmp}, i32 0, i32 1"));
                self.emit(&format!("store ptr {addr_tmp}, ptr {f1_addr}"));
                Some(("ptr".into(), entry_tmp))
            }
            "Dns.GetHostName" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_dns_get_host_name()"));
                Some(("ptr".into(), tmp))
            }

            // ── BitConverter (host endian) ──
            // float/double 位重释走 LLVM bitcast（编译期内建）；字节编解码复用
            // rt_bitconverter_* i32/i64 ABI（与 int/long 端序行为一致，无新增 rt_* 符号）。
            "BitConverter.IsLittleEndian" => {
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_bitconverter_is_little_endian()"
                ));
                Some(("i32".into(), tmp))
            }
            "BitConverter.GetBytes" => {
                let (ty, v) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                match ty.as_str() {
                    "i64" => {
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_bitconverter_get_bytes_i64(i64 {v})"
                        ));
                    }
                    // double：位型重释为 i64 后走既有 8 字节 ABI。
                    "double" => {
                        let bits = self.fresh_temp();
                        self.emit(&format!("{bits} = bitcast double {v} to i64"));
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_bitconverter_get_bytes_i64(i64 {bits})"
                        ));
                    }
                    // float：位型重释为 i32 后走既有 4 字节 ABI。
                    "float" => {
                        let bits = self.fresh_temp();
                        self.emit(&format!("{bits} = bitcast float {v} to i32"));
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_bitconverter_get_bytes_i32(i32 {bits})"
                        ));
                    }
                    _ => {
                        let arg = if ty == "i32" {
                            v
                        } else {
                            let t = self.fresh_temp();
                            self.emit(&format!("{t} = trunc {ty} {v} to i32"));
                            t
                        };
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_bitconverter_get_bytes_i32(i32 {arg})"
                        ));
                    }
                }
                Some(("ptr".into(), tmp))
            }
            "BitConverter.ToInt32" => {
                let (_, bytes) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, start) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_bitconverter_to_i32(ptr {bytes}, i32 {start})"
                ));
                Some(("i32".into(), tmp))
            }
            "BitConverter.ToInt64" => {
                let (_, bytes) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, start) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i64 @rt_bitconverter_to_i64(ptr {bytes}, i32 {start})"
                ));
                Some(("i64".into(), tmp))
            }
            "BitConverter.ToSingle" => {
                let (_, bytes) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, start) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let t32 = self.fresh_temp();
                self.emit(&format!(
                    "{t32} = call i32 @rt_bitconverter_to_i32(ptr {bytes}, i32 {start})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast i32 {t32} to float"));
                Some(("float".into(), tmp))
            }
            "BitConverter.ToDouble" => {
                let (_, bytes) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, start) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let t64 = self.fresh_temp();
                self.emit(&format!(
                    "{t64} = call i64 @rt_bitconverter_to_i64(ptr {bytes}, i32 {start})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast i64 {t64} to double"));
                Some(("double".into(), tmp))
            }
            "BitConverter.SingleToInt32Bits" => {
                let (ty, v) = self
                    .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstFloat(0.0)));
                let src = if ty == "double" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = fptrunc double {v} to float"));
                    t
                } else {
                    v
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast float {src} to i32"));
                Some(("i32".into(), tmp))
            }
            "BitConverter.Int32BitsToSingle" => {
                let (ty, v) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let src = if ty == "i64" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = trunc i64 {v} to i32"));
                    t
                } else {
                    v
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast i32 {src} to float"));
                Some(("float".into(), tmp))
            }
            "BitConverter.DoubleToInt64Bits" => {
                let (ty, v) = self
                    .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstFloat(0.0)));
                let src = if ty == "float" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = fpext float {v} to double"));
                    t
                } else {
                    v
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast double {src} to i64"));
                Some(("i64".into(), tmp))
            }
            "BitConverter.Int64BitsToDouble" => {
                let (ty, v) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let src = if ty == "i32" {
                    let t = self.fresh_temp();
                    self.emit(&format!("{t} = sext i32 {v} to i64"));
                    t
                } else {
                    v
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = bitcast i64 {src} to double"));
                Some(("double".into(), tmp))
            }
            // ── Buffer.BlockCopy (byte[] → rt_array_copy) ──
            "Buffer.BlockCopy" => {
                let (_, src) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, src_off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, dst) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, dst_off) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, count) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_array_copy(ptr {src}, i32 {src_off}, ptr {dst}, i32 {dst_off}, i32 {count})"
                ));
                Some(("void".into(), String::new()))
            }

            // ── Array static (P5-F) ──
            "Array.Copy" => {
                let (_, src) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, src_off) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, dst) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, dst_off) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, len) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!("call void @rt_array_copy(ptr {src}, i32 {src_off}, ptr {dst}, i32 {dst_off}, i32 {len})"));
                Some(("void".into(), String::new()))
            }
            "Array.Clear" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, idx) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, len) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_array_clear(ptr {arr}, i32 {idx}, i32 {len})"
                ));
                Some(("void".into(), String::new()))
            }
            "Array.Reverse" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_array_reverse(ptr {arr})"));
                Some(("void".into(), String::new()))
            }
            "Array.IndexOf" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_index_of_int(ptr {arr}, i32 {val})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.LastIndexOf" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_last_index_of_int(ptr {arr}, i32 {val})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.Resize" => {
                let (_, slot) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, new_size) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstInt(0)));
                self.emit(&format!(
                    "call void @rt_array_resize(ptr {slot}, i32 {new_size})"
                ));
                Some(("void".into(), String::new()))
            }
            "Array.Empty" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_array_create(i32 0, i32 4)"));
                Some(("ptr".into(), tmp))
            }
            "Array.Exists" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_array_exists(ptr {arr}, ptr {pred})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                Some(("i1".into(), tmp))
            }
            "Array.Find" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_find_int(ptr {arr}, ptr {pred})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.FindLast" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_find_last_int(ptr {arr}, ptr {pred})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.FindIndex" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_find_index(ptr {arr}, ptr {pred})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.FindLastIndex" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_find_last_index(ptr {arr}, ptr {pred})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.TrueForAll" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_array_true_for_all(ptr {arr}, ptr {pred})"
                ));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = icmp ne i32 {raw}, 0"));
                Some(("i1".into(), tmp))
            }
            "Array.ForEach" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, action) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!(
                    "call void @rt_array_for_each(ptr {arr}, ptr {action})"
                ));
                Some(("void".into(), String::new()))
            }
            "Array.Sort" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                self.emit(&format!("call void @rt_array_sort_int(ptr {arr})"));
                Some(("void".into(), String::new()))
            }
            "Array.BinarySearch" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, val) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_array_binary_search_int(ptr {arr}, i32 {val})"
                ));
                Some(("i32".into(), tmp))
            }
            "Array.FindAll" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, pred) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_array_find_all_int(ptr {arr}, ptr {pred})"
                ));
                Some(("ptr".into(), tmp))
            }
            "Array.ConvertAll" => {
                let (_, arr) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, conv) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_array_convert_all_int(ptr {arr}, ptr {conv})"
                ));
                Some(("ptr".into(), tmp))
            }

            // ── Math / Vector (delegate to per-method handlers) ──
            func if func.starts_with("Math.") => {
                let method = func.strip_prefix("Math.").unwrap();
                self.try_emit_math_call(method, args)
            }
            func if func.starts_with("Vector.") => {
                let method = func.strip_prefix("Vector.").unwrap();
                self.try_emit_vector_call(method, args, expected)
            }

            // ── FileStream 静态工厂 ──
            func if func.starts_with("FileStream.") => {
                let method = func.strip_prefix("FileStream.").unwrap();
                let mode = match method {
                    "OpenRead" => 0,
                    "OpenWrite" => 1,
                    "Create" => 2,
                    _ => return None,
                };
                let path = args
                    .first()
                    .cloned()
                    .unwrap_or(MirOperand::ConstString(String::new()));
                Some(self.emit_new("FileStream", &[path, MirOperand::ConstInt(mode)], &[]))
            }

            // ── File/Directory/Path batch I/O ──
            func if func.starts_with("File.")
                || func.starts_with("Directory.")
                || func.starts_with("Path.") =>
            {
                let (rt, m) = func.split_once('.').unwrap();
                self.try_emit_io_static(rt, m, args)
            }

            // ── L3 Orm SQLite MVP ──
            func if func.starts_with("SqliteDb.") => {
                let method = func.strip_prefix("SqliteDb.").unwrap();
                self.try_emit_sqlite_static(method, args)
            }

            // ── RFC 029 M1 图像编解码：ImageNative.* → rt_image_* ──
            // RFC 037 §10 AL-P0：PngNative.* 复用同一分派（渲染域 PNG 直出 facade，
            // 见 builtin_facade.rs；facade 类须为纯 stub——PngEncoder 含真实逻辑不能入清单）。
            func if func.starts_with("ImageNative.") || func.starts_with("PngNative.") => {
                let method = func
                    .strip_prefix("ImageNative.")
                    .or_else(|| func.strip_prefix("PngNative."))
                    .unwrap();
                self.try_emit_image_native_static(method, args)
            }

            // ── RFC 029 M2 二维码生成：QrCodeNative.* → rt_qrcode_* ──
            func if func.starts_with("QrCodeNative.") => {
                let method = func.strip_prefix("QrCodeNative.").unwrap();
                self.try_emit_qrcode_native_static(method, args)
            }

            // ── RFC 029 M4 条形码解码：BarcodeNative.* → rt_barcode_* ──
            func if func.starts_with("BarcodeNative.") => {
                let method = func.strip_prefix("BarcodeNative.").unwrap();
                self.try_emit_barcode_native_static(method, args)
            }

            // ── RFC 029 M6 字体：Font::_* → rt_image_font_*（regular class 私有
            // 静态 [Builtin]，`::` 分隔；AesGcm 先例）──
            "Font::_Load" | "Font::_Metrics" | "Font::_Measure" | "Font::_Glyph"
            | "Font::_Free" => {
                let method = func.strip_prefix("Font::_").unwrap();
                self.try_emit_font_native_static(method, args)
            }

            // ── RFC 042: P2P static methods ──
            // RFC 026 M3 P0-1: PeerKey is now an AesGcm-pattern regular class —
            // fake `PeerKey.` arms removed; real private static ABI lives in the
            // `PeerKey::_Xxx` arms below.
            // RFC 042 P0-2: Noise fake facade arms (`SecureSession.Create` /
            // `NoiseTransport.Initiate|Respond`) removed the same way —
            // byte[] ABI lives in the `::_XxxArr` arms below.
            "SecureSession::_CreateArr" => {
                let (_, sk) = self.emit_operand(&args.first().cloned()?);
                let (_, pk) = self.emit_operand(&args.get(1).cloned()?);
                let (_, init) = self.emit_operand(&args.get(2).cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_session_create_arr(ptr {sk}, ptr {pk}, i32 {init})"
                ));
                Some(("ptr".into(), tmp))
            }
            "NoiseTransport::_InitiateArr" => {
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_initiate_handshake_arr(ptr {handle})"
                ));
                Some(("ptr".into(), tmp))
            }
            "NoiseTransport::_RespondArr" => {
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let (_, inmsg) = self.emit_operand(&args.get(1).cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_respond_handshake_arr(ptr {handle}, ptr {inmsg})"
                ));
                Some(("ptr".into(), tmp))
            }
            "NoiseTransport::_FinalizeArr" => {
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let (_, inmsg) = self.emit_operand(&args.get(1).cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_initiate_finalize_arr(ptr {handle}, ptr {inmsg})"
                ));
                Some(("ptr".into(), tmp))
            }
            "NoiseTransport::_RespondFinalizeArr" => {
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let (_, inmsg) = self.emit_operand(&args.get(1).cloned()?);
                let raw = self.fresh_temp();
                self.emit(&format!(
                    "{raw} = call i32 @rt_noise_respond_finalize_arr(ptr {handle}, ptr {inmsg})"
                ));
                let flag = self.fresh_temp();
                self.emit(&format!("{flag} = icmp ne i32 {raw}, 0"));
                Some(("i1".into(), flag))
            }
            "NoiseTransport::_HandshakeHashArr" => {
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_session_handshake_hash_arr(ptr {handle})"
                ));
                Some(("ptr".into(), tmp))
            }

            // ── RFC 026 M1: S0 原语 facade 静态私有 ABI（regular class → `Class::Method`）──
            // `[Builtin]` 静态方法经 `user_type_static_method_func` 降级为 `Class::Method`
            // （`::` 分隔），与 facade 类（`.` 分隔）不同——此处显式匹配 `::` 形态。
            "AesGcm::_GenerateKey" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_crypto_aesgcm_new_key()"));
                Some(("ptr".into(), tmp))
            }
            // RFC 042 / P0-1: PeerKey Ed25519 private static ABI (AesGcm pattern).
            // keygen_arr → byte[64] = seed‖pk; null on CSPRNG failure (Arc side
            // propagates null).
            "PeerKey::_KeygenArr" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr @rt_crypto_ed25519_keygen_arr()"));
                Some(("ptr".into(), tmp))
            }
            "PeerKey::_SeedKeygenArr" => {
                let (_, seed) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_crypto_ed25519_seed_keygen_arr(ptr {seed})"
                ));
                Some(("ptr".into(), tmp))
            }
            "Rsa::_Keygen" => {
                let (_, bits) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(2048)));
                let p = self.fresh_temp();
                self.emit(&format!("{p} = call ptr @rt_crypto_rsa_keygen(i32 {bits})"));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }
            "Rsa::_ImportSpki" => {
                let (_, der) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_crypto_rsa_spki_import(ptr {der})"
                ));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }
            "ECDiffieHellman::_Keygen" => {
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_crypto_x25519_keygen()"));
                Some(("i32".into(), tmp))
            }
            "ECDiffieHellman::_ImportPrivate" => {
                let (_, priv_key) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call i32 @rt_crypto_x25519_import_private(ptr {priv_key})"
                ));
                Some(("i32".into(), tmp))
            }
            // RFC 026 M3: X509Certificate2 静态私有 ABI（PEM/DER → opaque 句柄）。
            "X509Certificate2::_ParseDer" => {
                let (_, der) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_crypto_x509_parse_der(ptr {der})"
                ));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }
            "X509Certificate2::_ParsePem" => {
                let (_, pem) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_crypto_x509_parse_pem(ptr {pem})"
                ));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }
            // 证书验签闭环（RFC 026 §1.2 ④）：leaf 由 trust 信任锚签发？
            // 两参均为 opaque 句柄（long）；经 `rt_crypto_x509_verify` 校验链。
            "X509Certificate2::_Verify" => {
                let leaf = args.first().cloned().unwrap_or(MirOperand::ConstNull);
                let trust = args.get(1).cloned().unwrap_or(MirOperand::ConstNull);
                let (_, leaf_v) = self.emit_operand(&leaf);
                let (_, trust_v) = self.emit_operand(&trust);
                let lp = self.fresh_temp();
                self.emit(&format!("{lp} = inttoptr i64 {leaf_v} to ptr"));
                let tp = self.fresh_temp();
                self.emit(&format!("{tp} = inttoptr i64 {trust_v} to ptr"));
                let r = self.fresh_temp();
                self.emit(&format!(
                    "{r} = call i32 @rt_crypto_x509_verify(ptr {lp}, ptr {tp})"
                ));
                Some(("i32".into(), r))
            }
            // RFC 026 M3: TlsClientSession 会话创建（server_name 字符串 + trust DER
            // byte[] + ALPN blob byte[] → opaque 会话句柄 long）。
            "TlsClientSession::_ClientNew" => {
                let (_, server_name) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let (_, trust_der) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, alpn_blob) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_crypto_tls_client_new(ptr {server_name}, ptr {trust_der}, ptr {alpn_blob})"
                ));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }
            // RFC 026 S5: TlsServerSession 公开服务端会话创建（证书 DER + 私钥 DER +
            // ALPN blob + flags int + client CA blob → opaque 会话句柄 long）。
            // flags：0x1 = tickets；0x2 = 客户端证书 VERIFY_REQUIRED；0x4 = 早数据。
            "TlsServerSession::_ServerNewEx" => {
                let (_, cert_der) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, key_der) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, alpn_blob) =
                    self.emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                let (_, flags) =
                    self.emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                let (_, ca_blob) =
                    self.emit_operand(&args.get(4).cloned().unwrap_or(MirOperand::ConstNull));
                let p = self.fresh_temp();
                self.emit(&format!(
                    "{p} = call ptr @rt_crypto_tls_server_new_ex(ptr {cert_der}, ptr {key_der}, ptr {alpn_blob}, i32 {flags}, ptr {ca_blob})"
                ));
                let t = self.fresh_temp();
                self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                Some(("i64".into(), t))
            }

            // ── RFC 026 M3 P0-1：Security 哈希/HMAC/CSPRNG 静态私有 ABI
            //（byte[] → byte[]；失败 NULL，公开体转为 CryptographicException）──
            "MD5::_ComputeHash"
            | "SHA1::_ComputeHash"
            | "SHA256::_ComputeHash"
            | "SHA384::_ComputeHash"
            | "SHA512::_ComputeHash"
            | "SHA3_256::_ComputeHash"
            | "SHA3_512::_ComputeHash" => {
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let abi = match func {
                    "MD5::_ComputeHash" => "@rt_crypto_md5_arr",
                    "SHA1::_ComputeHash" => "@rt_crypto_sha1_arr",
                    "SHA256::_ComputeHash" => "@rt_crypto_sha256_arr",
                    "SHA384::_ComputeHash" => "@rt_crypto_sha384_arr",
                    "SHA512::_ComputeHash" => "@rt_crypto_sha512_arr",
                    "SHA3_256::_ComputeHash" => "@rt_crypto_sha3_256_arr",
                    "SHA3_512::_ComputeHash" => "@rt_crypto_sha3_512_arr",
                    _ => return None,
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }
            "HMACSHA256::_ComputeHash"
            | "HMACSHA384::_ComputeHash"
            | "HMACSHA512::_ComputeHash" => {
                let (_, key) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let (_, msg) =
                    self.emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                let abi = match func {
                    "HMACSHA256::_ComputeHash" => "@rt_crypto_hmac_sha256_arr",
                    "HMACSHA384::_ComputeHash" => "@rt_crypto_hmac_sha384_arr",
                    "HMACSHA512::_ComputeHash" => "@rt_crypto_hmac_sha512_arr",
                    _ => return None,
                };
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {key}, ptr {msg})"));
                Some(("ptr".into(), tmp))
            }
            "CSPRNG::_GetBytes" => {
                let (_, count) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstInt(0)));
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_crypto_random_bytes_arr(i32 {count})"
                ));
                Some(("ptr".into(), tmp))
            }

            // ── RFC 016 子项 M1：RefCount.GetRefCount(obj) 诊断 ──
            // 读取对象 ARC 引用计数（`rt_arc_count`）。纯诊断只读接口、非热路径；
            // 供 faulted Task 异常所有权等场景的确定性泄漏检测在 Arc 源码断言 rc。
            // func 以 `::` 分隔（如 `RefCount::GetRefCount`），兼容点号形式。
            "RefCount::GetRefCount" | "RefCount.GetRefCount" => {
                let (_, obj) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_arc_count(ptr {obj})"));
                Some(("i32".into(), tmp))
            }

            _ => None,
        }
    }

    // ---- Instance method-form dispatch ----

    /// Try to handle `receiver.Method(...)` via facade classify + handlers.
    pub(super) fn try_emit_builtin_method(
        &mut self,
        receiver: &MirOperand,
        method: &str,
        args: &[MirOperand],
        receiver_type: &str,
        expected: &TypeId,
    ) -> Option<TyVal> {
        // 非 facade 清单内的类型仍字符串匹配。
        // 单一惯用原则：TypeInfo 已合并入 Type，仅剩 Type / RuntimeType。
        // M3+：RuntimeMethodInfo / RuntimeFieldInfo / RuntimePropertyInfo 的 Name 等从 Rt*Info 拦截。
        if matches!(receiver_type, "RuntimeType" | "Type") {
            return self.try_emit_runtime_typeinfo_getter(receiver, method);
        }
        if matches!(receiver_type, "RuntimeMethodInfo" | "MethodInfo") {
            return self.try_emit_runtime_methodinfo_getter(receiver, method);
        }
        if matches!(receiver_type, "RuntimeFieldInfo" | "FieldInfo") {
            return self.try_emit_runtime_fieldinfo_getter(receiver, method);
        }
        if matches!(receiver_type, "RuntimePropertyInfo" | "PropertyInfo") {
            return self.try_emit_runtime_propertyinfo_getter(receiver, method);
        }
        if receiver_type == "Environment" {
            return self.try_emit_environment_static(method, args);
        }
        // FileStream 实例方法（非 facade——静态工厂保留真实 Arc 体）。
        if receiver_type == "FileStream" {
            return self.try_emit_file_stream_method(receiver, method, args);
        }
        // RFC 008 AsyncStream：TaskCompletionSource<T>（泛型实例 mangled
        // "TaskCompletionSource_<T>"）——实例成员经 try_emit_tcs_method 直射
        // rt_task_* ABI（对象即 RtTask*，同 CTS facade 拦截模式）。
        if receiver_type.starts_with("TaskCompletionSource") {
            return self.try_emit_tcs_method(receiver, method, args);
        }
        // RFC 026 M1/M3/S5 + RFC 042 P0-2: S0 原语 facade 实例方法（AesGcm / Rsa /
        // ECDiffieHellman / X509Certificate2 / TlsClientSession / TlsServerSession /
        // PeerKey / SecureSession——regular class，`[Builtin]` 实例方法经 codegen
        // 拦截直射 vendored ABI）。
        if matches!(
            receiver_type,
            "AesGcm"
                | "Rsa"
                | "ECDiffieHellman"
                | "X509Certificate2"
                | "TlsClientSession"
                | "TlsServerSession"
                | "PeerKey"
                | "SecureSession"
        ) {
            return self.try_emit_s0_crypto_method(receiver, receiver_type, method, args);
        }

        match classify_builtin_facade(receiver_type) {
            Some(BuiltinFacadeKind::Math) => self.try_emit_math_call(method, args),
            Some(BuiltinFacadeKind::Vector) => self.try_emit_vector_call(method, args, expected),
            Some(BuiltinFacadeKind::Console) => self.try_emit_console_static(method, args),
            Some(BuiltinFacadeKind::Environment) => self.try_emit_environment_static(method, args),
            Some(
                BuiltinFacadeKind::File | BuiltinFacadeKind::Directory | BuiltinFacadeKind::Path,
            ) => self.try_emit_io_static(receiver_type, method, args),
            Some(BuiltinFacadeKind::WindowHost) => {
                let func_full = format!("WindowHost.{method}");
                self.try_emit_window_host_element(&func_full, args)
            }
            Some(BuiltinFacadeKind::Window) => {
                match method {
                    "Close" => {
                        // `rt_window_close` C ABI 期望**平台窗口镜像句柄**（`RtWindowImpl*`，
                        // Arc 侧 `long`），不是 Arc `this` 对象指针——把 Arc 对象指针直接传给
                        // C 会把对象头内存当 `RtWindowImpl` 解引用（调用即崩）。
                        // 句柄由 `FramePump` 在 `Show`/`ShowAsync` 流程 `CreateWindow` 成功后
                        // 经 `FocusManager.SetWindowHandle` 回填到 `@__static_FocusManager__windowHandle`
                        // 静态槽（非 facade 内部类，静态槽可靠发射；单窗口 M3 状态与
                        // `InvalidateActiveWindow` 共用「活动窗口句柄」语义）。此处 load 静态槽 →
                        // inttoptr → 直射 `rt_window_close`。未 Show 时句柄为 0，C 侧
                        // `if (!window) return` 为安全 no-op（Close 语义需先 Show）。
                        let handle = self.fresh_temp();
                        self.emit(&format!(
                            "{handle} = load i64, ptr @__static_FocusManager__windowHandle"
                        ));
                        let hp = self.fresh_temp();
                        self.emit(&format!("{hp} = inttoptr i64 {handle} to ptr"));
                        self.emit(&format!("call void @rt_window_close(ptr {hp})"));
                        Some(("void".into(), String::new()))
                    }
                    _ => None,
                }
            }
            Some(BuiltinFacadeKind::Task) => self
                .try_emit_task_static(method, args, expected)
                .or_else(|| self.try_emit_task_method(receiver, method, args, expected)),
            Some(BuiltinFacadeKind::StdDefensive) if receiver_type == "CancellationTokenSource" => {
                self.try_emit_cts_method(receiver, method, args)
            }
            Some(BuiltinFacadeKind::StdDefensive) if receiver_type == "CancellationToken" => {
                self.try_emit_ct_method(receiver, method, args)
            }
            Some(BuiltinFacadeKind::Net)
                if matches!(
                    receiver_type,
                    "Socket" | "TcpClient" | "TcpListener" | "UdpClient"
                ) =>
            {
                self.try_emit_socket_method(receiver, method, args, receiver_type)
            }
            // RFC 048: 命名管道门面（本机 IPC · rt_pipe_* 同步面）。
            Some(BuiltinFacadeKind::Pipe)
                if matches!(receiver_type, "NamedPipeServerStream" | "NamedPipeClientStream") =>
            {
                self.try_emit_pipe_method(receiver, method, args)
            }
            Some(BuiltinFacadeKind::Thread) => self.try_emit_thread_method(receiver, method),
            Some(BuiltinFacadeKind::Mutex) => self.try_emit_mutex_method(receiver, method),
            Some(BuiltinFacadeKind::Semaphore) => {
                self.try_emit_semaphore_method(receiver, method, args)
            }
            Some(BuiltinFacadeKind::ThreadPoolScheduler) => {
                self.try_emit_threadpool_method(receiver, method, args)
            }
            Some(BuiltinFacadeKind::Interlocked) => self.try_emit_interlocked_static(method, args),
            Some(BuiltinFacadeKind::Base64 | BuiltinFacadeKind::Hex | BuiltinFacadeKind::Url)
                if matches!(method, "Encode" | "Decode") =>
            {
                let abi = match (receiver_type, method) {
                    ("Base64", "Encode") => "@rt_text_base64_encode",
                    ("Base64", "Decode") => "@rt_text_base64_decode",
                    ("Hex", "Encode") => "@rt_text_hex_encode",
                    ("Hex", "Decode") => "@rt_text_hex_decode",
                    ("Url", "Encode") => "@rt_text_url_encode",
                    ("Url", "Decode") => "@rt_text_url_decode",
                    _ => return None,
                };
                let (_, arg) = self.emit_operand(
                    &args
                        .first()
                        .cloned()
                        .unwrap_or(MirOperand::ConstString(String::new())),
                );
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }
            Some(BuiltinFacadeKind::Encoding)
                if matches!(method, "GetBytes" | "GetString" | "GetByteCount") =>
            {
                if method == "GetByteCount" {
                    let (_, arg) =
                        self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                    let tmp = self.fresh_temp();
                    self.emit(&format!(
                        "{tmp} = call i32 @rt_text_utf8_get_byte_count(ptr {arg})"
                    ));
                    return Some(("i32".into(), tmp));
                }
                let abi = match method {
                    "GetBytes" => "@rt_text_utf8_get_bytes",
                    "GetString" => "@rt_text_utf8_get_string",
                    _ => return None,
                };
                let (_, arg) =
                    self.emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call ptr {abi}(ptr {arg})"));
                Some(("ptr".into(), tmp))
            }
            // ── RFC 042: P2P facade builtins ──
            Some(BuiltinFacadeKind::P2P) => self.try_emit_p2p_method(receiver_type, method, args),
            Some(BuiltinFacadeKind::Sqlite) => self.try_emit_sqlite_static(method, args),
            _ => None,
        }
    }

    /// RFC 042: P2P facade method dispatch.
    /// PeerKey / NoiseTransport / SecureSession fake arms removed (P0-1/P0-2) —
    /// they are AesGcm-pattern regular classes now; their private `[Builtin]`
    /// ABI lives in the `Class::_Xxx` static arms and the try_emit_s0_crypto_method
    /// receiver arms. Remaining P2P facades have no arms here (falls to None).
    fn try_emit_p2p_method(
        &mut self,
        class_name: &str,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        match (class_name, method) {
            // RFC 026 M3 P0-1: PeerKey fake arms removed — PeerKey is an
            // AesGcm-pattern regular class now; see `PeerKey::_Xxx` static arms
            // and the "PeerKey" receiver arm of try_emit_s0_crypto_method.
            // ── NoiseSession + SecureSession: Noise transport ──
            ("SecureSession", "Create") => {
                // static SecureSession Create(localSk, remotePk, initiator)
                let (_, sk) = self.emit_operand(&args.first().cloned()?);
                let (_, pk) = self.emit_operand(&args.get(1).cloned()?);
                let (_, init) = self.emit_operand(&args.get(2).cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!(
                    "{tmp} = call ptr @rt_noise_session_create(ptr {sk}, ptr {pk}, i32 {init})"
                ));
                Some(("ptr".into(), tmp))
            }
            ("NoiseTransport", "Initiate") => {
                // static Initiate(handle) → msg1 = e.pub(32) + 空 payload AEAD tag(16) = 48 bytes
                // （rt_noise.c spec 合规实现：msg1 含空 payload 的 16B tag，与官方向量一致）。
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let tmp_buf = self.fresh_temp();
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp_buf} = alloca i8, i32 48"));
                self.emit(&format!("{tmp} = call i32 @rt_noise_initiate_handshake(ptr {handle}, ptr {tmp_buf}, i32 48)"));
                Some(("ptr".into(), tmp_buf))
            }
            ("NoiseTransport", "Respond") => {
                // static Respond(handle, inMsg, inLen) → outMsg(48 bytes)
                let (_, handle) = self.emit_operand(&args.first().cloned()?);
                let (_, inmsg) = self.emit_operand(&args.get(1).cloned()?);
                let (_, inlen) = self.emit_operand(&args.get(2).cloned()?);
                let tmp_buf = self.fresh_temp();
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp_buf} = alloca i8, i32 48"));
                self.emit(&format!("{tmp} = call i32 @rt_noise_respond_handshake(ptr {handle}, ptr {inmsg}, i32 {inlen}, ptr {tmp_buf}, i32 48)"));
                Some(("ptr".into(), tmp_buf))
            }
            ("SecureSession", "Encrypt") => {
                let (_, pt) = self.emit_operand(&args.first().cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_noise_session_encrypt(ptr null, ptr {pt}, i32 0, ptr null, ptr null)"));
                Some(("i32".into(), tmp))
            }
            ("SecureSession", "Decrypt") => {
                let (_, ct) = self.emit_operand(&args.first().cloned()?);
                let tmp = self.fresh_temp();
                self.emit(&format!("{tmp} = call i32 @rt_noise_session_decrypt(ptr null, ptr {ct}, i32 0, ptr null, ptr null)"));
                Some(("i32".into(), tmp))
            }
            _ => None,
        }
    }

    /// RFC 026 M1: S0 TLS 1.3 原语 facade 实例方法拦截。
    ///
    /// regular class（非 stub facade）——字段布局对齐 FileStream 先例：
    /// 唯一字段位于对象头后 offset 16。各实例方法加载句柄/密钥后直射
    /// vendored `crypto_native.dll` 的 `rt_crypto_*` ABI。
    fn try_emit_s0_crypto_method(
        &mut self,
        receiver: &MirOperand,
        receiver_type: &str,
        method: &str,
        args: &[MirOperand],
    ) -> Option<TyVal> {
        let (_, recv) = self.emit_operand(receiver);
        let field_addr = self.fresh_temp();
        self.emit(&format!(
            "{field_addr} = getelementptr inbounds i8, ptr {recv}, i32 16"
        ));

        match receiver_type {
            // ── AesGcm：`_key`（byte[]，offset 16）──
            "AesGcm" => {
                let key = self.fresh_temp();
                self.emit(&format!("{key} = load ptr, ptr {field_addr}"));
                match method {
                    "_Encrypt" => {
                        let (_, nonce) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, plain) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_aesgcm_encrypt(ptr {key}, ptr {nonce}, ptr {plain})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "_Decrypt" => {
                        let (_, nonce) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, ct) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, tag) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_aesgcm_decrypt(ptr {key}, ptr {nonce}, ptr {ct}, ptr {tag})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    _ => None,
                }
            }
            // ── Rsa：`_handle`（long，offset 16 → inttoptr）──
            "Rsa" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load i64, ptr {field_addr}"));
                let hp = self.fresh_temp();
                self.emit(&format!("{hp} = inttoptr i64 {handle} to ptr"));
                match method {
                    "ExportSubjectPublicKeyInfo" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_rsa_spki_export(ptr {hp})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "ExportPkcs8PrivateKey" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_rsa_pkcs8_export(ptr {hp})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "Sign" => {
                        let (_, data) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_rsa_sign_pss(ptr {hp}, ptr {data})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "Verify" => {
                        let (_, data) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, sig) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let raw = self.fresh_temp();
                        self.emit(&format!(
                            "{raw} = call i32 @rt_crypto_rsa_verify_pss(ptr {hp}, ptr {data}, ptr {sig})"
                        ));
                        let flag = self.fresh_temp();
                        self.emit(&format!("{flag} = icmp eq i32 {raw}, 0"));
                        Some(("i1".into(), flag))
                    }
                    _ => None,
                }
            }
            // ── ECDiffieHellman：`_handle`（int = PSA key id，offset 16）──
            "ECDiffieHellman" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load i32, ptr {field_addr}"));
                match method {
                    "get_PublicKey" | "PublicKey" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_x25519_pubkey(i32 {handle})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "DeriveKeyMaterial" => {
                        let (_, other) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_x25519_derive(i32 {handle}, ptr {other})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    _ => None,
                }
            }
            // ── X509Certificate2：`_handle`（long，offset 16 → inttoptr）──
            "X509Certificate2" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load i64, ptr {field_addr}"));
                let hp = self.fresh_temp();
                self.emit(&format!("{hp} = inttoptr i64 {handle} to ptr"));
                match method {
                    // _LoadSubject：ABI 返回 malloc'd C 字符串 → 直接作为 Arc string
                    //（Arc `string` 即 UTF-8 C 字符串，同 rt_crypto_sha256 惯例）。
                    // RFC 026 M3 P0-1: Arc 侧构造时缓存（Subject 改普通属性读字段），
                    // 避免每次属性访问泄漏一份 malloc'd 字符串。
                    "_LoadSubject" => {
                        let s = self.fresh_temp();
                        self.emit(&format!("{s} = call ptr @rt_crypto_x509_subject(ptr {hp})"));
                        Some(("ptr".into(), s))
                    }
                    // RFC 026 M3 P0-1: 释放 mbedTLS 句柄（幂等：NULL 直接返回）。
                    "_Dispose" => {
                        self.emit(&format!("call void @rt_crypto_x509_free(ptr {hp})"));
                        Some(("void".into(), String::new()))
                    }
                    // _GetPublicKeyHandle：返回 RSA opaque 句柄（long）。
                    "_GetPublicKeyHandle" => {
                        let p = self.fresh_temp();
                        self.emit(&format!("{p} = call ptr @rt_crypto_x509_pubkey(ptr {hp})"));
                        let t = self.fresh_temp();
                        self.emit(&format!("{t} = ptrtoint ptr {p} to i64"));
                        Some(("i64".into(), t))
                    }
                    _ => None,
                }
            }
            // ── TlsServerSession：`_handle`（long，offset 16 → inttoptr）。
            // RFC 026 S5：公开服务端 facade（rt_crypto_tls_server_new 测试 harness
            // 面提升）；实例方法与 TlsClientSession 同栈复用（握手/读写/ALPN/释放）
            // + `_Drain`（flush post-handshake 消息如 NewSessionTicket）+ `_ReadEarlyData`
            // （0-RTT 早数据接收）。`_ServerNewEx` 为静态（见 try_emit_builtin_static）。
            "TlsServerSession" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load i64, ptr {field_addr}"));
                let hp = self.fresh_temp();
                self.emit(&format!("{hp} = inttoptr i64 {handle} to ptr"));
                match method {
                    "_Handshake" => {
                        let (_, recv) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let state = self.emit_native_byref_arg(
                            &args.get(1).cloned().unwrap_or(MirOperand::ConstNull),
                            "i32",
                        );
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_handshake(ptr {hp}, ptr {recv}, {state})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "_Write" => {
                        let (_, plain) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_write(ptr {hp}, ptr {plain})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "_Read" => {
                        let (_, enc) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, buffer) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, offset) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let (_, count) = self
                            .emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_read(ptr {hp}, ptr {enc}, ptr {buffer}, i32 {offset}, i32 {count})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    "_Alpn" => {
                        let s = self.fresh_temp();
                        self.emit(&format!("{s} = call ptr @rt_crypto_tls_alpn(ptr {hp})"));
                        Some(("ptr".into(), s))
                    }
                    "_Drain" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!("{tmp} = call ptr @rt_crypto_tls_drain(ptr {hp})"));
                        Some(("ptr".into(), tmp))
                    }
                    "_ReadEarlyData" => {
                        let (_, enc) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, buffer) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, offset) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let (_, count) = self
                            .emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_read_early_data(ptr {hp}, ptr {enc}, ptr {buffer}, i32 {offset}, i32 {count})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    "_Free" => {
                        self.emit(&format!("call void @rt_crypto_tls_free(ptr {hp})"));
                        Some(("void".into(), String::new()))
                    }
                    _ => None,
                }
            }
            // ── TlsClientSession：`_handle`（long，offset 16 → inttoptr）──
            "TlsClientSession" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load i64, ptr {field_addr}"));
                let hp = self.fresh_temp();
                self.emit(&format!("{hp} = inttoptr i64 {handle} to ptr"));
                match method {
                    // 非阻塞握手：喂入 recv byte[] → send_out byte[]；state 经 out 参数。
                    "_Handshake" => {
                        let (_, recv) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let state = self.emit_native_byref_arg(
                            &args.get(1).cloned().unwrap_or(MirOperand::ConstNull),
                            "i32",
                        );
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_handshake(ptr {hp}, ptr {recv}, {state})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    // ── RFC 026 S5：显式校验策略 / CRL / 会话恢复 / 0-RTT / 客户端证书 ──
                    // 校验策略：mode（0=None / 1=Anchor DER / 2=FullChain PEM）+ 信任链 blob。
                    "_SetVerify" => {
                        let (_, mode) = self.emit_operand(
                            &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        );
                        let (_, blob) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_set_verify(ptr {hp}, i32 {mode}, ptr {blob})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // CRL（吊销 · 最小面）：DER CRL → 挂到 CA 链校验路径。
                    "_SetCrl" => {
                        let (_, crl_der) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_set_crl(ptr {hp}, ptr {crl_der})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 握手后校验结果（i32 位标志；0 = 通过）。
                    "_VerifyResult" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_verify_result(ptr {hp})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 客户端证书（双向认证）：DER 证书 + DER 私钥。
                    "_SetClientCert" => {
                        let (_, cert_der) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, key_der) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_set_client_cert(ptr {hp}, ptr {cert_der}, ptr {key_der})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 会话序列化保存（握手完成后）→ 字节（含内部 0x00）。
                    "_SessionSave" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_session_save(ptr {hp})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    // 会话载入（握手前恢复）。
                    "_SessionLoad" => {
                        let (_, bytes) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_session_load(ptr {hp}, ptr {bytes})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 0-RTT 早数据启用（握手前）。
                    "_EnableEarlyData" => {
                        let (_, enabled) = self.emit_operand(
                            &args.first().cloned().unwrap_or(MirOperand::ConstInt(0)),
                        );
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_enable_early_data(ptr {hp}, i32 {enabled})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 0-RTT 早数据写（握手期间）：喂 recv + plain → 密文字节；
                    // state（1=写出 / 0=需更多输入 / -1=无法写·退正常握手 / -2=硬错误）。
                    "_WriteEarlyData" => {
                        let (_, recv) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, plain) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let state = self.emit_native_byref_arg(
                            &args.get(2).cloned().unwrap_or(MirOperand::ConstNull),
                            "i32",
                        );
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_write_early_data(ptr {hp}, ptr {recv}, ptr {plain}, {state})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    // 早数据状态（握手完成后）：0=未指示 / 1=ACCEPTED / 2=REJECTED。
                    "_EarlyDataStatus" => {
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_early_data_status(ptr {hp})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 明文写 → 密文字节（byte[]；内部 0x00 不截断）。
                    "_Write" => {
                        let (_, plain) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_tls_write(ptr {hp}, ptr {plain})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    // 密文读 → 明文写入 buffer[offset..]；返回实际字节数。
                    "_Read" => {
                        let (_, enc) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, buffer) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, offset) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let (_, count) = self
                            .emit_operand(&args.get(3).cloned().unwrap_or(MirOperand::ConstInt(0)));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call i32 @rt_crypto_tls_read(ptr {hp}, ptr {enc}, ptr {buffer}, i32 {offset}, i32 {count})"
                        ));
                        Some(("i32".into(), tmp))
                    }
                    // 协商出的 ALPN 协议（C 字符串 → Arc string）。
                    "_Alpn" => {
                        let s = self.fresh_temp();
                        self.emit(&format!("{s} = call ptr @rt_crypto_tls_alpn(ptr {hp})"));
                        Some(("ptr".into(), s))
                    }
                    // 释放会话句柄。
                    "_Free" => {
                        self.emit(&format!("call void @rt_crypto_tls_free(ptr {hp})"));
                        Some(("void".into(), String::new()))
                    }
                    _ => None,
                }
            }
            // ── PeerKey: no field load — sk/pk are passed in as arguments by the
            // Arc real-body (existing S0 classes are single-field @16; PeerKey has
            // two fields, so avoid the offset-24 hardcode). C 侧 `rt_array_length`
            // 读 RtArray header，而 Arc string 是裸 char*——msg 必须先经
            // `rt_text_utf8_get_bytes` 转 byte[]（null → 空 byte[0]）。转换产物是
            // 纯临时（仅本次调用可见，不逃逸给 Arc），数组非 ARC 对象故须显式
            // `rt_array_destroy`（null-safe）防逐调用泄漏；调用后销毁安全——
            // sign_arr/verify_arr 只在调用期间读取 msg。
            "PeerKey" => {
                match method {
                    "_SignArr" => {
                        let (_, msg) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, sk) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_crypto_ed25519_sign_arr(ptr {msg}, ptr {sk})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "_VerifyArr" => {
                        let (_, msg) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, sig) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, pk) = self
                            .emit_operand(&args.get(2).cloned().unwrap_or(MirOperand::ConstNull));
                        let raw = self.fresh_temp();
                        self.emit(&format!(
                            "{raw} = call i32 @rt_crypto_ed25519_verify_arr(ptr {msg}, ptr {sig}, ptr {pk})"
                        ));
                        // verify_arr → 1/0/-1: only 1 is true.
                        let flag = self.fresh_temp();
                        self.emit(&format!("{flag} = icmp eq i32 {raw}, 1"));
                        Some(("i1".into(), flag))
                    }
                    _ => None,
                }
            }
            // ── SecureSession：`_handle`（string 会话句柄，offset 16）──
            "SecureSession" => {
                let handle = self.fresh_temp();
                self.emit(&format!("{handle} = load ptr, ptr {field_addr}"));
                match method {
                    "_EncryptArr" => {
                        let (_, pt) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_noise_session_encrypt_arr(ptr {handle}, ptr {pt})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    "_DecryptArr" => {
                        let (_, ct) = self
                            .emit_operand(&args.first().cloned().unwrap_or(MirOperand::ConstNull));
                        let (_, tag) = self
                            .emit_operand(&args.get(1).cloned().unwrap_or(MirOperand::ConstNull));
                        let tmp = self.fresh_temp();
                        self.emit(&format!(
                            "{tmp} = call ptr @rt_noise_session_decrypt_arr(ptr {handle}, ptr {ct}, ptr {tag})"
                        ));
                        Some(("ptr".into(), tmp))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
