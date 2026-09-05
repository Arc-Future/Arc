namespace Arc.Illusory;

using Arc.Collections;

/// <summary>世界组态——构造 <see cref="IWorld"/> 时注入的参数集。</summary>
/// <remarks>
/// 构造器校验后固定不可变：步长 <see cref="FixedStepMilliseconds"/> 须为正；
/// 服务列表承载 DI 注入的引擎服务（IInputService/IPhysicsWorld 等），M2 起对接既有服务容器。
/// </remarks>
public class WorldOptions {
    /// <summary>固定步长（毫秒）默认值：1000/60 ≈ 16.667（60Hz）。</summary>
    private const float DefaultStepMilliseconds = 16.666666f;

    /// <summary>固定步长（毫秒）。恒定，不随帧率漂移。</summary>
    public float FixedStepMilliseconds { get; }

    /// <summary>注入的引擎服务列表。</summary>
    public IReadOnlyList<object> Services { get; }

    /// <summary>构造默认世界组态（60Hz 固定步长、无预设服务）。</summary>
    public WorldOptions() {
        this.FixedStepMilliseconds = DefaultStepMilliseconds;
        this.Services = new ReadOnlyCollection<object>(new List<object>());
    }

    /// <summary>构造指定步长与服务的世界组态。</summary>
    /// <param name="fixedStepMilliseconds">固定步长（毫秒），必须为正。</param>
    /// <param name="services">注入的引擎服务列表。</param>
    public WorldOptions(float fixedStepMilliseconds, IReadOnlyList<object> services) {
        if (fixedStepMilliseconds <= 0.0f)
        {
            throw new ArgumentException("FixedStepMilliseconds must be positive.");
        }
        this.FixedStepMilliseconds = fixedStepMilliseconds;
        this.Services = services;
    }
}