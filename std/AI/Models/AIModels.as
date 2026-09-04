// RFC 041 §7.5：AIModels — 统一模型服务门面（唯一入口，域是门面上的子面）。
//
// 一次 `new AIModels(registry)` 构造，随后 `models.Ocr.RecognizeAsync(...)` /
// `models.Asr.TranscribeAsync(...)` / `models.Embed.EmbedAsync(...)` ——域是门面上的
// 轻量只读子面，非独立类；内部经注册表取句柄 + §7.3 服务骨架，共享全局 Options。
// 门面不拥有注册表（构造注入，组合根持有）；Dispose 幂等仅标记已释放，不释放注册表。
// 零 Agent 依赖（依赖方向 Arc.AI.Models → Arc.AI）。
namespace Arc.AI.Models;

using Arc;
using Arc.AI;

/// <summary>统一模型服务门面（RFC 041 §7.5）。</summary>
public class AIModels : IDisposable {
    private bool _disposed;

    /// <summary>唯一构造（默认 Options）。</summary>
    public AIModels(AIModelRegistry registry) {
        this.Init(registry, AIModelsOptions.Default);
    }

    /// <summary>唯一构造（显式 Options）。注册表为注入（组合根持有，门面不拥有）。</summary>
    public AIModels(AIModelRegistry registry, AIModelsOptions options) {
        this.Init(registry, options);
    }

    /// <summary>共享初始化（Arc 不支持 `: this()` 构造链，经私有方法承载）。</summary>
    private void Init(AIModelRegistry registry, AIModelsOptions options) {
        if (registry == null) {
            throw new ArgumentNullException("registry");
        }
        Registry = registry;
        Options = options != null ? options : AIModelsOptions.Default;
        Ocr = new AIOcrFace(Registry, Options.OcrModelId, this.BuildServiceOptions());
        Asr = new AIAsrFace(Registry, Options.AsrModelId, this.BuildServiceOptions());
        Embed = new AIEmbedFace(Registry, Options.EmbedModelId, this.BuildServiceOptions());
        Tts = new AITtsFace(Registry, Options.TtsModelId, this.BuildServiceOptions());
        Clip = new AIClipFace();
        Face = new AIFaceFace();
        Vision = new AIVisionFace();
        _disposed = false;
    }

    /// <summary>OCR 域子面（RecognizeAsync / RecognizeBatchAsync）。</summary>
    public AIOcrFace Ocr { get; }

    /// <summary>ASR 域子面（TranscribeAsync / TranscribeBatchAsync）。</summary>
    public AIAsrFace Asr { get; }

    /// <summary>嵌入域子面（EmbedAsync / EmbedOneAsync）。</summary>
    public AIEmbedFace Embed { get; }

    /// <summary>CLIP 域子面（后续 P4）。</summary>
    public AIClipFace Clip { get; }

    /// <summary>人脸域子面（后续 P4）。</summary>
    public AIFaceFace Face { get; }

    /// <summary>TTS 域子面（后续 P4）。</summary>
    public AITtsFace Tts { get; }

    /// <summary>多模态理解域子面（后续 P4）。</summary>
    public AIVisionFace Vision { get; }

    /// <summary>绑定注册表（统计/预算审计面）。</summary>
    public AIModelRegistry Registry { get; }

    /// <summary>内存预算（只读统计/记账可审计）。</summary>
    public AIModelBudget Budget {
        get { return Registry.Budget; }
    }

    /// <summary>全局默认 Options。</summary>
    public AIModelsOptions Options { get; }

    /// <summary>幂等释放：仅标记已释放；注册表生命周期由组合根持有（注入，不在此释放）。</summary>
    public void Dispose() {
        if (_disposed) {
            return;
        }
        _disposed = true;
    }

    /// <summary>由全局 Options 构造每域服务选项（超时/重试/退避）。</summary>
    private AIModelServiceOptions BuildServiceOptions() {
        AIModelServiceOptions o = new AIModelServiceOptions();
        o.TimeoutMs = Options.TimeoutMs;
        o.MaxRetries = Options.MaxRetries;
        o.RetryBackoffMs = Options.RetryBackoffMs;
        return o;
    }
}
