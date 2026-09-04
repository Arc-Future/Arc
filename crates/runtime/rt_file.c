//! File & Directory runtime ABI（M1 + M3：基础文件操作 + 目录与路径）。
//!
//! 与 C# System.IO.File / System.IO.Directory / System.IO.Path 对齐的能力子集：
//!   - M1：基础文件操作（ReadAllText/WriteAllText/Exists/Delete/AppendAllText/Copy/Move）
//!   - M3：目录操作（CreateDirectory/Exists/Delete/GetFiles/GetDirectories）+ 路径操作（Combine 等）
//!
//! 设计决策：
//!   - 所有错误返回 0（false）或 NULL（空串），不引入异常机制
//!   - 与现有 facade 模式一致：.as 文件方法体为空 stub，codegen 拦截调用并
//!     直接发射 @rt_file_* / @rt_dir_* / @rt_path_* ABI
//!   - 从原 rt_str.c 迁出 rt_read_file / rt_write_file，让每个关注点独立成翻译单元
//!   - 路径操作为纯字符串计算，但仍通过 rt_path_* ABI 实现以保持 facade 模式一致性
//!
//! 平台兼容性：
//!   - 路径分隔符统一使用 '/'，与 Windows/Unix 均兼容（现代 Windows 接受正斜杠）
//!   - GetFiles → rt_dir_list_files / rt_dir_list_files_pattern（* / ? 在 C 侧匹配）
//!   - GetDirectories → rt_dir_list_dirs：直接子目录完整路径；失败/空 Length 0

#include "rt_abi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <errno.h>
#include <stdatomic.h>

#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#include <direct.h>
#include <windows.h>
#define RT_PATH_SEP '\\'
#define RT_ACCESS _access
#define RT_MKDIR(path) _mkdir(path)
#define RT_RMDIR(path) _rmdir(path)
/* ssize_t 是 POSIX 类型，Windows MSVC 不原生提供；此处用 long long 替代
 * （64 位有符号，足以表示任何文件路径长度）。 */
typedef long long ssize_t;
#else
#include <unistd.h>
#include <dirent.h>
#include <fnmatch.h>
#include <sys/types.h>
#define RT_PATH_SEP '/'
#define RT_ACCESS access
#define RT_MKDIR(path) mkdir(path, 0755)
#define RT_RMDIR(path) rmdir(path)
#endif

/* ---- M1: 文件读取与写入（从 rt_str.c 迁入） ---- */

/* 低层 I/O（跨平台）：Windows 走 Win32 CreateFile/ReadFile/WriteFile/CloseHandle，
 * POSIX 走 open/read/write。相对 stdio FILE*：省 fseek/ftell 两趟 lseek + 默认
 * 4KB 缓冲导致的多次小读写（64KiB 载荷 fread ≈16 次 _read），吞吐基准由此直接受益。
 * Windows 不用 CRT `_open`：本机实测单次开销 ~200-390µs（AV 扫描拖慢），
 * CreateFileA/W 快约 10 倍（见 target/scratch/prof_io.c 对照）。 */

#ifdef _WIN32
typedef HANDLE rt_io_handle_t;
#define RT_IO_HANDLE_INVALID INVALID_HANDLE_VALUE
#define RT_IO_SHARE (FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
#else
typedef int rt_io_handle_t;
#define RT_IO_HANDLE_INVALID (-1)
#endif

#ifdef _WIN32
/* ASCII 快速路径：UTF-8 与 ANSI 代码页在 0x00-0x7F 一致，直接用 CreateFileA
 * 免去 UTF-8→UTF-16 转换；非 ASCII 路径转 UTF-16 走 CreateFileW（CRT `_open`
 * 按 ANSI 代码页解释，本就无法正确打开这类路径）。 */
static int rt_io_ascii_only(const char* s) {
    const unsigned char* p = (const unsigned char*)s;
    for (; *p; p++) {
        if (*p >= 0x80) return 0;
    }
    return 1;
}

