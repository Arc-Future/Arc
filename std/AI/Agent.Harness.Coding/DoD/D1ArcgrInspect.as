// RFC 043 H-3：D1 语义完整性门——`.arcgr` 引用图完整性判定（Coding 领域）。
//
// D1 通过谓词（最小可证伪、不造假的最小契约）：
//   1. `arc inspect <entry> --format json`（源码模式）exit 0；
//   2. stdout 可解析，且含非空 symbol 表 + `edges` 键（引用图可导出）；
//   3. 无「引用断裂」：每条边的 caller/callee 端点都存在于符号表；
//   4. 无「不可达入口」：每个入口符号都出现在 reachable 集合（可达性崩溃信号）。
//   5. 入口符号 explain 探测：`arc explain <arcgr> <sym> --format json` exit 0 且 is_reachable=true。
// unreachable 符号本身属正常输入面（私有未用方法 / 库导出面未被本 TU 消费等），不判红；
// 仅在「引用断裂 / 不可达入口」这类机器可判信号上判红。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Collections;
using Arc.Text.Json;

/// <summary>arc inspect --format json 的结构化视图 + D1 通过谓词。</summary>
public class D1ArcgrInspect {
    public int ExitCode;
    public bool JsonOk;
    public string Error;
    public List<int> Symbols;
    public List<string> SymbolNames;
    public List<int> EdgesCallers;
    public List<int> EdgesCallees;
    public List<int> EntryPoints;
    public List<int> Reachable;
    public List<int> Unreachable;

    public D1ArcgrInspect() {
        this.ExitCode = -1;
        this.JsonOk = false;
        this.Error = "";
        this.Symbols = new List<int>();
        this.SymbolNames = new List<string>();
        this.EdgesCallers = new List<int>();
        this.EdgesCallees = new List<int>();
        this.EntryPoints = new List<int>();
        this.Reachable = new List<int>();
        this.Unreachable = new List<int>();
    }

