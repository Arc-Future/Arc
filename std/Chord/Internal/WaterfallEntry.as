// WaterfallEntry —— 瀑布订阅条目（RFC 045 D5.1）。
//
// _dead 标记支持快照遍历中的惰性退订：Run 先快照订阅列表，回调中途
// 退订（置 dead）不影响本轮瀑布；由 Run 尾部紧凑清除。
namespace Arc.Chord;

using Arc;
using Arc.Collections;


internal class WaterfallEntry {
    internal bool _dead;
    internal Func<object, Func<object, object>, object> _handler;

    internal WaterfallEntry(Func<object, Func<object, object>, object> handler) {
        _handler = handler;
        _dead = false;
    }
}