static rt_io_handle_t rt_io_open(const char* path, DWORD access, DWORD disposition) {
    if (rt_io_ascii_only(path)) {
        return CreateFileA(path, access, RT_IO_SHARE, NULL, disposition,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    }
    int n = MultiByteToWideChar(CP_UTF8, 0, path, -1, NULL, 0);
    if (n <= 0) return INVALID_HANDLE_VALUE;
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    if (!w) return INVALID_HANDLE_VALUE;
    MultiByteToWideChar(CP_UTF8, 0, path, -1, w, n);
    HANDLE h = CreateFileW(w, access, RT_IO_SHARE, NULL, disposition,
                           FILE_ATTRIBUTE_NORMAL, NULL);
    free(w);
    return h;
}
#endif

static ssize_t rt_io_read(rt_io_handle_t h, void* buf, size_t n) {
#ifdef _WIN32
    DWORD rd = 0;
    return ReadFile(h, buf, (DWORD)n, &rd, NULL) ? (ssize_t)rd : (ssize_t)-1;
#else
    return read(h, buf, n);
#endif
}

static ssize_t rt_io_write(rt_io_handle_t h, const void* buf, size_t n) {
#ifdef _WIN32
    DWORD wr = 0;
    return WriteFile(h, buf, (DWORD)n, &wr, NULL) ? (ssize_t)wr : (ssize_t)-1;
#else
    return write(h, buf, n);
#endif
}

static void rt_io_close(rt_io_handle_t h) {
#ifdef _WIN32
    CloseHandle(h);
#else
    close(h);
#endif
}

static int rt_io_fstat_size(rt_io_handle_t h, int64_t* out_size) {
#ifdef _WIN32
    LARGE_INTEGER sz;
    if (!GetFileSizeEx(h, &sz)) return -1;
    *out_size = sz.QuadPart;
    return 0;
#else
    struct stat st;
    if (fstat(h, &st) != 0) return -1;
    *out_size = (int64_t)st.st_size;
    return 0;
#endif
}

/* 读写复用句柄（缓存路径）：以 GENERIC_READ|GENERIC_WRITE 打开，同一句柄既服务
 * rt_read_file 又服务 rt_write_file，避免同路径反复 open/close（见 TLS 缓存下钻）。
 * 写路径先 OPEN_EXISTING（比 CREATE_ALWAYS 的截断打开便宜 ~2-3 倍，本机实测
 * 41µs vs 97µs），写后按需裁剪；文件不存在时回退 CREATE_ALWAYS。 */
static rt_io_handle_t rt_io_open_rw(const char* path) {       /* OPEN_EXISTING */
#ifdef _WIN32
    return rt_io_open(path, GENERIC_READ | GENERIC_WRITE, OPEN_EXISTING);
#else
    return open(path, O_RDWR);
#endif
}

static rt_io_handle_t rt_io_open_rw_create(const char* path) { /* CREATE_ALWAYS */
#ifdef _WIN32
    return rt_io_open(path, GENERIC_READ | GENERIC_WRITE, CREATE_ALWAYS);
#else
    return open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
#endif
}

/* 只读句柄（只读文件回退：GENERIC_READ|GENERIC_WRITE 打开只读文件会拒访）。 */
static rt_io_handle_t rt_io_open_ro(const char* path) {
#ifdef _WIN32
    return rt_io_open(path, GENERIC_READ, OPEN_EXISTING);
#else
    return open(path, O_RDONLY);
#endif
}

/* 将文件截断到 len（写路径兜底）；返回 0 成功 / -1 失败。
 * 截断后把文件指针复位到 0（写路径随后从头写入）。 */
static int rt_io_truncate(rt_io_handle_t h, int64_t len) {
#ifdef _WIN32
    LARGE_INTEGER pos;
    pos.QuadPart = len;
    if (!(SetFilePointerEx(h, pos, NULL, FILE_BEGIN) && SetEndOfFile(h))) return -1;
    pos.QuadPart = 0;
    return SetFilePointerEx(h, pos, NULL, FILE_BEGIN) ? 0 : -1;
#else
    return ftruncate(h, (off_t)len);
#endif
}

/* 读满 n 字节（容忍短读）；返回实际读到的字节数。 */
static size_t rt_io_read_full(rt_io_handle_t h, void* buf, size_t n) {
    char* p = (char*)buf;
    size_t got = 0;
    while (got < n) {
        ssize_t r = rt_io_read(h, p + got, n - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    return got;
}

/* 写满 n 字节（容忍短写）；返回 0 成功 / -1 失败。 */
static int rt_io_write_full(rt_io_handle_t h, const void* buf, size_t n) {
    const char* p = (const char*)buf;
    size_t done = 0;
    while (done < n) {
        ssize_t w = rt_io_write(h, p + done, n - done);
        if (w <= 0) return -1;
        done += (size_t)w;
    }
    return 0;
}

/* 将文件指针定位到文件开头（读/写复用句柄前必做）。返回 0 成功 / -1 失败。 */
static int rt_io_seek_begin(rt_io_handle_t h, int64_t off) {
#ifdef _WIN32
    LARGE_INTEGER pos;
    pos.QuadPart = off;
    return SetFilePointerEx(h, pos, NULL, FILE_BEGIN) ? 0 : -1;
#else
    return lseek(h, (off_t)off, SEEK_SET) < 0 ? -1 : 0;
#endif
}

/* 空串失败值（malloc(1) 的 '\0'；无法分配时返回 NULL）。 */
static char* rt_io_malloc_empty(void) {
    char* e = (char*)malloc(1);
    if (e) e[0] = '\0';
    return e;
}

/* ---- TLS 单槽句柄缓存（RFC 009 下钻）----
 *
 * 目标：同路径反复 read/write 时复用句柄，消除每次 open/close 的 CreateFile
 * syscall（本机 AV 扫描下 CreateFile 单次 ~41-97µs，占 file_io 总耗时 >80%）。
 *
 * 安全设计：
 *   - 单槽、per-thread（TLS）：天然无跨线程共享，无锁，无生命周期竞态。
 *   - 以路径字符串精确匹配：路径变更即冲刷旧句柄，杜绝串文件。
 *   - 模式分离：RW 句柄可服务读写；RO 句柄仅服务读（只读文件回退），写不复用 RO。
 *   - 内嵌路径副本（免堆分配，杜绝 OOM 分支）：超长路径（>=RT_IO_CACHE_PATH_MAX）
 *     不缓存，回退逐次 open/close（仍正确，仅失去缓存加速）。
 *   - 写路径以 cached known_len 做条件裁剪（new_len < known_len 才 SetEndOfFile），
 *     同尺寸覆写（基准主模式）零额外元数据 syscall。
 *   - 已知限制：句柄不感知外部进程对同一路径的并发修改；每线程至多滞留一个句柄
 *     （程序生命周期内有效，进程退出时由 OS 关闭）。删除/移动路径会冲刷缓存。 */
#define RT_IO_CACHE_PATH_MAX 1024

typedef struct rt_io_tls_cache {
    rt_io_handle_t h;
    int            mode;      /* 0=RO，1=RW */
    int            populated;
    int64_t        known_len; /* RW 句柄最近一次写入后的逻辑长度（RO 不使用） */
    unsigned       epoch;     /* 缓存该句柄时的全局失效纪元（见 g_io_cache_epoch） */
    uint64_t       file_id;   /* 打开时文件身份（Windows 卷序列号+文件索引 / POSIX dev+ino） */
    int            has_id;    /* file_id 是否可判定 */
    char           path[RT_IO_CACHE_PATH_MAX];
} rt_io_tls_cache;

#ifdef _WIN32
static __declspec(thread) rt_io_tls_cache g_tls_io_cache = {0};
#else
static _Thread_local rt_io_tls_cache g_tls_io_cache = {0};
#endif

/* 全局缓存失效纪元：删除/移动使路径身份改变——任意线程 TLS 中缓存的句柄都可能指向
 * 已卸载的旧 inode（rename 替换后读到旧内容）。删除/移动时全局递增纪元；各线程缓存
 * 记录其缓存时的纪元，lookup 遇纪元不一致即失效重开（跨线程缓存一致性，正确性兜底）。
 * 热路径仅一次 relaxed atomic load（x86 约 1 周期，远低于其所门控的 syscall）。 */
static atomic_uint g_io_cache_epoch = 0;

/* 查询句柄的文件身份（Windows 卷序列号+文件索引 / POSIX dev+ino）。
 * rename 替换（外部进程原子写，如 git checkout/merge 恢复）后身份变化——
 * TLS 缓存据此识别「句柄已指向被替换的旧 inode」。身份不可判定返回 0、has=0。 */
static uint64_t rt_io_file_id(rt_io_handle_t h, int* out_has) {
    *out_has = 0;
#ifdef _WIN32
    BY_HANDLE_FILE_INFORMATION info;
    if (GetFileInformationByHandle(h, &info)) {
        *out_has = 1;
        return ((uint64_t)info.dwVolumeSerialNumber << 32)
             | ((uint64_t)info.nFileIndexHigh << 16) | info.nFileIndexLow;
    }
#else
    struct stat st;
    if (fstat(h, &st) == 0) {
        *out_has = 1;
        return ((uint64_t)st.st_dev << 32) | (uint64_t)st.st_ino;
    }
#endif
    return 0;
}

/* 冲刷缓存：关闭缓存的句柄并清空。删除/移动等使路径失效的操作调用它。 */
static void rt_io_cache_flush(void) {
    rt_io_tls_cache* c = &g_tls_io_cache;
    if (c->populated) {
        if (c->h != RT_IO_HANDLE_INVALID) rt_io_close(c->h);
        c->h = RT_IO_HANDLE_INVALID;
        c->mode = 0;
        c->populated = 0;
    }
}

/* 查找缓存。命中返回 1 并出参句柄；need_write=1 时仅 RW 句柄可用。
 * 纪元不一致（期间发生过删除/移动）→ 冲刷本线程句柄并按未命中处理（跨线程正确性）。
 * 2026-08-21（CD-34 根因）：外部进程（git 等）以 rename 替换同一路径文件（原子写）时，
 * 缓存句柄仍指向旧 inode——每命中校验文件身份（卷+索引 / dev+ino），不一致即冲刷重开
 * （「句柄不感知外部进程并发修改」的既有已知限制的兜底）。 */
static int rt_io_cache_lookup(const char* path, int need_write, rt_io_handle_t* out) {
    rt_io_tls_cache* c = &g_tls_io_cache;
    if (!c->populated) return 0;
    if (atomic_load_explicit(&g_io_cache_epoch, memory_order_relaxed) != c->epoch) {
        rt_io_cache_flush();
        return 0;
    }
    if (need_write && c->mode != 1) return 0;
    if (strcmp(c->path, path) == 0) {
        if (c->has_id) {
            int has = 0;
            uint64_t id = rt_io_file_id(c->h, &has);
            if (has && id != c->file_id) {
                rt_io_cache_flush();
                return 0;
            }
        }
        *out = c->h;
        return 1;
    }
    return 0;
}

/* 填充缓存。路径超长（>=RT_IO_CACHE_PATH_MAX）返回 0（不缓存，调用方自行管理句柄）。 */
static int rt_io_cache_store(const char* path, rt_io_handle_t h, int mode, int64_t known_len) {
    rt_io_tls_cache* c = &g_tls_io_cache;
    size_t n = strlen(path);
    if (n >= RT_IO_CACHE_PATH_MAX) return 0;
    if (!(c->populated && strcmp(c->path, path) == 0)) {
        rt_io_cache_flush();
        memcpy(c->path, path, n + 1);
    }
    c->h = h;
    c->mode = mode;
    c->known_len = known_len;
    c->epoch = atomic_load_explicit(&g_io_cache_epoch, memory_order_relaxed);
    c->file_id = rt_io_file_id(h, &c->has_id);
    c->populated = 1;
    return 1;
}

/* 在句柄 h 上执行覆写：seek0 + 条件裁剪(known_len>len) + 写 len 字节。
 * 成功后回写 *known_len = len。返回 1 成功 / 0 失败。
 * 复用句柄时 known_len 提供「上次逻辑长度」，同尺寸覆写可省 SetEndOfFile。 */
static int rt_io_overwrite(rt_io_handle_t h, int64_t* known_len,
                           const char* content, size_t len) {
    if (rt_io_seek_begin(h, 0) != 0) return 0;
    if (*known_len > (int64_t)len && rt_io_truncate(h, (int64_t)len) != 0) return 0;
    int ok = (len == 0) || (rt_io_write_full(h, content, len) == 0);
    if (ok) *known_len = (int64_t)len;
    return ok;
}

/// 读取文本文件全部内容。失败返回空串（malloc(1) 的 '\0'）。
char* rt_read_file(const char* path) {
    if (!path) return rt_io_malloc_empty();
    /* 2026-08-21（CD-34 根因链）：读路径**绕过 TLS 句柄缓存**——缓存句柄不感知
     * 外部进程（git 等）对同一路径的替换/改写：rename 替换后旧 inode 句柄读到过期
     * 内容；NTFS 删除重建可能复用文件索引，file-id 校验也无法区分。读正确性优先于
     * 缓存收益（每次 open/close ~50-100µs；写路径缓存保留，读写复用场景由写侧
     * known_len/file-id 维持）。 */
    rt_io_handle_t h;
    int mode = 1;
    h = rt_io_open_rw(path);
    if (h == RT_IO_HANDLE_INVALID) {
        h = rt_io_open_ro(path);
        mode = 0;
        if (h == RT_IO_HANDLE_INVALID) return rt_io_malloc_empty();
    }
    int64_t size = 0;
    if (rt_io_fstat_size(h, &size) != 0 || size < 0) { rt_io_close(h); return rt_io_malloc_empty(); }
    if (rt_io_seek_begin(h, 0) != 0) { rt_io_close(h); return rt_io_malloc_empty(); }
    char* out = (char*)malloc((size_t)size + 1);
    if (!out) { rt_io_close(h); return NULL; }
    size_t read = rt_io_read_full(h, out, (size_t)size);
    out[read] = '\0';
    rt_io_close(h);
    return out;
}

/// 写入文本到文件（覆盖模式）。成功返回 1，失败返回 0。
int32_t rt_write_file(const char* path, const char* content) {
    if (!path) return 0;
    if (!content) content = "";
    size_t len = strlen(content);
    rt_io_handle_t h;
    if (rt_io_cache_lookup(path, 1, &h)) {
        return rt_io_overwrite(h, &g_tls_io_cache.known_len, content, len);
    }
    int owned = 1;
    h = rt_io_open_rw(path);
    if (h == RT_IO_HANDLE_INVALID) {
        /* 文件可能不存在：回退创建/截断打开 */
        h = rt_io_open_rw_create(path);
        if (h == RT_IO_HANDLE_INVALID) return 0;
        if (!rt_io_cache_store(path, h, 1, 0)) owned = 0; /* CREATE_ALWAYS 已截断为 0 */
    } else {
        int64_t old_size = 0;
        if (rt_io_fstat_size(h, &old_size) != 0) { rt_io_close(h); return 0; }
        if (!rt_io_cache_store(path, h, 1, old_size)) owned = 0;
    }
    if (owned) return rt_io_overwrite(h, &g_tls_io_cache.known_len, content, len);
    /* 路径超长未缓存：临时句柄覆写后关闭。 */
    int64_t tmp_len = 0;
    if (rt_io_fstat_size(h, &tmp_len) != 0) { rt_io_close(h); return 0; }
    int ok = rt_io_overwrite(h, &tmp_len, content, len);
    rt_io_close(h);
    return ok;
}

/* ---- M1: 基础文件操作 ---- */

/// 判断文件是否存在。
int32_t rt_file_exists(const char* path) {
    if (!path) return 0;
    struct stat st;
    if (stat(path, &st) != 0) return 0;
    /* S_IFREG: 常规文件（非目录/设备/符号链接） */
    return (st.st_mode & S_IFMT) == S_IFREG ? 1 : 0;
}

/// 删除文件。成功返回 1，文件不存在或失败返回 0。
int32_t rt_file_delete(const char* path) {
    if (!path) return 0;
    /* 删除使路径身份失效：本线程句柄立即冲刷；**全局**递增纪元使其它线程 TLS 中缓存的
     * 句柄（指向将被卸载的旧 inode）在下次 lookup 时失效重开——跨线程缓存一致性。 */
    rt_io_cache_flush();
    atomic_fetch_add_explicit(&g_io_cache_epoch, 1, memory_order_relaxed);
    return remove(path) == 0 ? 1 : 0;
}

/// 追加文本到文件（不存在则创建）。成功返回 1，失败返回 0。
int32_t rt_file_append(const char* path, const char* content) {
    if (!path) return 0;
    FILE* f = fopen(path, "ab");
    if (!f) return 0;
    if (content) fputs(content, f);
    fclose(f);
    return 1;
}

/// 复制文件。成功返回 1，失败（源不存在/无法读/无法写）返回 0。
int32_t rt_file_copy(const char* src, const char* dst) {
    if (!src || !dst) return 0;
    FILE* in = fopen(src, "rb");
    if (!in) return 0;
    FILE* out = fopen(dst, "wb");
    if (!out) {
        fclose(in);
        return 0;
    }
    char buf[4096];
    size_t n;
    int ok = 1;
    while ((n = fread(buf, 1, sizeof(buf), in)) > 0) {
        if (fwrite(buf, 1, n, out) != n) {
            ok = 0;
            break;
        }
    }
    fclose(in);
    fclose(out);
    return ok;
}

/// 移动/重命名文件。成功返回 1，失败返回 0。
int32_t rt_file_move(const char* src, const char* dst) {
    if (!src || !dst) return 0;
    /* 重命名改变路径身份：本线程句柄立即冲刷；**全局**递增纪元使其它线程 TLS 缓存
     * （指向旧路径/旧 inode 的句柄）失效——rename 替换后读到新内容，杜绝串文件。 */
    rt_io_cache_flush();
    atomic_fetch_add_explicit(&g_io_cache_epoch, 1, memory_order_relaxed);
    /* rename 跨卷失败时退化为 copy + delete */
    if (rename(src, dst) == 0) return 1;
    if (!rt_file_copy(src, dst)) return 0;
    rt_file_delete(src);
    return 1;
}

/* ---- M3: 目录操作 ---- */

/// 创建目录（单层；不递归创建父目录）。成功返回 1，已存在或失败返回 0。
int32_t rt_dir_create(const char* path) {
    if (!path) return 0;
    if (RT_MKDIR(path) == 0) return 1;
    /* EEXIST：目录已存在视为成功 */
    if (errno == EEXIST) return 1;
    return 0;
}

/// 判断目录是否存在。
int32_t rt_dir_exists(const char* path) {
    if (!path) return 0;
    struct stat st;
    if (stat(path, &st) != 0) return 0;
    return (st.st_mode & S_IFMT) == S_IFDIR ? 1 : 0;
}

/// 删除空目录。成功返回 1，非空或失败返回 0。
int32_t rt_dir_delete(const char* path) {
    if (!path) return 0;
    return RT_RMDIR(path) == 0 ? 1 : 0;
}

/* forward：本文件后文定义；GetFiles 填完整路径时复用 */
char* rt_path_combine(const char* a, const char* b);

static void* rt_dir_empty_files(void) {
    return rt_array_create(0, (int32_t)sizeof(char*));
}

static void rt_dir_array_set_len(void* payload, int32_t len) {
    if (!payload) return;
    /* RtArrayHeader.length 紧挨 payload 前 8 字节之首字段 */
    int32_t* length_field = (int32_t*)((char*)payload - sizeof(int32_t) * 2);
    *length_field = len;
}

static int rt_dir_is_dot_or_dotdot(const char* name) {
    return name && (strcmp(name, ".") == 0 || strcmp(name, "..") == 0);
}

/// Directory.GetFiles(path, searchPattern)：非递归；* / ? 在 C 侧匹配（非 codegen filter）。
/// 失败 / 空 pattern / 无匹配 → Length 0。
void* rt_dir_list_files_pattern(const char* path, const char* search_pattern) {
    if (!path || !search_pattern || search_pattern[0] == '\0') {
        return rt_dir_empty_files();
    }

#ifdef _WIN32
    char pattern[4096];
    int plen = (int)strlen(path);
    int spatlen = (int)strlen(search_pattern);
    if (plen <= 0 || spatlen <= 0 || plen + spatlen + 2 >= (int)sizeof(pattern)) {
        return rt_dir_empty_files();
    }
    memcpy(pattern, path, (size_t)plen);
    int pos = plen;
    if (path[plen - 1] != '\\' && path[plen - 1] != '/') {
        pattern[pos++] = '\\';
    }
    memcpy(pattern + pos, search_pattern, (size_t)spatlen);
    pattern[pos + spatlen] = '\0';

    struct _finddata_t fd;
    intptr_t handle = _findfirst(pattern, &fd);
    if (handle == -1) return rt_dir_empty_files();

    int32_t count = 0;
    do {
        if (!(fd.attrib & _A_SUBDIR)) count++;
    } while (_findnext(handle, &fd) == 0);
    _findclose(handle);

    if (count == 0) return rt_dir_empty_files();

    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) return rt_dir_empty_files();

    handle = _findfirst(pattern, &fd);
    int32_t idx = 0;
    if (handle != -1) {
        do {
            if (!(fd.attrib & _A_SUBDIR) && idx < count) {
                char* full = rt_path_combine(path, fd.name);
                if (full) {
                    ((char**)arr)[idx++] = full;
                }
            }
        } while (_findnext(handle, &fd) == 0);
        _findclose(handle);
    }
    if (idx != count) rt_dir_array_set_len(arr, idx);
    return arr;
#else
    DIR* dir = opendir(path);
    if (!dir) return rt_dir_empty_files();

    int32_t count = 0;
    struct dirent* entry;
    while ((entry = readdir(dir)) != NULL) {
        if (entry->d_type == DT_REG
            && fnmatch(search_pattern, entry->d_name, 0) == 0) {
            count++;
        }
    }
    rewinddir(dir);

    if (count == 0) {
        closedir(dir);
        return rt_dir_empty_files();
    }

    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) {
        closedir(dir);
        return rt_dir_empty_files();
    }

    int32_t idx = 0;
    while ((entry = readdir(dir)) != NULL && idx < count) {
        if (entry->d_type == DT_REG
            && fnmatch(search_pattern, entry->d_name, 0) == 0) {
            char* full = rt_path_combine(path, entry->d_name);
            if (full) {
                ((char**)arr)[idx++] = full;
            }
        }
    }
    closedir(dir);
    if (idx != count) rt_dir_array_set_len(arr, idx);
    return arr;
#endif
}

/// Directory.GetFiles(path)：等价于 searchPattern = "*"。
void* rt_dir_list_files(const char* path) {
    return rt_dir_list_files_pattern(path, "*");
}

/// Directory.GetDirectories(path)：非递归直接子目录；跳过 . / ..。
void* rt_dir_list_dirs(const char* path) {
    if (!path) return rt_dir_empty_files();

#ifdef _WIN32
    char pattern[4096];
    int plen = (int)strlen(path);
    if (plen <= 0 || plen >= (int)sizeof(pattern) - 3) return rt_dir_empty_files();
    memcpy(pattern, path, (size_t)plen);
    int pos = plen;
    if (path[plen - 1] != '\\' && path[plen - 1] != '/') {
        pattern[pos++] = '\\';
    }
    pattern[pos++] = '*';
    pattern[pos] = '\0';

    struct _finddata_t fd;
    intptr_t handle = _findfirst(pattern, &fd);
    if (handle == -1) return rt_dir_empty_files();

    int32_t count = 0;
    do {
        if ((fd.attrib & _A_SUBDIR) && !rt_dir_is_dot_or_dotdot(fd.name)) count++;
    } while (_findnext(handle, &fd) == 0);
    _findclose(handle);

    if (count == 0) return rt_dir_empty_files();

    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) return rt_dir_empty_files();

    handle = _findfirst(pattern, &fd);
    int32_t idx = 0;
    if (handle != -1) {
        do {
            if ((fd.attrib & _A_SUBDIR) && !rt_dir_is_dot_or_dotdot(fd.name) && idx < count) {
                char* full = rt_path_combine(path, fd.name);
                if (full) {
                    ((char**)arr)[idx++] = full;
                }
            }
        } while (_findnext(handle, &fd) == 0);
        _findclose(handle);
    }
    if (idx != count) rt_dir_array_set_len(arr, idx);
    return arr;
#else
    DIR* dir = opendir(path);
    if (!dir) return rt_dir_empty_files();

    int32_t count = 0;
    struct dirent* entry;
    while ((entry = readdir(dir)) != NULL) {
        if (entry->d_type == DT_DIR && !rt_dir_is_dot_or_dotdot(entry->d_name)) {
            count++;
        }
    }
    rewinddir(dir);

    if (count == 0) {
        closedir(dir);
        return rt_dir_empty_files();
    }

    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) {
        closedir(dir);
        return rt_dir_empty_files();
    }

    int32_t idx = 0;
    while ((entry = readdir(dir)) != NULL && idx < count) {
        if (entry->d_type == DT_DIR && !rt_dir_is_dot_or_dotdot(entry->d_name)) {
            char* full = rt_path_combine(path, entry->d_name);
            if (full) {
                ((char**)arr)[idx++] = full;
            }
        }
    }
    closedir(dir);
    if (idx != count) rt_dir_array_set_len(arr, idx);
    return arr;
