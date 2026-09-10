// RFC 037 M5: Arc.UI.Components — ICommand 命令接口。
//
// 设计决策：Arc 无 C# event 体系；本切片仅 CanExecute/Execute 同步面。
// CanExecuteChanged（Signal 推送 → 自动刷新 IsEnabled）为后移项——调用方在
// RaiseClick / 程序化查询时再读 CanExecute（诚实最小面，禁假开推送通道）。
//
// 与 WPF ICommand 对比：
//   WPF: event EventHandler CanExecuteChanged
//   Arc: 本面无变更通知通道（后置）；点击路径经 Button.RaiseClick 再查
//
// 使用模式：
//   - MVVM: RelayCommand / 自定义 ICommand → Button.Command
//   - 简单场景：直接用 Button.Clicked / OnClick

namespace Arc.UI.Components;

/// <summary>
/// 命令接口——封装可执行的用户操作，支持启用/禁用查询。
/// </summary>
/// <remarks>
/// 变更通知（对标 WPF CanExecuteChanged）未纳入本面；见文件头诚实边界。
/// 最小实现见 <see cref="RelayCommand"/>；Button.RaiseClick 在 IsEnabled 通过后
/// 若 Command 为 ICommand 且 CanExecute 为真则 Execute，并仍触发 Clicked。
/// </remarks>
public interface ICommand {
    /// <summary>判断命令当前是否可执行。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    /// <returns>true 可执行；false 禁用状态。</returns>
    bool CanExecute(object parameter);

    /// <summary>执行命令逻辑。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    void Execute(object parameter);
}
