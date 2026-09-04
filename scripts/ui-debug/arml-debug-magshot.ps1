# arml-debug-magshot.ps1 - ArmlDemo UI 调试辅助：Magnification API 捕获硬件加速窗口
#
# 归属：scripts/ui-debug/（ArmlDemo 运行时 UI 调试工具）。
# 用法：先启动 ArmlDemo，再运行本脚本；截图输出到 target/scratch/。
# 背景：本机桌面全硬件合成时 GDI CopyFromScreen 整屏得黑、PrintWindow 只得 GDI 白底，
# Magnification API 经 DWM 合成回读（MagSetImageScalingCallback）可获 swapchain 真实像素。

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies @('System.Drawing','System.Windows.Forms') -TypeDefinition @"
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public class MagCapture {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct MAGIMAGEHEADER {
        public uint width;
        public uint height;
        public Guid format;
        public uint stride;
        public uint offset;
        public UIntPtr cbSize;
    }

    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    public delegate bool MagImageScalingCallback(IntPtr hwnd, IntPtr srcdata, MAGIMAGEHEADER srcheader,
        IntPtr destdata, MAGIMAGEHEADER destheader, RECT unclipped, RECT clipped, IntPtr dirty);

    [DllImport("Magnification.dll")] public static extern bool MagInitialize();
    [DllImport("Magnification.dll")] public static extern bool MagUninitialize();
    [DllImport("Magnification.dll")] public static extern bool MagSetWindowSource(IntPtr hwnd, RECT rect);
    [DllImport("Magnification.dll")] public static extern bool MagSetImageScalingCallback(IntPtr hwnd, MagImageScalingCallback cb);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool DestroyWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool InvalidateRect(IntPtr hWnd, IntPtr r, bool erase);

    public static byte[] Pixels;
    public static int PixW, PixH, PixStride;
    public static bool Done;

    public static bool OnFrame(IntPtr hwnd, IntPtr srcdata, MAGIMAGEHEADER srcheader,
        IntPtr destdata, MAGIMAGEHEADER destheader, RECT unclipped, RECT clipped, IntPtr dirty) {
        int w = (int)srcheader.width;
        int h = (int)srcheader.height;
        int stride = (int)srcheader.stride;
        if (w > 0 && h > 0 && stride > 0 && srcdata != IntPtr.Zero) {
            Pixels = new byte[stride * h];
            Marshal.Copy(srcdata, Pixels, 0, Pixels.Length);
            PixW = w; PixH = h; PixStride = stride;
            Done = true;
        }
        return true;
    }

    public static string Capture(IntPtr targetHwnd, string outPath) {
        RECT wr;
        GetWindowRect(targetHwnd, out wr);
        int w = wr.Right - wr.Left;
        int h = wr.Bottom - wr.Top;
        if (w <= 0 || h <= 0) return "BAD RECT";
        // 目标窗口提到最前但不激活（避免前台独占 direct-flip 黑屏路径）
        SetWindowPos(targetHwnd, IntPtr.Zero, 60, 60, 0, 0, 0x0001 | 0x0010 | 0x0040);
        System.Threading.Thread.Sleep(800);
        GetWindowRect(targetHwnd, out wr);
        w = wr.Right - wr.Left; h = wr.Bottom - wr.Top;

        if (!MagInitialize()) return "MagInitialize failed";

        // 宿主窗口 + Magnifier 子窗口（尺寸 = 源区域）
        Form host = new Form();
        host.FormBorderStyle = FormBorderStyle.None;
        host.StartPosition = FormStartPosition.Manual;
        host.Location = new Point(0, 0); // 须屏内可见，DWM 才为 magnifier 产生合成帧
        host.TopMost = true;
        host.Width = w; host.Height = h;
        host.Show();
        IntPtr mag = CreateWindowExW(0, "Magnifier", "mag", 0x40000000 /*WS_CHILD*/ | 0x10000000 /*WS_VISIBLE*/,
            0, 0, w, h, host.Handle, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        if (mag == IntPtr.Zero) { MagUninitialize(); host.Close(); return "CreateWindowEx Magnifier failed"; }

        MagImageScalingCallback cb = new MagImageScalingCallback(OnFrame);
        MagSetImageScalingCallback(mag, cb);
        RECT src = wr;
        Done = false;
        var sw = System.Diagnostics.Stopwatch.StartNew();
        while (!Done && sw.ElapsedMilliseconds < 10000) {
            MagSetWindowSource(mag, src);
            InvalidateRect(mag, IntPtr.Zero, true);
            Application.DoEvents();
            System.Threading.Thread.Sleep(100);
        }
        string result;
        if (Done) {
            Bitmap bmp = new Bitmap(PixW, PixH, PixelFormat.Format32bppArgb);
            BitmapData bd = bmp.LockBits(new Rectangle(0, 0, PixW, PixH), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
            int rowBytes = PixW * 4;
            for (int y = 0; y < PixH; y++) {
                Marshal.Copy(Pixels, y * PixStride, bd.Scan0 + y * bd.Stride, rowBytes);
            }
            bmp.UnlockBits(bd);
            bmp.Save(outPath, ImageFormat.Png);
            bmp.Dispose();
            result = "OK " + PixW + "x" + PixH;
        } else {
            result = "TIMEOUT (no frame)";
        }
        DestroyWindow(mag);
        host.Close(); host.Dispose();
        MagUninitialize();
        GC.KeepAlive(cb);
        return result;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr CreateWindowExW(uint exStyle, string className, string windowName,
        uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
}
"@
$p = Get-Process -Name ArmlDemo -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Output "NO PROC"; exit }
$scratch = Join-Path $PSScriptRoot '..\..\target\scratch'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$out = Join-Path $scratch 'armldemo_magshot.png'
$r = [MagCapture]::Capture($p.MainWindowHandle, $out)
Write-Output "capture: $r -> $out"
