// RFC 037 §3.6 Motion · RFC 037 Internal: 状态过渡插值引擎（120–200ms + easing）。
//
// 渲染器每帧读到交互态（:hover / :pressed / :focus-visible / :disabled）解析出的
// 终态色后，经本引擎按角色（Background/Foreground/Border/FocusRing/Accent）做
// 时间插值，使态切换呈现平滑过渡而非瞬时跳变。
//
// 设计（对齐 PointerRouter ≤8 固定槽 + 平行 List 模式）：
//   - 按 (platformHandle, role) 建立槽位；首现直返终态（无过渡）；
//   - 目标色变化 → 从「当前显示色」向新目标色开始 160ms ease-out 过渡；
//   - 目标色不变 → 按已流逝时间插值；到时长即吸附目标并停摆；
//   - <see cref="Active"/> 供 FramePump 在过渡期间保持每帧渲染。
//
// 时间源：<see cref="Stopwatch"/>（Arc.Diagnostics · QPC/CLOCK_MONOTONIC）。
// 与 C 侧无耦合；纯 Arc 实现（架构红线：编译器 arc-ui 不含视觉插值领域逻辑）。

namespace Arc.UI.Internal;

using Arc.Collections;
using Arc.Diagnostics;
using Arc.Text;
using Arc.UI.Components;
using Arc.UI.Media;
using Arc.UI.Styling;

/// <summary>状态过渡插值引擎（Internal 命名空间：非用户 API；e2e 验收直驱）。</summary>
public class MotionEngine {
    private MotionEngine() {
    }

    // ---- 渲染角色（渲染器消费：每个 palette 颜色走一个角色槽）----
    public const int RoleBackground = 0;
    public const int RoleForeground = 1;
    public const int RoleBorder = 2;
    public const int RoleFocusRing = 3;
    public const int RoleAccent = 4;

    // ---- 平行槽位表（对齐 DataRow 平行 List 模式）----
    private static List<long> _handle = new List<long>();
    private static List<int> _role = new List<int>();
    private static List<int> _active = new List<int>();
    private static List<double> _fromR = new List<double>();
    private static List<double> _fromG = new List<double>();
    private static List<double> _fromB = new List<double>();
    private static List<double> _fromA = new List<double>();
    private static List<double> _targetR = new List<double>();
    private static List<double> _targetG = new List<double>();
    private static List<double> _targetB = new List<double>();
    private static List<double> _targetA = new List<double>();
    private static List<long> _startTick = new List<long>();
    private static List<double> _durationMs = new List<double>();

    // ---- double 插值槽位表（布局/几何动画：Opacity/Width/Height/FontSize/CornerRadius 等）----
    // 与颜色槽位表独立（role 空间不冲突），同一 (handle, role) 可同时在颜色和 double 两表活跃。
    private static List<long> _dHandle = new List<long>();
    private static List<int> _dRole = new List<int>();
    private static List<int> _dActive = new List<int>();
    private static List<double> _dFrom = new List<double>();
    private static List<double> _dTarget = new List<double>();
    private static List<long> _dStartTick = new List<long>();
    private static List<double> _dDurationMs = new List<double>();

    // ---- 动画完成回调独立表（与槽位表解耦，避免槽位 Add/Begin 改动）----
    // 按 (handle, role) 注册；动画完成时查找并触发，触发后标记 fired 避免重复。
    private static List<long> _cbHandle = new List<long>();
    private static List<int> _cbRole = new List<int>();
    private static List<Action> _cbCallback = new List<Action>();
    private static List<int> _cbFired = new List<int>();
    private static List<long> _dCbHandle = new List<long>();
    private static List<int> _dCbRole = new List<int>();
    private static List<Action> _dCbCallback = new List<Action>();
    private static List<int> _dCbFired = new List<int>();

    // ===== 颜色插值 API =====

    /// <summary>
    /// 解析一个角色色的当前显示值（含过渡插值）。
    /// <paramref name="target"/> 为渲染器读到的终态色字符串（hex 或命名色）。
    /// 返回插值后的 <see cref="Color"/>（sRGB 分量）；无过渡时等价于目标色。
    /// </summary>
    public static Color ResolveColor(long handle, int role, string target) {
        return ResolveColorCore(handle, role, target, DurationMs(role));
    }

