// RFC 037 §3.6 Motion — SequentialChain 顺序动画链。
//
// 设计：
//   - 每步包含 (handle, role, isColor, applyTarget)；applyTarget 由调用方实现，
//     在该步启动时调用以设置控件属性（触发 MotionEngine 过渡）。
//   - Begin 启动第一步：注册完成回调 + 调用 applyTarget 设置目标值。
//   - 渲染器每帧调用 ResolveColor/ResolveDouble 驱动插值；动画完成时 MotionEngine 触发回调。
//   - 回调中推进到下一步：注册下一步回调 + 调用下一步 applyTarget。
//   - 最后一步完成时触发整体 Completed 回调。
//
// 与 Storyboard 的差异：
//   Storyboard 是并行聚合（全部同时进行，全部完成触发）；SequentialChain 是顺序串行
//   （前一步完成才启动下一步）。SequentialChain 内部用平行 List 存储步骤数据
//   （与 MotionEngine 槽位模式一致），按索引推进。
//
// 使用示例：
//   SequentialChain chain = new SequentialChain();
//   chain.AddColorStep(handle, MotionEngine.RoleBackground, () => { btn.Background = "#FF0000"; })
//        .AddColorStep(handle, MotionEngine.RoleBackground, () => { btn.Background = "#00FF00"; })
//        .AddColorStep(handle, MotionEngine.RoleBackground, () => { btn.Background = "#0000FF"; });
//   chain.Begin(() => { /* 三步全部完成 */ });

namespace Arc.UI.Animation;

using Arc.UI.Internal;

/// <summary>顺序动画链：每步完成触发下一步，最后一步完成触发整体回调。</summary>
public class SequentialChain {
    private Action _onComplete;
    private int _currentStep;
    private bool _running;

    // ---- 步骤平行表（对齐 MotionEngine 平行 List 模式）----
    private List<long> _stepHandle = new List<long>();
    private List<int> _stepRole = new List<int>();
    private List<int> _stepIsColor = new List<int>();
    private List<Action> _stepApply = new List<Action>();

    /// <summary>添加颜色动画步骤（顺序执行）。</summary>
    public SequentialChain AddColorStep(long handle, int role, Action applyTarget) {
        this._stepHandle.Add(handle);
        this._stepRole.Add(role);
        this._stepIsColor.Add(1);
        this._stepApply.Add(applyTarget);
        return this;
    }

    /// <summary>添加 double 动画步骤（顺序执行）。</summary>
    public SequentialChain AddDoubleStep(long handle, int role, Action applyTarget) {
        this._stepHandle.Add(handle);
        this._stepRole.Add(role);
        this._stepIsColor.Add(0);
        this._stepApply.Add(applyTarget);
        return this;
    }

    /// <summary>启动顺序动画链。零步骤时立即触发整体回调。</summary>
    public void Begin(Action onComplete) {
        this._onComplete = onComplete;
        this._currentStep = 0;
        this._running = true;
        this.RunCurrentStep();
    }

    /// <summary>运行当前步骤：注册完成回调 + 调用 applyTarget 设置目标值。</summary>
    private void RunCurrentStep() {
        if (this._currentStep >= this._stepHandle.Count) {
            this._running = false;
            if (this._onComplete != null) {
                this._onComplete();
            }
            return;
        }
        long handle = this._stepHandle[this._currentStep];
        int role = this._stepRole[this._currentStep];
        int isColor = this._stepIsColor[this._currentStep];
        Action applyTarget = this._stepApply[this._currentStep];
        // 注册完成回调（覆写前步残留，确保只触发一次）。
        if (isColor != 0) {
            MotionEngine.OnColorComplete(handle, role, this.OnStepComplete);
        } else {
            MotionEngine.OnDoubleComplete(handle, role, this.OnStepComplete);
        }
        // 调用 applyTarget 设置控件属性 → 触发 MotionEngine 过渡 → 渲染器驱动 → 完成回调。
        if (applyTarget != null) {
            applyTarget();
        }
    }

    /// <summary>当前步骤完成通知（由 MotionEngine 回调触发）：推进到下一步。</summary>
    private void OnStepComplete() {
        if (!this._running) {
            return;
        }
        this._currentStep++;
        this.RunCurrentStep();
    }
}
