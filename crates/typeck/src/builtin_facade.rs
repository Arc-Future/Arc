//! Stub facade 类名的单一事实源（typeck / MIR / codegen 契约）。
//!
//! std 中部分类方法体为空，行为由 codegen 拦截并发射 `rt_*` ABI。
//! typeck 跳过这些类的方法体检查；MIR 可按同类名路由静态调用。
//!
//! **维护规则**：新增 facade 时先在此登记，再实现 codegen handler。
//! handler 仍在 `codegen`（发射逻辑无法上提）；类名集合不得在 codegen 另起清单。

/// Facade 族分类——供 codegen 顶层分发与文档交叉引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltinFacadeKind {
    /// `List_` / `Dictionary_` / … 单态前缀集合
    Collection,
    /// RFC 048: 命名管道门面（NamedPipeServerStream/NamedPipeClientStream）
    Pipe,
    Math,
    Vector,
    /// `Tensor_` 前缀
    Tensor,
    StringBuilder,
    Task,
    Console,
    /// `Environment` — ArgCount/GetArg/env vars/process info → `rt_env_*`
    Environment,
    File,
    Directory,
    Path,
    Base64,
    Hex,
    Encoding,
    /// Arc.Text.Url — Encode/Decode 百分号编解码 → rt_text_url_*
    Url,
    /// Arc.Text.Regex — IsMatch/Match/MatchGroup/Matches/Replace/Split → rt_regex_*
    Regex,
    Window,
    WindowHost,
    /// 基元 `string` 的静态方法拦截
    StringPrim,
    Net,
    Thread,
    Mutex,
    Semaphore,
    Monitor,
    /// RFC 009 §7.5：Interlocked int 原子面 → LLVM atomicrmw/cmpxchg
    Interlocked,
    Lock,
    ThreadPoolScheduler,
    Parallel,
    /// 属性 / 取消令牌等 std 弱符号防御层
    StdDefensive,
    /// RFC 042: P2P 网络库 facade — Ed25519 / Noise / Kademlia
    P2P,
    /// L3 Orm SQLite execute MVP — `SqliteDb` → `rt_sqlite_*`
    Sqlite,
    /// RFC 029 M1 图像编解码 — `ImageNative` → `rt_image_*`（std/Drawing 内部门面）
    Drawing,
}

const COLLECTION_PREFIXES: &[&str] = &[
    "List_",
    "Dictionary_",
    "SortedDictionary_",
    "SortedSet_",
    "LinkedList_",
    "LinkedListNode_",
    "HashSet_",
    "Queue_",
    "ConcurrentDictionary_",
    "ConcurrentQueue_",
    "ConcurrentBag_",
    "ConcurrentStack_",
    "BlockingCollection_",
    "ListEnumerator_",
    "Stack_",
];

