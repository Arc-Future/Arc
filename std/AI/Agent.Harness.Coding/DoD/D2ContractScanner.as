// RFC 043 H-3：D2 契约硬规则门 — 源码级机器扫描（Coding 领域）。
//
// D2 通过谓词（最小可证伪、不造假的最小契约）：
//   1. 收集目标 .as 文件集（文件直用；目录递归，跳过 obj/bin/target/.git）。
//      空文件集 → 数据不足，调用方必须返回 Pending（非 Passed）。
//   2. 逐文件做行级真实扫描，逐项给出 通过/失败 + file:line 命中样例：
//      - async-suffix：返回类型为 Task / Task<T> 的方法声明必须以 Async 结尾（入口 Main 例外）；
//      - method-name-pascalcase：声明的方法名首字母必须大写（Main 例外）；
//      - type-name-pascalcase：class / interface / struct / enum 类型名首字母必须大写；
//      - control-flow-allman：if / else / while / for / foreach / switch 一律 {} 括起，
//        左花括号独立成行（禁止 K&R 同行花括号与省略大括号的单语句形式）。
//   3. 任一命中 → Failed；全部通过 → Passed。
// 未接线的契约项（this. 成员前缀 / [Builtin] stub 显式体 / 单一惯用法）诚实列在
// SkippedRules（Describe 可见），不冒充全绿。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Collections;
using Arc.IO;

/// <summary>契约硬规则扫描命中条目（file:line + 样例文本）。</summary>
public class D2Finding {
    public string Rule;
    public string File;
    public int Line;
    public string Text;

    public D2Finding() {
        this.Rule = "";
        this.File = "";
        this.Line = 0;
        this.Text = "";
    }

    /// <summary>格式化命中样例（file:line: rule — text）。</summary>
    public string Format() {
        return this.File + ":" + this.Line + ": " + this.Rule + " — " + this.Text;
    }
}

/// <summary>D2 扫描结果：文件集 + 命中列表 + 未扫契约项。</summary>
public class D2ScanResult {
    public List<string> ScannedFiles;
    public List<D2Finding> Findings;
    public List<string> SkippedRules;

    public D2ScanResult() {
        this.ScannedFiles = new List<string>();
        this.Findings = new List<D2Finding>();
        this.SkippedRules = new List<string>();
    }

    public bool Passed {
        get {
            return this.Findings.Count == 0;
        }
    }

    /// <summary>判定摘要（Signal/Detail 回喂模型）。</summary>
    public string Describe() {
        string s = "files=" + this.ScannedFiles.Count + ", findings=" + this.Findings.Count;
        int i = 0;
        while (i < this.Findings.Count && i < 5) {
            s = s + "\n  " + this.Findings[i].Format();
            i = i + 1;
        }
        if (this.Findings.Count > 5) {
            s = s + "\n  ... +" + (this.Findings.Count - 5) + " more";
        }
        if (this.SkippedRules.Count > 0) {
            s = s + "\nunscanned:";
            int j = 0;
            while (j < this.SkippedRules.Count) {
                s = s + "\n  - " + this.SkippedRules[j];
                j = j + 1;
            }
        }
        return s;
    }
}

/// <summary>D2 契约硬规则扫描器：收集 .as 文件集 + 行级真实扫描（可证伪、不空扫）。</summary>
public static class D2ContractScanner {
    /// <summary>收集目标 .as 文件集：文件直用；目录递归（跳过 obj/bin/target/.git）。空 → Length 0。</summary>
    public static List<string> CollectAsFiles(string projectOrFile) {
        List<string> result = new List<string>();
        string target = projectOrFile != null && projectOrFile != "" ? projectOrFile : ".";
        if (File.Exists(target)) {
            if (D2ContractScanner.EndsWithAs(target)) {
                result.Add(target);
            }
            return result;
        }
        if (Directory.Exists(target)) {
            D2ContractScanner.CollectRecursive(target, 0, result);
        }
        return result;
    }

    /// <summary>扫描文件集：逐文件行级检查，产出命中列表。</summary>
    public static D2ScanResult ScanFiles(List<string> files) {
        D2ScanResult result = new D2ScanResult();
        result.SkippedRules.Add("this. member prefix — needs symbol-aware parse; source-level approximation skipped");
        result.SkippedRules.Add("[Builtin] stub explicit getter — needs ABI-aware parse; skipped");
        result.SkippedRules.Add("single-idiom / anti-pattern prose rules — not machine-checkable; skipped");
        int i = 0;
        while (i < files.Count) {
            string path = files[i];
            result.ScannedFiles.Add(path);
            string[] lines = File.ReadAllLines(path);
            D2ContractScanner.ScanLines(lines, path, result.Findings);
            i = i + 1;
        }
        return result;
    }

