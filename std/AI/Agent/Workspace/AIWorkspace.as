// RFC 038 §3.2（M6）：工作区 —— 文件作用域沙箱。
// 限定根目录；规范路径 + 逃逸拒绝（前缀校验）；读/写分权授权。
// 定位：工具执行沙箱的实体化（现 AIToolSandbox 仅能力门控、无路径隔离）。
// 异步为主：I/O 面全走 async（File.*Async / Directory.*Async；目录枚举不经 Task.Run）。
namespace Arc.Agent;
using Arc;
using Arc.Collections;
using Arc.IO;

/// <summary>工作区访问模式（读写分权）。</summary>
public enum AIWorkspaceAccess {
    /// <summary>只读：不可写文件。</summary>
    ReadOnly,
    /// <summary>读写：可在根内写文件（仍须逃逸拒绝）。</summary>
    ReadWrite,
}

/// <summary>
/// 文件作用域沙箱：所有 FS 操作须经 <see cref="ResolvePath"/> 规范化并校验逃逸。
/// 逃逸（根外路径 / ".." 越界）一律拒绝——模型无关的宿主级路径隔离。
/// </summary>
public class AIWorkspace {
    public string Root { get; }
    public AIWorkspaceAccess Mode { get; }

    public AIWorkspace(string root, AIWorkspaceAccess mode) {
        Root = root != null ? root : "";
        Mode = mode;
    }


    public bool CanWrite() {
        return Mode == AIWorkspaceAccess.ReadWrite;
    }

    /// <summary>
    /// 规范化 path 并校验其落在根内：先规范化根与目标（合并 "." / 弹出 ".."），
    /// 再前缀校验（目标 === 根 或 目标以 "根/ 前缀开头）。
    /// 相对路径先拼根；逃逸 / 非法 → 返回 null（调用方据此拒绝）。可空返回：null = 逃逸。
    /// 注：Arc typeck 对方法调用的返回类型丢失 nullable 标注（见 ProcessStream 注释），
    /// 调用方须以 `== null` 显式判空——本标注是面向开发者的契约文档。
    /// </summary>
    public string? ResolvePath(string path) {
        if (path == null || path == "") {
            return null;
        }
        if (Root == null || Root == "") {
            return null;
        }
        string rootNorm = AIWorkspace.Normalize(Root);
        if (rootNorm == null) {
            return null;
        }
        // 相对路径（无前导 "/" 且非盘符）先拼根再规范化。
        string cand = AIWorkspace.IsAbsolute(path) ? path : (rootNorm + "/" + path);
        string targetNorm = AIWorkspace.Normalize(cand);
        if (targetNorm == null) {
            return null;
        }
        // 前缀校验：目标 === 根，或 目标以 "根/" 开头。
        if (targetNorm == rootNorm) {
            return targetNorm;
        }
        string prefix = rootNorm + "/";
        if (targetNorm.StartsWith(prefix)) {
            return targetNorm;
        }
        return null;
    }

    /// <summary>判断是否为绝对路径（前导 "/" 或盘符 "X:"）。</summary>
    private static bool IsAbsolute(string path) {
        if (path == null) {
            return false;
        }
        if (path.StartsWith("/")) {
            return true;
        }
        // Windows 盘符：C:/ 或 C:\（第 2 个字符为 ':'）。
        if (path.Length >= 2 && path.Substring(1, 1) == ":") {
            return true;
        }
        return false;
    }

    /// <summary>
    /// 段级路径规范化：按 "/" 切段，"." 丢弃、".." 弹出上一段；越界弹出失败返回 null。
    /// 返回规范化路径（无尾部多余分隔符；保留根段结构）。可空返回：null = 越界逃逸。
    /// </summary>
    private static string? Normalize(string path) {
        string p = path != null ? path : "";
        // 统一分隔符（Arc 跨平台统一正斜杠；容忍反斜杠输入）。
        p = p.Replace("\\", "/");
        if (p == "") {
            return "";
        }
        string[] segs = p.Split("/");
        int n = segs.Length;
        List<string> stack = new List<string>();
        bool absolute = p.StartsWith("/");
        int i = 0;
        while (i < n) {
            string s = segs[i];
            if (s == "" || s == ".") {
                // 空段（重复分隔符）与当前段丢弃。
                i = i + 1;
                continue;
            }
            if (s == "..") {
                if (stack.Count == 0) {
                    // 向上越界（如 "/../x" 或首个即 ".."）→ 无法在根内表达，判为逃逸。
                    return null;
                }
                stack.RemoveAt(stack.Count - 1);
                i = i + 1;
                continue;
            }
            stack.Add(s);
            i = i + 1;
        }
        // 拼回规范化路径。绝对路径保留前导分隔符语义。
        string result = "";
        int j = 0;
        while (j < stack.Count) {
            if (result != "") {
                result = result + "/";
            }
            result = result + stack[j];
            j = j + 1;
        }
        if (result == "") {
            result = absolute ? "/" : "";
        } else if (absolute) {
            result = "/" + result;
        }
        return result;
    }

