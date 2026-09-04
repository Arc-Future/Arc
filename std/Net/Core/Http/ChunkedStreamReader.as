// RFC 025 M4: Arc.Net — HTTP 传输编码支持。
//
// ChunkedStreamReader 解析 HTTP/1.1 chunked transfer encoding 响应体。

namespace Arc.Net;

/// <summary>
/// Chunked Transfer-Encoding 解码器。
///
/// 按 RFC 7230 §4.1 解析 chunked 编码的响应体：
///   1. 读取 chunk-size (hex) 行
///   2. 读取 chunk-data
///   3. 重复直到 chunk-size = 0
///   4. 读取可选的 trailer 头
/// 纯 Arc 代码（非 facade），基于 NetworkStream 构建。
/// </summary>
public class ChunkedStreamReader {
    private StreamTransport _stream;

    /// <summary>终止 chunk 之后的 trailer 头集合（RFC 7230 §4.1.2；无 trailer 时为空）。</summary>
    public WebHeaderCollection Trailers;

    /// <summary>创建 ChunkedStreamReader 包装指定传输载体（明文 NetworkStream 或 TLS TlsNetworkStream）。</summary>
    public ChunkedStreamReader(StreamTransport stream) {
        _stream = stream;
        this.Trailers = new WebHeaderCollection();
    }

    /// <summary>读取并解码所有 chunked 数据块，拼接为完整字符串。</summary>
    /// <returns>解码后的完整响应体；失败返回空串。</returns>
    public string ReadAllChunks() {
        string allBody = "";

        while (true) {
            string chunkData = this.ReadChunk();
            if (chunkData == null) { break; }
            allBody = allBody + chunkData;
        }

        return allBody;
    }

    /// <summary>
    /// 读取并解码单个 chunk 数据块（SSE/流式增量消费；对齐 <see cref="ReadAllChunks"/> 的
    /// 单块解码逻辑）。返回解码后的 chunk 数据；读到终止 chunk（size=0）或流结束时返回 null，
    /// 此时 trailer 已读取进 <see cref="Trailers"/>。
    /// </summary>
    public string ReadChunk() {
        // 1. 读取 chunk-size 行（hex）
        string sizeLine = _stream.ReadLine();
        if (sizeLine == null || sizeLine == "") {
            return null;
        }

        // 解析十六进制 chunk-size
        int chunkSize = this.ParseHex(sizeLine);
        if (chunkSize == 0) {
            // 终止 chunk——读取可选的 trailer 头（RFC 7230 §4.1.2）
            while (true) {
                string trailer = _stream.ReadLine();
                if (trailer == null || trailer == "") { break; }
                int cp = trailer.IndexOf(":");
                if (cp > 0) {
                    string tname = trailer.Substring(0, cp).Trim();
                    string tvalue = trailer.Substring(cp + 1, trailer.Length - cp - 1).Trim();
                    this.Trailers.Add(tname, tvalue);
                }
            }
            return null;
        }

        // 2. 读取 chunk-data（需要确切的 chunkSize 字节）
        string chunkData = this.ReadExact(chunkSize);
        if (chunkData == null) { return null; }

        // 3. 读取 chunk-data 后的 CRLF
        _stream.ReadLine();

        return chunkData;
    }

    /// <summary>读取指定字节数的数据。</summary>
    private string ReadExact(int bytesNeeded) {
        string result = "";
        while (result.Length < bytesNeeded) {
            int remaining = bytesNeeded - result.Length;
            string chunk = _stream.ReadString(remaining);
            if (chunk == null || chunk == "") { return null; }
            result = result + chunk;
        }
        return result;
    }

    /// <summary>
    /// 异步读取并解码单个 chunk 数据块（SSE/流式增量消费 · 真异步 · Reactor 不阻塞）。
    /// 对齐同步 <see cref="ReadChunk"/> 的单块解码语义。返回解码后的 chunk 数据；
    /// 读到终止 chunk（size=0）或流结束时返回 null，此时 trailer 已读取进
    /// <see cref="Trailers"/>。
    /// </summary>
    public async Task<string> ReadChunkAsync() {
        // 1. 读取 chunk-size 行（hex）
        string sizeLine = await _stream.ReadLineAsync();
        if (sizeLine == null || sizeLine == "") {
            return null;
        }
        int chunkSize = this.ParseHex(sizeLine);
        if (chunkSize == 0) {
            // 终止 chunk——读取可选的 trailer 头（RFC 7230 §4.1.2）
            while (true) {
                string trailer = await _stream.ReadLineAsync();
                if (trailer == null || trailer == "") { break; }
                int cp = trailer.IndexOf(":");
                if (cp > 0) {
                    string tname = trailer.Substring(0, cp).Trim();
                    string tvalue = trailer.Substring(cp + 1, trailer.Length - cp - 1).Trim();
                    this.Trailers.Add(tname, tvalue);
                }
            }
            return null;
        }
        // 2. 读取 chunk-data（需要确切的 chunkSize 字节）
        string chunkData = await this.ReadExactAsync(chunkSize);
        if (chunkData == null) { return null; }
        // 3. 读取 chunk-data 后的 CRLF
        await _stream.ReadLineAsync();
        return chunkData;
    }

    /// <summary>异步读取指定字节数的数据（真异步）。</summary>
    private async Task<string> ReadExactAsync(int bytesNeeded) {
        string result = "";
        while (result.Length < bytesNeeded) {
            int remaining = bytesNeeded - result.Length;
            string chunk = await _stream.ReadStringAsync(remaining);
            if (chunk == null || chunk == "") { return null; }
            result = result + chunk;
        }
        return result;
    }

    /// <summary>解析十六进制字符串为整数。</summary>
    private int ParseHex(string s) {
        // 去掉可选的 chunk-extension（';' 之后部分）
        int semiPos = s.IndexOf(";");
        if (semiPos > 0) {
            s = s.Substring(0, semiPos);
        }
        return Convert.ToInt32(s, 16);
    }
}
