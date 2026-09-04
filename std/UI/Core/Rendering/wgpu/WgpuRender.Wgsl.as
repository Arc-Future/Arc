// WGSL shader 源码（partial 拆分）。
//
// WgpuRender 的 WGSL shader 源码（partial 扩展）：RectWgslSource / TextWgslSource。
// 方法与私有字段跨文件共享，详见核心文件 WgpuRender.as。

namespace Arc.UI.Rendering.Wgpu;

using Arc.Collections;
using Arc.UI.Components;
using Arc.UI.Internal;

public partial class WgpuRender {
    /// <summary>
    /// 表面填充 WGSL——圆角 SDF + 纯色/两停靠点线性渐变统一原语（单一惯用法）。
    /// 维护正本为 std/UI/Rendering/wgpu/rect.wgsl；本内嵌副本经 arc-ui 契约测试
    /// wgsl_source_sync 与正本保持逐字一致（漂移即测试红，UPDATE_WGSL=1 再生）。
    /// 直角/圆角、纯色/渐变、填充/描边均走本管线；禁止第二填充轨。
    /// </summary>
    private string RectWgslSource() {
        return "struct RectUniform {\n" +
               "  x: f32, y: f32, w: f32, h: f32,\n" +
               "  c0r: f32, c0g: f32, c0b: f32, c0a: f32,\n" +
               "  c1r: f32, c1g: f32, c1b: f32, c1a: f32,\n" +
               "  sx: f32, sy: f32, ex: f32, ey: f32,\n" +
               "  surface_w: f32, surface_h: f32, radius: f32, stroke: f32,\n" +
               "}\n" +
               "\n" +
               "struct VsOut {\n" +
               "  @builtin(position) pos: vec4<f32>,\n" +
               "  @location(0) px: f32,\n" +
               "  @location(1) py: f32,\n" +
               "}\n" +
               "\n" +
               "@group(0) @binding(0) var<uniform> u: RectUniform;\n" +
               "\n" +
               "@vertex\n" +
               "fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {\n" +
               "  var pos = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0),\n" +
               "    vec2<f32>(1.0, 0.0),\n" +
               "    vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0),\n" +
               "    vec2<f32>(1.0, 1.0),\n" +
               "    vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  let p = pos[vi];\n" +
               "  let px = u.x + p.x * u.w;\n" +
               "  let py = u.y + p.y * u.h;\n" +
               "  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;\n" +
               "  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;\n" +
               "  var out: VsOut;\n" +
               "  out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);\n" +
               "  out.px = px;\n" +
               "  out.py = py;\n" +
               "  return out;\n" +
               "}\n" +
               "\n" +
               "@fragment\n" +
               "fn fs_main(in: VsOut) -> @location(0) vec4<f32> {\n" +
               "  // —— 圆角矩形 SDF ——\n" +
               "  let hw = u.w * 0.5;\n" +
               "  let hh = u.h * 0.5;\n" +
               "  let r = min(u.radius, min(hw, hh));\n" +
               "  let cx = u.x + hw;\n" +
               "  let cy = u.y + hh;\n" +
               "  let qx = abs(in.px - cx) - (hw - r);\n" +
               "  let qy = abs(in.py - cy) - (hh - r);\n" +
               "  let outside = length(vec2<f32>(max(qx, 0.0), max(qy, 0.0)));\n" +
               "  let inside = min(max(qx, qy), 0.0);\n" +
               "  let d = outside + inside - r;\n" +
               "  let aa = max(fwidth(d), 0.5);\n" +
               "  var mask = 1.0 - smoothstep(-aa, aa, d);\n" +
               "  if (u.stroke > 0.0) {\n" +
               "    let inner = 1.0 - smoothstep(-aa, aa, d + u.stroke);\n" +
               "    mask = mask - inner;\n" +
               "  }\n" +
               "\n" +
               "  // —— 纯色 / 两停靠点线性渐变 ——\n" +
               "  let nx = (in.px - u.x) / max(u.w, 1e-6);\n" +
               "  let ny = (in.py - u.y) / max(u.h, 1e-6);\n" +
               "  let p0 = vec2<f32>(u.sx, u.sy);\n" +
               "  let dir = vec2<f32>(u.ex, u.ey) - p0;\n" +
               "  let len2 = dot(dir, dir);\n" +
               "  var t = 0.0;\n" +
               "  if (len2 > 1e-6) {\n" +
               "    t = clamp(dot(vec2<f32>(nx, ny) - p0, dir) / len2, 0.0, 1.0);\n" +
               "  }\n" +
               "  let c0 = vec4<f32>(u.c0r, u.c0g, u.c0b, u.c0a);\n" +
               "  let c1 = vec4<f32>(u.c1r, u.c1g, u.c1b, u.c1a);\n" +
               "  let color = mix(c0, c1, t);\n" +
               "  return vec4<f32>(color.rgb, color.a * mask);\n" +
               "}\n";
    }

