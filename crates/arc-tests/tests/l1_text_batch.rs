use arc_tests::assert_compiles_batch;

#[test]
fn compiles_text_batch() {
    assert_compiles_batch(
        "text",
        &[
            (
                "codec_base64_hex",
                r#"using Arc;

void Main() {
    string b64 = Base64.Encode("Hello");
    Console.WriteLine(b64);
    string b64d = Base64.Decode(b64);
    Console.WriteLine(b64d);
    string hex = Hex.Encode("Hi");
    Console.WriteLine(hex);
    string hexd = Hex.Decode(hex);
    Console.WriteLine(hexd);
}
"#,
            ),
            (
                "codec_url",
                r#"using Arc;

void Main() {
    string enc = Url.Encode("a b&c");
    Console.WriteLine(enc);
    string dec = Url.Decode("a+b%26c");
    Console.WriteLine(dec);
    string dec2 = Url.Decode("a%20b");
    Console.WriteLine(dec2);
    string empty = Url.Encode("");
    Console.WriteLine("[" + empty + "]");
}
"#,
            ),
            (
                "string_builder",
                r#"using Arc;
using Arc.Text;

void Main() {
    StringBuilder sb = new StringBuilder();
    sb.Append("Hello").Append(", ").Append("World");
    string s1 = sb.ToString();
    Console.WriteLine(s1);
    sb.AppendLine("!");
    sb.AppendLine();
    string s2 = sb.ToString();
    Console.WriteLine(s2);
}
"#,
            ),
            (
                "string_char_index",
                r#"using Arc;

void Main() {
    string s = "hi";
    Console.WriteLine("" + (int)s[0]);
    Console.WriteLine("" + (int)s[1]);
    Console.WriteLine("" + s.Length);
    Console.WriteLine("" + (int)s[2]);
}
"#,
            ),
            (
                "encoding_utf8",
                r#"using Arc;
using Arc.Text;

void Main() {
    byte[] b = Encoding.GetBytes("Hello");
    Console.WriteLine("" + b.Length);
    byte h = b[0];
    Console.WriteLine("" + (int)h);
    string s = Encoding.GetString(b);
    Console.WriteLine(s);

    byte[] empty = Encoding.GetBytes("");
    Console.WriteLine("" + empty.Length);
    string emptyS = Encoding.GetString(empty);
    Console.WriteLine("" + emptyS.Length);

    byte[] zh = Encoding.GetBytes("中");
    Console.WriteLine("" + zh.Length);
    string zhS = Encoding.GetString(zh);
    Console.WriteLine(zhS);

    Console.WriteLine("" + Encoding.GetByteCount("Hello"));
    Console.WriteLine("" + Encoding.GetByteCount("中"));
    Console.WriteLine("" + Encoding.GetByteCount(""));
}
"#,
            ),
            (
                "base64_bytes",
                r#"using Arc;
using Arc.Text;

void Main() {
    bool ok = true;
    byte[] b = Encoding.GetBytesLatin1("Hello");
    string s = Base64.ToBase64String(b);
    if (s != "SGVsbG8=") { ok = false; }

    byte[] rb = Base64.FromBase64String("SGVsbG8=");
    if (rb.Length != 5) { ok = false; }
    if ((int)rb[0] != 0x48) { ok = false; }
    if ((int)rb[1] != 0x65) { ok = false; }
    if ((int)rb[2] != 0x6c) { ok = false; }
    if ((int)rb[4] != 0x6f) { ok = false; }

    byte[] emb = Base64.FromBase64String("RGEA");
    if (emb.Length != 3) { ok = false; }
    if ((int)emb[0] != 0x44) { ok = false; }
    if ((int)emb[1] != 0x61) { ok = false; }
    if ((int)emb[2] != 0) { ok = false; }
    if (Base64.ToBase64String(emb) != "RGEA") { ok = false; }

    byte[] emptyArr = Base64.FromBase64String("");
    if (emptyArr.Length != 0) { ok = false; }
    if (Base64.ToBase64String(emptyArr) != "") { ok = false; }

    byte[] zeros = Hex.FromHexString("000000");
    if (Base64.ToBase64String(zeros) != "AAAA") { ok = false; }

    if (ok) { Console.WriteLine("base64_bytes_ok"); } else { Console.WriteLine("fail"); }
}
"#,
            ),
        ],
    );
}
