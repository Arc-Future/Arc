// IToneRequirements —— 音依赖声明（RFC 045 D12）。
//
// 可选契约：实现之则内核按声明准入——任一依赖未就绪时音保持 Pending
// （不执行 Apply），依赖经 Provide 出现后自动启动；Start 级联同样按
// 声明校验，启动序由依赖图推导而非手动编排。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


/// <summary>
/// 可选依赖声明：对象形态音实现之，内核按声明准入；
/// 未实现视为无声明（零破坏，既有 ITone 实现不受影响）。
/// </summary>
public interface IToneRequirements {
    /// <summary>依赖的服务名列表；全部就绪（祖先链可达）方可执行 Apply。</summary>
    List<string> Requires { get; }
}
