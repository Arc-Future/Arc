// RFC 037 M2: Arc.UI.Components —— WindowHost 静态桥接。
//
// WindowHost 是 codegen 拦截的静态桥接类，提供 Window.Show() 到原生 ABI
// 的转发。Arc 源码层面，Window.Show() 调用 WindowHost.RunWithRoot(title,
// width, height, rootHandle)；codegen emit_call.rs 拦截此静态调用，直接
// 发射 `call void @__arc_window_run_with_root(...)` LLVM IR，方法体不执行。
//
// 这种模式对标 std/Arc/IO/File.as 的 stub 设计——Arc 层声明方法签名供
// typeck 解析，codegen 在 emit 阶段拦截转发到原生 ABI。
//
// 历史演进：
//   - 早期 demo：`Window.RunWithText` 静态方法（含 text 参数）——已废弃
//   - M2 重构：`WindowHost.Run` 4 参数（title/w/h/text）——已废弃
//   - M3 当前：`WindowHost.RunWithRoot` 4 参数（title/w/h/rootHandle）
//     废弃 text 参数——内容由 rootHandle 指向的元素树承载，
//     不再需要纯文本回退路径。Window.Text 字段已移除。

namespace Arc.UI.Components;

/// <summary>
/// 静态桥接类——Window 实例方法到原生 ABI 的转发层。
///
/// 用户代码不应直接调用此类——Window.Show() 会自动调用。
/// </summary>
public static class WindowHost {
    // ── RFC 037 M3：UI Element Tree 平台镜像 ABI ──
    //
    // 以下静态方法均为 codegen 拦截 stub——Arc 源码层方法体不执行，
    // emit_call.rs::try_emit_window_host_element 直接发射 rt_ui_* LLVM IR。
    // 句柄在 Arc 侧为 `long`（i64），C ABI 侧为 `RtUiElement*`（ptr）。

    /// <summary>
    /// 创建原生 RtUiElement 节点，返回 i64 句柄。
    /// </summary>
    [Builtin(ABI = "rt_ui_element_create")]
    internal static long ElementCreate(string typeName) {
        return 0;
    }

    /// <summary>
    /// 设置元素字符串属性。
    /// </summary>
    [Builtin(ABI = "rt_ui_element_set_string")]
    internal static void ElementSetString(long handle, string name, string value) {
    }

    /// <summary>
    /// 设置元素数值属性（double）。
    /// </summary>
    [Builtin(ABI = "rt_ui_element_set_number")]
    internal static void ElementSetNumber(long handle, string name, double value) {
    }

    /// <summary>
    /// 设置元素布尔属性（int 0/1）。
    /// </summary>
    [Builtin(ABI = "rt_ui_element_set_bool")]
    internal static void ElementSetBool(long handle, string name, int value) {
    }

    /// <summary>
    /// 将子元素挂到父元素下。
    /// </summary>
    [Builtin(ABI = "rt_ui_element_add_child")]
    internal static void ElementAddChild(long parentHandle, long childHandle) {
    }

    /// <summary>
    /// 注册平台 Button 点击回调（RFC 037 D10.4）。
    /// </summary>
    [Builtin(ABI = "rt_ui_set_button_click_handler")]
    internal static void SetButtonClickHandler(Action<long> handler) {
    }

    /// <summary>Register platform Button visual-state callback (Hover/Pressed Draft).</summary>
    [Builtin(ABI = "rt_ui_set_button_visual_state_handler")]
    internal static void SetButtonVisualStateHandler(Action<long, int, int> handler) {
    }

    /// <summary>Register platform control click callback by type name (RFC 037 D10.6).</summary>
    [Builtin(ABI = "rt_ui_set_control_click_handler")]
    internal static void SetControlClickHandler(string typeName, Action<long> handler) {
    }

    /// <summary>Register platform control visual-state callback by type name (RFC 037 D10.6).</summary>
    [Builtin(ABI = "rt_ui_set_control_visual_state_handler")]
    internal static void SetControlVisualStateHandler(string typeName, Action<long, int, int> handler) {
    }

    /// <summary>Register platform control drag callback by type name (Slider value, RFC 037 D10.6).</summary>
    [Builtin(ABI = "rt_ui_set_control_drag_handler")]
    internal static void SetControlDragHandler(string typeName, Action<long, double> handler) {
    }

