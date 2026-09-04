/* RFC 026 Win32 + IMM32 IME. Common: native/platform/common/ */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../common/rt_ui_platform.h"
#include "pointer_win32.h"
#include "scroll_win32.h"
#include "ime_win32.h"
#include "keyboard_win32.h"

#define WIN32_LEAN_AND_MEAN
#define UNICODE
#define _UNICODE
#include <windows.h>

/* RFC 038 M3：已删除 Direct2D / DirectWrite 软回退（wgpu 唯一后端）。
 * text_content 仅保留供 rt_window_set_text 设置窗口标题（ABI 兼容）。
 * root_element：元素树根节点，非 NULL 时 WM_PAINT 走软件光栅（device fallback，
 * 027 §8 R7）。wgpu 接管渲染后由 Arc 侧 FramePump 驱动，WM_PAINT 不再参与。 */
typedef struct RtWindowImpl {
    HWND hwnd;
    int should_close;
    wchar_t* text_content;            /* UTF-16 窗口标题（rt_window_set_text） */
    struct RtUiElement* root_element;
    struct RtUiElement* pointer_down_elem;
    struct RtUiElement* pointer_over_elem;
    int wgpu_active;   /* wgpu 接管渲染标志（Arc 侧 WgpuRender 置位） */
    int logical_w;     /* Arc DIP 客户区逻辑宽（创建参数；WM_SIZE 反推同步） */
    int logical_h;
} RtWindowImpl;

/* 窗口实际 DPI 缩放系数（定义见下，extern "C" 导出）；WndProc 内 WM_SIZE/WM_DPICHANGED 需用。 */
#ifdef __cplusplus
extern "C" {
#endif
double rt_window_dpi_scale(void);
#ifdef __cplusplus
}
#endif

static int g_platform_initialized = 0;
static RtWindowImpl* g_rt_ui_active_win = NULL;

/* 进程级初始化：设置 DPI awareness。
 * 调用时机：rt_window_create 首次执行时（idempotent）。 */
static void rt_platform_init(void) {
    if (g_platform_initialized) return;
    g_platform_initialized = 1;

    /* 1. Per-Monitor V2 DPI awareness——优先用 Win10 1703+ API，
     *    降级到 Per-Monitor V1，再降级到 System DPI Aware。 */
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (user32) {
        typedef BOOL (WINAPI* PFN_SetProcessDpiAwarenessContext)(HANDLE);
        typedef BOOL (WINAPI* PFN_SetProcessDpiAwareness)(int);
        PFN_SetProcessDpiAwarenessContext set_ctx =
            (PFN_SetProcessDpiAwarenessContext)GetProcAddress(
                user32, "SetProcessDpiAwarenessContext");
        if (set_ctx) {
            /* PER_MONITOR_AWARE_V2 == ((DPI_CONTEXT_HANDLE)-4) */
            set_ctx((HANDLE)-4);
        } else {
            PFN_SetProcessDpiAwareness set_aware =
                (PFN_SetProcessDpiAwareness)GetProcAddress(
                    user32, "SetProcessDpiAwareness");
            if (set_aware) {
                /* PROCESS_PER_MONITOR_DPI_AWARE == 2 */
                set_aware(2);
            } else {
                SetProcessDPIAware();
            }
        }
    }

    /* RFC 038 M3：已删除 Direct2D / DirectWrite 软回退（wgpu 唯一后端）。 */
}

/* 系统 DPI 缩放系数（DPI / 96.0）。Per-Monitor V2 感知进程下，Arc 的 Width/Height
 * 约定为 DIP（逻辑像素），物理客户端像素 = DIP * scale。Windows 10 1607+ 用
 * GetDpiForSystem；旧系统回退 GetDeviceCaps(LOGPIXELSX)。 */
