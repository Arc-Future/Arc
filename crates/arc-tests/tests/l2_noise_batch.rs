//! L2 批量：Noise XK 安全传输（Arc.Net.P2P）运行时冒烟集（RFC 042 P0-2）。
//!
//! 3 case，纯内存双端握手 + 传输往返（不经 TCP/网络腿）：
//! - `noise_xk_handshake_bytes`：固定 32B 测试向量（initSk=01..20、
//!   respSk=21..40）经 ECDiffieHellman.ImportPrivateKey 推导双端静态公钥 →
//!   SecureSession.Create 双端建会话 → Initiate/Respond/Finalize/
//!   RespondFinalize 三消息闭环：msg1/msg2 = 48B（e.pub 32 + tag 16）、
//!   msg3 = 64B（s.pub 32 + tag 16）；msg1 断言非零字节计数 ≥ 40（CSPRNG
//!   密钥材料，证明 byte[] 面无 NUL 截断——旧 string 假面按 NUL 终止读，
//!   随机材料首 NUL 概率 ~17%）；双端 HandshakeHash 32B 逐字节一致。
//! - `noise_transport_roundtrip`：Split 后传输面 Encrypt/Decrypt——
//!   「密文‖tag」合并出参（pt_len+16）逐字节切分（ct[0..ptLen) / tag
//!   [ptLen..ptLen+16)，Arc 无切片语法先例故用循环拷贝）；含 NUL 边界
//!   字节的 100B 载荷正向往返 + 37B 反向往返逐字节比对；tag 翻转 1 bit →
//!   Decrypt 返回 null（AEAD 认证失败）。
//! - `noise_facade_validation`：公开面参数校验（AesGcm 模式真实体）——
//!   Create 短 sk / 短 pk / 非法 initiator 抛 ArgumentException；Encrypt(null)
//!   抛 ArgumentNullException；Decrypt 收 8B tag 抛 ArgumentException。
//!
//! 批依赖：`("Arc.Net.P2P", "Net/P2P")` + `("Arc.Security", "Security")`
//! （包名取自各自 arc.toml；ECDiffieHellman 位于 std/Security/Cryptography/
//! 且按文件级 `namespace Arc.Security.Cryptography` 声明，无需独立包清单）。
//!
//! DLL 落位：由 codegen copy_crypto_native_dll_if_needed 门卫（is_windows_target）
//! 在链接后自动完成，无需测试侧预拷贝。

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
fn runs_noise_batch() {
    let results = assert_compiles_and_runs_batch_with_deps(
        "noise",
        &[
            (
                "noise_xk_handshake_bytes",
                r#"using Arc;
using Arc.Net.P2P;
using Arc.Security.Cryptography;

void Main() {
    byte[] initSk = new byte[32];
    byte[] respSk = new byte[32];
    for (int i = 0; i < 32; i++) {
        initSk[i] = (byte)(i + 1);
        respSk[i] = (byte)(i + 33);
    }
    byte[] initPk = ECDiffieHellman.ImportPrivateKey(initSk).PublicKey;
    byte[] respPk = ECDiffieHellman.ImportPrivateKey(respSk).PublicKey;
    if (initPk == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:init-pk-null"); return; }
    if (initPk.Length != 32) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:init-pk-len=" + initPk.Length); return; }
    if (respPk == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:resp-pk-null"); return; }
    if (respPk.Length != 32) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:resp-pk-len=" + respPk.Length); return; }

    SecureSession ini = SecureSession.Create(initSk, respPk, 1);
    if (ini == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:ini-null"); return; }
    SecureSession rsp = SecureSession.Create(respSk, initPk, 0);
    if (rsp == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:rsp-null"); return; }

    byte[] msg1 = NoiseTransport.Initiate(ini.Handle);
    if (msg1 == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg1-null"); return; }
    if (msg1.Length != 48) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg1-len=" + msg1.Length); return; }
    int nonzero = 0;
    for (int i = 0; i < msg1.Length; i++) {
        if (msg1[i] != 0) { nonzero = nonzero + 1; }
    }
    if (nonzero < 40) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg1-nonzero=" + nonzero); return; }
    Console.WriteLine("msg1-nonzero=" + nonzero);

    byte[] msg2 = NoiseTransport.Respond(rsp.Handle, msg1);
    if (msg2 == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg2-null"); return; }
    if (msg2.Length != 48) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg2-len=" + msg2.Length); return; }

    byte[] msg3 = NoiseTransport.Finalize(ini.Handle, msg2);
    if (msg3 == null) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg3-null"); return; }
    if (msg3.Length != 64) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:msg3-len=" + msg3.Length); return; }

    if (!NoiseTransport.RespondFinalize(rsp.Handle, msg3)) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:rsp-finalize"); return; }

    byte[] hashI = NoiseTransport.HandshakeHash(ini.Handle);
    byte[] hashR = NoiseTransport.HandshakeHash(rsp.Handle);
    if (hashI == null || hashI.Length != 32) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:hash-init"); return; }
    if (hashR == null || hashR.Length != 32) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:hash-resp"); return; }
    for (int i = 0; i < 32; i++) {
        if (hashI[i] != hashR[i]) { Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:FAIL:hash-mismatch@" + i); return; }
    }

    Console.WriteLine("ARC_CASE:noise_xk_handshake_bytes:PASS");
}
"#,
            ),
            (
                "noise_transport_roundtrip",
                r#"using Arc;
using Arc.Net.P2P;
using Arc.Security.Cryptography;

void Main() {
    byte[] initSk = new byte[32];
    byte[] respSk = new byte[32];
    for (int i = 0; i < 32; i++) {
        initSk[i] = (byte)(i + 1);
        respSk[i] = (byte)(i + 33);
    }
    byte[] initPk = ECDiffieHellman.ImportPrivateKey(initSk).PublicKey;
    byte[] respPk = ECDiffieHellman.ImportPrivateKey(respSk).PublicKey;
    if (initPk == null || respPk == null) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:pk-derive"); return; }

    SecureSession ini = SecureSession.Create(initSk, respPk, 1);
    SecureSession rsp = SecureSession.Create(respSk, initPk, 0);
    if (ini == null || rsp == null) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:create"); return; }

    byte[] msg1 = NoiseTransport.Initiate(ini.Handle);
    byte[] msg2 = NoiseTransport.Respond(rsp.Handle, msg1);
    byte[] msg3 = NoiseTransport.Finalize(ini.Handle, msg2);
    if (msg1 == null || msg1.Length != 48) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:msg1"); return; }
    if (msg2 == null || msg2.Length != 48) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:msg2"); return; }
    if (msg3 == null || msg3.Length != 64) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:msg3"); return; }
    if (!NoiseTransport.RespondFinalize(rsp.Handle, msg3)) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:rsp-finalize"); return; }

    int ptLen = 100;
    byte[] pt = new byte[ptLen];
    for (int i = 0; i < ptLen; i++) {
        pt[i] = (byte)((i * 31 + 7) % 256);
    }
    pt[0] = (byte)0;
    pt[ptLen - 1] = (byte)0;

    byte[] ct = ini.Encrypt(pt);
    if (ct == null) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:ct-null"); return; }
    if (ct.Length != ptLen + 16) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:ct-len=" + ct.Length); return; }

    byte[] ctOnly = new byte[ptLen];
    byte[] tag = new byte[16];
    for (int i = 0; i < ptLen; i++) { ctOnly[i] = ct[i]; }
    for (int i = 0; i < 16; i++) { tag[i] = ct[ptLen + i]; }

    byte[] back = rsp.Decrypt(ctOnly, tag);
    if (back == null) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:back-null"); return; }
    if (back.Length != ptLen) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:back-len=" + back.Length); return; }
    for (int i = 0; i < ptLen; i++) {
        if (back[i] != pt[i]) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:mismatch@" + i); return; }
    }

    int pt2Len = 37;
    byte[] pt2 = new byte[pt2Len];
    for (int i = 0; i < pt2Len; i++) {
        pt2[i] = (byte)(255 - i);
    }
    byte[] ct2 = rsp.Encrypt(pt2);
    if (ct2 == null || ct2.Length != pt2Len + 16) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:ct2"); return; }
    byte[] ct2Only = new byte[pt2Len];
    byte[] tag2 = new byte[16];
    for (int i = 0; i < pt2Len; i++) { ct2Only[i] = ct2[i]; }
    for (int i = 0; i < 16; i++) { tag2[i] = ct2[pt2Len + i]; }
    byte[] back2 = ini.Decrypt(ct2Only, tag2);
    if (back2 == null || back2.Length != pt2Len) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:back2"); return; }
    for (int i = 0; i < pt2Len; i++) {
        if (back2[i] != pt2[i]) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:mismatch2@" + i); return; }
    }

    tag[0] = (byte)(tag[0] ^ 1);
    byte[] tampered = rsp.Decrypt(ctOnly, tag);
    if (tampered != null) { Console.WriteLine("ARC_CASE:noise_transport_roundtrip:FAIL:tamper-accepted"); return; }

    Console.WriteLine("ARC_CASE:noise_transport_roundtrip:PASS");
}
"#,
            ),
            (
                "noise_facade_validation",
                r#"using Arc;
using Arc.Net.P2P;
using Arc.Security.Cryptography;

void Main() {
    byte[] goodSk = new byte[32];
    byte[] goodPk = new byte[32];
    for (int i = 0; i < 32; i++) {
        goodSk[i] = (byte)(i + 1);
        goodPk[i] = (byte)(i + 33);
    }

    int shortLen = 16;
    byte[] shortSk = new byte[shortLen];
    bool threwShort = false;
    try {
        SecureSession.Create(shortSk, goodPk, 1);
    } catch (ArgumentException e) {
        Console.WriteLine("expected-short-sk:" + e.Message);
        threwShort = true;
    }
    if (!threwShort) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:create-short-sk"); return; }

    int shortPkLen = 31;
    byte[] shortPk = new byte[shortPkLen];
    bool threwPk = false;
    try {
        SecureSession.Create(goodSk, shortPk, 1);
    } catch (ArgumentException e) {
        Console.WriteLine("expected-short-pk:" + e.Message);
        threwPk = true;
    }
    if (!threwPk) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:create-short-pk"); return; }

    int badInitiator = 7;
    bool threwInitiator = false;
    try {
        SecureSession.Create(goodSk, goodPk, badInitiator);
    } catch (ArgumentException e) {
        Console.WriteLine("expected-initiator:" + e.Message);
        threwInitiator = true;
    }
    if (!threwInitiator) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:create-initiator"); return; }

    SecureSession s = SecureSession.Create(goodSk, goodPk, 1);
    if (s == null) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:session-null"); return; }
    if (s.Handle == null) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:handle-null"); return; }

    bool threwNullPt = false;
    try {
        s.Encrypt(null);
    } catch (ArgumentNullException e) {
        Console.WriteLine("expected-null-pt:" + e.Message);
        threwNullPt = true;
    }
    if (!threwNullPt) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:encrypt-null"); return; }

    int tagLen = 8;
    byte[] badTag = new byte[tagLen];
    bool threwTag = false;
    int ctLen = 32;
    byte[] dummyCt = new byte[ctLen];
    try {
        s.Decrypt(dummyCt, badTag);
    } catch (ArgumentException e) {
        Console.WriteLine("expected-tag:" + e.Message);
        threwTag = true;
    }
    if (!threwTag) { Console.WriteLine("ARC_CASE:noise_facade_validation:FAIL:decrypt-tag-len"); return; }

    Console.WriteLine("ARC_CASE:noise_facade_validation:PASS");
}
"#,
            ),
        ],
        &[("Arc.Net.P2P", "Net/P2P"), ("Arc.Security", "Security")],
    );
    assert_all_passed("noise", &results);
}

#[cfg(not(feature = "full-rt"))]
#[test]
fn runs_noise_batch() {
    // L2 runtime tests require --features full-rt
}
