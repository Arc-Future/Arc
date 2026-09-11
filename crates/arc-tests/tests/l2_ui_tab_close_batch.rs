//! L2 批量：TabControl 关闭按钮（选中常显 / 未选中悬停 + CloseTab）。
//!
//! 验收面（headless）：
//! - `close_selected_slides_next`：关选中页 → 右侧滑入同索引
//! - `close_last_selected_goes_prev`：关末页选中 → 选左侧
//! - `close_before_selected_decrements`：关选中左侧 → SelectedIndex-1
//! - `close_after_keeps_and_oob`：关右侧不改选中；越界 / 再关 no-op
//! - `close_via_item_last_empty`：TabItem.Close 关最后一页 → 开页 0
//! - `close_glyph_active_always_inactive_hover`：选中常显；未选中仅 HoverTabIndex
//!
//! 宣称纪律：VSCode 悬停显隐 + CloseTab 选中调整 + IsClosed 仍留 Children；
//! **不**宣称切换动画、Tooltip、`rt_ui_element_remove_child`。
//! 需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_tab_close_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_tab_close",
        &[
            BatchCase {
                name: "close_selected_slides_next",
                src: r##"using Arc;
using Arc.UI.Components;

class TabCloseHost {
    public static int OpenCount(TabControl tabs) {
        int n = 0;
        int i = 0;
        while (i < tabs.Children.Count)
        {
            TabItem page = (TabItem)tabs.Children[i];
            if (!page.IsClosed)
            {
                n = n + 1;
            }
            i++;
        }
        return n;
    }
}

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    TabItem b = new TabItem();
    b.Header = "B";
    TabItem c = new TabItem();
    c.Header = "C";
    tabs.AddChild(a);
    tabs.AddChild(b);
    tabs.AddChild(c);
    tabs.SelectedIndex = 1;
    tabs.CloseTab(1);
    if (!b.IsClosed || a.IsClosed || c.IsClosed)
    {
        Console.WriteLine("ARC_CASE:close_selected_slides_next:FAIL:closed");
        return;
    }
    if (TabCloseHost.OpenCount(tabs) != 2 || tabs.Children.Count != 3)
    {
        Console.WriteLine("ARC_CASE:close_selected_slides_next:FAIL:count");
        return;
    }
    if (tabs.SelectedIndex != 1)
    {
        Console.WriteLine("ARC_CASE:close_selected_slides_next:FAIL:idx=" + tabs.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:close_selected_slides_next:PASS");
}
"##,
            },
            BatchCase {
                name: "close_last_selected_goes_prev",
                src: r##"using Arc;
using Arc.UI.Components;

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    TabItem b = new TabItem();
    b.Header = "B";
    TabItem c = new TabItem();
    c.Header = "C";
    tabs.AddChild(a);
    tabs.AddChild(b);
    tabs.AddChild(c);
    tabs.SelectedIndex = 2;
    tabs.CloseTab(2);
    if (!c.IsClosed || tabs.SelectedIndex != 1)
    {
        Console.WriteLine("ARC_CASE:close_last_selected_goes_prev:FAIL:idx=" + tabs.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:close_last_selected_goes_prev:PASS");
}
"##,
            },
            BatchCase {
                name: "close_before_selected_decrements",
                src: r##"using Arc;
using Arc.UI.Components;

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    TabItem b = new TabItem();
    b.Header = "B";
    TabItem c = new TabItem();
    c.Header = "C";
    tabs.AddChild(a);
    tabs.AddChild(b);
    tabs.AddChild(c);
    tabs.SelectedIndex = 2;
    tabs.CloseTab(0);
    if (!a.IsClosed || tabs.SelectedIndex != 1)
    {
        Console.WriteLine("ARC_CASE:close_before_selected_decrements:FAIL:idx=" + tabs.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:close_before_selected_decrements:PASS");
}
"##,
            },
            BatchCase {
                name: "close_after_keeps_and_oob",
                src: r##"using Arc;
using Arc.UI.Components;

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    TabItem b = new TabItem();
    b.Header = "B";
    TabItem c = new TabItem();
    c.Header = "C";
    tabs.AddChild(a);
    tabs.AddChild(b);
    tabs.AddChild(c);
    tabs.SelectedIndex = 0;
    tabs.CloseTab(2);
    if (!c.IsClosed || tabs.SelectedIndex != 0)
    {
        Console.WriteLine("ARC_CASE:close_after_keeps_and_oob:FAIL:after");
        return;
    }
    tabs.CloseTab(-1);
    tabs.CloseTab(9);
    if (a.IsClosed || b.IsClosed || tabs.SelectedIndex != 0)
    {
        Console.WriteLine("ARC_CASE:close_after_keeps_and_oob:FAIL:oob");
        return;
    }
    c.Close();
    if (!c.IsClosed || tabs.SelectedIndex != 0)
    {
        Console.WriteLine("ARC_CASE:close_after_keeps_and_oob:FAIL:reclose");
        return;
    }
    Console.WriteLine("ARC_CASE:close_after_keeps_and_oob:PASS");
}
"##,
            },
            BatchCase {
                name: "close_via_item_last_empty",
                src: r##"using Arc;
using Arc.UI.Components;

class TabEmptyHost {
    public static int OpenCount(TabControl tabs) {
        int n = 0;
        int i = 0;
        while (i < tabs.Children.Count)
        {
            TabItem page = (TabItem)tabs.Children[i];
            if (!page.IsClosed)
            {
                n = n + 1;
            }
            i++;
        }
        return n;
    }
}

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    tabs.AddChild(a);
    a.Close();
    if (!a.IsClosed || TabEmptyHost.OpenCount(tabs) != 0 || tabs.Children.Count != 1)
    {
        Console.WriteLine("ARC_CASE:close_via_item_last_empty:FAIL:empty");
        return;
    }
    if (tabs.SelectedIndex != 0)
    {
        Console.WriteLine("ARC_CASE:close_via_item_last_empty:FAIL:idx=" + tabs.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:close_via_item_last_empty:PASS");
}
"##,
            },
            BatchCase {
                name: "close_glyph_active_always_inactive_hover",
                src: r##"using Arc;
using Arc.UI.Components;

void Main() {
    TabControl tabs = new TabControl();
    TabItem a = new TabItem();
    a.Header = "A";
    TabItem b = new TabItem();
    b.Header = "B";
    TabItem c = new TabItem();
    c.Header = "C";
    tabs.AddChild(a);
    tabs.AddChild(b);
    tabs.AddChild(c);
    tabs.SelectedIndex = 0;
    if (!tabs.IsCloseGlyphVisible(0) || tabs.IsCloseGlyphVisible(1) || tabs.IsCloseGlyphVisible(2))
    {
        Console.WriteLine("ARC_CASE:close_glyph_active_always_inactive_hover:FAIL:idle");
        return;
    }
    tabs.HoverTabIndex = 1;
    if (!tabs.IsCloseGlyphVisible(0) || !tabs.IsCloseGlyphVisible(1) || tabs.IsCloseGlyphVisible(2))
    {
        Console.WriteLine("ARC_CASE:close_glyph_active_always_inactive_hover:FAIL:hover");
        return;
    }
    tabs.HoverTabIndex = -1;
    if (!tabs.IsCloseGlyphVisible(0) || tabs.IsCloseGlyphVisible(1) || tabs.HoverTabIndex != -1)
    {
        Console.WriteLine("ARC_CASE:close_glyph_active_always_inactive_hover:FAIL:leave");
        return;
    }
    tabs.HoverTabIndex = 2;
    tabs.CloseTab(2);
    if (tabs.HoverTabIndex != -1 || tabs.IsCloseGlyphVisible(2) || !c.IsClosed)
    {
        Console.WriteLine("ARC_CASE:close_glyph_active_always_inactive_hover:FAIL:closeHover");
        return;
    }
    Console.WriteLine("ARC_CASE:close_glyph_active_always_inactive_hover:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );
    assert!(batch_case_result(&results, "close_selected_slides_next").passed);
    assert!(batch_case_result(&results, "close_last_selected_goes_prev").passed);
    assert!(batch_case_result(&results, "close_before_selected_decrements").passed);
    assert!(batch_case_result(&results, "close_after_keeps_and_oob").passed);
    assert!(batch_case_result(&results, "close_via_item_last_empty").passed);
    assert!(batch_case_result(&results, "close_glyph_active_always_inactive_hover").passed);
}
