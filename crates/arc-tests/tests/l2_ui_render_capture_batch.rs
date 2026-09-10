//! L2 批量：AI 原生渲染回读硬门槛（RFC 037 §10 · ai-native-render-capture）。
//!
//! 验收面（headless，无 HWND）：
//! - `offscreen_png_magic`：InitializeOffscreen → CreateOffscreenTarget(64×48)
//!   → 空 DrawList RenderToOffscreen → ReadbackPixels → PngEncoder.Encode
//!   → 文件存在 / PNG 魔数 / IHDR 宽高 / 回读像素可读
//! - `offscreen_fillrect_color`：FillRect(hex) → ExecuteDrawList → 回读采样点
//!   非纯清屏（opaque red vs clear black）
//! - `offscreen_limit_reject`：超评审上限（>2048）创建失败返回 0
//! - `png_encoder_reject`：PngEncoder 参数校验返回 false
//!
//! 根因纪要：DrawCommand.FillRect(FillRectPayload) 经 List 存储时，variant
//! 深拷贝曾只复制外壳、struct payload 仍挂创建帧栈 → Color.Parse AV；codegen
//! 已对 struct payload 堆化（emit_variant_construct / deep_copy）。
//!
//! 宣称纪律：仅关 AL-P0 渲染回读（含 FillRect 着色）headless 硬门槛；
//! **不**宣称 G1–G3 / 布局快照 / 保真闭环。
//!
//! 依赖：`("Arc.UI", "UI/Core")`。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_render_capture_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_render_capture",
        &[
            BatchCase {
                name: "offscreen_png_magic",
                src: r##"using Arc;
using Arc.Drawing;
using Arc.IO;
using Arc.UI.Rendering;
using Arc.UI.Rendering.Wgpu;

void Main() {
    WgpuRender render = new WgpuRender();
    if (!render.InitializeOffscreen()) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:init");
        return;
    }
    int w = 64;
    int h = 48;
    int id = render.CreateOffscreenTarget(w, h);
    if (id == 0) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:create");
        return;
    }

    DrawList list = new DrawList();
    int rc = render.RenderToOffscreen(id, list, (double)w, (double)h);
    if (rc != 0) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:render=" + rc);
        return;
    }

    Bitmap bmp = new Bitmap(w, h);
    long pixels = bmp.GetPixels();
    if (!render.ReadbackPixels(id, pixels, w * h * 4)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:readback");
        return;
    }

    // clear 黑色：回读缓冲可读（禁静默空失败）。
    RgbColor sample = bmp.GetPixel(0, 0);
    if ((int)sample.A < 200) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:pixel_a=" + (int)sample.A);
        return;
    }

    string path = Path.Combine(Path.GetTempPath(), "arc_ui_render_capture_64x48.png");
    if (File.Exists(path)) {
        File.Delete(path);
    }
    if (!PngEncoder.Encode(path, pixels, w, h)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:encode");
        return;
    }
    bmp.Dispose();
    render.DestroyOffscreenTarget(id);

    if (!File.Exists(path)) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:missing");
        return;
    }
    byte[] bytes = File.ReadAllBytes(path);
    File.Delete(path);
    if (bytes.Length < 24) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:too_short=" + bytes.Length);
        return;
    }
    if ((bytes[0] & 0xFF) != 0x89 || (bytes[1] & 0xFF) != 0x50
        || (bytes[2] & 0xFF) != 0x4E || (bytes[3] & 0xFF) != 0x47
        || (bytes[4] & 0xFF) != 0x0D || (bytes[5] & 0xFF) != 0x0A
        || (bytes[6] & 0xFF) != 0x1A || (bytes[7] & 0xFF) != 0x0A) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:magic");
        return;
    }
    if ((bytes[12] & 0xFF) != 0x49 || (bytes[13] & 0xFF) != 0x48
        || (bytes[14] & 0xFF) != 0x44 || (bytes[15] & 0xFF) != 0x52) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:ihdr_tag");
        return;
    }
    int iw = ((bytes[16] & 0xFF) << 24) | ((bytes[17] & 0xFF) << 16)
        | ((bytes[18] & 0xFF) << 8) | (bytes[19] & 0xFF);
    int ih = ((bytes[20] & 0xFF) << 24) | ((bytes[21] & 0xFF) << 16)
        | ((bytes[22] & 0xFF) << 8) | (bytes[23] & 0xFF);
    if (iw != w || ih != h) {
        Console.WriteLine("ARC_CASE:offscreen_png_magic:FAIL:size=" + iw + "x" + ih);
        return;
    }
    Console.WriteLine("ARC_CASE:offscreen_png_magic:PASS");
}
"##,
            },
            BatchCase {
                name: "offscreen_fillrect_color",
                src: r##"using Arc;
using Arc.Drawing;
using Arc.UI.Rendering;
using Arc.UI.Rendering.Wgpu;

void Main() {
    WgpuRender render = new WgpuRender();
    if (!render.InitializeOffscreen()) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:init");
        return;
    }
    int w = 64;
    int h = 48;
    int id = render.CreateOffscreenTarget(w, h);
    if (id == 0) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:create");
        return;
    }

    DrawList list = new DrawList();
    DrawContext dc = new DrawContext(list);
    dc.Begin();
    // #AARRGGBB opaque red over clear black — sample inside rect must not be clear.
    dc.FillRect(8.0, 8.0, 48.0, 32.0, "#FFFF0000");
    dc.End();

    int rc = render.RenderToOffscreen(id, list, (double)w, (double)h);
    if (rc != 0) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:render=" + rc);
        return;
    }

    Bitmap bmp = new Bitmap(w, h);
    long pixels = bmp.GetPixels();
    if (!render.ReadbackPixels(id, pixels, w * h * 4)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:readback");
        return;
    }

    RgbColor clear = bmp.GetPixel(0, 0);
    RgbColor fill = bmp.GetPixel(32, 24);
    bmp.Dispose();
    render.DestroyOffscreenTarget(id);

    if ((int)clear.R > 40 || (int)clear.G > 40 || (int)clear.B > 40) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:clear_not_black r="
            + (int)clear.R + " g=" + (int)clear.G + " b=" + (int)clear.B);
        return;
    }
    if ((int)fill.R < 180) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:fill_r=" + (int)fill.R
            + " g=" + (int)fill.G + " b=" + (int)fill.B + " a=" + (int)fill.A);
        return;
    }
    if ((int)fill.G > 80 || (int)fill.B > 80) {
        Console.WriteLine("ARC_CASE:offscreen_fillrect_color:FAIL:fill_not_red g="
            + (int)fill.G + " b=" + (int)fill.B);
        return;
    }
    Console.WriteLine("ARC_CASE:offscreen_fillrect_color:PASS");
}
"##,
            },
            BatchCase {
                name: "offscreen_limit_reject",
                src: r##"using Arc;
using Arc.UI.Rendering.Wgpu;

void Main() {
    WgpuRender render = new WgpuRender();
    if (!render.InitializeOffscreen()) {
        Console.WriteLine("ARC_CASE:offscreen_limit_reject:FAIL:init");
        return;
    }
    int over = render.CreateOffscreenTarget(2049, 64);
    if (over != 0) {
        Console.WriteLine("ARC_CASE:offscreen_limit_reject:FAIL:accepted_oversize");
        return;
    }
    int zero = render.CreateOffscreenTarget(0, 64);
    if (zero != 0) {
        Console.WriteLine("ARC_CASE:offscreen_limit_reject:FAIL:accepted_zero");
        return;
    }
    int ok = render.CreateOffscreenTarget(16, 16);
    if (ok == 0) {
        Console.WriteLine("ARC_CASE:offscreen_limit_reject:FAIL:valid_rejected");
        return;
    }
    render.DestroyOffscreenTarget(ok);
    Console.WriteLine("ARC_CASE:offscreen_limit_reject:PASS");
}
"##,
            },
            BatchCase {
                name: "png_encoder_reject",
                src: r##"using Arc;
using Arc.Drawing;
using Arc.UI.Rendering;

void Main() {
    Bitmap bmp = new Bitmap(8, 8);
    long px = bmp.GetPixels();
    if (PngEncoder.Encode(null, px, 8, 8)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:png_encoder_reject:FAIL:null_path");
        return;
    }
    if (PngEncoder.Encode("", px, 8, 8)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:png_encoder_reject:FAIL:empty_path");
        return;
    }
    if (PngEncoder.Encode("x.png", 0, 8, 8)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:png_encoder_reject:FAIL:zero_handle");
        return;
    }
    if (PngEncoder.Encode("x.png", px, 0, 8)) {
        bmp.Dispose();
        Console.WriteLine("ARC_CASE:png_encoder_reject:FAIL:zero_width");
        return;
    }
    bmp.Dispose();
    Console.WriteLine("ARC_CASE:png_encoder_reject:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );

    for name in [
        "offscreen_png_magic",
        "offscreen_fillrect_color",
        "offscreen_limit_reject",
        "png_encoder_reject",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "ui_render_capture: case {name} failed: {:?}\nstdout:\n{}",
            r.error, r.stdout
        );
    }
}