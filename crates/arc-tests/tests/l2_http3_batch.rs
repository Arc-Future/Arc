//! L2 批量：HTTP/3 QPACK（RFC 9204）协议向量测试（Arc.Net internal 面）。
//!
//! 覆盖 Qpack/QpackStaticTable 修复的 golden 验证，向量逐字节抄自 RFC 9204
//! 原文（Appendix B 编解码示例 + §A.1 静态表 + §4.5 前缀矩阵），编码侧期望
//! 与解码侧输入共用同一向量——非自编自解回环：
//! - `qpack_golden_b1_encode_decode`：B.1 字面量 + 静态名引用（0x51 = 0b0101_0001，
//!   4 位前缀 flags 0x50 起）双向；field section 前缀 `00 00`（RIC=0 / Delta Base=0）。
//! - `qpack_static_indexed_anchors`：静态 indexed 锚点（:authority → 0xC0、
//!   :path=/ → 0xC1）双向 + §A.1 排序锚点（index 0/1/17/25）+ 哨兵 -1（未命中）。
//! - `qpack_literal_name_roundtrip`：§4.5.6 字面量名字（flags 0x20 起、3 位前缀）回环。
//! - `qpack_dynamic_base_roundtrip`：动态表插入 + 前基引用（T=0 flags 128 起，
//!   rel = Base - 1 - absolute）编码/解码回环 + 3 位前缀整数扩展路径（名字长 10）。
//! - `qpack_golden_b2_postbase_decode`：B.2 post-Base indexed（0b0001 = 16 起，
//!   absolute = Base + rel）解码，含 RIC 重建（encRic=3, tni=2 → 2）与 S=1 Delta。
//! - `qpack_golden_b4_mixed_decode`：B.4 动态前基（0x80/0x81）+ 静态 indexed（0xc1）
//!   混合解码，含 RIC 重建（encRic=5, tni=4 → 4）与 S=0 Delta Base。
//!
//! internal 可见性：Qpack/QpackStaticTable 为 Arc.Net internal——经
//! std/Net/Core/arc.toml `internals_visible_to = ["http3-qpack"]` 放行
//! （RFC 025 M2+，对标 C# InternalsVisibleTo；批生成包名 = 批名下划线转连字符，
//! 见 arc-tests lib.rs assert_compiles_and_runs_batch_with_deps 的 arc.toml 拼装）。
//!
//! 批依赖：`("Arc.Net", "Net/Core")`——包名取自 std/Net/Core/arc.toml。
//! Arc.Security 传递依赖使产物隐式导入 vendored crypto_native.dll——照抄
//! l2_net_batch 的 best-effort DLL 兜底（Windows 0xC0000135 规避）。

#[cfg(feature = "full-rt")]
use arc_tests::assert_compiles_and_runs_batch_with_deps;

#[cfg(feature = "full-rt")]
fn assert_all_passed(batch: &str, results: &[arc_tests::BatchRunResult]) {
    for r in results {
        assert!(
            r.passed,
            "{batch}: case {} failed: {:?}\nstdout:\n{}",
            r.name, r.error, r.stdout
        );
    }
}

