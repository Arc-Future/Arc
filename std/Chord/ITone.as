// ITone —— 音契约（RFC 045 D1/D7/D12）。
//
// 和弦（Chord）由音（Tone）构成：一个音 = 一个可编排的功能单元。
// 函数形态（Action<ChordContext> / Func<ChordContext, IDisposable>）与对象形态等价；
// 对象形态以 Name 显式命名（Arc 无实例类型反查，RFC 018 GetType() 永久剔除）。
namespace Arc.Chord;

/// <summary>
/// 对象形态音：Apply 在安装时立即执行，其副作用全部纳入音作用域账本。
/// </summary>
public interface ITone {
    /// <summary>音名（作用域 Name 使用）。</summary>
    string Name { get; }

    /// <summary>
    /// 安装入口：注册副作用 / 服务 / 事件等；抛异常将触发失败回滚
    /// （已注册副作用全部撤销、作用域置 Failed，不向安装方传播）。
    /// </summary>
    /// <param name="context">音专属子上下文。</param>
    /// <param name="config">音配置（未传入为 null）。</param>
    void Apply(ChordContext context, object? config);
}
