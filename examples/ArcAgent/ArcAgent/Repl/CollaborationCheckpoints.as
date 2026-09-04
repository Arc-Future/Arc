// CollaborationCheckpoints —— 协作确认点检测（RFC 043 references/collaboration-checkpoints）。
// 高风险 = 高沟通价值决策点：改 public API/ABI/std、删除/重构核心模块 → D7 强确认。
// 机器自判能否决策：平凡改动（非 public / 新增文件 / 修复编译）不触发，无需人确认。
namespace ArcAgent.Repl;
using Arc;
using Arc.Agent;
using Arc.Diagnostics;
using ArcAgent.Process;
using ArcAgent.Workspace;

/// <summary>协作确认点检测：从计划步骤文件声明 + 工作区 git 状态提取高风险面（静态只读）。</summary>
public static class CollaborationCheckpoints {
    /// <summary>检测高风险面；返回确认点清单（空 = 无高风险，D7 走普通一次确认）。</summary>
    public static async Task<List<string>> DetectAsync(AgentWorkspace workspace, AIPlan plan) {
        List<string> flags = new List<string>();
        // 1) 计划步骤文件声明面：std/ 前缀 → public API/ABI/std 影响。
        if (plan != null && plan.Steps != null) {
            int i = 0;
            int n = plan.Steps.Count;
            while (i < n) {
                string files = plan.Steps[i].Files;
                if (files != null && files != "") {
                    string[] parts = files.Split(",");
                    int j = 0;
                    while (j < parts.Length) {
                        string f = parts[j].Trim();
                        if (f != "" && CollaborationCheckpoints.IsStdPath(f)) {
                            flags.Add("改 public API/ABI/std：步骤 " + (i + 1) + " 声明 " + f);
                        }
                        j = j + 1;
                    }
                }
                i = i + 1;
            }
        }
        // 2) 工作区 git 状态面：std/ 改动 → ABI 影响；核心路径删除 → 职责迁移意图确认。
        if (workspace != null) {
            string status = await CollaborationCheckpoints.GitStatusAsync(workspace);
            if (status != null && status != "") {
                string[] lines = status.Split("\n");
                int k = 0;
                while (k < lines.Length) {
                    string line = lines[k];
                    if (line != null && line.Length >= 4) {
                        string xy = line.Substring(0, 2);
                        string path = line.Substring(3).Trim();
                        if (xy.IndexOf("D") >= 0 && CollaborationCheckpoints.IsCorePath(path)) {
                            flags.Add("删除/重构核心模块：" + path);
                        } else if (path.StartsWith("std/") && xy.IndexOf("D") < 0) {
                            flags.Add("改 public API/ABI/std：" + path);
                        }
                    }
                    k = k + 1;
                }
            }
        }
        return flags;
    }

    private static bool IsStdPath(string path) {
        if (path == null) {
            return false;
        }
        return path == "std" || path.StartsWith("std/") || path.IndexOf("/std/") >= 0;
    }

    private static bool IsCorePath(string path) {
        if (path == null) {
            return false;
        }
        return path.StartsWith("crates/") || path.StartsWith("std/") || path.StartsWith("stdlib/");
    }

    private static async Task<string> GitStatusAsync(AgentWorkspace workspace) {
        try {
            ProcessRunResult r = await ProcessRunner.RunAsync("git -C \"" + workspace.Root + "\" status --porcelain");
            if (r == null || r.ExitCode != 0) {
                return "";
            }
            return r.StandardOutput != null ? r.StandardOutput : "";
        } catch (Exception) {
            return "";
        }
    }
}
