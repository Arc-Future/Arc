// RFC 038：AIWiki —— 互链知识图（唯一内置结构化记忆）。
//
// 增量扩展：保留 RFC 038 扁平页 API（Get/Upsert/Delete/List/CreateSnapshot/Restore）向后兼容，
// 新增三操作基础：
//   - Ingest(AIWikiSource)：编译翻译器——把不可变源整合进知识图。落库前防腐校验
//     （L1 源不可变+指纹 / L2 声明-源锚定）：无源断言（G1）fail-closed 拒绝，持久层不含坏数据。
//   - Query / QueryByTag / ClaimsFor：按目录/标签/断言确定性结构命中并合成（本项目不做向量 RAG）。
//   - Lint()：编译期体检（可证伪 G8 broken_links / G9 dup_aliases / G10 orphan_claims / L6 weak_citations）。
namespace Arc.Agent;
using Arc;
using Arc.Collections;
using Arc.IO;
using Arc.Text.Json;

/// <summary>唯一内置结构化记忆（进程内完整 + 落盘持久化）。禁 IMemory 乐高。</summary>
public class AIWiki : IJsonSerializable, IJsonDeserializable {
    private List<string> _paths;
    private List<AIWikiPage> _pages;
    private List<AIWikiSource> _sources;
    private List<AIWikiClaim> _claims;

    public AIWiki() {
        _paths = new List<string>();
        _pages = new List<AIWikiPage>();
        _sources = new List<AIWikiSource>();
        _claims = new List<AIWikiClaim>();
    }

    // ── RFC 038：落盘持久化（SaveAsync / LoadAsync；Reactor 真异步文件 I/O）──

    /// <summary>把整图（pages/sources/claims）序列化为 JSON 并异步写入 <paramref name="path"/>。
    /// 成功返回 true。Backlinks 为派生态，不落盘（加载时重建）。</summary>
    public async Task<bool> SaveAsync(string path, CancellationToken cancellationToken) {
        IJsonSerializable boxed = this;
        string json = JsonSerializer.Serialize(boxed);
        cancellationToken.ThrowIfCancellationRequested();
        return await File.WriteAllTextAsync(path, json);
    }

    /// <summary>从 <paramref name="path"/> 异步加载整图；文件不存在返回空 AIWiki。
    /// 加载后重建路径索引与反向引用（Backlinks）。</summary>
    public static async Task<AIWiki> LoadAsync(string path, CancellationToken cancellationToken) {
        if (!await File.ExistsAsync(path)) {
            return new AIWiki();
        }
        string json = await File.ReadAllTextAsync(path);
        cancellationToken.ThrowIfCancellationRequested();
        if (json == null) {
            return new AIWiki();
        }
        AIWiki wiki = JsonSerializer.Deserialize<AIWiki>(json);
        wiki.RebuildIndexes();
        return wiki;
    }

    /// <summary>JSON 序列化：整图三表（pages/sources/claims）。</summary>
    public void WriteJson(JsonWriter writer) {
        writer.WriteStartObject();
        writer.WritePropertyName("pages");
        writer.WriteStartArray();
        int np = _pages.Count;
        int pi = 0;
        while (pi < np) {
            _pages[pi].WriteJson(writer);
            pi = pi + 1;
        }
        writer.WriteEndArray();
        writer.WritePropertyName("sources");
        writer.WriteStartArray();
        int ns = _sources.Count;
        int si = 0;
        while (si < ns) {
            _sources[si].WriteJson(writer);
            si = si + 1;
        }
        writer.WriteEndArray();
        writer.WritePropertyName("claims");
        writer.WriteStartArray();
        int nc = _claims.Count;
        int ci = 0;
        while (ci < nc) {
            _claims[ci].WriteJson(writer);
            ci = ci + 1;
        }
        writer.WriteEndArray();
        writer.WriteEndObject();
    }

