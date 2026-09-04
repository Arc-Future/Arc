// 领域二（ReviewAgent）：文档审查领域共享判定逻辑 — [AITool] 与 DoD evaluator 同源复用，
// 不焊进基座、不依赖 Coding 领域。
namespace ReviewAgent.Tools;
using Arc;
using Arc.Collections;
using Arc.IO;

/// <summary>目录扫描结果：文档集 + 空文档 + 交叉引用断链。</summary>
public class ReviewScanResult {
    public List<string> Documents;
    public List<string> EmptyDocs;
    public List<string> BrokenLinks;
    public int LinkCount;

    public ReviewScanResult() {
        this.Documents = new List<string>();
        this.EmptyDocs = new List<string>();
        this.BrokenLinks = new List<string>();
        this.LinkCount = 0;
    }

    /// <summary>折叠为决策面文本（Signal/Detail 回喂模型）。</summary>
    public string Describe() {
        string s = "docs=" + this.Documents.Count
            + ", links=" + this.LinkCount
            + ", broken=" + this.BrokenLinks.Count;
        int i = 0;
        while (i < this.BrokenLinks.Count && i < 5) {
            s = s + "\n  " + this.BrokenLinks[i];
            i = i + 1;
        }
        if (this.BrokenLinks.Count > 5) {
            s = s + "\n  ... +" + (this.BrokenLinks.Count - 5) + " more";
        }
        return s;
    }
}

/// <summary>文档审查领域逻辑：递归收集 .md 文档、空文档检测、交叉引用一致性检查（可证伪、不空扫）。</summary>
/// <remarks>Arc 编译器暂不支持 static class 持有字段，故以常规类 + 私有构造承载静态成员（std 惯例）。</remarks>
public class ReviewChecks {
    private const string MarkdownExt = "*.md";

    private ReviewChecks() {
    }

    /// <summary>
    /// 扫描目录：递归收集 .md 文档（跳过 obj/bin/target/.git/.arcagent）；逐文档提取
    /// [text](target) 交叉引用并校验目标文件存在。目录不存在 → 空结果（数据不足，调用方定 Pending/Failed）。
    /// </summary>
    public static ReviewScanResult ScanFolder(string folder) {
        ReviewScanResult result = new ReviewScanResult();
        string dir = folder != null && folder != "" ? folder : ".";
        if (!Directory.Exists(dir)) {
            return result;
        }
        List<string> docs = new List<string>();
        ReviewChecks.CollectMarkdown(dir, 0, docs);
        result.Documents = docs;
        int i = 0;
        while (i < docs.Count) {
            string path = docs[i];
            if (ReviewChecks.IsEmptyFile(path)) {
                result.EmptyDocs.Add(path);
            }
            ReviewChecks.CollectLinks(path, result);
            i = i + 1;
        }
        return result;
    }

    /// <summary>单文件审查：行数 + TODO/FIXME 标记（review_file 工具信号）。</summary>
    public static string ReviewFileText(string file) {
        if (file == null || file == "") {
            return "error: file path is empty";
        }
        if (!File.Exists(file)) {
            return "error: file not found: " + file;
        }
        string[] lines = File.ReadAllLines(file);
        int markers = 0;
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i] : "";
            if (line.IndexOf("TODO") >= 0 || line.IndexOf("FIXME") >= 0) {
                markers = markers + 1;
            }
            i = i + 1;
        }
        return "file " + file
            + "\n  lines=" + lines.Length
            + ", todo_markers=" + markers;
    }

    /// <summary>文档是否无实质内容（全部为空/空白行）。</summary>
    public static bool IsEmptyFile(string path) {
        string[] lines = File.ReadAllLines(path);
        if (lines == null || lines.Length == 0) {
            return true;
        }
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i] : "";
            if (ReviewChecks.Trim(line) != "") {
                return false;
            }
            i = i + 1;
        }
        return true;
    }

    private static void CollectMarkdown(string dir, int depth, List<string> outList) {
        if (depth > 10) {
            return;
        }
        string[] files = Directory.GetFiles(dir, ReviewChecks.MarkdownExt);
        if (files != null) {
            int i = 0;
            while (i < files.Length) {
                outList.Add(files[i]);
                i = i + 1;
            }
        }
        string[] dirs = Directory.GetDirectories(dir);
        if (dirs != null) {
            int j = 0;
            while (j < dirs.Length) {
                string name = Path.GetFileName(dirs[j]);
                if (name != "obj" && name != "bin" && name != "target" && name != ".git" && name != ".arcagent") {
                    ReviewChecks.CollectMarkdown(dirs[j], depth + 1, outList);
                }
                j = j + 1;
            }
        }
    }

    private static void CollectLinks(string path, ReviewScanResult result) {
        string[] lines = File.ReadAllLines(path);
        string dir = Path.GetDirectoryName(path);
        if (dir == null || dir == "") {
            dir = ".";
        }
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i] : "";
            ReviewChecks.CollectLinksInLine(line, path, dir, result);
            i = i + 1;
        }
    }

    private static void CollectLinksInLine(string line, string docPath, string docDir, ReviewScanResult result) {
        int idx = line.IndexOf("](");
        while (idx >= 0) {
            int close = line.IndexOf(")", idx + 2);
            if (close < 0) {
                break;
            }
            string target = line.Substring(idx + 2, close - (idx + 2));
            if (!ReviewChecks.IsExternalLink(target)) {
                result.LinkCount = result.LinkCount + 1;
                string resolved = ReviewChecks.ResolveTarget(target, docDir);
                if (resolved != "" && !File.Exists(resolved)) {
                    result.BrokenLinks.Add(docPath + " → " + target);
                }
            }
            int next = line.IndexOf("](", close + 1);
            if (next < 0 || next <= idx) {
                break;
            }
            idx = next;
        }
    }

    private static bool IsExternalLink(string target) {
        return target.StartsWith("http://")
            || target.StartsWith("https://")
            || target.StartsWith("mailto:")
            || target.StartsWith("#");
    }

    /// <summary>解析链接目标为候选文件路径；锚点/空目标 → ""（不计入）。</summary>
    private static string ResolveTarget(string target, string docDir) {
        string t = target != null ? target : "";
        int hash = t.IndexOf("#");
        if (hash >= 0) {
            t = t.Substring(0, hash);
        }
        if (ReviewChecks.Trim(t) == "") {
            return "";
        }
        if (t.StartsWith("/")) {
            return t;
        }
        return Path.Combine(docDir, t);
    }

    private static string Trim(string s) {
        if (s == null) {
            return "";
        }
        int i = 0;
        int n = s.Length;
        while (i < n && ReviewChecks.IsSpace(s.Substring(i, 1))) {
            i = i + 1;
        }
        int j = n;
        while (j > i && ReviewChecks.IsSpace(s.Substring(j - 1, 1))) {
            j = j - 1;
        }
        return s.Substring(i, j - i);
    }

    private static bool IsSpace(string ch) {
        return ch == " " || ch == "\t" || ch == "\r" || ch == "\n";
    }
}
