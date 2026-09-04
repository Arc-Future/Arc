// RFC 037 §8 修订（text-editing.md §3）：TextBoxModel 编辑内核——唯一编辑真相。
//
// 纯逻辑、零渲染/平台依赖（internal）；可无窗口 headless 全量测试。
// 消除 D6（编辑语义 4 层散布）与 D8（选区 4 字段 3 冗余）：
//   - 全部编辑语义（插入/删除/移动/选区/撤销/组字）收敛于此；
//   - 状态唯一真值集 = text + caret + anchor（start/length 为派生只读）；
//   - MaxLength/IsReadOnly 策略在内核统一裁决（不再散落各编辑入口）。
//
// 不变量（§3.4）：任何操作出口保证 0 ≤ caret, anchor ≤ text.Length；
// 状态突变必经操作方法；镜像同步单向（Model → TextBox DP → mirror → 渲染）。
//
// MoveCaret 签名说明：granularity（Char/Word/Home/End）与 direction（向后/
// 向前）正交——Win32/macOS 编辑惯例；Home/End 单行等价于行首/行尾，direction
// 不参与取值。多行 Line 粒度为非目标（text-editing.md §6）。

namespace Arc.UI.Editing;

using Arc.Collections;

/// <summary>光标移动方向（Word/Char 粒度配对使用）。</summary>
public enum MoveDirection {
    /// <summary>向文本头部方向。</summary>
    Backward,
    /// <summary>向文本尾部方向。</summary>
    Forward,
}

/// <summary>光标移动粒度。</summary>
public enum MoveGranularity {
    /// <summary>单字符。</summary>
    Char,
    /// <summary>词（以空白为界；CJK 连续段视为一词）。</summary>
    Word,
    /// <summary>文本头（多行为行首）。</summary>
    Home,
    /// <summary>文本尾（多行为行尾）。</summary>
    End,
}

/// <summary>撤销/重做快照（text + caret + anchor 三元组）。</summary>
internal class TextBoxSnapshot {
    public string Text;
    public int Caret;
    public int Anchor;

    public TextBoxSnapshot(string text, int caret, int anchor) {
        this.Text = text;
        this.Caret = caret;
        this.Anchor = anchor;
    }
}

/// <summary>
/// 单行文本编辑内核：文本/选区/撤销/组字的唯一编辑真相（RFC 037 §8）。
/// </summary>
internal class TextBoxModel {
    /// <summary>撤销/重做栈容量上限（快照式，溢出丢最旧）。</summary>
    public const int StackCapacity = 100;

    string _text = "";
    int _caret;
    int _anchor;
    string _composition = "";
    int _version;
    bool _mergeableInsertOpen;
    List<TextBoxSnapshot> _undoStack;
    List<TextBoxSnapshot> _redoStack;

    // ===== 策略（§3.3 内建裁决）=====

    int _maxLength;
    bool _isReadOnly;

    public TextBoxModel() {
        _undoStack = new List<TextBoxSnapshot>();
        _redoStack = new List<TextBoxSnapshot>();
    }

    // ===== 只读状态面（供壳同步 DP/镜像）=====

    /// <summary>已提交文本。</summary>
    public string Text {
        get { return _text; }
    }

    /// <summary>选区活动端（= 插入符位置）。</summary>
    public int Caret {
        get { return _caret; }
    }

    /// <summary>选区不动端（Shift 扩选收缩原点）。</summary>
    public int Anchor {
        get { return _anchor; }
    }

    /// <summary>IME 组字 overlay（不进 text）。</summary>
    public string Composition {
        get { return _composition; }
    }

    /// <summary>状态版本号（每次突变 +1；前缀宽度缓存失效依据）。</summary>
    public int Version {
        get { return _version; }
    }

    /// <summary>选区起点（派生只读：归一化区间下端，始终 ≤ 活动端）。</summary>
    public int SelectionStart {
        get {
            if (_caret < _anchor) {
                return _caret;
            }
            return _anchor;
        }
    }

    /// <summary>选区长度（派生只读：0 = 无选区）。</summary>
    public int SelectionLength {
        get {
            int d = _caret - _anchor;
            if (d < 0) {
                d = -d;
            }
            return d;
        }
    }

    /// <summary>最大长度（0 = 不限）。</summary>
    public int MaxLength {
        get { return _maxLength; }
        set { _maxLength = value; }
    }

