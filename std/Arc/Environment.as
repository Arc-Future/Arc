// Arc.Environment — 环境信息访问（Phase 1 + Phase 2）。
//
// 提供运行时环境基础能力：命令行参数、环境变量、进程控制、系统信息、
// 当前目录、机器/用户名、平台检测。对标 C# System.Environment + System.OperatingSystem。
//
// Phase 1 (2026-07-20):
//   - ArgCount()            命令行参数总数（含程序名）
//   - GetArg(int)           按索引获取命令行参数（0 = 程序名）
//
// Phase 2 (2026-07-21):
//   - GetEnvironmentVariable(string) / SetEnvironmentVariable(string, string)
//   - Exit(int) / GetExitCode() / SetExitCode(int) / FailFast(string)
//   - NewLine() / ProcessorCount() / Is64BitProcess()
//   - GetCurrentDirectory() / SetCurrentDirectory(string)
//   - MachineName() / UserName()
//   - Platform() / IsWindows() / IsLinux() / IsMacOS() / IsAndroid() / IsIOS() / IsOHOS()
//
// 已撤面（禁假 ABI / 空串 stub；runtime 与 codegen 均无对应符号）：
//   - ProcessId / ProcessPath / ExpandEnvironmentVariables / GetFolderPath
//
// 与 C# 的偏离：
//   - C# Environment.GetCommandLineArgs() 返回 string[]；Arc 当前无托管数组 C ABI，
//     采用索引式访问（ArgCount + GetArg(index)）作为过渡方案。
//   - C# 使用静态属性（NewLine / ExitCode / CurrentDirectory / ProcessorCount 等）；
//     Arc 当前无静态属性机制，统一以方法形式提供。
//   - C# SetEnvironmentVariable(name, null) 等价于删除；Arc 同样语义——
//     传入空串或 null 视为删除该环境变量。
//   - C# GetEnvironmentVariable 未设置返回 null；Arc 返回空串（C ABI 不返回 NULL）。
//   - C# FailFast 携带 exception object 用于诊断日志；Arc 仅接收消息字符串。
//   - C# 平台检测通过 OperatingSystem 对象；Arc 以 bool 方法 + 字符串标识符直接提供。

namespace Arc;

