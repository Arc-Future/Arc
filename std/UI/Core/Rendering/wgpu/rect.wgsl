// RFC 037 §3.6: 表面填充 WGSL——圆角 SDF + 纯色/两停靠点线性渐变统一原语
// 本文件是维护正本；WgpuRender.Wgsl.as 内嵌副本经 arc-ui 契约测试 wgsl_source_sync
// 与正本保持逐字一致（漂移即测试红，UPDATE_WGSL=1 再生），Initialize 时编译。
//
// 单一惯用法：直角/圆角、纯色/渐变、填充/描边均走本管线，禁止第二填充轨。
//
// uniform 布局（80 字节，256 字节对齐槽位）：
//   offset  0: x, y, w, h
//   offset 16: c0r, c0g, c0b, c0a   起点色 / 纯色
//   offset 32: c1r, c1g, c1b, c1a   终点色（纯色时 = c0）
//   offset 48: sx, sy, ex, ey       渐变轴（归一化；长度≈0 → 纯色）
//   offset 64: surface_w, surface_h, radius, stroke

struct RectUniform {
  x: f32, y: f32, w: f32, h: f32,
  c0r: f32, c0g: f32, c0b: f32, c0a: f32,
  c1r: f32, c1g: f32, c1b: f32, c1a: f32,
  sx: f32, sy: f32, ex: f32, ey: f32,
  surface_w: f32, surface_h: f32, radius: f32, stroke: f32,
}

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) px: f32,
  @location(1) py: f32,
}

@group(0) @binding(0) var<uniform> u: RectUniform;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var pos = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(0.0, 1.0)
  );
  let p = pos[vi];
  let px = u.x + p.x * u.w;
  let py = u.y + p.y * u.h;
  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;
  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;
  var out: VsOut;
  out.pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
  out.px = px;
  out.py = py;
  return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  // —— 圆角矩形 SDF ——
  let hw = u.w * 0.5;
  let hh = u.h * 0.5;
  let r = min(u.radius, min(hw, hh));
  let cx = u.x + hw;
  let cy = u.y + hh;
  let qx = abs(in.px - cx) - (hw - r);
  let qy = abs(in.py - cy) - (hh - r);
  let outside = length(vec2<f32>(max(qx, 0.0), max(qy, 0.0)));
  let inside = min(max(qx, qy), 0.0);
  let d = outside + inside - r;
  let aa = max(fwidth(d), 0.5);
  var mask = 1.0 - smoothstep(-aa, aa, d);
  if (u.stroke > 0.0) {
    let inner = 1.0 - smoothstep(-aa, aa, d + u.stroke);
    mask = mask - inner;
  }

  // —— 纯色 / 两停靠点线性渐变 ——
  let nx = (in.px - u.x) / max(u.w, 1e-6);
  let ny = (in.py - u.y) / max(u.h, 1e-6);
  let p0 = vec2<f32>(u.sx, u.sy);
  let dir = vec2<f32>(u.ex, u.ey) - p0;
  let len2 = dot(dir, dir);
  var t = 0.0;
  if (len2 > 1e-6) {
    t = clamp(dot(vec2<f32>(nx, ny) - p0, dir) / len2, 0.0, 1.0);
  }
  let c0 = vec4<f32>(u.c0r, u.c0g, u.c0b, u.c0a);
  let c1 = vec4<f32>(u.c1r, u.c1g, u.c1b, u.c1a);
  let color = mix(c0, c1, t);
  return vec4<f32>(color.rgb, color.a * mask);
}
