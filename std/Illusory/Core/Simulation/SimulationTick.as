namespace Arc.Illusory;

/// <summary>不可变仿真步印——一切确定性计算的唯一时间源与回放/预测锚点。</summary>
/// <remarks>
/// 时间语义由 World 的固定步长推进保证：<see cref="Step"/> 单调递增、<see cref="DeltaTime"/> 恒定，
/// <see cref="Time"/> 恒为积 <c>Step * DeltaTime</c>。物理/行为/网络快照一律引用步印，不读墙钟。
/// </remarks>
public readonly struct SimulationTick {
    private readonly int _step;
    private readonly float _time;
    private readonly float _deltaTime;

    /// <summary>单调递增步号（从 1 起）。确定性回放/网络快照的锚点。</summary>
    public int Step {
        get { return _step; }
    }

    /// <summary>累计仿真时间（秒）。</summary>
    public float Time {
        get { return _time; }
    }

    /// <summary>恒定固定步长（秒）。</summary>
    public float DeltaTime {
        get { return _deltaTime; }
    }

    /// <summary>构造一步仿真步印（由内部 Simulation 按固定步长推进生成，不对外公开可变入口）。</summary>
    internal SimulationTick(int step, float time, float deltaTime) {
        _step = step;
        _time = time;
        _deltaTime = deltaTime;
    }
}