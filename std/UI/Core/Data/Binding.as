// RFC 037 D4.2: Arc.UI — Binding 绑定描述。
//
// Binding 描述 {x:Bind Path, Mode=...} 中的语义信息：
//   - Path: 源属性路径
//   - Mode: OneWay/TwoWay/OneTime/OneWayToSource
//   - Converter: 值转换器
//   - FallbackValue: 绑定失败时的回退值
//
// **命名空间归属**：本文件位于 std/UI/Data/ 子目录，但归属到 `Arc.UI`
// 命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」+ RFC 037 D9.2
// Data 扁平化原则）。Data 基础类型（Signal/Binding/DataContext）作为
// Arc.UI 子库的对外门面类型，扁平化便于高频引用。

namespace Arc.UI;

/// <summary>绑定描述。</summary>
public struct Binding {
    /// <summary>源属性路径（如 "User.Name"）。</summary>
    public string Path;

    /// <summary>绑定模式："OneWay"/"TwoWay"/"OneTime"/"OneWayToSource"。</summary>
    public string Mode;

    /// <summary>值转换器名（来自 {StaticResource}）。</summary>
    public string Converter;

    /// <summary>转换器参数。</summary>
    public string ConverterParameter;

    /// <summary>绑定失败时的回退值。</summary>
    public string FallbackValue;

    /// <summary>绑定源为 null 时的目标值。</summary>
    public string TargetNullValue;

    public Binding() {
        this.Mode = "OneWay";
    }

    public Binding(string path) {
        this.Path = path;
        this.Mode = "OneWay";
    }
}
