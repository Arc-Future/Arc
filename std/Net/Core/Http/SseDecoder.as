// RFC 025 / RFC 033：通用异步 SSE 解码器 —— HTTP 层协议解析（对标 .NET System.Net.Http.SseParser 分层）。
//
// 原生异步迭代器（RFC 044 单一惯用法）：行式状态机由编译器合成，源码只有线性
// 控制流——消费者每次拉取驱动一次网络读与行解析，天然背压、不阻塞线程、不整段
// 缓冲。领域层（AI Provider 等）消费本序列做 SSE 字段 → 领域事件映射，
// 禁止各自重复实现行式解析。
namespace Arc.Net;

using Arc;
using Arc.Collections;

/// <summary>
/// SSE 流解码器：把 <see cref="StreamTransport"/> 上的 text/event-stream 字节流解码为
/// <see cref="SseEvent"/> 异步序列。每个事件块以空行分隔；仅携带 data 的事件块派发。
/// 取消经 <see cref="Decode"/> 参数令牌传递（生产者侧单通道），取消后序列即刻终结。
/// </summary>
public static class SseDecoder {

    /// <summary>把 SSE 字节流解码为异步事件序列（冷流：拉取即驱动网络读与行解析）。</summary>
    /// <param name="stream">SSE 字节流载体（明文/TLS StreamTransport）。</param>
    /// <param name="chunked">响应体是否 chunked 编码（true 则经 ChunkedStreamReader 先解帧）。</param>
    /// <param name="cancellationToken">取消令牌（取消后序列即刻终结，不再发起 IO）。</param>
    public static async IAsyncEnumerable<SseEvent> Decode(StreamTransport stream, bool chunked, CancellationToken cancellationToken) {
        ChunkedStreamReader chunkReader = chunked ? new ChunkedStreamReader(stream) : null;
        string lineBuf = "";
        bool eof = false;
        // 当前事件块累积面（WHATWG：空行前累积，空行派发并复位）
        string name = "";
        string data = "";
        string id = "";
        int retry = -1;
        bool hasData = false;
        while (true) {
            if (cancellationToken.IsCancellationRequested) {
                yield break;
            }
            string line = null;
            int nl = lineBuf.IndexOf("\n", 0);
            if (nl >= 0) {
                line = lineBuf.Substring(0, nl);
                if (line.Length > 0 && line.Substring(line.Length - 1, 1) == "\r") {
                    line = line.Substring(0, line.Length - 1);
                }
                lineBuf = lineBuf.Substring(nl + 1, lineBuf.Length - nl - 1);
            } else if (eof) {
                if (lineBuf != "") {
                    // EOF 前最后一段未换行文本：作为末行参与解析
                    line = lineBuf;
                    if (line.Length > 0 && line.Substring(line.Length - 1, 1) == "\r") {
                        line = line.Substring(0, line.Length - 1);
                    }
                    lineBuf = "";
                } else if (hasData) {
                    // 伪空行：EOF 冲刷最后一个未终结事件块
                    line = "";
                } else {
                    yield break;
                }
            } else {
                string more = null;
                if (chunkReader != null) {
                    string chunk = await chunkReader.ReadChunkAsync();
                    more = chunk != null ? chunk : "";
                } else {
                    string s = await stream.ReadStringAsync(4096);
                    more = s == null || s == "" ? "" : s;
                }
                if (more == "") {
                    eof = true;
                } else {
                    lineBuf = lineBuf + more;
                }
                continue;
            }

            if (line == "") {
                // 空行 = 事件块边界；无 data 的块（注释/心跳）不派发
                if (hasData) {
                    SseEvent current = new SseEvent(name, data, id, retry);
                    name = "";
                    data = "";
                    id = "";
                    retry = -1;
                    hasData = false;
                    yield return current;
                }
                continue;
            }

            // 行解析（WHATWG）：冒号前为字段名，值剥离单个前导空格；注释行（: 开头）忽略
            if (line.Substring(0, 1) == ":") {
                continue;
            }
            string field = line;
            string value = "";
            int colon = line.IndexOf(":", 0);
            if (colon >= 0) {
                field = line.Substring(0, colon);
                value = line.Substring(colon + 1, line.Length - colon - 1);
                if (value.Length > 0 && value.Substring(0, 1) == " ") {
                    value = value.Substring(1, value.Length - 1);
                }
            }
            if (field == "event") {
                name = value;
            } else if (field == "data") {
                data = data == "" ? value : data + "\n" + value;
                hasData = true;
            } else if (field == "id") {
                id = value;
            } else if (field == "retry") {
                retry = SseDecoder.ParseInt(value);
            }
        }
    }

    /// <summary>
    /// 非抛错单步读取：推进 SSE 枚举器并把网络/解析异常收敛为 <see cref="SseReadStep.Error"/>
    /// 消息值——迭代器方法体内不能 try/catch（RFC 044 M1），消费侧异常→值边界收敛于此。
    /// </summary>
    /// <param name="enumerator">被驱动的 SSE 枚举器。</param>
    /// <returns>单步结果（推进成功携带当前事件；失败携带异常消息）。</returns>
    public static async Task<SseReadStep> ReadAsync(IAsyncEnumerator<SseEvent> enumerator) {
        SseReadStep step = new SseReadStep();
        try {
            step.Moved = await enumerator.MoveNextAsync();
            if (step.Moved) {
                step.Event = enumerator.Current;
            }
            return step;
        } catch (Exception ex) {
            step.Error = ex != null && ex.Message != null ? ex.Message : "stream error";
            return step;
        }
    }

    /// <summary>十进制整数解析（retry 字段）；非法值返回 -1（视为未携带）。</summary>
    private static int ParseInt(string s) {
        if (s == null || s == "") {
            return -1;
        }
        try {
            return Convert.ToInt32(s, 10);
        } catch {
            return -1;
        }
    }
}

/// <summary>SSE 单步读取结果（非抛错边界载体）：<see cref="Error"/> 非 null 即失败。</summary>
public class SseReadStep {
    /// <summary>是否推进成功（false = 序列终结）。</summary>
    public bool Moved { get; set; }

    /// <summary>推进成功时的当前事件（未推进为 null）。</summary>
    public SseEvent Event { get; set; }

    /// <summary>读取异常消息（null = 无异常）。</summary>
    public string Error { get; set; }
}
