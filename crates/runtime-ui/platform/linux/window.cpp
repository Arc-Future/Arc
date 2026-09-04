/* RFC 026 X11 + XIM IME. Common: native/platform/common/ */
#include "../common/rt_ui_platform.h"
#include "../common/rt_ui_ime_internal.h"
#include "../common/rt_ui_ime_types.h"

#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>

/*
 * X11 + XIM（026-ime-east-asian-input.md §5 · Linux 桌面绿路径 Draft）
 *
 * 最小 IME：XOpenIM / XCreateIC / XFilterEvent / Xutf8LookupString →
 * rt_ui_ime_dispatch（与 Win32 rt_ui_ime_* 同 ABI）。
 *
 * 运行时依赖：桌面 IME（IBus / Fcitx 等）经 XIM 转发；无自绘候选窗。
 * 无 DISPLAY（CI/headless）：XOpenDisplay 失败 → rt_window_create 返回 NULL。
 *
 * Wayland TODO（登记后置 · 非本刀 · 勿假实现）：
 *   native Wayland 须 zwp_text_input_v3（text-input-unstable-v3）：
 *     preedit_string / commit_string / set_surrounding_text /
 *     set_cursor_rectangle；XWayland 会话可继续走本 XIM 路径。
 *   OHOS 输入法另立项（InputMethodConnection 族），不在 RFC 026 IME 矩阵本刀。
 */

typedef struct {
    Display* display;
    Window window;
    Atom wm_protocols;
    Atom wm_delete;
    int should_close;
    RtUiElement* root_element;
    RtUiElement* pointer_down_elem;
    XIM xim;
    XIC xic;
} RtWindowImpl;

/* 活动窗口登记（rt_event_poll / rt_event_wait 维护；跨线程唤醒消费）。 */
static RtWindowImpl* g_rt_linux_active_win = NULL;

static RtWindowImpl* g_x11_ime_win = NULL;

/* unique-wgpu：进程级 X11 Display 指针——供 wgpu_create_surface_from_handle
 * 构造 WGPUSurfaceSourceXlibWindow（需要 Display* + Window 配对）。 */
static Display* g_arc_x11_display = NULL;

void* rt_x11_display_get(void) {
    return g_arc_x11_display;
}

static int rt_x11_client_size(RtWindowImpl* win, int32_t* out_w, int32_t* out_h) {
    if (!win || !out_w || !out_h) return 0;
    XWindowAttributes attrs;
    if (!XGetWindowAttributes(win->display, win->window, &attrs)) return 0;
    *out_w = attrs.width;
    *out_h = attrs.height;
    return (*out_w > 0 && *out_h > 0);
}

static void rt_x11_request_redraw(RtWindowImpl* win) {
    if (!win || !win->display || !win->window) return;
    XClearArea(win->display, win->window, 0, 0, 0, 0, True);
    XFlush(win->display);
}

static void rt_x11_pointer_down(RtWindowImpl* win, int32_t px, int32_t py) {
    if (!win || !win->root_element) return;
    int32_t cw = 0, ch = 0;
    if (!rt_x11_client_size(win, &cw, &ch)) return;
    RtUiElement* hit = rt_ui_hit_test(win->root_element, cw, ch, px, py);
    if (win->pointer_down_elem) {
        win->pointer_down_elem->is_pressed = 0;
    }
    win->pointer_down_elem = hit;
    if (hit) {
        hit->is_pressed = 1;
        rt_x11_request_redraw(win);
    }
}

static void rt_x11_pointer_up(RtWindowImpl* win, int32_t px, int32_t py) {
    if (!win || !win->root_element) return;
    int32_t cw = 0, ch = 0;
    if (!rt_x11_client_size(win, &cw, &ch)) return;
    RtUiElement* hit = rt_ui_hit_test(win->root_element, cw, ch, px, py);
    RtUiElement* down = win->pointer_down_elem;
    win->pointer_down_elem = NULL;
    if (down) {
        down->is_pressed = 0;
    }
    if (hit && hit == down &&
        hit->type_name && strcmp(hit->type_name, "Button") == 0) {
        rt_ui_dispatch_button_click(hit);
        rt_x11_request_redraw(win);
    } else if (hit || down) {
        rt_x11_request_redraw(win);
    }
}

