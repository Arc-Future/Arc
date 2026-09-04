// Arc.Array — Stable 最小面（P5-F；空桩审计后按 tip 能力补齐）。
// Array 为 stub facade：公开面均为 [Builtin] + 已接线 ABI。
// Stable：Copy/Clear/Reverse；int IndexOf/LastIndexOf/Empty/Resize；
// int 谓词 Exists/Find/FindLast/FindIndex/FindLastIndex/TrueForAll/ForEach；
// int Sort / BinarySearch（升序；未找到返回 ~insertionPoint）；
// int FindAll / ConvertAll（int→int；converter 复用 pred ABI 返回映射值）。
// 后置：Join（C# System.Array 无此成员，勿发明）/泛型 Empty/定制比较器/跨类型 ConvertAll——禁止空 stub。
namespace Arc;

/// <summary>对齐 C# <c>System.Array</c> 的静态工具面（诚实最小子集）。</summary>
public class Array {
    [Builtin(ABI = "Array.Copy")]
    public static void Copy<T>(T[] src, int srcOffset, T[] dst, int dstOffset, int length) { }

    [Builtin(ABI = "Array.Clear")]
    public static void Clear<T>(T[] array, int index, int length) { }

    [Builtin(ABI = "Array.Reverse")]
    public static void Reverse<T>(T[] array) { }

    [Builtin(ABI = "Array.Copy")]
    public static void Copy(int[] src, int srcOffset, int[] dst, int dstOffset, int length) { }

    [Builtin(ABI = "Array.Clear")]
    public static void Clear(int[] array, int index, int length) { }

    [Builtin(ABI = "Array.Reverse")]
    public static void Reverse(int[] array) { }

    [Builtin(ABI = "Array.Empty")]
    public static int[] Empty() { return null; }

    [Builtin(ABI = "Array.IndexOf")]
    public static int IndexOf(int[] array, int value) { return -1; }

    [Builtin(ABI = "Array.LastIndexOf")]
    public static int LastIndexOf(int[] array, int value) { return -1; }

    [Builtin(ABI = "Array.Resize")]
    public static void Resize(ref int[] array, int newSize) { }

    [Builtin(ABI = "Array.Exists")]
    public static bool Exists(int[] array, Func<int, bool> predicate) { return false; }

    [Builtin(ABI = "Array.Find")]
    public static int Find(int[] array, Func<int, bool> predicate) { return 0; }

    [Builtin(ABI = "Array.FindLast")]
    public static int FindLast(int[] array, Func<int, bool> predicate) { return 0; }

    [Builtin(ABI = "Array.FindIndex")]
    public static int FindIndex(int[] array, Func<int, bool> predicate) { return -1; }

    [Builtin(ABI = "Array.FindLastIndex")]
    public static int FindLastIndex(int[] array, Func<int, bool> predicate) { return -1; }

    [Builtin(ABI = "Array.TrueForAll")]
    public static bool TrueForAll(int[] array, Func<int, bool> predicate) { return false; }

    [Builtin(ABI = "Array.ForEach")]
    public static void ForEach(int[] array, Action<int> action) { }

    /// <summary>原地升序排序（<c>int[]</c>；默认比较）。</summary>
    [Builtin(ABI = "Array.Sort")]
    public static void Sort(int[] array) { }

    /// <summary>已排序 <c>int[]</c> 上二分查找；命中返回下标，否则 <c>~insertionPoint</c>。</summary>
    [Builtin(ABI = "Array.BinarySearch")]
    public static int BinarySearch(int[] array, int value) { return -1; }

    /// <summary>返回满足谓词的全部元素（新建 <c>int[]</c>）。</summary>
    [Builtin(ABI = "Array.FindAll")]
    public static int[] FindAll(int[] array, Func<int, bool> predicate) { return null; }

    /// <summary>逐元素映射为新 <c>int[]</c>（<c>Func&lt;int,int&gt;</c>；跨类型 ConvertAll 后置）。</summary>
    [Builtin(ABI = "Array.ConvertAll")]
    public static int[] ConvertAll(int[] array, Func<int, int> converter) { return null; }
}