    /// <summary>Clear per-type control handler registrations (Window calls before each Show).</summary>
    [Builtin(ABI = "rt_ui_clear_control_handlers")]
    internal static void ClearControlHandlers() {
    }


    [Builtin(ABI = "rt_ui_set_input_focus_handler")]
    internal static void SetInputFocusHandler(Action<long> handler) {
    }

    /// <summary>Register input click handler (M-caret2: click positions caret).</summary>
    [Builtin(ABI = "rt_ui_set_input_click_handler")]
    internal static void SetInputClickHandler(Action<long, double> handler) {
    }

    [Builtin(ABI = "rt_ui_set_keyboard_handler")]
    internal static void SetKeyboardHandler(Action<int, int> handler) {
    }

    /// <summary>Bind Arc logical element to platform mirror (pointer-events / IME).</summary>
    [Builtin(ABI = "rt_ui_element_set_arc_ptr")]
    internal static void ElementSetArcPtr(long platformHandle, object arcElement) {
    }

    [Builtin(ABI = "rt_ui_set_scroll_wheel_handler")]
    internal static void SetScrollWheelHandler(Action<long, int, int> handler) {
    }

    [Builtin(ABI = "rt_ui_hit_test")]
    internal static long HitTest(long rootHandle, int width, int height, int x, int y) {
        return 0;
    }

    [Builtin(ABI = "rt_ui_set_scroll_bar_handler")]
    internal static void SetScrollBarHandler(Action<long, int, double> handler) {
    }

    [Builtin(ABI = "rt_ui_invalidate_active_window")]
    internal static void InvalidateActiveWindow() {
    }

    // ── RFC 037 · 026-ime-east-asian-input.md §5.2 handler ABI ──

    [Builtin(ABI = "rt_ui_ime_install_arc_handler")]
    internal static void ImeInstallHandler() {
    }

    [Builtin(ABI = "rt_ui_ime_set_focus")]
    internal static void ImeSetFocus(long inputHandle) {
    }

    [Builtin(ABI = "rt_ui_ime_set_candidate_rect")]
    internal static void ImeSetCandidateRect(long inputHandle, int x, int y, int w, int h) {
    }

    /// <summary>从 native UTF-8 C 串复制为 Arc string（不释放 payload）。</summary>
    /// <summary>
    /// Copy native UTF-8 C string to Arc string (does not free payload).
    /// </summary>
    internal static string NativeCStringFromPtr(long cstrPtr) {
        return "";
    }

    /// <summary>读取 RtUiImeComposition* 的 text 字段（UTF-8）。</summary>
    /// <summary>
    /// Read RtUiImeComposition text field (UTF-8).
    /// </summary>
    internal static string ImeCompositionText(long compositionPtr) {
        return "";
    }

    [Builtin(ABI = "rt_event_poll")]
    internal static int EventPoll(long windowHandle) {
        return 0;
    }

    /// <summary>
    /// 空闲阻塞等待：直至窗口线程收到新输入/消息，或 timeoutMs 毫秒超时
    /// （负值 = 无限等待）。帧泵空闲期调用，替代忙轮询空转。
    /// </summary>
    [Builtin(ABI = "rt_event_wait")]
    internal static int WaitEvents(long windowHandle, int timeoutMs) {
        return 0;
    }

    /// <summary>跨线程唤醒 UI 泵：使阻塞中的 <see cref="WaitEvents"/> 立即返回。</summary>
    [Builtin(ABI = "rt_ui_wake_ui_thread")]
    internal static void WakeUIThread() {
    }

    [Builtin(ABI = "rt_window_should_close")]
    internal static int ShouldClose(long windowHandle) {
        return 1;
    }

    [Builtin(ABI = "rt_window_set_root_element")]
    internal static void SetRootElement(long windowHandle, long rootHandle) {
    }

    /// <summary>
    /// RFC 037 wgpu：设置 wgpu 接管渲染标志。激活后平台 WM_PAINT 跳过软件
    /// 光栅，由 FramePump 每 tick 驱动 wgpu 呈现。
    /// </summary>
    [Builtin(ABI = "rt_window_set_wgpu_active")]
    internal static void SetWgpuActive(long windowHandle, int active) {
    }



    // RFC 037 M3.5: element tree read accessors (codegen stubs -> rt_ui_element_get_*)

