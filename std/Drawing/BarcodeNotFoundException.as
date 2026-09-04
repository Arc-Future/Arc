namespace Arc.Drawing;

using Arc;

/// <summary>
/// 条形码/二维码解码未命中（RFC 029 M4）。显式失败面——BarcodeReader.Read
/// 在图中未检出可解码条码时抛出，禁静默返回 0/null。
/// </summary>
public class BarcodeNotFoundException : SystemException {
    public BarcodeNotFoundException() : base() { }
    public BarcodeNotFoundException(string message) : base(message) { }
    public BarcodeNotFoundException(string message, Exception? innerException) : base(message, innerException) { }
}
