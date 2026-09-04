// RFC 037 §D7.2 + RFC 016 .ani 契约：wgpu-native C API thin shim 实现。
//
// 本文件为 `crates/arc/native/wgpu-native.ani` 契约的真实 C 实现，调用 vendored
// wgpu-native C API（https://github.com/gfx-rs/wgpu-native v29.0.1.1）。
//
// 设计原则（对齐 Arc 高性能章程 D7.2）：
//   1. 简化高层签名——避免在 .ani 暴露 WGPUInstanceDescriptor 等 struct
//   2. 平台原生窗口 handle 转换在 C 侧封装（HWND → WGPUSurfaceDescriptor）
//   3. 零分配热路径——Instance/Adapter/Device/Queue 在 Initialize 一次性创建
//   4. 类型标签 wrapper——让通用 wgpu_release(handle) 能按类型 dispatch
//
// 构建配置：
//   - 头文件搜索路径：`-I<crates/runtime-ui/wgpu-native/include>`（mod.rs prepare_runtime_objects 注入）
//   - 链接库：`-lwgpu_native`（link.rs 移除 skip，mod.rs 注入 -L<crates/runtime-ui/wgpu-native/bin/<os>>）
//   - 运行时 DLL：Windows 需 `wgpu_native.dll` 与可执行文件同目录
//     （mod.rs compile_via_llvm_ir 链接后自动复制 DLL 到输出目录）

#include "../runtime/rt_abi.h"

// wgpu-native C API（vendored at crates/runtime-ui/wgpu-native/include/wgpu.h）
#include <wgpu.h>

// RFC 037 M2 内置 8x16 ASCII 点阵字体（同目录 vendored，公共领域 IBM VGA 位图）
#include <wgpu_font8x16.h>

#include <stdbool.h>  // C99 true/false（rt_wgpu_native.c 使用 bool 字面量）
#include <stdio.h>    // fprintf/stderr（wgpu uncaptured error 回调上报）
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#endif

// ============================================================
// 类型标签 wrapper——让通用 wgpu_release(handle) 能按类型 dispatch
//
// .ani 契约规定所有 wgpu 句柄在 Arc 侧统一为 NativePtr（C void*），
// 而 wgpu-native C API 对每种句柄类型有独立的 Release 函数
//（wgpuInstanceRelease / wgpuSurfaceRelease / ...）。本 wrapper 在每个
// 句柄前加 4 字节 tag，使 wgpu_release 能 dispatch 到正确的 Release 函数。
//
// aux 字段：对 TEXTURE_VIEW 类型，存储关联的 WGPUTexture（来自
// wgpuSurfaceGetCurrentTexture 的 ReturnedWithOwnership texture），
// 在 release TextureView 时一并释放 texture。
// ============================================================
typedef enum {
    WGPU_T_NONE = 0,
    WGPU_T_INSTANCE,
    WGPU_T_SURFACE,
    WGPU_T_ADAPTER,
    WGPU_T_DEVICE,
    WGPU_T_QUEUE,
    WGPU_T_SHADER,
    WGPU_T_PIPELINE,
    WGPU_T_ENCODER,
    WGPU_T_PASS,
    WGPU_T_TEXTURE_VIEW,
    WGPU_T_COMMAND_BUFFER,
    WGPU_T_BUFFER,             // RFC 037 M3.5: WGPUBuffer
    WGPU_T_BIND_GROUP_LAYOUT,  // RFC 037 M3.5: WGPUBindGroupLayout
    WGPU_T_BIND_GROUP,          // RFC 037 M3.5: WGPUBindGroup
    WGPU_T_TEXTURE,             // RFC 037 M1: WGPUTexture
    WGPU_T_SAMPLER,             // RFC 037 M1: WGPUSampler
    WGPU_T_OFFSCREEN,           // RFC 037 §10 AL-P0: wgpu_offscreen_t 离屏渲染目标
} wgpu_tag_t;

/* RFC 037 §10 AL-P0：wgpu_offscreen_destroy 定义于文件尾部（wgpu_release 引用前向声明）。 */
void wgpu_offscreen_destroy(void* offscreen_wrap);

typedef struct {
    wgpu_tag_t tag;
    void* handle;  // 实际 WGPUInstance/Surface/etc 指针
    void* aux;     // TEXTURE_VIEW: 关联的 WGPUTexture；其他类型为 NULL
} wgpu_wrap_t;

static void* wgpu_wrap_new(wgpu_tag_t tag, void* handle) {
    if (!handle) return NULL;
    wgpu_wrap_t* w = (wgpu_wrap_t*)malloc(sizeof(wgpu_wrap_t));
    if (!w) return NULL;
    w->tag = tag;
    w->handle = handle;
    w->aux = NULL;
    return w;
}

// ============================================================
// 进程级 Instance 单例——供 adapter/device 同步请求的 event polling 使用。
//
// wgpuInstanceRequestAdapter / wgpuAdapterRequestDevice 是异步的（v29 API
// 移除了 Sync 变体）。我们用 WGPUCallbackMode_AllowProcessEvents + 循环
// wgpuInstanceProcessEvents 实现同步语义。
// ============================================================
static WGPUInstance g_instance = NULL;

// ============================================================
// Instance / Surface / Adapter / Device —— 初始化链
// ============================================================
void* wgpu_create_instance(void* descriptor) {
    (void)descriptor;
    WGPUInstanceDescriptor desc = WGPU_INSTANCE_DESCRIPTOR_INIT;
    /* Backend 选择：ARC_WGPU_BACKEND=dx12|vulkan|gl 显式指定（部署调优逃生口）。
     * Windows 默认 DX12——宿主内存开销显著低于 Vulkan ICD（实测 Intel iGPU：
     * DX12 ≈269MB vs Vulkan ≈416MB 私有提交；Vulkan 枚举路径还会加载 GL 栈），
     * 且与系统合成器集成更优。非 Windows 默认全后端枚举。 */
    const char* backend_env = getenv("ARC_WGPU_BACKEND");
    WGPUInstanceBackend mask = 0;
    if (backend_env && backend_env[0]) {
        if (strcmp(backend_env, "dx12") == 0) {
            mask = WGPUInstanceBackend_DX12;
        } else if (strcmp(backend_env, "vulkan") == 0) {
            mask = WGPUInstanceBackend_Vulkan;
        } else if (strcmp(backend_env, "gl") == 0) {
            mask = WGPUInstanceBackend_GL;
        }
    }
#ifdef _WIN32
    if (!mask) {
        mask = WGPUInstanceBackend_DX12;
    }
#endif
    if (mask) {
        static WGPUInstanceExtras extras;
        memset(&extras, 0, sizeof(extras));
        extras.chain.sType = WGPUSType_InstanceExtras;
        extras.chain.next = NULL;
        extras.backends = mask;
        desc.nextInChain = (WGPUChainedStruct*)&extras;
    }
    WGPUInstance inst = wgpuCreateInstance(&desc);
    fprintf(stderr, "[wgpu_diag] wgpu_create_instance: inst=%p backend=%s\n",
            (void*)inst, backend_env ? backend_env : "default");
    if (!inst) return NULL;
    g_instance = inst;  /* 记录单例供 adapter/device 同步等待 */
    return wgpu_wrap_new(WGPU_T_INSTANCE, inst);
}

/* wgpu_create_surface_from_handle 定义于本函数之后（前向声明）。 */
void* wgpu_create_surface_from_handle(void* instance_wrap, void* native_window_handle);

/// long 句柄版（RFC 037 §10 AL-P0 · 规避编译器 CD-29 long→NativePtr cast 缺陷）：
/// handle 为 long（HWND 整数形态），C 层内部转指针后复用同一 surface 创建逻辑。
void* wgpu_create_surface_from_hwnd(void* instance_wrap, int64_t native_window_handle) {
    return wgpu_create_surface_from_handle(instance_wrap, (void*)(uintptr_t)native_window_handle);
}

void* wgpu_create_surface_from_handle(void* instance_wrap, void* native_window_handle) {
    fprintf(stderr, "[wgpu_diag] wgpu_create_surface_from_handle: inst_wrap=%p hwnd=%p\n",
            instance_wrap, native_window_handle);
    if (!instance_wrap || !native_window_handle) {
        fprintf(stderr, "[wgpu_diag] surface: NULL arg, fail\n");
        return NULL;
    }
    WGPUInstance inst = (WGPUInstance)((wgpu_wrap_t*)instance_wrap)->handle;

#ifdef _WIN32
    // Windows: native_window_handle 是 HWND；hinstance 用当前模块
    HWND hwnd = (HWND)native_window_handle;
    HINSTANCE hinst = GetModuleHandleW(NULL);

    WGPUSurfaceSourceWindowsHWND src = WGPU_SURFACE_SOURCE_WINDOWS_HWND_INIT;
    src.hinstance = (void*)hinst;
    src.hwnd = (void*)hwnd;

    WGPUSurfaceDescriptor desc = WGPU_SURFACE_DESCRIPTOR_INIT;
    desc.nextInChain = (WGPUChainedStruct*)&src;

    WGPUSurface surface = wgpuInstanceCreateSurface(inst, &desc);
    return wgpu_wrap_new(WGPU_T_SURFACE, surface);
#elif defined(__linux__)
    // Linux: native_window_handle 是 X11 Window；Display* 经 rt_x11_display_get()
    //（linux/window.cpp 进程级全局）获取，构造 WGPUSurfaceSourceXlibWindow。
    // 真机验证：X11/wgpu（Vulkan）完整链路。
    extern void* rt_x11_display_get(void);
    void* display = rt_x11_display_get();
    if (!display) {
        fprintf(stderr, "[wgpu_diag] surface: X11 display NULL, fail\n");
        return NULL;
    }
    WGPUSurfaceSourceXlibWindow src = WGPU_SURFACE_SOURCE_XLIB_WINDOW_INIT;
    src.display = display;
    src.window = (uint64_t)(uintptr_t)native_window_handle;

    WGPUSurfaceDescriptor desc = WGPU_SURFACE_DESCRIPTOR_INIT;
    desc.nextInChain = (WGPUChainedStruct*)&src;

    WGPUSurface surface = wgpuInstanceCreateSurface(inst, &desc);
    return wgpu_wrap_new(WGPU_T_SURFACE, surface);
#elif defined(__APPLE__)
    // macOS: native_window_handle 是 CAMetalLayer*（由 macos/window.mm 的
    // ArcTextInputView setMetalLayer 建立并返回），构造 WGPUSurfaceSourceMetalLayer。
    // 真机验证：Cocoa/wgpu（Metal）完整链路。
    if (!native_window_handle) {
        fprintf(stderr, "[wgpu_diag] surface: metal layer NULL, fail\n");
        return NULL;
    }
    WGPUSurfaceSourceMetalLayer src = WGPU_SURFACE_SOURCE_METAL_LAYER_INIT;
    src.layer = native_window_handle;

    WGPUSurfaceDescriptor desc = WGPU_SURFACE_DESCRIPTOR_INIT;
    desc.nextInChain = (WGPUChainedStruct*)&src;

    WGPUSurface surface = wgpuInstanceCreateSurface(inst, &desc);
    return wgpu_wrap_new(WGPU_T_SURFACE, surface);
#else
    (void)inst;
    return NULL;
#endif
}

// Adapter 同步请求——回调上下文
typedef struct {
    volatile int done;
    WGPUAdapter adapter;
    WGPURequestAdapterStatus status;
} adapter_req_ctx;

static void adapter_req_cb(WGPURequestAdapterStatus status, WGPUAdapter adapter,
                            WGPUStringView message, void* u1, void* u2) {
    (void)message; (void)u2;
    fprintf(stderr, "[wgpu_diag] adapter_req_cb fired status=%d\n", (int)status);
    adapter_req_ctx* ctx = (adapter_req_ctx*)u1;
    ctx->status = status;
    ctx->adapter = (status == WGPURequestAdapterStatus_Success) ? adapter : NULL;
    ctx->done = 1;
}

void* wgpu_request_adapter_sync(void* instance_wrap, void* surface_wrap, int power_preference) {
    if (!instance_wrap) return NULL;
    WGPUInstance inst = (WGPUInstance)((wgpu_wrap_t*)instance_wrap)->handle;

    WGPURequestAdapterOptions opts = WGPU_REQUEST_ADAPTER_OPTIONS_INIT;
    opts.compatibleSurface = surface_wrap
        ? (WGPUSurface)((wgpu_wrap_t*)surface_wrap)->handle
        : NULL;
    opts.powerPreference = (power_preference == 0)
        ? WGPUPowerPreference_LowPower
        : WGPUPowerPreference_HighPerformance;

    adapter_req_ctx ctx = {0};
    WGPURequestAdapterCallbackInfo cb = {0};
    cb.mode = WGPUCallbackMode_AllowProcessEvents;
    cb.callback = adapter_req_cb;
    cb.userdata1 = &ctx;

    fprintf(stderr, "[wgpu_diag] request_adapter_sync: calling RequestAdapter\n");
    wgpuInstanceRequestAdapter(inst, &opts, cb);
    long long spin = 0;
    while (!ctx.done) {
        wgpuInstanceProcessEvents(inst);
        spin++;
        if (spin % 2000000 == 0) {
            fprintf(stderr, "[wgpu_diag] request_adapter_sync: spinning spin=%lld\n", spin);
        }
    }
    fprintf(stderr, "[wgpu_diag] request_adapter_sync: done status=%d\n", (int)ctx.status);

    if (ctx.status != WGPURequestAdapterStatus_Success || !ctx.adapter) {
        return NULL;
    }
    return wgpu_wrap_new(WGPU_T_ADAPTER, ctx.adapter);
}

// Device 同步请求——回调上下文
typedef struct {
    volatile int done;
    WGPUDevice device;
    WGPURequestDeviceStatus status;
} device_req_ctx;

static void device_req_cb(WGPURequestDeviceStatus status, WGPUDevice device,
                           WGPUStringView message, void* u1, void* u2) {
    (void)message; (void)u2;
    fprintf(stderr, "[wgpu_diag] device_req_cb fired status=%d\n", (int)status);
    device_req_ctx* ctx = (device_req_ctx*)u1;
    ctx->status = status;
    ctx->device = (status == WGPURequestDeviceStatus_Success) ? device : NULL;
    ctx->done = 1;
}

