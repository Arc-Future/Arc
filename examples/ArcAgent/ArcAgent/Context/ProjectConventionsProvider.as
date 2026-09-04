// ProjectConventionsProvider —— 自定义上下文源：读 .arcagent/conventions.md 注入 Rules 层。
//
// 演示 AIContextProvider 扩展点（RFC 038 上下文工程）：静态源，首轮构建时读约定文件，
// 产出 Rules 层上下文块；调用后无副作用（继承默认空实现）。文件不存在 → 无贡献。
namespace ArcAgent.Context;
using Arc;
using Arc.Agent;
using Arc.Collections;
using Arc.IO;

/// <summary>项目约定上下文源（.arcagent/conventions.md → Rules 层）。</summary>
public class ProjectConventionsProvider : AIContextProvider {
    private string _file;

    public ProjectConventionsProvider(string workspaceRoot) {
        _file = "";
        if (workspaceRoot != null && workspaceRoot != "") {
            _file = workspaceRoot + "/.arcagent/conventions.md";
        }
    }

    public override string GetName() {
        return "project.conventions";
    }

    public override string GetDescription() {
        return "项目编码约定（.arcagent/conventions.md）";
    }

    public override int GetPriority() {
        return 0;
    }

    public override async Task<List<AIContextBlock>> ProvideContextAsync(
        AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> blocks = new List<AIContextBlock>();
        if (_file != "" && File.Exists(_file)) {
            string text = await File.ReadAllTextAsync(_file);
            if (text != null && text != "") {
                AIContextBlock blk = new AIContextBlock(
                    this.GetName(),
                    "Rules",
                    0,
                    "Project conventions (must follow when modifying code):\n" + text);
                blocks.Add(blk);
            }
        }
        return blocks;
    }
}
