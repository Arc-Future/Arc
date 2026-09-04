namespace Arc.Illusory;

/// <summary>系统分相钩子——在固定步长内按 Begin/Update/End 驱动一次。</summary>
/// <remarks>
/// <paramref name="tick"/> 贯穿一次推进的三相，供系统据 Step/Time 做确定性决策。
/// 三相固定次序不可重排（诊断/快照对齐依赖此序）；M4 后由独立系统调度器按序分发。
/// </remarks>
public interface IRunnable {
    /// <summary>步启动：进入一次的确定性计算。</summary>
    void Begin(SimulationTick tick);

    /// <summary>步推进：执行本轮确定性更新。</summary>
    void Update(SimulationTick tick);

    /// <summary>步收尾：提交/快照当前步结果。</summary>
    void End(SimulationTick tick);
}