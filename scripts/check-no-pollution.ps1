# check-no-pollution.ps1 - source-tree pollution gate (CI)
#
# Authority: .gitignore 遗留调试/复现/测试运行时产物段（2026-08-09 收敛）
#
# 目的：防止调试/复现/测试运行时产物再次被提交进源码树（tracked）或
# 以非忽略状态堆积在源码树（untracked）。与 clean-debug-artifacts.ps1
# （清除工作树）互补：本脚本在 CI / 手工运行时检查 git 索引与工作树，发现即失败。
#
# 治理机制（2026-08-10 收敛）：已移除 .cursor/hooks/deny-repo-pollution.ps1
# （beforeShellExecution 拦截）与 .gitignore 对根目录调试产物的掩盖段。
# 根目录调试 log/txt 等不再被 .gitignore 隐藏，故此处 untracked 检测
# （git ls-files --others --exclude-standard）能在未忽略状态诚实暴露污染。
#
# 用法（仓库根）：
#   pwsh scripts/check-no-pollution.ps1
#
# 退出码：
#   0 = 干净（无污染）
#   1 = 发现污染（逐项打印）
#
# 注：vendored 本地库目录（crates/runtime-*/bin/、wgpu-native/bin/ 等）内的
# .dll/.lib/.a/.o 属合法资产，不在此门禁范围内（见 .gitignore 豁免）。

param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

function Test-PollutionPath {
    param([string]$Path)
    $p = $Path -replace '\\', '/'
    if ($p -match '^_repro(/|$)') { return $true }                            # 一次性复现目录
    if ($p -match '^debug-[A-Za-z0-9_.-]+\.md$') { return $true }             # 根目录调试会话备忘
    if ($p -match '^[A-Za-z0-9_.-]+\.ps1$') { return $true }                  # 根目录散落脚本（应归 scripts/）
    if ($p -match '(^|/)seek\.bin$') { return $true }                         # e2e 测试运行时二进制
    # 已跟踪的测试二进制/日志（排除 vendored runtime 本地库资产）
    if ($p -match '(^|/)[A-Za-z0-9_.-]+\.(bin|exe|log)$' -and $p -notmatch '^crates/runtime-(crypto|quic|ui)/') { return $true }
    if ($p -match '(^|/)\.tmp_') { return $true }                             # .tmp_* 写入污染
    if ($p -match '(^|/)tmp_[A-Za-z0-9_.-]*(\.[a-z]+)?$') { return $true }    # tmp_* 写入污染
    if ($p -match '(^|/)obj-[A-Za-z0-9_.-]+(/|$)') { return $true }           # 非标准 obj-* 目录
    if ($p -match '(^|/)bin-[A-Za-z0-9_.-]+(/|$)') { return $true }           # 非标准 bin-* 目录
    if ($p -match '(^|/)target-h[0-9]+(/|$)') { return $true }                # 根 target-h* 目录
    if ($p -match '^target-[A-Za-z0-9_.-]+(/|$)') { return $true }            # 根 target-* 目录
    if ($p -match '^(test_|tmp_)[A-Za-z0-9_.-]*\.as$') { return $true }       # 根一次性测试源码
    if ($p -match '^(stderr|stdout|err|out)\.txt$') { return $true }          # 根调试重定向
    if ($p -match '(^|/)\.tmp-[A-Za-z0-9_.-]+(/|$)') { return $true }         # .tmp-* 目录
    return $false
}

$violations = [System.Collections.Generic.List[string]]::new()

# 1) tracked：已被提交进 git 索引的污染（最严重——必须清理后提交）
foreach ($f in (git ls-files)) {
    if (Test-PollutionPath -Path $f) {
        $violations.Add("tracked: $f")
    }
}

# 2) untracked 非忽略：工作树中堆积、且未被 .gitignore 忽略的污染
foreach ($f in (git ls-files --others --exclude-standard)) {
    if (Test-PollutionPath -Path $f) {
        $violations.Add("untracked: $f")
    }
}

# 3) 根目录散落 .ps1（无 git 依赖的独立扫描）：被 `*.ps1` 忽略故不走索引，
#    但散落根目录即污染（规范：脚本一律归 scripts/）。见 arc-workspace-hygiene.mdc「脚本归属」。
Get-ChildItem -LiteralPath $RepoRoot -File -Filter '*.ps1' -ErrorAction SilentlyContinue | ForEach-Object {
    $violations.Add("root-script: $($_.Name)")   # 应归入 scripts/（含子目录）
}

if ($violations.Count -gt 0) {
    Write-Host ('Source-tree pollution: ' + $violations.Count + ' violation(s)') -ForegroundColor Red
    foreach ($v in $violations) { Write-Host ('  - ' + $v) }
    Write-Host 'cleanup: pwsh scripts/clean-debug-artifacts.ps1 ; or git rm --cached <path> then commit'
    exit 1
}

Write-Host 'Source-tree pollution: clean (0 violations)'
exit 0
