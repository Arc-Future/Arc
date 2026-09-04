namespace Arc.IO;

/// <summary>目录操作门面（L2 Stable 最小面）。</summary>
///
/// Stable：CreateDirectory / Exists / Delete / <c>GetFiles</c>（含 searchPattern）/
/// <c>GetDirectories</c>（codegen → <c>rt_dir_*</c>）。
/// Move、当前目录、带 SearchOption 的重载等未接线——禁止 stub 静默 null。
public class Directory {
    /// <summary>创建目录（已存在视为成功）。</summary>
    [Builtin(ABI = "rt_dir_create")]
    public static bool CreateDirectory(string path) { return false; }

    /// <summary>判断目录是否存在。</summary>
    [Builtin(ABI = "rt_dir_exists")]
    public static bool Exists(string path) { return false; }

    /// <summary>删除空目录。</summary>
    [Builtin(ABI = "rt_dir_delete")]
    public static bool Delete(string path) { return false; }

    /// <summary>
    /// 枚举目录下常规文件（非递归；不含子目录）。
    /// 返回完整路径 <c>string[]</c>；失败或空目录返回 Length 0（非 null）。
    /// </summary>
    [Builtin(ABI = "rt_dir_list_files")]
    public static string[] GetFiles(string path) { return null; }

    /// <summary>
    /// 按 <c>searchPattern</c>（<c>*</c>/<c>?</c>）枚举常规文件（非递归）。
    /// 返回完整路径；失败/无匹配/空 pattern → Length 0（非 null）。
    /// </summary>
    [Builtin(ABI = "rt_dir_list_files_pattern")]
    public static string[] GetFiles(string path, string searchPattern) { return null; }

    /// <summary>
    /// 枚举直接子目录（非递归；跳过 <c>.</c>/<c>..</c>）。
    /// 返回完整路径；失败或无子目录 → Length 0（非 null）。
    /// </summary>
    [Builtin(ABI = "rt_dir_list_dirs")]
    public static string[] GetDirectories(string path) { return null; }

    // ── Async（RFC 009 异步优先；AIWorkspace/AICoordinator 依赖）──

    /// <summary>异步创建目录（已存在视为成功；语义同 <see cref="CreateDirectory"/>）。</summary>
    [Builtin(ABI = "rt_dir_create_async")]
    public static Task<bool> CreateDirectoryAsync(string path) { return null; }

    /// <summary>异步判断目录是否存在（语义同 <see cref="Exists"/>）。</summary>
    [Builtin(ABI = "rt_dir_exists_async")]
    public static Task<bool> ExistsAsync(string path) { return null; }

    /// <summary>异步删除空目录（语义同 <see cref="Delete"/>）。</summary>
    [Builtin(ABI = "rt_dir_delete_async")]
    public static Task<bool> DeleteAsync(string path) { return null; }

    /// <summary>异步枚举常规文件（非递归；语义同 <see cref="GetFiles(string)"/>）。</summary>
    [Builtin(ABI = "rt_dir_list_files_async")]
    public static Task<string[]> GetFilesAsync(string path) { return null; }

    /// <summary>异步按 pattern 枚举常规文件（非递归；语义同 <see cref="GetFiles(string,string)"/>）。</summary>
    [Builtin(ABI = "rt_dir_list_files_pattern_async")]
    public static Task<string[]> GetFilesAsync(string path, string searchPattern) { return null; }

    /// <summary>异步枚举直接子目录（非递归；语义同 <see cref="GetDirectories"/>）。</summary>
    [Builtin(ABI = "rt_dir_list_dirs_async")]
    public static Task<string[]> GetDirectoriesAsync(string path) { return null; }
}