#endif
}

/* ---- M3: 路径操作（纯字符串计算） ---- */

/* 分配 len+1 字节并写入 NUL 终止符 */
static char* rt_path_dup_n(const char* s, size_t len) {
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    if (s && len) memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

/// 路径拼接：a/b。智能处理分隔符（避免双斜杠）。
/// 例：Combine("/foo", "bar") → "/foo/bar"；Combine("/foo/", "bar") → "/foo/bar"
char* rt_path_combine(const char* a, const char* b) {
    if (!a) a = "";
    if (!b) b = "";
    size_t la = strlen(a);
    size_t lb = strlen(b);
    if (la == 0) return rt_path_dup_n(b, lb);
    if (lb == 0) return rt_path_dup_n(a, la);

    char last = a[la - 1];
    char first = b[0];
    int need_sep = (last != '/' && last != '\\') && (first != '/' && first != '\\');

    size_t total = la + (need_sep ? 1 : 0) + lb;
    char* out = (char*)malloc(total + 1);
    if (!out) return NULL;
    memcpy(out, a, la);
    size_t pos = la;
    if (need_sep) out[pos++] = '/';
    memcpy(out + pos, b, lb);
    pos += lb;
    out[pos] = '\0';
    return out;
}

/// 获取目录名：path 去掉最后一段文件名后的部分。
/// 例：GetDirectoryName("/foo/bar.txt") → "/foo"；GetDirectoryName("bar.txt") → ""
char* rt_path_get_dir_name(const char* path) {
    if (!path) return rt_path_dup_n("", 0);
    size_t len = strlen(path);
    if (len == 0) return rt_path_dup_n("", 0);

    /* 从尾部向前查找最后一个路径分隔符 */
    ssize_t i = (ssize_t)len - 1;
    /* 跳过尾部连续分隔符（如 "/foo/bar//"） */
    while (i >= 0 && (path[i] == '/' || path[i] == '\\')) i--;
    /* 查找前一个分隔符 */
    while (i >= 0 && path[i] != '/' && path[i] != '\\') i--;
    if (i < 0) return rt_path_dup_n("", 0);
    return rt_path_dup_n(path, (size_t)i);
}

/// 获取文件名（含扩展名）。
/// 例：GetFileName("/foo/bar.txt") → "bar.txt"；GetFileName("/foo/") → ""
/// C# 语义：若路径以分隔符结尾，返回空串（表示目录而非文件）。
char* rt_path_get_file_name(const char* path) {
    if (!path) return rt_path_dup_n("", 0);
    size_t len = strlen(path);
    if (len == 0) return rt_path_dup_n("", 0);

    /* 若路径以分隔符结尾，返回空串（C# System.IO.Path.GetFileName 语义） */
    char last = path[len - 1];
    if (last == '/' || last == '\\') return rt_path_dup_n("", 0);

    /* 从尾部向前查找最后一个分隔符 */
    ssize_t i = (ssize_t)len - 1;
    while (i >= 0 && path[i] != '/' && path[i] != '\\') i--;
    return rt_path_dup_n(path + i + 1, len - (size_t)i - 1);
}

/// 获取扩展名（含前导点）。
/// 例：GetExtension("foo.txt") → ".txt"；GetExtension("foo") → ""；GetExtension("foo.") → "."
char* rt_path_get_extension(const char* path) {
    if (!path) return rt_path_dup_n("", 0);
    size_t len = strlen(path);
    if (len == 0) return rt_path_dup_n("", 0);

    /* 从尾部向前查找最后一个点 */
    ssize_t i = (ssize_t)len - 1;
    while (i >= 0 && path[i] != '.' && path[i] != '/' && path[i] != '\\') i--;
    if (i < 0 || path[i] != '.') return rt_path_dup_n("", 0);
    return rt_path_dup_n(path + i, len - (size_t)i);
}

/// 获取不含扩展名的文件名。
/// 例：GetFileNameWithoutExtension("/tmp/test.txt") → "test"；GetFileNameWithoutExtension("foo") → "foo"
char* rt_path_get_file_name_without_ext(const char* path) {
    if (!path) return rt_path_dup_n("", 0);
    size_t len = strlen(path);
    if (len == 0) return rt_path_dup_n("", 0);

    /* 先取文件名（最后一个路径分隔符之后的部分） */
    ssize_t i = (ssize_t)len - 1;
    while (i >= 0 && path[i] != '/' && path[i] != '\\') i--;
    size_t name_start = (size_t)i + 1;
    size_t name_len = len - name_start;
    if (name_len == 0) return rt_path_dup_n("", 0);

    /* 在文件名范围内从尾部查找最后一个点 */
    ssize_t j = (ssize_t)len - 1;
    while (j >= (ssize_t)name_start && path[j] != '.') j--;
    if (j < (ssize_t)name_start) {
        /* 无扩展名，返回完整文件名 */
        return rt_path_dup_n(path + name_start, name_len);
    }
    return rt_path_dup_n(path + name_start, (size_t)j - name_start);
}

/// 更换扩展名（对齐 C# Path.ChangeExtension）。
/// ext 为 NULL/"" 时去掉扩展名；ext 不以 '.' 开头时自动补 '.'。
/// 例：ChangeExtension("a.txt", ".md") → "a.md"；ChangeExtension("a.txt", "") → "a"
char* rt_path_change_extension(const char* path, const char* ext) {
    if (!path) path = "";
    size_t len = strlen(path);

    /* 文件名范围内最后一个 '.' 的位置；无则 append */
    ssize_t i = (ssize_t)len - 1;
    while (i >= 0 && path[i] != '/' && path[i] != '\\' && path[i] != '.') i--;
    size_t stem_len = len;
    if (i >= 0 && path[i] == '.') {
        stem_len = (size_t)i;
    }

    if (!ext || ext[0] == '\0') {
        return rt_path_dup_n(path, stem_len);
    }

    int need_dot = (ext[0] != '.');
    size_t elen = strlen(ext);
    size_t out_len = stem_len + (need_dot ? 1 : 0) + elen;
    char* out = (char*)malloc(out_len + 1);
    if (!out) return NULL;
    if (stem_len) memcpy(out, path, stem_len);
    size_t o = stem_len;
    if (need_dot) out[o++] = '.';
    memcpy(out + o, ext, elen);
    out[out_len] = '\0';
    return out;
}

/// 路径是否含扩展名（含仅 "."）。对齐 C# Path.HasExtension。
int32_t rt_path_has_extension(const char* path) {
    char* ext = rt_path_get_extension(path);
    int32_t ok = (ext && ext[0] != '\0') ? 1 : 0;
    free(ext);
    return ok;
}

/// 读取文件全部字节为 `byte[]`（rt_array payload，elem_size=1）。
/// 失败或空文件返回 Length 0 的数组（非 NULL）。
void* rt_file_read_all_bytes(const char* path) {
    if (!path) {
        return rt_array_create(0, 1);
    }
    FILE* f = fopen(path, "rb");
    if (!f) {
        return rt_array_create(0, 1);
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return rt_array_create(0, 1);
    }
    long size = ftell(f);
    if (size < 0) {
        fclose(f);
        return rt_array_create(0, 1);
    }
    if (fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return rt_array_create(0, 1);
    }
    void* arr = rt_array_create((int32_t)size, 1);
    if (!arr) {
        fclose(f);
        return NULL;
    }
    if (size > 0) {
        size_t n = fread(arr, 1, (size_t)size, f);
        if ((long)n != size) {
            /* 短读：缩为实际长度（重建） */
            void* trimmed = rt_array_create((int32_t)n, 1);
            if (trimmed && n > 0) {
                memcpy(trimmed, arr, n);
            }
            fclose(f);
            return trimmed ? trimmed : arr;
        }
    }
    fclose(f);
    return arr;
}

/// 将 `byte[]` 覆盖写入文件。成功返回 1，失败返回 0。
int32_t rt_file_write_all_bytes(const char* path, void* bytes) {
    if (!path) return 0;
    FILE* f = fopen(path, "wb");
    if (!f) return 0;
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    if (len > 0 && bytes) {
        size_t n = fwrite(bytes, 1, (size_t)len, f);
        if ((int32_t)n != len) {
            fclose(f);
            return 0;
        }
    }
    fclose(f);
    return 1;
}

static char* rt_file_dup_n(const char* s, size_t len) {
    char* out = (char*)malloc(len + 1);
    if (!out) return NULL;
    if (len) memcpy(out, s, len);
    out[len] = '\0';
    return out;
}

/// 读取全部行 → `string[]`（对齐 C# ReadAllLines：识别 `\r\n`/`\n`/`\r`；
/// 文件以换行结尾不产生尾部空行；失败 → Length 0）。
void* rt_file_read_all_lines(const char* path) {
    char* text = rt_read_file(path);
    if (!text) {
        return rt_array_create(0, (int32_t)sizeof(char*));
    }
    size_t len = strlen(text);
    /* First pass: count lines */
    int32_t count = 0;
    size_t i = 0;
    if (len == 0) {
        free(text);
        return rt_array_create(0, (int32_t)sizeof(char*));
    }
    while (i < len) {
        count++;
        while (i < len && text[i] != '\n' && text[i] != '\r') i++;
        if (i < len) {
            if (text[i] == '\r' && i + 1 < len && text[i + 1] == '\n') i += 2;
            else i += 1;
            /* Trailing newline: do not count an extra empty line */
            if (i >= len) break;
        }
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) {
        free(text);
        return NULL;
    }
    char** items = (char**)arr;
    int32_t idx = 0;
    i = 0;
    while (i < len && idx < count) {
        size_t start = i;
        while (i < len && text[i] != '\n' && text[i] != '\r') i++;
        items[idx++] = rt_file_dup_n(text + start, i - start);
        if (i < len) {
            if (text[i] == '\r' && i + 1 < len && text[i + 1] == '\n') i += 2;
            else i += 1;
            if (i >= len) break;
        }
    }
    free(text);
    return arr;
}

/// 系统临时目录路径；始终带尾部目录分隔符（对齐 C# Path.GetTempPath）。
char* rt_path_get_temp_path(void) {
#ifdef _WIN32
    char buf[MAX_PATH + 2];
    DWORD n = GetTempPathA(MAX_PATH, buf);
    if (n == 0 || n > MAX_PATH) {
        return rt_file_dup_n(".\\", 2);
    }
    return rt_file_dup_n(buf, (size_t)n);
#else
    const char* t = getenv("TMPDIR");
    if (!t || !*t) t = "/tmp";
    size_t len = strlen(t);
    int need_sep = (len == 0 || t[len - 1] != '/') ? 1 : 0;
    char* out = (char*)malloc(len + (size_t)need_sep + 1);
    if (!out) return NULL;
    memcpy(out, t, len);
    if (need_sep) out[len++] = '/';
    out[len] = '\0';
    return out;
#endif
}

/* ------------------------------------------------------------------ */
/* File.*Async 真异步（RFC 009 M2 / RFC 009 异步为主 · 缺漏纠正）       */
/* ------------------------------------------------------------------ */
/*
 * 数据面文件 I/O（read_all_text / read_all_bytes / read_all_lines /
 * write_all_text / write_all_bytes / append_all_text）从「线程池包装同步
 * ABI（async-over-sync）」纠正为 **Reactor 真异步**：以 FILE_FLAG_OVERLAPPED
 * （IOCP）/ io_uring pread/pwrite 直接在 OS 非阻塞原语上完成，不占用线程池
 * worker，与网络 async 同构。完成事件经 EventLoop tick → rt_io_completion_complete
 * 分发到本文件的 rt_file_io_completion_complete。
 *
 * 元数据操作（exists / delete / move / copy）与目录操作（create/exists/delete/
 * list）为短耗时元数据 / 目录枚举，OS 无对应异步原语，**保留线程池包装**，
 * 见 rt_task_run.c——诚实标注，非隐藏回退。
 *
 * 完成上下文：以 RtIoCompletion 为 embedded base 的 RtFileIoCompletion。
 * 句柄 int32 fd 截断与 rt_net.c 一致（Windows 句柄值在进程句柄表内通常 <2^31）。
 */

typedef struct RtFileIoCompletion {
    RtIoCompletion base;      /* task / op_type / buf / buf_size */
    rt_io_handle_t handle;    /* 打开的 async 句柄（完成后关闭） */
} RtFileIoCompletion;

#ifdef _WIN32
/* 异步句柄：必须带 FILE_FLAG_OVERLAPPED 才能走 IOCP / ReadFile 重叠 I/O。 */
static rt_io_handle_t rt_io_open_async(const char* path, DWORD access,
                                       DWORD disposition) {
    DWORD flags = FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OVERLAPPED;
    if (rt_io_ascii_only(path)) {
        return CreateFileA(path, access, RT_IO_SHARE, NULL, disposition, flags, NULL);
    }
    int n = MultiByteToWideChar(CP_UTF8, 0, path, -1, NULL, 0);
    if (n <= 0) return INVALID_HANDLE_VALUE;
    wchar_t* w = (wchar_t*)malloc((size_t)n * sizeof(wchar_t));
    if (!w) return INVALID_HANDLE_VALUE;
    MultiByteToWideChar(CP_UTF8, 0, path, -1, w, n);
    HANDLE h = CreateFileW(w, access, RT_IO_SHARE, NULL, disposition, flags, NULL);
    free(w);
    return h;
}
#else
static int rt_io_open_async_ro(const char* path) { return open(path, O_RDONLY); }
static int rt_io_open_async_w(const char* path, int append) {
    /* 写：O_TRUNC 保证覆写裁剪到 0（对齐同步 WriteAll 语义）。 */
    if (append) return open(path, O_WRONLY | O_CREAT | O_APPEND, 0644);
    return open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
}
#endif

static int32_t rt_io_handle_to_fd(rt_io_handle_t h) {
#ifdef _WIN32
    return (int32_t)(intptr_t)h;
#else
    return (int32_t)h;
#endif
}

/* 追加写偏移：Windows FILE_APPEND_DATA 句柄忽略 offset；POSIX io_uring 用
 * off=-1（当前文件位置，O_APPEND 追加）。 */
static uint64_t rt_io_append_offset(void) {
#ifdef _WIN32
    return 0;
#else
    return (uint64_t)-1;
#endif
}

/* 设置数组 length 字段（RtArrayHeader{length, elem_size} 在 payload 前 8 字节）。 */
static void rt_file_array_set_len(void* payload, int32_t len) {
    if (!payload) return;
    int32_t* length_field = (int32_t*)((char*)payload - sizeof(int32_t) * 2);
    *length_field = len;
}

/* 从缓存区解析全部行 → string[]（对齐 C# ReadAllLines；失败/空 → Length 0）。 */
static void* rt_file_lines_from_buf(const char* text, size_t len) {
    int32_t count = 0;
    size_t i = 0;
    if (len == 0) return rt_array_create(0, (int32_t)sizeof(char*));
    while (i < len) {
        count++;
        while (i < len && text[i] != '\n' && text[i] != '\r') i++;
        if (i < len) {
            if (text[i] == '\r' && i + 1 < len && text[i + 1] == '\n') i += 2;
            else i += 1;
            /* 尾部换行不产生额外空行 */
            if (i >= len) break;
        }
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) return NULL;
    char** items = (char**)arr;
    int32_t idx = 0;
    i = 0;
    while (i < len && idx < count) {
        size_t start = i;
        while (i < len && text[i] != '\n' && text[i] != '\r') i++;
        items[idx++] = rt_file_dup_n(text + start, i - start);
        if (i < len) {
            if (text[i] == '\r' && i + 1 < len && text[i + 1] == '\n') i += 2;
            else i += 1;
            if (i >= len) break;
        }
    }
    return arr;
}

/* 读取类共享提交：打开只读 async 句柄 → fstat 尺寸 → 分配缓冲 → 提交 read。 */
static void* rt_file_read_async(const char* path, RtIoOpType op_type) {
    if (!path) return NULL;
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) return NULL;

    rt_io_handle_t h;
#ifdef _WIN32
    h = rt_io_open_async(path, GENERIC_READ, OPEN_EXISTING);
#else
    h = rt_io_open_async_ro(path);
#endif
    if (h == RT_IO_HANDLE_INVALID) return NULL;
    int64_t size = 0;
    if (rt_io_fstat_size(h, &size) != 0 || size < 0) { rt_io_close(h); return NULL; }

    RtTask* task = rt_task_alloc();
    if (!task) { rt_io_close(h); return NULL; }
    task->status = RT_TASK_PENDING;

    RtFileIoCompletion* f = (RtFileIoCompletion*)calloc(1, sizeof(RtFileIoCompletion));
    if (!f) { rt_task_release(task); rt_io_close(h); return NULL; }
    f->base.task = task;
    f->base.op_type = op_type;
    f->handle = h;

    uint32_t cap = (uint32_t)(size > 0 ? size : 1);
    if (op_type == RT_IO_OP_FILE_READ_BYTES) {
        /* 直接以 byte[] payload 作读缓冲，完成后仅设长度，免拷贝 */
        f->base.buf = rt_array_create((int32_t)cap, 1);
    } else {
        /* text / lines：+1 供 NUL 终止 */
        f->base.buf = (char*)calloc(1, (size_t)cap + 1);
    }
    if (!f->base.buf) { free(f); rt_task_release(task); rt_io_close(h); return NULL; }
    f->base.buf_size = (int32_t)cap;

    int32_t fd = rt_io_handle_to_fd(h);
    rt_reactor_register(reactor, fd, 0);
    int32_t rc = rt_reactor_submit_read(reactor, fd, f->base.buf, cap, 0, f);
    if (rc != 0) { free(f->base.buf); free(f); rt_task_release(task); rt_io_close(h); return NULL; }
    return task;
}

