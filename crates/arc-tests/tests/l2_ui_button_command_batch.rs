//! L2 批量：Button.Command → ICommand.Execute（MVVM 命令最小面）。
//!
//! 验收面（headless）：
//! - `relay_execute_on_raise`：`RelayCommand` + `RaiseClick` 执行且传 CommandParameter
//! - `can_execute_false_skips`：CanExecute=false 时不 Execute（Clicked 仍可发）
//! - `disabled_skips_all`：IsEnabled=false 时既不 Clicked 也不 Execute
//!
//! 宣称纪律：仅关 RaiseClick×ICommand 同步查询面；**不**宣称 CanExecuteChanged→
//! IsEnabled 自动同步、`{x:Bind}`/`{Binding}` Command 标记扩展、RoutedCommand。
//! 需 `--features full-rt`。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

const UI_DEPS: &[(&str, &str)] = &[("Arc.UI", "UI/Core")];

#[test]
fn ui_button_command_batch() {
    let results = build_and_run_batch_with_deps(
        "ui_button_command",
        &[
            BatchCase {
                name: "relay_execute_on_raise",
                src: r##"using Arc;
using Arc.UI.Components;

class CmdHost {
    public static int Hits;
    public static object LastParam;
    public static int ClickHits;

    public static void OnExec(object p) {
        Hits = Hits + 1;
        LastParam = p;
    }

    public static void OnClick(bool v) {
        ClickHits = ClickHits + 1;
    }
}

void Main() {
    CmdHost.Hits = 0;
    CmdHost.LastParam = null;
    CmdHost.ClickHits = 0;
    Button btn = new Button();
    btn.CommandParameter = "p1";
    btn.Command = new RelayCommand(CmdHost.OnExec);
    btn.OnClick(CmdHost.OnClick);
    btn.RaiseClick();
    if (CmdHost.Hits != 1 || CmdHost.ClickHits != 1) {
        Console.WriteLine("ARC_CASE:relay_execute_on_raise:FAIL:hits=" + CmdHost.Hits.ToString() + " clicks=" + CmdHost.ClickHits.ToString());
        return;
    }
    if (CmdHost.LastParam == null || (string)CmdHost.LastParam != "p1") {
        Console.WriteLine("ARC_CASE:relay_execute_on_raise:FAIL:param");
        return;
    }
    Console.WriteLine("ARC_CASE:relay_execute_on_raise:PASS");
}
"##,
            },
            BatchCase {
                name: "can_execute_false_skips",
                src: r##"using Arc;
using Arc.UI.Components;

class CmdGate {
    public static int Hits;
    public static int ClickHits;
    public static bool Allow;

    public static void OnExec(object p) {
        Hits = Hits + 1;
    }

    public static bool OnCan(object p) {
        return Allow;
    }

    public static void OnClick(bool v) {
        ClickHits = ClickHits + 1;
    }
}

void Main() {
    CmdGate.Hits = 0;
    CmdGate.ClickHits = 0;
    CmdGate.Allow = false;
    Button btn = new Button();
    btn.Command = new RelayCommand(CmdGate.OnExec, CmdGate.OnCan);
    btn.OnClick(CmdGate.OnClick);
    btn.RaiseClick();
    if (CmdGate.Hits != 0 || CmdGate.ClickHits != 1) {
        Console.WriteLine("ARC_CASE:can_execute_false_skips:FAIL:hits=" + CmdGate.Hits.ToString() + " clicks=" + CmdGate.ClickHits.ToString());
        return;
    }
    CmdGate.Allow = true;
    btn.RaiseClick();
    if (CmdGate.Hits != 1 || CmdGate.ClickHits != 2) {
        Console.WriteLine("ARC_CASE:can_execute_false_skips:FAIL:after_allow hits=" + CmdGate.Hits.ToString() + " clicks=" + CmdGate.ClickHits.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:can_execute_false_skips:PASS");
}
"##,
            },
            BatchCase {
                name: "disabled_skips_all",
                src: r##"using Arc;
using Arc.UI.Components;

class CmdOff {
    public static int Hits;
    public static int ClickHits;

    public static void OnExec(object p) {
        Hits = Hits + 1;
    }

    public static void OnClick(bool v) {
        ClickHits = ClickHits + 1;
    }
}

void Main() {
    CmdOff.Hits = 0;
    CmdOff.ClickHits = 0;
    Button btn = new Button();
    btn.IsEnabled = false;
    btn.Command = new RelayCommand(CmdOff.OnExec);
    btn.OnClick(CmdOff.OnClick);
    btn.RaiseClick();
    if (CmdOff.Hits != 0 || CmdOff.ClickHits != 0) {
        Console.WriteLine("ARC_CASE:disabled_skips_all:FAIL:hits=" + CmdOff.Hits.ToString() + " clicks=" + CmdOff.ClickHits.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:disabled_skips_all:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );
    assert!(batch_case_result(&results, "relay_execute_on_raise").passed);
    assert!(batch_case_result(&results, "can_execute_false_skips").passed);
    assert!(batch_case_result(&results, "disabled_skips_all").passed);
}
