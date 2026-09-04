// AIModelResult — 服务基座执行结果基类（RFC 041 §7.3）。
//
// 抽象标记基类（对齐 RFC §7.5 AIUnderstandPart 的「抽象基类禁 object 袋」品味）：
// 域实现（P2 起 AIOcrResult/AIEmbedResult 等）可派生本类型经统一骨架返回；P1 只
// 承载基座契约，不引入领域类型进 Arc.AI 核心。
namespace Arc.AI;

/// <summary>统一服务基座执行结果基类（RFC 041 §7.3；域结果派生，禁 object 袋）。</summary>
public abstract class AIModelResult {
}