// wgpu uncaptured error 回调——对齐 WGPU.NET `device.SetUncapturedErrorCallback(...)`。
// 本 vendored wgpu 无独立 SetUncapturedErrorCallback 函数，回调经
// WGPUDeviceDescriptor.uncapturedErrorCallbackInfo 在设备创建时安装。
// 回调可能从任意线程触发（AllowSpontaneous），仅做 stderr 上报，不回调 Arc。
static void wgpu_uncaptured_error_cb(WGPUDevice const* device, WGPUErrorType type,
                                     WGPUStringView message, void* u1, void* u2) {
    (void)device; (void)u1; (void)u2;
    const char* type_str = "Unknown";
    switch (type) {
        case WGPUErrorType_Validation:  type_str = "Validation";  break;
        case WGPUErrorType_OutOfMemory: type_str = "OutOfMemory"; break;
        case WGPUErrorType_Internal:    type_str = "Internal";    break;
        default: break;
    }
    fprintf(stderr, "[wgpu] uncaptured %s error: %.*s\n", type_str,
            (int)message.length, message.data ? message.data : "");
}

void* wgpu_request_device_sync(void* adapter_wrap, void* descriptor) {
    if (!adapter_wrap) return NULL;
    WGPUAdapter adapter = (WGPUAdapter)((wgpu_wrap_t*)adapter_wrap)->handle;
    (void)descriptor;  // NULL 用默认配置

    WGPUDeviceDescriptor desc = WGPU_DEVICE_DESCRIPTOR_INIT;

    // item 3：adapter limits 透传 device（对齐 WGPU.NET
    // `RequestDevice(limits: supportedLimits.limits)`）。
    WGPULimits supported_limits = {0};
    if (wgpuAdapterGetLimits(adapter, &supported_limits) == WGPUStatus_Success) {
        desc.requiredLimits = &supported_limits;
    }

    // item 1：uncaptured error 回调上报 GPU 错误（对齐 WGPU.NET
    // `device.SetUncapturedErrorCallback(...)`）。
    desc.uncapturedErrorCallbackInfo.callback = wgpu_uncaptured_error_cb;

    device_req_ctx ctx = {0};
    WGPURequestDeviceCallbackInfo cb = {0};
    cb.mode = WGPUCallbackMode_AllowProcessEvents;
    cb.callback = device_req_cb;
    cb.userdata1 = &ctx;

    wgpuAdapterRequestDevice(adapter, &desc, cb);
    // 通过 instance 的 process events 循环等待回调触发
    WGPUInstance inst = g_instance;
    if (!inst) return NULL;
    fprintf(stderr, "[wgpu_diag] request_device_sync: calling RequestDevice\n");
    long long spin = 0;
    while (!ctx.done) {
        wgpuInstanceProcessEvents(inst);
        spin++;
        if (spin % 2000000 == 0) {
            fprintf(stderr, "[wgpu_diag] request_device_sync: spinning spin=%lld\n", spin);
        }
    }
    fprintf(stderr, "[wgpu_diag] request_device_sync: done status=%d\n", (int)ctx.status);

    if (ctx.status != WGPURequestDeviceStatus_Success || !ctx.device) {
        return NULL;
    }
    return wgpu_wrap_new(WGPU_T_DEVICE, ctx.device);
}

void* wgpu_device_get_queue(void* device_wrap) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUQueue queue = wgpuDeviceGetQueue(dev);
    return wgpu_wrap_new(WGPU_T_QUEUE, queue);
}

// ============================================================
// Surface 配置 + 帧循环
// ============================================================
int wgpu_surface_configure(void* surface_wrap, void* device_wrap, int format, int usage,
                           int width, int height, int present_mode) {
    if (!surface_wrap || !device_wrap) return -1;
    WGPUSurface surface = (WGPUSurface)((wgpu_wrap_t*)surface_wrap)->handle;
    WGPUDevice device = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    fprintf(stderr, "[wgpu_diag] surface_configure: fmt=%d %dx%d present=%d\n", format, width, height, present_mode);

    WGPUSurfaceConfiguration cfg = WGPU_SURFACE_CONFIGURATION_INIT;
    cfg.device = device;
    cfg.format = (WGPUTextureFormat)format;          // 0=BGRA8Unorm(0x1B), 1=RGBA8Unorm(0x16)
    cfg.usage = (WGPUTextureUsage)usage;             // 1=RenderAttachment
    cfg.width = (uint32_t)width;
    cfg.height = (uint32_t)height;
    cfg.presentMode = (WGPUPresentMode)present_mode; // 1=Fifo(VSync on)

    wgpuSurfaceConfigure(surface, &cfg);
    fprintf(stderr, "[wgpu_diag] surface_configure: wgpuSurfaceConfigure(void) called ok\n");
    return 0;
}

// Surface 首选格式——对齐 WGPU.NET `surface.GetPreferredFormat(adapter)`。
// 本 vendored wgpu 已弃用 GetPreferredFormat（v24+），改用
// wgpuSurfaceGetCapabilities(...).formats[0]（capabilities 数组按偏好排序）。
int wgpu_surface_get_preferred_format(void* surface_wrap, void* adapter_wrap) {
    if (!surface_wrap || !adapter_wrap) return -1;
    WGPUSurface surface = (WGPUSurface)((wgpu_wrap_t*)surface_wrap)->handle;
    WGPUAdapter adapter = (WGPUAdapter)((wgpu_wrap_t*)adapter_wrap)->handle;

    WGPUSurfaceCapabilities caps = WGPU_SURFACE_CAPABILITIES_INIT;
    if (wgpuSurfaceGetCapabilities(surface, adapter, &caps) != WGPUStatus_Success) {
        return -1;
    }
    int fmt = -1;
    if (caps.formatCount > 0 && caps.formats) {
        fmt = (int)caps.formats[0];
    }
    /* wgpuSurfaceGetCapabilities 返回的首选格式是 sRGB 变体（BGRA8UnormSrgb=28/RGBA8UnormSrgb=23）；
     * 必须使用 sRGB surface——GPU 在 sRGB 帧缓冲上自动执行线性空间混合 + 输出 gamma 编码，
     * 这是抗锯齿边缘平滑、字体半透明像素正确显示的前提。强制 non-sRGB 会导致所有 AA 边缘发黑锯齿。 */
    fprintf(stderr, "[wgpu_diag] caps: formatCount=%u fmt0=%d (0x%X) presentCount=%u alphaCount=%u\n",
            caps.formatCount, fmt, (unsigned)fmt,
            caps.presentModeCount, caps.alphaModeCount);
    for (uint32_t i = 0; i < caps.presentModeCount && i < 8; i++) {
        fprintf(stderr, "[wgpu_diag] caps.presentMode[%u]=%d\n", i, (int)caps.presentModes[i]);
    }
    wgpuSurfaceCapabilitiesFreeMembers(caps);
    return fmt;
}

int wgpu_surface_get_current_texture(void* surface_wrap, void** texture_view) {
    if (!surface_wrap || !texture_view) return -1;
    *texture_view = NULL;
    WGPUSurface surface = (WGPUSurface)((wgpu_wrap_t*)surface_wrap)->handle;

    WGPUSurfaceTexture surf_tex = WGPU_SURFACE_TEXTURE_INIT;
    wgpuSurfaceGetCurrentTexture(surface, &surf_tex);
    // wgpu-native v29 拆分 Success 为 SuccessOptimal / SuccessSuboptimal；
    // 两者都表示可渲染——后者仅提示需要重新 configure。
    if ((surf_tex.status != WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal
         && surf_tex.status != WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal)
        || !surf_tex.texture) {
        // 释放可能存在的 texture（错误状态下仍可能返回 non-null）
        if (surf_tex.texture) wgpuTextureRelease(surf_tex.texture);
        return (int)surf_tex.status;
    }

    WGPUTextureView view = wgpuTextureCreateView(surf_tex.texture, NULL);
    if (!view) {
        wgpuTextureRelease(surf_tex.texture);
        return -1;
    }

    wgpu_wrap_t* w = (wgpu_wrap_t*)malloc(sizeof(wgpu_wrap_t));
    if (!w) {
        wgpuTextureViewRelease(view);
        wgpuTextureRelease(surf_tex.texture);
        return -1;
    }
    w->tag = WGPU_T_TEXTURE_VIEW;
    w->handle = view;
    w->aux = surf_tex.texture;  // 关联 texture，release 时一并释放
    *texture_view = w;
    return 0;
}

void wgpu_surface_present(void* surface_wrap) {
    if (!surface_wrap) return;
    WGPUSurface surface = (WGPUSurface)((wgpu_wrap_t*)surface_wrap)->handle;
    wgpuSurfacePresent(surface);
}

// ============================================================
// Command 编码
// ============================================================
void* wgpu_command_encoder_create(void* device_wrap) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUCommandEncoder enc = wgpuDeviceCreateCommandEncoder(dev, NULL);
    return wgpu_wrap_new(WGPU_T_ENCODER, enc);
}

void* wgpu_render_pass_begin(void* encoder_wrap, void* texture_view_wrap, int clear,
                             double clear_r, double clear_g,
                             double clear_b, double clear_a) {
    if (!encoder_wrap || !texture_view_wrap) return NULL;
    WGPUCommandEncoder enc = (WGPUCommandEncoder)((wgpu_wrap_t*)encoder_wrap)->handle;
    WGPUTextureView view = (WGPUTextureView)((wgpu_wrap_t*)texture_view_wrap)->handle;

    WGPURenderPassColorAttachment color_att = WGPU_RENDER_PASS_COLOR_ATTACHMENT_INIT;
    color_att.view = view;
    color_att.loadOp = clear ? WGPULoadOp_Clear : WGPULoadOp_Load;
    color_att.storeOp = WGPUStoreOp_Store;
    color_att.clearValue = (WGPUColor){ clear_r, clear_g, clear_b, clear_a };

    WGPURenderPassDescriptor desc = WGPU_RENDER_PASS_DESCRIPTOR_INIT;
    desc.colorAttachmentCount = 1;
    desc.colorAttachments = &color_att;

    WGPURenderPassEncoder pass = wgpuCommandEncoderBeginRenderPass(enc, &desc);
    return wgpu_wrap_new(WGPU_T_PASS, pass);
}

void wgpu_render_pass_set_pipeline(void* pass_wrap, void* pipeline_wrap) {
    if (!pass_wrap || !pipeline_wrap) return;
    WGPURenderPassEncoder pass = (WGPURenderPassEncoder)((wgpu_wrap_t*)pass_wrap)->handle;
    WGPURenderPipeline pipeline = (WGPURenderPipeline)((wgpu_wrap_t*)pipeline_wrap)->handle;
    wgpuRenderPassEncoderSetPipeline(pass, pipeline);
}

void wgpu_render_pass_draw(void* pass_wrap, int vertex_count, int instance_count,
                           int first_vertex, int first_instance) {
    if (!pass_wrap) return;
    WGPURenderPassEncoder pass = (WGPURenderPassEncoder)((wgpu_wrap_t*)pass_wrap)->handle;
    wgpuRenderPassEncoderDraw(pass,
        (uint32_t)vertex_count, (uint32_t)instance_count,
        (uint32_t)first_vertex, (uint32_t)first_instance);
}

void wgpu_render_pass_end(void* pass_wrap) {
    if (!pass_wrap) return;
    WGPURenderPassEncoder pass = (WGPURenderPassEncoder)((wgpu_wrap_t*)pass_wrap)->handle;
    wgpuRenderPassEncoderEnd(pass);
}

void* wgpu_command_encoder_finish(void* encoder_wrap) {
    if (!encoder_wrap) return NULL;
    WGPUCommandEncoder enc = (WGPUCommandEncoder)((wgpu_wrap_t*)encoder_wrap)->handle;
    WGPUCommandBuffer cmd = wgpuCommandEncoderFinish(enc, NULL);
    return wgpu_wrap_new(WGPU_T_COMMAND_BUFFER, cmd);
}