/// <summary>
/// 运行时环境信息访问（Phase 1 + Phase 2）。
///
/// 提供：
/// - <b>命令行参数</b>：通过 <see cref="ArgCount"/> 与 <see cref="GetArg"/> 索引式访问
///   （0 = 程序名，与 C 的 argv 一致）。
/// - <b>环境变量</b>：通过 <see cref="GetEnvironmentVariable"/> 读取、
///   <see cref="SetEnvironmentVariable"/> 写入或删除。
/// - <b>进程控制</b>：通过 <see cref="Exit"/> 终止进程，<see cref="GetExitCode"/> /
///   <see cref="SetExitCode"/> 读写退出码，<see cref="FailFast"/> 立即中止（不运行 finally）。
/// - <b>系统信息</b>：通过 <see cref="NewLine"/> 获取平台换行符，
///   <see cref="ProcessorCount"/> 获取 CPU 核数，
///   <see cref="Is64BitProcess"/> 判断进程位宽。
///   （间隔/性能计时见 <c>Arc.Diagnostics.Stopwatch</c>；单一惯用法，不再经 Environment。）
/// - <b>当前目录</b>：通过 <see cref="GetCurrentDirectory"/> 读取、
///   <see cref="SetCurrentDirectory"/> 修改。
/// - <b>机器/用户</b>：通过 <see cref="MachineName"/> / <see cref="UserName"/> 获取标识。
/// - <b>平台检测</b>：对标 C# <c>OperatingSystem</c>，通过
///   <see cref="IsWindows"/> / <see cref="IsLinux"/> / <see cref="IsMacOS"/> /
///   <see cref="IsAndroid"/> / <see cref="IsIOS"/> / <see cref="IsOHOS"/>
///   做 bool 条件分支；通过 <see cref="Platform"/> 获取字符串标识符。
///
/// **C ABI 映射**（声明于 rt_abi.h，实现于 rt_env.c）：
///   - ArgCount()             → rt_env_argc()
///   - GetArg(i)              → rt_env_argv(i)
///   - GetEnvironmentVariable → rt_env_get_var(name)
///   - SetEnvironmentVariable → rt_env_set_var(name, value)
///   - Exit                   → rt_env_exit(code)
///   - GetExitCode/SetExitCode→ rt_env_get_exit_code / rt_env_set_exit_code
///   - FailFast               → rt_env_fail_fast(msg)
///   - NewLine                → rt_env_newline()
///   - ProcessorCount         → rt_env_processor_count()
///   - Is64BitProcess         → rt_env_is_64bit_process()
///   - GetCurrentDirectory    → rt_env_get_cwd()
///   - SetCurrentDirectory    → rt_env_set_cwd(path)
///   - MachineName            → rt_env_machine_name()
///   - UserName               → rt_env_user_name()
///   - Platform               → rt_env_platform()
///   - IsWindows/IsLinux/...  → rt_env_is_windows/linux/macos/android/ios/ohos()
///
/// **用法**：
/// <code>
/// // 读取环境变量
/// string path = Environment.GetEnvironmentVariable("PATH");
///
/// // 跨平台换行
/// Console.Write("line1" + Environment.NewLine() + "line2");
///
/// // 退出码
/// Environment.SetExitCode(42);
/// Environment.Exit(Environment.GetExitCode());
///
/// // 平台条件分支（对标 C# OperatingSystem）
/// if (Environment.IsWindows()) {
///     Console.Write("Windows-specific logic");
/// } else if (Environment.IsLinux()) {
///     Console.Write("Linux-specific logic");
/// } else if (Environment.IsAndroid()) {
///     Console.Write("Android-specific logic");
/// }
/// </code>
/// </summary>
public static class Environment {
    // ── Phase 1：命令行参数 ──

    /// <summary>
    /// 获取命令行参数总数（含程序名，即 argv[0]）。
    ///
    /// codegen 拦截并发射 <c>rt_env_argc()</c> ABI。
    /// </summary>
    /// <returns>参数总数；无参数时返回 0。</returns>
    [Builtin(ABI = "rt_env_argc")]
    public static int ArgCount() { return 0; }

    /// <summary>
    /// 按索引获取命令行参数。
    ///
    /// codegen 拦截并发射 <c>rt_env_argv(index)</c> ABI。
    /// </summary>
    /// <param name="index">参数索引（0 = 程序名）。</param>
    /// <returns>参数值；索引越界时返回空串。</returns>
    [Builtin(ABI = "rt_env_argv")]
    public static string GetArg(int index) { return ""; }

    // ── Phase 2：环境变量 ──

    /// <summary>
    /// 检索环境变量的值。
    ///
    /// codegen 拦截并发射 <c>rt_env_get_var(name)</c> ABI。
    /// 与 C# 的偏离：未设置时返回空串而非 null（C ABI 不返回 NULL）。
    /// </summary>
    /// <param name="name">环境变量名（区分大小写由平台决定：Windows 不区分，POSIX 区分）。</param>
    /// <returns>变量值；未设置时返回空串。</returns>
    [Builtin(ABI = "rt_env_get_var")]
    public static string GetEnvironmentVariable(string name) { return ""; }

    /// <summary>
    /// 创建、修改或删除环境变量。
    ///
    /// codegen 拦截并发射 <c>rt_env_set_var(name, value)</c> ABI。
    /// 与 C# 行为一致：<paramref name="value"/> 为 null 或空串时删除该变量。
    /// </summary>
    /// <param name="name">环境变量名。</param>
    /// <param name="value">变量值；null 或空串表示删除。</param>
    /// <returns>1=成功，0=失败（如名称非法）。</returns>
    [Builtin(ABI = "rt_env_set_var")]
    public static int SetEnvironmentVariable(string name, string value) { return 0; }

