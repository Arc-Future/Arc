// RFC 025 M4: Arc.Net — IPHostEntry 主机解析结果。
namespace Arc.Net;

/// <summary>
/// 主机解析结果——包含主机名和 IP 地址列表。
/// 对标 C# System.Net.IPHostEntry。
/// </summary>
public struct IPHostEntry {
    /// <summary>主机名。</summary>
    public string HostName;
    /// <summary>IPv4/IPv6 地址列表（空格分隔）。</summary>
    public string AddressList;
    /// <summary>地址数量。</summary>
    public int AddressCount { get { return this.CountAddresses(); } }

    private int CountAddresses() {
        if (this.AddressList == "") { return 0; }
        int n = 1;
        int i = 0;
        while (i < this.AddressList.Length) {
            if (this.AddressList.Substring(i, 1) == " ") { n = n + 1; }
            i = i + 1;
        }
        return n;
    }

    /// <summary>获取第 index 个 IP 地址（0-based）。</summary>
    public string GetAddress(int index) {
        if (this.AddressList == "") { return ""; }
        int start = 0;
        int j = 0;
        while (j < index) {
            int sp = this.AddressList.IndexOf(" ", start);
            if (sp < 0) { return ""; }
            start = sp + 1;
            j = j + 1;
        }
        int end = this.AddressList.IndexOf(" ", start);
        if (end < 0) { return this.AddressList.Substring(start, this.AddressList.Length - start); }
        return this.AddressList.Substring(start, end - start);
    }
}