    /// <summary>
    /// 带显式过渡时长（VSM 每状态 motion 覆写）的解析。
    /// <paramref name="durationMs"/> 为负时回退角色默认时长（<see cref="ResolveColor"/> 语义）。
    /// </summary>
    public static Color ResolveColorDur(long handle, int role, string target, double durationMs) {
        if (!(durationMs >= 0.0)) {
            durationMs = DurationMs(role);
        }
        return ResolveColorCore(handle, role, target, durationMs);
    }

    /// <summary>过渡插值核心（时长可由调用方覆写；0 表示无过渡瞬达终态）。</summary>
    private static Color ResolveColorCore(long handle, int role, string target, double durationMs) {
        if (handle == (long)0) {
            return Color.Transparent();
        }
        if (target == null) {
            return Color.Transparent();
        }
        if (target.Length == 0) {
            return Color.Transparent();
        }
        // 目标色经 Arc.UI.Media.Color 类型化解析（唯一解析入口；sRGB 分量）。
        Color tc = Color.Parse(target);
        double tr = tc.R;
        double tg = tc.G;
        double tb = tc.B;
        double ta = tc.A;

        int i = FindColorSlot(handle, role);
        if (i == -1) {
            AddColorSlot(handle, role, tr, tg, tb, ta);
            return tc;
        }

        if (_active[i] == 0) {
            if (!SameColor(_targetR[i], _targetG[i], _targetB[i], _targetA[i], tr, tg, tb, ta)) {
                BeginTransition(i, tr, tg, tb, ta, durationMs);
                if (durationMs <= 0.0) {
                    _active[i] = 0;
                    FireColorComplete(handle, role);
                }
            }
            return tc;
        }

        long nowTick = Stopwatch.GetTimestamp();
        double elapsedMs = ElapsedMs(_startTick[i], nowTick);

        if (!SameColor(_targetR[i], _targetG[i], _targetB[i], _targetA[i], tr, tg, tb, ta)) {
            double eased = Ease(Clamp01(elapsedMs / _durationMs[i]));
            BeginTransitionFrom(i,
                Interp(_fromR[i], _targetR[i], eased),
                Interp(_fromG[i], _targetG[i], eased),
                Interp(_fromB[i], _targetB[i], eased),
                Interp(_fromA[i], _targetA[i], eased),
                tr, tg, tb, ta, durationMs);
        }

        if (elapsedMs >= _durationMs[i]) {
            _active[i] = 0;
            FireColorComplete(handle, role);
            return tc;
        }

        double et = Ease(Clamp01(elapsedMs / _durationMs[i]));
        return Color.FromRgba(
            Interp(_fromR[i], _targetR[i], et),
            Interp(_fromG[i], _targetG[i], et),
            Interp(_fromB[i], _targetB[i], et),
            Interp(_fromA[i], _targetA[i], et));
    }

    // ===== double 插值 API（布局/几何动画）=====
    // 消费场景：Opacity 透明度过渡、Width/Height 尺寸动画、FontSize 字号过渡、
    //           CornerRadius 圆角过渡、OffsetX/OffsetY 位移动画。

    /// <summary>
    /// 解析一个 double 角色的当前显示值（含过渡插值）。
    /// <paramref name="target"/> 为调用方读到的终态值。
    /// 返回插值后的值；无过渡时等价于 <paramref name="target"/>。
    /// </summary>
    public static double ResolveDouble(long handle, int role, double target) {
        return ResolveDoubleCore(handle, role, target, DurationMs(role));
    }

    /// <summary>
    /// 带显式过渡时长的 double 解析。
    /// <paramref name="durationMs"/> 为负时回退角色默认时长。
    /// </summary>
    public static double ResolveDoubleDur(long handle, int role, double target, double durationMs) {
        if (!(durationMs >= 0.0)) {
            durationMs = DurationMs(role);
        }
        return ResolveDoubleCore(handle, role, target, durationMs);
    }

