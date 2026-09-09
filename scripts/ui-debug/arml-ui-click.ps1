# arml-ui-click.ps1 — ArmlDemo / Arc.UI 点击坐标契约（唯一权威辅助）
#
# 坐标空间（与 pointer_win32.c / window.cpp 同源）：
#   - Arc LayoutX/Y、命中测试 = 客户区 DIP
#   - WM_MOUSE / SetCursorPos 物理像素 = DIP * (GetDpiForWindow/96)
#   - 屏幕坐标 = ClientToScreen(0,0) + 物理偏移
#   - 禁用 GetWindowRect 当客户区原点（含标题栏非客户区 → 点击上偏）
#   - 本脚本启动即 Per-Monitor V2 DPI 感知；未感知时 Win32 虚化坐标会与 *scale 双倍缩放
#
# 用法：
#   . .\scripts\ui-debug\arml-ui-click.ps1
#   $ctx = Get-ArmlUiClickContext   # 附加已运行 ArmlDemo，或 -Start
#   Invoke-ArmlClickDip $ctx 40 18  # 点第一页签附近
#   Invoke-ArmlProofClickMe         # 最小验收：点 Hello「Click Me」，查 stdout

param(
    [switch]$ProofOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class ArmlUiClickNative {
    public const int DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4;
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT pt);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT rc);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rc);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    public const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    public const uint MOUSEEVENTF_LEFTUP = 0x0004;
    public struct POINT { public int X; public int Y; }
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@

# 必须在任何 GetClientRect / ClientToScreen 之前声明感知，否则坐标虚化。
[void][ArmlUiClickNative]::SetProcessDpiAwarenessContext(
    [IntPtr][ArmlUiClickNative]::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)

