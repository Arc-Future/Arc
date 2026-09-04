// RFC 041 §7.5：AIModelProgress — 统一批量进度（跨域一致契约）。
//
// 批量 = 本地循环 + 按条进度（IProgress 无此类型，用 Action 回调承载，对齐仓库既有
// Action<...> 回调先例）。默认零开销——不传回调即不收集。
namespace Arc.AI.Models;

/// <summary>批量/长任务进度（RFC 041 §7.5 统一能力面）。</summary>
public class AIModelProgress {
    /// <summary>当前进度（1-based）。</summary>
    public int Current { get; set; }
    /// <summary>总数。</summary>
    public int Total { get; set; }
    /// <summary>阶段标识（如 "ocr"/"transcribe"/"embed"）。</summary>
    public string Stage { get; set; }

    public AIModelProgress() {
        this.Current = 0;
        this.Total = 0;
        this.Stage = "";
    }
}
