// RFC 043 H-3 / SR-2 前哨：结构化编译诊断条目。
// `--message-format json`（SR-2）落地前由 QualityCli 从 stderr 诚实启发式提取；
// File/Line/Col 仅在来源格式携带（如 clang `path:line:col: error:`）时填充，否则留空（不造假）。
namespace Arc.Agent.Harness;

/// <summary>单条结构化编译诊断：位置 + 诊断码 + 消息 + 建议（SR-2 结构化诊断的稳定载体）。</summary>
public class AIDoDErrorItem {
    public string File;
    public int Line;
    public int Col;
    public string Code;
    public string Message;
    public string Suggestion;

    public AIDoDErrorItem() {
        this.File = "";
        this.Line = 0;
        this.Col = 0;
        this.Code = "";
        this.Message = "";
        this.Suggestion = "";
    }

    /// <summary>折叠为单行（模型可消费）：`[code] message`；位置存在时前缀 `file:line:col: `。</summary>
    public string Format() {
        string head = "";
        if (this.File != "") {
            head = this.File;
            if (this.Line > 0) {
                head = head + ":" + this.Line;
                if (this.Col > 0) {
                    head = head + ":" + this.Col;
                }
            }
            head = head + ": ";
        }
        string code = this.Code != "" ? "[" + this.Code + "] " : "";
        return head + code + this.Message;
    }
}
