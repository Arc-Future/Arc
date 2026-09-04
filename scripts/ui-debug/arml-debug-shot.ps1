# arml-debug-shot.ps1 - ArmlDemo UI 调试辅助：捕获窗口截图
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；截图输出到 target/scratch/（禁止写入源码树根目录）。
#
# 规范：任何脚本的临时输出（截图/log/txt）一律落 target/scratch/ 或 $env:TEMP，
# 不得写入仓库根目录。见 .cursor/rules/arc-workspace-hygiene.mdc。

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public struct RECT { public int Left, Top, Right, Bottom; }
public class Win {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
}
"@
$p = Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Output "NO PROC"; exit }
$h = $p.MainWindowHandle
[Win]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$wr = New-Object RECT
[Win]::GetWindowRect($h, [ref]$wr) | Out-Null
$cr = New-Object RECT
[Win]::GetClientRect($h, [ref]$cr) | Out-Null
$w = $wr.Right - $wr.Left
$hh = $wr.Bottom - $wr.Top
Write-Output "WindowRect=$w x $hh  Client=$($cr.Right) x $($cr.Bottom)"
$b = New-Object System.Drawing.Bitmap($w, $hh)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen($wr.Left, $wr.Top, 0, 0, $b.Size)
$scratch = Join-Path $PSScriptRoot '..\..\target\scratch'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$out = Join-Path $scratch 'armhdemo_shot.png'
$b.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $b.Dispose()
Write-Output "saved $out"