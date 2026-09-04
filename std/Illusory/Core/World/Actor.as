namespace Arc.Illusory;

using Arc.Math;

/// <summary>Actor 壳——游戏世界中一切可动元素的身份与其上数据挂载点。纯数据，无行为。</summary>
/// <remarks>
/// 差异化靠挂组件而非派生子类。实例仅经 <c>World.SpawnActor</c> 创建
/// （内部构造器），变换/标签由引擎内部维护，对开发者只读。
/// </remarks>
public class Actor {
    /// <summary>全局唯一身份，作为确定性/回放/网络引用的锚点。</summary>
    public ActorId Id { get; }

    /// <summary>空间姿态。由 World 内部维护，对开发者只读。</summary>
    public Transform Transform { get; private set; }

    /// <summary>标签集（creature/attack/stun…），查询与互斥判据。</summary>
    public GameplayTags Tags { get; private set; }

    /// <summary>内部构造——仅供 World 注册表初始化壳数据。</summary>
    internal Actor(ActorId id, Transform transform, GameplayTags tags) {
        this.Id = id;
        this.Transform = transform;
        this.Tags = tags;
    }
}