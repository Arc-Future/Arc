# git-sync.ps1 - 任务完成后自动提交并推送远程主仓库（origin）
#
# 用途：Agent / 开发者完成一项验收任务后，将当前工作树改动原子地
#       提交（Conventional Commits 风格）并推送到远程。
#
# 行为：
#   1. git add -A
#   2. 无任何变更 -> 跳过提交与推送，退出 0（幂等，不产生空提交）
#   3. 有变更 -> git commit -m <Message>（消息必须显式提供）
#   4. git push origin HEAD（当前分支推到远程同名分支；在 main 上即 origin/main）
#
# 安全：
#   - 绝不 --force；绝不 --amend；绝不改动 git config
#   - 提交消息必须由调用方显式提供（不自动生成），避免低质量历史
#   - 提交或推送失败即非零退出并暴露错误（不静默）
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\git-sync.ps1 -Message "feat(arc): ..."
#   pwsh scripts/git-sync.ps1 -Message "fix(codegen): ..." -NoPush

param(
    [string]$Message = '',
    [switch]$NoPush
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

# --- 1. stage everything ---
git add -A
if ($LASTEXITCODE -ne 0) {
    Write-Error "git-sync: git add failed"
    exit 1
}

# --- 2. nothing to commit -> idempotent clean exit ---
$porcelain = git status --porcelain
if (-not $porcelain) {
    Write-Host "git-sync: working tree clean, nothing to commit/push"
    exit 0
}

# --- 3. commit (message is mandatory when there are changes) ---
if ([string]::IsNullOrWhiteSpace($Message)) {
    Write-Error "git-sync: -Message is required when there are changes (Conventional Commits style)"
    exit 1
}
git commit -m $Message
if ($LASTEXITCODE -ne 0) {
    Write-Error "git-sync: git commit failed (pre-commit hook?)"
    exit 1
}

# --- 4. push current branch to origin ---
if (-not $NoPush) {
    git push origin HEAD
    if ($LASTEXITCODE -ne 0) {
        Write-Error "git-sync: git push failed"
        exit 1
    }
}

$shortHead = git rev-parse --short HEAD
if ($NoPush) {
    Write-Host "git-sync: committed $shortHead (push skipped via -NoPush)"
} else {
    Write-Host "git-sync: committed $shortHead and synced to origin"
}
exit 0
