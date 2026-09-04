//! `arc-server` 进程入口（RFC 038 M0）。
//!
//! stdio 主循环：从 stdin 读取 JSON-RPC 消息 → 通过 [`MethodDispatcher`] 分发 →
//! 将响应写入 stdout → 收到 `exit` 通知后退出进程。
//!
//! ## LSP 退出码（[规范](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#exit)）
//!
//! - `0`：收到 `shutdown` 后再收到 `exit`——正常关闭
//! - `1`：未收到 `shutdown` 而收到 `exit`，或 stdin EOF 异常断开

use std::io::{self, Read, Write};
use std::process::ExitCode;

use arc_server::lsp::json_rpc::{JsonRpcError, StdioTransport};
use arc_server::lsp::method_dispatcher::{MethodDispatcher, ServerState};

fn main() -> ExitCode {
    // 初始化日志——`ARC_LOG=debug` 启用调试输出
    let _ = env_logger::Builder::from_env(env_logger::Env::default().filter_or("ARC_LOG", "info"))
        .format_timestamp_millis()
        .try_init();

    log::info!("arc-server starting (RFC 038 M0)");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let dispatcher = MethodDispatcher::new();

    match run_server(stdin.lock(), stdout.lock(), &dispatcher) {
        Ok(true) => {
            log::info!("arc-server exited cleanly (code 0)");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            log::warn!("arc-server exited without prior shutdown (code 1)");
            ExitCode::from(1)
        }
        Err(e) => {
            log::error!("arc-server fatal io error: {e}");
            ExitCode::from(1)
        }
    }
}

/// stdio 主循环——从 stdin 读消息、分发、写响应到 stdout。
///
/// 返回值：
/// - `Ok(true)`：收到 `exit` 通知且之前收到过 `shutdown`——正常关闭
/// - `Ok(false)`：stdin EOF 或收到 `exit` 但之前未 `shutdown`——异常关闭
/// - `Err`：底层 IO 错误（如 stdout 写入失败）
///
/// 分离为独立函数便于单元测试——可传入模拟 reader/writer。
fn run_server<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    dispatcher: &MethodDispatcher,
) -> io::Result<bool> {
    loop {
        let message = match StdioTransport::read_message(&mut reader) {
            Ok(Some(m)) => m,
            Ok(None) => {
                // stdin EOF——客户端断开连接
                log::info!("stdin EOF — client disconnected");
                return Ok(false);
            }
            Err(JsonRpcError::Io(e)) => return Err(e),
            Err(e) => {
                // 消息解析失败——LSP 规范要求发 parse_error 响应（id 应为 null）
                // M0 简化：仅记录日志，不发响应（id 未知，无法正确关联请求）
                // TODO(M1+)：扩展 RequestId 支持 Null 变体以符合 JSON-RPC 规范
                log::warn!("message parse error (no response sent): {e}");
                continue;
            }
        };

        // 在 dispatch 前记录状态——用于判断 exit 是否在 shutdown 之后
        let state_before = dispatcher.state();

        if let Some(response) = dispatcher.dispatch(&message) {
            StdioTransport::write_message(&mut writer, &response).map_err(json_rpc_error_to_io)?;
        }

        // 检查是否收到 exit 通知——dispatcher 进入 Exited 状态
        if dispatcher.state() == ServerState::Exited {
            // exit 通知到达——若之前状态是 ShuttingDown 则正常关闭
            return Ok(state_before == ServerState::ShuttingDown);
        }
    }
}