/* 写入类共享提交：打开 async 写句柄 → 深拷贝数据 → 提交 write。 */
static void* rt_file_write_async(const char* path, const void* data, int32_t len,
                                 RtIoOpType op_type, int append) {
    if (!path) return NULL;
    void* loop = rt_event_loop_current();
    void* reactor = loop ? rt_event_loop_get_reactor(loop) : NULL;
    if (!reactor) return NULL;

    rt_io_handle_t h;
#ifdef _WIN32
    if (append) {
        /* FILE_APPEND_DATA：写入总是追加到末尾，忽略 OVERLAPPED offset */
        h = rt_io_open_async(path, FILE_APPEND_DATA, OPEN_ALWAYS);
    } else {
        /* CREATE_ALWAYS：截断到 0（对齐同步 WriteAll 覆写语义） */
        h = rt_io_open_async(path, GENERIC_READ | GENERIC_WRITE, CREATE_ALWAYS);
    }
#else
    h = rt_io_open_async_w(path, append);
#endif
    if (h == RT_IO_HANDLE_INVALID) return NULL;

    RtTask* task = rt_task_alloc();
    if (!task) { rt_io_close(h); return NULL; }
    task->status = RT_TASK_PENDING;

    RtFileIoCompletion* f = (RtFileIoCompletion*)calloc(1, sizeof(RtFileIoCompletion));
    if (!f) { rt_task_release(task); rt_io_close(h); return NULL; }
    f->base.task = task;
    f->base.op_type = op_type;
    f->handle = h;

    if (len == 0) {
        /* 打开已截断（CREATE_ALWAYS/O_TRUNC）；空写无需提交 */
        task->int_result = 1;
        rt_task_complete(task);
        rt_io_close(h);
        free(f);
        return task;
    }

    /* 深拷贝写数据，与调用方生命周期解耦 */
    char* copy = (char*)malloc((size_t)len);
    if (!copy) { free(f); rt_task_release(task); rt_io_close(h); return NULL; }
    memcpy(copy, data, (size_t)len);
    f->base.buf = copy;
    f->base.buf_size = len;

    int32_t fd = rt_io_handle_to_fd(h);
    rt_reactor_register(reactor, fd, 0);
    uint64_t offset = append ? rt_io_append_offset() : 0;
    int32_t rc = rt_reactor_submit_write(reactor, fd, copy, (uint32_t)len, offset, f);
    if (rc != 0) { free(copy); free(f); rt_task_release(task); rt_io_close(h); return NULL; }
    return task;
}

