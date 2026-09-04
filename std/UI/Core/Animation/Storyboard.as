// RFC 037 §3.6 Motion — Storyboard 并行动画完成聚合器。
//
// 设计：
//   - 调用方在 AddColor/AddDouble 时立即将「条目完成回调」注册到 MotionEngine；
//     MotionEngine 在动画完成（elapsedMs >= durationMs）时触发回调，Storyboard 计数。
//   - 全部条目完成（completedCount >= totalCount）时触发整体 Completed 回调。
//   - 零条目时 Begin 立即触发 Completed（对标 WPF Storyboard.Empty 语义）。
//
// 与 WPF 的差异：
//   Arc 的 MotionEngine 是被动插值引擎（渲染器每帧调用 ResolveColor/ResolveDouble 驱动），
//   Storyboard 不主动驱动动画——目标值由控件/渲染器设置，Storyboard 只聚合完成通知。
//   因此 Storyboard.Begin() 只设置整体回调；动画的开始由控件属性变化触发。
//
// 使用示例：
//   Storyboard sb = new Storyboard()
//       .AddColor(handle, MotionEngine.RoleBackground)
//       .AddDouble(handle, MotionEngine.RoleOpacity);
//   sb.Begin(() => { /* 全部动画完成 */ });
//   // 控件设置 Background/Opacity 触发动画 → 渲染器驱动 → 完成时回调

namespace Arc.UI.Animation;

using Arc.UI.Internal;

/// <summary>并行动画完成聚合器：注册多个动画完成回调，全部完成时触发整体回调。</summary>
public class Storyboard {
    private Action _onComplete;
    private int _completedCount;
    private int _totalCount;

    /// <summary>
    /// 添加颜色动画条目（立即注册完成回调到 MotionEngine）。
    /// 同一 (handle, role) 重复 Add 将覆写前次回调——同一槽位动画只完成一次。
    /// </summary>
    public Storyboard AddColor(long handle, int role) {
        this._totalCount++;
        MotionEngine.OnColorComplete(handle, role, this.NotifyEntryComplete);
        return this;
    }

    /// <summary>
    /// 添加 double 动画条目（立即注册完成回调到 MotionEngine）。
    /// 同一 (handle, role) 重复 Add 将覆写前次回调——同一槽位动画只完成一次。
    /// </summary>
    public Storyboard AddDouble(long handle, int role) {
        this._totalCount++;
        MotionEngine.OnDoubleComplete(handle, role, this.NotifyEntryComplete);
        return this;
    }

    /// <summary>
    /// 设置整体完成回调并启动计数。零条目时立即触发。
    /// 动画的实际开始由控件属性变化触发（MotionEngine 被动插值）。
    /// </summary>
    public void Begin(Action onComplete) {
        this._onComplete = onComplete;
        this._completedCount = 0;
        if (this._totalCount == 0) {
            if (this._onComplete != null) {
                this._onComplete();
            }
        }
    }

    /// <summary>条目完成通知（由 MotionEngine 回调触发）。</summary>
    private void NotifyEntryComplete() {
        this._completedCount++;
        if (this._completedCount >= this._totalCount) {
            if (this._onComplete != null) {
                this._onComplete();
            }
        }
    }
}
