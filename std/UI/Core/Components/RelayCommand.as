// RFC 037 M5: Arc.UI.Components — RelayCommand（ICommand 最小实现）。
//
// MVVM 命令最小面：持有 Execute / 可选 CanExecute 委托；无 CanExecuteChanged
// 推送通道（点击时再查 CanExecute；IsEnabled 自动同步后置）。
//
// 与 WPF RelayCommand 对比：
//   WPF: event CanExecuteChanged + CommandManager.RequerySuggested
//   Arc: 无 event；本切片仅同步查询 CanExecute（诚实最小面）

namespace Arc.UI.Components;

/// <summary>
/// <see cref="ICommand"/> 的委托实现——ViewModel 命令最小惯用法。
/// </summary>
public class RelayCommand : ICommand {
    private Action<object> _execute;
    private Func<object, bool> _canExecute;

    /// <summary>仅 Execute；CanExecute 恒 true。</summary>
    public RelayCommand(Action<object> execute) {
        _execute = execute;
        _canExecute = null;
    }

    /// <summary>Execute + CanExecute。</summary>
    public RelayCommand(Action<object> execute, Func<object, bool> canExecute) {
        _execute = execute;
        _canExecute = canExecute;
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