    /// <summary>只读策略（true = 拒绝一切编辑突变）。</summary>
    public bool IsReadOnly {
        get { return _isReadOnly; }
        set { _isReadOnly = value; }
    }

    // ===== 编辑操作（§3.2 全部同步、可单测）=====

    /// <summary>
    /// 在选区活动端插入文本；有选区先整体替换（选区消费收敛为一点）。
    /// 连续 Insert 合并为一个撤销单元。
    /// </summary>
    public void Insert(string chunk) {
        if (_isReadOnly) {
            return;
        }
        if (chunk == null || chunk == "") {
            return;
        }
        this.PushUndoSnapshot(true);
        this.ReplaceRange(SelectionStart, SelectionLength, chunk);
        _caret = SelectionStart + chunk.Length;
        _anchor = _caret;
        _version = _version + 1;
    }

    /// <summary>Backspace：有选区整体删除；否则删 caret 前一字符。</summary>
    public void DeleteBackward() {
        if (_isReadOnly) {
            return;
        }
        if (SelectionLength > 0) {
            this.PushUndoSnapshot(false);
            this.ReplaceRange(SelectionStart, SelectionLength, "");
            _caret = SelectionStart;
            _anchor = _caret;
            _version = _version + 1;
            return;
        }
        if (_caret <= 0) {
            return;
        }
        this.PushUndoSnapshot(false);
        this.ReplaceRange(_caret - 1, 1, "");
        _caret = _caret - 1;
        _anchor = _caret;
        _version = _version + 1;
    }

    /// <summary>Delete：有选区整体删除；否则删 caret 后一字符。</summary>
    public void DeleteForward() {
        if (_isReadOnly) {
            return;
        }
        if (SelectionLength > 0) {
            this.PushUndoSnapshot(false);
            this.ReplaceRange(SelectionStart, SelectionLength, "");
            _caret = SelectionStart;
            _anchor = _caret;
            _version = _version + 1;
            return;
        }
        if (_caret >= _text.Length) {
            return;
        }
        this.PushUndoSnapshot(false);
        this.ReplaceRange(_caret, 1, "");
        _version = _version + 1;
    }

    /// <summary>
    /// 移动光标（= 选区活动端）。extend 对应 Shift 扩选（保 anchor）；
    /// 无 extend 时光标落点同时成为新 anchor（选区清空）。
    /// </summary>
    public void MoveCaret(MoveDirection direction, MoveGranularity granularity, bool extend) {
        int target = _caret;
        if (granularity == MoveGranularity.Char) {
            if (direction == MoveDirection.Backward) {
                target = _caret - 1;
            } else {
                target = _caret + 1;
            }
        } else if (granularity == MoveGranularity.Word) {
            if (direction == MoveDirection.Backward) {
                target = this.WordBoundaryBackward(_caret);
            } else {
                target = this.WordBoundaryForward(_caret);
            }
        } else if (granularity == MoveGranularity.Home) {
            target = 0;
        } else {
            target = _text.Length;
        }
        if (target < 0) {
            target = 0;
        }
        if (target > _text.Length) {
            target = _text.Length;
        }
        _caret = target;
        if (!extend) {
            _anchor = target;
        }
        _version = _version + 1;
    }

    /// <summary>全选（anchor=0，caret 落于末端；Shift 收缩自末端）。</summary>
    public void SelectAll() {
        _anchor = 0;
        _caret = _text.Length;
        _version = _version + 1;
    }

    /// <summary>清空选区（anchor = caret，不动光标）。</summary>
    public void ClearSelection() {
        _anchor = _caret;
        _version = _version + 1;
    }

    /// <summary>设置选区（anchor=不动端，active=活动端）并同步 caret。</summary>
    public void SetSelection(int anchor, int active) {
        _anchor = this.Clamp(anchor);
        _caret = this.Clamp(active);
        _version = _version + 1;
    }

    /// <summary>程序化设置光标（选区收敛为点）。</summary>
    public void SetCaret(int index) {
        _caret = this.Clamp(index);
        _anchor = _caret;
        _version = _version + 1;
    }

    /// <summary>撤销（快照式恢复；恢复体入 redo 栈）。</summary>
    public bool Undo() {
        return this.RestoreFrom(_undoStack, _redoStack);
    }

