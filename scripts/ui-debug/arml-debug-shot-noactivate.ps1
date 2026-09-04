# arml-debug-shot-noactivate.ps1 - ArmlDemo UI 调试辅助：不激活窗口的 GDI 屏幕捕获
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；截图输出到 target/scratch/。
# 背景：DirectFlip/MPO 仅在窗口前台独占时生效；SWP_NOACTIVATE 置前但不激活，
# 窗口内容由 DWM 合成，GDI CopyFromScreen 可读（前台独占时 swapchain 直翻呈全黑）。

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class WinNA {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr hWndInsertAfter,
        int X, int Y, int cx, int cy, uint uFlags);
}
"@
$p = Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Output "NO PROC"; exit }
$h = $p.MainWindowHandle
# HWND_TOP + SWP_NOACTIVATE|SWP_SHOWWINDOW：提到最前但不抢占前台（避免 direct-flip）
[WinNA]::SetWindowPos($h, [IntPtr]::Zero, 40, 40, 0, 0, 0x0001 -bor 0x0010 -bor 0x0040) | Out-Null
Start-Sleep -Milliseconds 800
$wr = New-Object RECT
[WinNA]::GetWindowRect($h, [ref]$wr) | Out-Null
$w = $wr.Right - $wr.Left
$hh = $wr.Bottom - $wr.Top
Write-Output "WindowRect=$w x $hh @($($wr.Left),$($wr.Top))"
$b = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen($wr.Left, $wr.Top, 0, 0, $b.Size)
$scratch = Join-Path $PSScriptRoot '..\..\target\scratch'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$out = Join-Path $scratch 'armldemo_shot_noactivate.png'
$b.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $b.Dispose()
Write-Output "saved $out"
