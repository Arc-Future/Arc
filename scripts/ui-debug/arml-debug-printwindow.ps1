# arml-debug-printwindow.ps1 - ArmlDemo UI 调试辅助：PrintWindow 捕获硬件加速窗口
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；截图输出到 target/scratch/。
# 背景：wgpu/DXGI flip-model swapchain 在 MPO/DirectFlip 下 GDI CopyFromScreen 得全黑，
# PrintWindow(PW_RENDERFULLCONTENT=0x2) 可让 DWM 重绘窗口内容到位图。

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinPW {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
public struct RECT { public int Left, Top, Right, Bottom; }
"@
$p = Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Output "NO PROC"; exit }
$h = $p.MainWindowHandle
[WinPW]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$wr = New-Object RECT
[WinPW]::GetWindowRect($h, [ref]$wr) | Out-Null
$w = $wr.Right - $wr.Left
$hh = $wr.Bottom - $wr.Top
Write-Output "WindowRect=$w x $hh"
$b = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($b)
$hdc = $g.GetHdc()
$ok = [WinPW]::PrintWindow($h, $hdc, 0x2)
$g.ReleaseHdc($hdc)
$g.Dispose()
$scratch = Join-Path $PSScriptRoot '..\..\target\scratch'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$out = Join-Path $scratch 'armldemo_printwindow.png'
$b.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$b.Dispose()
Write-Output "PrintWindow ok=$ok saved $out"