    /// <summary>
    /// RFC 037 §3.5 浮层阴影（Shadow.Surface）：软阴影 WGSL shader 源码。
    /// 与矩形 shader 同构：6 顶点单位 quad + uniform 定位；片元阶段按
    /// 到核心矩形边界的高斯衰减计算半透明黑阴影（软毛边）。
    /// uniform（64 字节）：quad 边界 + 核心矩形 + radius/blur/alpha + surface。
    /// </summary>
    private string ShadowWgslSource() {
        return "struct ShadowUniform {\n" +
               "  x: f32, y: f32, w: f32, h: f32,\n" +
               "  cx: f32, cy: f32, cw: f32, ch: f32,\n" +
               "  radius: f32, blur: f32, a: f32, surface_w: f32,\n" +
               "  surface_h: f32, _pad0: f32, _pad1: f32, _pad2: f32,\n" +
               "}\n" +
               "struct VsOut {\n" +
               "  @builtin(position) pos: vec4<f32>,\n" +
               "  @location(0) px: f32,\n" +
               "  @location(1) py: f32,\n" +
               "}\n" +
               "@group(0) @binding(0) var<uniform> u: ShadowUniform;\n" +
               "@vertex\n" +
               "fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {\n" +
               "  var pos = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  let p = pos[vi];\n" +
               "  let px = u.x + p.x * u.w;\n" +
               "  let py = u.y + p.y * u.h;\n" +
               "  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;\n" +
               "  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;\n" +
               "  var out: VsOut;\n" +
               "  out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);\n" +
               "  out.px = px;\n" +
               "  out.py = py;\n" +
               "  return out;\n" +
               "}\n" +
               "@fragment\n" +
               "fn fs_main(in: VsOut) -> @location(0) vec4<f32> {\n" +
               "  let inside = (in.px >= u.cx) && (in.px <= u.cx + u.cw) &&\n" +
               "               (in.py >= u.cy) && (in.py <= u.cy + u.ch);\n" +
               "  if (inside) {\n" +
               "    return vec4<f32>(0.0, 0.0, 0.0, 0.0);\n" +
               "  }\n" +
               "  let dx = max(u.cx - in.px, in.px - (u.cx + u.cw));\n" +
               "  let dy = max(u.cy - in.py, in.py - (u.cy + u.ch));\n" +
               "  let d = length(vec2<f32>(max(dx, 0.0), max(dy, 0.0)));\n" +
               "  let t = d / u.blur;\n" +
               "  let gauss = exp(-t * t * 0.5);\n" +
               "  return vec4<f32>(0.0, 0.0, 0.0, u.a * gauss);\n" +
               "}\n";
    }

