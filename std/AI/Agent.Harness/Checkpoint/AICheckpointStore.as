// RFC 043 P2：绿点快照存储 — 会话级工作区状态快照 + 多绿点历史 + 指定回滚。
//
// 快照内容（checkpoint:green 时落盘 `target/scratch/arc-checkpoints/`）：
//   - 索引 `index.json`：绿点摘要列表（Id / Seq / Label / AIRfc Revision / PlanStatus /
//     git HEAD），按捕获顺序追加，尾部为最近绿点；
//   - 全量快照 `checkpoint-<seq>.json`：git HEAD + stash 列表 + 文件清单 + AIRfc Revision
//     + AIPlan 状态摘要；
//   - 大文件副本 `objects/<sha256>.bin`：>MaxFileContentBytes 的文件内容寻址存储（SHA256 +
//     副本），回滚真实恢复副本；副本不可写 → 仅登记存在（回滚退化为 git checkout 或跳过，
//     边界诚实暴露）。
//
// 回滚（checkpoint:rollback 时执行）：按指定绿点（默认最近）快照恢复内容、删除快照后新建
// 文件；无快照 → FoundSnapshot=false（升级人）。文件清单截断（Truncated）时不删除「新建
// 文件」（清单不完整时删除语义不成立）。恢复失败/跳过 → Success=false（部分恢复诚实暴露）。
namespace Arc.Agent.Harness;
using Arc;
using Arc.Collections;
using Arc.Diagnostics;
using Arc.IO;
using Arc.Security;
using Arc.Text;
using Arc.Text.Json;

/// <summary>
/// 绿点快照存储：捕获 / 列出历史 / 读取 / 回滚到指定绿点（默认最近）。
/// 多绿点历史 + 大文件内容寻址，供 AIRfc/Plan 联动回滚。
/// </summary>
public class AICheckpointStore {
    private const int MaxFileContentBytes = 65536;
    private const int MaxManifestEntries = 4000;
    private const string StoreRelDir = "target/scratch/arc-checkpoints";
    private const string IndexFile = "index.json";
    private const string CheckpointFilePrefix = "checkpoint-";
    private const string ObjectsDirName = "objects";

    private string _root;
    private string _storeDir;
    private bool _ready;
    private int _latestSeq;
    private string _latestId;

    public AICheckpointStore(string project) {
        this.InitStore(project, AICheckpointStore.StoreRelDir);
    }

    /// <summary>
    /// 分支级绿点隔离（方案 B B2，conflict-branch §3）：显式指定相对目录
    /// （如 <c>target/scratch/arc-checkpoints/&lt;branch&gt;</c>），使多分支绿点互不踩踏。
    /// </summary>
    public AICheckpointStore(string project, string storeRelDir) {
        string rel = storeRelDir != null && storeRelDir != "" ? storeRelDir : AICheckpointStore.StoreRelDir;
        this.InitStore(project, rel);
    }

    private void InitStore(string project, string storeRelDir) {
        string target = project != null && project != "" ? project : ".";
        string root = target;
        if (File.Exists(target)) {
            string parent = Path.GetDirectoryName(target);
            root = parent != null && parent != "" ? parent : ".";
        }
        _root = root != null ? root : "";
        _ready = Directory.Exists(_root);
        _storeDir = _ready ? Path.Combine(_root, storeRelDir) : "";
        _latestSeq = 0;
        _latestId = "";
    }

    public string Root {
        get { return _root; }
    }

    public string StoreDir {
        get { return _storeDir; }
    }

    public bool IsReady {
        get { return _ready; }
    }

    /// <summary>最近捕获/读取的绿点 id（如 "cp-000001"）；尚未捕获时为空。</summary>
    public string LatestCheckpointId {
        get { return _latestId; }
    }