function Get-ArmlUiClickContext {
    param(
        [switch]$Start,
        [string]$Exe = '',
        [string]$WorkDir = '',
        [string]$OutLog = '',
        [string]$ErrLog = ''
    )
    $repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    if (-not $Exe) {
        $Exe = Join-Path $repo 'examples\ArmlDemo\bin\Debug\ArmlDemo.exe'
    }
    if (-not $WorkDir) {
        $WorkDir = Join-Path $repo 'examples\ArmlDemo'
    }
    $scratch = Join-Path $repo 'target\scratch'
    New-Item -ItemType Directory -Force -Path $scratch | Out-Null
    if (-not $OutLog) { $OutLog = Join-Path $scratch 'arml_click_out.log' }
    if (-not $ErrLog) { $ErrLog = Join-Path $scratch 'arml_click_err.log' }

    $proc = Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1
    $started = $false
    if ($Start -or -not $proc) {
        if ($proc) {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 400
        }
        Remove-Item $OutLog, $ErrLog -ErrorAction SilentlyContinue
        $proc = Start-Process -FilePath $Exe -WorkingDirectory $WorkDir `
            -RedirectStandardOutput $OutLog -RedirectStandardError $ErrLog -PassThru
        $started = $true
        $deadline = (Get-Date).AddSeconds(12)
        while ((Get-Date) -lt $deadline) {
            if ($proc.HasExited) { throw "ArmlDemo exited early code=$($proc.ExitCode)" }
            $proc.Refresh()
            if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 200
        }
    }
    $hwnd = $proc.MainWindowHandle
    if ($hwnd -eq [IntPtr]::Zero) {
        $hwnd = [ArmlUiClickNative]::FindWindowW($null, 'ARML Demo - TabControl')
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw 'ArmlDemo HWND not found' }

    $dpi = [ArmlUiClickNative]::GetDpiForWindow($hwnd)
    if ($dpi -lt 96) { $dpi = 96 }
    $scale = [double]$dpi / 96.0

    $origin = New-Object ArmlUiClickNative+POINT
    $origin.X = 0; $origin.Y = 0
    if (-not [ArmlUiClickNative]::ClientToScreen($hwnd, [ref]$origin)) {
        throw 'ClientToScreen failed'
    }
    $cr = New-Object ArmlUiClickNative+RECT
    [ArmlUiClickNative]::GetClientRect($hwnd, [ref]$cr) | Out-Null
    $physW = $cr.Right - $cr.Left
    $physH = $cr.Bottom - $cr.Top
    $dipW = [int]([math]::Round($physW / $scale))
    $dipH = [int]([math]::Round($physH / $scale))

    [pscustomobject]@{
        Process = $proc
        Hwnd    = $hwnd
        Dpi     = $dpi
        Scale   = $scale
        ClientOriginScreenX = $origin.X
        ClientOriginScreenY = $origin.Y
        ClientPhysW = $physW
        ClientPhysH = $physH
        ClientDipW  = $dipW
        ClientDipH  = $dipH
        OutLog  = $OutLog
        ErrLog  = $ErrLog
        Started = $started
    }
}

function Convert-ArmlDipToScreen {
    param($Context, [double]$DipX, [double]$DipY)
    $sx = [int]([math]::Round($Context.ClientOriginScreenX + $DipX * $Context.Scale))
    $sy = [int]([math]::Round($Context.ClientOriginScreenY + $DipY * $Context.Scale))
    [pscustomobject]@{ ScreenX = $sx; ScreenY = $sy; DipX = $DipX; DipY = $DipY }
}

function Invoke-ArmlClickDip {
    param(
        $Context,
        [double]$DipX,
        [double]$DipY,
        [int]$SettleMs = 400
    )
    if ($Context.Process.HasExited) {
        throw "ArmlDemo already exited code=$($Context.Process.ExitCode)"
    }
    [ArmlUiClickNative]::SetForegroundWindow($Context.Hwnd) | Out-Null
    $pt = Convert-ArmlDipToScreen -Context $Context -DipX $DipX -DipY $DipY
    [ArmlUiClickNative]::SetCursorPos($pt.ScreenX, $pt.ScreenY) | Out-Null
    Start-Sleep -Milliseconds 40
    [ArmlUiClickNative]::mouse_event([ArmlUiClickNative]::MOUSEEVENTF_LEFTDOWN, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 30
    [ArmlUiClickNative]::mouse_event([ArmlUiClickNative]::MOUSEEVENTF_LEFTUP, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $SettleMs
    if ($Context.Process.HasExited) {
        throw "ArmlDemo crashed after click DIP($DipX,$DipY) screen($($pt.ScreenX),$($pt.ScreenY)) exit=$($Context.Process.ExitCode)"
    }
    return $pt
}

function Write-ArmlUiClickDiag {
    param($Context)
    Write-Output ("hwnd=0x{0:X} dpi={1} scale={2:N2}" -f $Context.Hwnd.ToInt64(), $Context.Dpi, $Context.Scale)
    Write-Output ("clientOriginScreen=({0},{1}) phys={2}x{3} dip={4}x{5}" -f `
        $Context.ClientOriginScreenX, $Context.ClientOriginScreenY, `
        $Context.ClientPhysW, $Context.ClientPhysH, $Context.ClientDipW, $Context.ClientDipH)
}

<#
.SYNOPSIS
  最小验收：点 Hello 页「Click Me」（DIP 估点），断言 stdout 含 Button clicked。
  Tab 顶栏高 36；页内 Margin 12 + 标题/文案约 4 行 → 按钮中心约 (80, 200)。
#>
function Invoke-ArmlProofClickMe {
    param(
        [double]$ButtonDipX = 80.0,
        [double]$ButtonDipY = 200.0
    )
    $ctx = Get-ArmlUiClickContext -Start
    Write-ArmlUiClickDiag $ctx
    # 确保在 Hello 页：点第一页签中心（Header 测宽左对齐，首签约 x=40）
    Invoke-ArmlClickDip $ctx 40 18 | Out-Null
    $before = ''
    if (Test-Path $ctx.OutLog) { $before = Get-Content $ctx.OutLog -Raw -ErrorAction SilentlyContinue }
    $pt = Invoke-ArmlClickDip $ctx $ButtonDipX $ButtonDipY -SettleMs 600
    Start-Sleep -Milliseconds 300
    $after = Get-Content $ctx.OutLog -Raw -ErrorAction SilentlyContinue
    $ok = ($after -match 'Button clicked')
    Write-Output ("clickMe screen=({0},{1}) dip=({2},{3}) ok={4}" -f $pt.ScreenX, $pt.ScreenY, $ButtonDipX, $ButtonDipY, $ok)
    if ($after) {
        $tail = ($after -split "`n" | Select-Object -Last 8) -join ' | '
        Write-Output ("stdout_tail: $tail")
    }
    if ($ctx.Started -and -not $ctx.Process.HasExited) {
        Stop-Process -Id $ctx.Process.Id -Force -ErrorAction SilentlyContinue
    }
    if (-not $ok) {
        throw 'PROOF FAILED: Click Me did not produce Button clicked (check DIP estimate or DPI awareness)'
    }
    Write-Output 'PROOF OK: DIP→screen click landed on Click Me'
}

if ($ProofOnly) {
    Invoke-ArmlProofClickMe
}
