#include "rt_abi.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <limits.h>

/* Regex 引擎（rt_regex_*）：Arc.Text.Regex 的 runtime。
 *
 * 架构：编译型字节码程序 + 显式回溯 VM（对标 .NET 默认 backtracking engine /
 * PCRE2 non-DFS）。模式调用即编译成指令数组（rx_inst[]），调用即释放，无共享
 * 全局态、线程安全。VM 携带统一步数预算（RX_STEP_LIMIT）作为 ReDoS 安全阀：
 * 达到预算即中止整次匹配并返回无匹配（诚实失败，不假绿）。这补齐 C#「默认无
 * 超时、需开发者显式 MatchTimeout」的短板——Arc 默认自带 ReDoS 保护。
 *
 * 诚实子集边界（byte-oriented，按 UTF-8 码元匹配，与 Arc string.Length / s[i]
 * 一致；`\d \w \s`、`\b` 与 IgnoreCase 折叠限 ASCII，见下）：
 *   - 语法：字面量(含多字节 UTF-8)、`.`(按码元消费，受 Singleline)、`*`、`+`、`?`、
 *     `{n,m}`/`{n,}`/`{n}`(与懒量词 `*?` `+?` `??` `{n,m}?`)、`[...]`
 *     (区间/`^` 否定/类内转义/类内简写 `[\d]`/字面 `-`)、`^` `$`、`\b \B \A \z`、
 *     `(...)`(捕获)、`(?:...)`(非捕获)、`(?<name>...)`(命名组)、`|`(分支)、
 *     前瞻 `(?=...)` `(?!...)`、后瞻 `(?<=...)` `(?<!...)`（定长/变长皆可）、原子组 `(?>...)`、
 *     反向引用 `\1..\9` / `\k<name>`、转义 `\. \* \? \+ \( \) \[ \] \\ \| \- \^ \$`
 *     `\d \D \w \W \s \S` `\b \B \A \z` `\n \t \r \f \v \0` `\xHH` `\uHHHH`。
 *   - 忽略大小写：`(?i)` / 编译 `RegexOptions.IgnoreCase`，ASCII 全折叠；非 ASCII 多字节
 *     码点折叠需 Unicode CaseFolding 表（后续可扩），在此边界内如实不折叠。
 *   - 多行 / 单行：`(?m)` `(?s)` / 编译 `Multiline` `Singleline`（含作用域 `(?i:...)`）。
 *   - 后瞻：定长与变长皆可（对齐 .NET 7+ backtracking 变长后瞻），子模式须收口于当前
 *     位置；不依赖对编译产物的静态长度分析，而是逐起点尝试。
 *   - 语义：贪婪左最优（leftmost；非锚定自每个起始位置扫描，首个命中返回；
 *     贪婪=最长、懒=最短）。
 *   - 替换：`$0`..`$9` 组引用与 `$$` 字面 `$`。
 *
 * 值为 malloc 出的编译产物；调用即编译即释放，无共享全局态，线程安全。
 * rt_array_create(0/b,) 返回的 string[] 元素为 strdup 产物，交由 ARC string 收编。
 */

enum {
    RX_ICASE = 1,  /* RegexOptions.IgnoreCase */
    RX_MLINE = 2,  /* RegexOptions.Multiline */
    RX_SLINE = 4,  /* RegexOptions.Singleline */
    RX_EXPLI = 8,  /* RegexOptions.ExplicitCapture */
};

#define RX_STEP_LIMIT 200000000LL

typedef struct rx_inst {
    int op, a1, a2, a3, a4;
} rx_inst;

enum {
    RXOP_MATCH, RXOP_CHAR, RXOP_CLASS, RXOP_ANY,
    RXOP_BOL, RXOP_EOL, RXOP_BOT, RXOP_EOT, RXOP_BOW, RXOP_NOBW,
    RXOP_SAVE, RXOP_BRANCH, RXOP_JMP, RXOP_REPEAT, RXOP_ENDREP,
    RXOP_BACKREF, RXOP_ASSERT, RXOP_ATOMIC, RXOP_FAIL
};

enum { AS_POS = 0, AS_NEG = 1, AS_BPOS = 2, AS_BNEG = 3 };

typedef struct rx_class {
    int negate;
    unsigned char tab[256];
} rx_class;

typedef struct rxprog {
    rx_inst* code;
    int ncode, cap;
    rx_class* cls;
    int ncls, clscap;
    int ngroups;            /* 捕获组槽位容量 = 最大组号 + 1（含整段 group 0） */
    char** gnames;          /* 命名组名字表 */
    int* gidx;              /* 名字 -> 组号 */
    int nnames, nnamescap;
    /* 未决命名反向引用：记录 code 索引 + 名字在 pattern 中的区间，收尾解析。 */
    int* un_pc; char** un_name; int* un_plen;
    int nun, un_cap;
} rxprog;

typedef struct {
    rxprog* p;
    const char* pat; int plen, pos;
    int flags;              /* 当前作用域选项（内联 (?i) 聚合） */
    int ngrp;               /* 下一捕获组号（0 保留给整段，捕获自 1 起） */
    int err;
} rxparser;

typedef struct rgcap { int start, len; } rgcap;

typedef struct rxcx {
    const rxprog* p;
    rgcap* caps;
    long long steps, limit;
    int aborted;
} rxcx;

#ifndef RX_NG
/* 捕获组槽位数：rxcx 经 p 指向 rxprog.ngroups（整段 group 0 计入，恒 ≥1）。 */
#define RX_NG(cxp) ((size_t)((cxp)->p->ngroups > 0 ? (cxp)->p->ngroups : 1))
#endif

/* ---- 字符串/编码辅助 ------------------------------------------------ */

static unsigned rx_fold(unsigned c) {  /* ASCII 大小写折叠（仅 ASCII 字母） */
    return (c >= 'A' && c <= 'Z') ? c + 32 : c;
}

static int rx_bytematch(unsigned a, unsigned b, int ic) {
    if (!ic) return a == b;
    return rx_fold(a) == rx_fold(b);
}

static int rx_isword(unsigned c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
           (c >= '0' && c <= '9') || c == '_';
}

/* UTF-8 序列长度：给定首字节返回码元字节数。 */
static int rx_utf8_len(unsigned char b) {
    if (b < 0x80) return 1;
    if (b < 0xE0) return 2;   /* 0xC0..0xDF */
    if (b < 0xF0) return 3;   /* 0xE0..0xEF */
    if (b < 0xF8) return 4;   /* 0xF0..0xF7 */
    return 1;
}

static char* rx_dup_n(const char* s, size_t n) {
    char* d = (char*)malloc(n + 1);
    if (!d) return NULL;
    if (n) memcpy(d, s, n);
    d[n] = '\0';
    return d;
}

