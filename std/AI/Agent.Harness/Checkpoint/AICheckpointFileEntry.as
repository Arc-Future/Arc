// RFC 043 P2：绿点快照文件条目 — 相对路径 + 内容哈希 + 可回滚内容（JSON 落盘）。
//
// 三级回滚能力（RFC 043 场景 3.4 多绿点 / 大文件策略）：
//   - HasContent=true：小文件（≤ MaxFileContentBytes）内联全文，无 git 环境也可回滚；
//   - ObjectRef 非空：大文件内容寻址存储（objects/<sha256>.bin 副本），回滚真实恢复副本；
//   - RegisteredOnly=true：大文件副本不可写时仅登记存在（回滚退化为 git checkout 或跳过，
//     边界诚实暴露，不冒充真回滚）。
namespace Arc.Agent.Harness;
using Arc.Text.Json;

/// <summary>
/// 绿点快照的单文件条目。小文件（≤ MaxFileContentBytes）存全文以便无 git 环境回滚；
/// 大文件经 <see cref="ObjectRef"/> 内容寻址存储副本；均不可行时仅登记存在
/// （<see cref="RegisteredOnly"/>，回滚退化为 git checkout 或跳过）。
/// </summary>
public class AICheckpointFileEntry : IJsonSerializable, IJsonDeserializable {
    public string RelativePath;
    public string Hash;
    public bool HasContent;
    public string Content;
    /// <summary>内容寻址对象引用（大文件副本，如 "a3f..."，对应 objects/&lt;ref&gt;.bin）；空 = 无副本。</summary>
    public string ObjectRef;
    /// <summary>大文件仅登记存在（无副本、无内联内容；回滚依赖 git checkout 或跳过）。</summary>
    public bool RegisteredOnly;

    public AICheckpointFileEntry() {
        this.RelativePath = "";
        this.Hash = "";
        this.HasContent = false;
        this.Content = "";
        this.ObjectRef = "";
        this.RegisteredOnly = false;
    }

    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WriteString("path", this.RelativePath);
        writer.WriteString("hash", this.Hash);
        writer.WriteBoolean("hasContent", this.HasContent);
        writer.WriteString("content", this.Content);
        writer.WriteString("objectRef", this.ObjectRef);
        writer.WriteBoolean("registeredOnly", this.RegisteredOnly);
        writer.WriteEndObject();
    }

    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType != JsonTokenType.PropertyName) {
                continue;
            }
            string prop = reader.GetString();
            if (!reader.Read()) {
                return;
            }
            if (prop == "path") {
                this.RelativePath = reader.GetString();
            } else if (prop == "hash") {
                this.Hash = reader.GetString();
            } else if (prop == "hasContent") {
                this.HasContent = reader.GetBoolean();
            } else if (prop == "content") {
                this.Content = reader.GetString();
            } else if (prop == "objectRef") {
                this.ObjectRef = reader.GetString();
            } else if (prop == "registeredOnly") {
                this.RegisteredOnly = reader.GetBoolean();
            } else {
                reader.Skip();
            }
        }
    }
}
