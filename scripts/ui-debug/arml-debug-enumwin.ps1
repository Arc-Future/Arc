# arml-debug-enumwin.ps1 - ArmlDemo UI 调试辅助：枚举窗口（含 client rect）
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；输出到 stdout，不写文件。

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinEnum {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT r);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$target = (Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1).Id
$rows = @()
$cb = {
    param($h, $l)
    $pid2 = 0
    [WinEnum]::GetWindowThreadProcessId($h, [ref]$pid2) | Out-Null
    if ($pid2 -eq $target) {
        $sb = New-Object System.Text.StringBuilder 256
        [WinEnum]::GetWindowTextW($h, $sb, 256) | Out-Null
        $wr = New-Object WinEnum+RECT
        [WinEnum]::GetWindowRect($h, [ref]$wr) | Out-Null
        $cr = New-Object WinEnum+RECT
        [WinEnum]::GetClientRect($h, [ref]$cr) | Out-Null
        $vis = [WinEnum]::IsWindowVisible($h)
        $script:rows += ("hwnd={0} vis={1} win={2}x{3}@({4},{5}) client={6}x{7} title='{8}'" -f $h, $vis, ($wr.Right-$wr.Left), ($wr.Bottom-$wr.Top), $wr.Left, $wr.Top, ($cr.Right-$cr.Left), ($cr.Bottom-$cr.Top), $sb.ToString())
    }
    return $true
}
[WinEnum]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$rows