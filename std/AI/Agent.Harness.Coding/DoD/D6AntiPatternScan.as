// RFC 043 P2：D6 反模式可查项扫描 — 源码级、确定性、可判红。
// RFC 043 P1（B4 声明=行为扫描）：追加「疑似死代码 + 宣称待证」源码级静态扫描
//（咨询信号，不判红，回喂人/模型复核）。
//
// 反模式最小集（可机器查、不造假）：
//   1. 占位壳：`NotImplemented` / `NotImplementedException` / `todo!()` — 命中判红；
//   2. 未完成标记：`TODO` / `FIXME` 注释（`//` / `/*` 注释内）— 命中判红。
// 判定信号 = 命中清单（path:line: marker）；无命中 = 通过；无 `.as` 源文件 =
// 数据不足 → 调用方返回 Pending（禁空扫 Passed）。
//
// B4「声明=行为」追加（咨询信号，不判红，只进 Describe/Advisories 回喂复核）：
//   3. 疑似死代码：`public` 类型/方法名在项目内文本引用 ≤1（仅声明处）→
//      DeadCodeSignals；标「疑似」——精确判定需 D1 `.arcgr` 引用图，不武断判红；
//   4. 宣称待证：宣称符号（默认 `--message-format json`，可经 ScanAsync 覆盖）仅
//      出现在注释、代码（非注释）零命中 → ClaimSignals（反向 grep 无实现）。
// 咨询信号不改变 D6 通过/失败判定（硬红仍只由反模式命中驱动）。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Text;

/// <summary>
/// D6 反模式扫描结果：扫描文件数 + 命中清单（path:line: marker）+
/// 咨询信号（疑似死代码 / 宣称待证）。
/// </summary>
public class D6AntiPatternScan {
    public int FileCount;
    public List<string> Hits;
    public List<string> DeadCodeSignals;
    public List<string> ClaimSignals;

    public D6AntiPatternScan() {
        this.FileCount = 0;
        this.Hits = new List<string>();
        this.DeadCodeSignals = new List<string>();
        this.ClaimSignals = new List<string>();
    }

    /// <summary>
    /// 扫描项目源码：文件直扫；目录递归收集 `*.as`（跳过 obj/bin/target/.git 等产物/元数据目录）。
    /// 命中记入 <see cref="Hits"/>；无源文件返回 FileCount=0（数据不足 → Pending）。
    /// 默认校验宣称符号清单（<see cref="DefaultClaimedSymbols"/>）。
    /// </summary>
    public static async Task<D6AntiPatternScan> ScanAsync(string projectOrFile, CancellationToken cancellationToken) {
        return await D6AntiPatternScan.ScanAsync(projectOrFile, D6AntiPatternScan.DefaultClaimedSymbols(), cancellationToken);
    }

    /// <summary>
    /// 扫描项目源码（显式宣称符号清单；null/空 → 跳过宣称扫描）。
    /// 在基础扫描上追加：①疑似死代码（public 符号零引用）；②宣称待证（宣称符号仅注释提及、代码零命中）。
    /// </summary>
    public static async Task<D6AntiPatternScan> ScanAsync(string projectOrFile, List<string> claimedSymbols, CancellationToken cancellationToken) {
        D6AntiPatternScan scan = new D6AntiPatternScan();
        string target = projectOrFile != null && projectOrFile != "" ? projectOrFile : ".";
        List<string> files = new List<string>();
        if (File.Exists(target)) {
            files.Add(target);
        } else if (Directory.Exists(target)) {
            D6AntiPatternScan.CollectSourceFiles(target, files, cancellationToken);
        }
        scan.FileCount = files.Count;
        string corpus = "";
        string codeCorpus = "";
        List<string> names = new List<string>();
        List<string> locations = new List<string>();
        int i = 0;
        while (i < files.Count) {
            cancellationToken.ThrowIfCancellationRequested();
            string[] lines = File.ReadAllLines(files[i]);
            int li = 0;
            while (li < lines.Length) {
                string line = lines[li] != null ? lines[li] : "";
                corpus = corpus + line + "\n";
                int c = D6AntiPatternScan.CommentStart(line);
                string code = c >= 0 ? line.Substring(0, c) : line;
                codeCorpus = codeCorpus + code + "\n";
                li = li + 1;
            }
            scan.ScanFile(files[i], lines);
            D6AntiPatternScan.CollectPublicSymbols(lines, files[i], names, locations);
            i = i + 1;
        }
        scan.ScanDeadCode(corpus, names, locations);
        scan.ScanClaims(codeCorpus, corpus, claimedSymbols);
        return scan;
    }

