// M8.3 CodeAct 原生后端（RFC 038 §3.4.2 · M8.3）：编译器运行时编译 ABI 后端。
//
// 把模型生成代码编译为原生单元执行：写临时单文件入口 Program.as → `arc build`
// 编译为原生可执行 → 运行捕获输出。编译与运行均在独立子进程——进程级隔离，
// 绝不在宿主进程内执行任意代码（安全底线对齐脚本后端）。
//
// 单文件入口语义（RFC 034 / AGENTS.md）：`arc build` 以单文件为入口、不加载同目录
// 兄弟文件，故每次执行用独立临时目录。`using Arc` 由编译器 core_arc 自动注入
// （find_core_arc_dir 回退到编译器源码树 std/Arc），无需项目级 Arc path 依赖。
//
// 编译器路径：构造参数 > 环境变量 `ARC_COMPILER`（缺省拒绝，禁静默降级）。
namespace Arc.Agent;
using Arc;
using Arc.Diagnostics;
using Arc.IO;

/// <summary>
/// 原生 CodeAct 后端：经 `arc build` 单文件入口把模型生成代码编译为原生单元后执行。
/// 编译器路径经构造参数或环境变量 <c>ARC_COMPILER</c> 提供。
/// </summary>
public class AINativeCodeActProvider : IAICodeActProvider {
    /// <summary>以编译器路径创建（空串则回退环境变量 ARC_COMPILER）。</summary>
    public AINativeCodeActProvider(string compilerPath) {
        this.Compiler = compilerPath != null ? compilerPath : "";
    }

    /// <summary>编译器（arc）可执行路径。</summary>
    public string Compiler { get; }

    /// <summary>临时工作根目录（缺省系统临时目录）。</summary>
    public string? WorkRoot { get; set; }

    public async Task<AICodeActResult> ExecuteAsync(
        string code,
        Dictionary<string, string?> env,
        long timeoutMs,
        int maxOutputChars,
        CancellationToken cancellationToken) {
        if (cancellationToken.IsCancellationRequested) {
            AICodeActResult pre = new AICodeActResult();
            pre.Cancelled = true;
            pre.Error = "cancelled before start";
            return pre;
        }
        // 编译器路径：构造参数 > 环境变量 ARC_COMPILER（缺省拒绝，禁静默降级）。
        string compiler = this.Compiler != "" ? this.Compiler : Environment.GetEnvironmentVariable("ARC_COMPILER");
        if (compiler == "") {
            return AICodeActResult.Fail("no compiler configured (set ARC_COMPILER or pass compilerPath)");
        }
        long start = Stopwatch.GetTimestamp();

        // 唯一临时工作目录（时间戳命名，规避并发/陈旧冲突）。
        string root = this.WorkRoot != null ? this.WorkRoot : Path.GetTempPath();
        string dir = Path.Combine(root, "arc-codeact-" + Stopwatch.GetTimestamp());
        bool dirOk = Directory.CreateDirectory(dir);
        if (!dirOk) {
            return AICodeActResult.Fail("failed to create temp dir: " + dir);
        }
        string srcPath = Path.Combine(dir, "Program.as");
        string exePath = Path.Combine(dir, "out") + (Environment.IsWindows() ? ".exe" : "");

        // 1. 写源码（单文件入口）与最小清单（arc build 向上查找 arc.toml）。
        bool wrote = File.WriteAllText(srcPath, code != null ? code : "");
        if (!wrote) {
            Directory.Delete(dir);
            return AICodeActResult.Fail("failed to write source: " + srcPath);
        }
        bool wroteManifest = File.WriteAllText(
            Path.Combine(dir, "arc.toml"),
            "[package]\nname = \"codeact\"\nedition = \"1\"\n");
        if (!wroteManifest) {
            Directory.Delete(dir);
            return AICodeActResult.Fail("failed to write manifest");
        }

        // 2. 编译：arc build <src> -o <exe>。
        ProcessStartInfo cpsi = new ProcessStartInfo();
        cpsi.FileName = compiler;
        cpsi.Arguments = "build " + AINativeCodeActProvider.Quote(srcPath)
            + " -o " + AINativeCodeActProvider.Quote(exePath);
        cpsi.RedirectStandardOutput = true;
        cpsi.RedirectStandardError = true;
        cpsi.CreateNoWindow = true;
        AICodeActResult compile = new AICodeActResult();
        bool compileStarted = CodeActProcessRunner.RunCaptured(
            cpsi, null, timeoutMs, maxOutputChars, cancellationToken, compile);
        if (!compileStarted) {
            Directory.Delete(dir);
            return AICodeActResult.Fail("failed to start compiler: " + compiler);
        }
        // 编译失败 = 模型生成代码含错误（返回编译错误信息）。
        if (compile.ExitCode != 0 || compile.TimedOut || compile.Cancelled) {
            Directory.Delete(dir);
            if (compile.TimedOut) {
                compile.Error = "compile timeout after " + timeoutMs + "ms";
            } else if (compile.Cancelled) {
                compile.Error = "cancelled during compile";
            }
            compile.DurationMs = CodeActProcessRunner.ElapsedMs(start);
            return compile;
        }
        if (!File.Exists(exePath)) {
            Directory.Delete(dir);
            return AICodeActResult.Fail("compiler produced no binary: " + exePath);
        }

        // 3. 运行产物，捕获输出（超时/取消 Kill）。
        ProcessStartInfo rpsi = new ProcessStartInfo();
        rpsi.FileName = exePath;
        rpsi.RedirectStandardOutput = true;
        rpsi.RedirectStandardError = true;
        rpsi.CreateNoWindow = true;
        AICodeActResult run = new AICodeActResult();
        bool runStarted = CodeActProcessRunner.RunCaptured(
            rpsi, null, timeoutMs, maxOutputChars, cancellationToken, run);
        Directory.Delete(dir);
        if (!runStarted) {
            return AICodeActResult.Fail("failed to start compiled binary: " + exePath);
        }
        run.DurationMs = CodeActProcessRunner.ElapsedMs(start);
        return run;
    }

    /// <summary>参数加引号（路径含空格时防拆）。</summary>
    private static string Quote(string s) {
        return "\"" + s + "\"";
    }
}