void wgpu_queue_submit_one(void* queue_wrap, void* command_buffer_wrap) {
    if (!queue_wrap || !command_buffer_wrap) return;
    WGPUQueue queue = (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle;
    WGPUCommandBuffer cmd = (WGPUCommandBuffer)((wgpu_wrap_t*)command_buffer_wrap)->handle;
    wgpuQueueSubmit(queue, 1, &cmd);
}

// ============================================================
// 资源创建
// ============================================================
void* wgpu_shader_module_create_wgsl(void* device_wrap, const char* source) {
    if (!device_wrap || !source) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUShaderSourceWGSL wgsl = WGPU_SHADER_SOURCE_WGSL_INIT;
    wgsl.code.data = source;
    wgsl.code.length = strlen(source);

    WGPUShaderModuleDescriptor desc = WGPU_SHADER_MODULE_DESCRIPTOR_INIT;
    desc.nextInChain = (WGPUChainedStruct*)&wgsl;

    WGPUShaderModule shader = wgpuDeviceCreateShaderModule(dev, &desc);
    return wgpu_wrap_new(WGPU_T_SHADER, shader);
}

void* wgpu_render_pipeline_create_basic(void* device_wrap, void* shader_wrap, int format) {
    if (!device_wrap || !shader_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUShaderModule shader = (WGPUShaderModule)((wgpu_wrap_t*)shader_wrap)->handle;

    // Vertex state: vs_main entry，无 vertex buffers（用 @builtin(vertex_index)）
    WGPUVertexState vertex = WGPU_VERTEX_STATE_INIT;
    vertex.module = shader;
    vertex.entryPoint = (WGPUStringView){ "vs_main", 7 };

    // Fragment state: fs_main entry，单 color target = surface format
    WGPUBlendState blend_state = WGPU_BLEND_STATE_INIT;
    blend_state.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    blend_state.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend_state.color.operation = WGPUBlendOperation_Add;
    blend_state.alpha.srcFactor = WGPUBlendFactor_One;
    blend_state.alpha.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend_state.alpha.operation = WGPUBlendOperation_Add;

    WGPUColorTargetState color_target = WGPU_COLOR_TARGET_STATE_INIT;
    color_target.format = (WGPUTextureFormat)format;
    color_target.blend = &blend_state;

    WGPUFragmentState fragment = {0};
    fragment.module = shader;
    fragment.entryPoint = (WGPUStringView){ "fs_main", 7 };
    fragment.targetCount = 1;
    fragment.targets = &color_target;

    WGPURenderPipelineDescriptor desc = WGPU_RENDER_PIPELINE_DESCRIPTOR_INIT;
    desc.vertex = vertex;
    desc.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    desc.fragment = &fragment;

    WGPURenderPipeline pipeline = wgpuDeviceCreateRenderPipeline(dev, &desc);
    return wgpu_wrap_new(WGPU_T_PIPELINE, pipeline);
}

// ============================================================
// RFC 037 M3.5 矩形绘制资源 ABI
// ============================================================

void* wgpu_buffer_create(void* device_wrap, int size, int usage) {
    if (!device_wrap || size <= 0) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUBufferDescriptor desc = WGPU_BUFFER_DESCRIPTOR_INIT;
    desc.size = (uint64_t)size;
    desc.usage = (WGPUBufferUsage)usage;
    desc.mappedAtCreation = false;

    WGPUBuffer buffer = wgpuDeviceCreateBuffer(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BUFFER, buffer);
}

void* wgpu_uniform_bind_group_layout_create(void* device_wrap, int binding, int stage) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUBindGroupLayoutEntry entry = WGPU_BIND_GROUP_LAYOUT_ENTRY_INIT;
    entry.binding = (uint32_t)binding;
    entry.visibility = (WGPUShaderStage)stage;
    entry.buffer.type = WGPUBufferBindingType_Uniform;
    entry.buffer.hasDynamicOffset = true;  // 动态偏移——单 buffer 池多 DrawRect
    entry.buffer.minBindingSize = 80;      // 表面填充 uniform = 20 floats = 80 字节（圆角+渐变统一）

    WGPUBindGroupLayoutDescriptor desc = WGPU_BIND_GROUP_LAYOUT_DESCRIPTOR_INIT;
    desc.entryCount = 1;
    desc.entries = &entry;

    WGPUBindGroupLayout layout = wgpuDeviceCreateBindGroupLayout(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP_LAYOUT, layout);
}

void* wgpu_uniform_bind_group_create(void* device_wrap, void* layout_wrap, void* buffer_wrap) {
    if (!device_wrap || !layout_wrap || !buffer_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUBindGroupLayout layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)layout_wrap)->handle;
    WGPUBuffer buffer = (WGPUBuffer)((wgpu_wrap_t*)buffer_wrap)->handle;

    WGPUBindGroupEntry entry = WGPU_BIND_GROUP_ENTRY_INIT;
    entry.binding = 0;
    entry.buffer = buffer;
    entry.offset = 0;
    entry.size = 80;  // 表面填充 uniform 大小——动态偏移会调整实际读取位置

    WGPUBindGroupDescriptor desc = WGPU_BIND_GROUP_DESCRIPTOR_INIT;
    desc.layout = layout;
    desc.entryCount = 1;
    desc.entries = &entry;

    WGPUBindGroup group = wgpuDeviceCreateBindGroup(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP, group);
}

void* wgpu_render_pipeline_create_rect(void* device_wrap, void* shader_wrap, int format, void* bg_layout_wrap) {
    if (!device_wrap || !shader_wrap || !bg_layout_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUShaderModule shader = (WGPUShaderModule)((wgpu_wrap_t*)shader_wrap)->handle;
    WGPUBindGroupLayout bg_layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)bg_layout_wrap)->handle;

    WGPUPipelineLayoutDescriptor pl_desc = WGPU_PIPELINE_LAYOUT_DESCRIPTOR_INIT;
    pl_desc.bindGroupLayoutCount = 1;
    pl_desc.bindGroupLayouts = &bg_layout;
    WGPUPipelineLayout pipeline_layout = wgpuDeviceCreatePipelineLayout(dev, &pl_desc);

    WGPUVertexState vertex = WGPU_VERTEX_STATE_INIT;
    vertex.module = shader;
    vertex.entryPoint = (WGPUStringView){ "vs_main", 7 };

    WGPUBlendState rect_blend = WGPU_BLEND_STATE_INIT;
    rect_blend.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    rect_blend.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    rect_blend.color.operation = WGPUBlendOperation_Add;
    rect_blend.alpha.srcFactor = WGPUBlendFactor_One;
    rect_blend.alpha.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    rect_blend.alpha.operation = WGPUBlendOperation_Add;

    WGPUColorTargetState color_target = WGPU_COLOR_TARGET_STATE_INIT;
    color_target.format = (WGPUTextureFormat)format;
    color_target.blend = &rect_blend;

    WGPUFragmentState fragment = {0};
    fragment.module = shader;
    fragment.entryPoint = (WGPUStringView){ "fs_main", 7 };
    fragment.targetCount = 1;
    fragment.targets = &color_target;

    WGPURenderPipelineDescriptor desc = WGPU_RENDER_PIPELINE_DESCRIPTOR_INIT;
    desc.layout = pipeline_layout;
    desc.vertex = vertex;
    desc.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    desc.fragment = &fragment;

    WGPURenderPipeline pipeline = wgpuDeviceCreateRenderPipeline(dev, &desc);
    // pipeline_layout 引用计数 +1（pipeline 持有），这里 release 释放本侧引用
    wgpuPipelineLayoutRelease(pipeline_layout);
    return wgpu_wrap_new(WGPU_T_PIPELINE, pipeline);
}

// ============================================================
// 帧内绘制 ABI
// ============================================================

// 阴影 bind group layout（minBindingSize=64——shadow uniform 64 字节）。
void* wgpu_shadow_bind_group_layout_create(void* device_wrap, int binding, int stage) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUBindGroupLayoutEntry entry = WGPU_BIND_GROUP_LAYOUT_ENTRY_INIT;
    entry.binding = (uint32_t)binding;
    entry.visibility = (WGPUShaderStage)stage;
    entry.buffer.type = WGPUBufferBindingType_Uniform;
    entry.buffer.hasDynamicOffset = true;  // 单 buffer 池多 DrawShadow
    entry.buffer.minBindingSize = 64;

    WGPUBindGroupLayoutDescriptor desc = WGPU_BIND_GROUP_LAYOUT_DESCRIPTOR_INIT;
    desc.entryCount = 1;
    desc.entries = &entry;

    WGPUBindGroupLayout layout = wgpuDeviceCreateBindGroupLayout(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP_LAYOUT, layout);
}

void* wgpu_shadow_bind_group_create(void* device_wrap, void* layout_wrap, void* buffer_wrap) {
    if (!device_wrap || !layout_wrap || !buffer_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUBindGroupLayout layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)layout_wrap)->handle;
    WGPUBuffer buffer = (WGPUBuffer)((wgpu_wrap_t*)buffer_wrap)->handle;

    WGPUBindGroupEntry entry = WGPU_BIND_GROUP_ENTRY_INIT;
    entry.binding = 0;
    entry.buffer = buffer;
    entry.offset = 0;
    entry.size = 64;

    WGPUBindGroupDescriptor desc = WGPU_BIND_GROUP_DESCRIPTOR_INIT;
    desc.layout = layout;
    desc.entryCount = 1;
    desc.entries = &entry;

    WGPUBindGroup group = wgpuDeviceCreateBindGroup(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP, group);
}

void* wgpu_render_pipeline_create_shadow(void* device_wrap, void* shader_wrap, int format, void* bg_layout_wrap) {
    if (!device_wrap || !shader_wrap || !bg_layout_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUShaderModule shader = (WGPUShaderModule)((wgpu_wrap_t*)shader_wrap)->handle;
    WGPUBindGroupLayout bg_layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)bg_layout_wrap)->handle;

    WGPUPipelineLayoutDescriptor pl_desc = WGPU_PIPELINE_LAYOUT_DESCRIPTOR_INIT;
    pl_desc.bindGroupLayoutCount = 1;
    pl_desc.bindGroupLayouts = &bg_layout;
    WGPUPipelineLayout pipeline_layout = wgpuDeviceCreatePipelineLayout(dev, &pl_desc);

    WGPUVertexState vertex = WGPU_VERTEX_STATE_INIT;
    vertex.module = shader;
    vertex.entryPoint = (WGPUStringView){ "vs_main", 7 };

    WGPUBlendState blend = WGPU_BLEND_STATE_INIT;
    blend.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    blend.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend.color.operation = WGPUBlendOperation_Add;
    blend.alpha.srcFactor = WGPUBlendFactor_One;
    blend.alpha.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    blend.alpha.operation = WGPUBlendOperation_Add;

    WGPUColorTargetState color_target = WGPU_COLOR_TARGET_STATE_INIT;
    color_target.format = (WGPUTextureFormat)format;
    color_target.blend = &blend;

    WGPUFragmentState fragment = {0};
    fragment.module = shader;
    fragment.entryPoint = (WGPUStringView){ "fs_main", 7 };
    fragment.targetCount = 1;
    fragment.targets = &color_target;

    WGPURenderPipelineDescriptor desc = WGPU_RENDER_PIPELINE_DESCRIPTOR_INIT;
    desc.layout = pipeline_layout;
    desc.vertex = vertex;
    desc.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    desc.fragment = &fragment;

    WGPURenderPipeline pipeline = wgpuDeviceCreateRenderPipeline(dev, &desc);
    wgpuPipelineLayoutRelease(pipeline_layout);
    return wgpu_wrap_new(WGPU_T_PIPELINE, pipeline);
}

// ============================================================
// 帧内绘制 ABI
// ============================================================

// 表面填充 uniform（80 字节）——圆角 SDF + 纯色/两停靠点线性渐变统一原语。
//   offset  0: x, y, w, h
//   offset 16: c0r, c0g, c0b, c0a   起点色 / 纯色
//   offset 32: c1r, c1g, c1b, c1a   终点色（纯色时 = c0）
//   offset 48: sx, sy, ex, ey       渐变轴（归一化；长度≈0 → 纯色）
//   offset 64: surface_w, surface_h, radius, stroke
typedef struct {
    float x, y, w, h;
    float c0r, c0g, c0b, c0a;
    float c1r, c1g, c1b, c1a;
    float sx, sy, ex, ey;
    float surface_w, surface_h, radius, stroke;
} wgpu_rect_uniform_t;

void wgpu_render_pass_set_bind_group(void* pass_wrap, int group_index,
                                       void* group_wrap, int dynamic_offset) {
    if (!pass_wrap || !group_wrap) return;
    WGPURenderPassEncoder pass = (WGPURenderPassEncoder)((wgpu_wrap_t*)pass_wrap)->handle;
    WGPUBindGroup group = (WGPUBindGroup)((wgpu_wrap_t*)group_wrap)->handle;

    uint32_t dynamic_offsets[1] = { (uint32_t)dynamic_offset };
    wgpuRenderPassEncoderSetBindGroup(pass, (uint32_t)group_index, group,
                                       1, dynamic_offsets);
}

/* RFC 037 P0 真实裁剪：scissor rect。后续所有 draw 仅绘制落在矩形内的像素，
 * 超出部分被硬件裁掉——ScrollView/ListView 内容溢出视口时不再外溢。
 * wgpu-native v29 的 SetScissorRect 参数为 (x, y, width, height)，全 u32。 */
void wgpu_render_pass_set_scissor(void* pass_wrap, int x, int y, int w, int h) {
    if (!pass_wrap || w < 0 || h < 0) return;
    if (x < 0) x = 0;
    if (y < 0) y = 0;
    WGPURenderPassEncoder pass = (WGPURenderPassEncoder)((wgpu_wrap_t*)pass_wrap)->handle;
    wgpuRenderPassEncoderSetScissorRect(pass, (uint32_t)x, (uint32_t)y,
                                        (uint32_t)w, (uint32_t)h);
}

// ============================================================
// RFC 037 M1: wgpu ABI 扩展 shim（texture / sampler / 通用队列写入）
//
// 数据缓冲（像素/顶点）在 Arc 侧以 NativePtr 持有，C 侧作为 void* 透传，
// 零拷贝直喂 wgpu WGPUQueueWriteBuffer / WGPUQueueWriteTexture。
// ============================================================

