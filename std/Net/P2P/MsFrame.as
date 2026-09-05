// MsFrame —— 拆分自 MultistreamSelect.as（一文件一公开类型）。
namespace Arc.Net.P2P;
using Arc;
using Arc.Net;
using Arc.Collections;
using Arc.Text;

/// <summary>multistream-select/1.0.0 帧（已解码载荷 + 原始字节）。</summary>
public class MsFrame {
    public string Payload;   // 协议标识（含尾部 \n 前的原文，可含 \n）
    public byte[] Raw;       // 原始帧字节（长度前缀 + 载荷）

    public MsFrame(string payload, byte[] raw) {
        Payload = payload;
        Raw = raw;
    }
}