    /// <summary>double 过渡插值核心（与 ResolveColorCore 对称，单通道）。</summary>
    private static double ResolveDoubleCore(long handle, int role, double target, double durationMs) {
        if (handle == (long)0) {
            return target;
        }

        int i = FindDoubleSlot(handle, role);
        if (i == -1) {
            AddDoubleSlot(handle, role, target);
            return target;
        }

        if (_dActive[i] == 0) {
            if (_dTarget[i] != target) {
                BeginDoubleTransition(i, target, target, durationMs);
                if (durationMs <= 0.0) {
                    _dActive[i] = 0;
                    FireDoubleComplete(handle, role);
                }
            }
            return target;
        }

        long nowTick = Stopwatch.GetTimestamp();
        double elapsedMs = ElapsedMs(_dStartTick[i], nowTick);

        if (_dTarget[i] != target) {
            double eased = Ease(Clamp01(elapsedMs / _dDurationMs[i]));
            BeginDoubleTransition(i, Interp(_dFrom[i], _dTarget[i], eased), target, durationMs);
        }

        if (elapsedMs >= _dDurationMs[i]) {
            _dActive[i] = 0;
            FireDoubleComplete(handle, role);
            return target;
        }

        double et = Ease(Clamp01(elapsedMs / _dDurationMs[i]));
        return Interp(_dFrom[i], _dTarget[i], et);
    }

    // ===== 动画完成回调 API =====
    // 调用方在动画开始前/开始后注册；动画完成时（elapsedMs >= durationMs）触发一次。
    // Storyboard 注册多个回调实现"全部完成"语义；SequentialChain 注册回调实现"完成触发下一步"。

    /// <summary>注册颜色动画完成回调（同一 (handle, role) 重复注册则覆写）。</summary>
    internal static void OnColorComplete(long handle, int role, Action callback) {
        int count = _cbHandle.Count;
        for (int i = 0; i < count; i++) {
            if (_cbHandle[i] == handle) {
                if (_cbRole[i] == role) {
                    _cbCallback[i] = callback;
                    _cbFired[i] = 0;
                    return;
                }
            }
        }
        _cbHandle.Add(handle);
        _cbRole.Add(role);
        _cbCallback.Add(callback);
        _cbFired.Add(0);
    }

    /// <summary>注册 double 动画完成回调（同一 (handle, role) 重复注册则覆写）。</summary>
    internal static void OnDoubleComplete(long handle, int role, Action callback) {
        int count = _dCbHandle.Count;
        for (int i = 0; i < count; i++) {
            if (_dCbHandle[i] == handle) {
                if (_dCbRole[i] == role) {
                    _dCbCallback[i] = callback;
                    _dCbFired[i] = 0;
                    return;
                }
            }
        }
        _dCbHandle.Add(handle);
        _dCbRole.Add(role);
        _dCbCallback.Add(callback);
        _dCbFired.Add(0);
    }

    /// <summary>颜色动画完成时触发回调（由 ResolveColorCore 调用）。</summary>
    private static void FireColorComplete(long handle, int role) {
        int count = _cbHandle.Count;
        for (int i = 0; i < count; i++) {
            if (_cbHandle[i] == handle) {
                if (_cbRole[i] == role) {
                    if (_cbFired[i] == 0) {
                        _cbFired[i] = 1;
                        Action cb = _cbCallback[i];
                        if (cb != null) {
                            cb();
                        }
                        return;
                    }
                }
            }
        }
    }

    /// <summary>double 动画完成时触发回调（由 ResolveDoubleCore 调用）。</summary>
    private static void FireDoubleComplete(long handle, int role) {
        int count = _dCbHandle.Count;
        for (int i = 0; i < count; i++) {
            if (_dCbHandle[i] == handle) {
                if (_dCbRole[i] == role) {
                    if (_dCbFired[i] == 0) {
                        _dCbFired[i] = 1;
                        Action cb = _dCbCallback[i];
                        if (cb != null) {
                            cb();
                        }
                        return;
                    }
                }
            }
        }
    }

    // ===== 槽位管理 =====

