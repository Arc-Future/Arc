/*
 * RFC 026 macOS 窗口 + 东亚 IME（NSTextInputClient）
 *
 * 候选窗由系统 IME 绘制；Arc 仅接收 composition / commit 回调并转发至
 * rt_ui_ime_*（026-ime-east-asian-input.md §5）。
 *
 * 构建：仅 macOS target 编译本文件（见 codegen prepare_runtime_objects）。
 * Windows / Linux CI 不编译 .mm。
 */

#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <QuartzCore/QuartzCore.h>
#import <Metal/Metal.h>

#include "../../../runtime/rt_abi.h"
#include "../../rt_ui_abi.h"
#include "../common/rt_ui_ime_internal.h"
#include "../common/rt_ui_ime_types.h"

#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* 元素树仅消费逻辑面（命中/指针/IME）；渲染由 Arc 侧 FramePump 驱动 wgpu
 * （WgpuRender）完成。跨平台软件光栅 rt_ui_render_to_buffer 已删除。 */
extern "C" void rt_ui_element_destroy(RtUiElement* elem);

@class ArcTextInputView;

/* ===== 窗口 impl（C 侧 opaque） ===== */

typedef struct RtWindowImpl {
    __unsafe_unretained NSWindow* window;
    __unsafe_unretained ArcTextInputView* view;
    int should_close;
    RtUiElement* root_element;
} RtWindowImpl;

@interface ArcTextInputView : NSView <NSTextInputClient> {
@public
    RtWindowImpl* _owner;
    NSString* _markedText;
    NSRange _markedSelectedRange;
    CAMetalLayer* _metalLayer;
}
- (instancetype)initWithFrame:(NSRect)frame owner:(RtWindowImpl*)owner;
- (void)setMetalLayer;
@end

static NSApplication* g_arc_nsapp = nil;
static RtWindowImpl* g_macos_ime_owner = NULL;

static void rt_macos_ensure_app(void) {
    if (g_arc_nsapp) return;
    g_arc_nsapp = [NSApplication sharedApplication];
    [g_arc_nsapp setActivationPolicy:NSApplicationActivationPolicyRegular];
}

@interface ArcWindowDelegate : NSObject <NSWindowDelegate>
@property (nonatomic, assign) RtWindowImpl* owner;
@end

@implementation ArcWindowDelegate

- (void)windowWillClose:(NSNotification*)notification {
    (void)notification;
    if (self.owner) self.owner->should_close = 1;
}

- (void)windowDidResignKey:(NSNotification*)notification {
    (void)notification;
    rt_ui_ime_dispatch(RT_UI_IME_FOCUS_LOST, NULL);
}

@end

@implementation ArcTextInputView

- (instancetype)initWithFrame:(NSRect)frame owner:(RtWindowImpl*)owner {
    self = [super initWithFrame:frame];
    if (self) {
        _owner = owner;
        _markedText = nil;
        _markedSelectedRange = NSMakeRange(0, 0);
    }
    return self;
}

- (BOOL)isFlipped { return YES; }
- (BOOL)acceptsFirstResponder { return YES; }
- (BOOL)canBecomeKeyView { return YES; }

- (void)drawRect:(NSRect)dirtyRect {
    (void)dirtyRect;
    /* unique-wgpu：渲染由 Arc 侧 FramePump 驱动 wgpu（Metal surface）完成，
     * 此处不再做软件光栅 blit。 */
}

- (void)setMetalLayer {
    /* unique-wgpu：为 NSView 创建 CAMetalLayer，供 wgpu 的 Metal surface 呈现。
     * （真机验证：Cocoa/wgpu 完整链路） */
    CAMetalLayer* layer = [CAMetalLayer layer];
    layer.device = MTLCreateSystemDefaultDevice();
    layer.pixelFormat = MTLPixelFormatBGRA8Unorm;
    layer.framebufferOnly = YES;
    self.wantsLayer = YES;
    self.layer = layer;
    _metalLayer = layer;
}

- (void)viewDidMoveToWindow {
    if (self.window && g_macos_ime_owner == _owner) {
        [self.window makeFirstResponder:self];
    }
}

- (void)keyDown:(NSEvent*)event {
    [self interpretKeyEvents:@[event]];
}

- (void)insertText:(id)string replacementRange:(NSRange)replacementRange {
    (void)replacementRange;
    NSString* s = [string isKindOfClass:[NSAttributedString class]]
        ? [(NSAttributedString*)string string]
        : (NSString*)string;
    if (!s.length) return;
    const char* utf8 = [s UTF8String];
    if (utf8 && utf8[0]) {
        rt_ui_ime_dispatch(RT_UI_IME_COMMIT, utf8);
    }
    [self unmarkText];
}