    /// <summary>
    /// Element type name (Text, Button, StackPanel, Window, Element, ...).
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_type_name")]
    internal static string ElementGetTypeName(long handle) {
        return "Element";
    }

    /// <summary>
    /// String property value; returns def when missing.
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_string")]
    internal static string ElementGetString(long handle, string name, string def) {
        return def;
    }

    /// <summary>
    /// Number property value; returns def when missing.
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_number")]
    internal static double ElementGetNumber(long handle, string name, double def) {
        return def;
    }

    /// <summary>
    /// Bool property value; returns def when missing.
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_bool")]
    internal static int ElementGetBool(long handle, string name, int def) {
        return def;
    }

    /// <summary>
    /// Child count.
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_child_count")]
    internal static int ElementGetChildCount(long handle) {
        return 0;
    }

    /// <summary>
    /// Child handle at index; out of range returns 0.
    /// </summary>
    [Builtin(ABI = "rt_ui_element_get_child")]
    internal static long ElementGetChild(long handle, int index) {
        return 0;
    }

    /// <summary>
    /// 创建窗口、设置根元素、进入消息循环。
    ///
    /// 4 参数版本（M3）：title/width/height/rootHandle。废弃的 text 参数
    /// 已移除——内容由 rootHandle 指向的元素树承载。codegen 拦截此调用并
    /// 直接发射 `call void @__arc_window_run_with_root(ptr title,
    /// i32 width, i32 height, i64 rootHandle)` LLVM IR。
    /// </summary>
    [Builtin(ABI = "rt_window_run_with_root")]
    internal static void RunWithRoot(string title, int width, int height, long rootHandle) {
        // body 不执行——codegen 拦截后直接 emit 原生 ABI 调用
    }

    // ── RFC 037 §D7.2: 渲染后端原生窗口 handle 提取 ──
    //
    // WgpuRender.Initialize 需要平台原生窗口 handle 创建 wgpu Surface。
    //   - Windows: HWND
    //   - Linux:   X11 Window
    //   - macOS:   NSView*
    //
    // Arc 侧通过 WindowHost.NativeHandle 获取——codegen emit_call.rs 拦截
    // 此调用并发射 `call i64 @rt_window_native_handle(ptr %window)` LLVM IR。
    // windowHandle 参数为 WindowHost.CreateWindow 返回的平台窗口句柄。

    /// <summary>
    /// 提取平台原生窗口 handle（HWND/Window/NSView）供 wgpu surface 创建。
    /// </summary>
    /// <param name="windowHandle">平台窗口不透明句柄（由 CreateWindow 返回）。</param>
    /// <returns>原生窗口 handle（i64 承载指针/Drawable）。</returns>
    [Builtin(ABI = "rt_window_native_handle")]
    public static long NativeHandle(long windowHandle) {
        return 0;
    }

    /// <summary>
    /// 创建平台窗口（不进入消息循环），返回不透明句柄。
    ///
    /// 与 RunWithRoot 不同——CreateWindow 仅创建窗口，由 WgpuRender
    /// 调用 NativeHandle 获取原生 handle 后初始化渲染后端。
    /// 然后由 WindowHost.RunEventLoop 进入消息循环。
    /// </summary>
    [Builtin(ABI = "rt_window_create")]
    public static long CreateWindow(string title, int width, int height) {
        return 0;
    }

    /// <summary>
    /// 进入平台消息循环（阻塞直到窗口关闭）。
    /// </summary>
    [Builtin(ABI = "rt_window_run_event_loop")]
    public static void RunEventLoop(long windowHandle) {
    }

    /// <summary>
    /// 销毁平台窗口（释放资源）。
    /// </summary>
    [Builtin(ABI = "rt_window_destroy")]
    public static void DestroyWindow(long windowHandle) {
    }

    /// <summary>
    /// 获取窗口客户区实际像素尺寸（创建窗口后/resize后调用，out参数返回宽高）。
    /// </summary>
    [Builtin(ABI = "rt_window_get_client_size")]
    internal static void GetClientSize(long windowHandle, out int width, out int height) {
        width = 0;
        height = 0;
    }

    /// <summary>
    /// 系统 DPI 缩放系数（DPI / 96.0）。WgpuRender 用它把 DIP 布局坐标换算为物理像素。
    /// </summary>
    [Builtin(ABI = "rt_window_dpi_scale")]
    internal static double SystemDpiScale() {
        return 1.0;
    }
}

