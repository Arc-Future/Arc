// RFC 038: host-side capability whitelist for tool sandbox.
namespace Arc.Agent;

using Arc.Collections;

/// <summary>
/// Effective capability set captured for a session/host.
/// Language gap: namespace capability only gates native modules ([4.4] Phase 1).
/// Tool dispatch uses this host-side check — honest, not a fake Skip.
/// M1 reject lock: AIToolResult.ErrorKind = CapabilityDenied; no handler side effects.
/// </summary>
public class AICapabilitySet {
    private List<string> _caps;

    public AICapabilitySet() {
        _caps = new List<string>();
    }

    public int Count {
        get { return _caps.Count; }
    }

    public void Add(string capability) {
        if (capability == null || capability == "") {
            return;
        }
        if (this.Contains(capability)) {
            return;
        }
        _caps.Add(capability);
    }

    public bool Contains(string capability) {
        // fail-closed：空/未知能力一律拒绝（未显式授权不放行）。空串不匹配任何已登记
        // 能力，杜绝 Capability="" 的工具绕过沙箱门禁静默放行。
        if (capability == null || capability == "") {
            return false;
        }
        int i = 0;
        int n = _caps.Count;
        while (i < n) {
            if (_caps[i] == capability) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    public static AICapabilitySet From(string c0) {
        AICapabilitySet s = new AICapabilitySet();
        s.Add(c0);
        return s;
    }

    public static AICapabilitySet From(string c0, string c1) {
        AICapabilitySet s = new AICapabilitySet();
        s.Add(c0);
        s.Add(c1);
        return s;
    }
}
