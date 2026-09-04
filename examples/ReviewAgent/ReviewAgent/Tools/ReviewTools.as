// 领域二（ReviewAgent）：文档审查工具集 — 声明式 [AITool]，只读、能力 review.Run，走 AIHost 自动装配。
namespace ReviewAgent.Tools;
using Arc;
using Arc.Agent;
using Arc.ComponentModel;

/// <summary>文档审查领域工具集（[AITool] 编译期贡献；不进计划门闩）。</summary>
public class ReviewTools {
    /// <summary>单文档审查：行数 + TODO/FIXME 标记。</summary>
    [Description("Review a single document file: reports line count and TODO/FIXME markers.")]
    [AITool("review_file", Capability = "review.Run")]
    public string ReviewFile(
        [Description("Path to the document file to review.")] string file) {
        return ReviewChecks.ReviewFileText(file);
    }

    /// <summary>目录级交叉引用一致性检查：文档集 + 链接数 + 断链清单。</summary>
    [Description("Check cross-reference consistency across markdown documents in a folder: reports doc set, link count and broken links.")]
    [AITool("check_consistency", Capability = "review.Run")]
    public string CheckConsistency(
        [Description("Folder path containing markdown documents.")] string folder) {
        ReviewScanResult scan = ReviewChecks.ScanFolder(folder);
        return scan.Describe();
    }
}
