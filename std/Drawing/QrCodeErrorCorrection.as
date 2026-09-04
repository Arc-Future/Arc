namespace Arc.Drawing;

/// <summary>
/// QR 纠错等级（RFC 029 M2 · 对齐 qrcodegen 枚举）。
/// 判别值 0-3 直射 qrcodegen_Ecc LOW..HIGH（boostEcl=false 保持严格请求等级）。
/// </summary>
public enum QrCodeErrorCorrection {
    L,
    M,
    Q,
    H,
}