    /// <summary>
    /// 捕获绿点快照：git 状态（best-effort）+ 文件清单 → 追加一条索引 + 一份
    /// `checkpoint-&lt;seq&gt;.json`。项目根不可解析（目录不存在）→ 返回 false（不建目录、不污染）。
    /// </summary>
    public async Task<bool> CaptureAsync(string label, int revision, string planStatus, CancellationToken cancellationToken) {
        if (!_ready) {
            return false;
        }
        cancellationToken.ThrowIfCancellationRequested();
        if (!await this.EnsureDirectoryAsync(_storeDir)) {
            return false;
        }
        if (!await this.EnsureDirectoryAsync(Path.Combine(_storeDir, AICheckpointStore.ObjectsDirName))) {
            return false;
        }
        List<AICheckpointIndexEntry> index = await this.LoadIndexAsync(cancellationToken);
        int seq = AICheckpointStore.NextSeq(index);
        string id = AICheckpointStore.CheckpointId(seq);
        AICheckpointSnapshot snap = new AICheckpointSnapshot();
        snap.Id = id;
        snap.Seq = seq;
        snap.Label = label != null ? label : "";
        snap.Revision = revision;
        snap.PlanStatus = planStatus != null ? planStatus : "";
        snap.CreatedAt = this.NowStamp();
        snap.GitHead = await this.GitHeadAsync(cancellationToken);
        snap.StashList = await this.GitStashListAsync(cancellationToken);
        this.BuildManifest(snap, cancellationToken);
        string json = JsonSerializer.Serialize((IJsonSerializable)snap);
        string path = Path.Combine(_storeDir, AICheckpointStore.CheckpointFilePrefix + seq + ".json");
        bool wrote = await File.WriteAllTextAsync(path, json);
        if (!wrote) {
            return false;
        }
        AICheckpointIndexEntry entry = new AICheckpointIndexEntry();
        entry.Id = id;
        entry.Seq = seq;
        entry.Label = snap.Label;
        entry.Revision = revision;
        entry.PlanStatus = snap.PlanStatus;
        entry.CreatedAt = snap.CreatedAt;
        entry.GitHead = snap.GitHead;
        index.Add(entry);
        if (!await this.SaveIndexAsync(index, cancellationToken)) {
            return false;
        }
        _latestSeq = seq;
        _latestId = id;
        return true;
    }

    /// <summary>是否存在任何绿点（index.json 存在即认为有历史；L2 迭代前置检查）。</summary>
    public bool HasSnapshot() {
        if (!_ready) {
            return false;
        }
        string path = Path.Combine(_storeDir, AICheckpointStore.IndexFile);
        return File.Exists(path);
    }

