// RFC 037 M3 / RFC 037 — SetterValue 提取辅助（单层 switch，供 StyleEvaluator 调用）。

namespace Arc.UI.Styling;

/// <summary>从 SetterValue 提取标量；StaticResource 须先 resolve。</summary>
internal class SetterValueHelper {
    public static string StringOrEmpty(SetterValue v) {
        switch (v)
        {
            case SetterValue.String(s):
            {
                return s;
            }
            default:
            {
                return "";
            }
        }
    }

    public static double NumberOrZero(SetterValue v) {
        double n = 0.0;
        switch (v)
        {
            case SetterValue.Number(x):
            {
                n = x;
            }
            default:
            {
            }
        }
        return n;
    }

    public static bool BooleanOrFalse(SetterValue v) {
        bool b = false;
        switch (v)
        {
            case SetterValue.Boolean(x):
            {
                b = x;
            }
            default:
            {
            }
        }
        return b;
    }

    public static SetterValue ResolveStatic(SetterValue v, ResourceDictionary resources) {
        switch (v)
        {
            case SetterValue.StaticResource(key):
            {
                SetterValue resolved = SetterValue.String("");
                if (SetterValueHelper.TryResolveKey(resources, key, ref resolved)) {
                    return resolved;
                }
                return SetterValue.String("");
            }
            default:
            {
                return v;
            }
        }
    }

    /// <summary>
    /// 按键解析资源为 SetterValue（未命中返回 false，调用方决定回退语义）。
    /// {StaticResource} 引用轨的解析底座；Brush → hex 字符串折叠经
    /// ResourceToSetter（ApplyDp Brush 分支还原）。
    /// </summary>
    public static bool TryResolveKey(ResourceDictionary resources, string key, ref SetterValue value) {
        if (resources == null || key == null || key == "") {
            return false;
        }
        ResourceValue v = ResourceValue.String("");
        if (!resources.TryLookup(key, ref v)) {
            return false;
        }
        value = SetterValueHelper.ResourceToSetter(v);
        return true;
    }

    public static SetterValue ResourceToSetter(ResourceValue rv) {
        switch (rv)
        {
            case ResourceValue.String(s):
            {
                return SetterValue.String(s);
            }
            case ResourceValue.Brush(b):
            {
                return SetterValue.String(b.ToHex());
            }
            case ResourceValue.Number(n):
            {
                return SetterValue.Number(n);
            }
            case ResourceValue.Boolean(b):
            {
                return SetterValue.Boolean(b);
            }
            default:
            {
                return SetterValue.String("");
            }
        }
    }
}
