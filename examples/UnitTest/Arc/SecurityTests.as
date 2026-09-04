namespace UnitTest.Arc;

using Arc;
using Arc.Security;
using Arc.Text;
using Arc.QIF;

/// <summary>
/// Security 加密单元测试：6 个密码学外观可证伪路径。
/// Hash：NIST 已知向量（byte[] 摘要经 ToHex 比对）；HMAC-SHA256：RFC 4231 / 常用 ASCII 向量；
/// CSPRNG：长度 + 两次抽取不相等（概率性；非统计合格性证明）。
/// RFC 026 M3 P0-1：ComputeHash/GetBytes 均返回 byte[]（失败抛 CryptographicException），
/// ToHex 助手转 lowercase hex 比对；CSPRNG 断言按原始字节长度计。
/// 禁止冒充云 KMS / 完整 PKI / AES 对称加密已落地。
/// </summary>
public class SecurityTests
{
    // ── MD5 ──

    [Fact]
    public void MD5_TestVector_Empty()
    {
        string hash = MD5.ToHex(MD5.ComputeHash(Encoding.GetBytes("")));
        Assert.True(hash == "d41d8cd98f00b204e9800998ecf8427e");
    }

    [Fact]
    public void MD5_TestVector_ABC()
    {
        string hash = MD5.ToHex(MD5.ComputeHash(Encoding.GetBytes("abc")));
        Assert.True(hash == "900150983cd24fb0d6963f7d28e17f72");
    }

    // ── SHA1 ──

