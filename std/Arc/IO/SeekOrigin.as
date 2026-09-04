// RFC 027 M0: 标准库本地化 — 定位参考点 SeekOrigin。
//
// 对标 C# System.IO.SeekOrigin。

namespace Arc.IO {
/// <summary>
/// 流的定位参考点。
/// </summary>
public enum SeekOrigin {
    /// <summary>从流起始位置定位。</summary>
    Begin = 0,

    /// <summary>从当前读写位置定位。</summary>
    Current = 1,

    /// <summary>从流末尾位置定位。</summary>
    End = 2,
}
}
