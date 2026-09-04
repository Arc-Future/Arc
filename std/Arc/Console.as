namespace Arc {
    /// <summary>
    /// 控制台 I/O 门面。
    /// </summary>
    public class Console {
        // ── 输出 ──

        [Builtin(ABI = "rt_console_write")]
        public static void Write(string message) { }

        [Builtin(ABI = "rt_console_write_line")]
        public static void WriteLine(string message) { }

        [Builtin(ABI = "rt_console_write_line")]
        public static void WriteLine() { }

        // ── 输入 ──

        [Builtin(ABI = "rt_console_read_line")]
        public static string ReadLine() { }

        [Builtin(ABI = "rt_console_read")]
        public static int Read() { }

        /// <summary>读取用户按下的下一个字符，不在控制台回显。</summary>
        [Builtin(ABI = "rt_console_read_key")]
        public static int ReadKey() { }

        /// <summary>获取键盘缓冲区中是否有按键可用。</summary>
        [Builtin(ABI = "rt_console_key_available")]
        public static bool KeyAvailable() { return false; }

        // ── 颜色 ──

        [Builtin(ABI = "rt_console_set_fg")]
        public static void SetForegroundColor(int color) { }

        [Builtin(ABI = "rt_console_set_bg")]
        public static void SetBackgroundColor(int color) { }

        [Builtin(ABI = "rt_console_get_fg")]
        public static int GetForegroundColor() { return 0; }

        [Builtin(ABI = "rt_console_get_bg")]
        public static int GetBackgroundColor() { return 0; }

        [Builtin(ABI = "rt_console_reset_color")]
        public static void ResetColor() { }

        // ── 屏幕控制 ──

        /// <summary>清空控制台缓冲区。</summary>
        [Builtin(ABI = "rt_console_clear")]
        public static void Clear() { }

        /// <summary>发出控制台蜂鸣声。</summary>
        [Builtin(ABI = "rt_console_beep")]
        public static void Beep() { }

        /// <summary>设置光标在控制台中的行列位置（0-based）。</summary>
        [Builtin(ABI = "rt_console_set_cursor_pos")]
        public static void SetCursorPosition(int left, int top) { }

        /// <summary>获取或设置光标是否可见。</summary>
        [Builtin(ABI = "rt_console_cursor_visible_get")]
        public static bool GetCursorVisible() { return false; }

        [Builtin(ABI = "rt_console_cursor_visible_set")]
        public static void SetCursorVisible(bool visible) { }

        /// <summary>获取控制台窗口宽度。</summary>
        [Builtin(ABI = "rt_console_window_width")]
        public static int WindowWidth() { return 0; }

        /// <summary>获取控制台窗口高度。</summary>
        [Builtin(ABI = "rt_console_window_height")]
        public static int WindowHeight() { return 0; }

        /// <summary>获取或设置控制台窗口标题。</summary>
        [Builtin(ABI = "rt_console_get_title")]
        public static string GetTitle() { return ""; }

        [Builtin(ABI = "rt_console_set_title")]
        public static void SetTitle(string title) { }

        // ── stderr 输出 ──

        [Builtin(ABI = "rt_console_error_write")]
        public static void ErrorWrite(string message) { }

        [Builtin(ABI = "rt_console_error_write_line")]
        public static void ErrorWriteLine(string message) { }
    }
}
