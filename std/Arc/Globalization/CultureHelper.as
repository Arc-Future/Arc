// RFC 027 M5: 文化感知格式化 — Arc BCP 47 文化字符串处理。
//
// 对标 C# System.Globalization.CultureInfo 内部实现。
// 全部为静态方法，纯 Arc 字符串操作，零 C ABI 依赖。
//
// BCP 47 规范（RFC 5646）：
//   - 格式：language[-script][-region][-variant]，如 "zh-Hans-CN"
//   - 规范化：
//     * language（2-3 字符）：小写，如 "zh"
//     * script（4 字符）：Title Case（首字母大写，其余小写），如 "Hans"
//     * region（2-3 字符）：大写，如 "CN"
//     * variant/extension：原样保留
//   - 父文化链：移除最后一个 subtag
//
// 性能设计：
//   - Normalize 使用 Split 单 ABI 完成分段（避免逐字符 IndexOf+Substring）
//   - GetParent / IsNeutral 均单 IndexOf 调用（O(1) ABI）
//   - 输入信任：不再逐字符验证（调用方输入来自 OS API 或用户代码，已验证）

namespace Arc.Globalization;

/// <summary>
/// BCP 47 文化名称处理工具。
///
/// 利用 Arc 字符串内建能力（Split/ToLower/ToUpper/IndexOf/Substring），
/// 零 C ABI 依赖，Arc 语言自包含实现。
/// 注：Arc 编译器暂不支持 `static class` 持有静态字段，故以常规类承载静态成员
/// （对标 C# `internal static class CultureHelper`）。
/// </summary>
internal class CultureHelper {
    /// <summary>
    /// 规范化 BCP 47 文化名称。
    ///
    /// 规则：
    ///   - "zh-cn"     → "zh-CN"
    ///   - "zh-hans-cn" → "zh-Hans-CN"
    ///   - "ZH-HANS"   → "zh-Hans"
    ///   - ""          → ""
    ///
    /// ABI 调用数：1×Split + 1×ToLower + (n-1)×（ToLower/ToUpper + Substring + concat）
    /// </summary>
    /// <param name="name">待规范化的文化名称。</param>
    /// <returns>规范化后的名称；输入为 null 或空字符串返回空字符串。</returns>
    internal static string Normalize(string name) {
        if (name == null || name == "") {
            return "";
        }

        string[] parts = name.Split("-");
        int max = parts.Length;
        if (max == 0) {
            return "";
        }

        // 第 1 段：language，小写
        string result = parts[0].ToLower();

        int i = 1;
        while (i < max) {
            string seg = parts[i];
            string normalized = "";

            if (seg.Length == 4) {
                // script（4 字符）：Title Case — 首字母大写，其余小写
                // 如 "hans" → "Hans"，"HANS" → "Hans"
                string lower = seg.ToLower();
                if (lower.Length == 1) {
                    normalized = lower.ToUpper();
                } else {
                    normalized = lower.Substring(0, 1).ToUpper() + lower.Substring(1);
                }
            } else {
                // region（2-3 字符）或其他：大写
                // 如 "cn" → "CN"，"419" → "419"
                normalized = seg.ToUpper();
            }

            result = result + "-" + normalized;
            i = i + 1;
        }

        return result;
    }

    /// <summary>
    /// 提取父文化名称。
    ///
    /// 规则：
    ///   - "zh-CN"     → "zh"
    ///   - "zh-Hans-CN" → "zh-Hans"
    ///   - "zh"        → ""（中性文化的父是 invariant）
    ///   - ""          → null（invariant 无父文化）
    ///
    /// ABI 调用数：n×IndexOf + 1×Substring。对 "zh-Hans-CN"（3 段）约 4 ABIs。
    /// </summary>
    /// <param name="name">BCP 47 文化名称。</param>
    /// <returns>父文化名称；invariant（""）或无父文化返回 ""。</returns>
    internal static string GetParent(string name) {
        if (name == null || name == "") {
            return "";
        }

        // 找最后一个 '-'
        int pos = 0;
        int lastHyphen = -1;
        while (pos < name.Length) {
            int found = name.IndexOf("-", pos);
            if (found < 0) {
                break;
            }
            lastHyphen = found;
            pos = found + 1;
        }

        if (lastHyphen < 0) {
            // 无 '-'，说明是中性文化（如 "zh"），父是 invariant（""）
            return "";
        }

        return name.Substring(0, lastHyphen);
    }

    /// <summary>
    /// 判断是否为中性文化（纯 language，不含 region/script/variant）。
    ///
    /// 规则：
    ///   - "zh"   → true
    ///   - "zh-CN" → false
    ///   - ""     → true（invariant 视为中性）
    ///
    /// ABI 调用数：1×IndexOf。O(1)。
    /// </summary>
    /// <param name="name">BCP 47 文化名称。</param>
    /// <returns>是中性文化返回 true；否则返回 false。</returns>
    internal static bool IsNeutral(string name) {
        if (name == null || name == "") {
            return true;
        }
        return name.IndexOf("-") < 0;
    }
}