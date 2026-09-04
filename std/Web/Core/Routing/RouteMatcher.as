// RouteMatcher —— 路由匹配器（RFC 040 §1.7 · internal · 纯字符串逻辑，非反射）。
namespace Arc.Web;
using Arc.Collections;

/// <summary>
/// 路由匹配器（internal）：模板分段 + 实际 path 匹配 + 路径参数捕获。
/// `{name}` 段捕获任意单个路径段；静态段须精确相等。纯字符串逻辑。
/// </summary>
internal static class RouteMatcher {
    /// <summary>拆分模板为段：`/api/users/{id}` → ["api","users","{id}"]。</summary>
    public static List<string> SplitTemplate(string template) {
        List<string> segs = new List<string>();
        string t = template;
        if (t != null && t.Length > 0 && t.Substring(0, 1) == "/") {
            t = t.Substring(1, t.Length - 1);
        }
        int start = 0;
        while (true) {
            int slash = IndexOf(t, "/", start);
            if (slash < 0) {
                string seg = t.Substring(start, t.Length - start);
                if (seg != "") { segs.Add(seg); }
                break;
            }
            string s = t.Substring(start, slash - start);
            if (s != "") { segs.Add(s); }
            start = slash + 1;
        }
        return segs;
    }

    /// <summary>匹配：path 分段 vs 模板段；命中返回 RouteMatch，否则 null。</summary>
    public static RouteMatch Match(EndpointDescriptor endpoint, string path) {
        List<string> pathSegs = SplitTemplate(path);
        if (pathSegs.Count != endpoint.Segments.Count) { return null; }
        RouteMatch m = new RouteMatch(endpoint);
        for (int i = 0; i < pathSegs.Count; i++) {
            string tseg = endpoint.Segments[i];
            string pseg = pathSegs[i];
            bool isParam = tseg.Length >= 2 &&
                tseg.Substring(0, 1) == "{" &&
                tseg.Substring(tseg.Length - 1, 1) == "}";
            if (isParam) {
                string name = tseg.Substring(1, tseg.Length - 2);
                m.ParamNames.Add(name);
                m.ParamValues.Add(pseg);
            } else if (tseg == pseg) {
                // 静态段精确匹配。
            } else {
                return null;
            }
        }
        return m;
    }

    private static int IndexOf(string s, string sub, int start) {
        int n = s.Length;
        int m = sub.Length;
        for (int i = start; i <= n - m; i++) {
            bool ok = true;
            for (int j = 0; j < m; j++) {
                if (s.Substring(i + j, 1) != sub.Substring(j, 1)) {
                    ok = false;
                    break;
                }
            }
            if (ok) { return i; }
        }
        return -1;
    }
}