- (void)setMarkedText:(id)string
        selectedRange:(NSRange)selectedRange
     replacementRange:(NSRange)replacementRange {
    (void)replacementRange;
    NSString* s = [string isKindOfClass:[NSAttributedString class]]
        ? [(NSAttributedString*)string string]
        : (NSString*)string;
    _markedText = [s copy];
    _markedSelectedRange = selectedRange;

    const char* utf8 = s.length ? [s UTF8String] : "";
    RtUiImeComposition payload = {0};
    payload.text = utf8 ? utf8 : "";
    /* M-ime2 简化：cursor 暂用 UTF-16 selectedRange.location（非字节索引） */
    payload.cursor = (int32_t)selectedRange.location;
    rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_UPDATE, &payload);
}

- (void)unmarkText {
    _markedText = nil;
    _markedSelectedRange = NSMakeRange(0, 0);
    rt_ui_ime_dispatch(RT_UI_IME_COMPOSITION_END, NULL);
}

- (BOOL)hasMarkedText {
    return _markedText.length > 0;
}

- (NSRange)markedRange {
    if (!_markedText.length) return NSMakeRange(NSNotFound, 0);
    return NSMakeRange(0, _markedText.length);
}

- (NSRange)selectedRange {
    return _markedSelectedRange;
}

- (NSAttributedString*)attributedSubstringForProposedRange:(NSRange)range
                                                actualRange:(NSRangePointer)actualRange {
    (void)range;
    if (actualRange) *actualRange = NSMakeRange(0, 0);
    return nil;
}

- (NSUInteger)characterIndexForPoint:(NSPoint)point {
    (void)point;
    return 0;
}

- (NSArray<NSAttributedStringKey>*)validAttributesForMarkedText {
    return @[];
}

- (NSRect)firstRectForCharacterRange:(NSRange)range actualRange:(NSRangePointer)actualRange {
    (void)range;
    if (actualRange) *actualRange = NSMakeRange(0, 0);
    /* 标记矩形：caret 下方一条带，供系统 IME 锚定候选窗（M-ime3 可细化） */
    NSRect local = NSMakeRect(8.0, NSMaxY([self bounds]) - 32.0,
                              MAX([self bounds].size.width - 16.0, 32.0), 24.0);
    NSRect inWindow = [self convertRect:local toView:nil];
    return [[self window] convertRectToScreen:inWindow];
}

- (void)doCommandBySelector:(SEL)selector {
    if (selector == @selector(cancelOperation:)) {
        [self unmarkText];
        return;
    }
    [super doCommandBySelector:selector];
}

@end

/* ===== C ABI（macOS 窗口） ===== */

extern "C" {

void rt_macos_ime_sync_focus(RtUiElement* input) {
    (void)input;
    if (!g_macos_ime_owner || !g_macos_ime_owner->view) return;
    NSWindow* win = g_macos_ime_owner->window;
    if (win) {
        [win makeKeyAndOrderFront:nil];
        [win makeFirstResponder:g_macos_ime_owner->view];
    }
}

void rt_macos_ime_sync_candidate_rect(void) {
    if (!g_macos_ime_owner || !g_macos_ime_owner->view) return;
    [g_macos_ime_owner->view invalidateMarkedTextDisplay];
}

void* rt_window_create(const char* title, int32_t width, int32_t height) {
    rt_macos_ensure_app();
    if (width <= 0) width = 640;
    if (height <= 0) height = 480;

    RtWindowImpl* win = (RtWindowImpl*)calloc(1, sizeof(RtWindowImpl));
    if (!win) return NULL;

    NSRect frame = NSMakeRect(0, 0, (CGFloat)width, (CGFloat)height);
    NSWindowStyleMask style = NSWindowStyleMaskTitled
        | NSWindowStyleMaskClosable
        | NSWindowStyleMaskMiniaturizable
        | NSWindowStyleMaskResizable;
    NSWindow* window = [[NSWindow alloc] initWithContentRect:frame
                                                   styleMask:style
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    ArcTextInputView* view = [[ArcTextInputView alloc] initWithFrame:frame owner:win];
    [view setMetalLayer];
    [window setContentView:view];
    ArcWindowDelegate* delegate = [[ArcWindowDelegate alloc] init];
    delegate.owner = win;
    [window setDelegate:delegate];
    [window setTitle:(title && title[0])
        ? [NSString stringWithUTF8String:title]
        : @"Arc"];
    [window center];
    [window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];

    win->window = window;
    win->view = view;
    win->should_close = 0;
    win->root_element = NULL;
    g_macos_ime_owner = win;
    return win;
}

void rt_window_destroy(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (g_macos_ime_owner == win) g_macos_ime_owner = NULL;
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
        win->root_element = NULL;
    }
    if (win->window) {
        [win->window close];
        win->window = nil;
    }
    win->view = nil;
    free(win);
}

void rt_window_close(void* window) {
    if (window) ((RtWindowImpl*)window)->should_close = 1;
}

int32_t rt_window_should_close(void* window) {
    if (!window) return 1;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->should_close) return 1;
    if (win->window && ![win->window isVisible]) return 1;
    return 0;
}

