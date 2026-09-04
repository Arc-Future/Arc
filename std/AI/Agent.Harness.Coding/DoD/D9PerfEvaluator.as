// RFC 043 P3（performance-observability）：D9 性能门判定 — 基线 diff 回归阈值。
//
// 纯函数 Compare：当前墙钟/峰值内存 ↔ 版本化基线，超软阈值 → Warning，超硬阈值 →
// Failed（WallClockSlow/MemorySpike 相对基线判定启用，对齐 performance-observability
// P3）。判定规则在 Coding 层（基座持类型与存储，见 AIPerfBaseline/AIPerfBaselineStore）。
// P1/P2「增强信号不新开门」语义不变——D9 是 P3 新增门。
namespace Arc.Agent.Harness.Coding;

/// <summary>D9 判定结果（Passed / Warning / Failed 三档）。</summary>
public enum D9PerfVerdict {
    /// <summary>在软阈值内，无回归。</summary>
    Passed,
    /// <summary>超软阈值但未超硬阈值（软回归，不判红，Detail 标注 warning）。</summary>
    Warning,
    /// <summary>超硬阈值（硬回归，判红）。</summary>
    Failed
}

/// <summary>D9 回归阈值（软/硬比率；相对基线）。</summary>
public class D9PerfThresholds {
    /// <summary>墙钟软阈值比率（默认 1.2 = 慢 20% 记 warning）。</summary>
    public double WallSoftRatio;
    /// <summary>墙钟硬阈值比率（默认 1.5 = 慢 50% 判红）。</summary>
    public double WallHardRatio;
    /// <summary>峰值内存软阈值比率。</summary>
    public double MemSoftRatio;
    /// <summary>峰值内存硬阈值比率。</summary>
    public double MemHardRatio;

    public D9PerfThresholds() {
        this.WallSoftRatio = 1.2;
        this.WallHardRatio = 1.5;
        this.MemSoftRatio = 1.2;
        this.MemHardRatio = 1.5;
    }

    /// <summary>默认阈值（墙钟/内存软 1.2x · 硬 1.5x）。</summary>
    public static D9PerfThresholds Default {
        get { return new D9PerfThresholds(); }
    }
}

/// <summary>D9 单次比较结果（判定 + 人类可读 Detail）。</summary>
public class D9PerfComparison {
    /// <summary>判定。</summary>
    public D9PerfVerdict Verdict;
    /// <summary>判定明细（墙钟/内存 vs 基线 + 百分比）。</summary>
    public string Detail;

    public D9PerfComparison() {
        this.Verdict = D9PerfVerdict.Passed;
        this.Detail = "";
    }
}

/// <summary>D9 基线 diff 阈值判定（纯函数；不跑进程，输入为已采集的墙钟/内存）。</summary>
public class D9PerfEvaluator {
    private D9PerfEvaluator() {
    }

    /// <summary>
    /// 基线 diff 回归阈值判定：当前墙钟/峰值内存相对基线算比率，超软阈值 → Warning，
    /// 超硬阈值 → Failed。基线为 0 的维度不参与判定（诚实跳过，不臆造比率）。
    /// </summary>
    public static D9PerfComparison Compare(
        long baselineWallMs,
        long baselineMemBytes,
        long currentWallMs,
        long currentMemBytes,
        D9PerfThresholds thresholds) {
        D9PerfComparison c = new D9PerfComparison();
        double wallRatio = 0.0;
        double memRatio = 0.0;
        if (baselineWallMs > 0 && currentWallMs > 0) {
            wallRatio = (double)currentWallMs / (double)baselineWallMs;
        }
        if (baselineMemBytes > 0 && currentMemBytes > 0) {
            memRatio = (double)currentMemBytes / (double)baselineMemBytes;
        }
        bool wallHard = wallRatio > thresholds.WallHardRatio;
        bool memHard = memRatio > thresholds.MemHardRatio;
        bool wallSoft = wallRatio > thresholds.WallSoftRatio;
        bool memSoft = memRatio > thresholds.MemSoftRatio;
        string detail = "wall=" + currentWallMs + "ms/" + baselineWallMs + "ms ("
            + D9PerfEvaluator.Pct(wallRatio) + "%)"
            + " mem=" + currentMemBytes + "B/" + baselineMemBytes + "B ("
            + D9PerfEvaluator.Pct(memRatio) + "%)";
        if (wallHard || memHard) {
            c.Verdict = D9PerfVerdict.Failed;
            c.Detail = detail + " — regression exceeds hard threshold";
        } else if (wallSoft || memSoft) {
            c.Verdict = D9PerfVerdict.Warning;
            c.Detail = detail + " — regression within soft threshold";
        } else {
            c.Verdict = D9PerfVerdict.Passed;
            c.Detail = detail;
        }
        return c;
    }

    /// <summary>比率 → 整数百分比（0 基线 → "0%"）。</summary>
    private static string Pct(double ratio) {
        if (ratio <= 0.0) {
            return "0";
        }
        return ((int)(ratio * 100.0)).ToString();
    }
}