void wgpu_queue_write_buffer(void* queue_wrap, void* buffer_wrap, int offset,
                             void* data, int size) {
    if (!queue_wrap || !buffer_wrap || !data || size <= 0) return;
    WGPUQueue queue = (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle;
    WGPUBuffer buffer = (WGPUBuffer)((wgpu_wrap_t*)buffer_wrap)->handle;
    wgpuQueueWriteBuffer(queue, buffer, (uint64_t)offset, data, (size_t)size);
}

void* wgpu_texture_create_2d(void* device_wrap, int width, int height,
                             int format, int usage) {
    if (!device_wrap || width <= 0 || height <= 0) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUTextureDescriptor desc = WGPU_TEXTURE_DESCRIPTOR_INIT;
    desc.dimension = WGPUTextureDimension_2D;
    desc.size = (WGPUExtent3D){ (uint32_t)width, (uint32_t)height, 1 };
    desc.format = (WGPUTextureFormat)format;
    desc.usage = (WGPUTextureUsage)usage;
    desc.mipLevelCount = 1;
    desc.sampleCount = 1;

    WGPUTexture tex = wgpuDeviceCreateTexture(dev, &desc);
    return wgpu_wrap_new(WGPU_T_TEXTURE, tex);
}

void* wgpu_texture_create_view(void* texture_wrap) {
    if (!texture_wrap) return NULL;
    WGPUTexture tex = (WGPUTexture)((wgpu_wrap_t*)texture_wrap)->handle;
    WGPUTextureView view = wgpuTextureCreateView(tex, NULL);
    // 视图由调用方独立持有纹理；aux 留空，release 视图时不释放纹理。
    return wgpu_wrap_new(WGPU_T_TEXTURE_VIEW, view);
}

void wgpu_texture_write(void* queue_wrap, void* texture_wrap, int width, int height,
                        void* data, int size) {
    if (!queue_wrap || !texture_wrap || !data || size <= 0) return;
    WGPUQueue queue = (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle;
    WGPUTexture tex = (WGPUTexture)((wgpu_wrap_t*)texture_wrap)->handle;

    WGPUTexelCopyBufferLayout layout = WGPU_TEXEL_COPY_BUFFER_LAYOUT_INIT;
    layout.offset = 0;
    layout.bytesPerRow = (uint32_t)width * 4;  // RGBA8: 4 bytes/pixel
    layout.rowsPerImage = (uint32_t)height;

    WGPUTexelCopyTextureInfo source = WGPU_TEXEL_COPY_TEXTURE_INFO_INIT;
    source.texture = tex;
    source.mipLevel = 0;
    source.origin = (WGPUOrigin3D){ 0, 0, 0 };
    source.aspect = WGPUTextureAspect_All;

    WGPUExtent3D copy_size = { (uint32_t)width, (uint32_t)height, 1 };
    wgpuQueueWriteTexture(queue, &source, data, (size_t)size, &layout, &copy_size);
}

void* wgpu_sampler_create(void* device_wrap, int filter) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUFilterMode mode = (filter == 1) ? WGPUFilterMode_Linear : WGPUFilterMode_Nearest;
    WGPUSamplerDescriptor desc = WGPU_SAMPLER_DESCRIPTOR_INIT;
    desc.magFilter = mode;
    desc.minFilter = mode;
    desc.mipmapFilter = (filter == 1) ? WGPUMipmapFilterMode_Linear : WGPUMipmapFilterMode_Nearest;

    WGPUSampler sampler = wgpuDeviceCreateSampler(dev, &desc);
    return wgpu_wrap_new(WGPU_T_SAMPLER, sampler);
}

// ============================================================
// RFC 037 M2: 内置 8x16 点阵字体 + glyph atlas + 文本纹理采样管线
//
// DrawText 从「估算尺寸 + 背景框占位」升级为真实字形上屏：
//   - 字体数据：wgpu_font8x16.h（公共领域 IBM VGA 8x16，95 可打印 ASCII 字形）
//   - 光栅路径：wgpu_font8x16_rasterize_ascii（字符串 → RGBA8 缓冲，可无 GPU 测试）
//   - atlas：wgpu_font8x16_build_atlas（95 字形 → 128x96 RGBA atlas，一次上传）
//   - 采样管线：wgpu_text_bind_group_layout_create / wgpu_text_pipeline_create /
//     wgpu_text_bind_group_create（uniform@0 动态偏移 + texture@1 + sampler@2）
//   - 帧内：wgpu_batch_text_write 写入 64 字节文本 uniform（quad rect +
//     atlas UV + tint + surface 尺寸）到 CPU staging，复用 256 字节槽位 uniform 池。
// ============================================================

void* wgpu_font8x16_rasterize_ascii(const char* text, int* out_w, int* out_h) {
    if (!text) {
        if (out_w) *out_w = 0;
        if (out_h) *out_h = 0;
        return NULL;
    }
    size_t len = strlen(text);
    if (len > 1024) len = 1024;  // 防御：超长截断（M2 基线字形用）
    const int w = (int)len * WGPU_FONT8X16_W;
    const int h = WGPU_FONT8X16_H;
    uint8_t* px = (uint8_t*)malloc((size_t)w * (size_t)h * 4);
    if (!px) {
        if (out_w) *out_w = 0;
        if (out_h) *out_h = 0;
        return NULL;
    }
    memset(px, 0, (size_t)w * (size_t)h * 4);
    for (size_t i = 0; i < len; i++) {
        int cp = (unsigned char)text[i];
        int g = cp - WGPU_FONT8X16_FIRST;
        if (g < 0 || g >= WGPU_FONT8X16_GLYPH_COUNT) g = 0;  // 超出范围按空格
        for (int row = 0; row < WGPU_FONT8X16_H; row++) {
            uint8_t bits = wgpu_font8x16_bits[g][row];
            for (int b = 0; b < WGPU_FONT8X16_W; b++) {
                if (bits & (0x80u >> b)) {
                    int x = (int)i * WGPU_FONT8X16_W + b;
                    int idx = (row * w + x) * 4;
                    px[idx + 0] = 0xFF;  // R
                    px[idx + 1] = 0xFF;  // G
                    px[idx + 2] = 0xFF;  // B
                    px[idx + 3] = 0xFF;  // A（前景不透明白，背景透明）
                }
            }
        }
    }
    if (out_w) *out_w = w;
    if (out_h) *out_h = h;
    return px;
}

void* wgpu_font8x16_build_atlas(int* out_w, int* out_h) {
    const int cols = 16;  // 每行 16 字形
    const int rows = 6;   // ceil(95/16) = 6 行
    const int w = cols * WGPU_FONT8X16_W;
    const int h = rows * WGPU_FONT8X16_H;
    uint8_t* px = (uint8_t*)malloc((size_t)w * (size_t)h * 4);
    if (!px) {
        if (out_w) *out_w = 0;
        if (out_h) *out_h = 0;
        return NULL;
    }
    memset(px, 0, (size_t)w * (size_t)h * 4);
    for (int g = 0; g < WGPU_FONT8X16_GLYPH_COUNT; g++) {
        int gx = (g % cols) * WGPU_FONT8X16_W;
        int gy = (g / cols) * WGPU_FONT8X16_H;
        for (int row = 0; row < WGPU_FONT8X16_H; row++) {
            uint8_t bits = wgpu_font8x16_bits[g][row];
            for (int b = 0; b < WGPU_FONT8X16_W; b++) {
                if (bits & (0x80u >> b)) {
                    int idx = ((gy + row) * w + (gx + b)) * 4;
                    px[idx + 0] = 0xFF;
                    px[idx + 1] = 0xFF;
                    px[idx + 2] = 0xFF;
                    px[idx + 3] = 0xFF;
                }
            }
        }
    }
    /* Tofu glyph (index 95, slot col=15 row=5): 空心方框 □，用于非 ASCII 缺失字形提示。
       8x16 单元内画 1px 边框（距边 1px），中央镂空。 */
    {
        int tgx = (95 % cols) * WGPU_FONT8X16_W;
        int tgy = (95 / cols) * WGPU_FONT8X16_H;
        for (int row = 0; row < WGPU_FONT8X16_H; row++) {
            for (int b = 0; b < WGPU_FONT8X16_W; b++) {
                int is_border = (b == 1 || b == WGPU_FONT8X16_W - 2 ||
                                 row == 1 || row == WGPU_FONT8X16_H - 2);
                if (is_border) {
                    int idx = ((tgy + row) * w + (tgx + b)) * 4;
                    px[idx + 0] = 0xCC;
                    px[idx + 1] = 0xCC;
                    px[idx + 2] = 0xCC;
                    px[idx + 3] = 0xFF;
                }
            }
        }
    }
    if (out_w) *out_w = w;
    if (out_h) *out_h = h;
    return px;
}

void wgpu_font_buffer_free(void* buf) {
    if (buf) free(buf);
}

void* wgpu_text_bind_group_layout_create(void* device_wrap) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;

    WGPUBindGroupLayoutEntry entries[3];

    WGPUBindGroupLayoutEntry uniform_entry = WGPU_BIND_GROUP_LAYOUT_ENTRY_INIT;
    uniform_entry.binding = 0;
    uniform_entry.visibility = WGPUShaderStage_Vertex | WGPUShaderStage_Fragment;
    uniform_entry.buffer.type = WGPUBufferBindingType_Uniform;
    uniform_entry.buffer.hasDynamicOffset = true;  // 共享 uniform 池（256 槽位）
    uniform_entry.buffer.minBindingSize = 64;       // 文本 uniform = 16 floats = 64 字节
    entries[0] = uniform_entry;

    WGPUBindGroupLayoutEntry tex_entry = WGPU_BIND_GROUP_LAYOUT_ENTRY_INIT;
    tex_entry.binding = 1;
    tex_entry.visibility = WGPUShaderStage_Fragment;
    tex_entry.texture.sampleType = WGPUTextureSampleType_Float;
    tex_entry.texture.viewDimension = WGPUTextureViewDimension_2D;
    entries[1] = tex_entry;

    WGPUBindGroupLayoutEntry smp_entry = WGPU_BIND_GROUP_LAYOUT_ENTRY_INIT;
    smp_entry.binding = 2;
    smp_entry.visibility = WGPUShaderStage_Fragment;
    smp_entry.sampler.type = WGPUSamplerBindingType_Filtering;
    entries[2] = smp_entry;

    WGPUBindGroupLayoutDescriptor desc = WGPU_BIND_GROUP_LAYOUT_DESCRIPTOR_INIT;
    desc.entryCount = 3;
    desc.entries = entries;

    WGPUBindGroupLayout layout = wgpuDeviceCreateBindGroupLayout(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP_LAYOUT, layout);
}

void* wgpu_text_pipeline_create(void* device_wrap, void* shader_wrap, int format, void* bg_layout_wrap) {
    if (!device_wrap || !shader_wrap || !bg_layout_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUShaderModule shader = (WGPUShaderModule)((wgpu_wrap_t*)shader_wrap)->handle;
    WGPUBindGroupLayout bg_layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)bg_layout_wrap)->handle;

    WGPUPipelineLayoutDescriptor pl_desc = WGPU_PIPELINE_LAYOUT_DESCRIPTOR_INIT;
    pl_desc.bindGroupLayoutCount = 1;
    pl_desc.bindGroupLayouts = &bg_layout;
    WGPUPipelineLayout pipeline_layout = wgpuDeviceCreatePipelineLayout(dev, &pl_desc);

    WGPUVertexState vertex = WGPU_VERTEX_STATE_INIT;
    vertex.module = shader;
    vertex.entryPoint = (WGPUStringView){ "vs_main", 7 };

    WGPUBlendState text_blend = WGPU_BLEND_STATE_INIT;
    text_blend.color.srcFactor = WGPUBlendFactor_SrcAlpha;
    text_blend.color.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    text_blend.color.operation = WGPUBlendOperation_Add;
    text_blend.alpha.srcFactor = WGPUBlendFactor_One;
    text_blend.alpha.dstFactor = WGPUBlendFactor_OneMinusSrcAlpha;
    text_blend.alpha.operation = WGPUBlendOperation_Add;

    WGPUColorTargetState color_target = WGPU_COLOR_TARGET_STATE_INIT;
    color_target.format = (WGPUTextureFormat)format;
    color_target.blend = &text_blend;

    WGPUFragmentState fragment = {0};
    fragment.module = shader;
    fragment.entryPoint = (WGPUStringView){ "fs_main", 7 };
    fragment.targetCount = 1;
    fragment.targets = &color_target;

    WGPURenderPipelineDescriptor desc = WGPU_RENDER_PIPELINE_DESCRIPTOR_INIT;
    desc.layout = pipeline_layout;
    desc.vertex = vertex;
    desc.primitive.topology = WGPUPrimitiveTopology_TriangleList;
    desc.fragment = &fragment;

    WGPURenderPipeline pipeline = wgpuDeviceCreateRenderPipeline(dev, &desc);
    wgpuPipelineLayoutRelease(pipeline_layout);
    return wgpu_wrap_new(WGPU_T_PIPELINE, pipeline);
}

void* wgpu_text_bind_group_create(void* device_wrap, void* layout_wrap, void* uniform_buffer_wrap,
                                   void* texture_view_wrap, void* sampler_wrap) {
    if (!device_wrap || !layout_wrap || !uniform_buffer_wrap || !texture_view_wrap || !sampler_wrap) {
        return NULL;
    }
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUBindGroupLayout layout = (WGPUBindGroupLayout)((wgpu_wrap_t*)layout_wrap)->handle;
    WGPUBuffer buffer = (WGPUBuffer)((wgpu_wrap_t*)uniform_buffer_wrap)->handle;
    WGPUTextureView view = (WGPUTextureView)((wgpu_wrap_t*)texture_view_wrap)->handle;
    WGPUSampler sampler = (WGPUSampler)((wgpu_wrap_t*)sampler_wrap)->handle;

    WGPUBindGroupEntry entries[3];

    WGPUBindGroupEntry buf_entry = WGPU_BIND_GROUP_ENTRY_INIT;
    buf_entry.binding = 0;
    buf_entry.buffer = buffer;
    buf_entry.offset = 0;
    buf_entry.size = 64;  // 文本 uniform 大小——动态偏移调整实际读取位置
    entries[0] = buf_entry;

    WGPUBindGroupEntry tex_entry = WGPU_BIND_GROUP_ENTRY_INIT;
    tex_entry.binding = 1;
    tex_entry.textureView = view;
    entries[1] = tex_entry;

    WGPUBindGroupEntry smp_entry = WGPU_BIND_GROUP_ENTRY_INIT;
    smp_entry.binding = 2;
    smp_entry.sampler = sampler;
    entries[2] = smp_entry;

    WGPUBindGroupDescriptor desc = WGPU_BIND_GROUP_DESCRIPTOR_INIT;
    desc.layout = layout;
    desc.entryCount = 3;
    desc.entries = entries;

    WGPUBindGroup group = wgpuDeviceCreateBindGroup(dev, &desc);
    return wgpu_wrap_new(WGPU_T_BIND_GROUP, group);
}

// 文本 uniform 数据布局（64 字节，无 padding——所有字段 f32 自然对齐）：
//   offset 0:  x, y, w, h (f32 × 4) = 16 bytes
//   offset 16: u0, v0, u1, v1 (f32 × 4) = 16 bytes
//   offset 32: r, g, b, a (f32 × 4) = 16 bytes
//   offset 48: surface_w, surface_h (f32 × 2) + padding (f32 × 2) = 16 bytes
typedef struct {
    float x, y, w, h;
    float u0, v0, u1, v1;
    float r, g, b, a;
    float surface_w, surface_h, _pad0, _pad1;
} wgpu_text_uniform_t;

// ============================================================
// RFC 042 P3 阶段1：CPU staging 批上传
//
// DrawRect/DrawText 将 uniform 写入 C 侧连续 staging 缓冲（纯 memcpy，
// 无逐绘制 GPU 往返），帧末一次 wgpu_queue_write_buffer 整片上传 + 重放 draw。
// 复用既有 wgpu_rect_uniform_t / wgpu_text_uniform_t 布局（与 shader 契约一致）。
// ============================================================

void* wgpu_batch_staging_create(int max_bytes) {
    if (max_bytes <= 0) return NULL;
    return malloc((size_t)max_bytes);
}

void wgpu_batch_rect_write(void* staging, int offset,
                           double x, double y, double w, double h,
                           double c0r, double c0g, double c0b, double c0a,
                           double c1r, double c1g, double c1b, double c1a,
                           double sx, double sy, double ex, double ey,
                           double radius, double stroke,
                           int surface_w, int surface_h) {
    if (!staging || offset < 0) return;
    wgpu_rect_uniform_t u;
    u.x = (float)x; u.y = (float)y; u.w = (float)w; u.h = (float)h;
    u.c0r = (float)c0r; u.c0g = (float)c0g; u.c0b = (float)c0b; u.c0a = (float)c0a;
    u.c1r = (float)c1r; u.c1g = (float)c1g; u.c1b = (float)c1b; u.c1a = (float)c1a;
    u.sx = (float)sx; u.sy = (float)sy; u.ex = (float)ex; u.ey = (float)ey;
    u.surface_w = (float)surface_w; u.surface_h = (float)surface_h;
    u.radius = (float)radius; u.stroke = (float)stroke;
    memcpy((char*)staging + offset, &u, sizeof(u));
}

