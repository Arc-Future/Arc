// FSTools —— 文件系统工具（fs.Read 只读即时执行；fs.Write 触发 HITL 门闩）。
//
// RFC 038 §5.1.3：工具按职责范围分类（FS 一组、shell 一组），每类一文件。
// 声明式 [AITool]：编译期合成 __AIToolHost（用户无感），经 AIHost 默认工具源自动装配；
// 真实生效由 AICapabilitySet 白名单 fail-closed 授权。
//
// 异步契约（RFC 038 §3.1.1）：所有工具统一返回 Task<string>；无原生异步的
// Directory/Delete 等同步 API 以 Task.Run 后台线程包装，值捕获用 by-ref string 载体。
namespace ArcAgent.Tools;
using Arc;
using Arc.Agent;
using Arc.ComponentModel;
using Arc.IO;

/// <summary>文件系统工具集（read / list / search / write / copy / delete）。</summary>
public class FSTools {
    /// <summary>读取文件全文（fs.Read）。</summary>
    [Description("Read the full text content of a file. Returns the raw file text.")]
    [AITool("read_file", Capability = "fs.Read")]
    public async Task<string> ReadFileAsync([Description("Absolute path of the file to read.")] string path) {
        if (path == "" || !File.Exists(path)) {
            throw new Exception("file not found: " + path);
        }
        return await File.ReadAllTextAsync(path);
    }

    /// <summary>枚举目录（文件 + 子目录，非递归；fs.Read）。</summary>
    [Description("List files and subdirectories (non-recursive) in a directory. Returns one entry per line, 'D ' prefix for directories and 'F ' for files.")]
    [AITool("list_dir", Capability = "fs.Read")]
    public async Task<string> ListDirAsync([Description("Absolute path of the directory to enumerate.")] string path) {
        if (path == "" || !Directory.Exists(path)) {
            throw new Exception("dir not found: " + path);
        }
        List<string> entries = new List<string>();
        await Task.Run(() => {
            string[] dirs = Directory.GetDirectories(path);
            int d = 0;
            while (d < dirs.Length) {
                entries.Add("D " + dirs[d]);
                d = d + 1;
            }
            string[] files = Directory.GetFiles(path);
            int f = 0;
            while (f < files.Length) {
                entries.Add("F " + files[f]);
                f = f + 1;
            }
        });
        if (entries.Count == 0) {
            return "(empty)";
        }
        string result = "";
        foreach (var entry in entries) {
            if (result != "") {
                result = result + "\n";
            }
            result = result + entry;
        }
        return result;
    }

    /// <summary>在文件中按关键词搜索，返回匹配行号与内容（fs.Read）。</summary>
    [Description("Search a file for lines containing the given keyword. Returns matching 'lineno: text' lines (1-based), or '(no matches)'.")]
    [AITool("search_text", Capability = "fs.Read")]
    public async Task<string> SearchTextAsync(
        [Description("Absolute path of the file to search.")] string path,
        [Description("Keyword (plain substring) to find in the file.")] string pattern) {
        if (path == "" || !File.Exists(path)) {
            throw new Exception("file not found: " + path);
        }
        string text = await File.ReadAllTextAsync(path);
        string[] lines = text.Split("\n");
        string result = "";
        int n = 0;
        while (n < lines.Length) {
            if (lines[n].Contains(pattern)) {
                if (result != "") {
                    result = result + "\n";
                }
                result = result + (n + 1) + ": " + lines[n];
            }
            n = n + 1;
        }
        if (result == "") {
            result = "(no matches)";
        }
        return result;
    }

    /// <summary>覆盖写入文件（fs.Write；RequireApproval=true 触发 HITL）。</summary>
    [Description("Overwrite a file with the given content. Requires human approval.")]
    [AITool("write_file", Capability = "fs.Write", RequireApproval = true)]
    public async Task<string> WriteFileAsync([Description("Absolute path of the file to write.")] string path, string content) {
        bool ok = await File.WriteAllTextAsync(path, content);
        if (!ok) {
            throw new Exception("failed to write: " + path);
        }
        return "wrote " + content.Length + " chars to " + path;
    }

    /// <summary>复制文件（fs.Write；RequireApproval）。</summary>
    [Description("Copy a file from src to dst. Requires human approval.")]
    [AITool("copy_file", Capability = "fs.Write", RequireApproval = true)]
    public async Task<string> CopyFileAsync(
        [Description("Absolute path of the source file.")] string src,
        [Description("Absolute path of the destination file.")] string dst) {
        if (src == "" || !File.Exists(src)) {
            throw new Exception("source file not found: " + src);
        }
        bool ok = await File.CopyAsync(src, dst);
        if (!ok) {
            throw new Exception("failed to copy: " + src + " -> " + dst);
        }
        return "copied " + src + " -> " + dst;
    }

    /// <summary>删除文件（fs.Write；RequireApproval）。</summary>
    [Description("Delete a file. Requires human approval.")]
    [AITool("delete_file", Capability = "fs.Write", RequireApproval = true)]
    public async Task<string> DeleteFileAsync([Description("Absolute path of the file to delete.")] string path) {
        if (path == "" || !File.Exists(path)) {
            throw new Exception("file not found: " + path);
        }
        await Task.Run(() => {
            bool ok = File.Delete(path);
            if (!ok) { throw new Exception("failed to delete: " + path); }
        });
        return "deleted " + path;
    }

    /// <summary>定点编辑文件（fs.Write；RequireApproval）：old_text 须在文件中唯一出现，否则报错提示加上下文。</summary>
    [Description("Replace an exact text fragment in a file. old_text must appear exactly once in the file, otherwise an error is returned; new_text is the replacement. Use write_file for whole-file overwrite.")]
    [AITool("edit_file", Capability = "fs.Write", RequireApproval = true)]
    public async Task<string> EditFileAsync(
        [Description("Absolute path of the file to edit.")] string path,
        [Description("Exact text fragment to replace (must appear exactly once).")] string oldText,
        [Description("Replacement text for the fragment.")] string newText) {
        if (path == "" || !File.Exists(path)) {
            throw new Exception("file not found: " + path);
        }
        if (oldText == null || oldText == "") {
            throw new Exception("edit_file: old_text is empty");
        }
        string text = await File.ReadAllTextAsync(path);
        int count = this.CountOccurrences(text, oldText);
        if (count == 0) {
            throw new Exception("edit_file: old_text not found in " + path);
        }
        if (count > 1) {
            throw new Exception("edit_file: old_text appears " + count + " times; add surrounding context to disambiguate");
        }
        string updated = text.Replace(oldText, newText);
        bool ok = await File.WriteAllTextAsync(path, updated);
        if (!ok) {
            throw new Exception("failed to write: " + path);
        }
        return "edited " + path + " (" + oldText.Length + " -> " + newText.Length + " chars)";
    }

    /// <summary>统计子串在文本中的非重叠出现次数。</summary>
    private static int CountOccurrences(string text, string fragment) {
        int count = 0;
        int pos = 0;
        while (true) {
            int found = text.IndexOf(fragment, pos);
            if (found < 0) {
                break;
            }
            count = count + 1;
            pos = found + fragment.Length;
        }
        return count;
    }
}
