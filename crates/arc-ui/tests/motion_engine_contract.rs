//! RFC 037 §3.6 Motion: MotionEngine 状态过渡插值契约（Internal）。
//!
//! 源契约：验证状态过渡引擎已实现并被渲染/帧泵接线——不依赖 GPU/真窗：
//!   - `std/UI/Core/Internal/MotionEngine.as` 提供 ResolveColor / Active / Ease / 角色常量；
//!   - `FramePump` 在过渡进行中（MotionEngine.Active）保持每帧渲染；
//!   - `WgpuRender.RenderTree` 将状态 palette 色经 MotionEngine 解析（插值）后上屏；
//!   - `Window.PrepareForShow` 每 Show 前清空过渡槽。

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn read_file(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn motion_engine_provides_interpolation_surface() {
    let motion = read_file("std/UI/Core/Internal/MotionEngine.as");

    // 角色常量（渲染器消费）
    for role in [
        "RoleBackground",
        "RoleForeground",
        "RoleBorder",
        "RoleFocusRing",
        "RoleAccent",
    ] {
        assert!(
            motion.contains(&format!("public const int {role} =")),
            "MotionEngine missing role constant {role}"
        );
    }

    // 核心接口
    assert!(
        motion.contains("public static Color ResolveColor(long handle, int role, string target)")
    );
    assert!(motion.contains("public static bool Active()"));
    assert!(motion.contains("public static double Ease(double t)"));

    // 时间源为 Stopwatch（QPC/CLOCK_MONOTONIC 单调）
    assert!(motion.contains("Stopwatch.GetTimestamp()"));
    assert!(motion.contains("Stopwatch.Frequency"));

    // 时长对齐 RFC 037 §3.6 Motion Token（经 Application.Current 单一解析根运行时解析）
    assert!(motion.contains("Application.Current.ResolveNumber(BuiltInTheme.MotionDurationNormal)"));
    assert!(motion.contains("Application.Current.ResolveNumber(BuiltInTheme.MotionDurationFast)"));
    // 显式时长覆写（VSM 每状态 motion 消费）
    assert!(motion.contains("public static Color ResolveColorDur(long handle, int role, string target, double durationMs)"));
}

#[test]
fn frame_pump_renders_during_transition() {
    let pump = read_file("std/UI/Core/Internal/FramePump.as");
    // 过渡进行中保持每帧渲染（仅需时渲染的例外）。
    // 908a04b1 后为局部变量写法：`bool dirty = FramePump.NeedsRender();
    // if (dirty || MotionEngine.Active())`，语义不变——渲染决策同时结合脏标记
    // 与过渡进行中状态。
    assert!(
        pump.contains("MotionEngine.Active()"),
        "FramePump must keep rendering while a transition is active"
    );
    assert!(
        pump.contains("NeedsRender()") && pump.contains("dirty || motion"),
        "FramePump render decision must combine dirty flag with active transition"
    );
}

#[test]
fn render_tree_resolves_state_colors_through_motion() {
    let render = read_file("std/UI/Core/Rendering/Wgpu/WgpuRender.RenderTree.as");
    // 状态色经 MotionEngine 解析后上屏（背景/前景/边框/焦点环/强调）
    let expected = [
        "MotionEngine.RoleBackground",
        "MotionEngine.RoleForeground",
        "MotionEngine.RoleBorder",
        "MotionEngine.RoleFocusRing",
        "MotionEngine.RoleAccent",
    ];
    for role in expected {
        assert!(
            render.contains(role),
            "WgpuRender.RenderTree must resolve {role} through MotionEngine"
        );
    }
    assert!(render.contains("MotionEngine.ResolveColor(handle"));
}

#[test]
fn window_resets_transition_slots_per_show() {
    let window = read_file("std/UI/Core/Components/Window.as");
    assert!(
        window.contains("MotionEngine.Reset()"),
        "Window.PrepareForShow must reset MotionEngine transition slots"
    );
}
