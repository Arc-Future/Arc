// RFC 043 H-2b：quality.* 验证器工具 — 归属 Coding；声明式 [AITool]，只读、不进 plan gate。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Agent;
using Arc.ComponentModel;

/// <summary>Coding 质量验证工具集（D0/D1/D3 信号源）。</summary>
public class QualityTools {
    /// <summary>D0 编译门：`arc build`（退出码 + stderr 文本；`--message-format json` 未实现，勿依赖）。</summary>
    [Description("Run arc build on a project path. Use for D0 compile gate; verdict is the process exit code.")]
    [AITool("arc_build", Capability = "quality.Verify")]
    public async Task<string> ArcBuildAsync(
        [Description("Project path or .as entry (relative to cwd).")] string project) {
        string target = project != null && project != "" ? project : ".";
        return await QualityCli.RunArcAsync(
            "build " + Quote(target), new CancellationToken());
    }

    /// <summary>D3 行为验证门：`arc test`。</summary>
    [Description("Run arc test on a project path. Use for D3 behavior gate.")]
    [AITool("arc_test", Capability = "quality.Verify")]
    public async Task<string> ArcTestAsync(
        [Description("Test project path.")] string project) {
        string target = project != null && project != "" ? project : ".";
        return await QualityCli.RunArcAsync("test " + Quote(target), new CancellationToken());
    }

    /// <summary>D0 快速反馈：`arc check`（typeck + borrowck，不 codegen）。</summary>
    [Description("Run arc check on a source file (typeck+borrowck, no codegen). Fast CI/Agent feedback.")]
    [AITool("arc_check", Capability = "quality.Verify")]
    public async Task<string> ArcCheckAsync(
        [Description("Source .as file to check.")] string file) {
        if (file == null || file == "") {
            throw new ArgumentException("file is empty");
        }
        return await QualityCli.RunArcAsync("check " + Quote(file), new CancellationToken());
    }

    /// <summary>D1 语义索引：`arc inspect` 源码模式（--format json 机器可读 symbols/edges/可达性）。</summary>
    [Description("Inspect a source file into a .arcgr semantic index (source mode). Use for D1 semantic gate; --format json returns machine-readable symbols/edges/reachability.")]
    [AITool("arc_inspect", Capability = "quality.Verify")]
    public async Task<string> ArcInspectAsync(
        [Description("Source .as file to inspect.")] string file,
        [Description("Output format: human (default) or json.")] string format) {
        if (file == null || file == "") {
            throw new ArgumentException("file is empty");
        }
        string fmt = format != null && format != "" ? format : "human";
        return await QualityCli.RunArcAsync(
            "inspect " + Quote(file) + " --format " + fmt, new CancellationToken());
    }

    /// <summary>D1 语义查询：`arc query|locate|explain` 经 .arcgr。</summary>
    [Description("Query .arcgr semantics. kind=callers|callees|impls|references|locate|explain. Requires arcgr path and symbol.")]
    [AITool("arcgr_query", Capability = "quality.Verify")]
    public async Task<string> ArcgrQueryAsync(
        [Description("Query kind: callers|callees|impls|references|locate|explain.")] string kind,
        [Description("Path to .arcgr file.")] string arcgr,
        [Description("Symbol name to query.")] string symbol) {
        if (arcgr == null || arcgr == "" || symbol == null || symbol == "") {
            throw new ArgumentException("arcgr and symbol are required");
        }
        string k = kind != null && kind != "" ? kind : "references";
        string args = "";
        if (k == "locate") {
            args = "locate " + Quote(arcgr) + " " + Quote(symbol) + " --format json";
        } else if (k == "explain") {
            args = "explain " + Quote(arcgr) + " " + Quote(symbol) + " --format json";
        } else {
            args = "query " + k + " " + Quote(arcgr) + " " + Quote(symbol) + " --format json";
        }
        return await QualityCli.RunArcAsync(args, new CancellationToken());
    }

    private static string Quote(string path) {
        if (path == null) {
            return "\"\"";
        }
        if (path.IndexOf(" ") >= 0) {
            return "\"" + path + "\"";
        }
        return path;
    }
}