    private static void ScanLines(string[] lines, string path, List<D2Finding> findings) {
        int i = 0;
        while (i < lines.Length) {
            string trimmed = D2ContractScanner.TrimLeft(lines[i] != null ? lines[i] : "");
            int lineNo = i + 1;
            if (trimmed != "" && !D2ContractScanner.IsCommentLine(trimmed)) {
                D2ContractScanner.CheckDeclaredNames(trimmed, path, lineNo, findings);
                D2ContractScanner.CheckControlFlow(lines, i, path, lineNo, findings);
            }
            i = i + 1;
        }
    }

    /// <summary>方法声明检查：async 后缀 + 方法名/类型名 PascalCase。</summary>
    private static void CheckDeclaredNames(string trimmed, string path, int lineNo, List<D2Finding> findings) {
        if (!D2ContractScanner.IsDeclarationStart(trimmed)) {
            return;
        }
        D2ContractScanner.CheckTypeName(trimmed, path, lineNo, findings);
        int openParen = trimmed.IndexOf("(");
        if (openParen < 0) {
            return;
        }
        int brace = trimmed.IndexOf("{");
        if (brace >= 0 && brace < openParen) {
            return; // 属性（含 getter 内方法调用），非方法声明
        }
        string head = trimmed.Substring(0, openParen);
        if (head.IndexOf("=") >= 0) {
            return; // 字段初始化表达式（= new X(...)），非方法声明
        }
        string name = D2ContractScanner.IdentifierBefore(trimmed, openParen);
        if (name == "" || D2ContractScanner.IsKeyword(name)) {
            return;
        }
        if (name != "Main" && !D2ContractScanner.IsUpperFirst(name)) {
            D2ContractScanner.AddFinding(findings, "method-name-pascalcase", path, lineNo, trimmed);
        }
        int nameAt = head.IndexOf(name);
        string retType = D2ContractScanner.TrimRight(head.Substring(0, nameAt));
        if (name != "Main" && D2ContractScanner.IsTaskReturn(retType) && !name.EndsWith("Async")) {
            D2ContractScanner.AddFinding(findings, "async-suffix", path, lineNo, trimmed);
        }
    }

