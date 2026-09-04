namespace Arc.Illusory;

using Arc.Collections;
using Arc.Math;

/// <summary>Actor/组件注册表——生命周期与组件仓库的唯一管辖者，藏起分配与检索细节。</summary>
/// <remarks>
/// 承担 Id 单调分配与回收、Actor 查改、组件按类型检索。作为 IRunnable 接收固定步进分相，
/// 为后续行为层/组件驱动预留驱动点（M1 三相空转）。
/// </remarks>
internal class ActorRegistry : IRunnable {
    private readonly Dictionary<ActorId, Actor> _actors;
    private readonly Dictionary<ActorId, List<IComponent>> _components;
    private int _nextId;

    internal ActorRegistry() {
        _actors = new Dictionary<ActorId, Actor>();
        _components = new Dictionary<ActorId, List<IComponent>>();
        _nextId = 1;
    }

    /// <summary>分配新 Id 并登记 Actor 壳。Id 单调递增，从 1 起。</summary>
    internal Actor Spawn(Transform initial, GameplayTags tags) {
        ActorId id = new ActorId(_nextId);
        _nextId = _nextId + 1;
        Actor actor = new Actor(id, initial, tags);
        _actors.Add(id, actor);
        _components.Add(id, new List<IComponent>());
        return actor;
    }

    /// <summary>销毁 Actor 及其组件。Id 不存在返回 false。</summary>
    internal bool Destroy(ActorId id) {
        if (!_actors.ContainsKey(id))
        {
            return false;
        }
        _actors.Remove(id);
        _components.Remove(id);
        return true;
    }

    /// <summary>按 Id 取 Actor。命中返回 true 并输出；未命中返回 false。</summary>
    internal bool Get(ActorId id, out Actor actor) {
        return _actors.TryGetValue(id, out actor);
    }

    internal void AddComponent(ActorId actorId, IComponent component) {
        if (component == null || !_components.ContainsKey(actorId))
        {
            return;
        }
        _components[actorId].Add(component);
    }

    internal bool RemoveComponent(ActorId actorId, IComponent component) {
        if (component == null || !_components.ContainsKey(actorId))
        {
            return false;
        }
        return _components[actorId].Remove(component);
    }

    internal bool TryGetComponent<T>(ActorId actorId, out T component) where T : IComponent {
        List<IComponent> list = null;
        if (_components.TryGetValue(actorId, out list))
        {
            for (int i = 0; i < list.Count; i++)
            {
                IComponent item = list[i];
                if (item is T)
                {
                    component = (T)item;
                    return true;
                }
            }
        }
        component = null;
        return false;
    }

    public void Begin(SimulationTick tick) {
    }

    public void Update(SimulationTick tick) {
    }

    public void End(SimulationTick tick) {
    }
}