/// 将类名归类为 facade 族；非 facade 返回 `None`。
pub fn classify_builtin_facade(class_name: &str) -> Option<BuiltinFacadeKind> {
    if COLLECTION_PREFIXES
        .iter()
        .any(|p| class_name.starts_with(p))
    {
        return Some(BuiltinFacadeKind::Collection);
    }
    if class_name.starts_with("Tensor_") {
        return Some(BuiltinFacadeKind::Tensor);
    }
    // RFC 008 AsyncStream：TaskCompletionSource<T> 为 stub facade（方法体由 codegen
    // try_emit_tcs_method 直射 rt_task_* ABI，`new` 拦截为 rt_task_create_pending）。
    // 泛型实例 mangled 为 "TaskCompletionSource_<T>"，前缀匹配须与 codegen 的
    // `class.starts_with("TaskCompletionSource")` 拦截条件一致，否则跨包消费时
    // external_decls 会为其方法发射 declare，与 emit_stubs 的 define 冲突
    // （LLVM invalid redefinition）。
    if class_name.starts_with("TaskCompletionSource") {
        return Some(BuiltinFacadeKind::Task);
    }
    match class_name {
        "Math" => Some(BuiltinFacadeKind::Math),
        "Vector" => Some(BuiltinFacadeKind::Vector),
        "StringBuilder" => Some(BuiltinFacadeKind::StringBuilder),
        "Task" => Some(BuiltinFacadeKind::Task),
        "Console" => Some(BuiltinFacadeKind::Console),
        "Environment" => Some(BuiltinFacadeKind::Environment),
        "File" => Some(BuiltinFacadeKind::File),
        "Directory" => Some(BuiltinFacadeKind::Directory),
        "Path" => Some(BuiltinFacadeKind::Path),
        // FileStream 不在 facade 清单：静态工厂 OpenRead/Create 须保留真实
        // `new FileStream(...)` 方法体；实例 [Builtin] 由 emit_stubs + 调用点拦截。
        "Base64" => Some(BuiltinFacadeKind::Base64),
        "Hex" => Some(BuiltinFacadeKind::Hex),
        "Encoding" => Some(BuiltinFacadeKind::Encoding),
        "Url" => Some(BuiltinFacadeKind::Url),
        "Regex" => Some(BuiltinFacadeKind::Regex),
        "Window" => Some(BuiltinFacadeKind::Window),
        "WindowHost" => Some(BuiltinFacadeKind::WindowHost),
        "string" => Some(BuiltinFacadeKind::StringPrim),
        // RFC 026 M3 P0-1：Security 哈希/HMAC/CSPRNG 门面已改 AesGcm 模式
        // （私有 [Builtin] _ComputeHash/_GetBytes + 公开真实体），方法体正常编译，
        // 不再是 stub facade——调用点由 builtin_dispatch 的 `Class::_Method` 臂拦截。
        "Socket" | "TcpClient" | "TcpListener" | "UdpClient" | "Dns" => {
            Some(BuiltinFacadeKind::Net)
        }
        // RFC 048: 命名管道门面（本机 IPC · rt_pipe_* 双后端直射）。
        "NamedPipeServerStream" | "NamedPipeClientStream" => Some(BuiltinFacadeKind::Pipe),
        "Thread" => Some(BuiltinFacadeKind::Thread),
        "Mutex" => Some(BuiltinFacadeKind::Mutex),
        "Semaphore" => Some(BuiltinFacadeKind::Semaphore),
        "Monitor" => Some(BuiltinFacadeKind::Monitor),
        "Interlocked" => Some(BuiltinFacadeKind::Interlocked),
        "Lock" => Some(BuiltinFacadeKind::Lock),
        "ThreadPoolScheduler" => Some(BuiltinFacadeKind::ThreadPoolScheduler),
        "Parallel" => Some(BuiltinFacadeKind::Parallel),
        // RFC 042: P2P facade 类（PeerKey / NoiseTransport / SecureSession 已改
        // AesGcm 模式 regular class，经 builtin_dispatch S0 拦截，不再列为 facade）
        "PeerId"
        | "DhtDiscovery" | "Cid" | "Multiaddr" | "PeerRecord"
        | "StunClient" | "TurnClient"
        | "IceAgent" | "P2PMessage" | "RelayServer" | "RelaySession"
        | "MdnsDiscovery" | "BootstrapDiscovery" | "AutoNat"
        | "CircuitRelay" | "GossipSubRouter" | "P2PNode"
        | "CompositeTransport" | "QuicTransport"
        | "InMemoryPeerStore" => Some(BuiltinFacadeKind::P2P),
        "SqliteDb" => Some(BuiltinFacadeKind::Sqlite),
        // RFC 029 M1/M2/M4：std/Drawing 内部 C ABI 门面（Image/Bitmap/QrCodeWriter/
        // QrCodeReader/BarcodeReader 纯 Arc 包装经此调用）
        "ImageNative" => Some(BuiltinFacadeKind::Drawing),
        "QrCodeNative" => Some(BuiltinFacadeKind::Drawing),
        "BarcodeNative" => Some(BuiltinFacadeKind::Drawing),
        // RFC 037 §10 AL-P0：渲染域 PNG 直出 facade（纯 stub 类，rt_image_* 复用；
        // builtin_dispatch 已登记 PngNative. 前缀，见 builtin_dispatch.rs）。
        "PngNative" => Some(BuiltinFacadeKind::Drawing),
        "CancellationToken"
        | "CancellationTokenSource"
        | "ParallelOptions"
        | "Attribute"
        | "TableAttribute"
        | "ColumnAttribute"
        | "KeyAttribute"
        | "AttributeUsageAttribute"
        | "RequiredAttribute"
        | "MaxLengthAttribute"
        | "BuiltinAttribute"
        | "Array"
        | "BitConverter"
        | "Buffer"
        // RFC 005：契约面；实例行为由 TypeId::Span Builtin 接线，跳过方法体。
        | "Span"
        | "ReadOnlySpan" => Some(BuiltinFacadeKind::StdDefensive),
        _ => None,
    }
}

/// stub facade 类检测——typeck 跳过方法体、MIR 路由静态调用的入口。
pub fn is_builtin_facade(class_name: &str) -> bool {
    classify_builtin_facade(class_name).is_some()
}

