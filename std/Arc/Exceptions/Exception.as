// Exception — 基础异常类（RFC 027 M0）
// 对标 C# System.Exception。
// 所有异常类型的基类，携带诊断信息与异常链。
namespace Arc;

/// <summary>
/// 表示应用程序执行期间发生的错误。
///
/// 属性：
///   - Message：人类可读的错误描述
///   - InnerException：内部异常（异常链），可为 null
///   - HResult：HRESULT 错误码（COM / native 互操作兼容），默认 0x80131500 (COR_E_EXCEPTION)
///   - Source：引发异常的应用或组件名称
///   - StackTrace：throw 时由 runtime 捕获（`rt_format_stacktrace`）；构造后为 null。
///     符号完备：嵌入 `__arc_dbg_table` 默认发射（与 DWARF `-g` 解耦）→ 函数名 +
///     可行时 file:line（Windows MSVC/MinGW 与 POSIX 同路径）；无符号帧诚实 `at <0x…>`。
///     禁止空串/假符号冒充完备；不宣称 PDB 级完美还原。
///   - ToString：Message（+ 内层链）；StackTrace 非 null 时换行追加。
///
/// 派生类使用 `base(...)` 构造函数链传递 Message 和 InnerException。
/// </summary>
public class Exception {
    /// <summary>描述错误的消息。</summary>
    public string Message { get; }

    /// <summary>内部异常（异常链）。</summary>
    public Exception? InnerException { get; }

    /// <summary>HRESULT 错误码，用于 COM / native 互操作兼容。</summary>
    public int HResult { get; set; }

    /// <summary>异常错误码（整数）。对标 C# Exception.HResult 的简化访问。</summary>
    public int Code { get { return this.HResult; } set { this.HResult = value; } }

    /// <summary>引发异常的应用或组件名称。</summary>
    public string? Source { get; set; }

    /// <summary>调用栈字符串（构造后 null；首次 throw 时由 runtime 填充）。</summary>
    public string? StackTrace { get; }

    /// <summary>构造异常。</summary>
    public Exception() {
        this.Message = "";
        this.InnerException = null;
        this.HResult = (int)0x80131500;
        this.Source = null;
        this.StackTrace = null;
    }

    /// <summary>使用指定消息构造异常。</summary>
    public Exception(string message) {
        this.Message = message;
        this.InnerException = null;
        this.HResult = (int)0x80131500;
        this.Source = null;
        this.StackTrace = null;
    }

    /// <summary>使用指定消息和内部异常构造异常。</summary>
    public Exception(string message, Exception? innerException) {
        this.Message = message;
        this.InnerException = innerException;
        this.HResult = (int)0x80131500;
        this.Source = null;
        this.StackTrace = null;
    }

    /// <summary>
    /// 返回异常的诊断字符串：Message → 内部异常链；若 StackTrace 非 null 则换行追加。
    /// 构造后未 throw 时仅 Message（+ 内层）；首次 throw 后与 C# 同形附栈。
    /// </summary>
    public override string ToString() {
        var result = this.Message;
        if (this.InnerException != null) {
            result = result + " ---> " + this.InnerException.ToString();
        }
        if (this.StackTrace != null) {
            result = result + "\n" + this.StackTrace;
        }
        return result;
    }
}