    /// <summary>
    /// 回滚到指定绿点（<paramref name="checkpointId"/> 为空 → 最近）：恢复清单内差异文件、
    /// 删除快照后新建文件（清单未截断时）；大文件经内容寻址副本恢复（副本缺失 → git checkout
    /// 兜底或跳过）。无快照 / 目标不存在 → FoundSnapshot=false（升级人）。成功 = 无跳过项。
    /// </summary>
    public async Task<AICheckpointRollbackOutcome> RollbackAsync(string? checkpointId, CancellationToken cancellationToken) {
        AICheckpointRollbackOutcome outcome = new AICheckpointRollbackOutcome();
        if (!_ready) {
            outcome.Detail = "rollback: checkpoint store not ready";
            return outcome;
        }
        cancellationToken.ThrowIfCancellationRequested();
        List<AICheckpointIndexEntry> index = await this.LoadIndexAsync(cancellationToken);
        if (index.Count == 0) {
            outcome.Detail = "rollback: no snapshot (escalate)";
            return outcome;
        }
        AICheckpointIndexEntry? target = AICheckpointStore.ResolveIndexEntry(index, checkpointId);
        if (target == null) {
            string idText = "(latest)";
            if (checkpointId != null) {
                if (checkpointId.Length > 0) {
                    idText = checkpointId;
                }
            }
            outcome.Detail = "rollback: checkpoint not found: " + idText;
            return outcome;
        }
        AICheckpointSnapshot snap = await this.LoadSnapshotAsync(target.Seq, cancellationToken);
        if (snap == null) {
            outcome.Detail = "rollback: index entry " + target.Id + " but snapshot file missing (escalate)";
            return outcome;
        }
        outcome.FoundSnapshot = true;
        outcome.CheckpointId = snap.Id != "" ? snap.Id : target.Id;
        outcome.RfcRevision = snap.Revision;
        outcome.PlanStatusSummary = snap.PlanStatus;
        _latestSeq = target.Seq;
        _latestId = target.Id;

        List<string> current = new List<string>();
        List<string> currentRel = new List<string>();
        this.CollectFiles(_root, current, cancellationToken);
        int ci = 0;
        while (ci < current.Count) {
            currentRel.Add(this.RelativePath(current[ci]));
            ci = ci + 1;
        }

        bool anySkipped = false;
        int i = 0;
        while (i < snap.Files.Count) {
            AICheckpointFileEntry entry = snap.Files[i];
            string abs = Path.Combine(_root, entry.RelativePath);
            if (entry.HasContent) {
                bool exists = await File.ExistsAsync(abs);
                bool differs = !exists;
                if (!differs) {
                    string currentText = await File.ReadAllTextAsync(abs);
                    differs = currentText != entry.Content;
                }
                if (differs) {
                    if (await this.WriteFileAsync(abs, entry.Content, cancellationToken)) {
                        outcome.RestoredCount = outcome.RestoredCount + 1;
                    } else {
                        outcome.SkippedCount = outcome.SkippedCount + 1;
                        anySkipped = true;
                    }
                }
            } else if (entry.ObjectRef != null && entry.ObjectRef != "") {
                // 大文件：内容寻址副本真实恢复（非仅 git 依赖）。
                if (this.RestoreObject(entry.ObjectRef, abs)) {
                    outcome.RestoredCount = outcome.RestoredCount + 1;
                } else {
                    outcome.SkippedCount = outcome.SkippedCount + 1;
                    anySkipped = true;
                }
            } else {
                // 仅登记存在（副本不可写）→ git checkout 兜底；无 git 环境 → 跳过（诚实边界）。
                bool existsHere = await File.ExistsAsync(abs);
                if (!existsHere && snap.GitHead != "") {
                    ProcessRunResult r = await this.RunGitAsync(
                        "checkout -- " + AICheckpointStore.Quote(entry.RelativePath), cancellationToken);
                    if (r.ExitCode == 0) {
                        outcome.RestoredCount = outcome.RestoredCount + 1;
                    } else {
                        outcome.SkippedCount = outcome.SkippedCount + 1;
                        anySkipped = true;
                    }
                }
            }
            i = i + 1;
        }

        if (!snap.Truncated) {
            int ci2 = 0;
            while (ci2 < currentRel.Count) {
                if (!AICheckpointStore.HasPath(snap, currentRel[ci2])) {
                    if (await File.DeleteAsync(current[ci2])) {
                        outcome.DeletedCount = outcome.DeletedCount + 1;
                    }
                }
                ci2 = ci2 + 1;
            }
        }

        outcome.Success = !anySkipped;
        outcome.Detail = outcome.Describe();
        return outcome;
    }

    // ── 私有：索引 / 快照 ──

