// RFC 037 section 4 M-CE1: frame DrawList contribution surface.
// Core must not depend on Arc.UI.Edit; interface decouples packages.

namespace Arc.UI.Rendering;

/// <summary>
/// Controls that contribute a per-frame DrawList (CodeEditor viewport virtualization).
/// </summary>
public interface IFrameDrawListProvider {
    /// <summary>Register platform mirror handle for RenderTree lookup.</summary>
    void BindDrawListMirror(long platformHandle);

    /// <summary>Bound platform handle (0 = unbound).</summary>
    long DrawListMirrorHandle { get; }

    /// <summary>
    /// Materialize visible content as DrawList in local coordinates (origin = control top-left).
    /// Renderer applies layout origin then ExecuteDrawList.
    /// </summary>
    DrawList BuildFrameDrawList();
}