/* 文件 async 完成处理器（rt_io_completion_complete 对文件 op_type 转发至此）。 */
void rt_file_io_completion_complete(void* user_data, int32_t result) {
    RtFileIoCompletion* f = (RtFileIoCompletion*)user_data;
    if (!f || !f->base.task) return;
    RtTask* task = f->base.task;

    switch (f->base.op_type) {
        case RT_IO_OP_FILE_READ_TEXT:
            if (result >= 0 && f->base.buf) {
                ((char*)f->base.buf)[result] = '\0';
                task->ptr_result = f->base.buf;
                f->base.buf = NULL; /* 所有权转移给 Task */
            } else {
                task->ptr_result = rt_io_malloc_empty();
            }
            break;
        case RT_IO_OP_FILE_READ_BYTES:
            /* buf 是 byte[] payload，直接复用；失败 → Length 0 */
            if (result < 0) result = 0;
            rt_file_array_set_len(f->base.buf, result);
            task->ptr_result = f->base.buf;
            f->base.buf = NULL;
            break;
        case RT_IO_OP_FILE_READ_LINES: {
            int32_t n = (result > 0) ? result : 0;
            task->ptr_result = rt_file_lines_from_buf(f->base.buf, (size_t)n);
            break;
        }
        case RT_IO_OP_FILE_WRITE_TEXT:
        case RT_IO_OP_FILE_WRITE_BYTES:
        case RT_IO_OP_FILE_APPEND:
            task->int_result = (result >= 0) ? 1 : 0;
            break;
        default:
            task->int_result = 0;
            break;
    }

    rt_task_complete(task);

    if (f->handle != RT_IO_HANDLE_INVALID) rt_io_close(f->handle);
    if (f->base.buf) free(f->base.buf);
    free(f);
}

