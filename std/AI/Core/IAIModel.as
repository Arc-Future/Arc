// RFC 041 §1.5: Arc.AI 共享抽象核心 — 统一推理执行接口 IAIModel。
//
// 后端无关的执行契约：ONNX 的 InferenceSession 与 IREE 的 IreeSession 各自实现本
// 接口（把各自原生张量转换为 Arc.AI.Tensor），业务侧统一消费、不感知后端差异
// （RFC 041 §1.1 各读各自域 · 禁双轨 API）。输入/输出按位置（List<Tensor>）传递，
// 名字经 GetInputName(i) 元数据取（对齐 ONNX 命名输入 / IREE 位置形参）。
//
// 异步为主（AI 域异步优先）：唯一执行入口为 RunAsync，带取消令牌（协作式取消，
// 已取消则抛 OperationCanceledException）；不提供同步孪生（单一惯用法，禁便利双轨）。
// 推理为 CPU 密集原生调用：后端实现负责把阻塞执行迁移出调用线程并保持取消语义。
namespace Arc.AI;

using Arc.Collections;

/// <summary>
/// 统一推理执行接口（后端无关 · 异步为主）。输入/输出均为宿主张量
/// <see cref="Tensor"/>；经 <see cref="RunAsync"/> 唯一执行入口调度。
/// 除执行外，本接口还暴露模型 I/O 元数据（数量 / 名字 / 元素类型 / 形状），
/// 使业务侧在运行前即可构造正确的输入张量（能力支持）。
/// </summary>
public interface IAIModel : IDisposable {
    /// <summary>模型输入张量数量。</summary>
    int InputCount { get; }

    /// <summary>模型输出张量数量。</summary>
    int OutputCount { get; }

    /// <summary>取第 <paramref name="index"/> 个输入张量名（无名字后端可返回空串）。</summary>
    string GetInputName(int index);

    /// <summary>取第 <paramref name="index"/> 个输入张量元素类型（后端无此元数据返回
    /// <see cref="TensorElementType.Undefined"/>）。</summary>
    TensorElementType GetInputElementType(int index);

    /// <summary>取第 <paramref name="index"/> 个输出张量元素类型（后端无此元数据返回
    /// <see cref="TensorElementType.Undefined"/>）。</summary>
    TensorElementType GetOutputElementType(int index);

    /// <summary>取第 <paramref name="index"/> 个输入张量形状（未知维为 -1；后端无形状
    /// 元数据返回空表）。</summary>
    List<long> GetInputShape(int index);

    /// <summary>取第 <paramref name="index"/> 个输出张量形状（未知维为 -1；后端无形状
    /// 元数据返回空表）。</summary>
    List<long> GetOutputShape(int index);

    /// <summary>异步执行推理。输入按位置提供；返回全部输出（位置序）。</summary>
    /// <param name="inputs">按位置提供的输入张量（数量须等于 <see cref="InputCount"/>）。</param>
    /// <param name="cancellationToken">协作式取消令牌（已取消抛 <see cref="OperationCanceledException"/>）。</param>
    /// <returns>位置序输出张量列表。</returns>
    Task<List<Tensor>> RunAsync(List<Tensor> inputs, CancellationToken cancellationToken);
}
