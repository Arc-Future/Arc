// RFC 037 M-CE1: read-only memory-mapped file facade.
// Blocker ABI for CodeEditor OpenPath — MUST NOT use File.ReadAllText on large files.

namespace Arc.IO;

/// <summary>
/// 只读内存映射文件（C# MemoryMappedFile 最小面 · Draft）。
/// </summary>
/// <remarks>
/// 底层 <c>rt_file_mmap_*</c>；映射在 <see cref="Dispose"/> 前有效。
/// 权威：见 RFC 037。
/// </remarks>
public class MemoryMappedFile {
    private long _handle;

    /// <summary>以只读方式映射现有文件。失败时 handle 为 0。</summary>
    [Builtin(ABI = "rt_file_mmap_open")]
    public MemoryMappedFile(string path) {
        _handle = 0;
    }

    /// <summary>映射字节长度。</summary>
    [Builtin(ABI = "rt_file_mmap_length")]
    // 该 getter 无 codegen 拦截（无 get_Length → rt_file_mmap_length 直射），
    // 须保留显式死代码体，勿改自动属性（自动属性无 backing field 将致未拦截访问失败）。
    public long Length { get { return 0; } }

    /// <summary>释放映射。</summary>
    [Builtin(ABI = "rt_file_mmap_close")]
    public void Dispose() {
    }
}
