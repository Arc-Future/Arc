// AIModelNotAvailableException — 模型后端不可用（RFC 041 §7.3）。
//
// 对齐 §3 门闩降级链：Native.IsAvailable == false（后端库未装 / 环境变量未配置）
// 时抛本类型。后端适配器（OnnxAIModelFactory/IreeAIModelFactory）在 Create 前先门闩
// 检查并抛本类型；注册表 EnsureLoaded 原样传播（不做降级处理）。
namespace Arc.AI;

using Arc;

/// <summary>模型后端不可用（未安装原生库 / 门闩灰化；RFC 041 §7.3）。</summary>
public class AIModelNotAvailableException : AIModelException {
    public AIModelNotAvailableException() : base() { }
    public AIModelNotAvailableException(string message) : base(message) { }
    public AIModelNotAvailableException(string message, Exception? innerException) : base(message, innerException) { }
}
