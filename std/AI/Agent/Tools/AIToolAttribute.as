// RFC 038: [AITool] attribute — compile-time tool metadata anchor (no reflection Invoke).
//
// 编译器按属性名扫描 [AITool] 方法，经领域绑定钩子（合成 __AIToolHost）直接发射
// 类型化绑定（显式静态注册，普通 build / test / publish 均生效）；属性仅作编译期
// 元数据锚点，运行期零反射 Invoke。
namespace Arc.Agent;

using Arc;

/// <summary>
/// Marks a tool method. M1 registers handlers via AIToolSet.Add (explicit);
/// attribute carries Name/Capability/RequireApproval for schema &amp; docs.
/// </summary>
[AttributeUsage(AttributeTargets.Method)]
public class AIToolAttribute : Attribute {
    public string Name;
    public string Capability;
    public bool RequireApproval;

    /// <summary>工具描述（供 schema/文档；缺省空串）。</summary>
    public string Description;

    public AIToolAttribute() {
        this.Name = "";
        this.Capability = "ai.Tool";
        this.RequireApproval = false;
        this.Description = "";
    }

    public AIToolAttribute(string name) {
        this.Name = name != null ? name : "";
        this.Capability = "ai.Tool";
        this.RequireApproval = false;
        this.Description = "";
    }
}
