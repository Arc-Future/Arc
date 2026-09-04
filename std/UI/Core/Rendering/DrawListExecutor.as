// RFC 037 §13: DrawListExecutor — DrawList → IRender 批绘入口。

namespace Arc.UI.Rendering;

/// <summary>
/// DrawList 执行器：将录制完成的 DrawList 提交至渲染后端。
/// M-draw1：委托 IRender.ExecuteDrawList；帧边界仍由 BeginFrame/EndFrame 管理。
/// </summary>
public static class DrawListExecutor {
    /// <summary>
    /// 执行 DrawList。
    /// </summary>
    /// <returns>0 成功；-1 后端未实现；-2 含不支持命令。</returns>
    public static int Execute(IRender backend, DrawList list) {
        if (backend == null || list == null) {
            return -1;
        }
        return backend.ExecuteDrawList(list);
    }
}
