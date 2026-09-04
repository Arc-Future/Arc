// RFC 037 §5.2/§11.5 统一刷新终点（M-U2）：AdaptiveHost —— 窗口级宿主。
//
// 职责：求值器 + Token 绑定 + DP/Signal 闭环。
//
//   环境/容器尺寸变化 → 求值器重算受影响子树 DP → SetValue<T>(prop, newValue)
//     → Signal<T>.Set 触发通知 → 渲染层 Observe<T> 收到 → 局部重绘（§5.2）
//
// 每窗口一个宿主实例（§11.6 多窗口求值器窗口级实例化）。窗口级快照变化走
// `ApplySnapshot`（一次投影索引计算 + 全绑定 SetValue）；容器级 finalSize
// 变化走 `ApplyContainerSize`（仅受影响容器 id 的绑定重算）。

namespace Arc.UI.Adaptive;

using Arc.Collections;

/// <summary>
/// 窗口级自适应宿主（§5.2/§11.5 统一刷新终点：求值器重算 → SetValue → Observe）。
/// </summary>
public class AdaptiveHost {
    private AdaptiveEvaluator _eval;
    private List<string> _names;
    private List<int> _tokenIds;
    private List<int> _containerIds;
    private List<AdaptiveProperty> _props;

    /// <summary>构造宿主。</summary>
    /// <param name="spec">窗口级投影规格（codegen 生成）。</param>
    public AdaptiveHost(AdaptiveSpec spec) {
        _eval = new AdaptiveEvaluator(spec);
        _names = new List<string>();
        _tokenIds = new List<int>();
        _containerIds = new List<int>();
        _props = new List<AdaptiveProperty>();
    }

    /// <summary>底层求值器（容器查询/规格访问）。</summary>
    public AdaptiveEvaluator Evaluator { get { return _eval; } }

    /// <summary>
    /// 注册 Token 绑定（求值器重算 → 该绑定的信号后端 SetValue → Observe 局部重绘）。
    /// </summary>
    /// <param name="name">属性名（诊断/追踪用）。</param>
    /// <param name="tokenId">Token 在规格 Tokens 中的索引。</param>
    /// <param name="containerId">容器上下文（0 = 窗口根；adaptiveId+1）。</param>
    /// <returns>绑定索引（供 Observe 消费）。</returns>
    public int BindToken(string name, int tokenId, int containerId) {
        AdaptiveProperty p = new AdaptiveProperty(0.0);
        _names.Add(name);
        _tokenIds.Add(tokenId);
        _containerIds.Add(containerId);
        _props.Add(p);
        return _props.Count - 1;
    }

    /// <summary>已注册绑定数。</summary>
    public int Count { get { return _props.Count; } }

    /// <summary>读取绑定索引对应的信号后端 DP 槽。</summary>
    public AdaptiveProperty Property(int index) {
        return _props[index];
    }

    /// <summary>
    /// 窗口级快照变化：一次投影索引计算 → 全绑定 SetValue（§11.5 窗口级热路径）。
    /// </summary>
    /// <param name="s">新快照。</param>
    public void ApplySnapshot(AdaptiveSnapshot s) {
        _eval.Recompute(s);
        for (int i = 0; i < _props.Count; i++) {
            double v = _eval.ResolveToken(_tokenIds[i], _containerIds[i]);
            _props[i].Set(v);
        }
    }

    /// <summary>
    /// 容器级 finalSize 变化：仅受影响容器 id 的绑定重算（§11.5 容器级热路径）。
    /// </summary>
    /// <param name="containerId">容器上下文（0 = 窗口根；adaptiveId+1）。</param>
    /// <param name="finalSize">容器 finalSize（vp）。</param>
    public void ApplyContainerSize(int containerId, double finalSize) {
        _eval.SetContainerSize(containerId, finalSize);
        for (int i = 0; i < _props.Count; i++) {
            if (_containerIds[i] == containerId) {
                double v = _eval.ResolveToken(_tokenIds[i], containerId);
                _props[i].Set(v);
            }
        }
    }
}
