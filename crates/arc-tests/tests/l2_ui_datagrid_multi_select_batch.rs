//! L2 批量：DataGrid / MultiSelector 程序化多选 + Ctrl/Shift 修饰键手势。
//!
//! 验收面（headless）：
//! - `datagrid_selectitem_subscribe` / `datagrid_selectall_count`：程序化多选 Subscribe
//! - `datagrid_mods_ctrl_toggle`：Ctrl(bit1) 切换加入/移出
//! - `datagrid_mods_shift_range`：Shift(bit0) 锚点范围选；无修饰替换单行
//!
//! 宣称纪律：程序化多选 ✅；Ctrl/Shift 手势最小面 ✅（模拟 PointerRouter）；
//! GUI 真键手测后置。需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_datagrid_multi_select_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_datagrid_multi_select",
        &[
            BatchCase {
                name: "datagrid_selectitem_subscribe",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class MdMultiHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(DataGrid grid) {
        grid.OnSelectionChanged(MdMultiHost.OnSel);
    }
}

void Main() {
    MdMultiHost.Hits = 0;
    MdMultiHost.Last = "";
    DataGrid grid = new DataGrid();
    grid.AddColumn("N", 80.0);
    List<List<string>> rows = new List<List<string>>();
    List<string> r0 = new List<string>();
    r0.Add("r0");
    List<string> r1 = new List<string>();
    r1.Add("r1");
    List<string> r2 = new List<string>();
    r2.Add("r2");
    rows.Add(r0);
    rows.Add(r1);
    rows.Add(r2);
    grid.ItemsSource = rows;
    grid.SelectionMode = "Multiple";
    MdMultiHost.Wire(grid);
    grid.SelectItem(0);
    grid.SelectItem(2);
    int selCount = grid.SelectedItems.Count;
    if (MdMultiHost.Hits != 2 || MdMultiHost.Last != "r2" || selCount != 2
        || grid.SelectedIndex != 2 || grid.SelectionChanged.Value != "r2") {
        Console.WriteLine(
            "ARC_CASE:datagrid_selectitem_subscribe:FAIL:hits=" + MdMultiHost.Hits.ToString()
            + " last=" + MdMultiHost.Last
            + " selCount=" + selCount.ToString()
            + " idx=" + grid.SelectedIndex.ToString()
            + " value=" + grid.SelectionChanged.Value);
        return;
    }
    Console.WriteLine("ARC_CASE:datagrid_selectitem_subscribe:PASS");
}
"##,
            },
            BatchCase {
                name: "datagrid_selectall_count",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class MdAllHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(DataGrid grid) {
        grid.OnSelectionChanged(MdAllHost.OnSel);
    }
}

