// RFC 038：响应格式契约（MAF contract-first）。
// 业务端只声明「要什么样的输出结构」（JsonObject / From<T>），
// 由各 Provider 按自身 API 协议内部映射（DeepSeek：json_object / json_schema）。
//
// From<T>() 采用契约式结构声明（RFC 032 static abstract 静态分派，零反射零虚分派）：
// T 实现 IAIJsonSchema 并声明自身 JSON Schema——结构归属类型本身，确定性强、跨供应商可移植，
// 规避运行时反射（FieldType 元数据对用户类型尚不可靠）的脆弱性。
namespace Arc.Agent;

/// <summary>响应格式类别（供应商无关的契约枚举）。</summary>
public enum AIResponseFormatKind {
    /// <summary>默认自由文本。</summary>
    Text,
    /// <summary>结构化 JSON 对象（供应商映射 json_object / response_format.type=json_object）。</summary>
    JsonObject,
    /// <summary>严格 JSON Schema 强约束（供应商映射 json_schema）。</summary>
    JsonSchema,
}

/// <summary>
/// 响应格式契约（MAF contract-first）：宿主声明目标结构，供应商按协议内部映射。
/// 仅承载结构意图（Kind + SchemaJson），不含任何供应商私有字段——映射在 Provider 层。
/// </summary>
public class AIResponseFormat {
    /// <summary>格式类别。</summary>
    public AIResponseFormatKind Kind;

    /// <summary>JSON Schema 文本（仅 JsonSchema 类别携带；其余为空串）。</summary>
    public string SchemaJson;

    private AIResponseFormat(AIResponseFormatKind kind) {
        this.Kind = kind;
        this.SchemaJson = "";
    }

    /// <summary>默认自由文本格式。</summary>
    public static AIResponseFormat Text() {
        return new AIResponseFormat(AIResponseFormatKind.Text);
    }

    /// <summary>结构化 JSON 对象（供应商映射 json_object）。</summary>
    public static AIResponseFormat JsonObject() {
        return new AIResponseFormat(AIResponseFormatKind.JsonObject);
    }

    /// <summary>
    /// 由类型契约获得结构（From&lt;T&gt;）：T 实现 <see cref="IAIJsonSchema"/> 并以实例方法把
    /// 自身 JSON Schema 写入 writer（对齐 JsonSerializer.Deserialize&lt;T&gt; 的 void 分派路径，
    /// 确定性、零反射；规避泛型接口值返回分派缺口）。返回 JsonSchema 类别格式。
    /// </summary>
    public static AIResponseFormat From<T>() where T : IAIJsonSchema, new() {
        AIResponseFormat f = new AIResponseFormat(AIResponseFormatKind.JsonSchema);
        T t = new T();
        AIJsonSchemaWriter w = new AIJsonSchemaWriter();
        t.WriteSchema(w);
        f.SchemaJson = w.Schema;
        return f;
    }
}

/// <summary>JSON Schema 写入载体（具体类：字段/方法访问不经泛型接口分派，可靠）。</summary>
public class AIJsonSchemaWriter {
    /// <summary>已写入的 JSON Schema 文本。</summary>
    public string Schema;

    public AIJsonSchemaWriter() {
        this.Schema = "";
    }

    /// <summary>写入类型自身的 JSON Schema 文本。</summary>
    public void Set(string schema) {
        this.Schema = schema != null ? schema : "";
    }
}

/// <summary>
/// JSON Schema 契约：实现方以实例方法声明自身结构化输出的 JSON Schema 文本。
/// 经 <see cref="AIResponseFormat.From&lt;T&gt;"/> 构造读取，零反射、跨供应商可移植。
/// </summary>
public interface IAIJsonSchema {
    /// <summary>把类型自身的 JSON Schema 文本写入 writer（{"type":"object","properties":{...},...}）。</summary>
    void WriteSchema(AIJsonSchemaWriter writer);
}
