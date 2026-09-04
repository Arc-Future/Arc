# dpi-probe.ps1 - 探测运行中 ArmlDemo 的 DPI 真值（一次性诊断脚本）
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class ArcDpiProbe {
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out ARCRECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out ARCRECT r);
    [DllImport("user32.dll")] public static extern IntPtr GetDpiAwarenessContextForProcess(IntPtr h);
    [DllImport("user32.dll")] public static extern bool AreDpiAwarenessContextsEqual(IntPtr a, IntPtr b);
}
public struct ARCRECT { public int Left, Top, Right, Bottom; }
"@
$pr = Get-Process ArmlDemo -ErrorAction SilentlyContinue
if (-not $pr) { Write-Output "no running ArmlDemo"; exit 0 }
$h = $pr[0].MainWindowHandle
$cr = New-Object ARCRECT; $wr = New-Object ARCRECT
[ArcDpiProbe]::GetClientRect($h, [ref]$cr) | Out-Null
[ArcDpiProbe]::GetWindowRect($h, [ref]$wr) | Out-Null
Write-Output ("pid={0} hwnd=0x{1:X}" -f $pr[0].Id, $h)
Write-Output ("GetDpiForWindow={0} GetDpiForSystem={1}" -f [ArcDpiProbe]::GetDpiForWindow($h), [ArcDpiProbe]::GetDpiForSystem())
Write-Output ("client={0}x{1} window={2}x{3}" -f ($cr.Right-$cr.Left), ($cr.Bottom-$cr.Top), ($wr.Right-$wr.Left), ($wr.Bottom-$wr.Top))
$ctx = [ArcDpiProbe]::GetDpiAwarenessContextForProcess($pr[0].Handle)
$m4 = New-Object IntPtr -ArgumentList -4
$m1 = New-Object IntPtr -ArgumentList -1
$m2 = New-Object IntPtr -ArgumentList -2
$m3 = New-Object IntPtr -ArgumentList -3
Write-Output ("ctx=0x{0:X} unaware(-1)={1} sysAware(-2)={2} PMv1(-3)={3} PMv2(-4)={4}" -f $ctx.ToInt64(), [ArcDpiProbe]::AreDpiAwarenessContextsEqual($ctx, $m1), [ArcDpiProbe]::AreDpiAwarenessContextsEqual($ctx, $m2), [ArcDpiProbe]::AreDpiAwarenessContextsEqual($ctx, $m3), [ArcDpiProbe]::AreDpiAwarenessContextsEqual($ctx, $m4))