static double rt_dpi_scale(void) {
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (user32) {
        typedef UINT (WINAPI* PFN_GetDpiForSystem)(void);
        PFN_GetDpiForSystem get_dpi = (PFN_GetDpiForSystem)GetProcAddress(
            user32, "GetDpiForSystem");
        if (get_dpi) {
            UINT dpi = get_dpi();
            if (dpi > 0) return (double)dpi / 96.0;
        }
    }
    HDC hdc = GetDC(NULL);
    double scale = 1.0;
    if (hdc) {
        int dpi = GetDeviceCaps(hdc, LOGPIXELSX);
        if (dpi > 0) scale = (double)dpi / 96.0;
        ReleaseDC(NULL, hdc);
    }
    return scale;
}

/* Win32 窗口过程——RFC 038 M3：wgpu 唯一后端。WM_PAINT 不再参与任何绘制
 * （wgpu 由 Arc 侧 FramePump 每 tick BeginFrame/RenderElementTree/EndFrame
 * 驱动）；此处的 BeginPaint/EndPaint 仅验证绘制区域，避免 GDI 重绘闪烁。 */
static LRESULT CALLBACK rt_window_wndproc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp) {
    RtWindowImpl* win =
        (RtWindowImpl*)GetWindowLongPtrW(hwnd, GWLP_USERDATA);

    if (rt_win32_ime_is_ime_message(msg)) {
        LRESULT ime_lr = rt_win32_ime_handle_message(hwnd, msg, wp, lp);
        if (ime_lr != (LRESULT)-1) return ime_lr;
    }

    if (msg == WM_PAINT) {
        /* RFC 038 M3：wgpu 唯一后端——WM_PAINT 仅验证绘制区域，
         * 实际呈现由 Arc 侧 FramePump 驱动 wgpu 完成。 */
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        EndPaint(hwnd, &ps);
        return 0;
    }
    if (msg == WM_ERASEBKGND) return 1;  /* wgpu 自己清屏，避免 GDI 闪烁 */
    if (msg == WM_SIZE) {
        /* 窗口物理尺寸变化 → 反推逻辑 DIP 并记录（跨 DPI 迁移的权威逻辑尺寸）。
         * wgpu surface 由 Arc 侧 WgpuRender 按需 reconfig，此处无需同步。 */
        if (win) {
            double scale = rt_window_dpi_scale();
            if (scale < 1.0) {
                scale = 1.0;
            }
            int lw = (int)((double)(int16_t)LOWORD(lp) / scale);
            int lh = (int)((double)(int16_t)HIWORD(lp) / scale);
            if (lw > 0) {
                win->logical_w = lw;
            }
            if (lh > 0) {
                win->logical_h = lh;
            }
        }
        return 0;
    }
    if (msg == WM_DPICHANGED) {
        /* Per-Monitor DPI 变化——以 Arc 权威逻辑 DIP 尺寸 × 新缩放重算物理客户区，
         * 不盲从 proposed 矩形：Windows 按创建时系统 DPI 推导的「逻辑尺寸」提案，
         * 多屏异 DPI（如系统 100% / 窗口屏 200%）下与 Arc 的 DIP 契约不一致，
         * 盲从会把窗口缩回错误物理尺寸（surface 与指针坐标随之全部错配）。 */
        RECT* proposed = (RECT*)lp;
        UINT new_dpi = HIWORD(wp);
        double scale = new_dpi > 0 ? (double)new_dpi / 96.0 : 1.0;
        if (scale < 1.0) {
            scale = 1.0;
        }
        int lw = win ? win->logical_w : 0;
        int lh = win ? win->logical_h : 0;
        if (lw <= 0 || lh <= 0) {
            SetWindowPos(hwnd, NULL,
                proposed->left, proposed->top,
                proposed->right - proposed->left,
                proposed->bottom - proposed->top,
                SWP_NOZORDER | SWP_NOACTIVATE);
            return 0;
        }
        RECT rc2 = { 0, 0, (int)((double)lw * scale), (int)((double)lh * scale) };
        typedef BOOL (WINAPI* PFN_AdjustForDpi)(RECT*, DWORD, BOOL, DWORD, UINT);
        HMODULE user32_dpi = GetModuleHandleW(L"user32.dll");
        PFN_AdjustForDpi adjust_for_dpi = user32_dpi ?
            (PFN_AdjustForDpi)GetProcAddress(user32_dpi, "AdjustWindowRectExForDpi") : NULL;
        if (adjust_for_dpi) {
            adjust_for_dpi(&rc2, WS_OVERLAPPEDWINDOW, FALSE, 0, new_dpi);
        } else {
            AdjustWindowRectEx(&rc2, WS_OVERLAPPEDWINDOW, FALSE, 0);
        }
        SetWindowPos(hwnd, NULL,
            proposed->left, proposed->top,
            rc2.right - rc2.left, rc2.bottom - rc2.top,
            SWP_NOZORDER | SWP_NOACTIVATE);
        return 0;
    }
    if (msg == WM_ACTIVATE && LOWORD(wp) != WA_INACTIVE) {
        SetFocus(hwnd);
        return 0;
    }
    if (msg == WM_KILLFOCUS) {
        rt_win32_ime_on_killfocus(hwnd);
        return 0;
    }
    if (msg == WM_MOUSEWHEEL && win && win->root_element) {
        rt_ui_win32_handle_scroll_wheel(hwnd, win->root_element, wp, lp);
        return 0;
    }
    if (msg == WM_SETCURSOR && win && win->root_element) {
        /* 客户区按命中元素类型切系统光标：Input/CodeEditor→I-beam、Button→Hand、
         * 其余→Arrow。坐标物理像素 → DIP 与 pointer_win32.c 同源（/dpi_scale）。 */
        if (LOWORD(lp) == HTCLIENT) {
            POINT pt;
            GetCursorPos(&pt);
            ScreenToClient(hwnd, &pt);
            RECT crc;
            GetClientRect(hwnd, &crc);
            double dip = rt_window_dpi_scale();
            if (dip < 1.0) dip = 1.0;
            RtUiElement* hit = rt_ui_hit_test(win->root_element,
                                              (int32_t)crc.right, (int32_t)crc.bottom,
                                              (int32_t)((double)pt.x / dip),
                                              (int32_t)((double)pt.y / dip));
            WORD cur_id = 32512; /* IDC_ARROW */
            if (hit && hit->type_name) {
                if (strcmp(hit->type_name, "TextBox") == 0 ||
                    strcmp(hit->type_name, "CodeEditor") == 0) {
                    cur_id = 32513; /* IDC_IBEAM */
                } else if (strcmp(hit->type_name, "Button") == 0) {
                    cur_id = 32649; /* IDC_HAND */
                }
            }
            SetCursor(LoadCursorW(NULL, MAKEINTRESOURCEW(cur_id)));
            return TRUE;
        }
    }
    if (win && win->root_element &&
        (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_MOUSEMOVE)) {
        LRESULT sb_lr = rt_ui_win32_handle_vscroll_message(hwnd, win->root_element, msg, wp, lp);
        if (sb_lr != (LRESULT)-1) {
            return sb_lr;
        }
    }
    if ((msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP || msg == WM_MOUSEMOVE || msg == WM_MOUSELEAVE)
        && win && win->root_element) {
        rt_ui_win32_handle_pointer_message(hwnd, &win->root_element,
                                           &win->pointer_down_elem,
                                           &win->pointer_over_elem,
                                           msg, (LPARAM)lp);
        return 0;
    }
    if (msg == WM_CHAR && win && win->root_element) {
        if (rt_win32_keyboard_handle_char(hwnd, wp)) {
            return 0;
        }
    }
    if (msg == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

#ifdef __cplusplus
extern "C" {
#endif

void* rt_window_create(const char* title, int32_t width, int32_t height) {
    rt_platform_init();
    static ATOM class_atom = 0;
    HINSTANCE inst = GetModuleHandleW(NULL);
    if (!class_atom) {
        WNDCLASSEXW wc = {0};
        wc.cbSize = sizeof(WNDCLASSEXW);
        wc.lpfnWndProc = rt_window_wndproc;
        wc.hInstance = inst;
        wc.lpszClassName = L"ArcWindow";
        wc.hCursor = LoadCursorW(NULL, MAKEINTRESOURCEW(32512));
        wc.hbrBackground = NULL;  /* wgpu 自己清屏，避免 GDI 闪烁 */
        class_atom = RegisterClassExW(&wc);
        if (!class_atom) {
            return NULL;
        }
    }
    RtWindowImpl* win = (RtWindowImpl*)calloc(1, sizeof(RtWindowImpl));
    if (!win) return NULL;

    int tw = MultiByteToWideChar(CP_UTF8, 0, title ? title : "Arc", -1, NULL, 0);
    wchar_t* wtitle = (wchar_t*)malloc((size_t)tw * sizeof(wchar_t));
    if (wtitle) {
        MultiByteToWideChar(CP_UTF8, 0, title ? title : "Arc", -1, wtitle, tw);
    }

    // Arc 的 Width/Height 约定为客户区 DIP 尺寸，Per-Monitor V2 下物理像素 =
    // DIP * dpi_scale。先按 DPI 缩放为客户区物理像素，再经 AdjustWindowRectEx 转外尺寸。
    double dpi_scale = rt_dpi_scale();
    int client_w = (int)((double)width * dpi_scale);
    int client_h = (int)((double)height * dpi_scale);
    RECT rc = { 0, 0, client_w, client_h };
    AdjustWindowRectEx(&rc, WS_OVERLAPPEDWINDOW, FALSE, 0);
    int win_w = rc.right - rc.left;
    int win_h = rc.bottom - rc.top;

    win->hwnd = CreateWindowExW(
        0, L"ArcWindow", wtitle ? wtitle : L"Arc",
        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
        CW_USEDEFAULT, CW_USEDEFAULT, win_w, win_h,
        NULL, NULL, inst, NULL);
    free(wtitle);
    if (!win->hwnd) {
        free(win);
        return NULL;
    }
    SetWindowLongPtrW(win->hwnd, GWLP_USERDATA, (LONG_PTR)win);
    g_rt_ui_active_win = win;
    /* 权威逻辑 DIP 尺寸（Arc 契约：CreateWindow 参数即客户区 DIP）；
     * WM_SIZE 反推同步、WM_DPICHANGED 以此为基准重算物理尺寸。 */
    win->logical_w = width;
    win->logical_h = height;
    {
        /* Per-Monitor V2 校正：GetDpiForSystem 返回系统主屏 DPI，与窗口实际所在显示器
         * 可能不一致（多屏异 DPI / 缩放变更未注销 / 创建即落在非主屏）。按 GetDpiForWindow
         * 重算客户区并一次性校正，保证 surface 配置与指针坐标与真实窗口 DPI 同源。 */
        typedef UINT (WINAPI* PFN_GetDpiForWindow)(HWND);
        HMODULE user32_dpi = GetModuleHandleW(L"user32.dll");
        PFN_GetDpiForWindow get_for_window = user32_dpi ?
            (PFN_GetDpiForWindow)GetProcAddress(user32_dpi, "GetDpiForWindow") : NULL;
        if (get_for_window) {
            UINT wdpi = get_for_window(win->hwnd);
            double wscale = wdpi > 0 ? (double)wdpi / 96.0 : 0.0;
            if (wscale >= 1.0 && wscale != dpi_scale) {
                int cw2 = (int)((double)width * wscale);
                int ch2 = (int)((double)height * wscale);
                RECT rc2 = { 0, 0, cw2, ch2 };
                AdjustWindowRectEx(&rc2, WS_OVERLAPPEDWINDOW, FALSE, 0);
                SetWindowPos(win->hwnd, NULL, 0, 0,
                             rc2.right - rc2.left, rc2.bottom - rc2.top,
                             SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }

    /* RFC 038 M3：wgpu 唯一后端——无需初始化 D2D/DWrite 资源。 */
    /* 确保窗口可接收键盘/IME（不 ImmAssociateContext(NULL) 禁用 IME）。 */
    SetFocus(win->hwnd);
    return win;
}

void rt_window_set_text(void* window, const char* text) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->text_content) {
        free(win->text_content);
        win->text_content = NULL;
    }
    if (text && text[0]) {
        int wlen = MultiByteToWideChar(CP_UTF8, 0, text, -1, NULL, 0);
        if (wlen > 0) {
            win->text_content = (wchar_t*)malloc((size_t)wlen * sizeof(wchar_t));
            if (win->text_content) {
                MultiByteToWideChar(CP_UTF8, 0, text, -1,
                                    win->text_content, wlen);
            }
        }
    }
    if (win->hwnd && win->text_content) {
        SetWindowTextW(win->hwnd, win->text_content);
    }
}

/* RFC 026 §D7.2 Win32: 返回 HWND 供 wgpu_create_surface_from_handle。
 * HWND 在 Win64 上为指针大小（8 字节），int64_t 足够承载。 */
int64_t rt_window_native_handle(void* window) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    return (int64_t)(uintptr_t)win->hwnd;
}

/* 获取窗口客户区实际像素尺寸——创建窗口后/窗口resize后调用，
 * 返回客户区宽度和高度（out参数）。 */
void rt_window_get_client_size(void* window, int32_t* out_w, int32_t* out_h) {
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (!win->hwnd) return;
    RECT rc;
    if (GetClientRect(win->hwnd, &rc)) {
        if (out_w) *out_w = rc.right - rc.left;
        if (out_h) *out_h = rc.bottom - rc.top;
    }
}

/* 窗口实际 DPI 缩放系数（Per-Monitor）。唯一权威为 GetDpiForWindow——
 * GetDpiForSystem 仅是系统主屏 DPI，不能代表本窗口（多屏异 DPI / 创建后被调整）。 */
extern "C" double rt_window_dpi_scale(void) {
    typedef UINT (WINAPI* PFN_GetDpiForWindow)(HWND);
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (user32) {
        PFN_GetDpiForWindow get_for_window =
            (PFN_GetDpiForWindow)GetProcAddress(user32, "GetDpiForWindow");
        if (get_for_window && g_rt_ui_active_win && g_rt_ui_active_win->hwnd) {
            UINT dpi = get_for_window(g_rt_ui_active_win->hwnd);
            if (dpi > 0) {
                /* [DPI-DIAG] 临时诊断：多屏混合 DPI 下 surface/命中错位定位。 */
                if (g_rt_ui_active_win && g_rt_ui_active_win->hwnd) {
                    RECT wr; GetWindowRect(g_rt_ui_active_win->hwnd, &wr);
                    RECT cr; GetClientRect(g_rt_ui_active_win->hwnd, &cr);
                    fprintf(stderr, "[DPI-DIAG] wndDpi=%u sysScale=%.2f winRect=%ldx%ld client=%ldx%ld\n",
                            dpi, rt_dpi_scale(),
                            wr.right - wr.left, wr.bottom - wr.top,
                            cr.right - cr.left, cr.bottom - cr.top);
                }
                return (double)dpi / 96.0;
            }
        }
    }
    return rt_dpi_scale();
}

/* 设置元素树根节点。wgpu 唯一后端下 root_element 仅用于命中测试/指针/
 * 键盘/滚动等逻辑面；渲染由 Arc 侧 FramePump 驱动 wgpu 完成。
 * Window 接管 root 所有权；destroy 时递归释放。 */
void rt_window_set_root_element(void* window, RtUiElement* root) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
    }
    win->root_element = root;
}

/* RFC 038 wgpu：设置 wgpu 接管渲染标志。激活后 WM_PAINT 跳过软件光栅，
 * 由 Arc 侧 FramePump 每 tick 驱动 wgpu 呈现。 */
void rt_window_set_wgpu_active(void* window, int32_t active) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    win->wgpu_active = active ? 1 : 0;
}

