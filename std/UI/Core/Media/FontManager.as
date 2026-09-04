// RFC 037 §9 / references/custom-fonts.md：应用级命名字体族注册（WPF 心智最小面）。
//
// 用户正道唯一入口：Application.Current.Fonts.RegisterFamily(...)。
// 后端 WgpuRender.RegisterFontFamily 仅为内部适配，禁止作为第二用户 API。
// 与 Arc.Drawing.Font（离屏成像）分轨，UI 帧内文本不得绑该类型。

namespace Arc.UI;

using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.UI.Rendering.Wgpu;

/// <summary>
/// 应用级字体注册表——命名族 → 字面文件（Normal / 可选 Bold）。
/// </summary>
public class FontManager
{
    private List<string> _familyNames;
    private List<string> _normalAbsPaths;
    private List<string> _boldAbsPaths;
    private List<string> _warnedUnknownFamilies;
    private WgpuRender _backend;

    public FontManager()
    {
        _familyNames = new List<string>();
        _normalAbsPaths = new List<string>();
        _boldAbsPaths = new List<string>();
        _warnedUnknownFamilies = new List<string>();
        _backend = null;
    }

    /// <summary>
    /// 注册命名族的 Normal 面。相对路径相对应用基目录（可执行文件所在目录，
    /// 即 <c>bin/&lt;Config&gt;/</c>）；绝对路径仅调试旁路。成功 true，失败 false。
    /// </summary>
    public bool RegisterFamily(string familyName, string relativePath)
    {
        return this.RegisterFamily(familyName, relativePath, "");
    }

    /// <summary>
    /// 注册命名族，并分别指定 Normal / Bold 字面。Bold 文件须存在（即使本帧仅用 Normal）。
    /// FontWeight→面由后端 weight 槽选用（D：Bold/700 → weight 1）；本面只产 chain。
    /// </summary>
    public bool RegisterFamily(string familyName, string normalRelativePath, string boldRelativePath)
    {
        if (familyName == null || familyName.Length == 0)
        {
            Console.ErrorWriteLine("[FontManager] RegisterFamily failed: empty family name");
            return false;
        }
        if (normalRelativePath == null || normalRelativePath.Length == 0)
        {
            Console.ErrorWriteLine("[FontManager] RegisterFamily failed: empty normal path for '" + familyName + "'");
            return false;
        }
        if (_familyNames.Contains(familyName))
        {
            Console.ErrorWriteLine("[FontManager] RegisterFamily failed: duplicate family '" + familyName + "'");
            return false;
        }

        string normalAbs = FontManager.ResolveFontPath(normalRelativePath);
        if (!FontManager.IsSupportedFontFile(normalAbs) || !File.Exists(normalAbs))
        {
            Console.ErrorWriteLine("[FontManager] RegisterFamily failed: normal font missing or unsupported: " + normalAbs);
            return false;
        }

        string boldAbs = "";
        if (boldRelativePath != null && boldRelativePath.Length > 0)
        {
            boldAbs = FontManager.ResolveFontPath(boldRelativePath);
            if (!FontManager.IsSupportedFontFile(boldAbs) || !File.Exists(boldAbs))
            {
                Console.ErrorWriteLine("[FontManager] RegisterFamily failed: bold font missing or unsupported: " + boldAbs);
                return false;
            }
        }

        // 后端已就绪：立即适配 atlas；否则排队，FramePump 绑定后 Flush（首帧绘制前）。
        if (_backend != null)
        {
            if (!this.ApplyToBackend(familyName, normalAbs, boldAbs))
            {
                return false;
            }
        }

        _familyNames.Add(familyName);
        _normalAbsPaths.Add(normalAbs);
        _boldAbsPaths.Add(boldAbs);
        return true;
    }

    /// <summary>族名是否已成功登记（含待 Flush 项）。</summary>
    public bool IsRegistered(string familyName)
    {
        if (familyName == null || familyName.Length == 0)
        {
            return false;
        }
        return _familyNames.Contains(familyName);
    }

