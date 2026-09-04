// RFC 041 §7.5：AIAudioInput — 音频语义输入值类型（ASR / TTS 共用）。
//
// 服务内部翻译为 Float32 [1,N] 张量进 IAIModel（§7.4 语义 I/O）；PCM 解码在
// std 库，不碰编译器。P2 核心工厂 FromPcmFloat / FromPcmInt16（WAV/文件解码 P4）。
namespace Arc.AI.Models;

using Arc.AI;
using Arc.Collections;

/// <summary>音频输入（RFC 041 §7.5）：PCM float 采样（-1..1）+ 采样率/声道。</summary>
public class AIAudioInput {
    /// <summary>PCM float 采样（-1..1）。</summary>
    public List<float> Samples { get; set; }
    /// <summary>采样率（如 16000 / 24000）。</summary>
    public int SampleRate { get; set; }
    /// <summary>声道数（1/2）。</summary>
    public int Channels { get; set; }

    public AIAudioInput() {
        this.Samples = new List<float>();
        this.SampleRate = 16000;
        this.Channels = 1;
    }

    /// <summary>从 PCM float 采样构造（-1..1）。</summary>
    public static AIAudioInput FromPcmFloat(List<float> samples, int sampleRate, int channels) {
        AIAudioInput input = new AIAudioInput();
        input.SampleRate = sampleRate;
        input.Channels = channels;
        if (samples != null) {
            int i = 0;
            while (i < samples.Count) {
                input.Samples.Add(samples[i]);
                i = i + 1;
            }
        }
        return input;
    }

    /// <summary>从 PCM int16 采样构造（归一化为 float：v / 32768）。</summary>
    public static AIAudioInput FromPcmInt16(List<short> samples, int sampleRate, int channels) {
        AIAudioInput input = new AIAudioInput();
        input.SampleRate = sampleRate;
        input.Channels = channels;
        if (samples != null) {
            int i = 0;
            while (i < samples.Count) {
                input.Samples.Add((float)((double)samples[i] / 32768.0));
                i = i + 1;
            }
        }
        return input;
    }

    /// <summary>翻译为模型输入张量列表（单 Float32 [1,N]）。</summary>
    public List<Tensor> ToInputs() {
        List<long> shape = new List<long>();
        shape.Add((long)1);
        shape.Add((long)this.Samples.Count);
        List<Tensor> inputs = new List<Tensor>();
        inputs.Add(Tensor.CreateFloat(shape, this.Samples));
        return inputs;
    }
}
