namespace Arc.IO;

/// <summary>路径操作门面（L2 Stable 最小面）。</summary>
///
/// 设计要点：
///   - Combine / GetDirectoryName / GetFileName / GetFileNameWithoutExtension / GetExtension /
///     ChangeExtension / HasExtension / GetTempPath 由 codegen 拦截并直连 <c>rt_path_*</c>
///   - 分隔符常量为真实 Arc 返回值（非假 ABI）
///   - GetFullPath 等未接线扩展仍不暴露（禁止 stub）
public class Path {
    /// <summary>目录分隔符（'/'）。跨平台统一正斜杠。</summary>
    public static string DirectorySeparatorChar { get { return "/"; } }

    /// <summary>路径拼接：a/b。智能处理分隔符（避免双斜杠）。</summary>
    [Builtin(ABI = "rt_path_combine")]
    public static string Combine(string a, string b) { return ""; }

    /// <summary>获取目录名：path 去掉最后一段文件名后的部分。</summary>
    [Builtin(ABI = "rt_path_get_dir_name")]
    public static string GetDirectoryName(string path) { return ""; }

    /// <summary>获取文件名（含扩展名）。</summary>
    [Builtin(ABI = "rt_path_get_file_name")]
    public static string GetFileName(string path) { return ""; }

    /// <summary>获取不含扩展名的文件名。</summary>
    [Builtin(ABI = "rt_path_get_file_name_without_ext")]
    public static string GetFileNameWithoutExtension(string path) { return ""; }

    /// <summary>获取扩展名（含前导点）。</summary>
    [Builtin(ABI = "rt_path_get_extension")]
    public static string GetExtension(string path) { return ""; }

    /// <summary>更换扩展名。ext 为空或 null 时去掉扩展名；不以 '.' 开头时自动补点。</summary>
    [Builtin(ABI = "rt_path_change_extension")]
    public static string ChangeExtension(string path, string extension) { return ""; }

    /// <summary>路径是否含扩展名（含仅 "."）。</summary>
    [Builtin(ABI = "rt_path_has_extension")]
    public static bool HasExtension(string path) { return false; }

    /// <summary>系统临时目录；始终带尾部目录分隔符（对齐 C#）。</summary>
    [Builtin(ABI = "rt_path_get_temp_path")]
    public static string GetTempPath() { return ""; }
}
