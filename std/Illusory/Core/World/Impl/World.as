namespace Arc.Illusory;

using Arc.Collections;
using Arc.Math;

/// <summary>IWorld 实现——组合固定步长仿真与有序系统注册表，对外提供统一入口。</summary>
/// <remarks>
/// <see cref="World"/> 为 internal 编排，不暴露其状态机；开发者仅经 <see cref="IWorld"/>
/// 门面交互。创建入口见 <see cref="Worlds"/>。固定步进驱动 <see cref="SystemRegistry"/>，
/// 由其按确定性阶段序三相驱动全部注册系统（含 Actor 注册表）。
/// </remarks>
internal class World : IWorld {
    private readonly Simulation _simulation;
    private readonly ActorRegistry _registry;
    private readonly SystemRegistry _systemRegistry;

    internal World(WorldOptions options) {
        _registry = new ActorRegistry();
        _systemRegistry = new SystemRegistry();
        _systemRegistry.Register(_registry, SystemPhase.Zero);
        IReadOnlyList<SystemRegistration> systems = options.Systems;
        for (int i = 0; i < systems.Count; i++)
        {
            _systemRegistry.Register(systems[i].System, systems[i].Phase);
        }
        _systemRegistry.Build();
        _simulation = new Simulation(options);
    }

    public SimulationTick CurrentTick {
        get { return _simulation.CurrentTick; }
    }

    public void Update(float frameDeltaMilliseconds) {
        _simulation.Update(frameDeltaMilliseconds, _systemRegistry);
    }

    public Actor SpawnActor(Transform initial) {
        return _registry.Spawn(initial, default(GameplayTags));
    }

    public Actor SpawnActor(Transform initial, GameplayTags tags) {
        return _registry.Spawn(initial, tags);
    }

    public bool TryDestroyActor(ActorId id) {
        return _registry.Destroy(id);
    }

    public bool TryGetActor(ActorId id, out Actor actor) {
        return _registry.Get(id, out actor);
    }

    public void AddComponent(ActorId actorId, IComponent component) {
        _registry.AddComponent(actorId, component);
    }

    public bool RemoveComponent(ActorId actorId, IComponent component) {
        return _registry.RemoveComponent(actorId, component);
    }

    public bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent {
        return _registry.TryGetComponent(actorId, out component);
    }
}