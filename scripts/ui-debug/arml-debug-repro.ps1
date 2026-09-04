# arml-debug-repro.ps1 - ArmlDemo 卡死复现：合成鼠标悬停/滚动扫描（DIAG-TEMP 实验）
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：脚本自行启动 ArmlDemo，合成鼠标事件，结束后输出日志计数；需先构建 DIAG 版本。
# 流程：scroll=0 悬停扫描 → 滚轮下滚 → 再扫描 → 观察卡死；日志在 target/scratch/arml_repro_*.log。

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinRepro {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
public struct RECT { public int Left, Top, Right, Bottom; }
public struct POINT { public int X, Y; }
"@

$exe = "examples\ArmlDemo\bin\Debug\ArmlDemo.exe"
$outLog = "target\scratch\arml_repro_out.log"
$errLog = "target\scratch\arml_repro_err.log"
$p = Start-Process -FilePath $exe -WorkingDirectory "examples\ArmlDemo" -RedirectStandardOutput $outLog -RedirectStandardError $errLog -PassThru
Write-Output "started pid=$($p.Id)"
Start-Sleep -Seconds 8

$h = $p.MainWindowHandle
if ($h -eq 0) { Write-Output "NO HWND"; exit }
[WinRepro]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500

# 客户区原点（屏幕坐标）与尺寸
$pt = New-Object POINT
[WinRepro]::ClientToScreen($h, [ref]$pt) | Out-Null
$cr = New-Object RECT
[WinRepro]::GetClientRect($h, [ref]$cr) | Out-Null
$cw = $cr.Right - $cr.Left
$ch = $cr.Bottom - $cr.Top
Write-Output "client origin=($($pt.X),$($pt.Y)) size=${cw}x${ch}"

function Sweep([int]$steps, [string]$tag) {
    for ($i = 0; $i -lt $steps; $i++) {
        $x = $pt.X + [int]($cw * 0.5)
        $y = $pt.Y + [int]($ch * ($i + 0.5) / $steps)
        [WinRepro]::SetCursorPos($x, $y) | Out-Null
        Start-Sleep -Milliseconds 350
        $pr = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        if (-not $pr) { Write-Output "$tag step=${i}: DEAD"; return $false }
        if (-not $pr.Responding) { Write-Output "$tag step=${i}: NOT RESPONDING (y fraction $(($i+0.5)/$steps))"; return $true }
    }
    Write-Output "$tag sweep done, process alive"
    return $true
}

# 1) scroll=0 悬停扫描（覆盖首屏各控件）
$null = Sweep 12 "pass1-scroll0"
Start-Sleep -Seconds 2

$pr = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
if ($pr) { Write-Output "final: alive Resp=$($pr.Responding) CPU=$([math]::Round($pr.CPU,1)) WS=$([math]::Round($pr.WorkingSet64/1MB,1))MB" } else { Write-Output "final: DEAD" }
Write-Output "--- tag counts ---"
$err = Get-Content $errLog
"PUMP frame : $(($err | Select-String '\[PUMP-DIAG\] frame').Count)"
"PUMP rearm : $(($err | Select-String '\[PUMP-DIAG\] rearm').Count)"
"EF steps   : $(($err | Select-String '\[EF\]').Count)"
"last EF    : $(($err | Select-String '\[EF\]') | Select-Object -Last 1)"
"PTR-VIS    : $(($err | Select-String '\[PTR-VIS\]').Count)"
"INPUT-FOCUS: $(($err | Select-String '\[INPUT-FOCUS\]').Count)"
"SCROLL-RLT : $(($err | Select-String '\[SCROLL-RELAYOUT\]').Count)"
"FOCUS-MGR  : $(($err | Select-String '\[FOCUS-MGR\]').Count)"
"SETVALUE   : $(($err | Select-String '\[SETVALUE\]').Count)"
"ADDCHILD   : $(($err | Select-String '\[ADDCHILD\]').Count)"
"VIS        : $(($err | Select-String '\[VIS\]').Count)"
