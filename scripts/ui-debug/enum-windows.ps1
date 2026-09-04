# enum-windows.ps1 - 枚举指定 PID 的所有顶层窗口句柄与标题
param([int]$ProcId)
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinEnum {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT2 r);
    public static List<string> Results = new List<string>();
    public static bool Callback(IntPtr h, IntPtr l) {
        uint pid; GetWindowThreadProcessId(h, out pid);
        if ((int)pid == TargetPid) {
            var sb = new StringBuilder(256);
            GetWindowTextW(h, sb, 256);
            RECT2 r; bool ok = GetWindowRect(h, out r);
            Results.Add(string.Format("hwnd=0x{0:X} visible={1} rect_ok={2} {3}x{4} title='{5}'",
                h.ToInt64(), IsWindowVisible(h), ok, r.Right - r.Left, r.Bottom - r.Top, sb.ToString()));
        }
        return true;
    }
    public static int TargetPid;
}
public struct RECT2 { public int Left, Top, Right, Bottom; }
"@
[WinEnum]::TargetPid = $ProcId
[WinEnum]::Results.Clear()
[WinEnum]::EnumWindows({ [WinEnum]::Callback($args[0], $args[1]) }, [IntPtr]::Zero) | Out-Null
[WinEnum]::Results | ForEach-Object { Write-Output $_ }
