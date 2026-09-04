// IreeException — IREE Runtime 原生调用失败时抛出。
//
// shim（iree_shim.cpp）把所有 IREE Runtime C 状态码转换为返回码 + 末次错误串
// （iree_last_error）；本库把这些返回码统一收敛为 IreeException，显式失败面，
// 禁静默吞错。模型加载、会话创建、推理运行等失败均抛本类型。
namespace Arc.AI.Iree;

using Arc;

/// <summary>IREE Runtime 操作失败（模块加载 / 推理 / 缓冲创建等）。</summary>
public class IreeException : SystemException {
    public IreeException() : base() { }
    public IreeException(string message) : base(message) { }
    public IreeException(string message, Exception? innerException) : base(message, innerException) { }
}
