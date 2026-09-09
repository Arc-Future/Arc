// RFC 037 section 4 M-CE1: IFrameDrawListProvider handle router (Core/Edit decouple).

namespace Arc.UI.Internal;

using Arc.Diagnostics;
using Arc.UI.Rendering;

/// <summary>Frame DrawList provider registry (internal; RenderTree / PlatformTreeSync).</summary>
internal class FrameDrawListRouter {
    private FrameDrawListRouter() {
    }

    const int SlotCapacity = 8;

    static long _handle0;
    static long _handle1;
    static long _handle2;
    static long _handle3;
    static long _handle4;
    static long _handle5;
    static long _handle6;
    static long _handle7;

    static IFrameDrawListProvider _provider0;
    static IFrameDrawListProvider _provider1;
    static IFrameDrawListProvider _provider2;
    static IFrameDrawListProvider _provider3;
    static IFrameDrawListProvider _provider4;
    static IFrameDrawListProvider _provider5;
    static IFrameDrawListProvider _provider6;
    static IFrameDrawListProvider _provider7;

    static int _slotCount;

    /// <summary>Register or update provider (same handle idempotent).</summary>
    internal static void Register(long platformHandle, IFrameDrawListProvider provider) {
        if (provider == null || platformHandle == 0) {
            return;
        }
        int existing = FindSlot(platformHandle);
        if (existing >= 0) {
            SetSlot(existing, platformHandle, provider);
            return;
        }
        if (_slotCount >= SlotCapacity) {
            Console.WriteLine("[FrameDrawListRouter] slot full (capacity="
                + SlotCapacity + "); CodeEditor paint registration dropped");
            return;
        }
        SetSlot(_slotCount, platformHandle, provider);
        _slotCount++;
    }

    /// <summary>Lookup by platform handle; null if missing.</summary>
    internal static IFrameDrawListProvider Lookup(long platformHandle) {
        if (platformHandle == 0) {
            return null;
        }
        int idx = FindSlot(platformHandle);
        if (idx < 0) {
            return null;
        }
        return GetProvider(idx);
    }

    static int FindSlot(long platformHandle) {
        if (_slotCount > 0 && _handle0 == platformHandle) { return 0; }
        if (_slotCount > 1 && _handle1 == platformHandle) { return 1; }
        if (_slotCount > 2 && _handle2 == platformHandle) { return 2; }
        if (_slotCount > 3 && _handle3 == platformHandle) { return 3; }
        if (_slotCount > 4 && _handle4 == platformHandle) { return 4; }
        if (_slotCount > 5 && _handle5 == platformHandle) { return 5; }
        if (_slotCount > 6 && _handle6 == platformHandle) { return 6; }
        if (_slotCount > 7 && _handle7 == platformHandle) { return 7; }
        return -1;
    }

    static void SetSlot(int idx, long handle, IFrameDrawListProvider provider) {
        if (idx == 0) { _handle0 = handle; _provider0 = provider; }
        else if (idx == 1) { _handle1 = handle; _provider1 = provider; }
        else if (idx == 2) { _handle2 = handle; _provider2 = provider; }
        else if (idx == 3) { _handle3 = handle; _provider3 = provider; }
        else if (idx == 4) { _handle4 = handle; _provider4 = provider; }
        else if (idx == 5) { _handle5 = handle; _provider5 = provider; }
        else if (idx == 6) { _handle6 = handle; _provider6 = provider; }
        else if (idx == 7) { _handle7 = handle; _provider7 = provider; }
    }

    static IFrameDrawListProvider GetProvider(int idx) {
        if (idx == 0) { return _provider0; }
        if (idx == 1) { return _provider1; }
        if (idx == 2) { return _provider2; }
        if (idx == 3) { return _provider3; }
        if (idx == 4) { return _provider4; }
        if (idx == 5) { return _provider5; }
        if (idx == 6) { return _provider6; }
        if (idx == 7) { return _provider7; }
        return null;
    }
}