    // ── Phase 2：进程控制 ──

    /// <summary>
    /// 终止当前进程并返回指定的退出码给操作系统。
    ///
    /// codegen 拦截并发射 <c>rt_env_exit(code)</c> ABI。
    /// 此方法不返回——调用后进程立即终止，finally 块不会执行。
    /// </summary>
    /// <param name="exitCode">进程退出码。</param>
    [Builtin(ABI = "rt_env_exit")]
    public static void Exit(int exitCode) { }

    /// <summary>
    /// 获取进程退出码。进程正常退出时若未通过 <see cref="Exit"/> 显式设置，
    /// 默认为 0。
    ///
    /// codegen 拦截并发射 <c>rt_env_get_exit_code()</c> ABI。
    /// </summary>
    /// <returns>当前进程退出码。</returns>
    [Builtin(ABI = "rt_env_get_exit_code")]
    public static int GetExitCode() { return 0; }

    /// <summary>
    /// 设置进程退出码。供 main 返回时使用。
    ///
    /// codegen 拦截并发射 <c>rt_env_set_exit_code(code)</c> ABI。
    /// </summary>
    /// <param name="exitCode">要设置的退出码。</param>
    [Builtin(ABI = "rt_env_set_exit_code")]
    public static void SetExitCode(int exitCode) { }

    /// <summary>
    /// 立即终止进程，不执行 finally 块也不展开栈。
    ///
    /// codegen 拦截并发射 <c>rt_env_fail_fast(msg)</c> ABI。
    /// 消息会输出到 stderr 后调用 <c>abort()</c>。
    /// </summary>
    /// <param name="message">终止前的诊断消息。</param>
    [Builtin(ABI = "rt_env_fail_fast")]
    public static void FailFast(string message) { }

    // ── Phase 2：系统信息 ──

    /// <summary>
    /// 获取当前平台的新行字符串（Windows 为 <c>"\r\n"</c>，POSIX 为 <c>"\n"</c>）。
    ///
    /// codegen 拦截并发射 <c>rt_env_newline()</c> ABI。返回静态常量，无需释放。
    /// </summary>
    /// <returns>平台换行符字符串。</returns>
    [Builtin(ABI = "rt_env_newline")]
    public static string NewLine() { return "\n"; }

    /// <summary>
    /// 获取当前进程可用的 CPU 核数。
    ///
    /// codegen 拦截并发射 <c>rt_env_processor_count()</c> ABI。
    /// </summary>
    /// <returns>处理器数；至少为 1。</returns>
    [Builtin(ABI = "rt_env_processor_count")]
    public static int ProcessorCount() { return 1; }

    /// <summary>
    /// 判断当前进程是否为 64 位进程。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_64bit_process()</c> ABI。
    /// </summary>
    /// <returns>64 位进程返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_64bit_process")]
    public static bool Is64BitProcess() { return false; }

    // ── Phase 2：当前目录 ──

    /// <summary>
    /// 获取当前工作目录的绝对路径。
    ///
    /// codegen 拦截并发射 <c>rt_env_get_cwd()</c> ABI。
    /// </summary>
    /// <returns>当前目录绝对路径；失败时返回空串。</returns>
    [Builtin(ABI = "rt_env_get_cwd")]
    public static string GetCurrentDirectory() { return ""; }

    /// <summary>
    /// 将当前工作目录设置为指定路径。
    ///
    /// codegen 拦截并发射 <c>rt_env_set_cwd(path)</c> ABI。
    /// </summary>
    /// <param name="path">目标路径（绝对或相对）。</param>
    /// <returns>1=成功，0=失败（路径不存在或无权限）。</returns>
    [Builtin(ABI = "rt_env_set_cwd")]
    public static int SetCurrentDirectory(string path) { return 0; }

