namespace Arc.IO {
/// <summary>文件 I/O 门面（L2 Stable 最小面）。</summary>
///
/// 设计要点：
///   - [Builtin] 方法由 codegen 拦截并直连 <c>rt_*</c> ABI（body 不执行）
///   - Stable 面仅含 ABI 文档已列出且 codegen 已接线的方法
///   - 未接线扩展面不得以 stub 静默返回 null/false 冒充完备
///   - 流打开：<c>OpenRead</c>/<c>OpenWrite</c>/<c>OpenText</c>/<c>Create</c> → <see cref="FileStream"/>
public class File {
    // ── 文本 I/O ──

    [Builtin(ABI = "rt_read_file")]
    public static string ReadAllText(string path) { return ""; }

    [Builtin(ABI = "rt_write_file")]
    public static bool WriteAllText(string path, string content) { return false; }

    [Builtin(ABI = "rt_file_append")]
    public static bool AppendAllText(string path, string content) { return false; }

    /// <summary>按行读取全文（识别 <c>\r\n</c>/<c>\n</c>/<c>\r</c>；尾部换行不产生空行）。
    /// 失败返回 Length 0 的数组（非 null）。</summary>
    [Builtin(ABI = "rt_file_read_all_lines")]
    public static string[] ReadAllLines(string path) { return null; }

    // ── 二进制 I/O（L2 deepen）──

    /// <summary>读取文件全部字节。失败返回 Length 0 的数组（非 null）。</summary>
    [Builtin(ABI = "rt_file_read_all_bytes")]
    public static byte[] ReadAllBytes(string path) { return null; }

    /// <summary>覆盖写入字节数组。成功返回 true。</summary>
    [Builtin(ABI = "rt_file_write_all_bytes")]
    public static bool WriteAllBytes(string path, byte[] bytes) { return false; }

    // ── 流 I/O（codegen 拦截 → new FileStream(path, mode)）──

    /// <summary>以只读方式打开现有文件。</summary>
    [Builtin(ABI = "rt_file_open_read")]
    public static Stream OpenRead(string path) { return null; }

    /// <summary>以写入方式打开或创建文件。</summary>
    [Builtin(ABI = "rt_file_open_write")]
    public static Stream OpenWrite(string path) { return null; }

    /// <summary>以文本读取方式打开现有文件（等同 OpenRead）。</summary>
    [Builtin(ABI = "rt_file_open_text")]
    public static Stream OpenText(string path) { return null; }

    /// <summary>创建或覆盖文件用于写入。</summary>
    [Builtin(ABI = "rt_file_create")]
    public static Stream Create(string path) { return null; }

    // ── 文件系统操作 ──

    [Builtin(ABI = "rt_file_exists")]
    public static bool Exists(string path) { return false; }

    [Builtin(ABI = "rt_file_delete")]
    public static bool Delete(string path) { return false; }

    [Builtin(ABI = "rt_file_copy")]
    public static bool Copy(string src, string dst) { return false; }

    [Builtin(ABI = "rt_file_move")]
    public static bool Move(string src, string dst) { return false; }

    // ── Async（Reactor 真异步，RFC 009 M2）──
    // 数据面 I/O（文本/字节/行）直连 OS 非阻塞原语（IOCP/io_uring），不占用线程池。

    [Builtin(ABI = "rt_file_read_all_text_async")]
    public static Task<string> ReadAllTextAsync(string path) { return null; }

    [Builtin(ABI = "rt_file_write_all_text_async")]
    public static Task<bool> WriteAllTextAsync(string path, string content) { return null; }

    [Builtin(ABI = "rt_file_append_all_text_async")]
    public static Task<bool> AppendAllTextAsync(string path, string content) { return null; }

    [Builtin(ABI = "rt_file_copy_async")]
    public static Task<bool> CopyAsync(string src, string dst) { return null; }

    [Builtin(ABI = "rt_file_move_async")]
    public static Task<bool> MoveAsync(string src, string dst) { return null; }

    // ── Async 补全（RFC 009 异步优先；数据面真异步 + 元数据线程池）──

    /// <summary>按行异步读取全文（语义同 <see cref="ReadAllLines"/>）。失败返回 Length 0（非 null）。Reactor 真异步。</summary>
    [Builtin(ABI = "rt_file_read_all_lines_async")]
    public static Task<string[]> ReadAllLinesAsync(string path) { return null; }

    /// <summary>异步读取全部字节（语义同 <see cref="ReadAllBytes"/>）。失败返回 Length 0（非 null）。Reactor 真异步。</summary>
    [Builtin(ABI = "rt_file_read_all_bytes_async")]
    public static Task<byte[]> ReadAllBytesAsync(string path) { return null; }

    /// <summary>异步覆盖写入字节数组（语义同 <see cref="WriteAllBytes"/>）。成功返回 true。Reactor 真异步。</summary>
    [Builtin(ABI = "rt_file_write_all_bytes_async")]
    public static Task<bool> WriteAllBytesAsync(string path, byte[] bytes) { return null; }

    /// <summary>异步删除文件（语义同 <see cref="Delete"/>）。删除成功返回 true。元数据操作，线程池包装。</summary>
    [Builtin(ABI = "rt_file_delete_async")]
    public static Task<bool> DeleteAsync(string path) { return null; }

    /// <summary>异步判断文件是否存在（语义同 <see cref="Exists"/>）。元数据操作，线程池包装。</summary>
    [Builtin(ABI = "rt_file_exists_async")]
    public static Task<bool> ExistsAsync(string path) { return null; }
}
}