static void rt_x11_ime_apply_spot(RtWindowImpl* win) {
    if (!win || !win->xic) return;
    int32_t x = 16, y = 16, w = 200, h = 28;
    RtUiElement* cand_in = NULL; int32_t cx=0,cy=0,cw=0,ch=0;
    if (rt_ui_ime_query_candidate_rect(&cand_in, &cx, &cy, &cw, &ch)) {
        x = cx; y = cy; w = cw; h = ch;
    }
    if (w < 8) w = 8;
    if (h < 8) h = 8;
    XPoint spot = {(short)x, (short)(y + h)};
    XVaNestedList attrs = XVaCreateNestedList(0, XNSpotLocation, &spot, NULL);
    if (attrs) {
        XSetICValues(win->xic, XNPreeditAttributes, attrs, NULL);
        XFree(attrs);
    }
}

void rt_linux_ime_on_focus_changed(RtUiElement* input) {
    (void)input;
    if (!g_x11_ime_win || !g_x11_ime_win->xic) return;
    if (rt_ui_ime_get_focus()) {
        XSetICFocus(g_x11_ime_win->xic);
    } else {
        XUnsetICFocus(g_x11_ime_win->xic);
    }
}

void rt_linux_ime_on_candidate_rect(void) {
    if (g_x11_ime_win) rt_x11_ime_apply_spot(g_x11_ime_win);
}

static void rt_x11_ime_shutdown(RtWindowImpl* win) {
    if (!win) return;
    if (win->xic) {
        XDestroyIC(win->xic);
        win->xic = NULL;
    }
    if (win->xim) {
        XCloseIM(win->xim);
        win->xim = NULL;
    }
}

static int rt_x11_ime_init(RtWindowImpl* win) {
    win->xim = XOpenIM(win->display, NULL, NULL, NULL);
    if (!win->xim) return 0;
    win->xic = XCreateIC(win->xim,
                         XNInputStyle, XIMPreeditNothing | XIMStatusNothing,
                         XNClientWindow, win->window,
                         XNFocusWindow, win->window,
                         NULL);
    if (!win->xic) {
        XCloseIM(win->xim);
        win->xim = NULL;
        return 0;
    }
    if (rt_ui_ime_get_focus()) XSetICFocus(win->xic);
    rt_x11_ime_apply_spot(win);
    return 1;
}

static void rt_x11_ime_dispatch_lookup(RtWindowImpl* win, XKeyEvent* key,
                                       char* buf, int buf_len, Status status) {
    (void)win;
    if (buf_len <= 0 || !rt_ui_ime_get_focus()) return;
    buf[buf_len] = '\0';
    if (status == XLookupPreviewing) {
        RtUiImeComposition comp = {0};
        comp.text = buf;
        comp.cursor = buf_len;
        rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_UPDATE, &comp);
    }
    if (status == XLookupChars || status == XLookupBoth) {
        rt_ui_ime_dispatch(RT_UI_IME_COMMIT, buf);
        rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_END, NULL);
    }
}

static void rt_x11_ime_handle_key(RtWindowImpl* win, XEvent* ev) {
    if (!win->xic || !rt_ui_ime_get_focus()) return;

    char buf[256];
    KeySym keysym = NoSymbol;
    Status status = 0;
    int count = Xutf8LookupString(win->xic, &ev->xkey, buf, (int)sizeof(buf) - 1,
                                  &keysym, &status);
    if (count == 0 && status == XLookupNone) return;
    if (count < 0) {
        size_t need = (size_t)(-count);
        char* big = (char*)malloc(need);
        if (!big) return;
        count = Xutf8LookupString(win->xic, &ev->xkey, big, (int)need - 1,
                                  &keysym, &status);
        rt_x11_ime_dispatch_lookup(win, &ev->xkey, big, count, status);
        free(big);
        return;
    }
    rt_x11_ime_dispatch_lookup(win, &ev->xkey, buf, count, status);
}