    /// <summary>类型声明命名：class / interface / struct / enum 名称 PascalCase。</summary>
    private static void CheckTypeName(string trimmed, string path, int lineNo, List<D2Finding> findings) {
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
            return;
        }
        string name = D2ContractScanner.ReadIdent(trimmed, kpos + kw.Length);
        if (name != "" && !D2ContractScanner.IsUpperFirst(name)) {
            D2ContractScanner.AddFinding(findings, "type-name-pascalcase", path, lineNo, trimmed);
        }
    }

    /// <summary>控制流大括号检查：K&R 同行花括号 / 省略大括号单语句 → 违规。</summary>
    private static void CheckControlFlow(string[] lines, int idx, string path, int lineNo, List<D2Finding> findings) {
        string raw = lines[idx] != null ? lines[idx] : "";
        string trimmed = D2ContractScanner.TrimRight(D2ContractScanner.StripInlineComment(D2ContractScanner.TrimLeft(raw)));
        if (!D2ContractScanner.IsControlStart(trimmed)) {
            return;
        }
        if (trimmed.EndsWith("{")) {
            D2ContractScanner.AddFinding(findings, "control-flow-allman", path, lineNo, "K&R brace on control line: " + trimmed);
            return;
        }
        if (trimmed.EndsWith(";") || (trimmed.EndsWith(")") && D2ContractScanner.ParensBalanced(trimmed))) {
            // 条件闭合后下一非空行必须为独立 {（缺大括号 / 单语句形式）。
            int j = idx + 1;
            while (j < lines.Length) {
                string nt = D2ContractScanner.TrimLeft(lines[j] != null ? lines[j] : "");
                if (nt != "" && !D2ContractScanner.IsCommentLine(nt)) {
                    break;
                }
                j = j + 1;
            }
            if (j < lines.Length) {
                string next = D2ContractScanner.TrimLeft(lines[j] != null ? lines[j] : "");
                if (!next.StartsWith("{")) {
                    D2ContractScanner.AddFinding(findings, "control-flow-allman", path, lineNo, "missing braces after: " + trimmed);
                }
            }
        }
    }

    private static void CollectRecursive(string dir, int depth, List<string> outList) {
        if (depth > 10) {
            return;
        }
        string[] files = Directory.GetFiles(dir, "*.as");
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
                if (name != "obj" && name != "bin" && name != "target" && name != ".git") {
                    D2ContractScanner.CollectRecursive(dirs[j], depth + 1, outList);
                }
                j = j + 1;
            }
        }
    }

    private static string IdentifierBefore(string text, int pos) {
        int i = pos - 1;
        while (i >= 0 && D2ContractScanner.IsSpace(text.Substring(i, 1))) {
            i = i - 1;
        }
        int end = i + 1;
        while (i >= 0 && D2ContractScanner.IsIdent(text.Substring(i, 1))) {
            i = i - 1;
        }
        int start = i + 1;
        if (start >= end) {
            return "";
        }
        if (start > 0 && text.Substring(start - 1, 1) == "<") {
            return ""; // 泛型实参（如 List<int>(），非方法名
        }
        return text.Substring(start, end - start);
    }

    private static string ReadIdent(string s, int start) {
        int i = start;
        int n = s.Length;
        while (i < n && D2ContractScanner.IsIdent(s.Substring(i, 1))) {
            i = i + 1;
        }
        if (i > start) {
            return s.Substring(start, i - start);
        }
        return "";
    }

    private static bool ParensBalanced(string s) {
        int depth = 0;
        int i = 0;
        while (i < s.Length) {
            string ch = s.Substring(i, 1);
            if (ch == "(") {
                depth = depth + 1;
            } else if (ch == ")") {
                depth = depth - 1;
            }
            i = i + 1;
        }
        return depth == 0;
    }

    private static string StripInlineComment(string s) {
        int idx = s.IndexOf("//");
        if (idx < 0) {
            return s;
        }
        return s.Substring(0, idx);
    }

    private static bool IsTaskReturn(string retType) {
        return retType.EndsWith("Task") || retType.EndsWith("Task?") || retType.IndexOf("Task<") >= 0;
    }

    private static bool IsDeclarationStart(string s) {
        return s.StartsWith("public ")
            || s.StartsWith("private ")
            || s.StartsWith("protected ")
            || s.StartsWith("internal ")
            || s.StartsWith("static ")
            || s.StartsWith("async ")
            || s.StartsWith("override ")
            || s.StartsWith("virtual ")
            || s.StartsWith("sealed ")
            || s.StartsWith("abstract ")
            || s.StartsWith("final ")
            || s.StartsWith("partial ");
    }

    private static bool IsControlStart(string s) {
        return s.StartsWith("if ")
            || s.StartsWith("if(")
            || s.StartsWith("while ")
            || s.StartsWith("while(")
            || s.StartsWith("for ")
            || s.StartsWith("for(")
            || s.StartsWith("foreach ")
            || s.StartsWith("foreach(")
            || s.StartsWith("switch ")
            || s.StartsWith("switch(")
            || s.StartsWith("else ")
            || s == "else"
            || s.StartsWith("} else ")
            || s == "} else";
    }

    private static bool IsCommentLine(string s) {
        return s.StartsWith("//") || s.StartsWith("/*") || s.StartsWith("*") || s.StartsWith("#");
    }

    private static bool IsKeyword(string s) {
        return s == "new" || s == "return" || s == "await" || s == "if" || s == "for"
            || s == "while" || s == "switch" || s == "catch" || s == "foreach"
            || s == "else" || s == "int" || s == "long" || s == "string" || s == "bool"
            || s == "double" || s == "float" || s == "char" || s == "byte" || s == "void"
            || s == "object" || s == "var" || s == "true" || s == "false" || s == "null";
    }

    private static bool IsUpperFirst(string s) {
        if (s == null || s == "") {
            return false;
        }
        string c = s.Substring(0, 1);
        return c >= "A" && c <= "Z";
    }

    private static bool IsSpace(string ch) {
        return ch == " " || ch == "\t";
    }

    private static bool IsIdent(string ch) {
        return (ch >= "a" && ch <= "z") || (ch >= "A" && ch <= "Z") || (ch >= "0" && ch <= "9") || ch == "_";
    }

    private static bool EndsWithAs(string path) {
        return path != null && path.EndsWith(".as");
    }

    private static string TrimLeft(string s) {
        if (s == null) {
            return "";
        }
        int i = 0;
        int n = s.Length;
        while (i < n && D2ContractScanner.IsSpace(s.Substring(i, 1))) {
            i = i + 1;
        }
        return s.Substring(i, n - i);
    }

    private static string TrimRight(string s) {
        if (s == null) {
            return "";
        }
        int i = s.Length - 1;
        while (i >= 0 && D2ContractScanner.IsSpace(s.Substring(i, 1))) {
            i = i - 1;
        }
        return s.Substring(0, i + 1);
    }

    private static void AddFinding(List<D2Finding> findings, string rule, string file, int line, string text) {
        D2Finding f = new D2Finding();
        f.Rule = rule;
        f.File = file;
        f.Line = line;
        f.Text = text;
        findings.Add(f);
    }
}