    private async Task<List<AICheckpointIndexEntry>> LoadIndexAsync(CancellationToken cancellationToken) {
        List<AICheckpointIndexEntry> list = new List<AICheckpointIndexEntry>();
        if (!_ready) {
            return list;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string path = Path.Combine(_storeDir, AICheckpointStore.IndexFile);
        bool exists = await File.ExistsAsync(path);
        if (!exists) {
            return list;
        }
        string json = await File.ReadAllTextAsync(path);
        if (json == null || json == "") {
            return list;
        }
        AICheckpointIndex idx = new AICheckpointIndex();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)idx);
        if (idx.Entries != null) {
            int i = 0;
            while (i < idx.Entries.Count) {
                list.Add(idx.Entries[i]);
                i = i + 1;
            }
        }
        if (list.Count > 0) {
            _latestSeq = list[list.Count - 1].Seq;
            _latestId = list[list.Count - 1].Id;
        }
        return list;
    }

    private async Task<bool> SaveIndexAsync(List<AICheckpointIndexEntry> entries, CancellationToken cancellationToken) {
        AICheckpointIndex idx = new AICheckpointIndex();
        if (entries != null) {
            int i = 0;
            while (i < entries.Count) {
                idx.Entries.Add(entries[i]);
                i = i + 1;
            }
        }
        string json = JsonSerializer.Serialize((IJsonSerializable)idx);
        string path = Path.Combine(_storeDir, AICheckpointStore.IndexFile);
        return await File.WriteAllTextAsync(path, json);
    }

    private async Task<AICheckpointSnapshot> LoadSnapshotAsync(int seq, CancellationToken cancellationToken) {
        if (!_ready) {
            return null;
        }
        cancellationToken.ThrowIfCancellationRequested();
        string path = Path.Combine(_storeDir, AICheckpointStore.CheckpointFilePrefix + seq + ".json");
        bool exists = await File.ExistsAsync(path);
        if (!exists) {
            return null;
        }
        string json = await File.ReadAllTextAsync(path);
        if (json == null || json == "") {
            return null;
        }
        AICheckpointSnapshot snap = new AICheckpointSnapshot();
        JsonSerializer.Deserialize(json, (IJsonDeserializable)snap);
        return snap;
    }

    private static AICheckpointIndexEntry? ResolveIndexEntry(List<AICheckpointIndexEntry> index, string? checkpointId) {
        if (index == null || index.Count == 0) {
            return null;
        }
        string id = "";
        if (checkpointId != null) {
            id = checkpointId;
        }
        if (id == "") {
            return index[index.Count - 1];
        }
        int i = 0;
        while (i < index.Count) {
            if (index[i].Id == id) {
                return index[i];
            }
            i = i + 1;
        }
        if (AICheckpointStore.IsDigits(id)) {
            int seq = Convert.ToInt32(id);
            int k = 0;
            while (k < index.Count) {
                if (index[k].Seq == seq) {
                    return index[k];
                }
                k = k + 1;
            }
        }
        return null;
    }

    private static bool IsDigits(string s) {
        if (s == null || s == "") {
            return false;
        }
        int i = 0;
        while (i < s.Length) {
            char c = s[i];
            if (c < '0' || c > '9') {
                return false;
            }
            i = i + 1;
        }
        return true;
    }

    private static int NextSeq(List<AICheckpointIndexEntry> index) {
        int max = 0;
        int i = 0;
        while (i < index.Count) {
            if (index[i].Seq > max) {
                max = index[i].Seq;
            }
            i = i + 1;
        }
        return max + 1;
    }

    private static string CheckpointId(int seq) {
        string s = seq.ToString();
        while (s.Length < 6) {
            s = "0" + s;
        }
        return "cp-" + s;
    }

    // ── 私有：文件清单 ──

    private void BuildManifest(AICheckpointSnapshot snap, CancellationToken cancellationToken) {
        List<string> files = new List<string>();
        this.CollectFiles(_root, files, cancellationToken);
        int i = 0;
        while (i < files.Count) {
            if (snap.Files.Count >= AICheckpointStore.MaxManifestEntries) {
                snap.Truncated = true;
                return;
            }
            snap.Files.Add(this.BuildEntry(files[i]));
            i = i + 1;
        }
    }

    private AICheckpointFileEntry BuildEntry(string absPath) {
        AICheckpointFileEntry entry = new AICheckpointFileEntry();
        entry.RelativePath = this.RelativePath(absPath);
        long len = AICheckpointStore.FileLength(absPath);
        if (len >= 0 && len <= AICheckpointStore.MaxFileContentBytes) {
            string content = File.ReadAllText(absPath);
            entry.Content = content;
            entry.Hash = SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes(content)));
            entry.HasContent = true;
        } else if (len > AICheckpointStore.MaxFileContentBytes) {
            // 大文件：内容寻址存储（SHA256 + 副本）；副本不可写 → 仅登记存在（诚实边界）。
            entry.Hash = AICheckpointStore.HashTextFile(absPath);
            entry.HasContent = false;
            bool stored = this.StoreObject(absPath, entry.Hash);
            if (stored) {
                entry.ObjectRef = entry.Hash;
            } else {
                entry.RegisteredOnly = true;
            }
        }
        return entry;
    }

    /// <summary>
    /// 大文件副本写入 objects/&lt;sha256&gt;.bin（内容寻址，天然去重；已存在 → 直接命中）。
    /// 同步拷贝：对象副本写入在清单构建循环内，嵌套 async 会触发编译器状态机已知缺陷
    /// （局部 List 跨 await 变 null），故保持同步（有界拷贝，量级同 git add 读盘）。
    /// </summary>
    private bool StoreObject(string absPath, string hash) {
        if (hash == null || hash == "") {
            return false;
        }
        string objectsDir = Path.Combine(_storeDir, AICheckpointStore.ObjectsDirName);
        if (!this.EnsureDirectory(objectsDir)) {
            return false;
        }
        string objPath = Path.Combine(objectsDir, hash + ".bin");
        if (File.Exists(objPath)) {
            return true;
        }
        return File.Copy(absPath, objPath);
    }

    /// <summary>从内容寻址副本恢复大文件到目标路径（父目录递归创建；覆盖写入）。同步拷贝（同上）。</summary>
    private bool RestoreObject(string objectRef, string absPath) {
        if (objectRef == null || objectRef == "") {
            return false;
        }
        string objectsDir = Path.Combine(_storeDir, AICheckpointStore.ObjectsDirName);
        string objPath = Path.Combine(objectsDir, objectRef + ".bin");
        if (!File.Exists(objPath)) {
            return false;
        }
        string parent = Path.GetDirectoryName(absPath);
        if (parent != null && parent != "") {
            if (!this.EnsureDirectory(parent)) {
                return false;
            }
        }
        return File.Copy(objPath, absPath);
    }

    private static string HashTextFile(string absPath) {
        string content = File.ReadAllText(absPath);
        return SHA256.ToHex(SHA256.ComputeHash(Encoding.GetBytes(content)));
    }

    private void CollectFiles(string dir, List<string> outFiles, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string[] files = Directory.GetFiles(dir);
        int fi = 0;
        while (fi < files.Length) {
            outFiles.Add(files[fi]);
            fi = fi + 1;
        }
        string[] sub = Directory.GetDirectories(dir);
        int di = 0;
        while (di < sub.Length) {
            string name = Path.GetFileName(sub[di]);
            if (!AICheckpointStore.IsSkippableDir(name)) {
                this.CollectFiles(sub[di], outFiles, cancellationToken);
            }
            di = di + 1;
        }
    }

    private static bool IsSkippableDir(string name) {
        return name == ".git" || name == "target" || name == "obj" || name == "bin"
            || name == ".arcagent" || name == "node_modules";
    }

    private static long FileLength(string absPath) {
        if (!File.Exists(absPath)) {
            return -1;
        }
        FileStream fs = FileStream.OpenRead(absPath);
        long len = fs.Length;
        fs.Dispose();
        return len;
    }

    private async Task<bool> WriteFileAsync(string absPath, string content, CancellationToken cancellationToken) {
        cancellationToken.ThrowIfCancellationRequested();
        string parent = Path.GetDirectoryName(absPath);
        if (parent != null && parent != "") {
            if (!await this.EnsureDirectoryAsync(parent)) {
                return false;
            }
        }
        return await File.WriteAllTextAsync(absPath, content);
    }

    /// <summary>逐层创建目录（rt_dir_create 仅建单层，父目录缺失时需递归创建）。</summary>
    private async Task<bool> EnsureDirectoryAsync(string dir) {
        bool exists = await Directory.ExistsAsync(dir);
        if (exists) {
            return true;
        }
        string parent = Path.GetDirectoryName(dir);
        if (parent != null && parent != "" && parent != dir) {
            bool okParent = await this.EnsureDirectoryAsync(parent);
            if (!okParent) {
                return false;
            }
        }
        return await Directory.CreateDirectoryAsync(dir);
    }

    /// <summary>逐层创建目录（同步版；供对象副本写入/恢复等清单循环内同步路径使用）。</summary>
    private bool EnsureDirectory(string dir) {
        if (Directory.Exists(dir)) {
            return true;
        }
        string parent = Path.GetDirectoryName(dir);
        if (parent != null && parent != "" && parent != dir) {
            if (!this.EnsureDirectory(parent)) {
                return false;
            }
        }
        return Directory.CreateDirectory(dir);
    }

    private static bool HasPath(AICheckpointSnapshot snap, string rel) {
        int i = 0;
        while (i < snap.Files.Count) {
            if (snap.Files[i].RelativePath == rel) {
                return true;
            }
            i = i + 1;
        }
        return false;
    }

    private string RelativePath(string absPath) {
        string rootNorm = this._root;
        if (rootNorm.EndsWith("/")) {
            rootNorm = rootNorm.Substring(0, rootNorm.Length - 1);
        }
        string prefix = rootNorm + "/";
        if (absPath.StartsWith(prefix)) {
            return absPath.Substring(prefix.Length);
        }
        return Path.GetFileName(absPath);
    }

    private async Task<string> GitHeadAsync(CancellationToken cancellationToken) {
        ProcessRunResult r = await Process.RunCaptureAsync(this.GitStartInfo("rev-parse HEAD"), cancellationToken);
        if (r == null || r.ExitCode != 0) {
            return "";
        }
        string h = "";
        if (r.StandardOutput != null) {
            h = r.StandardOutput.Trim();
        }
        return h;
    }

    private async Task<string> GitStashListAsync(CancellationToken cancellationToken) {
        ProcessRunResult r = await Process.RunCaptureAsync(this.GitStartInfo("stash list"), cancellationToken);
        if (r == null || r.ExitCode != 0) {
            return "";
        }
        string h = "";
        if (r.StandardOutput != null) {
            h = r.StandardOutput.Trim();
        }
        return h;
    }

    private async Task<ProcessRunResult> RunGitAsync(string args, CancellationToken cancellationToken) {
        return await Process.RunCaptureAsync(this.GitStartInfo(args), cancellationToken);
    }

    private ProcessStartInfo GitStartInfo(string args) {
        ProcessStartInfo si = new ProcessStartInfo();
        string gitArgs = "git -C " + AICheckpointStore.Quote(this._root) + " " + args;
        if (Environment.IsWindows()) {
            si.FileName = "cmd.exe";
            si.Arguments = "/c " + gitArgs;
        } else {
            si.FileName = "/bin/sh";
            si.Arguments = "-c " + gitArgs;
        }
        si.RedirectStandardOutput = true;
        si.RedirectStandardError = true;
        return si;
    }

    private string NowStamp() {
        DateTime now = DateTime.Now;
        string s = now.Year.ToString();
        s = s + AICheckpointStore.Pad2(now.Month);
        s = s + AICheckpointStore.Pad2(now.Day);
        s = s + "-" + AICheckpointStore.Pad2(now.Hour);
        s = s + AICheckpointStore.Pad2(now.Minute);
        s = s + AICheckpointStore.Pad2(now.Second);
        return s;
    }

    private static string Pad2(int value) {
        return (value < 10 ? "0" : "") + value.ToString();
    }

    private static string Quote(string path) {
        if (path == null) {
            return "\"\"";
        }
        if (path.IndexOf(" ") >= 0) {
            return "\"" + path + "\"";
        }
        return path;
    }
}