    /// <summary>
    /// 文本渲染 WGSL shader 源码（RFC 037 M2）。
    /// 与矩形 shader 同构：6 顶点单位 quad + uniform 定位，逐字形绘制；
    /// 片段阶段从 glyph atlas 采样（bind group layout：
    /// uniform@0 动态偏移 + texture@1 + sampler@2）。
    /// </summary>
    private string TextWgslSource() {
        return "struct TextUniform {\n" +
               "  x: f32, y: f32, w: f32, h: f32,\n" +
               "  u0: f32, v0: f32, u1: f32, v1: f32,\n" +
               "  r: f32, g: f32, b: f32, a: f32,\n" +
               "  surface_w: f32, surface_h: f32, _pad0: f32, _pad1: f32,\n" +
               "}\n" +
               "struct VsOut {\n" +
               "  @builtin(position) pos: vec4<f32>,\n" +
               "  @location(0) uv: vec2<f32>,\n" +
               "}\n" +
               "@group(0) @binding(0) var<uniform> u: TextUniform;\n" +
               "@group(0) @binding(1) var font_tex: texture_2d<f32>;\n" +
               "@group(0) @binding(2) var font_smp: sampler;\n" +
               "@vertex\n" +
               "fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {\n" +
               "  var pos = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  var uvs = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  let p = pos[vi];\n" +
               "  let px = u.x + p.x * u.w;\n" +
               "  let py = u.y + p.y * u.h;\n" +
               "  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;\n" +
               "  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;\n" +
               "  var out: VsOut;\n" +
               "  out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);\n" +
               "  out.uv = vec2<f32>(u.u0 + (u.u1 - u.u0) * uvs[vi].x,\n" +
               "                     u.v0 + (u.v1 - u.v0) * uvs[vi].y);\n" +
               "  return out;\n" +
               "}\n" +
               "@fragment\n" +
               "fn fs_main(in: VsOut) -> @location(0) vec4<f32> {\n" +
               "  let tex = textureSample(font_tex, font_smp, in.uv);\n" +
               "  // 笔画对比度补偿：gamma-correct（线性空间）混合会让 AA 边缘显示得比 sRGB 域\n" +
               "  // 混合更淡更柔，小字号正文笔画显虚（Chrome 字体发虚同款问题）。pow 0.7 提升\n" +
               "  // 中低 alpha，等效 DirectWrite enhancedContrast，让无 hinting 光栅的笔画恢复实感。\n" +
               "  let alpha = pow(tex.a, 0.7);\n" +
               "  return vec4<f32>(u.r, u.g, u.b, alpha * u.a);\n" +
               "}\n";
    }

    /// <summary>
    /// 图像渲染 WGSL shader 源码（RFC 037 references/texture-surface）。
    /// 与文本 shader 同构：同一 TextUniform 布局 + 同一顶点 UV 映射；
    /// 仅 fragment 不同——**直出纹理 RGB**（乘 tint），而非文字用纹理 alpha 作透明度。
    /// 由此复用文本 bind group layout / pipeline create / staging 写入器（wgpu_batch_text_write）。
    /// </summary>
    private string ImageWgslSource() {
        return "struct TextUniform {\n" +
               "  x: f32, y: f32, w: f32, h: f32,\n" +
               "  u0: f32, v0: f32, u1: f32, v1: f32,\n" +
               "  r: f32, g: f32, b: f32, a: f32,\n" +
               "  surface_w: f32, surface_h: f32, _pad0: f32, _pad1: f32,\n" +
               "}\n" +
               "struct VsOut {\n" +
               "  @builtin(position) pos: vec4<f32>,\n" +
               "  @location(0) uv: vec2<f32>,\n" +
               "}\n" +
               "@group(0) @binding(0) var<uniform> u: TextUniform;\n" +
               "@group(0) @binding(1) var tex: texture_2d<f32>;\n" +
               "@group(0) @binding(2) var smp: sampler;\n" +
               "@vertex\n" +
               "fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {\n" +
               "  var pos = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  var uvs = array<vec2<f32>, 6>(\n" +
               "    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),\n" +
               "    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)\n" +
               "  );\n" +
               "  let p = pos[vi];\n" +
               "  let px = u.x + p.x * u.w;\n" +
               "  let py = u.y + p.y * u.h;\n" +
               "  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;\n" +
               "  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;\n" +
               "  var out: VsOut;\n" +
               "  out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);\n" +
               "  out.uv = vec2<f32>(u.u0 + (u.u1 - u.u0) * uvs[vi].x,\n" +
               "                     u.v0 + (u.v1 - u.v0) * uvs[vi].y);\n" +
               "  return out;\n" +
               "}\n" +
               "@fragment\n" +
               "fn fs_main(in: VsOut) -> @location(0) vec4<f32> {\n" +
               "  let tex = textureSample(tex, smp, in.uv);\n" +
               "  return vec4<f32>(tex.rgb * vec3<f32>(u.r, u.g, u.b), tex.a * u.a);\n" +
               "}\n";
    }
}
