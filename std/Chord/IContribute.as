// IContribute —— 贡献点接口（RFC 045 D11 修订，插件容器热插拔架构）。
//
// 插件模块提供扩展功能的基础契约：每个贡献项以唯一标识 Id 区分。
// 契约不引用上下文（剥离语言核心）；可逆性由 ChordContextExtensions
// 经副作用账本组合（音卸载自动 Unregister）。
namespace Arc.Chord;

/// <summary>
/// 贡献点——插件交付给插件容器（贡献主机）的扩展项。
/// </summary>
public interface IContribute {
    /// <summary>贡献项唯一标识。</summary>
    string Id { get; }
}