    /// <summary>
    /// 获取当前进程可执行文件的绝对路径。
    ///
    /// codegen 拦截并发射 <c>rt_env_self_exe()</c> ABI。
    /// （对标 C# Environment.ProcessPath；此前因无 runtime 实现被裁撤，
    /// RFC 048 M1 跨进程 echo 需自 spawn 而正名落位。）
    /// </summary>
    /// <returns>自身可执行文件绝对路径；不可得（嵌入式等）时返回空串。</returns>
    [Builtin(ABI = "rt_env_self_exe")]
    public static string SelfProcessPath() { return ""; }

    // ── Phase 2：机器名 / 用户名 ──

    /// <summary>
    /// 获取本机的 NetBIOS 名称（Windows）或主机名（POSIX）。
    ///
    /// codegen 拦截并发射 <c>rt_env_machine_name()</c> ABI。
    /// </summary>
    /// <returns>机器名；失败时返回空串。</returns>
    [Builtin(ABI = "rt_env_machine_name")]
    public static string MachineName() { return ""; }

    /// <summary>
    /// 获取当前线程关联的用户名。
    ///
    /// codegen 拦截并发射 <c>rt_env_user_name()</c> ABI。
    /// POSIX 实现优先读取 USER/LOGNAME 环境变量，回退到 getlogin_r。
    /// </summary>
    /// <returns>当前用户名；失败时返回空串。</returns>
    [Builtin(ABI = "rt_env_user_name")]
    public static string UserName() { return ""; }

    // ── Phase 2：平台检测（对标 C# System.OperatingSystem） ──

    /// <summary>
    /// 判断当前运行时是否为 Windows 平台。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_windows()</c> ABI（编译期 `#ifdef _WIN32`）。
    /// </summary>
    /// <returns>Windows 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_windows")]
    public static bool IsWindows() { return false; }

    /// <summary>
    /// 判断当前运行时是否为 Linux 桌面/服务器平台（不含 Android / OHOS）。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_linux()</c> ABI（编译期回退检测）。
    /// </summary>
    /// <returns>标准 Linux 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_linux")]
    public static bool IsLinux() { return false; }

    /// <summary>
    /// 判断当前运行时是否为 macOS 桌面平台。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_macos()</c> ABI（编译期 `__APPLE__ && !TARGET_OS_IPHONE`）。
    /// </summary>
    /// <returns>macOS 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_macos")]
    public static bool IsMacOS() { return false; }

    /// <summary>
    /// 判断当前运行时是否为 Android 平台。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_android()</c> ABI（编译期 `#ifdef __ANDROID__`）。
    /// </summary>
    /// <returns>Android 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_android")]
    public static bool IsAndroid() { return false; }

    /// <summary>
    /// 判断当前运行时是否为 iOS 平台。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_ios()</c> ABI（编译期 `__APPLE__ && TARGET_OS_IPHONE`）。
    /// </summary>
    /// <returns>iOS 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_ios")]
    public static bool IsIOS() { return false; }

    /// <summary>
    /// 判断当前运行时是否为 OpenHarmony 平台。
    ///
    /// codegen 拦截并发射 <c>rt_env_is_ohos()</c> ABI（编译期 `#ifdef __OHOS__`）。
    /// </summary>
    /// <returns>OpenHarmony 平台返回 true，否则返回 false。</returns>
    [Builtin(ABI = "rt_env_is_ohos")]
    public static bool IsOHOS() { return false; }

    /// <summary>
    /// 获取当前运行时平台标识字符串。
    ///
    /// codegen 拦截并发射 <c>rt_env_platform()</c> ABI。
    /// 返回编译期常量（静态存储，无需释放）：<c>"Windows"</c> / <c>"Linux"</c> /
    /// <c>"macOS"</c> / <c>"Android"</c> / <c>"iOS"</c> / <c>"OHOS"</c>。
    /// 适合日志输出；条件分支建议使用 <see cref="IsWindows"/> 等 bool 方法。
    /// </summary>
    /// <returns>平台标识字符串。</returns>
    [Builtin(ABI = "rt_env_platform")]
    public static string Platform() { return ""; }
}