void Main() {
    MdAllHost.Hits = 0;
    MdAllHost.Last = "";
    DataGrid grid = new DataGrid();
    grid.AddColumn("N", 80.0);
    List<List<string>> rows = new List<List<string>>();
    List<string> r0 = new List<string>();
    r0.Add("a");
    List<string> r1 = new List<string>();
    r1.Add("b");
    List<string> r2 = new List<string>();
    r2.Add("c");
    rows.Add(r0);
    rows.Add(r1);
    rows.Add(r2);
    grid.ItemsSource = rows;
    grid.SelectionMode = "Multiple";
    MdAllHost.Wire(grid);
    grid.SelectAll();
    int selCount = grid.SelectedItems.Count;
    if (MdAllHost.Hits != 1 || selCount != 3 || MdAllHost.Last != "c"
        || grid.SelectedIndex != 2 || grid.SelectionChanged.Value != "c") {
        Console.WriteLine(
            "ARC_CASE:datagrid_selectall_count:FAIL:hits=" + MdAllHost.Hits.ToString()
            + " last=" + MdAllHost.Last
            + " selCount=" + selCount.ToString()
            + " idx=" + grid.SelectedIndex.ToString()
            + " value=" + grid.SelectionChanged.Value);
        return;
    }
    grid.ClearSelection();
    if (grid.SelectedItems.Count != 0 || grid.SelectedIndex != -1 || MdAllHost.Hits != 2) {
        Console.WriteLine(
            "ARC_CASE:datagrid_selectall_count:FAIL:clear hits=" + MdAllHost.Hits.ToString()
            + " selCount=" + grid.SelectedItems.Count.ToString()
            + " idx=" + grid.SelectedIndex.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:datagrid_selectall_count:PASS");
}
"##,
            },
            BatchCase {
                name: "datagrid_mods_ctrl_toggle",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class MdCtrlHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(DataGrid grid) {
        grid.OnSelectionChanged(MdCtrlHost.OnSel);
    }
}

void Main() {
    MdCtrlHost.Hits = 0;
    MdCtrlHost.Last = "";
    DataGrid grid = new DataGrid();
    grid.AddColumn("N", 80.0);
    List<List<string>> rows = new List<List<string>>();
    List<string> r0 = new List<string>();
    r0.Add("r0");
    List<string> r1 = new List<string>();
    r1.Add("r1");
    List<string> r2 = new List<string>();
    r2.Add("r2");
    rows.Add(r0);
    rows.Add(r1);
    rows.Add(r2);
    grid.ItemsSource = rows;
    grid.SelectionMode = "Multiple";
    MdCtrlHost.Wire(grid);
    grid.SelectIndexWithMods(0, 0);
    grid.SelectIndexWithMods(2, 2);
    int afterAdd = grid.SelectedItems.Count;
    grid.SelectIndexWithMods(0, 2);
    int afterRemove = grid.SelectedItems.Count;
    if (MdCtrlHost.Hits != 3 || afterAdd != 2 || afterRemove != 1
        || grid.SelectedIndex != 2 || MdCtrlHost.Last != "r2") {
        Console.WriteLine(
            "ARC_CASE:datagrid_mods_ctrl_toggle:FAIL:hits=" + MdCtrlHost.Hits.ToString()
            + " afterAdd=" + afterAdd.ToString()
            + " afterRemove=" + afterRemove.ToString()
            + " idx=" + grid.SelectedIndex.ToString()
            + " last=" + MdCtrlHost.Last);
        return;
    }
    Console.WriteLine("ARC_CASE:datagrid_mods_ctrl_toggle:PASS");
}
"##,
            },
            BatchCase {
                name: "datagrid_mods_shift_range",
                src: r##"using Arc;
using Arc.Collections;
using Arc.UI.Components;

class MdShiftHost {
    public static int Hits;
    public static string Last;

    public static void OnSel(string v) {
        Hits = Hits + 1;
        Last = v;
    }

    public static void Wire(DataGrid grid) {
        grid.OnSelectionChanged(MdShiftHost.OnSel);
    }
}

void Main() {
    MdShiftHost.Hits = 0;
    MdShiftHost.Last = "";
    DataGrid grid = new DataGrid();
    grid.AddColumn("N", 80.0);
    List<List<string>> rows = new List<List<string>>();
    List<string> r0 = new List<string>();
    r0.Add("r0");
    List<string> r1 = new List<string>();
    r1.Add("r1");
    List<string> r2 = new List<string>();
    r2.Add("r2");
    List<string> r3 = new List<string>();
    r3.Add("r3");
    rows.Add(r0);
    rows.Add(r1);
    rows.Add(r2);
    rows.Add(r3);
    grid.ItemsSource = rows;
    grid.SelectionMode = "Multiple";
    MdShiftHost.Wire(grid);
    grid.SelectIndexWithMods(1, 0);
    grid.SelectIndexWithMods(3, 1);
    int rangeCount = grid.SelectedItems.Count;
    if (MdShiftHost.Hits != 2 || rangeCount != 3 || grid.SelectedIndex != 3
        || MdShiftHost.Last != "r3") {
        Console.WriteLine(
            "ARC_CASE:datagrid_mods_shift_range:FAIL:hits=" + MdShiftHost.Hits.ToString()
            + " rangeCount=" + rangeCount.ToString()
            + " idx=" + grid.SelectedIndex.ToString()
            + " last=" + MdShiftHost.Last);
        return;
    }
    grid.SelectIndexWithMods(0, 0);
    if (grid.SelectedItems.Count != 1 || grid.SelectedIndex != 0
        || MdShiftHost.Hits != 3 || MdShiftHost.Last != "r0") {
        Console.WriteLine(
            "ARC_CASE:datagrid_mods_shift_range:FAIL:plain hits=" + MdShiftHost.Hits.ToString()
            + " selCount=" + grid.SelectedItems.Count.ToString()
            + " idx=" + grid.SelectedIndex.ToString()
            + " last=" + MdShiftHost.Last);
        return;
    }
    Console.WriteLine("ARC_CASE:datagrid_mods_shift_range:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );

    for name in [
        "datagrid_selectitem_subscribe",
        "datagrid_selectall_count",
        "datagrid_mods_ctrl_toggle",
        "datagrid_mods_shift_range",
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
