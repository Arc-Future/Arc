// OnnxException — ONNX Runtime 原生调用失败时抛出。
//
// shim（onnx_shim.cpp）把所有 ONNX Runtime C++ 异常转换为返回码 + 末次错误串
// （onnx_last_error）；本库把这些返回码统一收敛为 OnnxException，显式失败面，
// 禁静默吞错。模型加载、会话创建、推理运行等失败均抛本类型。
namespace Arc.AI.Onnx;

using Arc;

/// <summary>ONNX Runtime 操作失败（模型加载 / 推理 / 张量创建等）。</summary>
public class OnnxException : SystemException {
    public OnnxException() : base() { }
    public OnnxException(string message) : base(message) { }
    public OnnxException(string message, Exception? innerException) : base(message, innerException) { }
}
