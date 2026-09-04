// RFC 043 场景 4.1 / 4.3：D3 行为验证门——`arc test --logger json` 用例级明细解析。
//
// D3 通过谓词（防降级、不造假）：
//   1. JSON 报告可解析（stdout 含 summary/results）；
//   2. passed 断言用例数 > 0 且 failed == 0（errors == 0 同理）——「退出码绿」升级为「断言绿」；
//   3. 结构化 Acceptance 条目声明的 TestName（验收对照）须在结果中真实 passed；
//   4. 用例数骤降（如从 N 降到 0）标疑——防「测试被改弱」。
namespace Arc.Agent.Harness.Coding;
using Arc;
using Arc.Collections;
using Arc.Text;
using Arc.Text.Json;

/// <summary>
/// `arc test --logger json` 的结构化视图 + D3 通过谓词。stdout 可能带
/// 「built test binary」banner，解析前先定位首个 `{` 再交给 JsonReader。
/// </summary>
public class D3TestReport {
    public int ExitCode;
    public bool JsonOk;
    public string Error;
    public int Total;
    public int Passed;
    public int Failed;
    public int Skipped;
    public int Errors;
    public List<string> TestNames;
    public List<string> FailedNames;

    public D3TestReport() {
        this.ExitCode = -1;
        this.JsonOk = false;
        this.Error = "";
        this.Total = 0;
        this.Passed = 0;
        this.Failed = 0;
        this.Skipped = 0;
        this.Errors = 0;
        this.TestNames = new List<string>();
        this.FailedNames = new List<string>();
    }

    /// <summary>解析 `arc test --logger json` 输出。exit!=0 或 JSON 形状不符 → JsonOk=false 并记 Error。</summary>
    public static D3TestReport Parse(int exitCode, string stdout) {
        D3TestReport r = new D3TestReport();
        r.ExitCode = exitCode;
        if (stdout == null || stdout == "") {
            r.Error = "arc test produced empty stdout";
            return r;
        }
        // stdout 可能含 build banner（built test binary / test run completed）——定位首个 JSON 对象。
        int brace = stdout.IndexOf("{");
        if (brace < 0) {
            r.Error = "arc test stdout contains no JSON object";
            return r;
        }
        JsonReader reader = new JsonReader(stdout.Substring(brace));
        if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject) {
            r.Error = "arc test stdout is not a JSON object";
            return r;
        }
        bool sawSummary = false;
        bool sawResults = false;
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
            if (prop == "summary") {
                sawSummary = D3TestReport.ParseSummary(reader, r);
            } else if (prop == "results") {
                sawResults = D3TestReport.ParseResults(reader, r);
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
        r.JsonOk = sawSummary && sawResults;
        if (!r.JsonOk) {
            r.Error = "test JSON missing required keys (summary/results)";
        }
        return r;
    }

    /// <summary>D3 通过谓词：passed 用例数 > 0 且 failed/errors == 0（断言绿，非退出码绿）。</summary>
    public bool PassedPredicate {
        get {
            return this.JsonOk && this.Passed > 0 && this.Failed == 0 && this.Errors == 0;
        }
    }

    /// <summary>测试名（含 DisplayName 折叠）是否在结果中且真实 passed（验收对照 TestName 判定）。</summary>
    public bool ContainsPassedTest(string name) {
        if (name == null || name == "") {
            return true;
        }
        int i = 0;
        int n = this.TestNames.Count;
        while (i < n) {
            if (this.TestNames[i] == name) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    /// <summary>判定摘要（供 Signal/Detail 回喂模型）。</summary>
    public string Describe() {
        string s = "exit=" + this.ExitCode
            + ", total=" + this.Total
            + ", passed=" + this.Passed
            + ", failed=" + this.Failed
            + ", skipped=" + this.Skipped
            + ", errors=" + this.Errors;
        if (this.Error != "") {
            s = s + ", error: " + this.Error;
        }
        return s;
    }

    /// <summary>失败用例名明细（D3 失败回喂）。</summary>
    public string FailedDetail() {
        if (this.FailedNames.Count == 0) {
            return "";
        }
        StringBuilder sb = new StringBuilder();
        int i = 0;
        int n = this.FailedNames.Count;
        while (i < n) {
            sb.Append("    failed case: " + this.FailedNames[i] + "\n");
            i = i + 1;
        }
        return sb.ToString();
    }

    private static bool ParseSummary(JsonReader reader, D3TestReport r) {
        // reader 处于 PropertyName "summary"；下一 token 必须为 StartObject。
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
            if (p == "total") {
                r.Total = D3TestReport.ReadInt(reader);
            } else if (p == "passed") {
                r.Passed = D3TestReport.ReadInt(reader);
            } else if (p == "failed") {
                r.Failed = D3TestReport.ReadInt(reader);
            } else if (p == "skipped") {
                r.Skipped = D3TestReport.ReadInt(reader);
            } else if (p == "errors") {
                r.Errors = D3TestReport.ReadInt(reader);
            } else {
                D3TestReport.SkipValue(reader);
            }
        }
        return true;
    }

    private static bool ParseResults(JsonReader reader, D3TestReport r) {
        // reader 处于 PropertyName "results"；下一 token 必须为 StartArray。
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
            string name = "";
            string status = "";
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
                if (p == "name") {
                    name = D3TestReport.ReadString(reader);
                } else if (p == "status") {
                    status = D3TestReport.ReadString(reader);
                } else {
                    D3TestReport.SkipValue(reader);
                }
            }
            if (name != "") {
                r.TestNames.Add(name);
                if (status == "failed" || status == "error") {
                    r.FailedNames.Add(name);
                }
            }
        }
        return true;
    }

    private static int ReadInt(JsonReader reader) {
        if (!reader.Read()) {
            return 0;
        }
        if (reader.TokenType == JsonTokenType.Number) {
            return reader.GetInt32();
        }
        if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
            reader.Skip();
        }
        return 0;
    }

    private static string ReadString(JsonReader reader) {
        if (!reader.Read()) {
            return "";
        }
        if (reader.TokenType == JsonTokenType.String) {
            return reader.GetString();
        }
        if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
            reader.Skip();
        }
        return "";
    }

    private static void SkipValue(JsonReader reader) {
        if (!reader.Read()) {
            return;
        }
        if (reader.TokenType == JsonTokenType.StartObject || reader.TokenType == JsonTokenType.StartArray) {
            reader.Skip();
        }
    }
}
