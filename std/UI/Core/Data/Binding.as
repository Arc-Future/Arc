// RFC 037 D4.2: Arc.UI — Binding 绑定描述。
//
// Binding 描述 {Binding Path, Mode=...} 的编译期脱糖语义：
//   - Path: 源属性（code-behind `this.Path` / `this.Foo.Bar`；错名编译失败）
//   - Mode: OneTime/OneWay/TwoWay
// Converter / ElementName / RelativeSource / UpdateSourceTrigger /
// OneWayToSource / 运行时路径行走：作者面硬拒绝，字段仅描述骨架。
//
// **命名空间归属**：本文件位于 std/UI/Data/ 子目录，但归属到 `Arc.UI`
// 命名空间（按 RFC 020 §3.2「子命名空间与目录解耦」+ RFC 037 D9.2
// Data 扁平化原则）。Data 基础类型（Signal/Binding/DataContext）作为
// Arc.UI 子库的对外门面类型，扁平化便于高频引用。

namespace Arc.UI;

/// <summary>绑定描述。</summary>
public struct Binding {
    /// <summary>源属性路径（如 "Greeting" 或 "Model.Name"；错名编译失败）。</summary>
    public string Path;

    /// <summary>绑定模式：作者面仅 "OneTime"/"OneWay"/"TwoWay"。</summary>
    public string Mode;

    /// <summary>值转换器名。作者面未开（typeck/codegen 硬拒绝）。</summary>
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
