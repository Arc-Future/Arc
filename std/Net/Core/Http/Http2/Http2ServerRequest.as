// Http2ServerRequest —— 拆分自 Http2ServerConnection.as（一文件一公开类型）。
namespace Arc.Net;
using Arc.Collections;
using Arc.Text;

/// <summary>服务端单请求（请求头 + 载荷累积至 END_STREAM）。</summary>
public class Http2ServerRequest {
    public int StreamId;
    public string Method;
    public string Path;
    public Http2HeaderList Headers;
    public byte[] Body;
    public bool EndStream;

    public Http2ServerRequest(int streamId) {
        StreamId = streamId;
        Method = "";
        Path = "";
        Headers = new Http2HeaderList();
        Body = Http2ByteUtils.ZeroBytes(0);
        EndStream = false;
    }
}