    /// <summary>判定摘要（Signal/Detail 回喂模型；含咨询信号计数）。</summary>
    public string Describe() {
        string s = "antipattern scan: " + this.FileCount + " files, " + this.Hits.Count + " hits";
        if (this.DeadCodeSignals.Count > 0) {
            s = s + ", " + this.DeadCodeSignals.Count + " dead-code";
        }
        if (this.ClaimSignals.Count > 0) {
            s = s + ", " + this.ClaimSignals.Count + " unverified-claims";
        }
        return s;
    }

    /// <summary>命中明细（每行一条；Detail 回喂模型定位修复）。</summary>
    public string Detail() {
        if (this.Hits.Count == 0) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = this.Hits.Count;
        while (i < n && i < 50) {
            if (i > 0) {
                sb.Append("; ");
            }
            sb.Append(this.Hits[i]);
            i = i + 1;
        }
        if (n > 50) {
            sb.Append("; ... and " + (n - 50) + " more");
        }
        return sb.ToString();
    }

    /// <summary>咨询信号明细（疑似死代码 + 宣称待证；无则空串）。</summary>
    public string Advisories() {
        if (this.DeadCodeSignals.Count == 0 && this.ClaimSignals.Count == 0) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        int i = 0;
        while (i < this.DeadCodeSignals.Count) {
            sb.Append(this.DeadCodeSignals[i]);
            sb.Append("\n");
            i = i + 1;
        }
        int j = 0;
        while (j < this.ClaimSignals.Count) {
            sb.Append(this.ClaimSignals[j]);
            sb.Append("\n");
            j = j + 1;
        }
        return sb.ToString();
    }

    private static List<string> DefaultClaimedSymbols() {
        List<string> claims = new List<string>();
        claims.Add("--message-format json");
        return claims;
    }

    private static void CollectSourceFiles(string dir, List<string> outFiles, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string[] files = Directory.GetFiles(dir, "*.as");
        int fi = 0;
        while (fi < files.Length) {
            outFiles.Add(files[fi]);
            fi = fi + 1;
        }
        string[] sub = Directory.GetDirectories(dir);
        int di = 0;
        while (di < sub.Length) {
            string name = Path.GetFileName(sub[di]);
            if (!D6AntiPatternScan.IsSkippableDir(name)) {
                D6AntiPatternScan.CollectSourceFiles(sub[di], outFiles, cancellationToken);
            }
            di = di + 1;
        }
    }

    private static bool IsSkippableDir(string name) {
        return name == ".git" || name == "obj" || name == "bin" || name == "target" || name == "node_modules";
    }

    private void ScanFile(string path, string[] lines) {
        int i = 0;
        while (i < lines.Length) {
            string line = lines[i] != null ? lines[i] : "";
            int lineNo = i + 1;
            if (line.IndexOf("NotImplemented") >= 0 || line.IndexOf("todo!()") >= 0) {
                this.AddHit(path, lineNo, "placeholder NotImplemented/todo!()");
            } else {
                int comment = D6AntiPatternScan.CommentStart(line);
                if (comment >= 0) {
                    string tail = line.Substring(comment);
                    if (tail.IndexOf("TODO") >= 0 || tail.IndexOf("FIXME") >= 0) {
                        this.AddHit(path, lineNo, "TODO/FIXME marker");
                    }
                }
            }
            i = i + 1;
        }
    }

    /// <summary>收集 public 类型/方法声明名（供死代码计数）。</summary>
    private static void CollectPublicSymbols(string[] lines, string path, List<string> names, List<string> locations) {
        int i = 0;
        while (i < lines.Length) {
            string raw = lines[i] != null ? lines[i] : "";
            string trimmed = D6AntiPatternScan.TrimLeft(raw);
            if (trimmed.StartsWith("public ")) {
                string name = D6AntiPatternScan.TypeName(trimmed);
                if (name == "") {
                    name = D6AntiPatternScan.MethodName(trimmed);
                }
                if (name != "" && name != "Main") {
                    names.Add(name);
                    locations.Add(path + ":" + (i + 1));
                }
            }
            i = i + 1;
        }
    }

    private void ScanDeadCode(string corpus, List<string> names, List<string> locations) {
        int i = 0;
        while (i < names.Count) {
            string name = names[i];
            int count = D6AntiPatternScan.CountIdentOccurrences(corpus, name);
            if (count <= 1) {
                this.DeadCodeSignals.Add("疑似死代码: " + locations[i] + " " + name + " (零引用)");
            }
            i = i + 1;
        }
    }