    /// <summary>JSON 反序列化：还原整图三表（Backlinks/路径索引随后由 LoadAsync 重建）。</summary>
    public void ReadJson(JsonReader reader) {
        while (reader.Read()) {
            if (reader.TokenType == JsonTokenType.EndObject) {
                return;
            }
            if (reader.TokenType == JsonTokenType.PropertyName) {
                string prop = reader.GetString();
                reader.Read();
                if (prop == "pages") {
                    this.ReadPages(reader);
                } else if (prop == "sources") {
                    this.ReadSources(reader);
                } else if (prop == "claims") {
                    this.ReadClaims(reader);
                } else {
                    reader.Skip();
                }
            }
        }
    }

    private void ReadPages(JsonReader reader) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            AIWikiPage page = new AIWikiPage();
            page.ReadJson(reader);
            _pages.Add(page);
            _paths.Add(page.Path);
            reader.Read();
        }
    }

    private void ReadClaims(JsonReader reader) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            AIWikiClaim claim = new AIWikiClaim();
            claim.ReadJson(reader);
            _claims.Add(claim);
            reader.Read();
        }
    }

    private void ReadSources(JsonReader reader) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            string id = "";
            string content = "";
            DateTime capturedAt = new DateTime(0);
            string previousId = "";
            List<AIWikiPage> pages = new List<AIWikiPage>();
            List<AIWikiClaim> claims = new List<AIWikiClaim>();
            bool go = true;
            while (go && reader.Read()) {
                if (reader.TokenType == JsonTokenType.EndObject) {
                    go = false;
                } else if (reader.TokenType == JsonTokenType.PropertyName) {
                    string prop = reader.GetString();
                    reader.Read();
                    if (prop == "id") {
                        id = reader.GetString();
                    } else if (prop == "content") {
                        content = reader.GetString();
                    } else if (prop == "capturedAt") {
                        capturedAt = DateTime.Parse(reader.GetString());
                    } else if (prop == "previousId") {
                        previousId = reader.GetString();
                    } else if (prop == "pages") {
                        this.ReadNestedPages(reader, pages);
                    } else if (prop == "claims") {
                        this.ReadNestedClaims(reader, claims);
                    } else {
                        reader.Skip();
                    }
                }
            }
            AIWikiSource src = new AIWikiSource(id, content, capturedAt, previousId, pages, claims);
            _sources.Add(src);
            reader.Read();
        }
    }

    private void ReadNestedPages(JsonReader reader, List<AIWikiPage> pages) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            AIWikiPage page = new AIWikiPage();
            page.ReadJson(reader);
            pages.Add(page);
            reader.Read();
        }
    }

    private void ReadNestedClaims(JsonReader reader, List<AIWikiClaim> claims) {
        reader.Read();
        while (reader.TokenType != JsonTokenType.EndArray) {
            AIWikiClaim claim = new AIWikiClaim();
            claim.ReadJson(reader);
            claims.Add(claim);
            reader.Read();
        }
    }

    /// <summary>加载后重建路径索引与反向引用（Backlinks）。</summary>
    private void RebuildIndexes() {
        List<string> paths = new List<string>();
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            paths.Add(_pages[i].Path);
            i = i + 1;
        }
        _paths = paths;
        this.RebuildBacklinks();
    }

    public AIWikiPage Get(string path) {
        int i = this.IndexOf(path);
        if (i < 0) { return null; }
        return _pages[i];
    }

    public void Upsert(string path, string body) {
        this.Upsert(path, body, new AIWikiMeta());
    }

    public void Upsert(string path, string body, AIWikiMeta meta) {
        string p = path != null ? path : "";
        string b = body != null ? body : "";
        AIWikiMeta m = meta != null ? meta : new AIWikiMeta();
        int i = this.IndexOf(p);
        AIWikiPage page = new AIWikiPage(p, b, m);
        if (i >= 0) {
            _pages[i] = page;
            return;
        }
        _paths.Add(p);
        _pages.Add(page);
    }

    public bool Delete(string path) {
        int i = this.IndexOf(path);
        if (i < 0) { return false; }
        _paths.RemoveAt(i);
        _pages.RemoveAt(i);
        return true;
    }

    public List<string> List(string pathPrefix) {
        List<string> outList = new List<string>();
        string prefix = pathPrefix != null ? pathPrefix : "";
        int n = _paths.Count;
        int i = 0;
        while (i < n) {
            string p = _paths[i];
            if (this.MatchesPrefix(p, prefix)) {
                outList.Add(p);
            }
            i = i + 1;
        }
        return outList;
    }

    // ── RFC 038：Ingest —— 编译翻译器（防腐 L1 源不可变+指纹 / L2 声明-源锚定） ──

    /// <summary>
    /// 把不可变源整合进知识图。落库前防腐校验：
    ///   - G1 fail-closed：任一断言未锚定 source_id → 抛异常，持久层不含任何坏数据。
    /// 落库后返回当前 Lint 报告（G8/G9/G10/L6 立即可见）。源本体只读消费，绝不改写（L1）。
    /// </summary>
    public AIWikiLinter Ingest(AIWikiSource source) {
        if (source == null) {
            throw new ArgumentNullException("source");
        }
        // G1：落库前校验无源断言；任一不锚定源 → 整次拒绝（fail-closed）。
        List<AIWikiClaim> claims = source.Claims;
        int nc = claims.Count;
        int ic = 0;
        while (ic < nc) {
            AIWikiClaim c = claims[ic];
            if (c != null && !c.HasSource()) {
                throw new ArgumentException(
                    "Ingest G1 fail-closed: assertion without source: " + c.Id);
            }
            ic = ic + 1;
        }
        // L1：追加源版本（同 Id 新指纹 = 新版本，PreviousId 由调用方给定；绝不原地改写）。
        this._sources.Add(source);
        // L2：登记断言（同 Id 已存在则保留原断言，Lint 记 dup）。
        int ci = 0;
        while (ci < nc) {
            AIWikiClaim c2 = claims[ci];
            if (c2 != null && this.IndexOfClaim(c2.Id) < 0) {
                this._claims.Add(c2);
            }
            ci = ci + 1;
        }
        // 登记知识页（同 Path 已存在则跳过，Lint 记 dup 别名）。
        List<AIWikiPage> pages = source.Pages;
        int np = pages.Count;
        int pi = 0;
        while (pi < np) {
            AIWikiPage p = pages[pi];
            this.AddPage(p);
            pi = pi + 1;
        }
        // 重建反向引用（backlink 支持）。
        this.RebuildBacklinks();
        return this.Lint();
    }

    // ── RFC 038：Query —— 确定性结构命中（目录/标签/断言合成；非向量 RAG） ──

    /// <summary>按目录前缀命中页面（结构序稳定）。</summary>
    public List<AIWikiPage> Query(string directoryPrefix) {
        List<AIWikiPage> result = new List<AIWikiPage>();
        string prefix = directoryPrefix != null ? directoryPrefix : "";
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage page = _pages[i];
            if (page != null && this.MatchesPrefix(page.Path, prefix)) {
                result.Add(page);
            }
            i = i + 1;
        }
        return result;
    }

    /// <summary>按标签命中页面（Meta.Tags 包含 tag）。</summary>
    public List<AIWikiPage> QueryByTag(string tag) {
        List<AIWikiPage> result = new List<AIWikiPage>();
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage page = _pages[i];
            if (page != null && this.HasTag(page.Meta, tag)) {
                result.Add(page);
            }
            i = i + 1;
        }
        return result;
    }

    /// <summary>断言命中/合成：返回页面引用的全部断言。</summary>
    public List<AIWikiClaim> ClaimsFor(AIWikiPage page) {
        List<AIWikiClaim> result = new List<AIWikiClaim>();
        if (page == null) {
            return result;
        }
        string[] ids = page.ClaimIds;
        int n = ids.Length;
        int i = 0;
        while (i < n) {
            string cid = ids[i];
            AIWikiClaim c = this.GetClaim(cid);
            if (c != null) {
                result.Add(c);
            }
            i = i + 1;
        }
        return result;
    }

    /// <summary>已登记源数量（版本链可证）。</summary>
    public int SourceCount {
        get { return _sources.Count; }
    }

    /// <summary>按 SourceId 取首个登记源（版本链头部）。</summary>
    public AIWikiSource GetSource(string sourceId) {
        int n = _sources.Count;
        int i = 0;
        while (i < n) {
            if (_sources[i].Id == sourceId) {
                return _sources[i];
            }
            i = i + 1;
        }
        return null;
    }

    // ── RFC 038：Lint —— 编译期体检（可证伪 G8/G9/G10/L6） ──

    /// <summary>全图体检：broken_links(G8) / dup_aliases(G9) / orphan_claims(G10) / weak_citations(L6)。</summary>
    public AIWikiLinter Lint() {
        AIWikiLinter report = new AIWikiLinter();
        this.LintBrokenLinks(report);
        this.LintDupAliases(report);
        this.LintOrphanClaims(report);
        this.LintWeakCitations(report);
        return report;
    }

    /// <summary>深拷贝页面表（进程内 Snapshot 正道；落盘后置）。</summary>
    public AIWikiSnapshot CreateSnapshot() {
        List<AIWikiPage> copy = new List<AIWikiPage>();
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            copy.Add(_pages[i].Clone());
            i = i + 1;
        }
        return new AIWikiSnapshot(copy);
    }

    /// <summary>用快照全量替换当前页面表。</summary>
    public void Restore(AIWikiSnapshot snapshot) {
        if (snapshot == null) {
            throw new ArgumentNullException("snapshot");
        }
        // H1: 换新 List，避免 Clear 批量释放页面与报告期堆交织。
        List<string> paths = new List<string>();
        List<AIWikiPage> copy = new List<AIWikiPage>();
        List<AIWikiPage> pages = snapshot.Pages;
        int n = pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage p = pages[i].Clone();
            paths.Add(p.Path);
            copy.Add(p);
            i = i + 1;
        }
        _paths = paths;
        _pages = copy;
    }

    private int IndexOf(string path) {
        string p = path != null ? path : "";
        int n = _paths.Count;
        int i = 0;
        while (i < n) {
            if (_paths[i] == p) { return i; }
            i = i + 1;
        }
        return -1;
    }

    // ── Lint 子体检 ──

    private void LintBrokenLinks(AIWikiLinter report) {
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage page = _pages[i];
            if (page != null) {
                string[] links = page.Links;
                int nl = links.Length;
                int j = 0;
                while (j < nl) {
                    string tgt = links[j];
                    if (this.IndexOf(tgt) < 0) {
                        report.BrokenLinks.Add(page.Path + " -> " + tgt);
                    }
                    j = j + 1;
                }
            }
            i = i + 1;
        }
    }

    private void LintDupAliases(AIWikiLinter report) {
        this.LintDupPaths(report);
        this.LintDupPageIds(report);
        this.LintDupSourceIds(report);
        this.LintDupClaimIds(report);
    }

    private void LintDupPaths(AIWikiLinter report) {
        int n = _paths.Count;
        int i = 0;
        while (i < n) {
            int j = i + 1;
            while (j < n) {
                if (_paths[i] != "" && _paths[i] == _paths[j]) {
                    this.NoteDup(report, "path:" + _paths[i]);
                }
                j = j + 1;
            }
            i = i + 1;
        }
    }

    private void LintDupPageIds(AIWikiLinter report) {
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage a = _pages[i];
            int j = i + 1;
            while (j < n) {
                AIWikiPage b = _pages[j];
                if (a.PageId != "" && a.PageId == b.PageId) {
                    this.NoteDup(report, "page:" + a.PageId);
                }
                j = j + 1;
            }
            i = i + 1;
        }
    }

    private void LintDupSourceIds(AIWikiLinter report) {
        int n = _sources.Count;
        int i = 0;
        while (i < n) {
            AIWikiSource a = _sources[i];
            int j = i + 1;
            while (j < n) {
                AIWikiSource b = _sources[j];
                // 同 Id 且同指纹 = 真重复；同 Id 异指纹 = 版本链，不算 dup。
                if (a.Id != "" && a.Id == b.Id && a.Fingerprint == b.Fingerprint) {
                    this.NoteDup(report, "source:" + a.Id);
                }
                j = j + 1;
            }
            i = i + 1;
        }
    }

    private void LintDupClaimIds(AIWikiLinter report) {
        int n = _claims.Count;
        int i = 0;
        while (i < n) {
            AIWikiClaim a = _claims[i];
            int j = i + 1;
            while (j < n) {
                AIWikiClaim b = _claims[j];
                if (a.Id != "" && a.Id == b.Id) {
                    this.NoteDup(report, "claim:" + a.Id);
                }
                j = j + 1;
            }
            i = i + 1;
        }
    }

    private void LintOrphanClaims(AIWikiLinter report) {
        int n = _claims.Count;
        int i = 0;
        while (i < n) {
            AIWikiClaim c = _claims[i];
            if (c != null && !this.IsClaimReferenced(c.Id)) {
                report.OrphanClaims.Add(c.Id);
            }
            i = i + 1;
        }
    }

    private void LintWeakCitations(AIWikiLinter report) {
        int n = _claims.Count;
        int i = 0;
        while (i < n) {
            AIWikiClaim c = _claims[i];
            if (c != null && (c.Confidence == AIWikiConfidence.Low || !c.IsVerified())) {
                report.WeakCitations.Add(c.Id);
            }
            i = i + 1;
        }
    }

    private void NoteDup(AIWikiLinter report, string key) {
        if (!this.ContainsStr(report.DupAliases, key)) {
            report.DupAliases.Add(key);
        }
    }

    // ── 图维护辅助 ──

    private void AddPage(AIWikiPage page) {
        if (page == null) {
            return;
        }
        // 同 Path 已存在 = 重复别名 → 跳过（Lint 记 dup）；绝不覆盖既有图节点。
        if (this.IndexOf(page.Path) >= 0) {
            return;
        }
        _paths.Add(page.Path);
        _pages.Add(page);
    }

    /// <summary>重建全部页面的反向引用（由 Ingest 在整合后调用）。</summary>
    private void RebuildBacklinks() {
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            AIWikiPage pi = _pages[i];
            pi.Backlinks = new List<string>();
            i = i + 1;
        }
        int j = 0;
        while (j < n) {
            AIWikiPage page = _pages[j];
            string[] links = page.Links;
            int nl = links.Length;
            int k = 0;
            while (k < nl) {
                string tgt = links[k];
                int t = this.IndexOf(tgt);
                if (t >= 0) {
                    AIWikiPage tp = _pages[t];
                    tp.Backlinks.Add(page.Path);
                }
                k = k + 1;
            }
            j = j + 1;
        }
    }

    private bool IsClaimReferenced(string claimId) {
        int n = _pages.Count;
        int i = 0;
        while (i < n) {
            string[] ids = _pages[i].ClaimIds;
            int m = ids.Length;
            int j = 0;
            while (j < m) {
                string cid = ids[j];
                if (cid == claimId) {
                    return true;
                }
                j = j + 1;
            }
            i = i + 1;
        }
        return false;
    }

    private bool HasTag(AIWikiMeta meta, string tag) {
        if (meta == null || tag == null || tag == "") {
            return false;
        }
        string[] tags = meta.Tags;
        int n = tags.Length;
        int i = 0;
        while (i < n) {
            string tg = tags[i];
            if (tg == tag) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private bool MatchesPrefix(string path, string prefix) {
        if (prefix == "") {
            return true;
        }
        if (path == null) {
            return false;
        }
        if (path.Length >= prefix.Length) {
            return path.Substring(0, prefix.Length) == prefix;
        }
        return false;
    }

    private bool ContainsStr(List<string> list, string s) {
        int n = list.Count;
        int i = 0;
        while (i < n) {
            if (list[i] == s) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private AIWikiClaim GetClaim(string claimId) {
        int n = _claims.Count;
        int i = 0;
        while (i < n) {
            if (_claims[i].Id == claimId) {
                return _claims[i];
            }
            i = i + 1;
        }
        return null;
    }

    private int IndexOfClaim(string claimId) {
        int n = _claims.Count;
        int i = 0;
        while (i < n) {
            if (_claims[i].Id == claimId) {
                return i;
            }
            i = i + 1;
        }
        return -1;
    }
}