// 阴影 uniform 数据布局（64 字节——独立 shadow bind group layout/pipeline）。
//   offset  0: x, y, w, h       阴影 quad 边界（像素）
//   offset 16: cx, cy, cw, ch   核心矩形（表面）边界（像素）
//   offset 32: radius, blur, a, surface_w
//   offset 48: surface_h, _pad0, _pad1, _pad2
typedef struct {
    float x, y, w, h;
    float cx, cy, cw, ch;
    float radius, blur, a, surface_w;
    float surface_h, _pad0, _pad1, _pad2;
} wgpu_shadow_uniform_t;

void wgpu_batch_shadow_write(void* staging, int offset,
                             double x, double y, double w, double h,
                             double cx, double cy, double cw, double ch,
                             double radius, double blur, double a,
                             int surface_w, int surface_h) {
    if (!staging || offset < 0) return;
    wgpu_shadow_uniform_t u;
    u.x = (float)x; u.y = (float)y; u.w = (float)w; u.h = (float)h;
    u.cx = (float)cx; u.cy = (float)cy; u.cw = (float)cw; u.ch = (float)ch;
    u.radius = (float)radius; u.blur = (float)blur; u.a = (float)a;
    u.surface_w = (float)surface_w; u.surface_h = (float)surface_h;
    u._pad0 = 0.0f; u._pad1 = 0.0f; u._pad2 = 0.0f;
    memcpy((char*)staging + offset, &u, sizeof(u));
}

void wgpu_batch_text_write(void* staging, int offset,
                           double x, double y, double w, double h,
                           double u0, double v0, double u1, double v1,
                           double r, double g, double b, double a,
                           int surface_w, int surface_h) {
    if (!staging || offset < 0) return;
    wgpu_text_uniform_t u;
    u.x = (float)x; u.y = (float)y; u.w = (float)w; u.h = (float)h;
    u.u0 = (float)u0; u.v0 = (float)v0; u.u1 = (float)u1; u.v1 = (float)v1;
    u.r = (float)r; u.g = (float)g; u.b = (float)b; u.a = (float)a;
    u.surface_w = (float)surface_w; u.surface_h = (float)surface_h;
    u._pad0 = 0.0f; u._pad1 = 0.0f;
    memcpy((char*)staging + offset, &u, sizeof(u));
}

void wgpu_batch_staging_destroy(void* staging) {
    free(staging);
}

// ============================================================
// RFC 037 M3: 跨平台动态字形 atlas（stb_truetype 高清光栅）
//
// 设计：
//   - 启动时加载系统默认 UI 字体（Windows: msyh.ttc 微软雅黑；macOS/Linux: 见
//     wgpu_font_find_system_font 平台路径列表），经 stb_truetype 解析。
//   - 预分配 2048×2048 RGBA8 atlas 纹理（16MB，够装 ~3500+ 个 32px 字形，
//     覆盖一屏 UI 中全部可见 Latin+CJK 字符）。
//   - 首次遇到 (codepoint) 时经 stbtt_GetCodepointBitmap 光栅到 alpha mask，
//     转 RGBA（白色 glyph on 透明，R=G=B=A=alpha），shelf-packing 入 atlas
//     CPU 侧像素缓冲，记录 UV/metrics；帧开始时一次性 wgpuQueueWriteTexture
//     写入脏 sub-rect（增量上传）。
//   - DrawText 经 wgpu_font_atlas_lookup_glyph 取得 UV+metrics，quad 绘制
//     复用已有 text pipeline（shader 不变）。
//
// 与 8x16 点阵的关系：8x16 保留为内嵌 fallback（字形光栅失败 / atlas 满 /
// 字体加载失败时），默认路径使用 stb_truetype 高清字形（RFC 016 §8.1 R3）。
// ============================================================

// ---- rt_image_font_* 前向声明（来自 runtime-drawing/rt_font.c，同为链接对象） ----
void* rt_image_font_load(const uint8_t* ttf, size_t len, float size);
int32_t rt_image_font_metrics(void* font, float* ascent, float* descent, float* line_gap);
float rt_image_font_measure(void* font, const char* text);
int32_t rt_image_font_glyph(void* font, uint32_t codepoint, uint8_t* bitmap_out,
                            int32_t* w, int32_t* h, float* xoff, float* yoff);
int32_t rt_image_font_glyph_full(void* font, uint32_t codepoint, uint8_t* bitmap_out,
                                  int32_t* w, int32_t* h, float* xoff, float* yoff,
                                  float* advance);
int32_t rt_image_font_glyph_full_px(void* font, uint32_t codepoint, double pixel_height,
                                     uint8_t* bitmap_out,
                                     int32_t* w, int32_t* h, float* xoff, float* yoff,
                                     float* advance);
int32_t rt_image_font_has_glyph(void* font, uint32_t codepoint);
const void* rt_image_font_get_stbtt_info(void* font);
float rt_image_font_get_scale(void* font);
void rt_image_font_free(void* font);

#define ATLAS_W 2048
#define ATLAS_H 2048
#define ATLAS_PAD 1
#define GLYPH_CACHE_CAP 8192
#define ATLAS_BASE_PX 32.0f    /* stb_truetype 光栅基准像素高度；shader quad 按 DPI/FontSize 缩放 */
#define MAX_FONTS 16           /* 回退链+字重面槽（Normal 链 + 可选 Bold 主面） */
#define MAX_FAMILIES 8         /* 命名字体族最大数（FontFamily 选择 API） */
#define FONT_WEIGHT_NORMAL 0   /* atlas 字重：Normal 面 */
#define FONT_WEIGHT_BOLD   1   /* atlas 字重：Bold 面（无 Bold 槽时回退 Normal） */

/* 单个回退字体条目 */
typedef struct {
    void* font;           /* rt_image_font_load 返回的 RtFont* */
    uint8_t* ttf_data;    /* malloc 的 TTF 文件内容（stbtt 需要保持生命周期） */
    float ascent, descent, line_gap;
    char label[64];       /* 调试用：字体来源路径简名 */
} FontSlot;

typedef struct {
    uint32_t codepoint;
    int valid;           /* 1 = 已光栅 */
    int placed;          /* 1 = 已上 GPU（dirty 清零） */
    int font_idx;        /* 光栅该字形所用的 FontSlot 索引 */
    int family_idx;      /* 请求该字形的 family（缓存 key 维度） */
    int weight;          /* FONT_WEIGHT_NORMAL / BOLD（缓存 key：同字不同字重字形可不同） */
    int size_px;         /* 光栅物理像素高度（缓存 key：per-size bucket，1:1 采样零缩放） */
    uint16_t x, y;       /* atlas 内像素原点 */
    uint16_t gw, gh;     /* glyph 像素尺寸 */
    float xoff, yoff;    /* 字形左/上 bearing（相对 baseline 左上为正） */
    float advance;       /* 水平 advance（像素） */
} GlyphEntry;

/* 命名字体族：fonts[] 池内的一段回退链切片。
 * 对标 egui FontFamily 列表 / cosmic-text FontFamily 概念。
 * Bold：可选主面槽（RegisterFamily normal+bold / 系统 *bd 变体）；缺失则字重请求回退 Normal。 */
typedef struct {
    char name[64];       /* family 名，如 "Segoe UI" / "SimSun" / "KaiTi" */
    int slot_start;      /* Normal 回退链在 fonts[] 中的起点 */
    int slot_count;      /* Normal 回退链长度（跨主/Symbol/Emoji 分类） */
    int bold_slot;       /* Bold 主面在 fonts[] 中的索引；-1 = 未注册 Bold 面 */
    float ascent, descent, line_gap;  /* 该 family 主字体度量（slot_start 处） */
} FontFamily;

typedef struct {
    /* 多字体回退链（对标 egui FontFamily 列表 / cosmic-text FontFallbackIter）。
     * fonts[] 为字体槽池，families[] 为命名族注册表（切片引用 fonts[]）。 */
    FontSlot fonts[MAX_FONTS];
    int font_count;
    float ascent, descent, line_gap;  /* 主字体度量（fonts[0] = family 0 主字体） */
    FontFamily families[MAX_FAMILIES];
    int family_count;                 /* families[0] 恒为默认族（"Segoe UI"） */

    /* atlas 纹理 */
    void* device;        /* wgpu_wrap_t* WGPU_DEVICE（只作透传，不所有） */
    void* queue;         /* wgpu_wrap_t* WGPU_QUEUE（即时上传字形用，不所有） */
    void* texture;       /* wgpu_wrap_t* WGPU_TEXTURE */
    void* texture_view;  /* wgpu_wrap_t* WGPU_TEXTURE_VIEW */
    uint8_t* pixels;     /* malloc 2048*2048*4 RGBA，CPU 侧 atlas 缓冲 */

    /* shelf packing 状态 */
    int shelf_y;
    int shelf_x;
    int shelf_h;

    /* glyph 缓存（open addressing by codepoint） */
    GlyphEntry* cache;
    int cache_cap;
    int cache_count;

    /* dirty 矩形（待上传到 GPU） */
    int dirty_x0, dirty_y0, dirty_x1, dirty_y1;
    int has_dirty;

    /* 字体加载失败回退：0 = 已加载，1 = 回退到 8x16 */
    int fallback;

    /* stb_truetype 光栅基准像素高度（create 传入；add_family 复用） */
    float base_size_px;
} WgpuFontAtlas;

/*
 * 平台默认字体族回退链（对标 egui families 列表 / cosmic-text FontFallbackIter）
 * 格式：'|' 分隔分类（0=主 CJK+Latin, 1=Symbol, 2=Emoji）；
 *       ';' 分隔同分类候选路径，首个可用者胜。
 * 逐字符用 stbtt_FindGlyphIndex 查找，第一个含字形的字体负责光栅。
 * 这是 family 0（默认族）的启动链；自定义 family 经 wgpu_font_atlas_add_family 注册。
 */
static const char* wgpu_font_default_candidates(void) {
#ifdef _WIN32
    return "C:\\Windows\\Fonts\\msyh.ttc;C:\\Windows\\Fonts\\msyh.ttf;"
           "C:\\Windows\\Fonts\\simsun.ttc;C:\\Windows\\Fonts\\simhei.ttf;"
           "C:\\Windows\\Fonts\\segoeui.ttf"
           "|C:\\Windows\\Fonts\\seguisym.ttf"
           "|C:\\Windows\\Fonts\\seguiemj.ttf";
#elif defined(__APPLE__)
    return "/System/Library/Fonts/PingFang.ttc;/System/Library/Fonts/STHeiti Medium.ttc;"
           "/Library/Fonts/Arial Unicode.ttf;/System/Library/Fonts/Helvetica.ttc"
           "|/System/Library/Fonts/Apple Symbols.ttf"
           "|/System/Library/Fonts/Apple Color Emoji.ttf";
#else
    return "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc;"
           "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc;"
           "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc;"
           "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
           "|/usr/share/fonts/truetype/noto/NotoSansSymbols.ttf;"
           "/usr/share/fonts/truetype/noto/NotoSansSymbols2.ttf"
           "|/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf";
#endif
}

/* 默认族 Bold 主面候选（仅 primary；无 '|' 分类）。与 Normal 链同源平台优先。 */
static const char* wgpu_font_default_bold_candidates(void) {
#ifdef _WIN32
    return "C:\\Windows\\Fonts\\msyhbd.ttc;C:\\Windows\\Fonts\\segoeuib.ttf;"
           "C:\\Windows\\Fonts\\simhei.ttf";
#elif defined(__APPLE__)
    return "/System/Library/Fonts/PingFang.ttc;/System/Library/Fonts/STHeiti Medium.ttc;"
           "/System/Library/Fonts/Helvetica.ttc";
#else
    return "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc;"
           "/usr/share/fonts/truetype/noto/NotoSansCJK-Bold.ttc;"
           "/usr/share/fonts/noto-cjk/NotoSansCJK-Bold.ttc;"
           "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf";
#endif
}

/* 前向声明：read_file 定义于本文件后部，load_chain 先于其使用（C99 禁隐式声明）。 */
static uint8_t* wgpu_font_read_file(const char* path, size_t* out_len);

/* 加载一条 '|'/'|' 分隔的字体回退链到 fonts[] 池。
 * 分类间 '|' 分隔；分类内候选 ';' 分隔，首个可用者胜（对标 egui 每 family 每分类取一字体）。
 * base_size：stb_truetype 光栅基准像素高度。
 * 返回本链实际加载的字体数；0 表示全失败。 */
static int wgpu_font_atlas_load_chain(WgpuFontAtlas* a, const char* chain, float base_size) {
    if (!a || !chain) return 0;
    int loaded = 0;
    const char* cat = chain;
    int category = 0;
    while (cat && a->font_count < MAX_FONTS && category < 3) {
        const char* cat_end = strchr(cat, '|');
        size_t cat_len = cat_end ? (size_t)(cat_end - cat) : strlen(cat);
        const char* path = cat;
        const char* seg_end = cat + cat_len;
        while (path < seg_end && a->font_count < MAX_FONTS) {
            const char* semi = strchr(path, ';');
            const char* end = semi && semi < seg_end ? semi : seg_end;
            size_t len = (size_t)(end - path);
            if (len > 0) {
                char tmp[512];
                size_t cl = len < sizeof(tmp) - 1 ? len : sizeof(tmp) - 1;
                memcpy(tmp, path, cl);
                tmp[cl] = '\0';
                char* t = tmp;
                while (*t == ' ' || *t == '\t') t++;  /* 首部去空白 */
                if (*t) {
                    fprintf(stderr, "[wgpu_font] trying[%s]: %s\n",
                            category == 0 ? "primary" : (category == 1 ? "symbol" : "emoji"), t);
                    size_t ttf_len = 0;
                    uint8_t* ttf_data = wgpu_font_read_file(t, &ttf_len);
                    if (ttf_data && ttf_len > 0) {
                        void* font = rt_image_font_load(ttf_data, ttf_len, base_size);
                        if (font) {
                            FontSlot* slot = &a->fonts[a->font_count];
                            slot->font = font;
                            slot->ttf_data = ttf_data;
                            rt_image_font_metrics(font, &slot->ascent, &slot->descent, &slot->line_gap);
                            const char* slash = strrchr(t, '/');
                            const char* bslash = strrchr(t, '\\');
                            const char* base = slash > bslash ? slash : bslash;
                            base = base ? base + 1 : t;
                            strncpy(slot->label, base, sizeof(slot->label) - 1);
                            slot->label[sizeof(slot->label) - 1] = '\0';
                            fprintf(stderr, "[wgpu_font] loaded[%d] (%s): %s (%zu bytes)\n",
                                    a->font_count,
                                    category == 0 ? "primary" : (category == 1 ? "symbol" : "emoji"),
                                    slot->label, ttf_len);
                            a->font_count++;
                            loaded++;
                            break;  /* 该分类取首个可用字体 */
                        }
                        free(ttf_data);
                    }
                }
            }
            if (!semi || semi >= seg_end) break;
            path = semi + 1;
        }
        category++;
        if (!cat_end) break;
        cat = cat_end + 1;
    }
    return loaded;
}