    /// <summary>查找颜色槽位索引（未找到返回 -1）。</summary>
    private static int FindColorSlot(long handle, int role) {
        int count = _handle.Count;
        for (int i = 0; i < count; i++) {
            if (_handle[i] != handle) {
                continue;
            }
            if (_role[i] != role) {
                continue;
            }
            return i;
        }
        return -1;
    }

    /// <summary>查找 double 槽位索引（未找到返回 -1）。</summary>
    private static int FindDoubleSlot(long handle, int role) {
        int count = _dHandle.Count;
        for (int i = 0; i < count; i++) {
            if (_dHandle[i] != handle) {
                continue;
            }
            if (_dRole[i] != role) {
                continue;
            }
            return i;
        }
        return -1;
    }

    /// <summary>新增颜色槽位（首现无过渡，记录终态空闲直返）。</summary>
    private static void AddColorSlot(long handle, int role, double tr, double tg, double tb, double ta) {
        _handle.Add(handle);
        _role.Add(role);
        _active.Add(0);
        _fromR.Add(tr);
        _fromG.Add(tg);
        _fromB.Add(tb);
        _fromA.Add(ta);
        _targetR.Add(tr);
        _targetG.Add(tg);
        _targetB.Add(tb);
        _targetA.Add(ta);
        _startTick.Add((long)0);
        _durationMs.Add(0.0);
    }

    /// <summary>新增 double 槽位（首现无过渡，记录终态空闲直返）。</summary>
    private static void AddDoubleSlot(long handle, int role, double value) {
        _dHandle.Add(handle);
        _dRole.Add(role);
        _dActive.Add(0);
        _dFrom.Add(value);
        _dTarget.Add(value);
        _dStartTick.Add((long)0);
        _dDurationMs.Add(0.0);
    }

    /// <summary>开启/重定向颜色过渡：from = target（首启或目标未变）。</summary>
    private static void BeginTransition(int i, double tr, double tg, double tb, double ta, double durationMs) {
        BeginTransitionFrom(i, tr, tg, tb, ta, tr, tg, tb, ta, durationMs);
    }

    /// <summary>重定向颜色过渡：from = 当前显示色，target = 新目标。</summary>
    private static void BeginTransitionFrom(int i, double fr, double fg, double fb, double fa,
                                             double tr, double tg, double tb, double ta, double durationMs) {
        long nowTick = Stopwatch.GetTimestamp();
        _active[i] = 1;
        _fromR[i] = fr;
        _fromG[i] = fg;
        _fromB[i] = fb;
        _fromA[i] = fa;
        _targetR[i] = tr;
        _targetG[i] = tg;
        _targetB[i] = tb;
        _targetA[i] = ta;
        _startTick[i] = nowTick;
        _durationMs[i] = durationMs;
    }

    /// <summary>开启/重定向 double 过渡：from→target。</summary>
    private static void BeginDoubleTransition(int i, double fromValue, double target, double durationMs) {
        long nowTick = Stopwatch.GetTimestamp();
        _dActive[i] = 1;
        _dFrom[i] = fromValue;
        _dTarget[i] = target;
        _dStartTick[i] = nowTick;
        _dDurationMs[i] = durationMs;
    }

    // ===== 状态查询/重置 =====

    /// <summary>是否有过渡进行中（颜色或 double；FramePump 据此保持每帧渲染）。</summary>
    public static bool Active() {
        int colorCount = _handle.Count;
        for (int i = 0; i < colorCount; i++) {
            if (_active[i] != 0) {
                return true;
            }
        }
        int doubleCount = _dHandle.Count;
        for (int i = 0; i < doubleCount; i++) {
            if (_dActive[i] != 0) {
                return true;
            }
        }
        return false;
    }

