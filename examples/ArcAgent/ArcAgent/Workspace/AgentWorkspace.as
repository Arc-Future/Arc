// AgentWorkspace —— 工作区管理：目标仓库根 + AIWorkspace 沙箱 + git 状态。
//
// 职责边界（分层：Workspace 层）：
//   - Root：目标仓库根目录（REPL 启动时指定；模型操作边界）。
//   - Sandbox：AIWorkspace 文件作用域沙箱（逃逸拒绝 / 读写分权）。
//   - GitBranch / Describe：只读 git 状态摘要，注入系统指令告知模型工作区边界。
namespace ArcAgent.Workspace;
using Arc;
using Arc.Agent;
using Arc.Diagnostics;
using Arc.IO;
using ArcAgent.Process;

/// <summary>工作区：目标仓库根 + 文件沙箱 + git 状态摘要（模型操作边界）。</summary>
public class AgentWorkspace {
    private string _root;
    private AIWorkspace _sandbox;

    public AgentWorkspace(string root, AIWorkspaceAccess mode) {
        _root = root != null ? root : "";
        _sandbox = new AIWorkspace(_root, mode);
    }

    /// <summary>目标仓库根目录（规范化绝对路径）。</summary>
    public string Root {
        get { return _root; }
    }

    /// <summary>文件作用域沙箱（根内读写；逃逸拒绝）。</summary>
    public AIWorkspace Sandbox {
        get { return _sandbox; }
    }

    /// <summary>解析并校验工作区：目录须存在；返回 ReadWrite 工作区实例。</summary>
    public static AgentWorkspace Resolve(string root) {
        string r = root != null ? root : "";
        if (r == "" || !Directory.Exists(r)) {
            throw new ArgumentException("工作区目录不存在：" + r);
        }
        return new AgentWorkspace(r, AIWorkspaceAccess.ReadWrite);
    }

    /// <summary>git 当前分支（非 git 仓库返回 null；只读查询）。</summary>
    public async Task<string> GitBranchAsync() {
        ProcessRunResult r = await this.RunGitAsync("branch --show-current");
        if (r == null || r.ExitCode != 0) {
            return null;
        }
        string b = r.StandardOutput != null ? r.StandardOutput.Trim() : "";
        return b != "" ? b : null;
    }

    /// <summary>工作区描述（注入系统指令；告知模型操作边界与仓库状态）。</summary>
    public async Task<string> DescribeAsync() {
        string s = "Workspace root: " + _root;
        string branch = await this.GitBranchAsync();
        if (branch != null) {
            s = s + " (git branch: " + branch + ")";
        } else {
            s = s + " (not a git repository)";
        }
        return s;
    }

    private async Task<ProcessRunResult> RunGitAsync(string args) {
        return await ProcessRunner.RunAsync("git -C \"" + _root + "\" " + args);
    }
}