void rt_window_destroy(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
        win->root_element = NULL;
    }
    if (win->text_content) free(win->text_content);
    if (win->hwnd) DestroyWindow(win->hwnd);
    free(win);
}

void rt_window_close(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    /* Win32: Post WM_CLOSE → wndproc sets should_close=1, message loop exits */
    if (win->hwnd) {
        PostMessageW(win->hwnd, WM_CLOSE, 0, 0);
    } else {
        win->should_close = 1;
    }
}

int32_t rt_window_should_close(void* window) {
    if (!window) return 1;
    return ((RtWindowImpl*)window)->should_close ? 1 : 0;
}

void rt_window_invalidate(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->hwnd) {
        InvalidateRect(win->hwnd, NULL, FALSE);
    }
}

int32_t rt_event_poll(void* window) {
    if (!window) return RT_EVENT_CLOSE;
    RtWindowImpl* win = (RtWindowImpl*)window;
    g_rt_ui_active_win = win;
    MSG msg;
    while (PeekMessageW(&msg, NULL, 0, 0, PM_REMOVE)) {
        if (msg.message == WM_QUIT) {
            win->should_close = 1;
            return RT_EVENT_CLOSE;
        }
        if (msg.message == WM_CLOSE) {
            win->should_close = 1;
            return RT_EVENT_CLOSE;
        }
        if (msg.message == WM_KEYDOWN && msg.wParam == VK_ESCAPE) {
            win->should_close = 1;
            return RT_EVENT_KEY;
        }
        if (msg.message == WM_KEYDOWN && win->hwnd &&
            rt_win32_keyboard_handle_keydown(win->hwnd, msg.wParam)) {
            return RT_EVENT_KEY;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    return RT_EVENT_NONE;
}

/* A-1② 配套：空闲阻塞等待。帧泵空闲期调用——阻塞至线程收到新输入/消息
 * （QS_ALLINPUT）或 timeout_ms 超时（负值 = 无限等待），替代忙轮询空转。
 * MWMO_INPUTAVAILABLE：队列中已有未处理消息时立即返回，不丢失既有唤醒。 */
int32_t rt_event_wait(void* window, int32_t timeout_ms) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->should_close) return 0;
    g_rt_ui_active_win = win;
    DWORD timeout = (timeout_ms < 0) ? INFINITE : (DWORD)timeout_ms;
    DWORD r = MsgWaitForMultipleObjectsEx(0, NULL, timeout, QS_ALLINPUT,
                                          MWMO_INPUTAVAILABLE);
    return (r == WAIT_OBJECT_0) ? 1 : 0;
}

/* 跨线程唤醒 UI 泵：向活动窗口投递 WM_APP 空消息，使阻塞中的
 * rt_event_wait 立即返回（UIDispatcher.Post 后台入队后调用；
 * WM_APP 经 DispatchMessage 落入 DefWindowProc，无副作用）。 */
void rt_ui_wake_ui_thread(void) {
    if (g_rt_ui_active_win && g_rt_ui_active_win->hwnd) {
        PostMessageW(g_rt_ui_active_win->hwnd, WM_APP, 0, 0);
    }
}



void rt_ui_invalidate_active_window(void) {
    if (g_rt_ui_active_win && g_rt_ui_active_win->hwnd) {
        InvalidateRect(g_rt_ui_active_win->hwnd, NULL, FALSE);
    }
}

#ifdef __cplusplus
} /* extern "C" */
#endif