void* rt_window_create(const char* title, int32_t width, int32_t height) {
    Display* dpy = XOpenDisplay(NULL);
    if (!dpy) return NULL;
    RtWindowImpl* win = (RtWindowImpl*)calloc(1, sizeof(RtWindowImpl));
    if (!win) { XCloseDisplay(dpy); return NULL; }
    win->display = dpy;
    g_arc_x11_display = dpy;
    Window root = DefaultRootWindow(dpy);
    win->window = XCreateSimpleWindow(
        dpy, root, 100, 100, (unsigned)width, (unsigned)height, 1, 0, 0);
    XStoreName(dpy, win->window, title ? title : "Arc");
    XSelectInput(dpy, win->window,
                 ExposureMask | KeyPressMask | FocusChangeMask | StructureNotifyMask |
                 ButtonPressMask | ButtonReleaseMask | PointerMotionMask);
    win->wm_protocols = XInternAtom(dpy, "WM_PROTOCOLS", False);
    win->wm_delete = XInternAtom(dpy, "WM_DELETE_WINDOW", False);
    XSetWMProtocols(dpy, win->window, &win->wm_delete, 1);
    XMapWindow(dpy, win->window);
    XFlush(dpy);
    g_x11_ime_win = win;
    (void)rt_x11_ime_init(win);
    return win;
}

void rt_window_destroy(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (g_x11_ime_win == win) g_x11_ime_win = NULL;
    rt_x11_ime_shutdown(win);
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
        win->root_element = NULL;
    }
    if (win->display) {
        if (win->window) XDestroyWindow(win->display, win->window);
        XCloseDisplay(win->display);
    }
    free(win);
}

void rt_window_close(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->display && win->window) {
        XEvent ev = {0};
        ev.type = ClientMessage;
        ev.xclient.window = win->window;
        ev.xclient.message_type = win->wm_protocols;
        ev.xclient.format = 32;
        ev.xclient.data.l[0] = (long)win->wm_delete;
        XSendEvent(win->display, win->window, False, NoEventMask, (XEvent*)&ev);
        XFlush(win->display);
    } else {
        win->should_close = 1;
    }
}

int32_t rt_window_should_close(void* window) {
    if (!window) return 1;
    return ((RtWindowImpl*)window)->should_close ? 1 : 0;
}

/* M-focus Draft: Linux keyboard 待迁 — invalidate 供 Arc FocusManager 重绘焦点框。 */
void rt_window_invalidate(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->window && win->display) {
        XClearArea(win->display, win->window, 0, 0, 0, 0, True);
    }
}

/* M3：跨平台 wgpu 唯一后端——X11 窗口不再做软件光栅 blit。渲染由 Arc 侧
 * FramePump 驱动 wgpu（WgpuRender）完成；root_element 仅用于命中测试/指针/
 * IME/滚动等逻辑面。X11 窗口自身交给 wgpu 的 Xlib surface 呈现。 */

int32_t rt_event_poll(void* window) {
    if (!window) return RT_EVENT_CLOSE;
    RtWindowImpl* win = (RtWindowImpl*)window;
    g_rt_linux_active_win = win;
    if (!XPending(win->display)) return RT_EVENT_NONE;
    XEvent ev;
    XNextEvent(win->display, &ev);

    if (XFilterEvent(&ev, None)) {
        return RT_EVENT_NONE;
    }

    if (ev.type == ClientMessage &&
        ev.xclient.message_type == win->wm_protocols &&
        (Atom)ev.xclient.data.l[0] == win->wm_delete) {
        win->should_close = 1;
        return RT_EVENT_CLOSE;
    }
    if (ev.type == FocusOut) {
        if (rt_ui_ime_get_focus()) {
            rt_ui_ime_dispatch(RT_UI_IME_FOCUS_LOST, NULL);
        }
        return RT_EVENT_NONE;
    }
    if (ev.type == KeyPress) {
        KeySym sym = XLookupKeysym(&ev.xkey, 0);
        if (sym == XK_Escape) { win->should_close = 1; return RT_EVENT_KEY; }
        if (rt_ui_ime_get_focus() && win->xic) {
            rt_x11_ime_handle_key(win, &ev);
            return RT_EVENT_NONE;
        }
    }
    if (ev.type == ButtonPress && ev.xbutton.button == Button1 && win->root_element) {
        rt_x11_pointer_down(win, (int32_t)ev.xbutton.x, (int32_t)ev.xbutton.y);
        return RT_EVENT_NONE;
    }
    if (ev.type == ButtonRelease && ev.xbutton.button == Button1 && win->root_element) {
        rt_x11_pointer_up(win, (int32_t)ev.xbutton.x, (int32_t)ev.xbutton.y);
        return RT_EVENT_NONE;
    }
    if (ev.type == ConfigureNotify) {
        return RT_EVENT_NONE;
    }
    if (ev.type == Expose && ev.xexpose.count == 0) {
        return RT_EVENT_NONE;
    }
    return RT_EVENT_NONE;
}

