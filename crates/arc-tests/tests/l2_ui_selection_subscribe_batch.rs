//! L2 批量：ListView/DataGrid SelectionChanged Signal.Subscribe 闭包链路（M-D0）。
//!
//! 验收面（headless）：
//! - `listview_subscribe_after_return`：`OnSelectionChanged` 在注册函数返回后，
//!   `SelectIndex` 仍触发回调（载荷=选中项文本）
//! - `datagrid_subscribe_after_return`：同上，DataGrid 载荷=行首列文本
//! - `signal_subscribe_direct`：`Signal<string>.Subscribe` 直挂（无包装 lambda）跨函数逃逸安全
//!
//! 宣称纪律：单选面 Subscribe 链路 ✅；GUI 点击手测仍后置（多选程序化面见 `ui_datagrid_multi_select`）。
//! 需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_selection_subscribe_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_selection_subscribe",
        &[
            BatchCase {
                name: "signal_subscribe_direct",
                src: r##"using Arc;

class Md0SigHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(Signal<string> sig) {
        sig.Subscribe(Md0SigHost.OnSel);
    }
}

void Main() {
    Md0SigHost.Hits = 0;
    Md0SigHost.Last = "";
    Signal<string> sig = new Signal<string>("");
    Md0SigHost.Wire(sig);
    sig.Set("ok");
    if (Md0SigHost.Hits != 1 || Md0SigHost.Last != "ok") {
        Console.WriteLine("ARC_CASE:signal_subscribe_direct:FAIL:hits=" + Md0SigHost.Hits.ToString() + " last=" + Md0SigHost.Last);
        return;
    }
    Console.WriteLine("ARC_CASE:signal_subscribe_direct:PASS");
}
"##,
            },
            BatchCase {
                name: "listview_subscribe_after_return",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class Md0ListHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(ListView lv) {
        lv.OnSelectionChanged(Md0ListHost.OnSel);
    }
}

void Main() {
    Md0ListHost.Hits = 0;
    Md0ListHost.Last = "";
    ListView lv = new ListView();
    List<string> items = new List<string>();
    items.Add("A");
    items.Add("B");
    items.Add("C");
    lv.ItemsSource = items;
    Md0ListHost.Wire(lv);
    lv.SelectIndex(1);
    if (Md0ListHost.Hits != 1 || Md0ListHost.Last != "B" || lv.SelectionChanged.Value != "B") {
        Console.WriteLine(
            "ARC_CASE:listview_subscribe_after_return:FAIL:hits=" + Md0ListHost.Hits.ToString()
            + " last=" + Md0ListHost.Last
            + " value=" + lv.SelectionChanged.Value
            + " idx=" + lv.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:listview_subscribe_after_return:PASS");
}
"##,
            },
            BatchCase {
                name: "datagrid_subscribe_after_return",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class Md0GridHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(DataGrid grid) {
        grid.OnSelectionChanged(Md0GridHost.OnSel);
    }
}

void Main() {
    Md0GridHost.Hits = 0;
    Md0GridHost.Last = "";
    DataGrid grid = new DataGrid();
    grid.AddColumn("N", 80.0);
    List<List<string>> rows = new List<List<string>>();
    List<string> r0 = new List<string>();
    r0.Add("r0");
    List<string> r1 = new List<string>();
    r1.Add("r1");
    rows.Add(r0);
    rows.Add(r1);
    grid.ItemsSource = rows;
    Md0GridHost.Wire(grid);
    grid.SelectIndex(1);
    if (Md0GridHost.Hits != 1 || Md0GridHost.Last != "r1" || grid.SelectionChanged.Value != "r1") {
        Console.WriteLine(
            "ARC_CASE:datagrid_subscribe_after_return:FAIL:hits=" + Md0GridHost.Hits.ToString()
            + " last=" + Md0GridHost.Last
            + " value=" + grid.SelectionChanged.Value
            + " idx=" + grid.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:datagrid_subscribe_after_return:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );

    for name in [
        "signal_subscribe_direct",
        "listview_subscribe_after_return",
        "datagrid_subscribe_after_return",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "{name}: passed={} err={:?}\nstdout:\n{}",
            r.passed,
            r.error,
            r.stdout
        );
    }
}