    [Fact]
    public void SHA1_TestVector_ABC()
    {
        string hash = SHA1.ToHex(SHA1.ComputeHash(Encoding.GetBytes("abc")));
        Assert.True(hash == "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    // ── SHA256 ──

    [Fact]
    public void SHA256_TestVector_Empty()
    {
        string hash = SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes("")));
        Assert.True(hash == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    [Fact]
    public void SHA256_TestVector_ABC()
    {
        string hash = SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes("abc")));
        Assert.True(hash == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    [Fact]
    public void SHA256_ConsistentHashing()
    {
        string h1 = SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes("hello")));
        string h2 = SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes("hello")));
        Assert.True(h1 == h2);
    }

    // ── SHA512 ──

    [Fact]
    public void SHA512_TestVector_ABC()
    {
        string hash = SHA512.ToHex(SHA512.ComputeHash(Encoding.GetBytes("abc")));
        Assert.True(hash == "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
    }

    // ── HMAC-SHA256（RFC 4231 / 常用 ASCII 向量；hex lowercase）──

    [Fact]
    public void HMACSHA256_TestVector_KeyFox()
    {
        // key="key", msg=fox 句；与 RFC 025 §8.3 dogfood 一致
        string mac = HMACSHA256.ToHex(HMACSHA256.ComputeHash(
            Encoding.GetBytes("key"),
            Encoding.GetBytes("The quick brown fox jumps over the lazy dog")));
        Assert.Equal("f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8", mac);
    }

    [Fact]
    public void HMACSHA256_TestVector_RFC4231_Case2()
    {
        // RFC 4231 Test Case 2（ASCII key/data）
        string mac = HMACSHA256.ToHex(HMACSHA256.ComputeHash(
            Encoding.GetBytes("Jefe"),
            Encoding.GetBytes("what do ya want for nothing?")));
        Assert.Equal("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843", mac);
    }

    [Fact]
    public void HMACSHA256_Consistent()
    {
        string a = HMACSHA256.ToHex(HMACSHA256.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        string b = HMACSHA256.ToHex(HMACSHA256.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        Assert.Equal(a, b);
        Assert.Equal(64, a.Length);
    }

    // ── SHA384 (FIPS 180-4 NIST vectors) ──

    [Fact]
    public void SHA384_TestVector_Empty()
    {
        string hash = SHA384.ToHex(SHA384.ComputeHash(Encoding.GetBytes("")));
        Assert.Equal("38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b", hash);
    }

    [Fact]
    public void SHA384_ConsistentHashing()
    {
        string h1 = SHA384.ToHex(SHA384.ComputeHash(Encoding.GetBytes("hello")));
        string h2 = SHA384.ToHex(SHA384.ComputeHash(Encoding.GetBytes("hello")));
        Assert.Equal(h1, h2);
        Assert.Equal(96, h1.Length);
    }

    // ── SHA3-256 (FIPS 202 NIST vectors) ──

    [Fact]
    public void SHA3_256_TestVector_Empty()
    {
        string hash = SHA3_256.ToHex(SHA3_256.ComputeHash(Encoding.GetBytes("")));
        Assert.Equal("a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a", hash);
    }

    [Fact]
    public void SHA3_256_ConsistentHashing()
    {
        string h1 = SHA3_256.ToHex(SHA3_256.ComputeHash(Encoding.GetBytes("hello")));
        string h2 = SHA3_256.ToHex(SHA3_256.ComputeHash(Encoding.GetBytes("hello")));
        Assert.Equal(h1, h2);
        Assert.Equal(64, h1.Length);
    }

    // ── SHA3-512 (FIPS 202 NIST vectors) ──

    [Fact]
    public void SHA3_512_TestVector_Empty()
    {
        string hash = SHA3_512.ToHex(SHA3_512.ComputeHash(Encoding.GetBytes("")));
        Assert.Equal("a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26", hash);
    }

    [Fact]
    public void SHA3_512_ConsistentHashing()
    {
        string h1 = SHA3_512.ToHex(SHA3_512.ComputeHash(Encoding.GetBytes("hello")));
        string h2 = SHA3_512.ToHex(SHA3_512.ComputeHash(Encoding.GetBytes("hello")));
        Assert.Equal(h1, h2);
        Assert.Equal(128, h1.Length);
    }

    // ── HMAC-SHA384 (RFC 4231 vectors) ──

    [Fact]
    public void HMACSHA384_TestVector_RFC4231_Case2()
    {
        string mac = HMACSHA384.ToHex(HMACSHA384.ComputeHash(
            Encoding.GetBytes("Jefe"),
            Encoding.GetBytes("what do ya want for nothing?")));
        Assert.Equal("af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649", mac);
    }

    [Fact]
    public void HMACSHA384_Consistent()
    {
        string a = HMACSHA384.ToHex(HMACSHA384.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        string b = HMACSHA384.ToHex(HMACSHA384.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        Assert.Equal(a, b);
        Assert.Equal(96, a.Length);
    }

    // ── HMAC-SHA512 (RFC 4231 vectors) ──

    [Fact]
    public void HMACSHA512_TestVector_RFC4231_Case2()
    {
        string mac = HMACSHA512.ToHex(HMACSHA512.ComputeHash(
            Encoding.GetBytes("Jefe"),
            Encoding.GetBytes("what do ya want for nothing?")));
        Assert.Equal("164b7a7bfcf819e2e395fbe73b56e0a387bd64222e831fd610270cd7ea2505549758bf75c05a994a6d034f65f8f0e6fdcaeab1a34d4a6b4b636e070a38bce737", mac);
    }

    [Fact]
    public void HMACSHA512_Consistent()
    {
        string a = HMACSHA512.ToHex(HMACSHA512.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        string b = HMACSHA512.ToHex(HMACSHA512.ComputeHash(Encoding.GetBytes("k"), Encoding.GetBytes("m")));
        Assert.Equal(a, b);
        Assert.Equal(128, a.Length);
    }

    // ── CSPRNG ──

    [Fact]
    public void CSPRNG_GetBytes_Length()
    {
        byte[] rng = CSPRNG.GetBytes(8);
        Assert.Equal(8, rng.Length);
    }

    [Fact]
    public void CSPRNG_GetBytes_NonEmpty()
    {
        byte[] rng1 = CSPRNG.GetBytes(16);
        byte[] rng2 = CSPRNG.GetBytes(16);
        Assert.Equal(16, rng1.Length);
        Assert.Equal(16, rng2.Length);
    }

    [Fact]
    public void CSPRNG_GetBytes_DistinctDraws()
    {
        // 两次独立抽取原始字节不相等（以 hex 序列比对；非 CAVP 合格性）
        byte[] rng1 = CSPRNG.GetBytes(16);
        byte[] rng2 = CSPRNG.GetBytes(16);
        Assert.NotEqual(Hex.ToHexString(rng1), Hex.ToHexString(rng2));
    }
}