    // ── 工作区作用域 FS 门控（read / list / write）──

    /// <summary>读文件全文（逃逸拒绝 / 文件不存在 → 返回空串）。异步优先。
    /// 失败返回空串（非 null，default 语义）：Arc typeck 无法在调用方消费 Task&lt;string?&gt;
    /// 的 await 结果（mangle 失配 expected string?, found Nullable_string），故以空串承载
    /// 「无内容」语义（AG-11 允许的 default/空对象改语义）。</summary>
    public async Task<string> ReadAllTextAsync(string path) {
        string rp = this.ResolvePath(path);
        if (rp == null) {
            return "";
        }
        if (!File.Exists(rp)) {
            return "";
        }
        // 编译器已修复 `return await` 返回值传播（B3）：Task 泛型名
        // 还原为 TypeId::Task，await 结果取 X 类型，直接 return await 即可。
        return await File.ReadAllTextAsync(rp);
    }

    /// <summary>
    /// 原子写文件（仅 ReadWrite 模式；逃逸拒绝；创建/覆盖根内文件）。
    /// 实现：先写同目录唯一临时文件（staging），再 move 覆盖目标。
    ///   - 同目录保证同文件系统 → rename 语义下目标原子替换：进程崩溃时目标文件
    ///     保持「旧内容」或「新内容」，绝不出现半写脏文件。
    ///   - 临时名含时间戳唯一后缀：多会话/多任务并发写同一目标不互踩 staging。
    ///   - 目标已存在时由 Move 覆盖（不预删目标：Move 失败时原文件保持完好）。
    /// </summary>
    public async Task<bool> WriteAllTextAsync(string path, string content) {
        if (!this.CanWrite()) { return false; }
        string rp = this.ResolvePath(path);
        if (rp == null) { return false; }
        string dir = Path.GetDirectoryName(rp);
        if (dir != "") {
            bool dirExists = await Directory.ExistsAsync(dir);
            if (!dirExists) {
                bool created = await Directory.CreateDirectoryAsync(dir);
                if (!created) { return false; }
            }
        }
        string staging = this.StagingPath(rp);
        bool ok = await File.WriteAllTextAsync(staging, content != null ? content : "");
        if (!ok) {
            bool stagingExists = await File.ExistsAsync(staging);
            if (stagingExists) { await File.DeleteAsync(staging); }
            return false;
        }
        // 原子替换：直接 Move 覆盖目标（rt_file_move 走 rename——POSIX 原子替换；Windows
        // rename 目标已存在时退化为 copy+delete 覆盖）。不预删目标：Move 失败时原文件
        // 保持完好，杜绝「删了却没换成」的数据丢失。
        bool moved = await File.MoveAsync(staging, rp);
        if (!moved) {
            bool stagingExists2 = await File.ExistsAsync(staging);
            if (stagingExists2) { await File.DeleteAsync(staging); }
            return false;
        }
        return true;
    }

    /// <summary>生成目标同目录的唯一临时路径（staging）。时间戳后缀保证并发唯一。</summary>
    private string StagingPath(string target) {
        DateTime now = DateTime.Now;
        long ticks = now.Ticks;
        return target + ".tmp-" + ("" + ticks);
    }

    /// <summary>枚举根内目录（非递归；逃逸拒绝 → 空数组）。目录 I/O 经 Task.Run 隔离。
    /// 返回空数组（非 null）：逃逸或目录不存在时为空；解析器不支持泛型内可空数组
    /// （Task&lt;string[]?&gt;），故以空数组承载「无结果」语义（AG-11 允许的 default/空对象改语义）。</summary>
    public async Task<string[]> ListFilesAsync(string path) {
        string rp = this.ResolvePath(path);
        if (rp == null) {
            string[] empty = [];
            return empty;
        }
        if (!Directory.Exists(rp)) {
            string[] none = [];
            return none;
        }
        // 目录枚举走原生异步 ABI（Directory.GetFilesAsync），不阻塞调用线程。
        return await Directory.GetFilesAsync(rp);
    }

    /// <summary>判断根内文件是否存在（逃逸拒绝 → false）。轻量同步属性查询。</summary>
    public bool FileExists(string path) {
        string rp = this.ResolvePath(path);
        if (rp == null) {
            return false;
        }
        return File.Exists(rp);
    }
}