static uint8_t* wgpu_font_read_file(const char* path, size_t* out_len) {
    if (!path || !out_len) return NULL;
    *out_len = 0;
    FILE* f = fopen(path, "rb");
    if (!f) return NULL;
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return NULL; }
    long sz = ftell(f);
    if (sz < 0) { fclose(f); return NULL; }
    if (fseek(f, 0, SEEK_SET) != 0) { fclose(f); return NULL; }
    uint8_t* buf = (uint8_t*)malloc((size_t)sz + 1);
    if (!buf) { fclose(f); return NULL; }
    size_t n = fread(buf, 1, (size_t)sz, f);
    fclose(f);
    if (n != (size_t)sz) { free(buf); return NULL; }
    buf[n] = 0;
    *out_len = n;
    return buf;
}

/* 简单 hash——纳入 family + weight + size_px，避免同字符不同族/字重/字号冲突 */
static unsigned wgpu_font_hash_cp(uint32_t cp, int family_idx, int weight, int size_px) {
    unsigned h = (unsigned)(cp * 2654435761u);
    h ^= (unsigned)family_idx * 0x9E3779B1u;
    h ^= (unsigned)weight * 0x85EBCA6Bu;
    h ^= (unsigned)size_px * 0xC2B2AE35u;
    return h;
}

static GlyphEntry* wgpu_font_cache_find(GlyphEntry* cache, int cap, uint32_t cp,
                                        int family_idx, int weight, int size_px,
                                        int* out_idx) {
    unsigned h = wgpu_font_hash_cp(cp, family_idx, weight, size_px) & (unsigned)(cap - 1);
    int i;
    for (i = 0; i < cap; i++) {
        int idx = (int)((h + (unsigned)i) & (unsigned)(cap - 1));
        GlyphEntry* e = &cache[idx];
        if (!e->valid) {
            if (out_idx) *out_idx = idx;
            return NULL;
        }
        if (e->codepoint == cp && e->family_idx == family_idx &&
            e->weight == weight && e->size_px == size_px) {
            if (out_idx) *out_idx = idx;
            return e;
        }
    }
    if (out_idx) *out_idx = -1;
    return NULL;
}

static void wgpu_font_atlas_place_glyph(WgpuFontAtlas* a, GlyphEntry* e,
                                        const uint8_t* alpha, int gw, int gh) {
    /* shelf packing */
    if (a->shelf_x + gw + ATLAS_PAD > ATLAS_W) {
        a->shelf_y += a->shelf_h + ATLAS_PAD;
        a->shelf_x = 0;
        a->shelf_h = 0;
    }
    int y = a->shelf_y;
    if (y + gh + ATLAS_PAD > ATLAS_H) {
        /* atlas 满——回退到 8x16 */
        e->valid = 0;
        return;
    }
    int x = a->shelf_x;
    /* 防御（RFC 042 A-1 堆损坏排查）：单字形超出 atlas 一行宽度（极宽字形）
     * 时，不得写入越界。正常 32px 字形 <64px，此分支仅在异常数据触发。 */
    if (x + gw + ATLAS_PAD > ATLAS_W) {
        fprintf(stderr, "[wgpu_font] BOUNDS: glyph too wide for atlas: x=%d gw=%d ATLAS_W=%d\n",
                x, gw, ATLAS_W);
        e->valid = 0;
        return;
    }
    e->x = (uint16_t)x;
    e->y = (uint16_t)y;
    e->gw = (uint16_t)gw;
    e->gh = (uint16_t)gh;
    e->placed = 0;
    /* 拷贝 alpha → RGBA（白色 glyph，A=亮度） */
    for (int row = 0; row < gh; row++) {
        uint8_t* dst = a->pixels + ((y + row) * ATLAS_W + x) * 4;
        const uint8_t* src = alpha + row * gw;
        for (int col = 0; col < gw; col++) {
            uint8_t a_val = src[col];
            dst[col * 4 + 0] = a_val;
            dst[col * 4 + 1] = a_val;
            dst[col * 4 + 2] = a_val;
            dst[col * 4 + 3] = a_val;
        }
    }
    a->shelf_x += gw + ATLAS_PAD;
    if (gh > a->shelf_h) a->shelf_h = gh;
    /* 扩展 dirty rect */
    if (!a->has_dirty) {
        a->dirty_x0 = x; a->dirty_y0 = y;
        a->dirty_x1 = x + gw; a->dirty_y1 = y + gh;
        a->has_dirty = 1;
    } else {
        if (x < a->dirty_x0) a->dirty_x0 = x;
        if (y < a->dirty_y0) a->dirty_y0 = y;
        if (x + gw > a->dirty_x1) a->dirty_x1 = x + gw;
        if (y + gh > a->dirty_y1) a->dirty_y1 = y + gh;
    }
}

