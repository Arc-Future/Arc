namespace Arc.Agent;

// RFC 038: AIToolArgsReader 条目种类（内部存储；非契约公开面）。
internal enum AIToolArgKind {
    String,
    Number,
    Bool,
    StringArray,
    /// <summary>嵌套对象（RawJson 承载原始文本；子字段以点路径扁平进入 entries）。</summary>
    Object,
    /// <summary>非纯字符串数组（RawJson 承载原始文本；GetObjectJson 取用）。</summary>
    JsonArray,
    /// <summary>JSON null（Text 恒为空串）。</summary>
    Null,
}
