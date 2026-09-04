// RFC 037 M-CE1 · RFC 037 Internal: CodeEditor keyboard/IME focus registry (Draft).

namespace Arc.UI.Internal;

using Arc.UI.Components;

internal class EditorInputRouter {
    static CodeEditor _focused;

    private EditorInputRouter() {
    }

    internal static void RegisterEditor(CodeEditor editor) {
        _focused = editor;
    }

    internal static CodeEditor FocusedEditor {
        get { return _focused; }
    }

    internal static void UnregisterEditor(CodeEditor editor) {
        if (_focused == editor) {
            _focused = null;
        }
    }
}