/// 拆分 `"Console.WriteLine"` → `("Console", "WriteLine")`。
pub fn split_qualified_method(func: &str) -> Option<(&str, &str)> {
    func.split_once('.')
}

/// 判定属性源码级别的 `[Builtin]` 标记（AST 属性路径末段名匹配）。
///
/// registry/check_class 需在属性解析（`resolve_attributes`）之前、在类型注册
/// 阶段判断属性是否内建，故基于原始 AST `attributes` 的末段路径名匹配，而不是
/// 依赖已解析的 `AttributeTable`。用于把 `[Builtin]` 属性在注册层面归入
/// "custom 访问器"（注册 `get_X`/`set_X`、不生成 backing field），使 MIR
/// `is_custom_accessor_property` 返回 true → 访问降为 `get_X`/`set_X`
/// MethodCall → codegen 拦截直射 `rt_*` ABI。
pub fn is_builtin_property_attr(attrs: &[ast::Attribute]) -> bool {
    attrs
        .iter()
        .any(|a| a.path.last().is_some_and(|i| i.as_str() == "Builtin"))
}

/// Codegen 侧 handler 模块提示（文档/诊断用；发射逻辑仍在 codegen）。
pub fn codegen_handler_hint(kind: BuiltinFacadeKind) -> &'static str {
    match kind {
        BuiltinFacadeKind::Collection => "emit_stubs / list_* ABI",
        BuiltinFacadeKind::Math => "try_emit_math_call",
        BuiltinFacadeKind::Vector => "try_emit_vector_call",
        BuiltinFacadeKind::Tensor => "emit_stubs / tensor_* ABI",
        BuiltinFacadeKind::StringBuilder => "emit_builtin StringBuilder",
        BuiltinFacadeKind::Task => "try_emit_task_*",
        BuiltinFacadeKind::Console => "try_emit_console_static",
        BuiltinFacadeKind::Environment => "try_emit_environment_static",
        BuiltinFacadeKind::File | BuiltinFacadeKind::Directory | BuiltinFacadeKind::Path => {
            "try_emit_io_static"
        }
        BuiltinFacadeKind::Base64 | BuiltinFacadeKind::Hex => "rt_text_* encode/decode",
        BuiltinFacadeKind::Url => "builtin_dispatch Url.* (rt_text_url_*)",
        BuiltinFacadeKind::Encoding => "rt_text_utf8_get_bytes/get_string/get_byte_count",
        BuiltinFacadeKind::Regex => "builtin_dispatch Regex.* (rt_regex_*)",
        BuiltinFacadeKind::Window => "Window.* static",
        BuiltinFacadeKind::WindowHost => "try_emit_window_host_element",
        BuiltinFacadeKind::StringPrim => "try_emit_primitive_static / string.*",
        BuiltinFacadeKind::Net => "try_emit_socket_*",
        BuiltinFacadeKind::Pipe => "try_emit_pipe_method",
        BuiltinFacadeKind::Thread => "try_emit_thread_*",
        BuiltinFacadeKind::Mutex => "try_emit_mutex_method",
        BuiltinFacadeKind::Semaphore => "try_emit_semaphore_method",
        BuiltinFacadeKind::Monitor => "try_emit_monitor_static",
        BuiltinFacadeKind::Interlocked => "try_emit_interlocked_static (LLVM atomics)",
        BuiltinFacadeKind::Lock => "threading lock ABI",
        BuiltinFacadeKind::ThreadPoolScheduler => "try_emit_threadpool_method",
        BuiltinFacadeKind::Parallel => "emit_parallel_for",
        BuiltinFacadeKind::StdDefensive => "linkonce_odr / std defensive",
        BuiltinFacadeKind::P2P => "try_emit_p2p_*",
        BuiltinFacadeKind::Sqlite => "try_emit_sqlite_static",
        BuiltinFacadeKind::Drawing => "try_emit_image_native_static",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_and_exact_facades() {
        assert!(is_builtin_facade("List_int"));
        assert!(is_builtin_facade("Console"));
        assert!(!is_builtin_facade("Person"));
        assert_eq!(
            classify_builtin_facade("File"),
            Some(BuiltinFacadeKind::File)
        );
        // RFC 008：TaskCompletionSource<T> stub（基类 + 泛型实例前缀）。
        assert!(is_builtin_facade("TaskCompletionSource"));
        assert!(is_builtin_facade("TaskCompletionSource_bool"));
        assert_eq!(
            split_qualified_method("Console.WriteLine"),
            Some(("Console", "WriteLine"))
        );
    }
}
