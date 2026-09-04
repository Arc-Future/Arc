// RFC 037 M5: Arc.UI.Components — ICommand 命令接口。
//
// 设计决策：Arc 无 event/delegate 体系，ICommand.CanExecuteChanged 改用
// Signal<bool> 替代——订阅者通过 Observe/Signal API 监听，无需引入事件系统。
//
// 与 WPF ICommand 对比：
//   WPF: event EventHandler CanExecuteChanged
//   Arc: Signal<bool> CanExecuteChanged  (Signal-based)
//
// 使用模式：
//   - MVVM: ViewModel 实现 ICommand，绑定到 Button.Command
//   - 简单场景：直接用 Button.Clicked 替代命令模式

namespace Arc.UI.Components;

/// <summary>
/// 命令接口——封装可执行的用户操作，支持启用/禁用状态。
/// Arc 版本用 Signal<bool> 替代 CanExecuteChanged 事件。
/// </summary>
public interface ICommand {
    /// <summary>判断命令当前是否可执行。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    /// <returns>true 可执行；false 禁用状态。</returns>
    bool CanExecute(object parameter);

    /// <summary>执行命令逻辑。</summary>
    /// <param name="parameter">命令参数（可选，传 null）。</param>
    void Execute(object parameter);
}
