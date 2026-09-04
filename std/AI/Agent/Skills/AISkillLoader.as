// RFC 038：AISkillLoader — 从 SKILL.md 加载/解析官方 Agent Skills 规范（spec 1.0）。
//
// 对齐 agentskills.io 开放标准：SKILL.md 顶部 `---` 闭合的 YAML frontmatter 形如
//   ---
//   name: my-skill
//   description: 一句话说明该能力何时/如何用
//   license: MIT
//   compatibility: ...
//   allowed-tools:
//     - read_file
//   metadata:
//     ...
//   ---
//   # body（激活层：能力说明正文）
//
// **frontmatter 解析复用 Arc.Text.Yaml**（YamlSerializer.Parse + YamlNode），不手写最小解析器——
// 与 std 既有 YAML 基础设施同源，字段读取经 YamlNode.Get/GetString/GetBoolean 等查询。
//
// 三级渐进披露映射：
//   发现层 = Name + Description（常驻 system，~100 tokens）；
//   激活层 = ActivationPrompt（= body，命中后注入）；
//   执行层 = references/scripts/assets（经 AISkill.SourcePath 按需定位）。
//
// 强制约束（fail-fast，违反即抛异常）：
//   - name：小写连字符 `[a-z0-9-]+`，≤64 字符，且须匹配父目录名（LoadDirectory 时校验）。
//   - description：必填，≤1024 字符。
//   - compatibility：可选，≤500 字符。
namespace Arc.Agent;
using Arc.Text.Yaml;
using Arc.Collections;
using Arc;

/// <summary>
/// 官方 Agent Skills 规范加载器：解析 SKILL.md 的 YAML frontmatter（复用
/// <see cref="YamlSerializer"/>）为 <see cref="AISkill"/>，并执行官方名称/描述约束校验。
/// </summary>
public class AISkillLoader {

    /// <summary>从 SKILL.md 文本解析 frontmatter + body，产出 AISkill。
    /// <paramref name="expectedName"/> 为父目录名（名称约束校验用；可为空则跳过匹配校验）。</summary>
    public AISkill Parse(string expectedName, string skillText) {
        string norm = this.NormalizeNewlines(skillText);
        string fm = this.ExtractFrontmatter(norm);
        string body = this.ExtractBody(norm);
        YamlNode root = YamlSerializer.Parse(fm);
        string name = this.ReadString(root, "name");
        if (name == "") {
            throw new Exception("SKILL.md missing required frontmatter 'name'");
        }
        this.ValidateName(name, expectedName);
        string description = this.ReadString(root, "description");
        if (description == "") {
            throw new Exception("SKILL.md missing required frontmatter 'description'");
        }
        if (description.Length > 1024) {
            throw new Exception("SKILL.md 'description' exceeds 1024 chars");
        }
        string compatibility = this.ReadString(root, "compatibility");
        if (compatibility.Length > 500) {
            throw new Exception("SKILL.md 'compatibility' exceeds 500 chars");
        }
        AISkill skill = new AISkill(name, description, body, new AIToolSet());
        skill.License = this.ReadString(root, "license");
        skill.Compatibility = compatibility;
        skill.AllowedTools = this.ReadStringList(root, "allowed-tools");
        return skill;
    }

    /// <summary>从目录加载：读取 <paramref name="dirPath"/>/SKILL.md，校验 name 匹配目录名，填充 SourcePath。</summary>
    public AISkill LoadDirectory(string dirPath) {
        if (dirPath == null || dirPath == "") {
            throw new Exception("AISkillLoader: empty directory path");
        }
        string skillPath = dirPath + "/SKILL.md";
        if (!File.Exists(skillPath)) {
            throw new Exception("AISkillLoader: no SKILL.md under " + dirPath);
        }
        string text = File.ReadAllText(skillPath);
        string dirName = this.BaseName(dirPath);
        AISkill skill = this.Parse(dirName, text);
        skill.SourcePath = dirPath;
        return skill;
    }

    /// <summary>异步从目录加载（语义同 <see cref="LoadDirectory"/>；异步优先，不阻塞调用线程）。</summary>
    public async Task<AISkill> LoadDirectoryAsync(string dirPath) {
        if (dirPath == null || dirPath == "") {
            throw new Exception("AISkillLoader: empty directory path");
        }
        string skillPath = dirPath + "/SKILL.md";
        if (!await File.ExistsAsync(skillPath)) {
            throw new Exception("AISkillLoader: no SKILL.md under " + dirPath);
        }
        string text = await File.ReadAllTextAsync(skillPath);
        string dirName = this.BaseName(dirPath);
        AISkill skill = this.Parse(dirName, text);
        skill.SourcePath = dirPath;
        return skill;
    }

