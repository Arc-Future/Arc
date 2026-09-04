namespace Arc.Illusory;

using Arc.Math;

/// <summary>引擎对外唯一入口——推进世界、生成/销毁 Actor、挂接组件。</summary>
/// <remarks>
/// <see cref="Update"/> 为同步无 I/O 的状态推进：内部按固定步长切分到 <see cref="SimulationTick"/>
/// 并驱动注册系统，异步一体原则下不做成 Async 版本。可失败/缺失操作一律 <c>Try* + out</c>。
/// </remarks>
public interface IWorld {
    /// <summary>最近已推进的仿真步印——供渲染插值/HUD/计时读取当前仿真进度，只读。</summary>
    SimulationTick CurrentTick { get; }

    /// <summary>推进一帧：内部按固定步长切分并驱动注册系统。无 I/O，同步。</summary>
    /// <param name="frameDeltaMilliseconds">帧耗时（毫秒）。一帧可消耗多个固定步长，余量累计到下帧。</param>
    void Update(float frameDeltaMilliseconds);

    /// <summary>生成并返回新 Actor（构造必成功）。</summary>
    Actor SpawnActor(Transform initial);

    /// <summary>生成并返回带标签集的 Actor（构造必成功）。</summary>
    Actor SpawnActor(Transform initial, GameplayTags tags);

    /// <summary>销毁指定 Actor。成功返回 true；Id 不存在返回 false。</summary>
    bool TryDestroyActor(ActorId id);

    /// <summary>获取指定 Actor。命中返回 true 并输出；未命中返回 false。</summary>
    bool TryGetActor(ActorId id, out Actor actor);

    /// <summary>给指定 Actor 挂接组件（Actor 不存在或无组件能力时静默忽略）。</summary>
    void AddComponent(ActorId actorId, IComponent component);

    /// <summary>移除指定组件。成功返回 true。</summary>
    bool RemoveComponent(ActorId actorId, IComponent component);

    /// <summary>按具体类型获取组件。命中返回 true 并输出；缺失返回 false。</summary>
    bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent;
}