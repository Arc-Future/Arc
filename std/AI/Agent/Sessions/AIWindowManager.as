namespace Arc.Agent;
using Arc.Collections;
/// <summary>
/// 窗口裁剪（RFC 038）：保留系统消息 + 最近 K 条（计数可观测 TrimCount）。
/// 纯函数：不改写传入列表，返回裁剪后的新请求面。仅影响请求面，transcript 保持完整（可审计）。
/// </summary>
internal class AIWindowManager {
    /// <summary>
    /// 裁剪 <paramref name="messages"/>：当条数超过 <paramref name="keepLast"/>（&gt;0）时，
    /// 保留全部 System 消息 + 最近 K 条；否则原样返回。0 = 关闭裁剪。
    /// </summary>
    public static List<AIMessage> Trim(List<AIMessage> messages, int keepLast) {
        if (messages == null) {
            return new List<AIMessage>();
        }
        if (keepLast <= 0) {
            return messages;
        }
        int n = messages.Count;
        if (n <= keepLast) {
            return messages;
        }
        List<AIMessage> kept = new List<AIMessage>();
        int i = 0;
        while (i < n) {
            AIMessage m = messages[i];
            if (m.Role == AIRole.System) {
                kept.Add(m);
            }
            i = i + 1;
        }
        int start = n - keepLast;
        i = start;
        while (i < n) {
            kept.Add(messages[i]);
            i = i + 1;
        }
        return kept;
    }
}