    /// <summary>清空全部过渡槽（颜色 + double + 回调；Window 每 Show 前调用）。</summary>
    public static void Reset() {
        _handle.Clear();
        _role.Clear();
        _active.Clear();
        _fromR.Clear();
        _fromG.Clear();
        _fromB.Clear();
        _fromA.Clear();
        _targetR.Clear();
        _targetG.Clear();
        _targetB.Clear();
        _targetA.Clear();
        _startTick.Clear();
        _durationMs.Clear();
        _dHandle.Clear();
        _dRole.Clear();
        _dActive.Clear();
        _dFrom.Clear();
        _dTarget.Clear();
        _dStartTick.Clear();
        _dDurationMs.Clear();
        _cbHandle.Clear();
        _cbRole.Clear();
        _cbCallback.Clear();
        _cbFired.Clear();
        _dCbHandle.Clear();
        _dCbRole.Clear();
        _dCbCallback.Clear();
        _dCbFired.Clear();
    }

    // ===== 缓动函数 =====

    /// <summary>标准 ease-out 缓动（RFC 037 §3.6 Motion.Easing.Standard → cubic-bezier 近似）。</summary>
    public static double Ease(double t) {
        if (t <= 0.0) {
            return 0.0;
        }
        if (t >= 1.0) {
            return 1.0;
        }
        // 1 - (1-t)^3：ease-out cubic，快入缓出。
        double u = 1.0 - t;
        return 1.0 - u * u * u;
    }

    /// <summary>线性缓动（匀速过渡；RFC 037 §3.6 备选 easing）。</summary>
    public static double EaseLinear(double t) {
        if (t <= 0.0) {
            return 0.0;
        }
        if (t >= 1.0) {
            return 1.0;
        }
        return t;
    }

    /// <summary>ease-in cubic 缓动（缓入快出；起步慢、末段加速）。</summary>
    public static double EaseIn(double t) {
        if (t <= 0.0) {
            return 0.0;
        }
        if (t >= 1.0) {
            return 1.0;
        }
        // t^3：ease-in cubic，缓入快出。
        return t * t * t;
    }

    /// <summary>ease-in-out cubic 缓动（两端缓、中段快；对称过渡）。</summary>
    public static double EaseInOut(double t) {
        if (t <= 0.0) {
            return 0.0;
        }
        if (t >= 1.0) {
            return 1.0;
        }
        // 前半 4*t^3（ease-in），后半 1 - (-2t+2)^3/2（ease-out）。
        if (t >= 0.5) {
            double u = -2.0 * t + 2.0;
            return 1.0 - (u * u * u) / 2.0;
        }
        return 4.0 * t * t * t;
    }

    // ===== 数值工具 =====

    /// <summary>单通道线性插值：fromValue + (target - fromValue) * eased。</summary>
    private static double Interp(double fromValue, double target, double eased) {
        return fromValue + (target - fromValue) * eased;
    }

    /// <summary>钳制到 [0.0, 1.0]（elapsedMs/durationMs 恒非负，仅上界判断）。</summary>
    private static double Clamp01(double v) {
        if (v >= 1.0) {
            return 1.0;
        }
        return v;
    }

    /// <summary>两个计时器 tick 间的流逝毫秒（单调）。</summary>
    private static double ElapsedMs(long startTick, long nowTick) {
        long freq = Stopwatch.Frequency;
        if (freq <= 0) {
            return 0.0;
        }
        return (double)(nowTick - startTick) * 1000.0 / (double)freq;
    }

    /// <summary>角色默认时长（RFC 037 §3.6：Fast 120 / Normal 160，经 Application.Current 运行时解析）。</summary>
    private static double DurationMs(int role) {
        if (role == RoleFocusRing) {
            return Application.Current.ResolveNumber(BuiltInTheme.MotionDurationFast);
        }
        return Application.Current.ResolveNumber(BuiltInTheme.MotionDurationNormal);
    }

    private static bool SameColor(double r1, double g1, double b1, double a1,
                                  double r2, double g2, double b2, double a2) {
        double eps = 0.002;  // ~半字节量化容差（1/255/2）
        if (Abs(r1 - r2) <= eps) {
            if (Abs(g1 - g2) <= eps) {
                if (Abs(b1 - b2) <= eps) {
                    if (Abs(a1 - a2) <= eps) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    private static double Abs(double v) {
        if (v >= 0.0) {
            return v;
        }
        return -v;
    }

}
