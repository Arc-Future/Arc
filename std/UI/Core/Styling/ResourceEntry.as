// RFC 037 D3.6 / RFC 037 D4: Arc.UI.Styling — ResourceEntry 资源条目。
namespace Arc.UI.Styling;

/// <summary>资源条目：键值对。</summary>
internal struct ResourceEntry {
    public string Key;
    public ResourceValue Value;

    public ResourceEntry() { }

    public ResourceEntry(string key, ResourceValue value) {
        this.Key = key;
        this.Value = value;
    }
}
