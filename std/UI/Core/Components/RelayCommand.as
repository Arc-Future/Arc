// RFC 037 M5: Arc.UI.Components — RelayCommand（ICommand 最小实现）。
//
// MVVM 命令最小面：持有 Execute / 可选 CanExecute 委托 + CanExecuteChanged 脉冲。
// ViewModel 在可执行条件变化时调用 RaiseCanExecuteChanged → Button 同步 IsEnabled。
//
// 与 WPF RelayCommand 对比：
//   WPF: event CanExecuteChanged + CommandManager.RequerySuggested
//   Arc: Signal 脉冲 + 显式 Raise（无全局 RequerySuggested）

namespace Arc.UI.Components;

using Arc;

/// <summary>
/// <see cref="ICommand"/> 的委托实现——ViewModel 命令最小惯用法。
/// </summary>
public class RelayCommand : ICommand {
    private Action<object> _execute;
    private Func<object, bool> _canExecute;
    private Signal<bool> _canExecuteChanged;

    /// <summary>仅 Execute；CanExecute 恒 true。</summary>
    public RelayCommand(Action<object> execute) {
        _execute = execute;
        _canExecute = null;
        _canExecuteChanged = new Signal<bool>(false);
    }

    /// <summary>Execute + CanExecute。</summary>
    public RelayCommand(Action<object> execute, Func<object, bool> canExecute) {
        _execute = execute;
        _canExecute = canExecute;
        _canExecuteChanged = new Signal<bool>(false);
    }

    /// <inheritdoc />
    public Signal<bool> CanExecuteChanged {
        get { return _canExecuteChanged; }
    }

    /// <summary>
    /// 通知可执行态已变——Button 等订阅方据此重查 CanExecute 并同步 IsEnabled。
    /// </summary>
    public void RaiseCanExecuteChanged() {
        if (_canExecuteChanged != null) {
            _canExecuteChanged.Set(true);
        }
    }

    /// <inheritdoc />
    public bool CanExecute(object parameter) {
        if (_canExecute == null) {
            return true;
        }
        return _canExecute(parameter);
    }

    /// <inheritdoc />
    public void Execute(object parameter) {
        if (_execute == null) {
            return;
        }
        if (!this.CanExecute(parameter)) {
            return;
        }
        _execute(parameter);
    }
}
