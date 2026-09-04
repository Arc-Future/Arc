//! RFC 026 M3.5: WGSL compile-time validation tests.
//!
//! 使用 naga（wgpu 项目的 WGSL 解析/编译库）在构建时验证所有 WGSL
//! shader 文件的语法正确性。这避免了将 shader 语法错误推迟到运行时
//!（wgpuShaderModuleCreateWGSL 失败 → Initialize 失败 → 无提示回退到
//! SoftwareBackend → 用户困惑）。
//!
//! 测试覆盖：
//!   - rect.wgsl：矩形绘制 shader（WgpuRender 主线）
//!   - mock 渲染上下文：内存中构造 WGSL → 直接验证，无需 GPU 硬件

use std::path::PathBuf;

/// 获取 Arc monorepo 根目录（crates/arc-ui/ 的上两级）。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent() // crates/
        .unwrap()
        .parent() // repo root
        .unwrap()
        .to_path_buf()
}

/// 读取并验证单个 WGSL 文件。解析成功后额外检查入口点。
fn validate_wgsl_file(path: &str) -> naga::Module {
    let full_path = repo_root().join(path);
    let source = std::fs::read_to_string(&full_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", full_path.display(), e));

    let result = naga::front::wgsl::parse_str(&source);
    match result {
        Ok(module) => {
            let has_vs = module.entry_points.iter().any(|ep| ep.name == "vs_main");
            let has_fs = module.entry_points.iter().any(|ep| ep.name == "fs_main");
            assert!(has_vs, "{}: missing vertex entry point 'vs_main'", path);
            assert!(has_fs, "{}: missing fragment entry point 'fs_main'", path);
            module
        }
        Err(e) => {
            let err_msg = e.emit_to_string(&source);
            panic!("WGSL validation failed for {}:\n{}", path, err_msg);
        }
    }
}

// ============================================================
// 测试 1：真实 shader 文件验证（磁盘 → parse → 入口点检查）
// ============================================================

#[test]
fn validate_rect_wgsl_from_file() {
    let module = validate_wgsl_file("std/UI/Core/Rendering/wgpu/rect.wgsl");

    // 诊断输出：shader 元数据
    eprintln!("=== WGSL Validation Report: rect.wgsl ===");
    eprintln!("  entry points:");
    for ep in &module.entry_points {
        eprintln!("    {:?} fn {}()", ep.stage, ep.name);
    }
    eprintln!("  global variables: {}", module.global_variables.len());
    eprintln!("  types: {}", module.types.len());
    eprintln!("  functions: {}", module.functions.len());
    eprintln!("=== PASS ===");
}

// ============================================================
// 测试 2：mock 渲染上下文 —— 内存中构造 WGSL，无 GPU 硬件依赖
// ============================================================

/// Mock 渲染上下文：模拟 WgpuRender 在 Initialize 阶段将 WGSL 字符串
/// 传递给 wgpuShaderModuleCreateWGSL 的路径，但用 naga 在编译期替代。
///
/// 这演示了：即使没有 GPU / wgpu-native DLL / 窗口系统，也能在 CI 或
/// 开发者机器上验证 shader 正确性。
struct MockRenderContext {
    shader_source: &'static str,
    pipeline_label: &'static str,
}

impl MockRenderContext {
    fn new(shader_source: &'static str, pipeline_label: &'static str) -> Self {
        Self {
            shader_source,
            pipeline_label,
        }
    }

    /// 模拟 WgpuRender.Initialize 中的 shader 创建路径。
    /// 成功返回 naga::Module；失败 panic 带格式化错误信息。
    fn compile_shader(&self) -> naga::Module {
        match naga::front::wgsl::parse_str(self.shader_source) {
            Ok(module) => module,
            Err(e) => {
                let err_msg = e.emit_to_string(self.shader_source);
                panic!(
                    "GPU shader compilation failed for pipeline '{}':\n{}",
                    self.pipeline_label, err_msg
                );
            }
        }
    }
}

#[test]
fn mock_wgpu_backend_initialize_success() {
    // 模拟 WgpuRender.rect_wgsl_source() 返回的 WGSL 字符串
    let wgsl = "\
struct RectUniform {
  x: f32, y: f32, w: f32, h: f32,
  r: f32, g: f32, b: f32, a: f32,
  surface_w: f32, surface_h: f32, _pad0: f32, _pad1: f32,
}
@group(0) @binding(0) var<uniform> u: RectUniform;
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
  var pos = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)
  );
  let p = pos[vi];
  let px = u.x + p.x * u.w;
  let py = u.y + p.y * u.h;
  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;
  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;
  return vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
  return vec4<f32>(u.r, u.g, u.b, u.a);
}";

    let ctx = MockRenderContext::new(wgsl, "RectPipeline");

    eprintln!("=== Mock: WgpuRender.Initialize ===");
    eprintln!("  pipeline: {}", ctx.pipeline_label);
    eprintln!("  shader source length: {} bytes", ctx.shader_source.len());

    let module = ctx.compile_shader();

    eprintln!("  entry points:");
    for ep in &module.entry_points {
        eprintln!("    {:?} fn {}()", ep.stage, ep.name);
    }
    eprintln!("  types: {}", module.types.len());
    eprintln!("  global variables: {}", module.global_variables.len());
    eprintln!("=== PASS (no GPU required) ===");
}

// ============================================================
// 测试 3：mock 渲染上下文 —— 注入 WGSL 错误，展示错误报告效果
// ============================================================

#[test]
#[should_panic(expected = "failed to convert expression")]
fn mock_broken_wgsl_type_mismatch() {
    // 故意错误：struct 字段 `x: i32` 但后面 `px = u.x + p.x * u.w`
    // 涉及 f32 × i32 类型不匹配。
    let broken_wgsl = "\
struct RectUniform {
  x: i32, y: f32, w: f32, h: f32,
  r: f32, g: f32, b: f32, a: f32,
  surface_w: f32, surface_h: f32, _pad0: f32, _pad1: f32,
}
@group(0) @binding(0) var<uniform> u: RectUniform;
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
  var pos = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)
  );
  let p = pos[vi];
  let px = u.x + p.x * u.w;
  let py = u.y + p.y * u.h;
  let ndc_x = (px / u.surface_w) * 2.0 - 1.0;
  let ndc_y = 1.0 - (py / u.surface_h) * 2.0;
  return vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
  return vec4<f32>(u.r, u.g, u.b, u.a);
}";

    eprintln!("=== Mock: Injecting broken WGSL (i32 × f32 type mismatch) ===");
    let ctx = MockRenderContext::new(broken_wgsl, "RectPipeline");
    ctx.compile_shader(); // should panic with type mismatch detail
}

#[test]
#[should_panic(expected = "GPU shader compilation failed")]
fn mock_broken_wgsl_missing_semicolon() {
    // 故意错误：struct 声明缺少分号。
    let broken_wgsl = "\
struct RectUniform {
  x: f32, y: f32, w: f32, h: f32  // MISSING COMMA
  r: f32, g: f32, b: f32, a: f32,
}
@group(0) @binding(0) var<uniform> u: RectUniform;
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
  return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
  return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}";

    eprintln!("=== Mock: Injecting broken WGSL (missing separator) ===");
    let ctx = MockRenderContext::new(broken_wgsl, "RectPipeline");
    ctx.compile_shader(); // should panic with parse error detail
}
