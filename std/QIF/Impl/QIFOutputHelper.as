namespace Arc.QIF;

using Arc.Text;

/// <summary>
/// 测试输出捕获器——实现 <see cref="IQIFOutput"/>，将 WriteLine 内容写入
/// <see cref="StringBuilder"/>，测试结束后由 Runner 读取附加到 QIFResult.Output。
/// 对标 XUnit TestOutputHelper。
/// </summary>
public class QIFOutputHelper : IQIFOutput {
    private StringBuilder _buffer;

    public QIFOutputHelper() {
        _buffer = new StringBuilder();
    }

    public void WriteLine(string message) {
        if (_buffer.Length > 0) {
            _buffer.Append("\n");
        }
        _buffer.Append(message);
    }

    public string Output {
        get { return _buffer.ToString(); }
    }

    public void Clear() {
        _buffer = new StringBuilder();
    }
}
