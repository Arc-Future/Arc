namespace Arc.Text.Json;

// JsonWriter 的配置选项——Stable 最小面仅接线已实现字段。
public class JsonWriterOptions
{
    // 是否启用缩进格式化输出（pretty print）
    public bool Indented;

    // 是否转义正斜杠 / 为 \/（RFC 8259 可选；默认 false）
    public bool EscapeForwardSlash;

    public JsonWriterOptions()
    {
        Indented = false;
        EscapeForwardSlash = false;
    }
}
