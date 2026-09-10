param()
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot
Write-Host 'derive-dark-from-light: writing Dark.arml...'
$env:UPDATE_DARK_THEME = '1'
cargo test -p arc-ui --test design_tokens_contract -- dark_arml_matches_light_seed_derivation --exact --nocapture
if ($LASTEXITCODE -ne 0) { Write-Error 'Dark.arml regen failed'; exit $LASTEXITCODE }
Remove-Item Env:UPDATE_DARK_THEME -ErrorAction SilentlyContinue
Write-Host 'derive-dark-from-light: regenerating Colors.g.as...'
$env:UPDATE_BUILTIN_THEME = '1'
cargo test -p arc-ui --test design_tokens_contract -- builtin_theme_colors_g_as_in_sync --exact --nocapture
if ($LASTEXITCODE -ne 0) { Write-Error 'Colors.g.as regen failed'; exit $LASTEXITCODE }
Remove-Item Env:UPDATE_BUILTIN_THEME -ErrorAction SilentlyContinue
Write-Host 'derive-dark-from-light: verifying contracts...'
cargo test -p arc-ui --test design_tokens_contract --test theme_switch_contract
if ($LASTEXITCODE -ne 0) { Write-Error 'contract verify failed'; exit $LASTEXITCODE }
Write-Host 'derive-dark-from-light: OK'