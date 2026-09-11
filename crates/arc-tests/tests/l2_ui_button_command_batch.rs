//! L2 批量：Button.Command → ICommand（Execute + CanExecuteChanged→IsEnabled）。
//!
//! 验收面（headless）：
//! - `relay_execute_on_raise`：`RelayCommand` + `RaiseClick` 执行且传 CommandParameter
//! - `can_execute_false_on_assign`：赋 Command 时 CanExecute=false → IsEnabled=false，点击全跳过
//! - `can_execute_changed_syncs`：RaiseCanExecuteChanged 翻转 IsEnabled 并恢复可点
//! - `disabled_after_command`：赋 Command 后再手写 IsEnabled=false 仍门控
//!
//! 宣称纪律：关 CanExecuteChanged→IsEnabled 直写同步 + RaiseClick×Execute；
//! **不**宣称 Converter/ElementName/RelativeSource、RoutedCommand、CommandManager.RequerySuggested、
//! WPF IsEnabledCore 合取语义。需 `--features full-rt`。

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
                name: "can_execute_false_on_assign",
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
    btn.OnClick(CmdGate.OnClick);
    btn.Command = new RelayCommand(CmdGate.OnExec, CmdGate.OnCan);
    if (btn.IsEnabled) {
        Console.WriteLine("ARC_CASE:can_execute_false_on_assign:FAIL:still_enabled");
        return;
    }
    btn.RaiseClick();
    if (CmdGate.Hits != 0 || CmdGate.ClickHits != 0) {
        Console.WriteLine("ARC_CASE:can_execute_false_on_assign:FAIL:hits=" + CmdGate.Hits.ToString() + " clicks=" + CmdGate.ClickHits.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:can_execute_false_on_assign:PASS");
}
"##,
            },
            BatchCase {
                name: "can_execute_changed_syncs",
                src: r##"using Arc;
using Arc.UI.Components;

class CmdFlip {
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
    CmdFlip.Hits = 0;
    CmdFlip.ClickHits = 0;
    CmdFlip.Allow = true;
    Button btn = new Button();
    btn.OnClick(CmdFlip.OnClick);
    RelayCommand cmd = new RelayCommand(CmdFlip.OnExec, CmdFlip.OnCan);
    btn.Command = cmd;
    if (!btn.IsEnabled) {
        Console.WriteLine("ARC_CASE:can_execute_changed_syncs:FAIL:initial_disabled");
        return;
    }
    CmdFlip.Allow = false;
    cmd.RaiseCanExecuteChanged();
    if (btn.IsEnabled) {
        Console.WriteLine("ARC_CASE:can_execute_changed_syncs:FAIL:not_disabled");
        return;
    }
    btn.RaiseClick();
    if (CmdFlip.Hits != 0 || CmdFlip.ClickHits != 0) {
        Console.WriteLine("ARC_CASE:can_execute_changed_syncs:FAIL:while_disabled hits=" + CmdFlip.Hits.ToString());
        return;
    }
    CmdFlip.Allow = true;
    cmd.RaiseCanExecuteChanged();
    if (!btn.IsEnabled) {
        Console.WriteLine("ARC_CASE:can_execute_changed_syncs:FAIL:not_reenabled");
        return;
    }
    btn.RaiseClick();
    if (CmdFlip.Hits != 1 || CmdFlip.ClickHits != 1) {
        Console.WriteLine("ARC_CASE:can_execute_changed_syncs:FAIL:after_reenable hits=" + CmdFlip.Hits.ToString() + " clicks=" + CmdFlip.ClickHits.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:can_execute_changed_syncs:PASS");
}
"##,
            },
            BatchCase {
                name: "disabled_after_command",
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
    btn.Command = new RelayCommand(CmdOff.OnExec);
    btn.OnClick(CmdOff.OnClick);
    btn.IsEnabled = false;
    btn.RaiseClick();
    if (CmdOff.Hits != 0 || CmdOff.ClickHits != 0) {
        Console.WriteLine("ARC_CASE:disabled_after_command:FAIL:hits=" + CmdOff.Hits.ToString() + " clicks=" + CmdOff.ClickHits.ToString());
        return;
    }
    Console.WriteLine("ARC_CASE:disabled_after_command:PASS");
}
"##,
            },
        ],
        UI_DEPS,
    );
    assert!(batch_case_result(&results, "relay_execute_on_raise").passed);
    assert!(batch_case_result(&results, "can_execute_false_on_assign").passed);
    assert!(batch_case_result(&results, "can_execute_changed_syncs").passed);
    assert!(batch_case_result(&results, "disabled_after_command").passed);
}
