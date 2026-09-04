<#
.SYNOPSIS
  Arc 规范守卫门禁——AGENTS.md 与 .cursor/rules 编码契约的机器可执行门禁。

.DESCRIPTION
  包装 scripts/spec-guard/spec-guard.cjs（Node 规则引擎，与 DSH 动态插件 arcs-1 同步）。
  无 -Path 时自动检查 git 检出的变更文件；始终执行工作区卫生与目录结构扫描。
  -All 时执行全库扫描（CI 门禁模式：checkout 后 git 无变更，变更文件模式会退化
  为仅卫生/布局扫描、规则引擎零执行——CI 必须用 -All 保证编码契约规则全库生效）。
  error 级违规未清零即退出码 1（可作 CI 门禁）。

.PARAMETER Path
  显式指定检查路径（仓库相对路径或目录），可多个。

.PARAMETER All
  全库扫描：std/crates/docs/scripts/examples 全集（CI 门禁模式）。

.PARAMETER Quick
  快速门禁：仅执行 error 级规则（warning/info 跳过）。

.PARAMETER Json
  输出原始 JSON（供其他工具消费）。

.EXAMPLE
  scripts/check-spec.ps1
  scripts/check-spec.ps1 -Path std/Orm -Quick
  scripts/check-spec.ps1 -All
#>
param(
  [string[]]$Path = @(),
  [switch]$All,
  [switch]$Quick,
  [switch]$Json
)

$ErrorActionPreference = 'Stop'
$cjs = Join-Path $PSScriptRoot 'spec-guard\spec-guard.cjs'
$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node) {
  Write-Error 'spec-guard 需要 Node.js（规则引擎为 .cjs）——请先安装 Node.js。'
}

$runArgs = @('check')
if ($Quick) { $runArgs += '--quick' }
if ($All) { $runArgs += '--all' }
if ($Path.Count -eq 0 -and -not $All) {
  # PS 层先行解析 git 变更（Node spawn 在部分受限环境不可用，此处与 cjs 解析逻辑保持一致）
  $status = git -c core.quotepath=false status --porcelain 2>$null
  if ($LASTEXITCODE -eq 0 -and $status) {
    $Path = @($status | ForEach-Object {
        # porcelain v1：`XY SP path`，路径固定从索引 3 开始；不可先 Trim 行首（. 开头的路径会丢点）
        $t = $_ -replace "`r$", ''
        if ($t.Trim()) {
          $code = $t.Substring(0, 2)
          if ($code -match 'D') { return }   # 已删除文件无需检查
          $p = if ($t.Length -gt 3) { $t.Substring(3) } else { $t.Substring(2) }
          if ($p -match ' -> ') { $p = ($p -split ' -> ')[-1] }
          $p = $p.Trim('"') -replace '\\', '/'
          $p = $p.Trim()
          $p
        }
      } | Where-Object { $_ })
  }
}
if ($Path.Count -gt 0) { $runArgs += $Path }
$runArgs += '--json'

$stdout = & $node.Source $cjs @runArgs 2>$null
$runExit = $LASTEXITCODE
if ($runExit -ne 0 -and $runExit -ne 1) {
  Write-Error "spec-guard 运行失败（exit $runExit）——请检查 Node.js 与脚本完整性。"
}

$r = $stdout | Out-String | ConvertFrom-Json
if ($Json) {
  Write-Output $stdout
  exit $r.exitCode
}

$verdict = if ($r.passed) { '✅ 通过' } else { '❌ 未通过' }
Write-Host ''
Write-Host ("# Arc 规范守卫 — {0}   error={1}  warning={2}  info={3}{4}" -f $verdict, $r.counts.error, $r.counts.warning, $r.counts.info, $(if ($r.quick) { '  （快速门禁：仅 error 级）' } else { '' }))
if ($r.note) { Write-Host ("> " + $r.note) }
if ($r.exemptions.Count -gt 0) {
  Write-Host ("豁免：" + (($r.exemptions | ForEach-Object { $_.id + '@' + $_.file }) -join '；'))
}

$errs = @($r.violations | Where-Object { $_.severity -eq 'error' })
$warns = @($r.violations | Where-Object { $_.severity -eq 'warning' })
$infos = @($r.violations | Where-Object { $_.severity -eq 'info' })

if ($errs.Count -gt 0) {
  Write-Host ''
  Write-Host ("## ERROR ({0})" -f $errs.Count)
  foreach ($v in $errs) { Write-Host ("- [{0}] {1}:{2} — {3}：{4}" -f $v.id, $v.file, $v.line, $v.rule, $v.message) }
}
if ($warns.Count -gt 0) {
  Write-Host ''
  Write-Host ("## WARNING ({0})" -f $warns.Count)
  $shown = 0
  foreach ($v in $warns) {
    if ($shown -ge 30) { break }
    Write-Host ("- [{0}] {1}:{2} — {3}：{4}" -f $v.id, $v.file, $v.line, $v.rule, $v.message)
    $shown++
  }
  if ($warns.Count -gt $shown) { Write-Host ("… 其余 {0} 条 warning，用 -Json 查看完整结果" -f ($warns.Count - $shown)) }
}
if ($infos.Count -gt 0) {
  Write-Host ''
  Write-Host ("## INFO ({0})" -f $infos.Count)
  foreach ($v in $infos | Select-Object -First 10) { Write-Host ("- [{0}] {1} — {2}" -f $v.id, $v.file, $v.message) }
}
if ($r.unreadable.Count -gt 0) {
  Write-Host ''
  Write-Host ("## 无法读取：" + ($r.unreadable -join ', '))
}
if ($r.verification.Count -gt 0) {
  Write-Host ''
  Write-Host '## 验证矩阵（arc-core）'
  foreach ($c in $r.verification) { Write-Host ("- {0} — {1}" -f $c.command, $c.reason) }
}
Write-Host ''
Write-Host ("exit code = {0}（error>0 即门禁失败，不得合入/推送）" -f $r.exitCode)
Write-Host ''
exit $r.exitCode