#[cfg(feature = "full-rt")]
#[test]
fn runs_http3_qpack_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "http3_qpack",
        &[
            // RFC 9204 B.1：[:path=/index.html] → 0000 510b 2f69 6e64 6578 2e68 746d 6c
            // （前缀 RIC=0/Delta=0；0x51 = 字面量 + 静态名引用 index 1；值长 11）。
            (
                "qpack_golden_b1_encode_decode",
                r#"using Arc;
using Arc.Net;

void Main() {
    List<byte> want = new List<byte>();
    want.Add((byte)0x00);
    want.Add((byte)0x00);
    want.Add((byte)0x51);
    want.Add((byte)0x0b);
    want.Add((byte)0x2f);
    want.Add((byte)0x69);
    want.Add((byte)0x6e);
    want.Add((byte)0x64);
    want.Add((byte)0x65);
    want.Add((byte)0x78);
    want.Add((byte)0x2e);
    want.Add((byte)0x68);
    want.Add((byte)0x74);
    want.Add((byte)0x6d);
    want.Add((byte)0x6c);
    byte[] bytes = want.ToArray();

    Qpack q = new Qpack();
    Http3HeaderList h = new Http3HeaderList();
    h.Add(":path", "/index.html");
    byte[] got = q.EncodeHeaders(h);
    if (got.Length != bytes.Length) {
        Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:FAIL:len=" + got.Length + " want=" + bytes.Length);
        return;
    }
    int i = 0;
    while (i < bytes.Length) {
        if ((int)got[i] != (int)bytes[i]) {
            Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:FAIL:at=" + i + " got=" + (int)got[i] + " want=" + (int)bytes[i]);
            return;
        }
        i = i + 1;
    }

    Qpack dq = new Qpack();
    Http3HeaderList out2 = new Http3HeaderList();
    if (!dq.DecodeHeaders(bytes, out2)) { Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:FAIL:decode-false"); return; }
    if (out2.Count != 1) { Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:FAIL:count=" + out2.Count); return; }
    if (out2.GetName(0) != ":path" || out2.GetValue(0) != "/index.html") {
        Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:FAIL:h0=" + out2.GetName(0) + "=" + out2.GetValue(0));
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_golden_b1_encode_decode:PASS");
}
"#,
            ),
            // §A.1 排序锚点（index 0 :authority / 1 :path=/ / 17 :method GET /
            // 25 :status 200）+ 未命中哨兵 -1 + 静态 indexed 双向（0xC0/0xC1）。
            (
                "qpack_static_indexed_anchors",
                r#"using Arc;
using Arc.Net;

void Main() {
    if (QpackStaticTable.FindName(":authority") != 0) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:findname-authority=" + QpackStaticTable.FindName(":authority"));
        return;
    }
    if (QpackStaticTable.Find(":path", "/") != 1) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:find-path=" + QpackStaticTable.Find(":path", "/"));
        return;
    }
    if (QpackStaticTable.Find(":method", "GET") != 17) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:find-get=" + QpackStaticTable.Find(":method", "GET"));
        return;
    }
    if (QpackStaticTable.Find(":status", "200") != 25) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:find-200=" + QpackStaticTable.Find(":status", "200"));
        return;
    }
    if (QpackStaticTable.Find(":authority", "www.example.com") != -1) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:sentinel-not-neg1");
        return;
    }

    Qpack q = new Qpack();
    Http3HeaderList ha = new Http3HeaderList();
    ha.Add(":authority", "");
    byte[] gota = q.EncodeHeaders(ha);
    if (gota.Length != 3 || (int)gota[2] != 0xc0 || (int)gota[0] != 0 || (int)gota[1] != 0) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:enc-authority len=" + gota.Length);
        return;
    }
    Http3HeaderList hp = new Http3HeaderList();
    hp.Add(":path", "/");
    byte[] gotp = q.EncodeHeaders(hp);
    if (gotp.Length != 3 || (int)gotp[2] != 0xc1 || (int)gotp[0] != 0 || (int)gotp[1] != 0) {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:enc-path len=" + gotp.Length);
        return;
    }

    Qpack da = new Qpack();
    Http3HeaderList outa = new Http3HeaderList();
    if (!da.DecodeHeaders(gota, outa)) { Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:dec-authority-false"); return; }
    if (outa.Count != 1 || outa.GetName(0) != ":authority" || outa.GetValue(0) != "") {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:dec-authority count=" + outa.Count);
        return;
    }
    Qpack dp = new Qpack();
    Http3HeaderList outp = new Http3HeaderList();
    if (!dp.DecodeHeaders(gotp, outp)) { Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:dec-path-false"); return; }
    if (outp.Count != 1 || outp.GetName(0) != ":path" || outp.GetValue(0) != "/") {
        Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:FAIL:dec-path count=" + outp.Count);
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_static_indexed_anchors:PASS");
}
"#,
            ),
            // §4.5.6 字面量名字：名字 "x-ab"（不在静态表）→ flags 0x20 + 3 位前缀
            // 长度 4 → 0x24；回环验证编解码对称。
            (
                "qpack_literal_name_roundtrip",
                r#"using Arc;
using Arc.Net;

void Main() {
    List<byte> want = new List<byte>();
    want.Add((byte)0x00);
    want.Add((byte)0x00);
    want.Add((byte)0x24);
    want.Add((byte)0x78);
    want.Add((byte)0x2d);
    want.Add((byte)0x61);
    want.Add((byte)0x62);
    want.Add((byte)0x02);
    want.Add((byte)0x63);
    want.Add((byte)0x64);
    byte[] bytes = want.ToArray();

    Qpack q = new Qpack();
    Http3HeaderList h = new Http3HeaderList();
    h.Add("x-ab", "cd");
    byte[] got = q.EncodeHeaders(h);
    if (got.Length != bytes.Length) {
        Console.WriteLine("ARC_CASE:qpack_literal_name_roundtrip:FAIL:len=" + got.Length + " want=" + bytes.Length);
        return;
    }
    int i = 0;
    while (i < bytes.Length) {
        if ((int)got[i] != (int)bytes[i]) {
            Console.WriteLine("ARC_CASE:qpack_literal_name_roundtrip:FAIL:at=" + i + " got=" + (int)got[i] + " want=" + (int)bytes[i]);
            return;
        }
        i = i + 1;
    }

    Qpack dq = new Qpack();
    Http3HeaderList out2 = new Http3HeaderList();
    if (!dq.DecodeHeaders(bytes, out2)) { Console.WriteLine("ARC_CASE:qpack_literal_name_roundtrip:FAIL:decode-false"); return; }
    if (out2.Count != 1 || out2.GetName(0) != "x-ab" || out2.GetValue(0) != "cd") {
        Console.WriteLine("ARC_CASE:qpack_literal_name_roundtrip:FAIL:rt count=" + out2.Count);
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_literal_name_roundtrip:PASS");
}
"#,
            ),
            // 动态表前基引用：Insert 后编码 [custom-key=custom-value] → 0200 80
            // （RIC 编码 2、Delta 0、T=0 rel 0）；再编码未命中值走 §4.5.6 且名字
            // 长 10 触发 3 位前缀扩展（0x27 03）。
            (
                "qpack_dynamic_base_roundtrip",
                r#"using Arc;
using Arc.Net;

void Main() {
    Qpack q = new Qpack();
    q.Insert("custom-key", "custom-value");

    List<byte> want = new List<byte>();
    want.Add((byte)0x02);
    want.Add((byte)0x00);
    want.Add((byte)0x80);
    byte[] wantBytes = want.ToArray();
    Http3HeaderList h = new Http3HeaderList();
    h.Add("custom-key", "custom-value");
    byte[] got = q.EncodeHeaders(h);
    if (got.Length != wantBytes.Length) {
        Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:len=" + got.Length + " want=" + wantBytes.Length);
        return;
    }
    int i = 0;
    while (i < wantBytes.Length) {
        if ((int)got[i] != (int)wantBytes[i]) {
            Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:at=" + i + " got=" + (int)got[i] + " want=" + (int)wantBytes[i]);
            return;
        }
        i = i + 1;
    }
    Http3HeaderList out2 = new Http3HeaderList();
    if (!q.DecodeHeaders(wantBytes, out2)) { Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:decode-false"); return; }
    if (out2.Count != 1 || out2.GetName(0) != "custom-key" || out2.GetValue(0) != "custom-value") {
        Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:rt count=" + out2.Count);
        return;
    }

    Http3HeaderList h2 = new Http3HeaderList();
    h2.Add("custom-key", "custom-value2");
    byte[] got2 = q.EncodeHeaders(h2);
    List<byte> want2 = new List<byte>();
    want2.Add((byte)0x00);
    want2.Add((byte)0x00);
    want2.Add((byte)0x27);
    want2.Add((byte)0x03);
    want2.Add((byte)0x63);
    want2.Add((byte)0x75);
    want2.Add((byte)0x73);
    want2.Add((byte)0x74);
    want2.Add((byte)0x6f);
    want2.Add((byte)0x6d);
    want2.Add((byte)0x2d);
    want2.Add((byte)0x6b);
    want2.Add((byte)0x65);
    want2.Add((byte)0x79);
    want2.Add((byte)0x0d);
    want2.Add((byte)0x63);
    want2.Add((byte)0x75);
    want2.Add((byte)0x73);
    want2.Add((byte)0x74);
    want2.Add((byte)0x6f);
    want2.Add((byte)0x6d);
    want2.Add((byte)0x2d);
    want2.Add((byte)0x76);
    want2.Add((byte)0x61);
    want2.Add((byte)0x6c);
    want2.Add((byte)0x75);
    want2.Add((byte)0x65);
    want2.Add((byte)0x32);
    byte[] want2Bytes = want2.ToArray();
    if (got2.Length != want2Bytes.Length) {
        Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:len2=" + got2.Length + " want=" + want2Bytes.Length);
        return;
    }
    int j = 0;
    while (j < want2Bytes.Length) {
        if ((int)got2[j] != (int)want2Bytes[j]) {
            Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:at2=" + j + " got=" + (int)got2[j] + " want=" + (int)want2Bytes[j]);
            return;
        }
        j = j + 1;
    }
    Http3HeaderList out3 = new Http3HeaderList();
    if (!q.DecodeHeaders(want2Bytes, out3)) { Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:decode2-false"); return; }
    if (out3.Count != 1 || out3.GetName(0) != "custom-key" || out3.GetValue(0) != "custom-value2") {
        Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:FAIL:rt2 count=" + out3.Count);
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_dynamic_base_roundtrip:PASS");
}
"#,
            ),
            // RFC 9204 B.2：动态表已插 2 条（tni=2），字段段 0381 10 11 →
            // RIC 重建 = 2、S=1 Delta=1 → Base=0、post-Base indexed rel 0/1
            // → absolute 0/1（:authority / :path）。
            (
                "qpack_golden_b2_postbase_decode",
                r#"using Arc;
using Arc.Net;

void Main() {
    Qpack q = new Qpack();
    q.Insert(":authority", "www.example.com");
    q.Insert(":path", "/sample/path");

    List<byte> bytes = new List<byte>();
    bytes.Add((byte)0x03);
    bytes.Add((byte)0x81);
    bytes.Add((byte)0x10);
    bytes.Add((byte)0x11);
    byte[] data = bytes.ToArray();

    Http3HeaderList out2 = new Http3HeaderList();
    if (!q.DecodeHeaders(data, out2)) { Console.WriteLine("ARC_CASE:qpack_golden_b2_postbase_decode:FAIL:decode-false"); return; }
    if (out2.Count != 2) { Console.WriteLine("ARC_CASE:qpack_golden_b2_postbase_decode:FAIL:count=" + out2.Count); return; }
    if (out2.GetName(0) != ":authority" || out2.GetValue(0) != "www.example.com") {
        Console.WriteLine("ARC_CASE:qpack_golden_b2_postbase_decode:FAIL:h0=" + out2.GetName(0) + "=" + out2.GetValue(0));
        return;
    }
    if (out2.GetName(1) != ":path" || out2.GetValue(1) != "/sample/path") {
        Console.WriteLine("ARC_CASE:qpack_golden_b2_postbase_decode:FAIL:h1=" + out2.GetName(1) + "=" + out2.GetValue(1));
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_golden_b2_postbase_decode:PASS");
}
"#,
            ),
            // RFC 9204 B.4：动态表 4 条（tni=4），字段段 0500 80 c1 81 →
            // RIC 重建 = 4、S=0 Delta=0 → Base=4；0x80 动态前基 rel 0（absolute 3）、
            // 0xc1 静态 index 1（:path=/）、0x81 动态前基 rel 1（absolute 2）。
            (
                "qpack_golden_b4_mixed_decode",
                r#"using Arc;
using Arc.Net;

void Main() {
    Qpack q = new Qpack();
    q.Insert(":authority", "www.example.com");
    q.Insert(":path", "/sample/path");
    q.Insert("custom-key", "custom-value");
    q.Insert(":authority", "www.example.com");

    List<byte> bytes = new List<byte>();
    bytes.Add((byte)0x05);
    bytes.Add((byte)0x00);
    bytes.Add((byte)0x80);
    bytes.Add((byte)0xc1);
    bytes.Add((byte)0x81);
    byte[] data = bytes.ToArray();

    Http3HeaderList out2 = new Http3HeaderList();
    if (!q.DecodeHeaders(data, out2)) { Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:FAIL:decode-false"); return; }
    if (out2.Count != 3) { Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:FAIL:count=" + out2.Count); return; }
    if (out2.GetName(0) != ":authority" || out2.GetValue(0) != "www.example.com") {
        Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:FAIL:h0=" + out2.GetName(0) + "=" + out2.GetValue(0));
        return;
    }
    if (out2.GetName(1) != ":path" || out2.GetValue(1) != "/") {
        Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:FAIL:h1=" + out2.GetName(1) + "=" + out2.GetValue(1));
        return;
    }
    if (out2.GetName(2) != "custom-key" || out2.GetValue(2) != "custom-value") {
        Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:FAIL:h2=" + out2.GetName(2) + "=" + out2.GetValue(2));
        return;
    }
    Console.WriteLine("ARC_CASE:qpack_golden_b4_mixed_decode:PASS");
}
"#,
            ),
        ],
        &[("Arc.Net", "Net/Core")],
    );
    assert_all_passed("http3_qpack", &results);
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_http3_qpack_batch() {
    // L2 运行时批仅在 --features full-rt 下执行（与 l2_net_batch 同门控约定）。
}