/* ---- 数据面 File.*Async 真异步入口（替换 rt_task_run.c 的线程池包装）---- */

/* 构造期（main 前，单线程）注册文件完成处理器到 g_rt_io_file_completion，
 * 使 rt_io_completion_complete 能分发到本文件；未链接 rt_file.c 时指针保持
 * NULL，网络分发表走安全 no-op。 */
#if defined(__GNUC__) || defined(__clang__)
__attribute__((constructor))
static void rt_file_register_io_completion(void) {
    g_rt_io_file_completion = rt_file_io_completion_complete;
}
#endif

void* rt_file_read_all_text_async(const char* path) {
    return rt_file_read_async(path, RT_IO_OP_FILE_READ_TEXT);
}

void* rt_file_read_all_bytes_async(const char* path) {
    return rt_file_read_async(path, RT_IO_OP_FILE_READ_BYTES);
}

void* rt_file_read_all_lines_async(const char* path) {
    return rt_file_read_async(path, RT_IO_OP_FILE_READ_LINES);
}

void* rt_file_write_all_text_async(const char* path, const char* content) {
    if (!content) content = "";
    return rt_file_write_async(path, content, (int32_t)strlen(content),
                               RT_IO_OP_FILE_WRITE_TEXT, 0);
}

void* rt_file_write_all_bytes_async(const char* path, void* bytes) {
    int32_t len = bytes ? rt_array_length(bytes) : 0;
    return rt_file_write_async(path, bytes, len, RT_IO_OP_FILE_WRITE_BYTES, 0);
}

void* rt_file_append_all_text_async(const char* path, const char* content) {
    if (!content) content = "";
    return rt_file_write_async(path, content, (int32_t)strlen(content),
                               RT_IO_OP_FILE_APPEND, 1);
}
