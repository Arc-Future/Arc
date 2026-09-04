// IContributeHost —— 插件容器接口（RFC 045 D11 修订）。
//
// 插件容器 = 一类扩展点的宿主（钩子），贡献项 = 挂件。容器本身可经
// IContributeRegistry.Add/Remove 在运行期热插拔；容器内贡献项经
// Register/Unregister 以 Group/Order/ParentId 组织。
namespace Arc.Chord;

/// <summary>
/// 插件容器——承载一类贡献项的扩展点宿主。Register/Unregister 严格对称，
/// 贡献项身份即撤销键；幂等性由实现负责。
/// </summary>
public interface IContributeHost {
    /// <summary>容器唯一标识（扩展点名，调度键）。</summary>
    string Id { get; }

    /// <summary>接收一条贡献（含组织元数据），加入容器管理。</summary>
    void Register(IContribute contribute, ContributeOptions options);

    /// <summary>注销一条贡献（与 Register 严格对称），移出容器。</summary>
    void Unregister(IContribute contribute);
}
