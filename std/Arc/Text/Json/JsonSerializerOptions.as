namespace Arc.Text.Json;

// JsonSerializer 的配置选项——Stable 最小面仅接线 WriteIndented。
// PropertyNaming / IgnoreNull / AllowTrailingCommas 等随 Deserialize 后置，不在此伪暴露。
public class JsonSerializerOptions
{
    // 是否缩进格式化输出（映射到 JsonWriterOptions.Indented）
    public bool WriteIndented;

    public static JsonSerializerOptions Default
    {
        get
        {
            JsonSerializerOptions opts = new JsonSerializerOptions();
            return opts;
        }
    }

    public JsonSerializerOptions()
    {
        WriteIndented = false;
    }
}
