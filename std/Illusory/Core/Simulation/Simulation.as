namespace Arc.Illusory;

/// <summary>固定步长仿真编排——累加帧耗时、切分到 SimulationTick 并按三相驱动系统。</summary>
/// <remarks>
/// 状态机不对外暴露，仅经 <see cref="IWorld"/> 间接驱动。固定步长 while 累加：
/// 一帧可消耗多个步印（减缓时补足），余量累计到下帧，保证 DeltaTime 恒定不漂移。
/// </remarks>
internal class Simulation {
    private readonly float _fixedStepMilliseconds;
    private float _accumulator;
    private int _step;
    private SimulationTick _currentTick;

    internal Simulation(WorldOptions options) {
        _fixedStepMilliseconds = options.FixedStepMilliseconds;
        _accumulator = 0.0f;
        _step = 0;
    }

    /// <summary>最近一次已推进的步印（对外读只读当前仿真进度）。</summary>
    internal SimulationTick CurrentTick {
        get { return _currentTick; }
    }

    /// <summary>按固定步长切分一帧并驱动 runner 三相；无 I/O，同步。</summary>
    internal void Update(float frameDeltaMilliseconds, IRunnable runner) {
        _accumulator += frameDeltaMilliseconds;
        while (_accumulator >= _fixedStepMilliseconds)
        {
            int nextStep = _step + 1;
            Advance(nextStep, runner);
            _accumulator -= _fixedStepMilliseconds;
            _step = nextStep;
        }
    }

    private void Advance(int step, IRunnable runner) {
        float time = (float)step * _fixedStepMilliseconds / 1000.0f;
        float deltaTime = _fixedStepMilliseconds / 1000.0f;
        SimulationTick tick = new SimulationTick(step, time, deltaTime);
        _currentTick = tick;
        runner.Begin(tick);
        runner.Update(tick);
        runner.End(tick);
    }
}