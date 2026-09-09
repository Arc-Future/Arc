// RFC 037 §0.1.1：Style 短键查找（与 arc-ui style_key.rs 同序）。
//
// 作者只写短键 Primary / Small。字典：控件覆写 = {TargetType}.{Short}；
// 全局复用 = {Short}（Shared.arml）。
// 查找：含 '.' → 精确；否则先 T.K 再 K；失败返回 null（调用方拼诊断）。

namespace Arc.UI.Styling;

using Arc.UI;
using Arc.Collections;

/// <summary>按 TargetType 作用域 → 全局 回退查找 Style 短键。</summary>
internal static class StyleKeyResolver {
    /// <summary>
    /// 查找样式。命中且 TargetType 匹配（空 TargetType = 任意）则返回；
    /// 未命中返回 null。triedOut 填已试键（可读诊断）。
    /// </summary>
    public static Style Lookup(ResourceDictionary chain, string key, string targetType, Element element, List<string> triedOut) {
        if (chain == null || key == null || key == "") {
            return null;
        }
        List<string> candidates = StyleKeyResolver.Candidates(key, targetType);
        for (int i = 0; i < candidates.Count; i++) {
            string cand = candidates[i];
            if (triedOut != null) {
                triedOut.Add(cand);
            }
            Style style = chain.LookupStyle(cand);
            if (style == null) {
                continue;
            }
            if (element != null && !style.Matches(element)) {
                continue;
            }
            return style;
        }
        return null;
    }

    /// <summary>兼容：无 tried 列表。</summary>
    public static Style Lookup(ResourceDictionary chain, string key, string targetType) {
        return StyleKeyResolver.Lookup(chain, key, targetType, null, null);
    }

    /// <summary>候选键顺序（与文档 §0.1.1 一致）。</summary>
    public static List<string> Candidates(string key, string targetType) {
        List<string> outList = new List<string>();
        if (key == null) {
            return outList;
        }
        string trimmed = key.Trim();
        if (trimmed == "") {
            return outList;
        }
        if (StyleKeyResolver.ContainsDot(trimmed)) {
            outList.Add(trimmed);
            return outList;
        }
        if (targetType != null && targetType != "") {
            outList.Add(targetType + "." + trimmed);
        }
        outList.Add(trimmed);
        return outList;
    }

    /// <summary>可读失败信息。</summary>
    public static string MissDiagnostic(string key, string targetType) {
        List<string> tried = StyleKeyResolver.Candidates(key, targetType);
        string joined = "";
        for (int i = 0; i < tried.Count; i++) {
            if (i > 0) {
                joined = joined + ", ";
            }
            joined = joined + tried[i];
        }
        return "Style key `" + key + "` not found for TargetType `" + targetType + "` (tried: " + joined + ")";
    }

    /// <summary>
    /// 从已应用 Style 键串解析 Button chrome（末个命中胜出；无则 Primary）。
    /// </summary>
    public static string ButtonChromeFromStyleKeys(string styleKeys) {
        string recipe = "Primary";
        if (styleKeys == null || styleKeys == "") {
            return recipe;
        }
        string[] parts = styleKeys.Split(",");
        for (int i = 0; i < parts.Length; i++) {
            string part = parts[i].Trim();
            if (part == "") {
                continue;
            }
            string leaf = StyleKeyResolver.LeafToken(part);
            if (StyleKeyResolver.IsButtonChrome(leaf)) {
                recipe = leaf;
            }
        }
        return recipe;
    }

    static string LeafToken(string key) {
        int dot = -1;
        for (int i = 0; i < key.Length; i++) {
            if (key[i] == '.') {
                dot = i;
            }
        }
        if (dot < 0 || dot + 1 >= key.Length) {
            return key;
        }
        return key.Substring(dot + 1);
    }

    static bool IsButtonChrome(string leaf) {
        return leaf == "Primary"
            || leaf == "Default"
            || leaf == "Ghost"
            || leaf == "Dashed"
            || leaf == "Text"
            || leaf == "Link"
            || leaf == "Danger";
    }

    static bool ContainsDot(string s) {
        for (int i = 0; i < s.Length; i++) {
            if (s[i] == '.') {
                return true;
            }
        }
        return false;
    }
}
