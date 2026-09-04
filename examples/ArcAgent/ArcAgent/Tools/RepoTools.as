// RepoTools —— 仓库级工具：递归搜索 + git 只读查询（fs.Read；即时执行）。
//
// 职责边界（分层：Tools 层能力面）：
//   - grep_search：目录递归关键词搜索（read_file 是单文件，grep 是跨文件代码库探索）。
//   - git_status / git_diff：只读 git 状态（编码智能体常用；不经 run_command 审批）。
namespace ArcAgent.Tools;
using Arc;
using Arc.Agent;
using Arc.ComponentModel;
using Arc.Diagnostics;
using Arc.IO;
using ArcAgent.Process;

/// <summary>仓库级工具集（grep 递归搜索 / git 只读查询）。</summary>
public class RepoTools {
    private const int MaxGrepHits = 200;

    /// <summary>递归搜索目录下全部文件中的关键词，返回 "path:lineno: text"（上限 200 条）。</summary>
    [Description("Recursively search all files under a directory for a keyword. Returns 'path:lineno: text' lines, capped at 200.")]
    [AITool("grep_search", Capability = "fs.Read")]
    public async Task<string> GrepSearchAsync(
        [Description("Root directory to search recursively.")] string root,
        [Description("Plain substring keyword to match.")] string keyword) {
        if (root == "" || keyword == "" || !Directory.Exists(root)) {
            return "grep_search: invalid root or empty keyword";
        }
        List<string> hits = new List<string>();
        await Task.Run(() => {
            this.GrepDir(root, keyword, hits);
        });
        if (hits.Count == 0) {
            return "(no matches)";
        }
        string result = "";
        foreach (var hit in hits) {
            if (result != "") {
                result = result + "\n";
            }
            result = result + hit;
        }
        return result;
    }

    private void GrepDir(string dir, string keyword, List<string> hits) {
        string[] files = Directory.GetFiles(dir);
        int fi = 0;
        while (fi < files.Length && hits.Count < RepoTools.MaxGrepHits) {
            string text = File.ReadAllText(files[fi]);
            if (text != null) {
                string[] lines = text.Split("\n");
                int ln = lines.Length;
                int li = 0;
                while (li < ln && hits.Count < RepoTools.MaxGrepHits) {
                    if (lines[li].Contains(keyword)) {
                        hits.Add(files[fi] + ":" + (li + 1) + ": " + lines[li]);
                    }
                    li = li + 1;
                }
            }
            fi = fi + 1;
        }
        string[] dirs = Directory.GetDirectories(dir);
        int di = 0;
        while (di < dirs.Length && hits.Count < RepoTools.MaxGrepHits) {
            this.GrepDir(dirs[di], keyword, hits);
            di = di + 1;
        }
    }

    /// <summary>git 工作区状态（short 格式；只读）。</summary>
    [Description("Show git working tree status in short format. Pass the repository root path.")]
    [AITool("git_status", Capability = "fs.Read")]
    public async Task<string> GitStatusAsync(
        [Description("Absolute path of the git repository root.")] string repo) {
        if (repo == "" || !Directory.Exists(repo)) {
            return "git_status: invalid repo path";
        }
        ProcessRunResult r = await ProcessRunner.RunAsync("git -C \"" + repo + "\" status --short");
        if (r == null) {
            return "git_status: command failed";
        }
        string outText = r.StandardOutput != null ? r.StandardOutput : "";
        return "exit=" + r.ExitCode + (outText != "" ? ("\n" + outText) : "");
    }

    /// <summary>git 工作区差异（未暂存改动；只读）。</summary>
    [Description("Show unstaged git diff of working tree changes.")]
    [AITool("git_diff", Capability = "fs.Read")]
    public async Task<string> GitDiffAsync(
        [Description("Absolute path of the git repository root.")] string repo) {
        if (repo == "" || !Directory.Exists(repo)) {
            return "git_diff: invalid repo path";
        }
        ProcessRunResult r = await ProcessRunner.RunAsync("git -C \"" + repo + "\" diff");
        if (r == null) {
            return "git_diff: command failed";
        }
        string outText = r.StandardOutput != null ? r.StandardOutput : "";
        return "exit=" + r.ExitCode + (outText != "" ? ("\n" + outText) : "");
    }
}
