# snap-window.ps1 - 截取指定 hwnd 窗口客户区到 PNG
param([long]$Hwnd, [string]$OutFile)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinSnap {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
public struct RECT { public int Left, Top, Right, Bottom; }
"@
$h = New-Object IntPtr $Hwnd
[WinSnap]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 400
$r = New-Object RECT
$ok = [WinSnap]::GetWindowRect($h, [ref]$r)
Write-Output "GetWindowRect=$ok L=$($r.Left) T=$($r.Top) R=$($r.Right) B=$($r.Bottom)"
$w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
$bmp = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
$bmp.Save($OutFile, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Output "saved $OutFile ${w}x${hh}"