/* 码点 → UTF-8 字节（写 s，返回字节数）。 */
static int rx_codepoint_utf8(unsigned cp, unsigned char* s) {
    if (cp < 0x80) { s[0] = (unsigned char)cp; return 1; }
    if (cp < 0x800) {
        s[0] = (unsigned char)(0xC0 | (cp >> 6));
        s[1] = (unsigned char)(0x80 | (cp & 0x3F));
        return 2;
    }
    if (cp < 0x10000) {
        s[0] = (unsigned char)(0xE0 | (cp >> 12));
        s[1] = (unsigned char)(0x80 | ((cp >> 6) & 0x3F));
        s[2] = (unsigned char)(0x80 | (cp & 0x3F));
        return 3;
    }
    if (cp < 0x200000) {
        s[0] = (unsigned char)(0xF0 | (cp >> 18));
        s[1] = (unsigned char)(0x80 | ((cp >> 12) & 0x3F));
        s[2] = (unsigned char)(0x80 | ((cp >> 6) & 0x3F));
        s[3] = (unsigned char)(0x80 | (cp & 0x3F));
        return 4;
    }
    return 0;
}

/* ---- 分配/生成 ------------------------------------------------------ */

/* 生成指令并返回其下标（调用方回填续点/分支目标用 `p->code[idx]`；失败 -1）。 */
static int rx_emit(rxparser* rp, int op, int a1, int a2, int a3, int a4) {
    rxprog* p = rp->p;
    if (p->ncode >= p->cap) {
        int nc = p->cap ? p->cap * 2 : 64;
        rx_inst* nd = (rx_inst*)realloc(p->code, (size_t)nc * sizeof(rx_inst));
        if (!nd) { rp->err = 1; return -1; }
        p->code = nd;
        p->cap = nc;
    }
    int idx = p->ncode++;
    rx_inst* I = &p->code[idx];
    I->op = op; I->a1 = a1; I->a2 = a2; I->a3 = a3; I->a4 = a4;
    return idx;
}

static int rx_prog_len(rxparser* rp) { return rp->p->ncode; }

static int rx_class_new(rxprog* p, int negate, const unsigned char* tab) {
    if (p->ncls >= p->clscap) {
        int nc = p->clscap ? p->clscap * 2 : 16;
        rx_class* nd = (rx_class*)realloc(p->cls, (size_t)nc * sizeof(rx_class));
        if (!nd) return -1;
        p->cls = nd;
        p->clscap = nc;
    }
    rx_class* c = &p->cls[p->ncls];
    c->negate = negate;
    memcpy(c->tab, tab, 256);
    return p->ncls++;
}

static void rx_register_name(rxprog* p, const char* name, int nlen, int group) {
    for (int i = 0; i < p->nnames; i++)
        if ((int)strlen(p->gnames[i]) == nlen && memcmp(p->gnames[i], name, (size_t)nlen) == 0) {
            p->gidx[i] = group;   /* 重名最后定义者生效 */
            return;
        }
    if (p->nnames >= p->nnamescap) {
        int nc = p->nnamescap ? p->nnamescap * 2 : 8;
        char** nn = (char**)realloc(p->gnames, (size_t)nc * sizeof(char*));
        int* gi = (int*)realloc(p->gidx, (size_t)nc * sizeof(int));
        if (!nn || !gi) return;
        p->gnames = nn; p->gidx = gi; p->nnamescap = nc;
    }
    char* d = rx_dup_n(name, (size_t)nlen);
    if (!d) return;
    p->gnames[p->nnames] = d;
    p->gidx[p->nnames] = group;
    p->nnames++;
}

static int rx_lookup_name(const rxprog* p, const char* name, int nlen) {
    for (int i = 0; i < p->nnames; i++)
        if ((int)strlen(p->gnames[i]) == nlen && memcmp(p->gnames[i], name, (size_t)nlen) == 0)
            return p->gidx[i];
    return -1;
}

static void rx_queue_unresolved(rxparser* rp, int pc, const char* name, int nlen) {
    rxprog* p = rp->p;
    if (p->nun >= p->un_cap) {
        int nc = p->un_cap ? p->un_cap * 2 : 8;
        int* up = (int*)realloc(p->un_pc, (size_t)nc * sizeof(int));
        char** un = (char**)realloc(p->un_name, (size_t)nc * sizeof(char*));
        int* ul = (int*)realloc(p->un_plen, (size_t)nc * sizeof(int));
        if (!up || !un || !ul) return;
        p->un_pc = up; p->un_name = un; p->un_plen = ul; p->un_cap = nc;
    }
    char* d = rx_dup_n(name, (size_t)nlen);
    if (!d) return;
    p->un_pc[p->nun] = pc;
    p->un_name[p->nun] = d;
    p->un_plen[p->nun] = nlen;
    p->nun++;
}

static void rx_free_prog(rxprog* p) {
    if (!p) return;
    free(p->code);
    free(p->cls);
    for (int i = 0; i < p->nnames; i++) free(p->gnames[i]);
    free(p->gnames); free(p->gidx);
    for (int i = 0; i < p->nun; i++) free(p->un_name[i]);
    free(p->un_pc); free(p->un_name); free(p->un_plen);
    free(p);
}

/* 查找 REPEAT@pc 对应的 ENDREP 指令下标。 */
static int rx_find_end(const rx_inst* code, int ncode, int pc) {
    int depth = 0;
    for (int i = pc + 1; i < ncode; i++) {
        if (code[i].op == RXOP_REPEAT) depth++;
        else if (code[i].op == RXOP_ENDREP) {
            if (depth == 0) return i;
            depth--;
        }
    }
    return ncode;
}

/* ---- 解析器 ----------------------------------------------------------- */

static int rx_peek(const rxparser* rp) {
    return rp->pos < rp->plen ? (unsigned char)rp->pat[rp->pos] : -1;
}

static void rx_adv(rxparser* rp) { if (rp->pos < rp->plen) rp->pos++; }

