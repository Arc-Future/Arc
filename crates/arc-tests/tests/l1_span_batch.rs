//! L1 compile-only batch: Span / ReadOnlySpan surface.
//! Extracted from arc-integration e2e tests (span_e2e).

use arc_tests::assert_compiles_batch;

#[test]
fn compiles_l1_span_batch() {
    assert_compiles_batch(
        "l1_span_batch",
        &[
            // span: AsSpan / Span conversions / CopyTo / TryCopyTo / ToArray / foreach
            (
                "span_aspan_conversions",
                r#"using Arc;
class SpanHelpers {
    private void Fill(Span<int> s, int v) {
        for (int i = 0; i < s.Length; i++) {
            s[i] = v;
        }
    }

    private int Sum(ReadOnlySpan<int> s) {
        int total = 0;
        for (int i = 0; i < s.Length; i++) {
            total = total + s[i];
        }
        return total;
    }

    private int SumBytes(ReadOnlySpan<byte> s) {
        int total = 0;
        for (int i = 0; i < s.Length; i++) {
            total = total + s[i];
        }
        return total;
    }

    public void Run() {
        int[] buf = [1, 2, 3, 4];
        Span<int> mid = buf.AsSpan(1, 2);
        int len1 = mid.Length;
        this.Fill(mid, 9);
        int v0 = buf[0];
        int v1 = buf[1];

        int[] buf2 = [10, 20, 30];
        Span<int> s = buf2.AsSpan();
        ReadOnlySpan<int> r = s;
        int sum1 = this.Sum(r);

        int[] buf3 = [5, 6];
        ReadOnlySpan<int> r2 = buf3.AsReadOnlySpan();
        int len2 = r2.Length;

        List<int> list = new List<int>();
        list.Add(1); list.Add(2); list.Add(3); list.Add(4);
        Span<int> mid2 = list.AsSpan(1, 2);
        int len3 = mid2.Length;
        this.Fill(mid2, 9);

        string str = "AB";
        ReadOnlySpan<byte> bytes = str.AsSpan();
        int len4 = bytes.Length;
        int b0 = bytes[0];
        int b1 = bytes[1];

        Span<int> s1 = [1, 2, 3];
        int len5 = s1.Length;
        this.Fill(s1, 7);
        ReadOnlySpan<int> r3 = s1;
        int sum2 = this.Sum(r3);

        ReadOnlySpan<int> r4 = [4, 5];
        int sum3 = this.Sum(r4);

        Span<int> empty = [];
        int len6 = empty.Length;
        bool isEmpty = empty.IsEmpty;
        ReadOnlySpan<int> rempty = [];
        int len7 = rempty.Length;

        int[] buf4 = [1, 2, 3, 4];
        Span<int> mid3 = buf4.AsSpan().Slice(1, 2);
        int len8 = mid3.Length;
        int v3 = mid3[0];
        this.Fill(mid3, 8);
        ReadOnlySpan<int> ros = buf4.AsSpan().AsReadOnly();
        int len9 = ros.Length;

        Span<int> e = Span<int>.Empty;
        bool eEmpty = e.IsEmpty;
        int eLen = e.Length;
        ReadOnlySpan<int> re = ReadOnlySpan<int>.Empty;
        bool reEmpty = re.IsEmpty;
        int[] buf5 = [9];
        bool notEmpty = buf5.AsSpan().IsEmpty;

        int[] srcBuf = [1, 2, 3];
        int[] dstBuf = [0, 0, 0];
        srcBuf.AsReadOnlySpan().CopyTo(dstBuf.AsSpan());
        int d0 = dstBuf[0];
        int d1 = dstBuf[1];
        int d2 = dstBuf[2];

        int[] srcBuf2 = [1, 2, 3];
        int[] shortBuf = [0, 0];
        bool ok1 = srcBuf2.AsReadOnlySpan().TryCopyTo(shortBuf.AsSpan());

        int[] srcBuf3 = [7, 8];
        int[] dstBuf3 = [0, 0, 0];
        Span<int> dst = dstBuf3.AsSpan(0, 2);
        bool ok2 = srcBuf3.AsSpan().TryCopyTo(dst);

        int[] buf6 = [1, 2, 3, 4];
        Span<int> mid4 = buf6.AsSpan(1, 2);
        int[] copy = mid4.ToArray();
        int copyLen = copy.Length;
        mid4[0] = 99;
        int orig = buf6[1];
        ReadOnlySpan<int> ros2 = buf6.AsReadOnlySpan();
        int[] all = ros2.ToArray();
        int allLen = all.Length;

        int[] buf7 = [1, 2, 3, 4];
        Span<int> mid5 = buf7.AsSpan(1, 2);
        int total = 0;
        foreach (var x in mid5) {
            total = total + x;
        }
        mid5[0] = 10;
        int total2 = 0;
        foreach (var x in mid5) {
            total2 = total2 + x;
        }

        ReadOnlySpan<int> r5 = [7, 8, 9];
        int total3 = 0;
        foreach (var x in r5) {
            total3 = total3 + x;
        }

        Span<int> sEmpty = [];
        int n = 0;
        foreach (var x in sEmpty) { n = n + 1; }
        ReadOnlySpan<int> rEmpty = ReadOnlySpan<int>.Empty;
        foreach (var x in rEmpty) { n = n + 1; }

        Console.WriteLine("ok");
    }
}

void Main() {
    var h = new SpanHelpers();
    h.Run();
}
"#,
            ),
            // span: collection expr target StackSpan (void Main pattern)
            (
                "span_collection_expr_stack",
                r#"using Arc;
void Main() {
    Span<int> s = [1, 2, 3];
    int len = s.Length;
    int v0 = s[0];
    int v1 = s[1];
    int v2 = s[2];
    Console.WriteLine("ok");
}
"#,
            ),
            // span: Slice(_startOnly) / Fill / Clear
            (
                "span_slice_fill_clear",
                r#"using Arc;
class SliceHelpers {
    public void Run() {
        int[] buf = [10, 20, 30, 40];
        Span<int> tail = buf.AsSpan().Slice(2);
        int tailLen = tail.Length;
        int t0 = tail[0];
        int t1 = tail[1];

        int[] buf2 = [1, 2, 3];
        Span<int> s = buf2.AsSpan();
        s.Fill(7);
        int sf0 = s[0]; int sf1 = s[1]; int sf2 = s[2];
        s.Clear();
        int sc0 = s[0]; int sc1 = s[1]; int sc2 = s[2];
        int b0 = buf2[0]; int b1 = buf2[1]; int b2 = buf2[2];
        Console.WriteLine("ok");
    }
}

void Main() {
    var h = new SliceHelpers();
    h.Run();
}
"#,
            ),
        ],
    );
}
