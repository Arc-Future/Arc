// RFC 041 §7.5：AIImageInput — 图像语义输入值类型。
//
// 服务内部翻译为 UInt8 [H,W,C] 张量进 IAIModel（§7.4 语义 I/O）；像素本体在
// std 库，不碰编译器。FromPixels 为 P2 核心工厂（FromFile/FromBase64 媒体解码 P4）。
// 字段名 Data（类型 Arc.AI.Tensor）：Arc 字段名遮蔽类型名（`Tensor` 字段会让静态工厂
// 的 `Tensor.CreateByte` 被解析为实例字段），故以 Data 命名承载像素张量。
namespace Arc.AI.Models;

using Arc.AI;
using Arc.Collections;

/// <summary>图像输入（RFC 041 §7.5）：UInt8 [H,W,C] 像素张量 + 尺寸元数据。</summary>
public class AIImageInput {
    /// <summary>像素张量（UInt8 [H,W,C] 行主序）。</summary>
    public Tensor Data { get; set; }
    /// <summary>宽（像素）。</summary>
    public int Width { get; set; }
    /// <summary>高（像素）。</summary>
    public int Height { get; set; }
    /// <summary>通道数（1/3/4）。</summary>
    public int Channels { get; set; }

    public AIImageInput() {
    }

    /// <summary>从行主序 UInt8 像素构造（像素数须 = width * height * channels）。</summary>
    public static AIImageInput FromPixels(int width, int height, int channels, List<byte> pixels) {
        AIImageInput input = new AIImageInput();
        input.Width = width;
        input.Height = height;
        input.Channels = channels;
        List<long> shape = new List<long>();
        shape.Add((long)height);
        shape.Add((long)width);
        shape.Add((long)channels);
        input.Data = Tensor.CreateByte(shape, pixels);
        return input;
    }

    /// <summary>翻译为模型输入张量列表（单 UInt8 [H,W,C]）。</summary>
    public List<Tensor> ToInputs() {
        List<Tensor> inputs = new List<Tensor>();
        if (this.Data != null) {
            inputs.Add(this.Data);
        }
        return inputs;
    }
}