    /// <summary>
    /// 未注册 / 空名时由渲染器回退默认族；此处一次性 stderr 诊断，避免静默误用。
    /// </summary>
    internal void WarnUnknownFamily(string familyName)
    {
        if (familyName == null || familyName.Length == 0)
        {
            return;
        }
        if (familyName == "Segoe UI")
        {
            return;
        }
        if (_familyNames.Contains(familyName))
        {
            return;
        }
        if (_warnedUnknownFamilies.Contains(familyName))
        {
            return;
        }
        _warnedUnknownFamilies.Add(familyName);
        Console.ErrorWriteLine("[FontManager] FontFamily '" + familyName + "' not registered; falling back to default family");
    }

    /// <summary>FramePump 在 WgpuRender.Initialize 成功后绑定；Flush 排队注册。</summary>
    internal void BindBackend(WgpuRender backend)
    {
        _backend = backend;
        if (_backend == null)
        {
            return;
        }
        this.FlushPending();
    }

    /// <summary>窗口关闭时解绑（族名表保留；进程内不支持热替换）。</summary>
    internal void UnbindBackend()
    {
        _backend = null;
    }

    /// <summary>
    /// 相对路径基准 = 应用基目录 = argv[0] 所在目录（<c>bin/&lt;Config&gt;/</c>）。
    /// 禁止多套 cwd 猜测；绝对路径原样使用（调试旁路，非用户正道）。
    /// </summary>
    public static string GetApplicationBaseDirectory()
    {
        if (Environment.ArgCount() > 0)
        {
            string exe = Environment.GetArg(0);
            if (exe != null && exe.Length > 0)
            {
                string dir = Path.GetDirectoryName(exe);
                if (dir != null && dir.Length > 0)
                {
                    return dir;
                }
            }
        }
        return Environment.GetCurrentDirectory();
    }

    private void FlushPending()
    {
        // 重建成功列表：禁止在遍历中 RemoveAt（NLL E_ITERATOR_INVALIDATION）。
        List<string> keepNames = new List<string>();
        List<string> keepNormal = new List<string>();
        List<string> keepBold = new List<string>();
        int n = _familyNames.Count;
        for (int i = 0; i < n; i++)
        {
            string name = _familyNames[i];
            string normalAbs = _normalAbsPaths[i];
            string boldAbs = _boldAbsPaths[i];
            if (this.ApplyToBackend(name, normalAbs, boldAbs))
            {
                keepNames.Add(name);
                keepNormal.Add(normalAbs);
                keepBold.Add(boldAbs);
            }
            else
            {
                Console.ErrorWriteLine("[FontManager] Flush failed for '" + name + "' (atlas rejected); unregistering");
            }
        }
        _familyNames = keepNames;
        _normalAbsPaths = keepNormal;
        _boldAbsPaths = keepBold;
    }

    /// <summary>
    /// 适配内部 atlas。chain 与 D / <c>wgpu_font_atlas_add_family</c> 对齐：
    /// <c>normalAbs</c> 仅 Normal；<c>normalAbs|boldAbs</c> 单 '|' = Bold 面（非 Symbol）。
    /// </summary>
    private bool ApplyToBackend(string familyName, string normalAbs, string boldAbs)
    {
        if (_backend == null)
        {
            return false;
        }
        // D 契约：单 '|' 分隔 Normal|Bold（不是 主|Symbol|Emoji 三分类）。
        string chain = normalAbs;
        if (boldAbs != null && boldAbs.Length > 0)
        {
            chain = normalAbs + "|" + boldAbs;
        }
        int idx = _backend.RegisterFontFamily(familyName, chain);
        if (idx < 0)
        {
            Console.ErrorWriteLine("[FontManager] atlas add_family failed for '" + familyName + "'");
            return false;
        }
        return true;
    }

    private static string ResolveFontPath(string path)
    {
        if (FontManager.IsPathRooted(path))
        {
            return path;
        }
        return Path.Combine(FontManager.GetApplicationBaseDirectory(), path);
    }

    private static bool IsPathRooted(string path)
    {
        if (path == null || path.Length == 0)
        {
            return false;
        }
        string c0 = path.Substring(0, 1);
        if (c0 == "/" || c0 == "\\")
        {
            return true;
        }
        if (path.Length >= 2 && path.Substring(1, 1) == ":")
        {
            return true;
        }
        return false;
    }

    private static bool IsSupportedFontFile(string path)
    {
        if (path == null || path.Length == 0)
        {
            return false;
        }
        string ext = Path.GetExtension(path).ToLower();
        return ext == ".ttf" || ext == ".otf";
    }
}
