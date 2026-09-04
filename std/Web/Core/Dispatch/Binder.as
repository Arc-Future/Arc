// Binder —— 请求绑定器（RFC 040 §1.8 · internal · 显式命名约定）。
namespace Arc.Web;
using Arc.Collections;
using Arc.Text;

/// <summary>
/// 绑定器（internal）：按 HTTP 方法构造绑定 JSON——
///   - GET/DELETE：从路径参数构造对象 `{ "name": value, ... }`；全数字值按 number
///     发射（对齐 `int Id` 绑定 `{id}`），否则按 string 发射。
///   - POST/PUT/PATCH：直接用请求体（JSON 整体反序列化到请求）。
/// 显式命名约定（属性名 ↔ 段名），无 binder/provider 顺序复杂度。
/// </summary>
internal static class Binder {
    public static string Build(string method, RouteMatch match, string body) {
        string m = method.ToUpper();
        if (m == "POST" || m == "PUT" || m == "PATCH") {
            // M-B：body 绑定（路径参数合并进 body 后置，见 RFC 040 §1.8 诚实边界）。
            return body;
        }
        // GET/DELETE：路径参数对象。
        StringBuilder sb = new StringBuilder();
        sb.Append("{");
        bool first = true;
        for (int i = 0; i < match.ParamNames.Count; i++) {
            if (!first) { sb.Append(","); }
            sb.Append("\"");
            sb.Append(match.ParamNames[i]);
            sb.Append("\":");
            string v = match.ParamValues[i];
            if (IsNumeric(v)) {
                sb.Append(v);
            } else {
                sb.Append("\"");
                sb.Append(Escape(v));
                sb.Append("\"");
            }
            first = false;
        }
        sb.Append("}");
        return sb.ToString();
    }

    private static bool IsNumeric(string s) {
        if (s == "") { return false; }
        for (int i = 0; i < s.Length; i++) {
            string c = s.Substring(i, 1);
            bool isDigit = c == "0" || c == "1" || c == "2" || c == "3" || c == "4" ||
                c == "5" || c == "6" || c == "7" || c == "8" || c == "9";
            if (!isDigit) { return false; }
        }
        return true;
    }

    private static string Escape(string s) {
        string r = "";
        for (int i = 0; i < s.Length; i++) {
            string c = s.Substring(i, 1);
            if (c == "\"") { r = r + "\\\""; }
            else if (c == "\\") { r = r + "\\\\"; }
            else { r = r + c; }
        }
        return r;
    }
}
