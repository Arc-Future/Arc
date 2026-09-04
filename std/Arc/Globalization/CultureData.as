// RFC 027 M5: 文化感知格式化 — 文化数据处理 CultureData。
//
// 对标 C# System.Globalization.CultureData（内部类）。
// 包含 60+ 常用 BCP 47 文化条目的显示名与自身语言名。
//
// 设计原则：
//   - 纯 Arc 实现，零 C ABI 依赖
//   - O(1) 查找（Dictionary + 回退线性扫描）
//   - 扩展路径——增至 CLDR 全量数据时切换为 .resources 二进制加载
//
// 性能设计：
//   - 主查找走 Dictionary（rt_dict_get: 1 ABI, O(1)）
//   - 未知文化回退走 Find 静态方法（返回规范化名称）

namespace Arc.Globalization;

/// <summary>文化数据条目——文化名称映射的单条记录。</summary>
internal struct CultureEntry {
    public string DisplayName;
    public string EnglishName;
    public string NativeName;
}

/// <summary>
/// 文化数据表——单一事实来源的文化名称映射。
///
/// 内部使用 Dictionary 实现 O(1) 查找。
/// 数据经 `static readonly` 惰性字段构建（RFC 006 A3 S6a：首触构造一次、线程安全）。
/// 注：Arc 编译器暂不支持 `static class` 持有静态字段，故以常规类承载静态成员
/// （与 `NumberFormatInfo` 缓存模式一致，对标 C# `internal static class CultureData`）。
/// </summary>
internal class CultureData {

    /// <summary>静态文化表（static readonly 惰性：首触经 _build 构造一次、线程安全）。</summary>
    private static readonly Dictionary<string, CultureEntry> _dict = CultureData._build();

    /// <summary>按 BCP 47 标签精确查找文化条目。O(1)。</summary>
    /// <returns>找到返回条目；未找到返回默认条目（display/english/native = tag 本身）。</returns>
    internal static CultureEntry Find(string tag) {
        if (tag == null) { tag = ""; }
        if (_dict.ContainsKey(tag)) {
            return _dict[tag];
        }
        // 未知文化——返回规范化名称作为显示名
        return new CultureEntry {
            DisplayName = tag,
            EnglishName = tag,
            NativeName = tag,
        };
    }

    // ── 静态文化表构建（static readonly 惰性：仅首触执行一次）──