static int rx_hexval(int c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/* 类内成员写入：用于把转义/简写展开到成员表。 */
static void rx_set_range(unsigned char* m, int lo, int hi) {
    if (lo < 0) return;
    if (hi > 255) hi = 255;
    for (int i = lo; i <= hi; i++) m[i] = 1;
}

static void rx_set_ascii_class(unsigned char* m, int which) {
    /* which: 0=\d 1=\D 2=\w 3=\W 4=\s 5=\S（构建后与外部 negate 叠加） */
    switch (which) {
    case 0: rx_set_range(m, '0', '9'); break;
    case 1: for (int i = 0; i < 256; i++) if (i < '0' || i > '9') m[i] = 1; break;
    case 2: rx_set_range(m, '0', '9'); rx_set_range(m, 'a', 'z');
            rx_set_range(m, 'A', 'Z'); m['_'] = 1; break;
    case 3: for (int i = 0; i < 256; i++)
                if (!((i >= '0' && i <= '9') || (i >= 'a' && i <= 'z') ||
                      (i >= 'A' && i <= 'Z') || i == '_')) m[i] = 1; break;
    case 4: m[' '] = m['\t'] = m['\n'] = m['\r'] = m['\f'] = m['\v'] = 1; break;
    case 5: for (int i = 0; i < 256; i++)
                if (i != ' ' && i != '\t' && i != '\n' && i != '\r' && i != '\f' && i != '\v') m[i] = 1; break;
    default: break;
    }
}

/* 解析一个字符类（'[' 已消费）。返回类下标，失败 -1。 */
static int rx_parse_class(rxparser* rp) {
    int negate = 0, first = 1, closed = 0;
    unsigned char m[256];
    memset(m, 0, sizeof(m));
    if (rx_peek(rp) == '^') { negate = 1; rx_adv(rp); }
    while (rp->pos < rp->plen) {
        int c = rx_peek(rp);
        if (c == ']' && !first) { rx_adv(rp); closed = 1; break; }
        first = 0;
        if (c == '\\') {
            rx_adv(rp);
            if (rp->pos >= rp->plen) break;
            int e = rx_peek(rp); rx_adv(rp);
            switch (e) {
            case 'd': case 'D': case 'w': case 'W': case 's': case 'S':
                rx_set_ascii_class(m, (e == 'd') ? 0 : (e == 'D') ? 1 :
                                     (e == 'w') ? 2 : (e == 'W') ? 3 :
                                     (e == 's') ? 4 : 5);
                goto enditem;
            case 'b': rx_set_range(m, 0x08, 0x08); goto enditem;
            case 'n': rx_set_range(m, '\n', '\n'); goto enditem;
            case 't': rx_set_range(m, '\t', '\t'); goto enditem;
            case 'r': rx_set_range(m, '\r', '\r'); goto enditem;
            case 'f': rx_set_range(m, '\f', '\f'); goto enditem;
            case 'v': rx_set_range(m, '\v', '\v'); goto enditem;
            case 'x': {
                int h1 = rp->pos < rp->plen ? rx_hexval(rx_peek(rp)) : -1;
                int h2 = h1 >= 0 && rp->pos + 1 < rp->plen ? rx_hexval((unsigned char)rp->pat[rp->pos + 1]) : -1;
                if (h1 >= 0) { rx_adv(rp); if (h2 >= 0) { rx_adv(rp); h1 = h1 * 16 + h2; } }
                if (h1 >= 0) rx_set_range(m, h1, h1);
                goto enditem;
            }
            default: rx_set_range(m, (unsigned char)e, (unsigned char)e); goto enditem;
            }
        }
        /* 普通字符（可能为区间起点） */
        rx_adv(rp);
        if (rp->pos + 1 < rp->plen && rx_peek(rp) == '-' &&
            rp->pat[rp->pos + 1] != ']') {
            rx_adv(rp);                      /* '-' */
            rx_adv(rp);                      /* 区间终点 */
            int hi = rp->pos >= 1 ? (unsigned char)rp->pat[rp->pos - 1] : c;
            rx_set_range(m, c, hi);
        } else {
            rx_set_range(m, c, c);
        }
    enditem:;
        (void)0;
    }
    /* 字符类必须有 ']' 闭合；否则按未闭合（生成空集 → 恒不匹配，诚实） */
    if (!closed) { memset(m, 0, sizeof(m)); }
    (void)negate;
    return rx_class_new(rp->p, negate, m);
}

static void rx_apply_quant(rxparser* rp, int start, int end) {
    int c = rx_peek(rp);
    int min = 0, max = -1, greedy = 1;
    if (c == '*') { min = 0; max = -1; rx_adv(rp); }
    else if (c == '+') { min = 1; max = -1; rx_adv(rp); }
    else if (c == '?') { min = 0; max = 1; rx_adv(rp); }
    else if (c == '{') {
        /* 尝试计数量词 {n} {n,} {n,m} {,m}；非法则作为字面 '{'，不消费 */
        int save = rp->pos, ok = 0;
        long n = -1, m = -1;
        int saw = 0;
        {
            int sv = rp->pos; rx_adv(rp);  /* '{' */
            long lo = 0, hi = -1; int lo_ok = 0;
            while (rp->pos < rp->plen) {
                int d = rx_peek(rp);
                if (d >= '0' && d <= '9') { if (lo < 10000000L) lo = lo * 10 + (d - '0'); rx_adv(rp); lo_ok = 1; continue; }
                break;
            }
            int nxt = rx_peek(rp);
            if (nxt == ',') {
                rx_adv(rp);
                hi = 0; int hi_ok = 0;
                while (rp->pos < rp->plen) {
                    int d = rx_peek(rp);
                    if (d >= '0' && d <= '9') { if (hi < 10000000L) hi = hi * 10 + (d - '0'); hi += 0; hi_ok = 1; rx_adv(rp); continue; }
                    break;
                }
                if (rx_peek(rp) == '}') {
                    rx_adv(rp);
                    /* C# 语义：必须存在前导下限 {n,} / {n,m}；无前导数字（如 {,m}）
                     * 视为字面 '{'，不当作 {0,m}。 */
                    if (lo_ok) { n = lo; m = hi_ok ? hi : -1; ok = 1; }
                }
            } else if (nxt == '}') {
                rx_adv(rp);
                if (lo_ok) { n = m = lo; ok = 1; }
            }
        }
        if (!ok) { rp->pos = save; return; }
        min = (int)(n < 0 ? 0 : n);
        max = (int)(m < 0 ? -1 : m);
        saw = 1;
    } else {
        return;
    }
    if (rx_peek(rp) == '?') { greedy = 0; rx_adv(rp); }   /* 懒量词 */
    if (rx_peek(rp) == '+') { rx_adv(rp); }                /* possessive 不支持，吞掉并诚实退化为贪心 */
    if (end <= start) return;                              /* 空单元不包装 */
    if (min == 0 && max == 0) return;                      /* a{0} 恒空 */
    /* 包装 REPEAT[body...]ENDREP */
    rxprog* p = rp->p;
    if (p->ncode >= p->cap) {
        int nc = p->cap ? p->cap * 2 : 64;
        rx_inst* nd = (rx_inst*)realloc(p->code, (size_t)nc * sizeof(rx_inst));
        if (!nd) { rp->err = 1; return; }
        p->code = nd; p->cap = nc;
    }
    if (p->cap < end + 2) {
        int nc = end + 2 + 64;
        rx_inst* nd = (rx_inst*)realloc(p->code, (size_t)nc * sizeof(rx_inst));
        if (!nd) { rp->err = 1; return; }
        p->code = nd; p->cap = nc;
    }
    memmove(p->code + start + 1, p->code + start, (size_t)(end - start) * sizeof(rx_inst));
    rx_inst rep;
    rep.op = RXOP_REPEAT; rep.a1 = min; rep.a2 = max; rep.a3 = greedy; rep.a4 = 0;
    p->code[start] = rep;
    rx_inst er;
    er.op = RXOP_ENDREP; er.a1 = er.a2 = er.a3 = er.a4 = 0;
    p->code[end + 1] = er;
    p->ncode = end + 2;
}

static void rx_altrx(rxparser* rp);
static void rx_seq(rxparser* rp);

/* 捕获组：emit SAVE(grp,0) body SAVE(grp,1)，并消费闭合 ')'。
 *
 * 关键：必须在此消费 ')'，否则调用方（rx_seq）会把第一个 ')' 误判为外层
 * 序列终止符而提前收口——后续原子（第二个组 `(\d)`、`.@\w+` 等）被整体丢弃，
 * 造成第二组捕获为空、替换/反向引用错乱（历史 group2 恒空、`$2.$1` 输出
 * ".user@.host" 的根因）。
 */
static void rx_capture(rxparser* rp, int grp) {
    rx_emit(rp, RXOP_SAVE, grp, 0, 0, 0);
    rx_altrx(rp);
    if (rp->pos < rp->plen && rx_peek(rp) == ')') rx_adv(rp);   /* 消费 ')' */
    rx_emit(rp, RXOP_SAVE, grp, 1, 0, 0);
}

static void rx_atom(rxparser* rp) {
    int start = rx_prog_len(rp);
    if (rp->pos >= rp->plen) { rp->err = 1; return; }
    int c = rx_peek(rp); rx_adv(rp);
    int fl = rp->flags;

    switch (c) {
    case '(': {
        if (rp->pos < rp->plen && rx_peek(rp) == '?') {
            rx_adv(rp);
            if (rp->pos < rp->plen) {
                int q = rx_peek(rp);
                if (q == ':') { rx_adv(rp); rx_altrx(rp); if (rp->pos < rp->plen) rx_adv(rp); } /* 非捕获组 */
                else if (q == '=' || q == '!') {
                    rx_adv(rp);
                    int kind = (q == '=') ? AS_POS : AS_NEG;
                    int as = rx_emit(rp, RXOP_ASSERT, 0, kind, 0, 0);
                    int sub = rx_prog_len(rp);
                    rx_altrx(rp);
                    if (rp->pos < rp->plen) rx_adv(rp);   /* ')' */
                    rx_emit(rp, RXOP_MATCH, 0, 0, 0, 0);
                    rx_inst* I = &rp->p->code[as];
                    I->a1 = sub; I->a3 = rx_prog_len(rp); /* 续点 = MATCH 之后 */
                }
                else if (q == '<') {
                    rx_adv(rp);
                    int q2 = rx_peek(rp);
                    if (q2 == '=' || q2 == '!') {
                        rx_adv(rp);
                        int kind = (q2 == '=') ? AS_BPOS : AS_BNEG;
                    int as = rx_emit(rp, RXOP_ASSERT, 0, kind, 0, 0);
                    int sub = rx_prog_len(rp);
                    rx_altrx(rp);
                    if (rp->pos < rp->plen) rx_adv(rp);
                    rx_emit(rp, RXOP_MATCH, 0, 0, 0, 0);
                    int after = rx_prog_len(rp);
                    rx_inst* I = &rp->p->code[as];
                    I->a1 = sub; I->a3 = after;
                    }
                    else {
                        /* 命名组 (?<name>...) */
                        int nstart = rp->pos;
                        while (rp->pos < rp->plen) {
                            int ch = rx_peek(rp);
                            if ((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
                                (ch >= '0' && ch <= '9') || ch == '_') rx_adv(rp);
                            else break;
                        }
                        int grp = rp->ngrp++;
                        rx_register_name(rp->p, rp->pat + nstart, rp->pos - nstart, grp);
                        if (rp->pos < rp->plen && rx_peek(rp) == '>') rx_adv(rp);
                        rx_capture(rp, grp);
                    }
                }
                else if (q == '>') {
                    rx_adv(rp);
                    int at = rx_emit(rp, RXOP_ATOMIC, 0, 0, 0, 0);
                    rx_altrx(rp);
                    if (rp->pos < rp->plen) rx_adv(rp);
                    rp->p->code[at].a1 = rx_prog_len(rp);   /* 续点 = 原子组之后 */
                }
                else if (q == 'P' && rp->pos + 1 < rp->plen && rp->pat[rp->pos + 1] == '<') {
                    /* (?P<name>...) */
                    rp->pos += 2; /* P< */
                    int nstart = rp->pos;
                    while (rp->pos < rp->plen) {
                        int ch = rx_peek(rp);
                        if ((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
                            (ch >= '0' && ch <= '9') || ch == '_') rx_adv(rp);
                        else break;
                    }
                    int grp = rp->ngrp++;
                    rx_register_name(rp->p, rp->pat + nstart, rp->pos - nstart, grp);
                    if (rp->pos < rp->plen && rx_peek(rp) == '>') rx_adv(rp);
                    rx_capture(rp, grp);
                }
                else if (q == 'i' || q == 'm' || q == 's' || q == '-' ||
                         (q == 'x')) {
                    /* 内联标志 (?im-s) 或 (?im-s:...) */
                    int old = rp->flags;
                    int scoped = 0;
                    int neg = 0;
                    while (rp->pos < rp->plen) {
                        int f = rx_peek(rp);
                        if (f == '-' ) { neg = 1; rx_adv(rp); continue; }
                        if (f == 'i') { rp->flags = neg ? (rp->flags & ~RX_ICASE) : (rp->flags | RX_ICASE); rx_adv(rp); }
                        else if (f == 'm') { rp->flags = neg ? (rp->flags & ~RX_MLINE) : (rp->flags | RX_MLINE); rx_adv(rp); }
                        else if (f == 's') { rp->flags = neg ? (rp->flags & ~RX_SLINE) : (rp->flags | RX_SLINE); rx_adv(rp); }
                        else if (f == ':') { rx_adv(rp); scoped = 1; rx_altrx(rp); if (rp->pos < rp->plen) rx_adv(rp); rp->flags = old; break; }
                        else if (f == ')') { rx_adv(rp); break; }   /* 裸 (?i) 保持作用于当前组剩余内容，不还原 */
                        else break;
                    }
                    (void)scoped;
                }
                else {
                    rx_altrx(rp);   /* 未知 (? 构造：按普通组容错 */
                    if (rp->pos < rp->plen) rx_adv(rp);
                }
                goto done_atom;
            }
        }
        /* 普通捕获组（ExplicitCapture 下为非捕获） */
        if (rp->flags & RX_EXPLI) { rx_altrx(rp); if (rp->pos < rp->plen) rx_adv(rp); }
        else { int grp = rp->ngrp++; rx_capture(rp, grp); }
        goto done_atom;
    }
    case '[': {
        int ci = rx_parse_class(rp);
        if (ci >= 0) rx_emit(rp, RXOP_CLASS, ci, fl & RX_ICASE, 0, 0);
        goto done_atom;
    }
    case '^': rx_emit(rp, RXOP_BOL, 0, fl & RX_MLINE, 0, 0); goto done_atom;
    case '$': rx_emit(rp, RXOP_EOL, 0, fl & RX_MLINE, 0, 0); goto done_atom;
    case '.': rx_emit(rp, RXOP_ANY, 0, fl & RX_SLINE, 0, 0); goto done_atom;
    case '\\': {
        if (rp->pos >= rp->plen) { rp->err = 1; return; }
        int e = rx_peek(rp); rx_adv(rp);
        switch (e) {
        case 'd': case 'D': case 'w': case 'W': case 's': case 'S': {
            int which = (e == 'd') ? 0 : (e == 'D') ? 1 : (e == 'w') ? 2 : (e == 'W') ? 3 : (e == 's') ? 4 : 5;
            unsigned char m[256]; memset(m, 0, sizeof(m));
            rx_set_ascii_class(m, which);
            int ci = rx_class_new(rp->p, 0, m);
            if (ci >= 0) rx_emit(rp, RXOP_CLASS, ci, fl & RX_ICASE, 0, 0);
            goto done_atom;
        }
        case 'b': rx_emit(rp, RXOP_BOW, 0, 0, 0, 0); goto done_atom;
        case 'B': rx_emit(rp, RXOP_NOBW, 0, 0, 0, 0); goto done_atom;
        case 'A': rx_emit(rp, RXOP_BOT, 0, 0, 0, 0); goto done_atom;
        case 'z': case 'Z': rx_emit(rp, RXOP_EOT, 0, 0, 0, 0); goto done_atom;
        case 'n': rx_emit(rp, RXOP_CHAR, '\n', fl & RX_ICASE, 0, 0); goto done_atom;
        case 't': rx_emit(rp, RXOP_CHAR, '\t', fl & RX_ICASE, 0, 0); goto done_atom;
        case 'r': rx_emit(rp, RXOP_CHAR, '\r', fl & RX_ICASE, 0, 0); goto done_atom;
        case 'f': rx_emit(rp, RXOP_CHAR, '\f', fl & RX_ICASE, 0, 0); goto done_atom;
        case 'v': rx_emit(rp, RXOP_CHAR, '\v', fl & RX_ICASE, 0, 0); goto done_atom;
        case '0': rx_emit(rp, RXOP_CHAR, 0, fl & RX_ICASE, 0, 0); goto done_atom;
        case 'x': {
            int h1 = rp->pos < rp->plen ? rx_hexval(rx_peek(rp)) : -1;
            int h2 = h1 >= 0 && rp->pos + 1 < rp->plen ? rx_hexval((unsigned char)rp->pat[rp->pos + 1]) : -1;
            if (h1 >= 0) { rx_adv(rp); if (h2 >= 0) { rx_adv(rp); h1 = h1 * 16 + h2; } }
            if (h1 >= 0) rx_emit(rp, RXOP_CHAR, h1, fl & RX_ICASE, 0, 0);
            goto done_atom;
        }
        case 'u': case 'U': {
            unsigned cp = 0; int nd = 0, valid = 1;
            while (nd < 8 && rp->pos < rp->plen) {
                int hv = rx_hexval(rx_peek(rp));
                if (hv < 0) { if (nd >= 4) break; valid = 0; break; }
                cp = cp * 16 + (unsigned)hv; rx_adv(rp); nd++;
                if ((e == 'u' && nd == 4) || (e == 'U' && nd == 8)) break;
            }
            if (valid && nd >= (e == 'u' ? 4 : 8)) {
                unsigned char b[4]; int nb = rx_codepoint_utf8(cp, b);
                for (int i = 0; i < nb; i++) rx_emit(rp, RXOP_CHAR, b[i], 0, 0, 0);
            }
            goto done_atom;
        }
        case 'k': {
            if (rp->pos < rp->plen && rx_peek(rp) == '<') {
                rx_adv(rp);
                int nstart = rp->pos;
                while (rp->pos < rp->plen) {
                    int ch = rx_peek(rp);
                    if ((ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') ||
                        (ch >= '0' && ch <= '9') || ch == '_') rx_adv(rp);
                    else break;
                }
                int pc = rx_prog_len(rp);
                rx_emit(rp, RXOP_BACKREF, 0, fl & RX_ICASE, 0, 0);
                int g = rx_lookup_name(rp->p, rp->pat + nstart, rp->pos - nstart);
                rp->p->code[pc].a1 = (g >= 0) ? g : 0;
                if (g < 0) rx_queue_unresolved(rp, pc, rp->pat + nstart, rp->pos - nstart);
                if (rp->pos < rp->plen && rx_peek(rp) == '>') rx_adv(rp);
                goto done_atom;
            }
            goto done_atom;
        }
        default:
            if (e >= '1' && e <= '9') {
                rx_emit(rp, RXOP_BACKREF, e - '0', fl & RX_ICASE, 0, 0);
                goto done_atom;
            }
            rx_emit(rp, RXOP_CHAR, (unsigned char)e, fl & RX_ICASE, 0, 0);
            goto done_atom;
        }
    }
    case '*': case '+': case '?':
        rx_emit(rp, RXOP_CHAR, (unsigned char)c, fl & RX_ICASE, 0, 0);
        goto done_atom;
    default:
        rx_emit(rp, RXOP_CHAR, (unsigned char)c, fl & RX_ICASE, 0, 0);
        goto done_atom;
    }
done_atom:
    /* 单元是否含 ASSERT/ATOMIC（可绕过量词包装） */
    {
        int has_ctl = 0;
        int len = rx_prog_len(rp);
        for (int i = start; i < len; i++) {
            int o = rp->p->code[i].op;
            if (o == RXOP_ASSERT || o == RXOP_ATOMIC) { has_ctl = 1; break; }
        }
        if (!has_ctl) rx_apply_quant(rp, start, len);
    }
}

static void rx_seq(rxparser* rp) {
    while (rp->pos < rp->plen) {
        int c = rx_peek(rp);
        if (c == '|' || c == ')') break;
        if (c == -1) break;
        int before = rx_prog_len(rp);
        rx_atom(rp);
        if (rx_prog_len(rp) == before) break;   /* 防死循环 */
    }
}

static int rx_in_first_head(void) { return 0; }   /* 占位；实现见 rx_altrx */

/* 分支：A1|A2|...|An 编译为 BRANCH 链。
 *
 * 布局：
 *   BRANCH(a1=L1) body0 JMP->after
 *   L1: BRANCH(a1=L2) body1 JMP->after
 *   ...
 *   Ln: BRANCH(a1=fail) bodyn JMP->after
 *   fail: FAIL
 *   after: <外层续点>（capture/repeat/顶层 MATCH 由调用方随后续写）
 *
 * 每个分支成功后经 JMP 越过 FAIL 到真实续点；分支体会自身失败返回 0 → BRANCH 回退
 * a1：非末支去下一分支，末支去 FAIL。FAIL 仅由「全部分支穷尽」进入，保证失败不回退
 * 到续点/MATCH 造成假成功，成功后也不会顺序砸到 FAIL。
 */
static void rx_altrx(rxparser* rp) {
    int bidx[256]; int nb = 0;
    int jmidx[256]; int nj = 0;
    bidx[nb++] = rx_emit(rp, RXOP_BRANCH, 0, 0, 0, 0);
    rx_seq(rp);
    jmidx[nj++] = rx_emit(rp, RXOP_JMP, 0, 0, 0, 0);
    while (rp->pos < rp->plen && rx_peek(rp) == '|') {
        rx_adv(rp);
        bidx[nb++] = rx_emit(rp, RXOP_BRANCH, 0, 0, 0, 0);
        rx_seq(rp);
        jmidx[nj++] = rx_emit(rp, RXOP_JMP, 0, 0, 0, 0);
    }
    int fail = rx_emit(rp, RXOP_FAIL, 0, 0, 0, 0);
    int after = rx_prog_len(rp);
    for (int i = 0; i < nj; i++) rp->p->code[jmidx[i]].a1 = after;
    for (int i = 0; i < nb; i++)
        rp->p->code[bidx[i]].a1 = (i + 1 < nb) ? bidx[i + 1] : fail;
}

/* ---- 匹配 VM ---------------------------------------------------------- */

static int rx_match_body(const rxprog* P, int pc, int stop, const char* S,
                         int len, int* pos, rxcx* cx);

/* 有界重复（贪心/懒）。body 停止于其 ENDREP；exit 为重复结构之后续点。 */
static int rx_repeat(const rxprog* P, int body, int exit, int min, int max,
                     int greedy, int taken, const char* S, int len, int* pos,
                     rxcx* cx) {
    if (cx->aborted) return 0;
    int M = (max < 0) ? INT_MAX : max;
    if (greedy) {
        if (taken < M) {
            int savep = *pos;
            rgcap sav[RX_NG(cx)];
            memcpy(sav, cx->caps, RX_NG(cx) * sizeof(rgcap));
            if (rx_match_body(P, body, exit - 1, S, len, pos, cx)) {
                if (cx->aborted) return 0;
                if (rx_repeat(P, body, exit, min, max, greedy, taken + 1, S, len, pos, cx))
                    return 1;
            } else if (cx->aborted) {
                return 0;
            }
            *pos = savep;
            memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
        }
        if (taken >= min) return rx_match_body(P, exit, 0, S, len, pos, cx);
        return 0;
    } else {
        if (taken >= min)
            if (rx_match_body(P, exit, 0, S, len, pos, cx)) return 1;
        if (taken < M) {
            int savep = *pos;
            rgcap sav[RX_NG(cx)];
            memcpy(sav, cx->caps, RX_NG(cx) * sizeof(rgcap));
            if (rx_match_body(P, body, exit - 1, S, len, pos, cx)) {
                if (cx->aborted) return 0;
                if (rx_repeat(P, body, exit, min, max, greedy, taken + 1, S, len, pos, cx))
                    return 1;
            }
            *pos = savep;
            memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
        }
        return 0;
    }
}

static int rx_match_body(const rxprog* P, int pc, int stop, const char* S,
                         int len, int* pos, rxcx* cx) {
    for (;;) {
        if (pc == stop) return 1;
        if (pc < 0 || pc >= P->ncode) return 0;
        if (++cx->steps > cx->limit) { cx->aborted = 1; return 0; }
        const rx_inst* I = &P->code[pc];
        switch (I->op) {
        case RXOP_MATCH: return 1;

        case RXOP_CHAR: {
            if (*pos >= len) return 0;
            unsigned b = (unsigned char)S[*pos];
            if (!rx_bytematch(b, (unsigned)I->a1, I->a2 & RX_ICASE)) return 0;
            (*pos)++; pc++; break;
        }
        case RXOP_CLASS: {
            if (*pos >= len) return 0;
            unsigned b = (unsigned char)S[*pos];
            const rx_class* C = &P->cls[I->a1];
            int in = C->tab[b];
            if ((I->a2 & RX_ICASE) && b < 0x80 && ((b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z')))
                in |= C->tab[rx_fold(b)];
            in = C->negate ? !in : in;
            if (!in) return 0;
            (*pos)++; pc++; break;
        }
        case RXOP_ANY: {
            if (*pos >= len) return 0;
            unsigned b = (unsigned char)S[*pos];
            if (b == '\n' && !(I->a2 & RX_SLINE)) return 0;
            int nb = rx_utf8_len((unsigned char)b);
            if (nb > len - *pos) nb = 1;
            *pos += nb; pc++; break;
        }
        case RXOP_BOL: {
            int ml = I->a2 & RX_MLINE;
            int ok = ml ? (*pos == 0 || S[*pos - 1] == '\n') : (*pos == 0);
            if (!ok) return 0;
            pc++; break;
        }
        case RXOP_EOL: {
            int ml = I->a2 & RX_MLINE;
            int ok = ml ? (*pos >= len || S[*pos] == '\n') : (*pos >= len);
            if (!ok) return 0;
            pc++; break;
        }
        case RXOP_BOT: if (*pos != 0) return 0; pc++; break;
        case RXOP_EOT: if (*pos != len) return 0; pc++; break;
        case RXOP_BOW: {
            int pv = (*pos > 0) && rx_isword((unsigned char)S[*pos - 1]);
            int cv = (*pos < len) && rx_isword((unsigned char)S[*pos]);
            if (pv == cv) return 0;
            pc++; break;
        }
        case RXOP_NOBW: {
            int pv = (*pos > 0) && rx_isword((unsigned char)S[*pos - 1]);
            int cv = (*pos < len) && rx_isword((unsigned char)S[*pos]);
            if (pv != cv) return 0;
            pc++; break;
        }
        case RXOP_SAVE: {
            rgcap* g = &cx->caps[I->a1];
            if (I->a2 == 0) g->start = *pos;
            else g->len = *pos - g->start;
            pc++; break;
        }
        case RXOP_BRANCH: {
            int savep = *pos;
            rgcap sav[RX_NG(cx)];
            memcpy(sav, cx->caps, RX_NG(cx) * sizeof(rgcap));
            if (rx_match_body(P, pc + 1, stop, S, len, pos, cx)) return 1;
            if (cx->aborted) return 0;
            *pos = savep;
            memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
            pc = I->a1;
            break;
        }
        case RXOP_JMP: pc = I->a1; break;
        case RXOP_REPEAT: {
            int endp = rx_find_end(P->code, P->ncode, pc);
            return rx_repeat(P, pc + 1, endp + 1, I->a1, I->a2, I->a3, 0,
                             S, len, pos, cx);
        }
        case RXOP_ENDREP: return (pc == stop) ? 1 : 0;
        case RXOP_BACKREF: {
            int g = I->a1;
            if (g < 0 || g >= P->ngroups) return 0;
            const rgcap* rc = &cx->caps[g];
            if (rc->len < 0 || rc->start < 0) return 0;
            if (rc->start + rc->len > len) return 0;
            if (*pos + rc->len > len) return 0;
            int ic = I->a2 & RX_ICASE;
            for (int k = 0; k < rc->len; k++) {
                unsigned a = (unsigned char)S[rc->start + k];
                unsigned bb = (unsigned char)S[*pos + k];
                if (!rx_bytematch(a, bb, ic)) return 0;
            }
            *pos += rc->len;
            pc++; break;
        }
        case RXOP_ASSERT: {
            int kind = I->a2;
            int sub = I->a1;
            int cont = I->a3;
            int pos0 = *pos;
            rgcap sav[RX_NG(cx)];
            memcpy(sav, cx->caps, RX_NG(cx) * sizeof(rgcap));
            int matched = 0;
            if (kind == AS_BPOS || kind == AS_BNEG) {
                /* 后瞻：试 [0,pos0] 每个起点，子模式须收口于 pos0（结束于当前位）。
                 * 天然覆盖定长与变长后瞻（对齐 .NET 7+ backtracking 变长后瞻），
                 * 避免对编译产物的指令长度做不可靠的静态分析。 */
                for (int st = 0; st <= pos0; st++) {
                    int p = st;
                    if (rx_match_body(P, sub, 0, S, len, &p, cx) && p == pos0) {
                        matched = 1; break;
                    }
                    if (cx->aborted) {
                        memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
                        return 0;
                    }
                }
                if (!matched) memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
            } else {
                int p = pos0;
                if (rx_match_body(P, sub, 0, S, len, &p, cx)) matched = 1;
                if (cx->aborted) return 0;
                if (!matched) memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
            }
            int take = (kind == AS_POS) ? matched :
                       (kind == AS_NEG) ? !matched :
                       (kind == AS_BPOS) ? matched :
                       (kind == AS_BNEG) ? !matched : 0;
            if (take) {
                *pos = pos0;                 /* 零宽 */
                pc = cont;
                break;
            } else {
                *pos = pos0;
                memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
                return 0;
            }
        }
        case RXOP_ATOMIC: {
            int savep = *pos;
            rgcap sav[RX_NG(cx)];
            memcpy(sav, cx->caps, RX_NG(cx) * sizeof(rgcap));
            if (rx_match_body(P, pc + 1, I->a1, S, len, pos, cx)) {
                pc = I->a1;                  /* 提交，不回溯 */
                break;
            }
            if (cx->aborted) return 0;
            *pos = savep;
            memcpy(cx->caps, sav, RX_NG(cx) * sizeof(rgcap));
            return 0;
        }
        case RXOP_FAIL:
        default:
            return 0;
        }
    }
}

/* 在 [from, len] 内从 from 起找首个匹配；命中填 caps[0]=整段、返回匹配区间。 */
static int rx_run_from(const rxprog* P, const char* S, int len, int from,
                       int* ms, int* me, rxcx* cx) {
    for (int start = from; start <= len; start++) {
        cx->steps = 0;
        int pos = start;
        if (cx->aborted) return 0;
        /* stop=-1：顶层无命中边界，只有 RXOP_MATCH 才算成功（否则 pc=0 与 stop=0 立即假成功）。 */
        if (rx_match_body(P, 0, -1, S, len, &pos, cx)) {
            if (cx->aborted) return 0;
            *ms = start;
            *me = pos;
            cx->caps[0].start = start;
            cx->caps[0].len = pos - start;
            return 1;
        }
        if (cx->aborted) return 0;
    }
    return 0;
}

static rxcx rx_make_cx(const rxprog* P) {
    rxcx cx;
    memset(&cx, 0, sizeof(cx));
    cx.p = P;
    cx.caps = (rgcap*)calloc(RX_NG((rxcx*)&cx), sizeof(rgcap));
    cx.limit = RX_STEP_LIMIT;
    return cx;
}

static void rx_patch_unresolved(rxprog* P) {
    for (int i = 0; i < P->nun; i++) {
        int g = rx_lookup_name(P, P->un_name[i], P->un_plen[i]);
        if (g >= 0 && P->un_pc[i] >= 0 && P->un_pc[i] < P->ncode)
            P->code[P->un_pc[i]].a1 = g;
    }
}

/* 编译：pattern + 编译期 options → 程序。失败返回 NULL（含变长后瞻等诚实拒配时
 * 也可返回程序，由 ASSERT a4=-1 运行时拒配）。 */
static rxprog* rx_compile(const char* pattern, int options) {
    if (!pattern) pattern = "";
    rxprog* P = (rxprog*)calloc(1, sizeof(rxprog));
    if (!P) return NULL;
    rxparser rp;
    memset(&rp, 0, sizeof(rp));
    rp.p = P;
    rp.pat = pattern;
    rp.plen = (int)strlen(pattern);
    rp.flags = options;
    rp.ngrp = 1;                       /* 整段为 group 0 */
    rx_emit(&rp, RXOP_SAVE, 0, 0, 0, 0);
    rx_altrx((rxparser*)&rp);
    rx_emit(&rp, RXOP_MATCH, 0, 0, 0, 0);
    P->ngroups = rp.ngrp;
    rx_patch_unresolved(P);
    /* 空输入也无法匹配的非法/空模式：仍返回程序，运行时天然无匹配。 */
    return P;
}

static void rx_free_cx(rxcx* cx) { free(cx->caps); }

/* 展开替换串：$0..$9 / $$。 */
typedef struct { char* data; size_t cap, len; } rxgrow;
static void rx_grow_append(rxgrow* w, const char* s, size_t n) {
    if (!s) return;
    if (w->len + n + 1 > w->cap) {
        size_t nc = w->cap ? w->cap * 2 : 32;
        while (nc < w->len + n + 1) nc *= 2;
        char* nd = (char*)realloc(w->data, nc);
        if (!nd) return;
        w->data = nd; w->cap = nc;
    }
    memcpy(w->data + w->len, s, n);
    w->len += n;
    w->data[w->len] = '\0';
}

static void rx_expand_replacement(const char* repl, size_t rl,
                                  const char* S, const rgcap* caps, int ng,
                                  rxgrow* w) {
    for (size_t k = 0; k < rl;) {
        if (repl[k] == '$' && k + 1 < rl) {
            char n = repl[k + 1];
            if (n == '$') { rx_grow_append(w, "$", 1); k += 2; continue; }
            if (n >= '0' && n <= '9') {
                int g = n - '0';
                if (g < ng && caps[g].len > 0 && caps[g].start >= 0)
                    rx_grow_append(w, S + caps[g].start, (size_t)caps[g].len);
                k += 2;
                continue;
            }
        }
        rx_grow_append(w, repl + k, 1);
        k++;
    }
}

/* ---- 公开 ABI --------------------------------------------------------- */

static int rx_is_match_impl(const char* pattern, const char* input, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return 0;
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    rxcx cx = rx_make_cx(P);
    int r = 0;
    {
        int ms, me;
        r = rx_run_from(P, S, len, 0, &ms, &me, &cx);
        (void)ms; (void)me;
    }
    rx_free_cx(&cx);
    rx_free_prog(P);
    return r;
}

static char* rx_match_impl(const char* pattern, const char* input, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return rx_dup_n("", 0);
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    rxcx cx = rx_make_cx(P);
    char* out;
    {
        int ms, me;
        if (rx_run_from(P, S, len, 0, &ms, &me, &cx))
            out = rx_dup_n(S + cx.caps[0].start, (size_t)cx.caps[0].len);
        else
            out = rx_dup_n("", 0);
    }
    rx_free_cx(&cx);
    rx_free_prog(P);
    return out;
}

static char* rx_match_group_impl(const char* pattern, const char* input,
                                 int group, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return rx_dup_n("", 0);
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    rxcx cx = rx_make_cx(P);
    char* out;
    {
        int ms, me;
        int r = rx_run_from(P, S, len, 0, &ms, &me, &cx);
        if (r && group >= 0 && group < P->ngroups && cx.caps[group].len > 0 &&
            cx.caps[group].start >= 0)
            out = rx_dup_n(S + cx.caps[group].start, (size_t)cx.caps[group].len);
        else
            out = rx_dup_n("", 0);
    }
    rx_free_cx(&cx);
    rx_free_prog(P);
    return out;
}

static void* rx_matches_impl(const char* pattern, const char* input, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return rt_array_create(0, (int32_t)sizeof(char*));
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    rxcx cx = rx_make_cx(P);
    int ms, me;
    int count = 0, pos = 0;
    while (pos <= len && !cx.aborted) {
        if (!rx_run_from(P, S, len, pos, &ms, &me, &cx)) break;
        count++;
        pos = (me > ms) ? me : ms + 1;
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) { rx_free_cx(&cx); rx_free_prog(P); return rt_array_create(0, (int32_t)sizeof(char*)); }
    char** items = (char**)arr;
    pos = 0; int idx = 0;
    while (pos <= len && !cx.aborted) {
        if (!rx_run_from(P, S, len, pos, &ms, &me, &cx)) break;
        items[idx++] = rx_dup_n(S + ms, (size_t)(me - ms));
        pos = (me > ms) ? me : ms + 1;
    }
    rx_free_cx(&cx);
    rx_free_prog(P);
    return arr;
}

static char* rx_replace_impl(const char* pattern, const char* input,
                             const char* replacement, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return rx_dup_n(input ? input : "", input ? strlen(input) : 0);
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    const char* repl = replacement ? replacement : "";
    size_t rl = strlen(repl);
    rxcx cx = rx_make_cx(P);
    int ms, me;
    rxgrow w = { NULL, 0, 0 };
    int pos = 0;
    while (pos <= len && !cx.aborted) {
        if (!rx_run_from(P, S, len, pos, &ms, &me, &cx)) break;
        rx_grow_append(&w, S + pos, (size_t)(ms - pos));
        rx_expand_replacement(repl, rl, S, cx.caps, P->ngroups, &w);
        pos = (me > ms) ? me : ms + 1;
    }
    rx_grow_append(&w, S + pos, (size_t)(len - pos));
    char* out = w.data ? w.data : rx_dup_n("", 0);
    rx_free_cx(&cx);
    rx_free_prog(P);
    return out;
}

static void* rx_split_impl(const char* pattern, const char* input, int options) {
    rxprog* P = rx_compile(pattern, options);
    if (!P) return rt_array_create(0, (int32_t)sizeof(char*));
    const char* S = input ? input : "";
    int len = (int)strlen(S);
    rxcx cx = rx_make_cx(P);
    int ms, me;
    int count = 1, pos = 0;
    while (pos <= len && !cx.aborted) {
        if (!rx_run_from(P, S, len, pos, &ms, &me, &cx)) break;
        count++;
        pos = (me > ms) ? me : ms + 1;
    }
    void* arr = rt_array_create(count, (int32_t)sizeof(char*));
    if (!arr) { rx_free_cx(&cx); rx_free_prog(P); return rt_array_create(0, (int32_t)sizeof(char*)); }
    char** items = (char**)arr;
    pos = 0; int idx = 0;
    while (pos <= len && !cx.aborted) {
        if (!rx_run_from(P, S, len, pos, &ms, &me, &cx)) break;
        items[idx++] = rx_dup_n(S + pos, (size_t)(ms - pos));
        pos = (me > ms) ? me : ms + 1;
    }
    items[idx] = rx_dup_n(S + pos, (size_t)(len - pos));
    rx_free_cx(&cx);
    rx_free_prog(P);
    return arr;
}

/* ---- 无 options（兼容旧 ABI，等价 options=0） ------------------------ */

int32_t rt_regex_is_match(const char* pattern, const char* input) {
    return rx_is_match_impl(pattern, input, 0);
}
char* rt_regex_match(const char* pattern, const char* input) {
    return rx_match_impl(pattern, input, 0);
}
char* rt_regex_match_group(const char* pattern, const char* input, int32_t group) {
    return rx_match_group_impl(pattern, input, group, 0);
}
void* rt_regex_matches(const char* pattern, const char* input) {
    return rx_matches_impl(pattern, input, 0);
}
char* rt_regex_replace(const char* pattern, const char* input, const char* replacement) {
    return rx_replace_impl(pattern, input, replacement, 0);
}
void* rt_regex_split(const char* pattern, const char* input) {
    return rx_split_impl(pattern, input, 0);
}

/* ---- options ABI（RegexOptions） ------------------------------------- */

int32_t rt_regex_is_match_opt(const char* pattern, const char* input, int32_t options) {
    return rx_is_match_impl(pattern, input, (int)options);
}
char* rt_regex_match_opt(const char* pattern, const char* input, int32_t options) {
    return rx_match_impl(pattern, input, (int)options);
}
char* rt_regex_match_group_opt(const char* pattern, const char* input,
                               int32_t group, int32_t options) {
    return rx_match_group_impl(pattern, input, group, (int)options);
}
void* rt_regex_matches_opt(const char* pattern, const char* input, int32_t options) {
    return rx_matches_impl(pattern, input, (int)options);
}
char* rt_regex_replace_opt(const char* pattern, const char* input,
                           const char* replacement, int32_t options) {
    return rx_replace_impl(pattern, input, replacement, (int)options);
}
void* rt_regex_split_opt(const char* pattern, const char* input, int32_t options) {
    return rx_split_impl(pattern, input, (int)options);
}