    /// <summary>重做（快照式恢复；恢复体入 undo 栈且重开合并窗口）。</summary>
    public bool Redo() {
        return this.RestoreFrom(_redoStack, _undoStack);
    }

    /// <summary>程序化设置全文（绕过撤销合并，独立快照；清空组字）。</summary>
    public void SetText(string text) {
        if (text == null) {
            text = "";
        }
        // 同值守卫：绑定回声（VM 写回 → 信号 → Apply 同值）不进撤销栈、
        // 不破坏组字进行态——文本与组字均无变化即无操作。
        if (text == _text && _composition == "") {
            return;
        }
        this.PushUndoSnapshot(false);
        _text = this.ApplyMaxLength(text);
        _composition = "";
        _caret = this.Clamp(_caret);
        _anchor = _caret;
        _version = _version + 1;
    }

    // ===== 组字（§3.2 IME；commit 是独立撤销单元）=====

    /// <summary>设置组字预览（不进 text，不影响撤销栈）。</summary>
    public void SetComposition(string text) {
        if (text == null) {
            text = "";
        }
        _composition = text;
        _version = _version + 1;
    }

    /// <summary>IME 上屏：消费选区、插入 chunk、清空组字（独立撤销单元）。</summary>
    public void CommitComposition(string chunk) {
        if (chunk == null || chunk == "") {
            _composition = "";
            _version = _version + 1;
            return;
        }
        this.PushUndoSnapshot(false);
        this.ReplaceRange(SelectionStart, SelectionLength, chunk);
        _caret = SelectionStart + chunk.Length;
        _anchor = _caret;
        _composition = "";
        _version = _version + 1;
    }

    /// <summary>取消组字（仅清空 overlay，不动 text/撤销栈）。</summary>
    public void CancelComposition() {
        _composition = "";
        _version = _version + 1;
    }

    // ===== 内部：突变原语 =====

    void ReplaceRange(int start, int length, string replacement) {
        string next = _text.Substring(0, start) + replacement + _text.Substring(start + length);
        _text = this.ApplyMaxLength(next);
    }

    string ApplyMaxLength(string next) {
        if (_maxLength > 0 && next.Length > _maxLength) {
            return next.Substring(0, _maxLength);
        }
        return next;
    }

    int Clamp(int index) {
        if (index < 0) {
            return 0;
        }
        if (index > _text.Length) {
            return _text.Length;
        }
        return index;
    }

    void PushUndoSnapshot(bool mergeable) {
        if (!mergeable || !_mergeableInsertOpen) {
            _undoStack.Add(new TextBoxSnapshot(_text, _caret, _anchor));
            while (_undoStack.Count > StackCapacity) {
                _undoStack.RemoveAt(0);
            }
            _redoStack.Clear();
        }
        _mergeableInsertOpen = mergeable;
    }

    bool RestoreFrom(List<TextBoxSnapshot> source, List<TextBoxSnapshot> target) {
        if (source.Count == 0) {
            return false;
        }
        TextBoxSnapshot snap = source[source.Count - 1];
        source.RemoveAt(source.Count - 1);
        target.Add(new TextBoxSnapshot(_text, _caret, _anchor));
        while (target.Count > StackCapacity) {
            target.RemoveAt(0);
        }
        _text = snap.Text;
        _caret = this.Clamp(snap.Caret);
        _anchor = this.Clamp(snap.Anchor);
        _composition = "";
        _mergeableInsertOpen = false;
        _version = _version + 1;
        return true;
    }

    // ===== 内部：词边界（空白为界；连续非空白段视为一词）=====

    bool IsBlankAt(int index) {
        string ch = _text.Substring(index, 1);
        return ch == " " || ch == "\t";
    }

    int WordBoundaryForward(int start) {
        int n = _text.Length;
        int i = start;
        while (i < n && this.IsBlankAt(i)) {
            i = i + 1;
        }
        while (i < n && !this.IsBlankAt(i)) {
            i = i + 1;
        }
        return i;
    }

    int WordBoundaryBackward(int start) {
        int i = start;
        while (i > 0 && this.IsBlankAt(i - 1)) {
            i = i - 1;
        }
        while (i > 0 && !this.IsBlankAt(i - 1)) {
            i = i - 1;
        }
        return i;
    }
}
