//! L3 std 行为测试批：std P2 效率+基建批修复点验证
//! （List.Capacity 注解对齐 / BarcodeWriter 实心矩形批渲染 / ProcessStream 批量管道契约）。
//!
//! 通过 `build_and_run_batch[_with_deps]` 合并多个 case 为一次编译 + 一次运行。
//! 每个 case 自行输出 `ARC_CASE:{name}:PASS/FAIL` 标记。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{
    batch_case_result, build_and_run_batch, build_and_run_batch_with_deps, BatchCase,
};

#[test]
fn std_list_capacity_batch() {
    // List.Capacity 注解对齐（rt_list_capacity ABI）：getter 返回真实容量（>= Count），
    // setter 按值扩容且保留既有元素（realloc 语义）。
    let results = build_and_run_batch(
        "std_list_capacity",
        &[BatchCase {
            name: "list_capacity_grow",
            src: r#"using Arc;
using Arc.Collections;

void Main() {
    List<int> l = new List<int>();
    l.Add(1);
    l.Add(2);
    l.Add(3);
    if (l.Capacity < l.Count) { Console.WriteLine("ARC_CASE:list_capacity_grow:FAIL:capacity_lt_count"); return; }
    l.Capacity = 100;
    if (l.Capacity < 100) { Console.WriteLine("ARC_CASE:list_capacity_grow:FAIL:set_capacity"); return; }
    if (l.Count != 3) { Console.WriteLine("ARC_CASE:list_capacity_grow:FAIL:count_after_resize"); return; }
    Console.WriteLine("ARC_CASE:list_capacity_grow:PASS");
}
"#,
        }],
    );

    let r = batch_case_result(&results, "list_capacity_grow");
    assert!(
        r.passed,
        "list_capacity_grow failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}

#[test]
fn std_barcode_code39_batch() {
    // EncodeCode39("A") 白盒布局：Code39Pattern = '*'(15 模块) + gap(1) + 'A'(15)
    // + gap(1) + '*'(15) = 47 模块；_Render（P2 批渲染）：moduleWidth=2、quietZone=10、
    // height=50 → width = (10+47+10)*2 = 134；模块 m 黑条占 x∈[(10+m)*2, (10+m)*2+2)。
    // 断言点（y=25）：quiet zone 白（x=2）、'*' 首元素窄 bar 黑（x=20）、
    // '*' 宽 space 内部白（x=24——若退化为等宽渲染此处为黑）、
    // 'A' 首元素宽 bar 黑（模块 16..18，x∈[52,58)）。
    let results = build_and_run_batch_with_deps(
        "std_barcode_code39",
        &[BatchCase {
            name: "barcode_code39_render",
            src: r#"using Arc;
using Arc.Drawing;

void Main() {
    Bitmap bm = BarcodeWriter.EncodeCode39("A");
    if (bm.Width != 134) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:width=" + bm.Width); return; }
    if (bm.Height != 50) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:height=" + bm.Height); return; }
    if ((int)bm.GetPixel(2, 25).R != 255) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:quiet_zone_white"); return; }
    if ((int)bm.GetPixel(20, 25).R != 0) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:first_bar_black"); return; }
    if ((int)bm.GetPixel(24, 25).R != 255) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:wide_space_white"); return; }
    if ((int)bm.GetPixel(52, 25).R != 0) { Console.WriteLine("ARC_CASE:barcode_code39_render:FAIL:A_leading_bar_black"); return; }
    Console.WriteLine("ARC_CASE:barcode_code39_render:PASS");
}
"#,
        }],
        &[("Arc.Drawing", "Drawing")],
    );

    let r = batch_case_result(&results, "barcode_code39_render");
    assert!(
        r.passed,
        "barcode_code39_render failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}

#[cfg(windows)]
#[test]
fn std_process_stream_batch() {
    // ProcessStream 管道契约：两条写路径（WriteString / Write）+ 读侧 EOF。
    //
    // p2-2 根因结论（决定性实验 target/arc-tests/std_process_diag/repro/reader.c，
    // 双向二分一次钉死）：WriteString/Write 写入管道的字节 = 源串 UTF-8 字节（零转码），
    // Read 读侧同样零转码（echo 回读 98,10,97,10 原样）。历史观测的 "？?"(3F 3F)
    // 出自 cmd 文本工具（more/sort）内部的输入编码启发式：偶数字节输入（如 "b\na\n"
    // 4 字节）的字节对呈「ASCII 低字节 + 0A 高字节」的 UTF-16LE 特征 → IsTextUnicode
    // 类误判 → 解码为 U+0A62/U+0A61 → GBK 无映射 → 3F 3F；奇数字节输入（9 字节）
    // 必判 ANSI 原样回显。故正向契约用 9 字节奇数输入，不依赖 cmd 转码行为。
    //
    // more 无控制台管道模式下会分包 flush（65..72 先到、10 后到），且可能补 \r，
    // 因此断言采用累积缓冲 + 子序列匹配，不假设单次 Read 到齐。
    let results = build_and_run_batch(
        "std_process_stream",
        &[
            BatchCase {
                name: "process_read_eof",
                src: r#"using Arc;
using Arc.Diagnostics;

void Main() {
    ProcessStartInfo psi = new ProcessStartInfo();
    psi.FileName = "cmd";
    psi.Arguments = "/c echo hello";
    psi.RedirectStandardOutput = true;
    psi.CreateNoWindow = true;
    Process p = Process.Start(psi);
    byte[] outBuf = new byte[64];
    int consumed = 0;
    int guard = 0;
    int n = p.StandardOutput.Read(outBuf, 0, 64);
    while (n > 0 && guard < 32) {
        consumed = consumed + n;
        guard = guard + 1;
        n = p.StandardOutput.Read(outBuf, 0, 64);
    }
    p.WaitForExit();
    p.Dispose();
    if (n != 0) { Console.WriteLine("ARC_CASE:process_read_eof:FAIL:no_eof"); return; }
    int posH = -1;
    int posO = -1;
    int i = 0;
    while (i < consumed) {
        if ((int)outBuf[i] == 104 && posH < 0) { posH = i; }
        if ((int)outBuf[i] == 111 && posO < 0) { posO = i; }
        i = i + 1;
    }
    if (posH < 0 || posO < 0 || posH >= posO) {
        Console.WriteLine("diag dump: consumed=" + consumed + " b0=" + (int)outBuf[0] + " b1=" + (int)outBuf[1] + " b2=" + (int)outBuf[2] + " b3=" + (int)outBuf[3] + " b4=" + (int)outBuf[4]);
        Console.WriteLine("ARC_CASE:process_read_eof:FAIL:read_missing");
        return;
    }
    Console.WriteLine("ARC_CASE:process_read_eof:PASS");
}
"#,
            },
            BatchCase {
                name: "process_write_string_pipe",
                src: r#"using Arc;
using Arc.Diagnostics;

void Main() {
    ProcessStartInfo psi = new ProcessStartInfo();
    psi.FileName = "cmd";
    psi.Arguments = "/c more";
    psi.RedirectStandardInput = true;
    psi.RedirectStandardOutput = true;
    psi.CreateNoWindow = true;
    Process p = Process.Start(psi);
    string s = "ABCDEFGH\n";
    p.StandardInput.WriteString(s);
    p.StandardInput.Dispose();
    byte[] outBuf = new byte[64];
    byte[] acc = new byte[256];
    int total = 0;
    int guard = 0;
    int n = p.StandardOutput.Read(outBuf, 0, 64);
    while (n > 0 && guard < 32) {
        int k = 0;
        while (k < n) { acc[total + k] = outBuf[k]; k = k + 1; }
        total = total + n;
        guard = guard + 1;
        n = p.StandardOutput.Read(outBuf, 0, 64);
    }
    p.WaitForExit();
    p.Dispose();
    if (n != 0) { Console.WriteLine("ARC_CASE:process_write_string_pipe:FAIL:no_eof"); return; }
    if (total < 9) { Console.WriteLine("ARC_CASE:process_write_string_pipe:FAIL:short total=" + total); return; }
    byte[] pat = new byte[9];
    pat[0] = (byte)65;
    pat[1] = (byte)66;
    pat[2] = (byte)67;
    pat[3] = (byte)68;
    pat[4] = (byte)69;
    pat[5] = (byte)70;
    pat[6] = (byte)71;
    pat[7] = (byte)72;
    pat[8] = (byte)10;
    int pi = 0;
    int i = 0;
    while (i < total) {
        if (pi < 9 && (int)acc[i] == (int)pat[pi]) { pi = pi + 1; }
        i = i + 1;
    }
    if (pi < 9) {
        int d = 0;
        string hex = "";
        while (d < total && d < 16) { hex = hex + (int)acc[d] + ","; d = d + 1; }
        Console.WriteLine("dump: total=" + total + " [" + hex + "]");
        Console.WriteLine("ARC_CASE:process_write_string_pipe:FAIL:sequence");
        return;
    }
    i = 0;
    while (i < total) {
        if ((int)acc[i] == 63) {
            Console.WriteLine("ARC_CASE:process_write_string_pipe:FAIL:question_mark");
            return;
        }
        i = i + 1;
    }
    Console.WriteLine("ARC_CASE:process_write_string_pipe:PASS");
}
"#,
            },
            BatchCase {
                name: "process_byte_write_pipe",
                src: r#"using Arc;
using Arc.Diagnostics;

void Main() {
    ProcessStartInfo psi = new ProcessStartInfo();
    psi.FileName = "cmd";
    psi.Arguments = "/c more";
    psi.RedirectStandardInput = true;
    psi.RedirectStandardOutput = true;
    psi.CreateNoWindow = true;
    Process p = Process.Start(psi);
    byte[] data = new byte[9];
    data[0] = (byte)65;
    data[1] = (byte)66;
    data[2] = (byte)67;
    data[3] = (byte)68;
    data[4] = (byte)69;
    data[5] = (byte)70;
    data[6] = (byte)71;
    data[7] = (byte)72;
    data[8] = (byte)10;
    p.StandardInput.Write(data, 0, 9);
    p.StandardInput.Dispose();
    byte[] outBuf = new byte[64];
    byte[] acc = new byte[256];
    int total = 0;
    int guard = 0;
    int n = p.StandardOutput.Read(outBuf, 0, 64);
    while (n > 0 && guard < 32) {
        int k = 0;
        while (k < n) { acc[total + k] = outBuf[k]; k = k + 1; }
        total = total + n;
        guard = guard + 1;
        n = p.StandardOutput.Read(outBuf, 0, 64);
    }
    p.WaitForExit();
    p.Dispose();
    if (n != 0) { Console.WriteLine("ARC_CASE:process_byte_write_pipe:FAIL:no_eof"); return; }
    if (total < 9) { Console.WriteLine("ARC_CASE:process_byte_write_pipe:FAIL:short total=" + total); return; }
    int pi = 0;
    int i = 0;
    while (i < total) {
        if (pi < 9 && (int)acc[i] == (int)data[pi]) { pi = pi + 1; }
        i = i + 1;
    }
    if (pi < 9) {
        int d = 0;
        string hex = "";
        while (d < total && d < 16) { hex = hex + (int)acc[d] + ","; d = d + 1; }
        Console.WriteLine("dump: total=" + total + " [" + hex + "]");
        Console.WriteLine("ARC_CASE:process_byte_write_pipe:FAIL:sequence");
        return;
    }
    i = 0;
    while (i < total) {
        if ((int)acc[i] == 63) {
            Console.WriteLine("ARC_CASE:process_byte_write_pipe:FAIL:question_mark");
            return;
        }
        i = i + 1;
    }
    Console.WriteLine("ARC_CASE:process_byte_write_pipe:PASS");
}
"#,
            },
            BatchCase {
                // std P3 契约补完：写失败抛 IOException（RFC 021）。子进程退出后
                // 其 stdin 读端关闭，父侧 WriteFile 报 broken pipe（C 侧 -1）→
                // Arc 侧必须抛异常而非静默吞掉。WaitForExit 保证管道已 teardown。
                name: "process_write_after_exit_throws",
                src: r#"using Arc;
using Arc.Diagnostics;
using Arc.IO;

void Main() {
    ProcessStartInfo psi = new ProcessStartInfo();
    psi.FileName = "cmd";
    psi.Arguments = "/c exit";
    psi.RedirectStandardInput = true;
    psi.CreateNoWindow = true;
    Process p = Process.Start(psi);
    p.WaitForExit();
    byte[] data = new byte[4];
    data[0] = (byte)65;
    data[1] = (byte)66;
    data[2] = (byte)67;
    data[3] = (byte)10;
    string err = "";
    try {
        p.StandardInput.Write(data, 0, 4);
        Console.WriteLine("ARC_CASE:process_write_after_exit_throws:FAIL:no_throw");
        return;
    } catch (IOException e) {
        err = e.Message;
    }
    p.Dispose();
    if (err == "") { Console.WriteLine("ARC_CASE:process_write_after_exit_throws:FAIL:empty_message"); return; }
    Console.WriteLine("ARC_CASE:process_write_after_exit_throws:PASS");
}
"#,
            },
        ],
    );

    for res in &results {
        eprintln!(
            "=== {} passed={:?} err={:?}\n{}",
            res.name, res.passed, res.error, res.stdout
        );
    }

    let r = batch_case_result(&results, "process_read_eof");
    assert!(
        r.passed,
        "process_read_eof failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "process_write_string_pipe");
    assert!(
        r.passed,
        "process_write_string_pipe failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "process_byte_write_pipe");
    assert!(
        r.passed,
        "process_byte_write_pipe failed: {:?} stdout: {}",
        r.error, r.stdout
    );

    let r = batch_case_result(&results, "process_write_after_exit_throws");
    assert!(
        r.passed,
        "process_write_after_exit_throws failed: {:?} stdout: {}",
        r.error, r.stdout
    );
}