    private void ScanClaims(string codeCorpus, string corpus, List<string> claimedSymbols) {
        if (claimedSymbols == null || claimedSymbols.Count == 0) {
            return;
        }
        int i = 0;
        while (i < claimedSymbols.Count) {
            string symbol = claimedSymbols[i];
            if (symbol != null && symbol != "" && corpus.IndexOf(symbol) >= 0 && codeCorpus.IndexOf(symbol) < 0) {
                this.ClaimSignals.Add("宣称待证: " + symbol + "（仅注释提及，代码无实现）");
            }
            i = i + 1;
        }
    }

    /// <summary>统计标识符在语料中的文本出现次数（按标识符边界，避免子串误配）。</summary>
    private static int CountIdentOccurrences(string corpus, string name) {
        int count = 0;
        int i = 0;
        int n = corpus.Length;
        while (i < n) {
            string ch = corpus.Substring(i, 1);
            if (D6AntiPatternScan.IsIdentChar(ch)) {
                int start = i;
                while (i < n && D6AntiPatternScan.IsIdentChar(corpus.Substring(i, 1))) {
                    i = i + 1;
                }
                string token = corpus.Substring(start, i - start);
                if (token == name) {
                    count = count + 1;
                }
            } else {
                i = i + 1;
            }
        }
        return count;
    }

    private static string TypeName(string trimmed) {
        string kw = "";
        int kpos = -1;
        if (trimmed.IndexOf(" class ") >= 0) {
            kw = " class ";
            kpos = trimmed.IndexOf(" class ");
        } else if (trimmed.IndexOf(" interface ") >= 0) {
            kw = " interface ";
            kpos = trimmed.IndexOf(" interface ");
        } else if (trimmed.IndexOf(" struct ") >= 0) {
            kw = " struct ";
            kpos = trimmed.IndexOf(" struct ");
        } else if (trimmed.IndexOf(" enum ") >= 0) {
            kw = " enum ";
            kpos = trimmed.IndexOf(" enum ");
        }
        if (kw == "") {
            return "";
        }
        return D6AntiPatternScan.ReadIdent(trimmed, kpos + kw.Length);
    }

    private static string MethodName(string trimmed) {
        int open = trimmed.IndexOf("(");
        if (open < 0) {
            return "";
        }
        int brace = trimmed.IndexOf("{");
        if (brace >= 0 && brace < open) {
            return ""; // 属性（getter 内调用），非方法声明
        }
        int i = open - 1;
        while (i >= 0 && D6AntiPatternScan.IsSpaceChar(trimmed.Substring(i, 1))) {
            i = i - 1;
        }
        int end = i + 1;
        while (i >= 0 && D6AntiPatternScan.IsIdentChar(trimmed.Substring(i, 1))) {
            i = i - 1;
        }
        int start = i + 1;
        if (start >= end) {
            return "";
        }
        string name = trimmed.Substring(start, end - start);
        if (name == "" || D6AntiPatternScan.IsKeyword(name)) {
            return "";
        }
        return name;
    }

    private static string ReadIdent(string s, int start) {
        int i = start;
        int n = s.Length;
        while (i < n && D6AntiPatternScan.IsIdentChar(s.Substring(i, 1))) {
            i = i + 1;
        }
        if (i > start) {
            return s.Substring(start, i - start);
        }
        return "";
    }

    private static bool IsKeyword(string s) {
        return s == "new" || s == "return" || s == "await" || s == "if" || s == "for"
            || s == "while" || s == "switch" || s == "catch" || s == "foreach"
            || s == "else" || s == "int" || s == "long" || s == "string" || s == "bool"
            || s == "double" || s == "float" || s == "char" || s == "byte" || s == "void"
            || s == "object" || s == "var" || s == "true" || s == "false" || s == "null";
    }

    private static bool IsIdentChar(string ch) {
        return (ch >= "a" && ch <= "z") || (ch >= "A" && ch <= "Z") || (ch >= "0" && ch <= "9") || ch == "_";
    }

    private static bool IsSpaceChar(string ch) {
        return ch == " " || ch == "\t";
    }

    private static string TrimLeft(string s) {
        if (s == null) {
            return "";
        }
        int i = 0;
        int n = s.Length;
        while (i < n && D6AntiPatternScan.IsSpaceChar(s.Substring(i, 1))) {
            i = i + 1;
        }
        return s.Substring(i, n - i);
    }

    private static int CommentStart(string line) {
        int s1 = line.IndexOf("//");
        int s2 = line.IndexOf("/*");
        if (s1 < 0) {
            return s2;
        }
        if (s2 < 0) {
            return s1;
        }
        return s1 < s2 ? s1 : s2;
    }

    private void AddHit(string path, int lineNo, string marker) {
        this.Hits.Add(path + ":" + lineNo + ": " + marker);
    }
}