void rt_window_set_text(void* window, const char* text) {
    (void)window; (void)text;
}

/* A-1② 配套：空闲阻塞等待——poll X 连接 fd，替代忙轮询空转。
 * timeout_ms 负值 = 无限等待；返回 1 = 有事件待读，0 = 超时。 */
int32_t rt_event_wait(void* window, int32_t timeout_ms) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (!win->display) return 0;
    if (win->should_close) return 0;
    g_rt_linux_active_win = win;
    struct pollfd pfd;
    pfd.fd = XConnectionNumber(win->display);
    pfd.events = POLLIN;
    pfd.revents = 0;
    int t = (timeout_ms < 0) ? -1 : (int)timeout_ms;
    return (poll(&pfd, 1, t) > 0) ? 1 : 0;
}

/* 跨线程唤醒：向活动窗口投递空 ClientMessage（XSendEvent 线程安全），
 * 使阻塞中的 rt_event_wait 立即返回；消息落入 rt_event_poll 的
 * ClientMessage 分支外的默认路径，无副作用。 */
void rt_ui_wake_ui_thread(void) {
    RtWindowImpl* win = g_rt_linux_active_win;
    if (!win || !win->display || !win->window) {
        return;
    }
    XEvent ev;
    memset(&ev, 0, sizeof(ev));
    ev.type = ClientMessage;
    ev.xclient.window = win->window;
    ev.xclient.message_type = None;
    ev.xclient.format = 8;
    XSendEvent(win->display, win->window, False, NoEventMask, &ev);
    XFlush(win->display);
}

/* RFC 026 §D7.2 X11: 返回 X11 Window（Drawable）供 wgpu_create_surface_from_handle。
 * Display* 通过 rt_x11_display_get 单独获取（wgpu X11 surface 需要 Display + Window 配对）。 */
int64_t rt_window_native_handle(void* window) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    return (int64_t)win->window;
}

/* unique-wgpu：设置 wgpu 接管渲染标志。X11 渲染由 Arc 侧 FramePump 驱动 wgpu
 * 完成；root_element 仅消费逻辑面（命中/指针/IME/滚动）。 */
void rt_window_set_wgpu_active(void* window, int32_t active) {
    (void)window; (void)active;
}

/* unique-wgpu：系统 DPI 缩放系数（X11_DpiScale）。Arc 侧 WgpuRender 用它把
 * DIP 布局坐标换算为物理像素。 */
double rt_window_dpi_scale(void) {
    const char* dpi_env = getenv("XFT_DPI");
    if (dpi_env) {
        double v = atof(dpi_env);
        if (v > 0.0) return v / 96.0;
    }
    return 1.0;
}

void rt_window_get_client_size(void* window, int32_t* out_w, int32_t* out_h) {
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (!win->display || !win->window) return;
    XWindowAttributes attrs;
    if (XGetWindowAttributes(win->display, win->window, &attrs)) {
        if (out_w) *out_w = attrs.width;
        if (out_h) *out_h = attrs.height;
    }
}

void rt_window_set_root_element(void* window, RtUiElement* root) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
    }
    win->root_element = root;
    win->pointer_down_elem = NULL;
    if (root) {
        XFlush(win->display);
    }
}