    /// <summary>
    /// 解析 inspect 输出。exit!=0 或 JSON 形状不符 → JsonOk=false 并记 Error。
    /// 所需顶层键：symbols / edges / entry_points / reachable / unreachable（顺序无关）。
    /// </summary>
    public static D1ArcgrInspect Parse(int exitCode, string stdout) {
        D1ArcgrInspect r = new D1ArcgrInspect();
        r.ExitCode = exitCode;
        if (exitCode != 0) {
            r.Error = "arc inspect exited " + exitCode;
            return r;
        }
        if (stdout == null || stdout == "") {
            r.Error = "arc inspect produced empty stdout";
            return r;
        }
        JsonReader reader = new JsonReader(stdout);
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
            r.Error = "arc inspect stdout is not a JSON object";
            return r;
        }
        bool sawSymbols = false;
        bool sawEdges = false;
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                cont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (prop == "symbols") {
                sawSymbols = D1ArcgrInspect.ParseSymbols(reader, r);
            } else if (prop == "edges") {
                sawEdges = D1ArcgrInspect.ParseEdges(reader, r);
            } else if (prop == "entry_points") {
                D1ArcgrInspect.ParseEntryPoints(reader, r);
            } else if (prop == "reachable") {
                D1ArcgrInspect.ParseIdList(reader, r.Reachable);
            } else if (prop == "unreachable") {
                D1ArcgrInspect.ParseIdList(reader, r.Unreachable);
            } else {
                // 未知顶层字段：消费其值（复合值整段跳过，保持解析位置）。
                if (!reader.Read()) {
                    cont = false;
                    break;
                }
                if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
                    reader.Skip();
                }
            }
        }
        r.JsonOk = sawSymbols && sawEdges;
        if (!r.JsonOk) {
            r.Error = "inspect JSON missing required keys (symbols/edges)";
        }
        return r;
    }

    /// <summary>D1 通过谓词（见文件头契约）。unreachable 不判红。</summary>
    public bool Passed {
        get {
            if (this.ExitCode != 0 || !this.JsonOk) {
                return false;
            }
            if (this.Symbols.Count == 0) {
                return false;
            }
            int i = 0;
            while (i < this.EdgesCallers.Count) {
                if (!this.HasSymbol(this.EdgesCallers[i]) || !this.HasSymbol(this.EdgesCallees[i])) {
                    return false;
                }
                i = i + 1;
            }
            int j = 0;
            while (j < this.EntryPoints.Count) {
                if (!this.InList(this.Reachable, this.EntryPoints[j])) {
                    return false;
                }
                j = j + 1;
            }
            return true;
        }
    }

    /// <summary>入口符号名（≤max 个），供 explain 可达性探测。</summary>
    public List<string> EntryPointNames(int maxCount) {
        List<string> names = new List<string>();
        int i = 0;
        while (i < this.EntryPoints.Count && names.Count < maxCount) {
            string n = this.NameOf(this.EntryPoints[i]);
            if (n != "") {
                names.Add(n);
            }
            i = i + 1;
        }
        return names;
    }

    /// <summary>判定摘要（供 Signal/Detail 回喂模型）。</summary>
    public string Describe() {
        string s = "exit=" + this.ExitCode
            + ", symbols=" + this.Symbols.Count
            + ", edges=" + this.EdgesCallers.Count
            + ", entry_points=" + this.EntryPoints.Count
            + ", reachable=" + this.Reachable.Count
            + ", unreachable=" + this.Unreachable.Count;
        if (this.Error != "") {
            s = s + ", error: " + this.Error;
        }
        return s;
    }

    /// <summary>explain JSON 是否标记 is_reachable=true（入口可达性探测断言）。</summary>
    public static bool ExplainIsReachable(string stdout) {
        if (stdout == null || stdout == "") {
            return false;
        }
        JsonReader reader = new JsonReader(stdout);
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
            return false;
        }
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                cont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string p = reader.GetString();
            if (p == "is_reachable") {
                if (!reader.Read()) {
                    return false;
                }
                return reader.GetBoolean();
            }
            // 其余字段：消费其值（复合值整段跳过）。
            if (!reader.Read()) {
                cont = false;
                break;
            }
            if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
                reader.Skip();
            }
        }
        return false;
    }

    private bool HasSymbol(int id) {
        int i = 0;
        while (i < this.Symbols.Count) {
            if (this.Symbols[i] == id) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private bool InList(List<int> list, int id) {
        int i = 0;
        while (i < list.Count) {
            if (list[i] == id) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private string NameOf(int id) {
        int i = 0;
        while (i < this.Symbols.Count) {
            if (this.Symbols[i] == id) {
                return this.SymbolNames[i];
            }
            i = i + 1;
        }
        return "";
    }

    private static bool ParseSymbols(JsonReader reader, D1ArcgrInspect r) {
        // reader 处于 PropertyName "symbols"；下一 token 必须为 StartArray。
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return false;
        }
        bool arrCont = true;
        while (arrCont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                arrCont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.StartObject) {
                continue;
            }
            int id = -1;
            string name = "";
            bool objCont = true;
            while (objCont && reader.Read()) {
                if (reader.TokenType == JsonTokenType.EndObject) {
                    objCont = false;
                    break;
                }
                if (reader.TokenType != JsonTokenType.PropertyName) {
                    continue;
                }
                string p = reader.GetString();
                if (p == "id") {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    id = reader.GetInt32();
                } else if (p == "name") {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    name = reader.GetString();
                } else {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
                        reader.Skip();
                    }
                }
            }
            if (id >= 0) {
                r.Symbols.Add(id);
                r.SymbolNames.Add(name != null ? name : "");
            }
        }
        return r.Symbols.Count > 0;
    }

    private static bool ParseEdges(JsonReader reader, D1ArcgrInspect r) {
        // reader 处于 PropertyName "edges"；下一 token 必须为 StartArray。
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return false;
        }
        bool arrCont = true;
        while (arrCont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                arrCont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.StartObject) {
                continue;
            }
            int caller = -1;
            int callee = -1;
            bool objCont = true;
            while (objCont && reader.Read()) {
                if (reader.TokenType == JsonTokenType.EndObject) {
                    objCont = false;
                    break;
                }
                if (reader.TokenType != JsonTokenType.PropertyName) {
                    continue;
                }
                string p = reader.GetString();
                if (p == "caller") {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    caller = reader.GetInt32();
                } else if (p == "callee") {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    callee = reader.GetInt32();
                } else {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
                        reader.Skip();
                    }
                }
            }
            if (caller >= 0 && callee >= 0) {
                r.EdgesCallers.Add(caller);
                r.EdgesCallees.Add(callee);
            }
        }
        // edges 允许为空数组（引用图可导出即可）。
        return true;
    }

    private static void ParseEntryPoints(JsonReader reader, D1ArcgrInspect r) {
        // reader 处于 PropertyName "entry_points"；下一 token 必须为 StartArray。
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return;
        }
        bool arrCont = true;
        while (arrCont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                arrCont = false;
                break;
            }
            if (reader.TokenType != JsonTokenType.StartObject) {
                continue;
            }
            bool objCont = true;
            while (objCont && reader.Read()) {
                if (reader.TokenType == JsonTokenType.EndObject) {
                    objCont = false;
                    break;
                }
                if (reader.TokenType != JsonTokenType.PropertyName) {
                    continue;
                }
                string p = reader.GetString();
                if (p == "symbol_id") {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    r.EntryPoints.Add(reader.GetInt32());
                } else {
                    if (!reader.Read()) {
                        objCont = false;
                        break;
                    }
                    if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
                        reader.Skip();
                    }
                }
            }
        }
    }

    private static void ParseIdList(JsonReader reader, List<int> outList) {
        // reader 处于 PropertyName（reachable/unreachable）；下一 token 必须为 StartArray。
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartArray) {
            return;
        }
        bool cont = true;
        while (cont && reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndArray) {
                cont = false;
                break;
            }
            if (reader.TokenType == JsonTokenType.Number) {
                outList.Add(reader.GetInt32());
            }
        }
    }
}