void* wgpu_font_atlas_create(void* device_wrap, void* queue_wrap, double base_size) {
    if (!device_wrap) return NULL;
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    WGPUQueue queue = queue_wrap ? (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle : NULL;
    (void)queue;

    WgpuFontAtlas* a = (WgpuFontAtlas*)calloc(1, sizeof(WgpuFontAtlas));
    if (!a) return NULL;
    a->device = device_wrap;
    a->queue = queue_wrap;
    a->pixels = (uint8_t*)calloc(ATLAS_W * ATLAS_H, 4);
    a->cache = (GlyphEntry*)calloc(GLYPH_CACHE_CAP, sizeof(GlyphEntry));
    a->cache_cap = GLYPH_CACHE_CAP;
    a->base_size_px = (float)base_size;
    if (!a->pixels || !a->cache) {
        free(a->pixels); free(a->cache); free(a);
        return NULL;
    }

    /* 加载默认字体族（family 0，主/Symbol/Emoji 三分类回退链） */
    wgpu_font_atlas_load_chain(a, wgpu_font_default_candidates(), (float)base_size);
    if (a->font_count > 0) {
        /* 注册 family 0（默认族 "Segoe UI"），覆盖整个字体池 */
        FontFamily* f0 = &a->families[0];
        strncpy(f0->name, "Segoe UI", sizeof(f0->name) - 1);
        f0->name[sizeof(f0->name) - 1] = '\0';
        f0->slot_start = 0;
        f0->slot_count = a->font_count;
        f0->bold_slot = -1;
        f0->ascent = a->fonts[0].ascent;
        f0->descent = a->fonts[0].descent;
        f0->line_gap = a->fonts[0].line_gap;
        a->family_count = 1;

        /* 系统 Bold 主面（msyhbd / segoeuib 等）；失败则 bold_slot 保持 -1，Bold 回退 Normal */
        int bold_start = a->font_count;
        int bn = wgpu_font_atlas_load_chain(a, wgpu_font_default_bold_candidates(), (float)base_size);
        if (bn > 0) {
            f0->bold_slot = bold_start;
            fprintf(stderr, "[wgpu_font] family[0] bold face: %s (slot %d)\n",
                    a->fonts[bold_start].label, bold_start);
        } else {
            fprintf(stderr, "[wgpu_font] family[0] no system bold face; Bold→Normal fallback\n");
        }

        a->ascent = a->fonts[0].ascent;
        a->descent = a->fonts[0].descent;
        a->line_gap = a->fonts[0].line_gap;
        fprintf(stderr, "[wgpu_font] primary metrics: ascent=%.1f descent=%.1f linegap=%.1f (%d fonts)\n",
                a->ascent, a->descent, a->line_gap, a->font_count);
    } else {
        fprintf(stderr, "[wgpu_font] WARNING: no system font, falling back to 8x16\n");
        a->fallback = 1;
    }

    /* 创建 atlas 纹理 */
    WGPUTextureDescriptor desc = WGPU_TEXTURE_DESCRIPTOR_INIT;
    desc.dimension = WGPUTextureDimension_2D;
    desc.size = (WGPUExtent3D){ ATLAS_W, ATLAS_H, 1 };
    desc.format = WGPUTextureFormat_RGBA8Unorm;
    desc.usage = WGPUTextureUsage_TextureBinding | WGPUTextureUsage_CopyDst;
    desc.mipLevelCount = 1;
    desc.sampleCount = 1;
    WGPUTexture tex = wgpuDeviceCreateTexture(dev, &desc);
    if (!tex) {
        for (int i = 0; i < a->font_count; i++) {
            rt_image_font_free(a->fonts[i].font);
            free(a->fonts[i].ttf_data);
        }
        free(a->pixels); free(a->cache); free(a);
        return NULL;
    }
    a->texture = wgpu_wrap_new(WGPU_T_TEXTURE, tex);
    WGPUTextureView view = wgpuTextureCreateView(tex, NULL);
    a->texture_view = wgpu_wrap_new(WGPU_T_TEXTURE_VIEW, view);
    if (a->texture_view) {
        /* 关联 texture 到 view，release view 时一并释放 texture */
        ((wgpu_wrap_t*)a->texture_view)->aux = a->texture;
    }

    /* 初次上传：清除纹理为全透明（像素初始为 0） */
    if (queue) {
        WGPUTexelCopyBufferLayout layout = WGPU_TEXEL_COPY_BUFFER_LAYOUT_INIT;
        layout.offset = 0;
        layout.bytesPerRow = ATLAS_W * 4;
        layout.rowsPerImage = ATLAS_H;
        WGPUTexelCopyTextureInfo dst = WGPU_TEXEL_COPY_TEXTURE_INFO_INIT;
        dst.texture = tex; dst.mipLevel = 0;
        dst.origin = (WGPUOrigin3D){0,0,0}; dst.aspect = WGPUTextureAspect_All;
        WGPUExtent3D sz = { ATLAS_W, ATLAS_H, 1 };
        wgpuQueueWriteTexture(queue, &dst, a->pixels, (size_t)(ATLAS_W*ATLAS_H*4), &layout, &sz);
    }
    return a;
}

void* wgpu_font_atlas_get_texture_view(void* atlas) {
    if (!atlas) return NULL;
    return ((WgpuFontAtlas*)atlas)->texture_view;
}

int wgpu_font_atlas_is_fallback(void* atlas) {
    if (!atlas) return 1;
    return ((WgpuFontAtlas*)atlas)->fallback;
}

float wgpu_font_atlas_get_ascent_f(void* atlas) {
    if (!atlas) return 0.0f;
    return ((WgpuFontAtlas*)atlas)->ascent;
}

float wgpu_font_atlas_get_descent_f(void* atlas) {
    if (!atlas) return 0.0f;
    return ((WgpuFontAtlas*)atlas)->descent;
}

float wgpu_font_atlas_get_line_gap_f(void* atlas) {
    if (!atlas) return 0.0f;
    return ((WgpuFontAtlas*)atlas)->line_gap;
}

/* Arc 侧 ABI（double 版本，与 .ani 声明一致） */
double wgpu_font_atlas_get_ascent(void* atlas) {
    return (double)wgpu_font_atlas_get_ascent_f(atlas);
}

double wgpu_font_atlas_get_descent(void* atlas) {
    return (double)wgpu_font_atlas_get_descent_f(atlas);
}

double wgpu_font_atlas_get_line_gap(void* atlas) {
    return (double)wgpu_font_atlas_get_line_gap_f(atlas);
}

/*
 * 注册命名字体族（FontFamily 选择 API 核心）。
 *
 * chain_str 约定（与 FontManager.ApplyToBackend 对齐）：
 *   - "normal.ttf"              → 仅 Normal 主面
 *   - "normal.ttf|bold.ttf"     → Normal + Bold（单 '|'，分类1 作 bold_slot，非 Symbol）
 *   - 三分类回退链（含两个 '|'）仍按 主|Symbol|Emoji 加载为 Normal 链
 *   - 可选 '#' 后缀："...#Bold.ttf" 显式 Bold 段（与单 '|' 二选一；'#' 优先）
 *
 * 返回 family 索引（>=1）；失败返回 -1。family 0 由 create 预注册。
 */
int wgpu_font_atlas_add_family(void* atlas, const char* name, const char* chain_str) {
    if (!atlas || !name || !chain_str || !*name || !*chain_str) return -1;
    WgpuFontAtlas* a = (WgpuFontAtlas*)atlas;
    if (a->fallback) return -1;
    if (a->family_count >= MAX_FAMILIES) return -1;
    if (a->font_count >= MAX_FONTS) return -1;

    /* '#' 显式 Bold 段优先；否则 FontManager 的单 '|' = normal|bold */
    const char* hash = strchr(chain_str, '#');
    char normal_buf[1024];
    const char* normal_chain = chain_str;
    const char* bold_chain = NULL;
    int fm_normal_bold = 0; /* FontManager "a|b" 形态 */

    if (hash) {
        size_t nlen = (size_t)(hash - chain_str);
        if (nlen == 0 || nlen >= sizeof(normal_buf)) return -1;
        memcpy(normal_buf, chain_str, nlen);
        normal_buf[nlen] = '\0';
        normal_chain = normal_buf;
        bold_chain = hash + 1;
        while (*bold_chain == ' ' || *bold_chain == '\t') bold_chain++;
        if (!*bold_chain) bold_chain = NULL;
    } else {
        const char* pipe = strchr(chain_str, '|');
        const char* pipe2 = pipe ? strchr(pipe + 1, '|') : NULL;
        if (pipe && !pipe2) {
            /* FontManager：normal|bold —— 第二段不是 Symbol */
            size_t nlen = (size_t)(pipe - chain_str);
            if (nlen == 0 || nlen >= sizeof(normal_buf)) return -1;
            memcpy(normal_buf, chain_str, nlen);
            normal_buf[nlen] = '\0';
            normal_chain = normal_buf;
            bold_chain = pipe + 1;
            while (*bold_chain == ' ' || *bold_chain == '\t') bold_chain++;
            if (!*bold_chain) bold_chain = NULL;
            fm_normal_bold = 1;
        }
    }
    (void)fm_normal_bold;

    int start = a->font_count;
    int n = wgpu_font_atlas_load_chain(a, normal_chain, a->base_size_px);
    if (n <= 0) return -1;

    int bold_slot = -1;
    if (bold_chain) {
        int bold_start = a->font_count;
        int bn = wgpu_font_atlas_load_chain(a, bold_chain, a->base_size_px);
        if (bn > 0) {
            bold_slot = bold_start;
        } else {
            fprintf(stderr, "[wgpu_font] family '%s': bold chain failed; Bold→Normal fallback\n", name);
        }
    }

    /* 查重：同名 family 已注册则返回其索引（幂等）；回滚本次加载 */
    for (int i = 0; i < a->family_count; i++) {
        if (strcmp(a->families[i].name, name) == 0) {
            a->font_count = start;
            return i;
        }
    }

    FontFamily* f = &a->families[a->family_count];
    strncpy(f->name, name, sizeof(f->name) - 1);
    f->name[sizeof(f->name) - 1] = '\0';
    f->slot_start = start;
    f->slot_count = n;
    f->bold_slot = bold_slot;
    f->ascent = a->fonts[start].ascent;
    f->descent = a->fonts[start].descent;
    f->line_gap = a->fonts[start].line_gap;
    int idx = a->family_count;
    a->family_count++;
    fprintf(stderr, "[wgpu_font] family[%d] '%s': %d font(s), bold_slot=%d, ascent=%.1f\n",
            idx, f->name, n, bold_slot, f->ascent);
    return idx;
}

/* 按名称解析 family 索引；未注册返回 -1（调用方应回退 family 0）。 */
int wgpu_font_atlas_get_family_index(void* atlas, const char* name) {
    if (!atlas || !name) return -1;
    WgpuFontAtlas* a = (WgpuFontAtlas*)atlas;
    for (int i = 0; i < a->family_count; i++) {
        if (strcmp(a->families[i].name, name) == 0) return i;
    }
    return -1;
}

/* 按 family 取度量；越界回退 family 0（默认族）。 */
static FontFamily* wgpu_font_family_at(WgpuFontAtlas* a, int family_idx) {
    if (family_idx < 0 || family_idx >= a->family_count) family_idx = 0;
    return &a->families[family_idx];
}

double wgpu_font_atlas_get_family_ascent(void* atlas, int family_idx) {
    if (!atlas) return 0.0;
    return (double)wgpu_font_family_at((WgpuFontAtlas*)atlas, family_idx)->ascent;
}
double wgpu_font_atlas_get_family_descent(void* atlas, int family_idx) {
    if (!atlas) return 0.0;
    return (double)wgpu_font_family_at((WgpuFontAtlas*)atlas, family_idx)->descent;
}
double wgpu_font_atlas_get_family_line_gap(void* atlas, int family_idx) {
    if (!atlas) return 0.0;
    return (double)wgpu_font_family_at((WgpuFontAtlas*)atlas, family_idx)->line_gap;
}

/*
 * 查询 glyph：若已缓存直接返回；否则光栅化并入 atlas。
 * family_index：请求的字体族；缺失字形时回退 family 0（默认族）链。
 * weight：FONT_WEIGHT_NORMAL(0) / FONT_WEIGHT_BOLD(1)；Bold 且无 bold_slot 时回退 Normal 面。
 * size_px：目标物理像素高度（per-size bucket）——按此尺寸 1:1 光栅化，
 *          屏幕采样零缩放（正文锐度的决定性前提）；<=0 时回退 base_size_px。
 * 返回值：1 = 新增（需要 flush_uploads 提交到 GPU）；0 = 已在 atlas；-1 = 失败。
 * 输出 UV（0-1）+ metrics（size_px 物理像素域）。
 */
int wgpu_font_atlas_lookup_glyph(void* atlas, int family_index, int weight, uint32_t cp,
                                 double size_px,
                                 double* out_u0, double* out_v0,
                                 double* out_u1, double* out_v1,
                                 double* out_advance,
                                 double* out_xoff, double* out_yoff,
                                 double* out_gw, double* out_gh) {
    if (!atlas || !out_u0) return -1;
    WgpuFontAtlas* a = (WgpuFontAtlas*)atlas;
    if (a->fallback || a->font_count == 0) return -1;
    if (family_index < 0 || family_index >= a->family_count) family_index = 0;
    if (weight != FONT_WEIGHT_BOLD) weight = FONT_WEIGHT_NORMAL;
    int size_bucket = (int)(size_px + 0.5);
    if (size_bucket < 8) size_bucket = 8;
    if (size_bucket > 256) size_bucket = 256;
    int idx = -1;
    GlyphEntry* e = wgpu_font_cache_find(a->cache, a->cache_cap, cp, family_index,
                                         weight, size_bucket, &idx);
    if (e && e->valid) {
        float inv_w = 1.0f / (float)ATLAS_W;
        float inv_h = 1.0f / (float)ATLAS_H;
        *out_u0 = (double)((float)e->x * inv_w);
        *out_v0 = (double)((float)e->y * inv_h);
        *out_u1 = (double)((float)(e->x + e->gw) * inv_w);
        *out_v1 = (double)((float)(e->y + e->gh) * inv_h);
        *out_advance = (double)e->advance;
        *out_xoff = (double)e->xoff;
        *out_yoff = (double)e->yoff;
        *out_gw = (double)e->gw;
        *out_gh = (double)e->gh;
        return 0;
    }
    if (idx < 0) return -1; /* cache 满 */

    /* 多字体回退：Bold 优先 bold_slot 主面，再走 family Normal 链，再回退 family 0。 */
    FontFamily* fam = wgpu_font_family_at(a, family_index);
    int use_bold = (weight == FONT_WEIGHT_BOLD && fam->bold_slot >= 0
                    && fam->bold_slot < a->font_count) ? 1 : 0;
    int chosen = -1;
    if (use_bold) {
        if (rt_image_font_has_glyph(a->fonts[fam->bold_slot].font, cp) == 1) {
            chosen = fam->bold_slot;
        }
    }
    if (chosen < 0) {
        for (int fi = fam->slot_start; fi < fam->slot_start + fam->slot_count && fi < a->font_count; fi++) {
            if (rt_image_font_has_glyph(a->fonts[fi].font, cp) == 1) { chosen = fi; break; }
        }
    }
    if (chosen < 0) {
        FontFamily* df = &a->families[0];
        if (weight == FONT_WEIGHT_BOLD && df->bold_slot >= 0 && df->bold_slot < a->font_count
            && rt_image_font_has_glyph(a->fonts[df->bold_slot].font, cp) == 1) {
            chosen = df->bold_slot;
        }
        if (chosen < 0) {
            for (int fi = df->slot_start; fi < df->slot_start + df->slot_count && fi < a->font_count; fi++) {
                if (rt_image_font_has_glyph(a->fonts[fi].font, cp) == 1) { chosen = fi; break; }
            }
        }
    }
    if (chosen < 0) {
        /* 全缺字形→用请求字重主面（Bold 优先 bold_slot）光栅 .notdef */
        chosen = use_bold ? fam->bold_slot : fam->slot_start;
    }
    void* font = a->fonts[chosen].font;

    /* 光栅化新 glyph（按 size_bucket 物理像素高度 1:1 光栅） */
    int gw = 0, gh = 0;
    float xoff = 0, yoff = 0;
    float advance = 0;
    if (rt_image_font_glyph_full_px(font, cp, (double)size_bucket, NULL,
                                    &gw, &gh, &xoff, &yoff, &advance) != 0) {
        return -1;
    }
    if (gw <= 0 || gh <= 0) {
        /* 空白 glyph（如空格/控制符）：记录为 valid 但零尺寸 */
        GlyphEntry* ne = &a->cache[idx];
        ne->codepoint = cp;
        ne->family_idx = family_index;
        ne->weight = weight;
        ne->size_px = size_bucket;
        ne->valid = 1;
        ne->placed = 1;
        ne->font_idx = chosen;
        ne->gw = 0; ne->gh = 0;
        ne->advance = advance;
        ne->xoff = 0; ne->yoff = 0;
        *out_u0 = 0; *out_v0 = 0; *out_u1 = 0; *out_v1 = 0;
        *out_advance = (double)ne->advance;
        *out_xoff = 0; *out_yoff = 0;
        *out_gw = 0; *out_gh = 0;
        a->cache_count++;
        return 1;
    }
    /* rasterize */
    uint8_t* alpha = (uint8_t*)malloc((size_t)gw * (size_t)gh);
    if (!alpha) return -1;
    int gw2 = 0, gh2 = 0;
    float xo2 = 0, yo2 = 0, adv2 = 0;
    if (rt_image_font_glyph_full_px(font, cp, (double)size_bucket, alpha,
                                    &gw2, &gh2, &xo2, &yo2, &adv2) != 0) {
        free(alpha); return -1;
    }
    /* 防御（RFC 042 A-1 堆损坏排查）：查询(GetCodepointBitmapBox)与光栅
     * (GetCodepointBitmap) 尺寸理论一致，但若异常数据导致光栅位图更大，
     * memcpy 会溢出 alpha 缓冲——显式拦截，绝不静默越界写。 */
    if ((size_t)gw2 * (size_t)gh2 > (size_t)gw * (size_t)gh) {
        free(alpha); return -1;
    }
    GlyphEntry* ne = &a->cache[idx];
    ne->codepoint = cp;
    ne->family_idx = family_index;
    ne->weight = weight;
    ne->size_px = size_bucket;
    ne->valid = 1;
    ne->font_idx = chosen;
    ne->advance = adv2;
    ne->xoff = xo2;
    ne->yoff = yo2;
    wgpu_font_atlas_place_glyph(a, ne, alpha, gw2, gh2);
    free(alpha);
    if (!ne->valid) return -1; /* atlas 满 */
    a->cache_count++;

    /* 即时上传该 glyph 区域到 GPU（避免首帧缺字） */
    if (a->queue && a->texture && a->has_dirty) {
        WGPUQueue q = (WGPUQueue)((wgpu_wrap_t*)a->queue)->handle;
        WGPUTexture tex = (WGPUTexture)((wgpu_wrap_t*)a->texture)->handle;
        int x = ne->x, y = ne->y;
        int w = (int)ne->gw, h = (int)ne->gh;
        if (w > 0 && h > 0) {
            /* wgpu writeTexture: data 指向要拷贝区域的起点，offset=0；
               bytesPerRow 仍是完整 atlas 行跨度（源内存布局）；
               dataSize = (h-1)*bytesPerRow + w*4（wgpu 要求覆盖到最后一个拷贝像素）。 */
            size_t byte_offset = (size_t)(y * ATLAS_W + x) * 4;
            WGPUTexelCopyBufferLayout layout = WGPU_TEXEL_COPY_BUFFER_LAYOUT_INIT;
            layout.offset = 0;
            layout.bytesPerRow = ATLAS_W * 4;
            layout.rowsPerImage = h;
            WGPUTexelCopyTextureInfo dst = WGPU_TEXEL_COPY_TEXTURE_INFO_INIT;
            dst.texture = tex; dst.mipLevel = 0;
            dst.origin = (WGPUOrigin3D){ (uint32_t)x, (uint32_t)y, 0 };
            dst.aspect = WGPUTextureAspect_All;
            WGPUExtent3D sz = { (uint32_t)w, (uint32_t)h, 1 };
            wgpuQueueWriteTexture(q, &dst, (const uint8_t*)a->pixels + byte_offset,
                                  (size_t)((h - 1) * ATLAS_W * 4 + w * 4), &layout, &sz);
        }
        a->has_dirty = 0;
    }

    float inv_w = 1.0f / (float)ATLAS_W;
    float inv_h = 1.0f / (float)ATLAS_H;
    *out_u0 = (double)((float)ne->x * inv_w);
    *out_v0 = (double)((float)ne->y * inv_h);
    *out_u1 = (double)((float)(ne->x + ne->gw) * inv_w);
    *out_v1 = (double)((float)(ne->y + ne->gh) * inv_h);
    *out_advance = (double)ne->advance;
    *out_xoff = (double)ne->xoff;
    *out_yoff = (double)ne->yoff;
    *out_gw = (double)ne->gw;
    *out_gh = (double)ne->gh;
    return 1;
}

void wgpu_font_atlas_flush(void* atlas, void* queue_wrap) {
    if (!atlas || !queue_wrap) return;
    WgpuFontAtlas* a = (WgpuFontAtlas*)atlas;
    if (!a->has_dirty) return;
    WGPUQueue queue = (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle;
    WGPUTexture tex = (WGPUTexture)((wgpu_wrap_t*)a->texture)->handle;
    int x = a->dirty_x0, y = a->dirty_y0;
    int w = a->dirty_x1 - a->dirty_x0;
    int h = a->dirty_y1 - a->dirty_y0;
    if (w <= 0 || h <= 0) { a->has_dirty = 0; return; }
    /* wgpu writeTexture: 数据指针定位到 dirty rect 起点 (x,y)，offset=0；
       bytesPerRow 为完整 atlas 行跨度（源内存布局），dataSize 覆盖到最后一行。 */
    size_t byte_offset = (size_t)(y * ATLAS_W + x) * 4;
    WGPUTexelCopyBufferLayout layout = WGPU_TEXEL_COPY_BUFFER_LAYOUT_INIT;
    layout.offset = 0;
    layout.bytesPerRow = ATLAS_W * 4;
    layout.rowsPerImage = h;
    WGPUTexelCopyTextureInfo dst = WGPU_TEXEL_COPY_TEXTURE_INFO_INIT;
    dst.texture = tex; dst.mipLevel = 0;
    dst.origin = (WGPUOrigin3D){ (uint32_t)x, (uint32_t)y, 0 };
    dst.aspect = WGPUTextureAspect_All;
    WGPUExtent3D sz = { (uint32_t)w, (uint32_t)h, 1 };
    wgpuQueueWriteTexture(queue, &dst, (const uint8_t*)a->pixels + byte_offset,
                          (size_t)((h - 1) * ATLAS_W * 4 + w * 4), &layout, &sz);
    /* 标记所有 glyph 在该 rect 内为 placed */
    for (int i = 0; i < a->cache_cap; i++) {
        GlyphEntry* e = &a->cache[i];
        if (!e->valid || e->placed) continue;
        if (e->x >= x && e->y >= y &&
            e->x + e->gw <= x + w && e->y + e->gh <= y + h) {
            e->placed = 1;
        }
    }
    a->has_dirty = 0;
}

/* wgpu_release 前向声明（定义在文件后半部分，按类型 tag dispatch） */
void wgpu_release(void* wrap_ptr);

void wgpu_font_atlas_destroy(void* atlas) {
    if (!atlas) return;
    WgpuFontAtlas* a = (WgpuFontAtlas*)atlas;
    for (int i = 0; i < a->font_count; i++) {
        if (a->fonts[i].font) rt_image_font_free(a->fonts[i].font);
        free(a->fonts[i].ttf_data);
    }
    if (a->texture_view) {
        if (((wgpu_wrap_t*)a->texture_view)->aux) {
            wgpu_release(((wgpu_wrap_t*)a->texture_view)->aux);
        }
        wgpu_release(a->texture_view);
    } else if (a->texture) {
        wgpu_release(a->texture);
    }
    free(a->pixels);
    free(a->cache);
    free(a);
}

// ============================================================
// 生命周期——按类型 tag dispatch 到对应的 wgpu<Type>Release
// ============================================================
void wgpu_release(void* wrap_ptr) {
    if (!wrap_ptr) return;
    wgpu_wrap_t* w = (wgpu_wrap_t*)wrap_ptr;
    void* h = w->handle;
    switch (w->tag) {
        case WGPU_T_INSTANCE:
            wgpuInstanceRelease((WGPUInstance)h);
            g_instance = NULL;
            break;
        case WGPU_T_SURFACE:
            wgpuSurfaceRelease((WGPUSurface)h);
            break;
        case WGPU_T_ADAPTER:
            wgpuAdapterRelease((WGPUAdapter)h);
            break;
        case WGPU_T_DEVICE:
            wgpuDeviceRelease((WGPUDevice)h);
            break;
        case WGPU_T_QUEUE:
            wgpuQueueRelease((WGPUQueue)h);
            break;
        case WGPU_T_SHADER:
            wgpuShaderModuleRelease((WGPUShaderModule)h);
            break;
        case WGPU_T_PIPELINE:
            wgpuRenderPipelineRelease((WGPURenderPipeline)h);
            break;
        case WGPU_T_ENCODER:
            wgpuCommandEncoderRelease((WGPUCommandEncoder)h);
            break;
        case WGPU_T_PASS:
            wgpuRenderPassEncoderRelease((WGPURenderPassEncoder)h);
            break;
        case WGPU_T_TEXTURE_VIEW:
            wgpuTextureViewRelease((WGPUTextureView)h);
            if (w->aux) {
                wgpuTextureRelease((WGPUTexture)w->aux);
            }
            break;
        case WGPU_T_COMMAND_BUFFER:
            wgpuCommandBufferRelease((WGPUCommandBuffer)h);
            break;
        case WGPU_T_BUFFER:
            wgpuBufferRelease((WGPUBuffer)h);
            break;
        case WGPU_T_BIND_GROUP_LAYOUT:
            wgpuBindGroupLayoutRelease((WGPUBindGroupLayout)h);
            break;
        case WGPU_T_BIND_GROUP:
            wgpuBindGroupRelease((WGPUBindGroup)h);
            break;
        case WGPU_T_TEXTURE:
            wgpuTextureRelease((WGPUTexture)h);
            break;
        case WGPU_T_SAMPLER:
            wgpuSamplerRelease((WGPUSampler)h);
            break;
        case WGPU_T_OFFSCREEN:
            // 离屏目标整体销毁（texture/view/readback buffer + struct）。
            wgpu_offscreen_destroy(wrap_ptr);
            return;
        default:
            break;
    }
    free(w);
}
// ============================================================
// RFC 037 §10 AI 原生 AL-P0：离屏渲染目标 + 像素回读（references/render-capture）
//
// 离屏渲染 = 不依赖窗口 surface 的渲染 target（RENDER_ATTACHMENT|COPY_SRC）：
//   - 创建：RGBA8Unorm 纹理 + 视图 + readback 缓冲（MAP_READ|COPY_DST）
//   - 渲染：wgpu_offscreen_begin_pass 绑定离屏视图开始 RenderPass，绘制命令
//     走既有 wgpu_render_pass_* 通用路径（与窗口帧同一管线，单一惯用法）
//   - 回读：copy_texture_to_buffer → map_async 同步等待 → memcpy 到调用方缓冲
// 约束：离屏尺寸即物理像素（无 DPI 缩放，headless 语义）；上限 2048×2048
//       （评审分辨率，防无界显存/回读带宽）。
// ============================================================
typedef struct {
    WGPUDevice device;       // 供 readback 创建 encoder
    WGPUTexture texture;     // RENDER_ATTACHMENT | COPY_SRC
    WGPUTextureView view;    // render target 视图（offscreen 独占所有权）
    WGPUBuffer readback;     // MAP_READ | COPY_DST，size = row_bytes*height
    uint32_t width;
    uint32_t height;
    uint32_t row_bytes;      // bytesPerRow，256 对齐（COPY_BYTES_PER_ROW_ALIGNMENT）
} wgpu_offscreen_t;

#define WGPU_OFFSCREEN_MAX 2048

void* wgpu_offscreen_create(void* device_wrap, int width, int height) {
    if (!device_wrap || width <= 0 || height <= 0 ||
        width > WGPU_OFFSCREEN_MAX || height > WGPU_OFFSCREEN_MAX) {
        return NULL;
    }
    WGPUDevice dev = (WGPUDevice)((wgpu_wrap_t*)device_wrap)->handle;
    wgpu_offscreen_t* o = (wgpu_offscreen_t*)calloc(1, sizeof(wgpu_offscreen_t));
    if (!o) return NULL;
    o->device = dev;
    o->width = (uint32_t)width;
    o->height = (uint32_t)height;

    WGPUTextureDescriptor tdesc = WGPU_TEXTURE_DESCRIPTOR_INIT;
    tdesc.dimension = WGPUTextureDimension_2D;
    tdesc.size = (WGPUExtent3D){ (uint32_t)width, (uint32_t)height, 1 };
    tdesc.format = WGPUTextureFormat_RGBA8Unorm;
    tdesc.usage = WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_CopySrc;
    tdesc.mipLevelCount = 1;
    tdesc.sampleCount = 1;
    o->texture = wgpuDeviceCreateTexture(dev, &tdesc);
    if (!o->texture) { free(o); return NULL; }

    o->view = wgpuTextureCreateView(o->texture, NULL);
    if (!o->view) { wgpuTextureRelease(o->texture); free(o); return NULL; }

    WGPUBufferDescriptor bdesc = WGPU_BUFFER_DESCRIPTOR_INIT;
    // CopyTextureToBuffer 要求 bytesPerRow 为 COPY_BYTES_PER_ROW_ALIGNMENT(256)
    // 的倍数——tight 行宽 width*4 补齐到 256，再按对齐行 × height 分配回读缓冲。
    o->row_bytes = ((uint32_t)width * 4u + 255u) & ~255u;
    bdesc.size = (uint64_t)o->row_bytes * (uint64_t)height;
    bdesc.usage = WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst;
    o->readback = wgpuDeviceCreateBuffer(dev, &bdesc);
    if (!o->readback) {
        wgpuTextureViewRelease(o->view);
        wgpuTextureRelease(o->texture);
        free(o);
        return NULL;
    }
    return wgpu_wrap_new(WGPU_T_OFFSCREEN, o);
}

void* wgpu_offscreen_begin_pass(void* offscreen_wrap, void* encoder_wrap, int clear,
                                double clear_r, double clear_g, double clear_b, double clear_a) {
    if (!offscreen_wrap || !encoder_wrap) return NULL;
    wgpu_offscreen_t* o = (wgpu_offscreen_t*)((wgpu_wrap_t*)offscreen_wrap)->handle;
    WGPUCommandEncoder enc = (WGPUCommandEncoder)((wgpu_wrap_t*)encoder_wrap)->handle;
    if (!o || !o->view || !enc) return NULL;

    WGPURenderPassColorAttachment att = WGPU_RENDER_PASS_COLOR_ATTACHMENT_INIT;
    att.view = o->view;
    att.loadOp = clear ? WGPULoadOp_Clear : WGPULoadOp_Load;
    att.storeOp = WGPUStoreOp_Store;
    if (clear) {
        att.clearValue = (WGPUColor){ clear_r, clear_g, clear_b, clear_a };
    }
    WGPURenderPassDescriptor pdesc = WGPU_RENDER_PASS_DESCRIPTOR_INIT;
    pdesc.colorAttachmentCount = 1;
    pdesc.colorAttachments = &att;
    WGPURenderPassEncoder pass = wgpuCommandEncoderBeginRenderPass(enc, &pdesc);
    return wgpu_wrap_new(WGPU_T_PASS, pass);
}

// readback map_async 同步等待上下文（同 adapter/device 同步请求模式：
// AllowProcessEvents 回调 + 循环 wgpuInstanceProcessEvents + 超时熔断）。
typedef struct {
    volatile int done;
    volatile int status;   // 0 未定 / 1 Success / -1 其他
} offscreen_map_ctx;

static void wgpu_offscreen_map_cb(WGPUMapAsyncStatus status, WGPUStringView message,
                                  void* u1, void* u2) {
    (void)message; (void)u2;
    offscreen_map_ctx* ctx = (offscreen_map_ctx*)u1;
    ctx->status = (status == WGPUMapAsyncStatus_Success) ? 1 : -1;
    ctx->done = 1;
}

int wgpu_offscreen_readback(void* offscreen_wrap, void* queue_wrap, int64_t out_rgba, int capacity) {
    // out_rgba 为 long 句柄（rt_image_alloc 形态）——C 层转指针，规避编译器 CD-29。
    void* out_rgba_ptr = (void*)(uintptr_t)out_rgba;
    if (!offscreen_wrap || !queue_wrap || !out_rgba_ptr) return -1;
    wgpu_offscreen_t* o = (wgpu_offscreen_t*)((wgpu_wrap_t*)offscreen_wrap)->handle;
    WGPUQueue queue = (WGPUQueue)((wgpu_wrap_t*)queue_wrap)->handle;
    if (!o || !o->texture || !queue) return -1;
    uint32_t tight = o->width * 4u;              // 调用方缓冲为紧密 RGBA8（w*4/行）
    uint32_t pixel_bytes = o->width * o->height * 4u;
    uint32_t map_bytes = o->row_bytes * o->height; // 回读缓冲含行尾 256 对齐填充
    if (capacity < (int)pixel_bytes) return -2;

    WGPUCommandEncoder enc = wgpuDeviceCreateCommandEncoder(o->device, NULL);
    if (!enc) return -3;

    WGPUTexelCopyBufferInfo dst = WGPU_TEXEL_COPY_BUFFER_INFO_INIT;
    dst.buffer = o->readback;
    dst.layout.offset = 0;
    dst.layout.bytesPerRow = o->row_bytes;
    dst.layout.rowsPerImage = o->height;

    WGPUTexelCopyTextureInfo src = WGPU_TEXEL_COPY_TEXTURE_INFO_INIT;
    src.texture = o->texture;
    src.mipLevel = 0;
    src.origin = (WGPUOrigin3D){ 0, 0, 0 };
    src.aspect = WGPUTextureAspect_All;

    WGPUExtent3D copy_size = { o->width, o->height, 1 };
    wgpuCommandEncoderCopyTextureToBuffer(enc, &src, &dst, &copy_size);

    WGPUCommandBuffer cmd = wgpuCommandEncoderFinish(enc, NULL);
    wgpuCommandEncoderRelease(enc);
    if (!cmd) return -3;

    wgpuQueueSubmit(queue, 1, &cmd);
    wgpuCommandBufferRelease(cmd);

    // map_async 同步等待（AllowProcessEvents + 循环 process events + 超时熔断）。
    offscreen_map_ctx ctx = {0};
    WGPUBufferMapCallbackInfo cb = WGPU_BUFFER_MAP_CALLBACK_INFO_INIT;
    cb.mode = WGPUCallbackMode_AllowProcessEvents;
    cb.callback = wgpu_offscreen_map_cb;
    cb.userdata1 = &ctx;
    wgpuBufferMapAsync(o->readback, WGPUMapMode_Read, 0, map_bytes, cb);

    long long spin = 0;
    while (!ctx.done) {
        if (g_instance) {
            wgpuInstanceProcessEvents(g_instance);
        }
        spin++;
        if (spin > 200000000LL) {  // 约 5 秒熔断
            break;
        }
    }
    if (!ctx.done || ctx.status != 1) {
        return -3;
    }
    const uint8_t* mapped = (const uint8_t*)wgpuBufferGetMappedRange(o->readback, 0, map_bytes);
    if (!mapped) {
        wgpuBufferUnmap(o->readback);
        return -3;
    }
    // 调用方期望紧密 RGBA8（w*4/行）：逐行跳过 256 对齐填充，剔除行尾 padding。
    uint8_t* out = (uint8_t*)out_rgba_ptr;
    for (uint32_t y = 0; y < o->height; y++) {
        memcpy(out + (uint64_t)y * tight, mapped + (uint64_t)y * o->row_bytes, tight);
    }
    wgpuBufferUnmap(o->readback);
    return 0;
}

void wgpu_offscreen_destroy(void* offscreen_wrap) {
    if (!offscreen_wrap) return;
    wgpu_offscreen_t* o = (wgpu_offscreen_t*)((wgpu_wrap_t*)offscreen_wrap)->handle;
    if (o) {
        if (o->readback) wgpuBufferRelease(o->readback);
        if (o->view) wgpuTextureViewRelease(o->view);
        if (o->texture) wgpuTextureRelease(o->texture);
        free(o);
    }
    free(offscreen_wrap);
}

