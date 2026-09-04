// IConfiguration —— 强类型配置契约（RFC 039 §1.4）：Get<T>() 反序列化获取强类型配置。
//
// 归属：标准库核心（Arc.Configuration）。配置读取是通用框架能力，不限于 Web；
// 故契约与实现均置于核心库，宿主（如 Arc.Web WebApplication）自动解析
// appSettings.json + appSettings.{env}.json 并注册单例，处理器经 DI 强类型读取。
// 显式 > 隐式：无 IOptions<T>/IOptionsMonitor 间接层。
//
// 单一惯用法：整个 Arc 仅此一个 IConfiguration 契约，禁止宿主/集成层各自声明同名
// 配置接口（Arc.Web 曾自建按 key 读取的 IConfiguration，属架构异味，已撤销）。
//
// 对标 C#：语义对齐 Microsoft.Extensions.Configuration.Binder 的
// `IConfiguration.Get<T>()`（整个配置绑定为强类型 T）。C# 借助反射免约束；
// Arc 无反射，以 `where T : IJsonDeserializable, new()` 显式声明反序列化能力。
namespace Arc.Configuration;
using Arc.Text.Json;

/// <summary>
/// 强类型配置契约：整体反序列化 + 按 key 的片段/标量读取。
/// 实现方约定：默认解析 appSettings.json + appSettings.{env}.json（ARC_ENV 指定
/// 环境，默认 Production，环境文件层叠覆盖基础文件），宿主自动注册为单例。
/// 对标 C# Microsoft.Extensions.Configuration：GetSection(key) / GetValue&lt;T&gt;(key) /
/// Get&lt;T&gt;() 三者语义分别对齐 GetSection、GetValue&lt;T&gt;（Binder 标量）、Get&lt;T&gt;（Binder 整体）。
/// </summary>
public interface IConfiguration {
    /// <summary>整体反序列化为强类型 T（对标 C# IConfiguration.Get&lt;T&gt;()）。</summary>
    T Get<T>() where T : IJsonDeserializable, new();

    /// <summary>按 key 取配置片段（子配置）；key 用 ':' 分隔嵌套路径，未命中返回空片段
    /// （对标 C# IConfiguration.GetSection(key)，空片段可继续 Get&lt;T&gt;() 得默认值）。</summary>
    IConfiguration GetSection(string key);

    /// <summary>按 key 取标量值并转换为 T；key 缺失或值为对象/数组/null 返回 default(T)
    /// （对标 C# IConfiguration.GetValue&lt;T&gt;(key)）。诚实边界：按 JSON 实际 token 类型
    /// 转换（string/int/bool），请求类型与 token 不符为 unbox 不匹配（硬错误，禁静默强转）。</summary>
    T GetValue<T>(string key);
}
