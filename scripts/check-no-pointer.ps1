# check-no-pointer.ps1 - RFC 095 no-pointer-surface constitutional gate
#
# Authority: docs/chapters/07-rfcs/095-no-pointer-surface.md section 4
#
# Scans .as files under std/ and examples/ for user-surface pointer regressions:
#   - C# pointer-wrapper types: IntPtr / UIntPtr / nint / nuint
#   - C# unsafe APIs: Unsafe.* / MemoryMarshal.*
#   - C# unsafe keyword: stackalloc
#   - void* (exempted under FFI whitelist - RFC 027 M3 / RFC 095 section 2.2)
#
# Does NOT scan *T / &T / unsafe / fixed:
#   - parse stage already rejects them (lexer has no such token; Star is only multiply)
#   - * and & are legal operators; scanning them yields massive false positives
#
# Usage (repo root):
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\check-no-pointer.ps1
#   pwsh scripts/check-no-pointer.ps1
#
# Exit:
#   0 = clean (0 violations)
#   1 = violations found (details printed)

param(
    [string[]]$Roots = @('std', 'examples'),
    [string[]]$FfiWhitelist = @(
        'std/Interop/*',
        'std/UI/Core/Rendering/Wgpu/*',
        'std/Arc/Runtime/*',
        'std/Arc/Resources/*'
    )
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

# RFC 095 section 2.1 forbidden types / section 4.3 diagnostic code families
$Patterns = @(
    '\bIntPtr\b',
    '\bUIntPtr\b',
    '\bnint\b',
    '\bnuint\b',
    '\bUnsafe\s*\.',
    '\bMemoryMarshal\s*\.',
    '\bstackalloc\b',
    'void\s*\*'
)

$violations = @()
foreach ($root in $Roots) {
    $rootPath = Join-Path $RepoRoot $root
    if (-not (Test-Path $rootPath)) { continue }
    $files = Get-ChildItem -Path $rootPath -Recurse -Filter '*.as' -File
    foreach ($file in $files) {
        # Compatible with Windows PowerShell 5.1 (no [IO.Path]::GetRelativePath)
        $rel = $file.FullName.Substring($RepoRoot.Length + 1) -replace '\\', '/'
        $isFfi = $false
        foreach ($glob in $FfiWhitelist) {
            if ($rel -like $glob) { $isFfi = $true; break }
        }
        $lines = Get-Content $file.FullName
        for ($i = 0; $i -lt $lines.Count; $i++) {
            # Strip single-line comment // ... (block comments /* */ rare; handle if hit)
            $code = $lines[$i] -replace '//.*$', ''
            foreach ($pat in $Patterns) {
                if ($code -match $pat) {
                    # void* exempted under FFI whitelist (RFC 027 / RFC 095 section 2.2)
                    if ($isFfi -and $pat -eq 'void\s*\*') { continue }
                    $violations += [pscustomobject]@{
                        File    = $rel
                        Line    = $i + 1
                        Pattern = $pat
                        Content = $lines[$i].Trim()
                    }
                }
            }
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Host "RFC 095 no-pointer surface: $($violations.Count) violation(s) found" -ForegroundColor Red
    $violations | Format-Table -AutoSize
    exit 1
}
Write-Host "RFC 095 no-pointer surface: clean (0 violations in std/ examples/)"
exit 0