    // ── frontmatter 提取 ──

    /// <summary>规整换行：统一 CRLF/CR → LF（Windows 编辑器产物兼容，避免 \r 混入正文与字段值）。</summary>
    private string NormalizeNewlines(string text) {
        if (text == null) {
            return "";
        }
        string t = text.Replace("\r\n", "\n");
        t = t.Replace("\r", "\n");
        return t;
    }

    /// <summary>提取顶部 `---` 闭合的 YAML frontmatter 文本；无闭合块返回空串。</summary>
    public string ExtractFrontmatter(string text) {
        if (text == null) {
            return "";
        }
        string[] lines = text.Split("\n");
        int size = lines.Length;
        int i = 0;
        // 跳过首行空白/注释，定位开标记 `---`
        while (i < size) {
            string t = lines[i].Trim();
            if (t == "") {
                i = i + 1;
                continue;
            }
            break;
        }
        if (i >= size || lines[i].Trim() != "---") {
            return "";
        }
        i = i + 1;
        StringBuilder sb = new StringBuilder();
        while (i < size) {
            string line = lines[i];
            if (line.Trim() == "---") {
                return sb.ToString();
            }
            sb.Append(line);
            sb.Append("\n");
            i = i + 1;
        }
        return "";
    }

    /// <summary>提取 frontmatter 之后的 body（SKILL.md 正文，激活层）。无 frontmatter 时返回全文。</summary>
    public string ExtractBody(string text) {
        if (text == null) {
            return "";
        }
        string[] lines = text.Split("\n");
        int size = lines.Length;
        int i = 0;
        while (i < size) {
            string t = lines[i].Trim();
            if (t == "") {
                i = i + 1;
                continue;
            }
            break;
        }
        if (i >= size || lines[i].Trim() != "---") {
            return text;
        }
        i = i + 1;
        while (i < size) {
            if (lines[i].Trim() == "---") {
                i = i + 1;
                break;
            }
            i = i + 1;
        }
        if (i >= size) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        while (i < size) {
            sb.Append(lines[i]);
            sb.Append("\n");
            i = i + 1;
        }
        return sb.ToString();
    }

    // ── 约束校验 ──

    /// <summary>官方名称约束：小写连字符 `[a-z0-9-]+`，≤64；expectedName 非空时须相等。</summary>
    public void ValidateName(string name, string expectedName) {
        if (name.Length == 0 || name.Length > 64) {
            throw new Exception("SKILL.md 'name' must be 1..64 chars, got '" + name + "'");
        }
        int n = name.Length;
        int i = 0;
        while (i < n) {
            string ch = name.Substring(i, 1);
            bool ok = (ch >= "a" && ch <= "z") || (ch >= "0" && ch <= "9") || ch == "-";
            if (!ok) {
                throw new Exception("SKILL.md 'name' must be lowercase-hyphen [a-z0-9-], got '" + name + "'");
            }
            i = i + 1;
        }
        if (expectedName != null && expectedName != "" && name != expectedName) {
            throw new Exception("SKILL.md 'name' '" + name + "' must match directory name '" + expectedName + "'");
        }
    }

    // ── Yaml 读取（复用 YamlNode 查询 API） ──

    private string ReadString(YamlNode root, string key) {
        if (root == null || !root.IsMapping()) {
            return "";
        }
        YamlNode v = root.Get(key);
        return v != null ? v.GetString() : "";
    }

    private List<string> ReadStringList(YamlNode root, string key) {
        List<string> list = new List<string>();
        if (root == null || !root.IsMapping()) {
            return list;
        }
        YamlNode v = root.Get(key);
        if (v == null || !v.IsSequence()) {
            return list;
        }
        List<YamlNode> items = v.GetItems();
        int n = items.Count;
        int i = 0;
        while (i < n) {
            string s = items[i] != null ? items[i].GetString() : "";
            if (s != "") {
                list.Add(s);
            }
            i = i + 1;
        }
        return list;
    }

    private string BaseName(string path) {
        if (path == null || path == "") {
            return "";
        }
        int len = path.Length;
        int end = len;
        // 去掉尾部分隔符
        while (end > 0 && (path.Substring(end - 1, 1) == "/" || path.Substring(end - 1, 1) == "\\")) {
            end = end - 1;
        }
        int start = end;
        while (start > 0) {
            string ch = path.Substring(start - 1, 1);
            if (ch == "/" || ch == "\\") {
                return path.Substring(start, end - start);
            }
            start = start - 1;
        }
        return path.Substring(0, end);
    }
}