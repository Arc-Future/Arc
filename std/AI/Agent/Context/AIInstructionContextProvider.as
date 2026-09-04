// RFC 038 上下文成体系：AIInstructionContextProvider — Instructions（系统指令）内置上下文源。
//
// 将 AISessionOptions.Instructions 封装为 AIContextProvider：产成为 Rules 层最前上下文块
// （注册序第一 → 前缀字节稳定 → KV cache 命中）。RFC 038：由 Host 侧经 AddProvider
// 作为普通 provider 注册；指令为空则不注册（无贡献）。静态源——忽略 query，返回已完成
// Task；不实现调用后方向（ProcessMessageAsync 走默认空实现）。
namespace Arc.Agent;
using Arc.Collections;

/// <summary>
/// 内置上下文源：系统指令。把用户配置的 Instructions 作为 Rules 层上下文块产出，
/// 由 <see cref="AIContextEngine"/>（Host 级组合根）统一组装到请求面最前。RFC 038
/// 收编为普通 provider：由宿主（AIHost）注册，开发者可整体替换 / 移除。
/// </summary>
public class AIInstructionContextProvider : AIContextProvider {
    private string _instructions;

    public AIInstructionContextProvider(string instructions) {
        _instructions = instructions != null ? instructions : "";
    }

    public override string GetName() { return "instructions"; }

    public override string GetDescription() { return "System instructions (Rules layer, first)."; }

    /// <summary>静态源：忽略 query/session，返回已完成 Task。</summary>
    public override Task<List<AIContextBlock>> ProvideContextAsync(AIContextQuery query, AIContextSession session, CancellationToken cancellationToken) {
        List<AIContextBlock> list = new List<AIContextBlock>();
        if (_instructions != "") {
            // 无标题 → ToMessage 正文 == Instructions 原文（缓存前缀字节稳定；不加修饰前缀）。
            list.Add(new AIContextBlock("instructions", "Rules", 0, _instructions));
        }
        return Task.FromResult(list);
    }
}
