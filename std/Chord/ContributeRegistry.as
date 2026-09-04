// ContributeRegistry —— IContributeRegistry 默认实现（RFC 045 D11 修订）。
//
// 容器 Id → 容器映射；Register 按容器 Id 路由并透传 ContributeOptions。
namespace Arc.Chord;

using Arc.Collections;


public class ContributeRegistry : IContributeRegistry {
    private Dictionary<string, IContributeHost> _hosts;

    public ContributeRegistry() {
        _hosts = new Dictionary<string, IContributeHost>();
    }

    /// <summary>容器是否已注册。</summary>
    public bool HasHost(string hostId) {
        return _hosts.ContainsKey(hostId);
    }

    /// <summary>新增插件容器（容器 Id 重复抛异常）。</summary>
    public void Add(IContributeHost host) {
        if (_hosts.ContainsKey(host.Id)) {
            throw new Exception("Arc.Chord: 插件容器重复注册: " + host.Id);
        }
        _hosts[host.Id] = host;
    }

    /// <summary>移除插件容器（未注册抛异常）。</summary>
    public void Remove(IContributeHost host) {
        if (!_hosts.ContainsKey(host.Id)) {
            throw new Exception("Arc.Chord: 插件容器未注册: " + host.Id);
        }
        _hosts.Remove(host.Id);
    }

    /// <summary>将贡献项注册到指定容器（容器未注册抛异常）。</summary>
    public void Register(string hostId, IContribute contribute, ContributeOptions options) {
        if (!_hosts.ContainsKey(hostId)) {
            throw new Exception("Arc.Chord: 插件容器未注册: " + hostId);
        }
        _hosts[hostId].Register(contribute, options);
    }

    /// <summary>从指定容器注销贡献项（与 Register 严格对称）。</summary>
    public void Unregister(string hostId, IContribute contribute) {
        if (!_hosts.ContainsKey(hostId)) {
            throw new Exception("Arc.Chord: 插件容器未注册: " + hostId);
        }
        _hosts[hostId].Unregister(contribute);
    }
}
