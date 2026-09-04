# check-project-artifact-layout.ps1 - project bin/obj layout iron-rule gate (CI)
#
# Authority: RFC 031 §5 · workspace hygiene G″ (MSBuild-aligned)
#
# Purpose: fail CI if crates/arc reintroduces default project outputs under
#   {project}/target/bin  or  {project}/target/obj
# Allowed: Cargo workspace target/, e2e fixtures target/e2e/<name>/, and
#   explicit CLI --obj-dir overrides (not default resolution).
#
# Usage (repo root):
#   pwsh scripts/check-project-artifact-layout.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-project-artifact-layout.ps1
#
# Exit codes:
#   0 = clean
#   1 = forbidden default-layout pattern found in crates/arc/src
#
# How this fails on regression: if someone restores
#   project_root.join("target").join("bin").join(config)
# (or Path::new("target/obj") defaults), this script prints the offending
# crates/arc/src line and exits 1. Pair with
# crates/arc-tests/tests/l1_artifact_layout_batch.rs for runtime proof.

param()

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

$scanRoot = Join-Path $RepoRoot 'crates/arc/src'
if (-not (Test-Path -LiteralPath $scanRoot)) {
    Write-Host "missing scan root: $scanRoot" -ForegroundColor Red
    exit 1
}

# Literal substrings (not regex) — avoids PowerShell quote-escaping pitfalls.
$needles = @(
    '.join("target").join("bin")',
    '.join("target").join("obj")',
    'Path::new("target/bin")',
    'Path::new("target/obj")',
    'PathBuf::from("target/bin")',
    'PathBuf::from("target/obj")'
)

$violations = [System.Collections.Generic.List[string]]::new()

Get-ChildItem -LiteralPath $scanRoot -Recurse -File -Filter '*.rs' | ForEach-Object {
    $file = $_.FullName
    $rel = $file.Substring($RepoRoot.Length).TrimStart('\', '/') -replace '\\', '/'
    # Force array: single-line files otherwise yield a String, and [$i] indexes chars.
    $lines = @(Get-Content -LiteralPath $file)
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = [string]$lines[$i]
        foreach ($needle in $needles) {
            if ($line.Contains($needle)) {
                $violations.Add(("{0}:{1}: {2}" -f $rel, ($i + 1), $line.Trim()))
            }
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Host ("Project artifact layout: {0} violation(s)" -f $violations.Count) -ForegroundColor Red
    Write-Host 'Iron rule: project finals -> bin/<Config>/ ; intermediates -> obj/<Config>/'
    Write-Host 'Do NOT default to target/bin or target/obj (Cargo target/ and target/e2e/ OK).'
    foreach ($v in $violations) { Write-Host ("  - " + $v) }
    Write-Host 'See: docs/rfc/031-compiler-cli.md section 5 ; crates/arc-tests/tests/l1_artifact_layout_batch.rs'
    exit 1
}

Write-Host 'Project artifact layout: clean (0 violations)'
exit 0