    private static Dictionary<string, CultureEntry> _build() {
        Dictionary<string, CultureEntry> d = new Dictionary<string, CultureEntry>();

        // ── 60+ 文化条目（按使用频次排序）──

        _add(d, "",        "Invariant Language (Invariant Country)", "Invariant Language (Invariant Country)", "Invariant Language (Invariant Country)");
        _add(d, "zh-CN",   "Chinese (Simplified, China)",            "Chinese (Simplified, China)",            "中文(中国)");
        _add(d, "zh-Hans", "Chinese (Simplified)",                   "Chinese (Simplified)",                   "中文(简体)");
        _add(d, "zh-HK",   "Chinese (Traditional, Hong Kong SAR)",   "Chinese (Traditional, Hong Kong SAR)",   "中文(香港特別行政區)");
        _add(d, "zh-TW",   "Chinese (Traditional, Taiwan)",          "Chinese (Traditional, Taiwan)",          "中文(台灣)");
        _add(d, "zh",      "Chinese",                                "Chinese",                                "中文");
        _add(d, "en-US",   "English (United States)",                "English (United States)",                "English (United States)");
        _add(d, "en-GB",   "English (United Kingdom)",               "English (United Kingdom)",               "English (United Kingdom)");
        _add(d, "en-AU",   "English (Australia)",                    "English (Australia)",                    "English (Australia)");
        _add(d, "en-CA",   "English (Canada)",                       "English (Canada)",                       "English (Canada)");
        _add(d, "en",      "English",                                "English",                                "English");
        _add(d, "ja-JP",   "Japanese (Japan)",                       "Japanese (Japan)",                       "日本語");
        _add(d, "ja",      "Japanese",                               "Japanese",                               "日本語");
        _add(d, "ko-KR",   "Korean (Korea)",                         "Korean (Korea)",                         "한국어");
        _add(d, "ko",      "Korean",                                 "Korean",                                 "한국어");
        _add(d, "de-DE",   "German (Germany)",                       "German (Germany)",                       "Deutsch (Deutschland)");
        _add(d, "de",      "German",                                 "German",                                 "Deutsch");
        _add(d, "fr-FR",   "French (France)",                        "French (France)",                        "Français (France)");
        _add(d, "fr-CA",   "French (Canada)",                        "French (Canada)",                        "Français (Canada)");
        _add(d, "fr",      "French",                                 "French",                                 "Français");
        _add(d, "ru-RU",   "Russian (Russia)",                       "Russian (Russia)",                       "Русский (Россия)");
        _add(d, "ru",      "Russian",                                "Russian",                                "Русский");
        _add(d, "es-ES",   "Spanish (Spain)",                        "Spanish (Spain)",                        "Español (España)");
        _add(d, "es-MX",   "Spanish (Mexico)",                       "Spanish (Mexico)",                       "Español (México)");
        _add(d, "es",      "Spanish",                                "Spanish",                                "Español");
        _add(d, "pt-BR",   "Portuguese (Brazil)",                    "Portuguese (Brazil)",                    "Português (Brasil)");
        _add(d, "pt-PT",   "Portuguese (Portugal)",                  "Portuguese (Portugal)",                  "Português (Portugal)");
        _add(d, "pt",      "Portuguese",                             "Portuguese",                             "Português");
        _add(d, "it-IT",   "Italian (Italy)",                        "Italian (Italy)",                        "Italiano (Italia)");
        _add(d, "it",      "Italian",                                "Italian",                                "Italiano");
        _add(d, "nl-NL",   "Dutch (Netherlands)",                    "Dutch (Netherlands)",                    "Nederlands (Nederland)");
        _add(d, "nl",      "Dutch",                                  "Dutch",                                  "Nederlands");
        _add(d, "sv-SE",   "Swedish (Sweden)",                       "Swedish (Sweden)",                       "Svenska (Sverige)");
        _add(d, "sv",      "Swedish",                                "Swedish",                                "Svenska");
        _add(d, "pl-PL",   "Polish (Poland)",                        "Polish (Poland)",                        "Polski (Polska)");
        _add(d, "pl",      "Polish",                                 "Polish",                                 "Polski");
        _add(d, "ar-SA",   "Arabic (Saudi Arabia)",                  "Arabic (Saudi Arabia)",                  "العربية (المملكة العربية السعودية)");
        _add(d, "ar",      "Arabic",                                 "Arabic",                                 "العربية");
        _add(d, "tr-TR",   "Turkish (Turkey)",                       "Turkish (Turkey)",                       "Türkçe (Türkiye)");
        _add(d, "tr",      "Turkish",                                "Turkish",                                "Türkçe");
        _add(d, "cs-CZ",   "Czech (Czech Republic)",                 "Czech (Czech Republic)",                 "Čeština (Česká republika)");
        _add(d, "cs",      "Czech",                                  "Czech",                                  "Čeština");
        _add(d, "th-TH",   "Thai (Thailand)",                        "Thai (Thailand)",                        "ไทย (ไทย)");
        _add(d, "th",      "Thai",                                   "Thai",                                   "ไทย");
        _add(d, "vi-VN",   "Vietnamese (Vietnam)",                   "Vietnamese (Vietnam)",                   "Tiếng Việt (Việt Nam)");
        _add(d, "vi",      "Vietnamese",                             "Vietnamese",                             "Tiếng Việt");
        _add(d, "id-ID",   "Indonesian (Indonesia)",                 "Indonesian (Indonesia)",                 "Bahasa Indonesia (Indonesia)");
        _add(d, "id",      "Indonesian",                             "Indonesian",                             "Bahasa Indonesia");
        _add(d, "hi-IN",   "Hindi (India)",                          "Hindi (India)",                          "हिन्दी (भारत)");
        _add(d, "hi",      "Hindi",                                  "Hindi",                                  "हिन्दी");
        _add(d, "el-GR",   "Greek (Greece)",                         "Greek (Greece)",                         "Ελληνικά (Ελλάδα)");
        _add(d, "el",      "Greek",                                  "Greek",                                  "Ελληνικά");
        _add(d, "he-IL",   "Hebrew (Israel)",                        "Hebrew (Israel)",                        "עברית (ישראל)");
        _add(d, "he",      "Hebrew",                                 "Hebrew",                                 "עברית");
        _add(d, "da-DK",   "Danish (Denmark)",                       "Danish (Denmark)",                       "Dansk (Danmark)");
        _add(d, "da",      "Danish",                                 "Danish",                                 "Dansk");
        _add(d, "fi-FI",   "Finnish (Finland)",                      "Finnish (Finland)",                      "Suomi (Suomi)");
        _add(d, "fi",      "Finnish",                                "Finnish",                                "Suomi");
        _add(d, "nb-NO",   "Norwegian Bokmål (Norway)",              "Norwegian Bokmål (Norway)",              "Norsk bokmål (Norge)");
        _add(d, "nb",      "Norwegian Bokmål",                       "Norwegian Bokmål",                       "Norsk bokmål");
        _add(d, "ro-RO",   "Romanian (Romania)",                     "Romanian (Romania)",                     "Română (România)");
        _add(d, "ro",      "Romanian",                               "Romanian",                               "Română");
        _add(d, "hu-HU",   "Hungarian (Hungary)",                    "Hungarian (Hungary)",                    "Magyar (Magyarország)");
        _add(d, "hu",      "Hungarian",                              "Hungarian",                              "Magyar");
        _add(d, "uk-UA",   "Ukrainian (Ukraine)",                    "Ukrainian (Ukraine)",                    "Українська (Україна)");
        _add(d, "uk",      "Ukrainian",                              "Ukrainian",                              "Українська");
        _add(d, "ms-MY",   "Malay (Malaysia)",                       "Malay (Malaysia)",                       "Bahasa Melayu (Malaysia)");
        _add(d, "ms",      "Malay",                                  "Malay",                                  "Bahasa Melayu");
        _add(d, "ta-IN",   "Tamil (India)",                          "Tamil (India)",                          "தமிழ் (இந்தியா)");
        _add(d, "ta",      "Tamil",                                  "Tamil",                                  "தமிழ்");
        return d;
    }

    private static void _add(Dictionary<string, CultureEntry> d, string tag, string display, string english, string native) {
        d[tag] = new CultureEntry {
            DisplayName = display,
            EnglishName = english,
            NativeName = native,
        };
    }
}