/* M-focus Draft: macOS keyboard 待迁 — invalidate 供 Arc FocusManager 重绘焦点框。 */
void rt_window_invalidate(void* window) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->window) {
        [win->window displayIfNeeded];
    }
}

int32_t rt_event_poll(void* window) {
    if (!window) return RT_EVENT_CLOSE;
    RtWindowImpl* win = (RtWindowImpl*)window;
    rt_macos_ensure_app();

    NSEvent* event = [NSApp nextEventMatchingMask:NSEventMaskAny
                                          untilDate:[NSDate distantPast]
                                             inMode:NSDefaultRunLoopMode
                                            dequeue:YES];
    if (event) {
        if (event.type == NSEventTypeKeyDown) {
            unichar c = [[event charactersIgnoringModifiers] characterAtIndex:0];
            if (c == 27) {
                win->should_close = 1;
                return RT_EVENT_KEY;
            }
        }
        [NSApp sendEvent:event];
    }

    if (win->window && [win->window isMiniaturized]) {
        usleep(16000);
        return RT_EVENT_NONE;
    }

    if (win->view && win->root_element) {
        [win->view setNeedsDisplay:YES];
    }
    return RT_EVENT_NONE;
}

/* A-1② 配套：空闲阻塞等待——peek 事件队列直至有事件或超时（dequeue:NO
 * 不消费事件，后续 rt_event_poll 的 dequeue:YES 正常取出）。
 * timeout_ms 负值 = 无限等待；返回 1 = 有事件，0 = 超时。 */
int32_t rt_event_wait(void* window, int32_t timeout_ms) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->should_close) return 0;
    rt_macos_ensure_app();
    NSDate* deadline = (timeout_ms < 0)
        ? [NSDate distantFuture]
        : [NSDate dateWithTimeIntervalSinceNow:(double)timeout_ms / 1000.0];
    NSEvent* event = [NSApp nextEventMatchingMask:NSEventMaskAny
                                          untilDate:deadline
                                             inMode:NSDefaultRunLoopMode
                                            dequeue:NO];
    return (event != nil) ? 1 : 0;
}

/* 跨线程唤醒：无独立投递通道（AppKit 事件投递须主线程）；后台 Post 路径
 * 尚未接入（登记后置），当前为 no-op——macOS 侧帧泵以有限超时兜底轮询。 */
void rt_ui_wake_ui_thread(void) {
}

void rt_window_set_text(void* window, const char* text) {
    (void)window;
    (void)text;
}

int64_t rt_window_native_handle(void* window) {
    if (!window) return 0;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (!win->view || !win->view->_metalLayer) return 0;
    return (int64_t)(intptr_t)(__bridge void*)win->view->_metalLayer;
}

/* unique-wgpu：设置 wgpu 接管渲染标志。macOS 渲染由 Arc 侧 FramePump 驱动
 * wgpu（Metal surface）完成；root_element 仅消费逻辑面。 */
void rt_window_set_wgpu_active(void* window, int32_t active) {
    (void)window; (void)active;
}

/* unique-wgpu：系统 DPI 缩放系数（NSScreen 缩放）。 */
double rt_window_dpi_scale(void) {
    if (g_arc_nsapp) {
        NSWindow* win = [g_arc_nsapp keyWindow];
        if (win) {
            double s = [win backingScaleFactor];
            if (s >= 1.0) return s;
        }
    }
    return 1.0;
}

void rt_window_get_client_size(void* window, int32_t* out_w, int32_t* out_h) {
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (!win->window) return;
    NSRect frame = [win->window contentLayoutRect];
    if (out_w) *out_w = (int32_t)frame.size.width;
    if (out_h) *out_h = (int32_t)frame.size.height;
}

void rt_window_set_root_element(void* window, RtUiElement* root) {
    if (!window) return;
    RtWindowImpl* win = (RtWindowImpl*)window;
    if (win->root_element) {
        rt_ui_element_destroy(win->root_element);
    }
    win->root_element = root;
    if (win->view) {
        [win->view setNeedsDisplay:YES];
    }
}

} /* extern "C" */