/// 将 [`JsonRpcError`] 转换为 [`io::Error`]——IO 错误原样传递，
/// 其他错误（如序列化失败）包装为 `io::Error::other`。
fn json_rpc_error_to_io(e: JsonRpcError) -> io::Error {
    match e {
        JsonRpcError::Io(io_err) => io_err,
        other => io::Error::other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arc_server::lsp::json_rpc::{JsonRpcMessage, MessageKind, RequestId};
    use arc_server::lsp::method_dispatcher::{
        DEFINITION_METHOD, DIAGNOSTIC_METHOD, DID_CHANGE_METHOD, DID_OPEN_METHOD,
        DOCUMENT_SYMBOL_METHOD, EXIT_METHOD, FOLDING_RANGE_METHOD, HOVER_METHOD,
        INITIALIZED_METHOD, INITIALIZE_METHOD, REFERENCES_METHOD, SEMANTIC_TOKENS_METHOD,
        SHUTDOWN_METHOD, WORKSPACE_SYMBOL_METHOD,
    };
    use arcgr::{
        ArcgrFile, FileEntry, ReferenceContext, ReferenceEntry, ReferenceGraph, SymbolEntry,
        SymbolKind, TypeSig, Visibility,
    };
    use std::fs;
    use std::io::Cursor;

    /// 模拟完整的 LSP 生命周期：initialize → initialized → shutdown → exit
    #[test]
    fn full_lifecycle_completes_cleanly() {
        let dispatcher = MethodDispatcher::new();
        let mut input = Vec::<u8>::new();
        let messages = vec![
            JsonRpcMessage::request(
                RequestId::Number(1),
                INITIALIZE_METHOD,
                Some(serde_json::json!({"processId": null})),
            ),
            JsonRpcMessage::notification(INITIALIZED_METHOD, Some(serde_json::json!({}))),
            JsonRpcMessage::request(RequestId::Number(2), SHUTDOWN_METHOD, None),
            JsonRpcMessage::notification(EXIT_METHOD, None),
        ];
        for m in &messages {
            StdioTransport::write_message(&mut input, m).unwrap();
        }

        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(
            shutdown_received,
            "shutdown should have been received before exit"
        );
        assert_eq!(dispatcher.state(), ServerState::Exited);

        // 验证输出包含 initialize 和 shutdown 的响应（通知无响应）
        let mut cursor = Cursor::new(output);
        let mut responses = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            responses.push(m);
        }
        assert_eq!(
            responses.len(),
            2,
            "should have 2 responses (initialize + shutdown)"
        );
    }

    /// stdin EOF 未收到 shutdown——返回 false（异常关闭）
    #[test]
    fn eof_without_shutdown_returns_false() {
        let dispatcher = MethodDispatcher::new();
        let input = Vec::<u8>::new();
        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(
            !shutdown_received,
            "EOF without shutdown should return false"
        );
    }

    /// exit 通知但未先 shutdown——返回 false（异常关闭，LSP 规范要求退出码 1）
    #[test]
    fn exit_without_shutdown_returns_false() {
        let dispatcher = MethodDispatcher::new();
        let mut input = Vec::<u8>::new();
        StdioTransport::write_message(&mut input, &JsonRpcMessage::notification(EXIT_METHOD, None))
            .unwrap();

        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(
            !shutdown_received,
            "exit without prior shutdown should return false"
        );
        assert_eq!(dispatcher.state(), ServerState::Exited);
    }

    // ─── 端到端：真实 .arcgr + 完整 LSP 流程 ───

    /// 字节偏移 → (行, 列)。
    fn pos_of(src: &str, offset: usize) -> (u32, u32) {
        let line = src[..offset].bytes().filter(|&b| b == b'\n').count() as u32;
        let line_start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = (offset - line_start) as u32;
        (line, col)
    }

    /// 构造真实 `.arcgr` 二进制 + 源码落盘，走完整 LSP 流程并校验响应。
    ///
    /// 验证：initialize → definition / hover / references / documentSymbol →
    /// shutdown → exit 全链路，且四个语义 provider 消费磁盘上的真实索引。
    #[test]
    fn e2e_lsp_semantic_flow_with_real_arcgr() {
        // 1. 临时 workspace：写入源码与 `.arcgr`
        let dir = std::env::temp_dir().join(format!("arc-server-e2e-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src_path = dir.join("main.as");
        let arcgr_path = dir.join("main.arcgr");
        let src = "interface IFoo {\n  void Bar();\n}\nclass FooImpl : IFoo {\n  void Bar() { }\n}\nint Main() {\n  FooImpl f = new FooImpl();\n  f.Bar();\n  return 0;\n}\n";
        fs::write(&src_path, src).unwrap();
        let path_str = src_path.to_string_lossy().replace('\\', "/");

        // 2. 构造 `.arcgr`（真实二进制；symbol/reference span 与源码字节偏移一致）
        let mut file = ArcgrFile::new();
        let file_id = 0u32;
        let line_count = src.lines().count() as u32;
        file.file_table.entries.push(FileEntry::new(
            file_id,
            path_str.clone(),
            0xDEAD,
            line_count,
        ));

        let ifoo = src.find("IFoo").unwrap() as u32;
        let fooimpl = src.find("FooImpl").unwrap() as u32;
        let main = src.find("Main").unwrap() as u32;
        let bar_first = src.find("Bar").unwrap() as u32;
        let bar_second = src.match_indices("Bar").nth(1).unwrap().0 as u32;

        let mut push_sym = |id: u32, name: &str, kind: SymbolKind, sig: TypeSig, start: u32| {
            file.symbol_table.entries.push(SymbolEntry::new(
                id,
                name,
                kind,
                Visibility::Public,
                file_id,
                start,
                start + 5,
                sig,
                Some(format!("doc {name}")),
            ));
        };
        push_sym(
            1,
            "IFoo",
            SymbolKind::Interface,
            TypeSig::Named {
                fully_qualified_name: "IFoo".into(),
                generic_args: vec![],
            },
            ifoo,
        );
        push_sym(
            2,
            "FooImpl",
            SymbolKind::Class,
            TypeSig::Named {
                fully_qualified_name: "FooImpl".into(),
                generic_args: vec![],
            },
            fooimpl,
        );
        push_sym(
            3,
            "Main",
            SymbolKind::Function,
            TypeSig::Func {
                params: vec![],
                ret: Box::new(TypeSig::Unit),
                captures: false,
            },
            main,
        );
        file.symbol_table.entries.push(SymbolEntry::new(
            4,
            "IFoo.Bar",
            SymbolKind::Method,
            Visibility::Public,
            file_id,
            bar_first,
            bar_first + 3,
            TypeSig::Method {
                receiver: Box::new(TypeSig::Named {
                    fully_qualified_name: "IFoo".into(),
                    generic_args: vec![],
                }),
                params: vec![],
                ret: Box::new(TypeSig::Unit),
                is_virtual: true,
                vtable_slot: 0,
            },
            Some("doc IFoo.Bar".into()),
        ));
        file.symbol_table.entries.push(SymbolEntry::new(
            5,
            "FooImpl.Bar",
            SymbolKind::Method,
            Visibility::Public,
            file_id,
            bar_second,
            bar_second + 3,
            TypeSig::Method {
                receiver: Box::new(TypeSig::Named {
                    fully_qualified_name: "FooImpl".into(),
                    generic_args: vec![],
                }),
                params: vec![],
                ret: Box::new(TypeSig::Unit),
                is_virtual: false,
                vtable_slot: 0,
            },
            Some("doc FooImpl.Bar".into()),
        ));

        // 引用：`new FooImpl()`（类型注解）→ FooImpl(2)；`f.Bar()` 调用 → FooImpl.Bar(5)
        let fooimpl_use = src.match_indices("FooImpl").nth(1).unwrap().0 as u32;
        let fbar = src.find("f.Bar").unwrap() as u32;
        file.reference_table.entries.push(ReferenceEntry::new(
            0,
            2,
            file_id,
            fooimpl_use,
            fooimpl_use + 7,
            ReferenceContext::TypeAnnotation,
        ));
        file.reference_table.entries.push(ReferenceEntry::new(
            1,
            5,
            file_id,
            fbar,
            fbar + 5,
            ReferenceContext::Call,
        ));
        file.reference_graph = ReferenceGraph::default();

        fs::write(&arcgr_path, file.serialize()).unwrap();

        // 3. dispatcher 注入真实 `.arcgr`
        let dispatcher = MethodDispatcher::new();
        dispatcher
            .load_workspace_arcgr(dir.clone(), &arcgr_path)
            .unwrap();

        // 4. 构造 LSP 消息序列
        let uri = format!("file://{path_str}");
        let (dline, dcol) = pos_of(src, fooimpl_use as usize); // 定义查询：FooImpl 使用点
        let (hline, hcol) = pos_of(src, main as usize); // hover：Main 定义点
        let (rline, rcol) = pos_of(src, bar_second as usize); // references：FooImpl.Bar 定义点
        let position = |l: u32, c: u32| serde_json::json!({ "line": l, "character": c });
        let pos_req = |u: &str, p: serde_json::Value| serde_json::json!({ "textDocument": { "uri": u }, "position": p });
        let doc_req = |u: &str| serde_json::json!({ "textDocument": { "uri": u } });

        let messages = vec![
            JsonRpcMessage::request(
                RequestId::Number(1),
                INITIALIZE_METHOD,
                Some(serde_json::json!({ "processId": null })),
            ),
            JsonRpcMessage::notification(INITIALIZED_METHOD, Some(serde_json::json!({}))),
            JsonRpcMessage::request(
                RequestId::Number(2),
                DEFINITION_METHOD,
                Some(pos_req(&uri, position(dline, dcol))),
            ),
            JsonRpcMessage::request(
                RequestId::Number(3),
                HOVER_METHOD,
                Some(pos_req(&uri, position(hline, hcol))),
            ),
            JsonRpcMessage::request(
                RequestId::Number(4),
                REFERENCES_METHOD,
                Some(pos_req(&uri, position(rline, rcol))),
            ),
            JsonRpcMessage::request(
                RequestId::Number(5),
                DOCUMENT_SYMBOL_METHOD,
                Some(doc_req(&uri)),
            ),
            JsonRpcMessage::request(RequestId::Number(6), SHUTDOWN_METHOD, None),
            JsonRpcMessage::notification(EXIT_METHOD, None),
        ];
        let mut input = Vec::<u8>::new();
        for m in &messages {
            StdioTransport::write_message(&mut input, m).unwrap();
        }

        // 5. 运行 stdio 主循环
        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(shutdown_received, "shutdown must precede exit");
        assert_eq!(dispatcher.state(), ServerState::Exited);

        // 6. 解析响应并按 id 断言
        let mut cursor = Cursor::new(&output);
        let mut responses = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            responses.push(m);
        }
        // initialize + definition + hover + references + documentSymbol + shutdown = 6 响应
        assert_eq!(responses.len(), 6);

        let by_id = |id: i64| -> serde_json::Value {
            responses
                .iter()
                .find_map(|m| match m.kind().unwrap() {
                    MessageKind::ResponseOk { id: rid, result } if rid == RequestId::Number(id) => {
                        Some(result.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing response for id {id}"))
        };

        // definition：`new FooImpl()` → 解析到 FooImpl 定义位置
        let def = by_id(2);
        let (fdline, fdcol) = pos_of(src, fooimpl as usize);
        assert_eq!(def["range"]["start"]["line"], fdline as u64);
        assert_eq!(def["range"]["start"]["character"], fdcol as u64);
        assert!(def["uri"].as_str().unwrap().contains("main.as"));

        // hover：Main 定义点 → 返回 markdown 签名
        let hover = by_id(3);
        assert_eq!(hover["contents"]["kind"], "markdown");
        assert!(hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Main"));
        assert!(hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("void"));

        // references：FooImpl.Bar 定义点 → 定义 + 调用点 = 2 处
        let refs = by_id(4);
        let arr = refs.as_array().unwrap();
        assert_eq!(
            arr.len(),
            2,
            "references should include definition + call site"
        );

        // documentSymbol：列出全部 5 个符号；键须为 LSP 的 selectionRange
        let syms = by_id(5);
        let arr = syms.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        let names: Vec<&str> = arr.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Main"));
        assert!(names.contains(&"FooImpl"));
        assert!(names.contains(&"IFoo.Bar"));
        assert!(
            arr[0].get("selectionRange").is_some(),
            "LSP key must be selectionRange"
        );

        // shutdown：result 为 null
        assert!(by_id(6).is_null());

        // 清理临时目录
        let _ = fs::remove_dir_all(&dir);
    }

    // ─── 端到端：语法服务完整 LSP 流程（不依赖 .arcgr）───

    /// 走完整 LSP 语法流程：didOpen → foldingRange / semanticTokens / diagnostic
    /// → didChange（增量）→ 再查询 → shutdown → exit。验证三个语法 provider
    /// 在 stdio 协议下消费开放文档的 SyntaxTree。
    #[test]
    fn e2e_lsp_syntax_flow() {
        let dispatcher = MethodDispatcher::new();
        let uri = "file:///main.as";

        // didOpen 打开含两个跨行花括号块的源码
        let open_src = "class A {\n  void F() {\n  }\n}\n";
        let open_params = serde_json::json!({
            "textDocument": { "uri": uri, "languageId": "arc", "version": 1, "text": open_src }
        });
        let doc_req = |u: &str| serde_json::json!({ "textDocument": { "uri": u } });
        // 全量替换为单行类（无跨行块）→ 折叠清空，验证 didChange 触发重解析
        let did_change = serde_json::json!({
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [ { "text": "class AB { }\n" } ]
        });

        let messages = vec![
            JsonRpcMessage::request(
                RequestId::Number(1),
                INITIALIZE_METHOD,
                Some(serde_json::json!({ "processId": null })),
            ),
            JsonRpcMessage::notification(INITIALIZED_METHOD, Some(serde_json::json!({}))),
            JsonRpcMessage::notification(DID_OPEN_METHOD, Some(open_params)),
            // 变更前：折叠两个块、无诊断
            JsonRpcMessage::request(
                RequestId::Number(2),
                FOLDING_RANGE_METHOD,
                Some(doc_req(uri)),
            ),
            JsonRpcMessage::request(
                RequestId::Number(3),
                SEMANTIC_TOKENS_METHOD,
                Some(doc_req(uri)),
            ),
            JsonRpcMessage::request(RequestId::Number(4), DIAGNOSTIC_METHOD, Some(doc_req(uri))),
            // 增量插入 "B" → "class AB {}"（无跨行块）
            JsonRpcMessage::notification(DID_CHANGE_METHOD, Some(did_change)),
            JsonRpcMessage::request(
                RequestId::Number(5),
                FOLDING_RANGE_METHOD,
                Some(doc_req(uri)),
            ),
            JsonRpcMessage::request(RequestId::Number(6), SHUTDOWN_METHOD, None),
            JsonRpcMessage::notification(EXIT_METHOD, None),
        ];
        let mut input = Vec::<u8>::new();
        for m in &messages {
            StdioTransport::write_message(&mut input, m).unwrap();
        }

        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(shutdown_received, "shutdown must precede exit");
        assert_eq!(dispatcher.state(), ServerState::Exited);

        // 解析响应
        let mut cursor = Cursor::new(&output);
        let mut responses = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            responses.push(m);
        }
        // initialize + folding(2) + semantic(3) + diagnostic(4) + folding(5) + shutdown = 6
        assert_eq!(responses.len(), 6);
        let by_id = |id: i64| -> serde_json::Value {
            responses
                .iter()
                .find_map(|m| match m.kind().unwrap() {
                    MessageKind::ResponseOk { id: rid, result } if rid == RequestId::Number(id) => {
                        Some(result.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing response for id {id}"))
        };

        // 变更前折叠：两个跨行块（方法体 + 类体）
        let folds = by_id(2);
        let arr = folds.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|f| f["startLine"] == 0 && f["endLine"] == 3));
        assert!(arr.iter().any(|f| f["startLine"] == 1 && f["endLine"] == 2));

        // semanticTokens/full：data 长度 = token 数 × 5
        let sem = by_id(3);
        let data = sem["data"].as_array().unwrap();
        assert_eq!(data.len() % 5, 0);
        assert!(!data.is_empty());

        // diagnostic：合法源码无诊断
        let diag = by_id(4);
        assert_eq!(diag["kind"], "full");
        assert_eq!(diag["items"].as_array().map(|a| a.len()), Some(0));

        // 全量替换后：无跨行块 → 折叠为空（验证 didChange 触发重解析）
        let folds_after = by_id(5);
        assert_eq!(
            folds_after.as_array().map(|a| a.is_empty()),
            Some(true),
            "after full-sync change to single-line class, no folding ranges"
        );

        // shutdown：result 为 null
        assert!(by_id(6).is_null());
    }

    // ─── 端到端：M3 跨包 workspace/symbol 查询 ───

    /// 构造单文件 `.arcgr`：src 落盘 + 给定公共符号（字节偏移取自源码）。
    fn build_arcgr(
        dir: &std::path::Path,
        file_name: &str,
        src: &str,
        symbols: &[(&str, SymbolKind)],
    ) -> std::path::PathBuf {
        let src_path = dir.join(file_name);
        fs::write(&src_path, src).unwrap();
        let mut file = ArcgrFile::new();
        let file_id = 0u32;
        file.file_table.entries.push(FileEntry::new(
            file_id,
            file_name.to_string(),
            0xCAFE,
            src.lines().count() as u32,
        ));
        for (i, (name, kind)) in symbols.iter().enumerate() {
            let start = src.find(name).unwrap() as u32;
            file.symbol_table.entries.push(SymbolEntry::new(
                i as u32 + 1,
                *name,
                *kind,
                Visibility::Public,
                file_id,
                start,
                start + name.len() as u32,
                TypeSig::Func {
                    params: vec![],
                    ret: Box::new(TypeSig::Unit),
                    captures: false,
                },
                None,
            ));
        }
        file.reference_table = Default::default();
        file.reference_graph = ReferenceGraph::default();
        let arcgr_path = dir.join(format!("{file_name}.arcgr"));
        fs::write(&arcgr_path, file.serialize()).unwrap();
        arcgr_path
    }

    /// 主包 + 依赖包双 `.arcgr`：验证 `workspace/symbol` 跨包聚合。
    ///
    /// 主包 App.as 导出 `runMain`；依赖包 lib 导出 `runHelper`（不在主包）。
    /// 查询 "run" 应同时命中两包符号——证明跨包查询生效。
    #[test]
    fn e2e_workspace_symbol_cross_package() {
        let dir = std::env::temp_dir().join(format!("arc-server-m3-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        let app_src = "int runMain() { return 0; }\n";
        let lib_src = "int runHelper() { return 0; }\n";
        let app_arcgr = build_arcgr(
            &dir,
            "App.as",
            app_src,
            &[("runMain", SymbolKind::Function)],
        );
        let lib_arcgr = build_arcgr(
            &dir,
            "Lib.as",
            lib_src,
            &[("runHelper", SymbolKind::Function)],
        );

        let dispatcher = MethodDispatcher::new();
        dispatcher
            .load_workspace_arcgr(dir.clone(), &app_arcgr)
            .unwrap();
        dispatcher
            .load_dependency_package("lib", &lib_arcgr)
            .unwrap();

        let ws_req = |q: &str| serde_json::json!({ "query": q });
        let messages = vec![
            JsonRpcMessage::request(
                RequestId::Number(1),
                INITIALIZE_METHOD,
                Some(serde_json::json!({ "processId": null })),
            ),
            JsonRpcMessage::request(
                RequestId::Number(2),
                WORKSPACE_SYMBOL_METHOD,
                Some(ws_req("run")),
            ),
            JsonRpcMessage::request(
                RequestId::Number(3),
                WORKSPACE_SYMBOL_METHOD,
                Some(ws_req("Helper")),
            ),
            JsonRpcMessage::request(
                RequestId::Number(4),
                WORKSPACE_SYMBOL_METHOD,
                Some(ws_req("NoSuch")),
            ),
            JsonRpcMessage::request(RequestId::Number(5), SHUTDOWN_METHOD, None),
            JsonRpcMessage::notification(EXIT_METHOD, None),
        ];
        let mut input = Vec::<u8>::new();
        for m in &messages {
            StdioTransport::write_message(&mut input, m).unwrap();
        }

        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(shutdown_received, "shutdown must precede exit");

        let mut cursor = Cursor::new(&output);
        let mut responses = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            responses.push(m);
        }
        assert_eq!(responses.len(), 5); // init + 3 symbol + shutdown
        let by_id = |id: i64| -> serde_json::Value {
            responses
                .iter()
                .find_map(|m| match m.kind().unwrap() {
                    MessageKind::ResponseOk { id: rid, result } if rid == RequestId::Number(id) => {
                        Some(result.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing response for id {id}"))
        };
        let names = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap().to_string())
                .collect()
        };

        // 跨包："run" 同时命中主包 runMain 与依赖包 runHelper
        let run = names(&by_id(2));
        assert!(run.contains(&"runMain".to_string()), "main package symbol");
        assert!(
            run.contains(&"runHelper".to_string()),
            "dependency package symbol"
        );
        // 仅依赖包符号：跨包定位到 lib
        let helper = names(&by_id(3));
        assert_eq!(helper, vec!["runHelper".to_string()]);
        // 无匹配 → 空数组
        assert!(by_id(4).as_array().unwrap().is_empty());
        // shutdown null
        assert!(by_id(5).is_null());

        let _ = fs::remove_dir_all(&dir);
    }

    /// 端到端：跨包 Goto Definition。
    ///
    /// 主包 main.as 对 `ExternalType` 有一个指向包外 symbol_id 的引用（外部引用）；
    /// 依赖包 Lib.as 定义公共 `ExternalType`。请求 `textDocument/definition`
    /// 应跨包跳转到 Lib.as 的定义位置。
    #[test]
    fn e2e_definition_cross_package() {
        let dir = std::env::temp_dir().join(format!("arc-server-def-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        // 主包 main.as：仅一个外部引用（span [0,12] 覆盖 "ExternalType"）
        let main_path = dir.join("main.as");
        let main_uri = format!("file://{}", main_path.display());
        let main_src = "ExternalType\n";
        fs::write(&main_path, main_src).unwrap();
        let mut main = ArcgrFile::new();
        main.file_table.entries.push(FileEntry::new(
            1,
            main_path.display().to_string().replace('\\', "/"),
            0xAA,
            1,
        ));
        main.reference_table.entries.push(ReferenceEntry::new(
            1,
            99, // 外部目标（主包无此符号）
            1,
            0,
            12,
            ReferenceContext::TypeAnnotation,
        ));
        main.reference_graph = ReferenceGraph::default();
        let main_arcgr = dir.join("main.arcgr");
        fs::write(&main_arcgr, main.serialize()).unwrap();

        // 依赖包 Lib.as：定义公共 ExternalType
        let lib_path = dir.join("Lib.as");
        let lib_src = "ExternalType\n";
        fs::write(&lib_path, lib_src).unwrap();
        let mut lib = ArcgrFile::new();
        lib.file_table.entries.push(FileEntry::new(
            1,
            lib_path.display().to_string().replace('\\', "/"),
            0xBB,
            1,
        ));
        lib.symbol_table.entries.push(SymbolEntry::new(
            7,
            "ExternalType",
            SymbolKind::Class,
            Visibility::Public,
            1,
            0,
            12,
            TypeSig::Named {
                fully_qualified_name: "ExternalType".into(),
                generic_args: vec![],
            },
            None,
        ));
        lib.reference_table = Default::default();
        lib.reference_graph = ReferenceGraph::default();
        let lib_arcgr = dir.join("Lib.arcgr");
        fs::write(&lib_arcgr, lib.serialize()).unwrap();

        let dispatcher = MethodDispatcher::new();
        dispatcher
            .load_workspace_arcgr(dir.clone(), &main_arcgr)
            .unwrap();
        dispatcher
            .load_dependency_package("lib", &lib_arcgr)
            .unwrap();

        let messages = vec![
            JsonRpcMessage::request(
                RequestId::Number(1),
                INITIALIZE_METHOD,
                Some(serde_json::json!({ "processId": null })),
            ),
            JsonRpcMessage::request(
                RequestId::Number(2),
                DEFINITION_METHOD,
                Some(serde_json::json!({
                    "textDocument": { "uri": main_uri },
                    "position": { "line": 0, "character": 1 }
                })),
            ),
            JsonRpcMessage::request(RequestId::Number(3), SHUTDOWN_METHOD, None),
            JsonRpcMessage::notification(EXIT_METHOD, None),
        ];
        let mut input = Vec::<u8>::new();
        for m in &messages {
            StdioTransport::write_message(&mut input, m).unwrap();
        }
        let mut output = Vec::<u8>::new();
        let shutdown_received = run_server(Cursor::new(input), &mut output, &dispatcher).unwrap();
        assert!(shutdown_received);

        let mut cursor = Cursor::new(&output);
        let mut responses = Vec::new();
        while let Some(m) = StdioTransport::read_message(&mut cursor).unwrap() {
            responses.push(m);
        }
        assert_eq!(responses.len(), 3);
        let by_id = |id: i64| -> serde_json::Value {
            responses
                .iter()
                .find_map(|m| match m.kind().unwrap() {
                    MessageKind::ResponseOk { id: rid, result } if rid == RequestId::Number(id) => {
                        Some(result.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing response for id {id}"))
        };

        // 跨包：definition 跳转到 Lib.as（依赖包）
        let def = by_id(2);
        assert!(
            def["uri"].as_str().unwrap().ends_with("Lib.as"),
            "expected dependency package definition, got {def}"
        );
        assert_eq!(def["range"]["start"]["line"], 0);
        assert_eq!(def["range"]["start"]["character"], 0);
        assert_eq!(def["range"]["end"]["character"], 12);
        // shutdown null
        assert!(by_id(3).is_null());

        let _ = fs::remove_dir_all(&dir);
    }
}
