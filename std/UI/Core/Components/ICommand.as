// RFC 037 M5: Arc.UI.Components — ICommand 命令接口。
//
// 设计决策：Arc 无 C# event 体系；变更通知走 Signal<bool> 脉冲通道。
// CanExecuteChanged → Button.IsEnabled 自动同步由 Button 订阅本信号完成；
// 调用方在可执行态变化时 Raise（RelayCommand.RaiseCanExecuteChanged）或
// 对自有 Signal 执行 Set。
//
// 与 WPF ICommand 对比：
//   WPF: event EventHandler CanExecuteChanged + CommandManager.RequerySuggested
//   Arc: Signal<bool> CanExecuteChanged（显式 Raise；无 CommandManager 全局重查）
//
// 使用模式：
//   - MVVM: RelayCommand / 自定义 ICommand → Button.Command
//   - 简单场景: 直接用 Button.Clicked / OnClick

namespace Arc.UI.Components;

using Arc;

/// <summary>
/// 命令接口——封装可执行的用户操作，支持启用/禁用查询与变更通知。
/// </summary>
/// <remarks>
/// <see cref="CanExecuteChanged"/> 为变更脉冲（Signal）；Button 订阅后直写
/// <c>IsEnabled = CanExecute(param)</c>。最小实现见 <see cref="RelayCommand"/>。
/// Button.RaiseClick 在 IsEnabled 通过后若 CanExecute 为真则 Execute，并仍触发 Clicked。
/// </remarks>
public interface ICommand {
    /// <summary>判断命令当前是否可执行。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    /// <returns>true 可执行；false 禁用状态。</returns>
    bool CanExecute(object parameter);

    /// <summary>执行命令逻辑。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    void Execute(object parameter);

    /// <summary>
    /// 可执行态变更脉冲——订阅方（Button）据此同步 IsEnabled。
    /// 载荷无语义（常为 true）；重复 Set(true) 仍通知（Signal 无相等短路）。
    /// </summary>
    Signal<bool> CanExecuteChanged { get; }
}
