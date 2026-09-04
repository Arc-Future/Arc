// IContributeRegistry —— 统一注册表接口（RFC 045 D11 修订）。
//
// 桥接贡献项与插件容器的唯一调度入口：容器经 Add/Remove 运行期热插拔，
// 贡献项经 Register/Unregister 按容器 Id 定向注册。
namespace Arc.Chord;

/// <summary>
/// 统一注册表——插件容器与贡献项的统一管理入口。
/// </summary>
public interface IContributeRegistry {
    /// <summary>新增插件容器（运行期动态扩展功能域）；容器 Id 重复抛异常。</summary>
    void Add(IContributeHost host);

    /// <summary>移除插件容器（容器热插拔；未注册抛异常）。</summary>
    void Remove(IContributeHost host);

    /// <summary>将贡献项注册到指定容器（容器未注册抛异常）。</summary>
    void Register(string hostId, IContribute contribute, ContributeOptions options);

    /// <summary>从指定容器注销贡献项（与 Register 严格对称）。</summary>
    void Unregister(string hostId, IContribute contribute);

    /// <summary>容器是否已注册。</summary>
    bool HasHost(string hostId);
}
