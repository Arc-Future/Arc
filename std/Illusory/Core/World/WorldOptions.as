namespace Arc.Illusory;

using Arc.Collections;
using Arc.DI;

/// <summary>世界组态——构造 <see cref="IWorld"/> 时注入的参数集。</summary>
/// <remarks>
/// 构造器校验后固定不可变：步长 <see cref="FixedStepMilliseconds"/> 须为正；
/// <see cref="Services"/> 承载 DI 注入的引擎服务（导航/持久化等 L3 端口），改为真正的
/// 服务容器；<see cref="Systems"/> 承载按确定性阶段序注册的系统。推荐经 <see cref="WorldBuilder"/>
/// 装配，亦可直接构造（默认 60Hz、空服务、空系统）。
/// </remarks>
public class WorldOptions {
    /// <summary>固定步长（毫秒）。恒定，不随帧率漂移。</summary>
    public float FixedStepMilliseconds { get; }

    /// <summary>注入的引擎服务容器（组合根装配）。</summary>
    public IServiceProvider Services { get; }

    /// <summary>按确定性阶段序注册的系统（组合根装配）。</summary>
    internal IReadOnlyList<SystemRegistration> Systems { get; }

    /// <summary>构造默认世界组态（60Hz 固定步长、空服务、空系统）。</summary>
    public WorldOptions() {
        this.FixedStepMilliseconds = 1000.0f / 60.0f;
        this.Services = new ServiceCollection().Build();
        this.Systems = new ReadOnlyCollection<SystemRegistration>(new List<SystemRegistration>());
    }

    /// <summary>构造指定步长与服务列表的世界组态。</summary>
    /// <param name="fixedStepMilliseconds">固定步长（毫秒），必须为正。</param>
    /// <param name="services">注入服务列表（向后兼容入口，推入空服务容器）。</param>
    public WorldOptions(float fixedStepMilliseconds, IReadOnlyList<object> services) {
        if (fixedStepMilliseconds <= 0.0f)
        {
            throw new ArgumentException("FixedStepMilliseconds must be positive.");
        }
        this.FixedStepMilliseconds = fixedStepMilliseconds;
        this.Services = new ServiceCollection().Build();
        this.Systems = new ReadOnlyCollection<SystemRegistration>(new List<SystemRegistration>());
    }

    /// <summary>构造完整组态（组合根专用）。</summary>
    /// <param name="fixedStepMilliseconds">固定步长（毫秒），必须为正。</param>
    /// <param name="services">注入的服务容器。</param>
    /// <param name="systems">按确定性阶段序注册的系统。</param>
    internal WorldOptions(float fixedStepMilliseconds, IServiceProvider services, IReadOnlyList<SystemRegistration> systems) {
        this.FixedStepMilliseconds = fixedStepMilliseconds;
        this.Services = services;
        this.Systems = systems;
    }
}