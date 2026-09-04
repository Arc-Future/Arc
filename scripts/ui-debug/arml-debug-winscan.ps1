# arml-debug-winscan.ps1 - ArmlDemo UI 调试辅助：枚举可见窗口
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；输出到 stdout，不写文件。

Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WinEnum2 {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
$target = (Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1).Id
if (-not $target) { Write-Output "NO PROC"; exit }
$cb = {
    param($h, $l)
    $script:pid2 = 0
    [WinEnum2]::GetWindowThreadProcessId($h, [ref]$pid2) | Out-Null
    if ($pid2 -eq $script:target) {
        $sb = New-Object System.Text.StringBuilder 256
        [WinEnum2]::GetWindowTextW($h, $sb, 256) | Out-Null
        $wr = New-Object WinEnum2+RECT
        [WinEnum2]::GetWindowRect($h, [ref]$wr) | Out-Null
        $vis = [WinEnum2]::IsWindowVisible($h)
        $w = $wr.Right-$wr.Left; $hh = $wr.Bottom-$wr.Top
        if ($vis -and $w -gt 0) {
            Write-Output ("VIS hwnd={0} win={1}x{2}@({3},{4}) title='{5}'" -f $h, $w, $hh, $wr.Left, $wr.Top, $sb.ToString())
        }
    }
    return $true
}
[WinEnum2]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null