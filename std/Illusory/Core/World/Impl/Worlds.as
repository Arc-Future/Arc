namespace Arc.Illusory;

/// <summary>世界创建门面——统一入口构造 <see cref="IWorld"/>。</summary>
public static class Worlds {
    /// <summary>以指定组态创建世界。</summary>
    /// <param name="options">世界组态（固定步长、服务列表）。</param>
    public static IWorld Create(WorldOptions options) {
        return new World(options);
    }
}