#!/usr/bin/env node
'use strict'

// ============================================================================
// Arc 规范守卫 CLI —— 规则引擎的项目资产版（跟随仓库版本化，跨会话/CI 可用）
// ============================================================================
// 与 DSH 动态插件 arcs-1（pkg-8）规则集保持同步维护：改动任一侧时须同步另一侧。
//
// 2026-08-19 规则校准（全库门禁清偿 · 同步 arcs-1）：
// - 测试文件豁免族：as-snake-member / as-async-suffix / as-acronym-case 对
//   examples/UnitTest*、crates/*/tests/、*Tests.as/*E2e.as 豁免——`Method_Scenario`
//   测试命名是 .NET 惯例，非 API 面；
// - as-file-type-name：Program.as 入口、parse/fixtures 夹具、*Models/*Types/*Enums/
//   *Generators 聚合文件、I 前缀接口、同族聚合（全部类型共享文件名前缀）豁免；
// - as-case-braces：parse/fixtures 夹具豁免（golden 最小化写法）；
// - rs-file-too-large 分级：tests/ 2000/1000；src 4000(error)/1200(warning)/200(warning)
//   ——编译器核心巨型文件（lower.rs 6298 行等）是单一职责巨型实现，拆分须按模块
//   边界走专项（plan.md 巨型文件拆分专项登记），禁机械切；
// - hygieneScan 递归化（SCAN_ROOTS 全树按扩展名检产物）+ --all 全库扫描模式。
//
// 用法：
//   node spec-guard.cjs check [paths...] [--quick] [--no-hygiene] [--no-layout] [--json]
//   node spec-guard.cjs check --all [--quick] [--no-hygiene] [--no-layout] [--json]
//   node spec-guard.cjs rules
//
// 退出码：error 级违规数 > 0 → 1（门禁失败）；否则 0。
// 无 paths 时自动采用 `git status --porcelain` 检出的变更文件。
// --all 全库扫描：显式扫描 std/crates/docs/scripts/examples 全部检查面（CI 门禁
// 模式——CI checkout 后 git 无变更，变更文件模式会退化为仅卫生/布局扫描，
// 规则引擎零执行；--all 保证编码契约规则在 CI 上真实生效）。
// ============================================================================

const fs = require('fs')
const path = require('path')
const { execFileSync } = require('child_process')

// ---------- 常量（与插件同步） ----------
const SEV_ERROR = 'error'
const SEV_WARNING = 'warning'
const SEV_INFO = 'info'
const CAT_AS = '编码契约'
const CAT_RS = 'Rust 模块化'
const CAT_UI = 'UI 铁律'
const CAT_DOC = '文档'
const CAT_LAYOUT = '目录结构'
const CAT_HYG = '结构卫生'
const EXEMPTIONS = [
  { id: 'as-enum-c-style', file: 'std/Arc/Diagnostics/PosixSignal.as', reason: 'POSIX 标准信号名（SIGHUP/SIGINT/...）为外部标准命名，豁免 C 风格枚举规则' },
  { id: 'as-enum-c-style', file: 'std/Net/Core/Http/HttpMethod.as', reason: 'HTTP 标准方法名（GET/POST/...，RFC 9110）为外部标准命名，豁免 C 风格枚举规则' },
  { id: 'as-async-suffix', file: 'std/Arc/Tasks/Task.as', reason: '.NET Task 兼容 API 面（FromResult/WhenAll/WhenAny/ContinueWith 等按 C# 惯例无 Async 后缀）' },
  { id: 'as-async-suffix', file: 'std/Arc/Collections/AsyncStream.as', reason: '异步流 API 面（MoveNextCore 为 .NET 异步枚举器惯用内部名）' },
  { id: 'as-async-suffix', file: 'std/Arc/Threading/ThreadPoolScheduler.as', reason: '.NET Task.Run 兼容 API 面（Run(Action)/Run<T>(Func<T>)）' },
  { id: 'as-c-style-const', file: 'std/Net/Core/WebSocket/WebSocketClient.as', reason: 'RFC 6455 帧常量与固定 GUID（OP_TEXT/OP_CLOSE/WS_GUID 等外部标准命名）' },
  { id: 'as-enum-c-style', file: 'std/Net/P2P/Cid.as', reason: 'multihash/multicodec 标准表（SHA256/SHA512/RAW 等外部标准命名）' },
  { id: 'as-enum-c-style', file: 'std/Net/P2P/Multiaddr.as', reason: 'multiaddr 协议表（IP4/IP6/TCP/UDP 等外部标准命名）' },
  { id: 'as-enum-c-style', file: 'std/Net/P2P/P2PMessage.as', reason: 'Kademlia/KAD 协议消息码（KADRPC 等外部标准命名）' },
  { id: 'as-snake-member', file: 'std/Security/SHA3_256.as', reason: 'NIST 标准算法名 SHA3-256（连字符转下划线的类型名）' },
  { id: 'as-snake-member', file: 'std/Security/SHA3_512.as', reason: 'NIST 标准算法名 SHA3-512（连字符转下划线的类型名）' },
  { id: 'as-c-style-const', file: 'std/Net/Core/Http/Http2/HuffmanCodec.as', reason: 'RFC 7541 静态 Huffman 码表（EOS 等标准终结符命名）' },
  { id: 'ui-stub-render', file: 'crates/runtime-ui/platform/common/rt_ui_image_stub.c', reason: '跨平台缺分支适配：Linux 无图像解码后端（真实实现 windows/rt_ui_image_win32.cpp 仅 Win32 编译），返回 failed 位图（loaded=0）为诚实失败路径，非渲染实现载体' },
  { id: 'ui-stub-render', file: 'crates/runtime-ui/platform/common/rt_ui_scrollbar_stub.c', reason: '跨平台缺分支适配：非 Win32（Linux/macOS/OHOS）竖滚动条仅布局、无滚动条 UI（真实实现 windows/rt_ui_scrollbar.cpp），几何/命中原语零值返回，绘制由 Arc 侧 WgpuRender 完成，无任何光栅绘制' },
  { id: 'ui-stub-render', file: 'crates/runtime-ui/platform/ohos/window_stub.c', reason: '跨平台缺分支适配：OHOS 无窗口后端（RFC 037 M5-OHOS，桌面/OHOS/WASM 为后续目标），创建窗口返回 NULL、事件恒 CLOSE，无任何渲染上屏' }
]
const VALUE_TYPES = new Set(['void','int','long','short','byte','sbyte','uint','ulong','ushort','char','float','double','decimal','bool','nint','nuint'])
// 缩写大小写规律（.NET 命名规范）：2 字母缩写全大写（UI/IO/IP/AI），3+ 字母缩写 PascalCase 化（Http/Xml/Tcp/Quic/Cid/Nat）。
// 要点：排除英文单词（To/In/Or/Up/Ms/Id 是单词或单位后缀或 .NET 惯例，非缩写）；3+ 只匹配已知协议/技术缩写表，未知缩写静默。
const COMMON_ABBR_2 = new Set(['UI','IO','IP','OS','CD','AI','BI','CL','CV','DC','DR','DS','EC','ED','EP','ER','EV','EX','FM','FP','FS','FT','GI','GP','GR','GS','GT','IM','IT','IV','LI','LO','LR','LS','MD','MI','ML','MP','MR','MT','NP','NR','NS','NT','PA','PC','PD','PE','PF','PG','PH','PI','PL','PM','PN','PP','PR','PS','PT','PU','QU','RA','RC','RD','RE','RF','RG','RH','RI','RM','RN','RO','RP','RR','RS','RT','RU','SA','SB','SC','SD','SE','SF','SG','SH','SI','SL','SM','SN','SO','SP','SQ','SR','SS','ST','SU','SW','TA','TB','TC','TD','TE','TF','TG','TH','TI','TL','TM','TN','TP','TR','TS','TT','TU','TV','TW','UA','UC','UD','UE','UF','UG','UL','UM','UR','UT','VA','VB','VC','VD','VE','VF','VG','VI','VM','VP','VR','VS','VT','WA','WB','WC','WD','WF','WG','WI','WL','WM','WN','WP','WR','WS','WT'])
const KNOWN_3P_ABBR = { HTTP: 'Http', XML: 'Xml', HTML: 'Html', TCP: 'Tcp', UDP: 'Udp', QUIC: 'Quic', CID: 'Cid', NAT: 'Nat', FTP: 'Ftp', SMTP: 'Smtp', SSH: 'Ssh', SQL: 'Sql', JSON: 'Json', CSV: 'Csv', YAML: 'Yaml', PNG: 'Png', JPEG: 'Jpeg', GIF: 'Gif', SVG: 'Svg', PDF: 'Pdf', URL: 'Url', URI: 'Uri', UUID: 'Uuid', CLI: 'Cli', SDK: 'Sdk', TLS: 'Tls', SSL: 'Ssl', API: 'Api', DNS: 'Dns', TTS: 'Tts', OCR: 'Ocr', ASR: 'Asr', RPC: 'Rpc', MIME: 'Mime', CORS: 'Cors', XSS: 'Xss', WASM: 'Wasm', GRPC: 'Grpc', IPFS: 'Ipfs', SSH: 'Ssh', IDE: 'Ide' }
const FULL_UPPER_EXCEPT = new Set(['SHA','AES','RSA','DES','HMAC','ECDSA','X509','UTF8','UTF16','ASCII','P2P','E2E','SIMD','AVX','SSE','GPU','CPU','GUID','ISO','IEEE'])
const CORE_CHAIN = ['ast','parse','hir','typeck','mir','codegen','arc']
const SIDE_CRATES = ['arc-ui','arcgr','arc-ssr','arc-server','reachability','arc-tests','runtime','runtime-crypto','runtime-drawing','runtime-iree','runtime-onnx','runtime-quic','runtime-sqlite','runtime-ui']
const SKIP_DIRS = new Set(['.git','target','obj','bin','node_modules','.cursor','.github','.trae','.idea','.vscode','.dsh'])
const CHECK_EXTS = ['.as','.rs','.arml','.md','.c','.h','.toml']
// 全库扫描面（--all）：源码/文档/脚本/示例全集。单一事实源——hygieneScan 递归
// 扫描与 --all 收集共用此表，新增面只改此处（架构级，禁各扫描点各自维护根表）。
const SCAN_ROOTS = ['crates', 'std', 'docs', 'scripts', 'examples']
// 全库扫描文件上限（--all 模式；普通路径模式保持 400 防御截断）。
const ALL_MAX_FILES = 5000
const UI_PATH_RE = /(^|\/)(std\/UI\/|crates\/(runtime-ui|arc-ui|runtime-drawing)\/)|\.arml$/i
// ---------- 仓库根定位 ----------
function findRoot(startDir) {
  let dir = path.resolve(startDir)
  for (;;) {
    if (fs.existsSync(path.join(dir, 'AGENTS.md'))) return dir
    const parent = path.dirname(dir)
    if (parent === dir) return dir
    dir = parent
  }
}
const ROOT = findRoot(__dirname)

// ---------- fs 封装（Node 原生） ----------
function readRel(rel) {
  try { return fs.readFileSync(path.join(ROOT, rel), 'utf8') } catch (e) { return null }
}
function statRel(rel) {
  try {
    const st = fs.statSync(path.join(ROOT, rel))
    return { isDirectory: function () { return st.isDirectory() }, type: st.isDirectory() ? 'directory' : 'file' }
  } catch (e) { return undefined }
}
function listRel(rel) {
  try {
    const dir = path.join(ROOT, rel === '' ? '.' : rel)
    return fs.readdirSync(dir, { withFileTypes: true }).map(function (d) {
      return { name: d.name, type: d.isDirectory() ? 'directory' : 'file' }
    })
  } catch (e) { return null }
}
function isDir(info) {
  return !!(info && info.isDirectory())
}

// ---------- 文本掩码/注释提取（与插件同步） ----------
function V(line, message, sev) {
  return { line: line, message: message, sev: sev }
}
function maskLines(text) {
  const ls = text.split(/\r?\n/)
  let inBlock = false
  const out = []
  for (let n = 0; n < ls.length; n++) {
    let line = ls[n]
      .replace(/"(?:\\.|[^"\\])*"/g, function (m) { return ' '.repeat(m.length) })
      .replace(/'((?:\\.|[^'\\])*)'/g, function (m) { return ' '.repeat(m.length) })
    if (inBlock) {
      const ei = line.indexOf('*/')
      if (ei >= 0) { line = line.slice(0, ei + 2).replace(/[^\s]/g, ' ') + line.slice(ei + 2); inBlock = false }
      else { line = ' '.repeat(line.length) }
    } else {
      const ci = line.indexOf('//')
      const bi = line.indexOf('/*')
      if (bi >= 0 && (ci < 0 || bi < ci)) {
        const ei = line.indexOf('*/', bi + 2)
        if (ei >= 0) { line = line.slice(0, bi) + ' '.repeat(ei + 2 - bi) + line.slice(ei + 2) }
        else { line = line.slice(0, bi) + ' '.repeat(Math.max(0, line.length - bi)); inBlock = true }
      } else if (ci >= 0) {
        line = line.slice(0, ci) + ' '.repeat(Math.max(0, line.length - ci))
      }
    }
    out.push(line)
  }
  return out
}
function commentLines(text) {
  const ls = text.split(/\r?\n/)
  let inBlock = false
  const out = []
  for (let n = 0; n < ls.length; n++) {
    const line = ls[n].replace(/"(?:\\.|[^"\\])*"/g, '""')
    let comment = ''
    if (inBlock) {
      comment = line
      if (comment.indexOf('*/') >= 0) inBlock = false
    } else {
      const ci = line.indexOf('//')
      const bi = line.indexOf('/*')
      if (bi >= 0 && (ci < 0 || bi < ci)) {
        comment = line.slice(bi)
        if (comment.indexOf('*/') < 0) inBlock = true
      } else if (ci >= 0) {
        comment = line.slice(ci)
      }
    }
    out.push(comment)
  }
  return out
}
function blockLen(ls, i) {
  let depth = 0
  let j = i
  for (; j < ls.length; j++) {
    const line = ls[j].replace(/"(?:\\.|[^"\\])*"/g, '""')
    depth += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length
    if (depth <= 0 && j > i) break
  }
  return j - i + 1
}
function isExempt(id, rel) {
  for (let i = 0; i < EXEMPTIONS.length; i++) {
    if (EXEMPTIONS[i].id === id && EXEMPTIONS[i].file === rel) return true
  }
  return false
}

// ---------- 规则表（与插件 pkg-8 同步） ----------
const RULES = [
  { id: 'as-no-let-mut', rule: '声明禁 let/mut（RFC 002）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        const re = /\b(?:let|mut)\s+[A-Za-z_$][A-Za-z0-9_$]*/g
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '`' + m[0] + '` 禁止 let/mut：声明用前导类型 `Type name` 或 `var`（无 let/mut，RFC 002）'))
      }
      return out
    } },
  { id: 'as-no-expression-keyword', rule: '无 expression 关键字（RFC 002/012）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        if (/\bexpression\s*\(/.test(msk[i])) out.push(V(i + 1, '禁止 `expression` 关键字：表达式树用 `Expression<Func<...>>` + Lambda（RFC 012）'))
      }
      return out
    } },
  { id: 'as-c-style-const', rule: '常量 PascalCase（禁 C 风格）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /\b(?:public|private|protected|internal)?\s*const\s+[A-Za-z_][A-Za-z0-9_<>?.,\s]*?\s+([A-Z][A-Z0-9_]{2,})\s*(?:=|;)/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, 'C 风格常量 `' + m[1] + '` 禁止；常量用 PascalCase（如 MaxLength），禁 C 风格 MAX_LENGTH'))
      }
      return out
    } },
  { id: 'as-this-private-field', rule: '私有字段禁 this. 前缀', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        const re = /\bthis\.[_][A-Za-z][A-Za-z0-9_]*/g
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '`' + m[0] + '` 私有字段应裸访问（this. 仅用于与参数/局部变量冲突时消歧，且优先改用不同命名参数）'))
      }
      return out
    } },
  { id: 'as-knr-brace', rule: 'Allman 风格：{ 独立成行', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re1 = /^\s*(?:(?:else\s*)?(?:if|for|foreach|while|switch|do)\b[^{]*|else|catch\s*\([^)]*\)|finally)\s*\{\s*$/
      const re2 = /^\s*\}\s*(?:else|catch\s*\([^)]*\)|finally)\s*\{\s*$/
      for (let i = 0; i < msk.length; i++) {
        if (re1.test(msk[i]) || re2.test(msk[i])) out.push(V(i + 1, '控制流块 `{` 应独立成行（Allman；新增代码一律 Allman，历史 K&R 写法触及处随重构转换）'))
      }
      return out
    } },
  { id: 'as-missing-braces', rule: '控制流必须 {} 括起（禁省略）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const isHeader = /^(?:else\s+)?(?:if|for|foreach|while|switch)\b.*\)\s*$/
      const inline = /^(?:else\s+)?(?:if|for|foreach|while|switch)\b[^{]*\)\s*[^;{]*;\s*$/
      for (let i = 0; i < msk.length; i++) {
        const line = msk[i].trim()
        if (line.indexOf('{') >= 0) continue
        if (inline.test(line)) { out.push(V(i + 1, '控制流块必须使用 {} 括起（禁止省略大括号）')); continue }
        let header = isHeader.test(line) || /^else\s*$/.test(line)
        if (!header) continue
        // 跳过多行条件续行（&& / || 开头）与空行/注释；续行本身以 `{` 结尾视为已括起
        let found = false
        let j = i + 1
        while (j < msk.length) {
          const n = msk[j].trim()
          if (n === '') { j++; continue }
          if (/^(\/\/|\/\*|\*)/.test(n)) { j++; continue }
          if (/^[&|]{2}/.test(n)) {
            if (/\{\s*$/.test(n)) { found = true; break }
            j++
            continue
          }
          if (n.charAt(0) === '{') found = true
          break
        }
        if (!found) out.push(V(i + 1, '控制流块必须使用 {} 括起（Allman：`{` 独立成行；禁止省略大括号）'))
      }
      return out
    } },
  { id: 'as-case-braces', rule: 'switch case 分支体必须 {} 括起', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt, rel) {
      const out = []
      // 解析器测试夹具（parse/fixtures/）豁免：夹具是 token/语法形状快照，case 裸语句
      // 是刻意的最小化写法（golden 文件），不适用生产编码契约。
      if (/\/fixtures\//i.test(rel || '')) return out
      for (let i = 0; i < msk.length; i++) {
        const line = msk[i].trim()
        if (!/^(?:case\s+.+:|default:)\s*$/.test(line)) continue
        let j = i + 1
        while (j < msk.length) {
          const n = msk[j].trim()
          if (n === '') { j++; continue }
          break
        }
        const n = j < msk.length ? msk[j].trim() : ''
        if (n && n.charAt(0) !== '{' && !/^case\b/.test(n) && !/^default:/.test(n)) out.push(V(i + 1, 'case/default 分支体必须用 {} 括起（switch 每个 case/default 分支体都须 {}，Allman）'))
      }
      return out
    } },
  { id: 'as-redundant-init', rule: '自动属性初值禁冗余', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /\{\s*get\s*;\s*(?:(?:private|protected|internal)\s+)?set\s*;\s*\}\s*=\s*(?:0|0\.0|false|null)\s*;|\{\s*get\s*;\s*\}\s*=\s*(?:0|0\.0|false|null)\s*;/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        if (re.test(msk[i])) out.push(V(i + 1, '冗余初值：`= expr` 等价于类型默认值（0/false/null）属噪声，一律省略（getter-only `{ get; }` 默认）'))
      }
      return out
    } },
  { id: 'as-async-suffix', rule: 'Task 方法必须 Async 后缀', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt, rel) {
      const out = []
      // 测试文件豁免：`AsyncFact_Scenario` 类测试方法名语义 = 场景描述（NUnit/xUnit
      // 惯例），非 API 面，不适用 Async 后缀硬规则。
      const lower = (rel || '').toLowerCase()
      const isTestFile = /^examples\/unittest/.test(lower) || /\/tests\//.test(lower) || /(?:tests?|e2e)\.as$/.test(lower)
      if (isTestFile) return out
      const re = /(?:public|private|protected|internal)\s+(?:(?:static|virtual|override|sealed|new|unsafe|readonly|partial)\s+)*(?:async\s+)?Task(?:<[^;\n()]*>)?\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) {
          const name = m[1]
          if (name !== 'Main' && !/Async$/.test(name)) out.push(V(i + 1, '返回 Task/Task<T> 的方法 `' + name + '` 必须以 Async 结尾（异步一体原则；无 I/O 纯状态操作才同步命名）'))
        }
      }
      return out
    } },
  { id: 'as-cancellation-token', rule: 'async 方法须 CancellationToken', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /(?:public|private|protected|internal)\s+(?:(?:static|virtual|override|sealed|new|unsafe|readonly|partial)\s+)*(?:async\s+)?Task(?:<[^;\n()]*>)?\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)/g
      const byName = {}
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) {
          const e = byName[m[1]] || { line: i + 1, hasToken: false }
          if (/cancellationToken|CancellationToken/.test(m[2])) e.hasToken = true
          byName[m[1]] = e
        }
      }
      const keys = Object.keys(byName)
      for (let k = 0; k < keys.length; k++) {
        if (!byName[keys[k]].hasToken) out.push(V(byName[keys[k]].line, 'async 方法 `' + keys[k] + '` 须提供 CancellationToken（参数或 M1 重载模式）'))
      }
      return out
    } },
  { id: 'as-nullable-return', rule: '可空返回显式 ? 标注', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const sigRe = /(?:public|private|protected|internal)\s+(?:(?:static|virtual|override|sealed|async|new|readonly)\s+)*(?:async\s+)?([A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/
      const noName = /^(var|new|return|if|else|for|foreach|while|switch|do|case|break|continue|throw|using|lock|try|catch|finally|get|set|await|async|out|ref|in|void|yield|base|this|sizeof|typeof|nameof|default|true|false|null|override)$/
      for (let i = 0; i < msk.length; i++) {
        const m = sigRe.exec(msk[i])
        if (!m) continue
        const ret = m[1].trim()
        const name = m[2]
        if (!ret || noName.test(ret)) continue
        if (ret.charAt(ret.length - 1) === '?' || VALUE_TYPES.has(ret)) continue
        if (/^Task\b/.test(ret) || /^[a-z]/.test(ret)) continue
        let depth = 0
        let found = false
        for (let j = i; j < msk.length; j++) {
          depth += (msk[j].match(/\{/g) || []).length - (msk[j].match(/\}/g) || []).length
          if (j > i && depth <= 0) break
          if (depth >= 1 && /return\s+null\s*;/.test(msk[j])) { found = true; break }
        }
        if (found) out.push(V(i + 1, '方法 `' + name + '` 返回引用类型 `' + ret + '`（无 `?` 标注）却含 `return null;` —— 必须显式 `?` 标注并说明 null 语义（std 禁止不明确可空）'))
      }
      return out
    } },
  { id: 'as-todo-hard', rule: '禁止 TODO/FIXME 占位注释', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt) {
      const out = []
      for (let i = 0; i < cmt.length; i++) {
        // 仅命中实际占位标记（TODO:/FIXME: 带冒号）；描述性提及（如「TODO 标记」说明）不误报
        if (/TODO\s*[:：]|FIXME\s*[:：]/.test(cmt[i])) out.push(V(i + 1, '禁止 TODO/FIXME 占位注释（注释面向开发者说明「为什么」，随代码同变更集维护）'))
      }
      return out
    } },
  { id: 'as-todo-soft', rule: '禁止占位/易失效/过时注释', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt) {
      const out = []
      for (let i = 0; i < cmt.length; i++) {
        if (/占位|以后(?:要|再)?改|待优化|暂不实现|过时说明/.test(cmt[i])) out.push(V(i + 1, '占位/易失效/过时注释应删除（写「为什么」，不写「以后改」）'))
      }
      return out
    } },
  { id: 'as-acronym-case', rule: '缩写大小写规律（2 字母全大写，3+ 字母 PascalCase）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt, rel) {
      const out = []
      // 测试文件豁免（同 as-snake-member：测试命名语义 = 场景描述，非 API 面）。
      const lower = (rel || '').toLowerCase()
      const isTestFile = /^examples\/unittest/.test(lower) || /\/tests\//.test(lower) || /(?:tests?|e2e)\.as$/.test(lower)
      if (isTestFile) return out
      const nameRe = /(?:public|internal)\s+(?:(?:sealed|static|abstract|partial|readonly|ref)\s+)*(?:class|struct|interface|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)|(?:public|internal)\s+(?:(?:static|virtual|override|sealed|async|new|unsafe|readonly|partial)\s+)*(?:[A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:\(|\{)/g
      for (let i = 0; i < msk.length; i++) {
        nameRe.lastIndex = 0
        let m
        while ((m = nameRe.exec(msk[i]))) {
          const name = m[1] || m[2]
          if (!name) continue
          if (name.indexOf('_') >= 0) continue // 下划线命名由 snake-member/c-style-const 管，避免重复
          // 1) 已知 3+ 字母协议/技术缩写全大写 → PascalCase 化（HTTP→Http、TCP→Tcp、QUIC→Quic、CID→Cid）
          if (![...FULL_UPPER_EXCEPT].some(function (e) { return name.indexOf(e) >= 0 })) {
            const keys = Object.keys(KNOWN_3P_ABBR)
            for (let k = 0; k < keys.length; k++) {
              if (name.indexOf(keys[k]) >= 0) {
                out.push(V(i + 1, '缩写 `' + keys[k] + '` 应 PascalCase 化：' + keys[k] + ' → ' + KNOWN_3P_ABBR[keys[k]] + '（3+ 字母缩写仅首字母大写，如 .NET 的 HttpClient/TcpClient/Uri）—— `' + name + '`'))
              }
            }
          }
          // 2) 2 字母缩写写成小写形式（Ui → UI）→ 全大写
          const words = name.split(/(?=[A-Z])/)
          for (let w = 0; w < words.length; w++) {
            const word = words[w]
            if (word.length === 2 && word.charAt(0) >= 'A' && word.charAt(0) <= 'Z' && word.charAt(1) >= 'a' && word.charAt(1) <= 'z') {
              const up = word.toUpperCase()
              if (COMMON_ABBR_2.has(up)) out.push(V(i + 1, '缩写 `' + up + '` 应全大写：' + word + ' → ' + up + '（2 字母缩写全大写惯例，如 UIElement）—— `' + name + '`'))
            }
          }
        }
      }
      return out
    } },
  { id: 'as-file-type-name', rule: '文件与主类型同名', severity: SEV_ERROR, scope: 'as', category: CAT_LAYOUT,
    fn: function (msk, cmt, rel, fileName) {
      // 生成文件（*.g.as）命名由生成器约定，豁免；partial 拆分（WgpuRender.Capture.as）stem 取点前主类型段
      if (/\.g\.as$/i.test(fileName)) return []
      // 入口文件 Program.as 为 CLI 约定名（对标 C# Main 入口），fixtures 为解析器测试
      // 夹具（非产物源码、无主类型约定）——均豁免。
      if (/^program\.as$/i.test(fileName)) return []
      if (/\/fixtures\//i.test(rel || '')) return []
      // 聚合文件豁免：`*Models.as`（AI 门面请求/响应聚合）、`*Types.as`/`*Enums.as`
      // （协议类型表聚合）、`*Generators.as`（代码生成器族）等复数聚合词文件为
      // **有意聚合**（单文件多类型是设计意图，如 AIOcrModels 聚合 Request/Result/Line），
      // 主类型同名与单公开类型检查均不适用；文件名即聚合语义。
      if (/(?:models|types|enums|generators|invocation)\.as$/i.test(fileName)) return []
      const stem = fileName.replace(/\.as$/i, '').split('.')[0]
      const normAbbr = function (s) { return s.replace(/([A-Z]{3,})/g, function (mm) { return mm.charAt(0) + mm.slice(1).toLowerCase() }) }
      const out = []
      let publicCount = 0
      const names = []
      const typeRe = /(?:public\s+|internal\s+)?(?:(?:sealed|static|abstract|partial|readonly|ref)\s+)*(?:class|struct|interface|enum|record)\s+([A-Za-z_][A-Za-z0-9_]*)/g
      for (let i = 0; i < msk.length; i++) {
        typeRe.lastIndex = 0
        let m
        while ((m = typeRe.exec(msk[i]))) {
          names.push(m[1])
          if (/public\s+/.test(msk[i].slice(0, m.index))) publicCount++
        }
      }
      if (names.length && names.indexOf(stem) < 0) {
        // 接口惯例豁免：`IConsole` 可存于 `Console.as` 或 `IConsole.as`；接口名 = 'I' + stem。
        const ifaceOk = names.some(function (t) { return t === 'I' + stem || t === stem })
        // 同族聚合豁免：文件内全部类型共享公共前缀（协议族/域聚合）——
        // ① stem 前缀：Yamux.as 的 YamuxConst/Codec/Stream/Session（类型名以文件名开头）；
        // ② 公共前缀：AIGeometry.as 的 AIRect/AIPoint 共享 'AI' 前缀、文件名 = AI + 域概念
        //    （Geometry 几何域）——域聚合是设计意图（C# 域聚合惯例），非命名违规。
        const allShareStem = names.length >= 2 && names.every(function (t) { return t.indexOf(stem) === 0 })
        let commonPrefix = names[0] || ''
        for (let i = 1; i < names.length; i++) {
          let j = 0
          while (j < commonPrefix.length && j < names[i].length && commonPrefix.charAt(j) === names[i].charAt(j)) j++
          commonPrefix = commonPrefix.slice(0, j)
        }
        const domainAggregate = names.length >= 2 && commonPrefix.length >= 2 &&
          commonPrefix === stem.slice(0, commonPrefix.length)
        if (!ifaceOk && !allShareStem && !domainAggregate) {
          // 缩写大小写差异（CID.as ↔ class Cid）给出规律提示；否则建议正确文件名
          const hint = normAbbr(stem) === normAbbr(names[0]) || stem.toLowerCase() === names[0].toLowerCase()
            ? '（缩写大小写规律：3+ 字母缩写 PascalCase 化，如 CID → Cid、TCP → Tcp；2 字母缩写全大写，如 UI → UI）'
            : ('（建议：' + names[0] + '.as）')
          out.push(V(1, '文件名须与主类型同名：' + fileName + ' ↔ ' + names[0] + hint))
        }
      }
      if (publicCount > 1) out.push(V(1, '单文件仅一个公开类型（当前 ' + publicCount + ' 个：' + names.join(', ') + '）；接口实现类归 Impl/，POCO 归 Models/'))
      return out
    } },
  { id: 'as-name-pascal', rule: '方法/属性命名 PascalCase', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const methRe = /(?:public|internal)\s+(?:(?:static|virtual|override|sealed|async|new|unsafe|readonly|partial)\s+)*(?:[A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+([a-z][A-Za-z0-9_]*)\s*\(/g
      const propRe = /(?:public|internal)\s+(?:(?:static|virtual|override|sealed|readonly)\s+)*(?:[A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+([a-z][A-Za-z0-9_]*)\s*\{\s*(?:get|set)\b/g
      for (let i = 0; i < msk.length; i++) {
        methRe.lastIndex = 0
        let m
        while ((m = methRe.exec(msk[i]))) out.push(V(i + 1, '方法名 `' + m[1] + '` 必须 PascalCase（参数/局部变量才是 camelCase）'))
        propRe.lastIndex = 0
        while ((m = propRe.exec(msk[i]))) out.push(V(i + 1, '属性名 `' + m[1] + '` 必须 PascalCase'))
      }
      return out
    } },
  { id: 'as-underscore-upper', rule: '私有字段 _camelCase', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /(?:private|protected|internal)\s+(?:(?:static|readonly|const|volatile)\s+)*(?:[A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+(_[A-Z][A-Za-z0-9_]*)\s*(?:=|;|\{)/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '私有字段 `' + m[1] + '` 须 _camelCase（禁止 _PascalCase）'))
      }
      return out
    } },
  { id: 'as-interface-prefix', rule: '接口 I 前缀', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /\b(?:public|internal)\s+interface\s+([A-Z][A-Za-z0-9_]*)/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) {
          if (m[1].charAt(0) !== 'I') out.push(V(i + 1, '接口 `' + m[1] + '` 必须 I 前缀（IRepository）'))
        }
      }
      return out
    } },
  { id: 'as-trivial-accessor', rule: '琐碎访问器应改自动属性', severity: SEV_WARNING, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      const re = /(?:get|set)\s*\{\s*return\s+_[A-Za-z][A-Za-z0-9_]*\s*;\s*\}|set\s*\{\s*_[A-Za-z][A-Za-z0-9_]*\s*=\s*value\s*;\s*\}/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '琐碎访问器应改用自动属性 `{ get; }`/`{ get; private set; }`（自定义访问器仅用于复杂逻辑；新增代码禁止退回原始写法，[Builtin] 死代码体除外）'))
      }
      for (let i = 0; i < msk.length; i++) {
        const t = msk[i].trim()
        if (!/^(?:get|set)\s*\{\s*$|^(?:get|set)\s*$/.test(t)) continue
        let depth = 1
        let stmts = 0
        let bad = false
        for (let j = i + 1; j < msk.length; j++) {
          const line = msk[j].trim()
          if (line === '') continue
          depth += (line.match(/\{/g) || []).length - (line.match(/\}/g) || []).length
          if (line !== '{' && line !== '}' && !/^\/\//.test(line)) {
            stmts++
            if (!/^(?:return\s+_[A-Za-z]\w*\s*;|_[A-Za-z]\w*\s*=\s*value\s*;)$/.test(line)) bad = true
          }
          if (depth <= 0) break
        }
        if (stmts === 1 && !bad) out.push(V(i + 1, '琐碎访问器应改用自动属性 `{ get; }`/`{ get; private set; }`（新增代码禁止退回原始写法，[Builtin] 死代码体除外）'))
      }
      return out
    } },
  { id: 'as-snake-member', rule: '成员命名禁下划线（PascalCase）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk, cmt, rel, fileName) {
      const out = []
      // 测试文件豁免：`Method_Scenario_Expected` 是 .NET 测试命名惯例（NUnit/xUnit），
      // 测试方法名语义 = 场景描述，非 API 面，不适用 PascalCase 硬规则。
      // 判定：路径在 examples/UnitTest*/ 或 crates/*/tests/ 下，或文件名 *Tests.as /
      // *E2e.as / *Test.as（e2e 探针程序）。
      const lower = (rel || '').toLowerCase()
      const isTestFile = /^examples\/unittest/.test(lower) || /\/tests\//.test(lower) || /(?:tests?|e2e)\.as$/.test(lower)
      if (isTestFile) return out
      const re = /(?:public|internal)\s+(?:(?:static|virtual|override|sealed|async|new|unsafe|readonly|partial)\s+)*(?:[A-Za-z_][A-Za-z0-9_<>?.,\[\]\s]*?)\s+([A-Za-z][A-Za-z0-9_]*_[A-Za-z0-9_]+)\s*(?:\(|\{)/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '成员 `' + m[1] + '` 含下划线 —— 类型/方法/属性/常量一律 PascalCase（禁 snake_case 中缀，如 Do_thing）'))
      }
      return out
    } },
  { id: 'as-enum-c-style', rule: '枚举成员 PascalCase（禁 C 风格）', severity: SEV_ERROR, scope: 'as', category: CAT_AS,
    fn: function (msk) {
      const out = []
      let inEnum = false
      const re = /^\s*([A-Z][A-Z0-9_]{2,})\s*(?:=|,|$)/g
      for (let i = 0; i < msk.length; i++) {
        const line = msk[i]
        if (!inEnum) {
          if (/\b(?:public|internal|private)?\s*enum\s+[A-Za-z_]\w*\s*(?::[^{]+)?\{/.test(line)) inEnum = true
          continue
        }
        re.lastIndex = 0
        let m
        while ((m = re.exec(line))) out.push(V(i + 1, '枚举成员 `' + m[1] + '` 须 PascalCase（禁 C 风格 MAX_SIZE；如 Active、Disabled）'))
        if ((line.match(/\}/g) || []).length > 0) inEnum = false
      }
      return out
    } },
  { id: 'as-layout-impl-dir', rule: '接口实现类归 Impl/', severity: SEV_WARNING, scope: 'as', category: CAT_LAYOUT,
    fn: function (msk, cmt, rel, fileName) {
      const lower = rel.toLowerCase()
      if (lower.indexOf('/impl/') >= 0) return []
      const re = /(?:public|internal)?\s*(?:(?:sealed|static|abstract|partial)\s+)*(?:class|struct)\s+([A-Za-z_]\w*Impl)\b/
      for (let i = 0; i < msk.length; i++) {
        const m = re.exec(msk[i])
        if (m) return [V(i + 1, '接口实现类 `' + m[1] + '` 应归入 `Impl/` 子目录（如 std/QIF/Impl/QIFHost.as；arc-language 目录结构惯例）')]
      }
      return []
    } },
  { id: 'rs-lib-rs-lines', rule: 'lib.rs ≤80 行门面', severity: SEV_ERROR, scope: 'rs-lib', category: CAT_RS,
    fn: function (msk) {
      const n = msk.length
      return n > 80 ? [V(1, 'lib.rs 共 ' + n + ' 行，超过 80 行上限（仅允许 mod 声明、pub use、crate 文档与 ≤5 行门面函数）')] : []
    } },
  { id: 'rs-lib-rs-facade', rule: 'lib.rs 仅 mod / pub use', severity: SEV_ERROR, scope: 'rs-lib', category: CAT_RS,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        const line = msk[i]
        if (/\b(?:pub\s+)?(?:struct|enum|trait|impl)\b/.test(line)) out.push(V(i + 1, 'lib.rs 禁止类型定义/实现块（仅 mod 与 pub use；实现拆到子模块）'))
        else if (/^\s*(?:pub\s+)?(?:const|static)\s/.test(line)) out.push(V(i + 1, 'lib.rs 禁止 const/static 项'))
        else if (/^\s*(?:pub\s+)?(?:async\s+)?fn\s/.test(line)) {
          const len = blockLen(msk, i)
          if (len > 5) out.push(V(i + 1, 'lib.rs 函数仅允许 ≤5 行纯委托门面（当前 ' + len + ' 行），请拆到子模块'))
        }
      }
      return out
    } },
  { id: 'rs-file-too-large', rule: '单文件拆分阈值', severity: SEV_ERROR, scope: 'rs', category: CAT_RS,
    fn: function (msk, cmt, rel) {
      const n = msk.length
      const lower = (rel || '').toLowerCase()
      // 测试文件（*/tests/**）放宽：e2e/单元测试单主题多断言是 Rust 测试惯例，
      // 拆多文件是机械切分、无概念收益（违背「按概念拆、不按行数机械切」本意）。
      const isTest = /\/tests?\//.test(lower)
      if (isTest) {
        if (n > 2000) return [V(1, n + ' 行 > 2000：测试文件超长（单主题多断言常态，但 2000 行以上须人工评估拆分成多测试文件）')]
        if (n > 1000) return [V(1, n + ' 行 > 1000：测试文件偏长（单主题多断言可接受；如含 ≥2 独立主题应拆）', SEV_WARNING)]
        return []
      }
      // 产品代码阈值分级（架构级校准，2026-08-19）：
      // - >4000：巨型单文件，无论概念必须拆分（error——此档为不可接受的单体）
      // - >1200：大文件，按概念评估拆分（warning——编译器核心模块如 mir/lower.rs
      //   6298 行是「单一职责巨型实现」，机械切分会破坏 cohesion；登记拆分专项，
      //   不阻断门禁）
      // - >200：常规拆分阈值（warning——新增代码不得越过；存量按概念评估）
      if (n > 4000) return [V(1, n + ' 行 > 4000：巨型单文件——无论概念必须拆分（arc-rust 模块化铁律；4000 行以上不可接受）')]
      if (n > 1200) return [V(1, n + ' 行 > 1200：大文件——按概念评估拆分（arc-rust；编译器核心模块多为单一职责巨型实现，拆分须按模块边界走专项，禁机械切）', SEV_WARNING)]
      if (n > 200) return [V(1, n + ' 行 > 200 且含 ≥2 独立概念 → 必须拆分；按概念拆、不按行数机械切（arc-rust）', SEV_WARNING)]
      return []
    } },
  { id: 'rs-todo-macro', rule: '禁止 todo!()/unimplemented!()', severity: SEV_ERROR, scope: 'rs', category: CAT_RS,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        if (/\btodo!\s*\(|\bunimplemented!\s*\(/.test(msk[i])) out.push(V(i + 1, '占位实现 todo!()/unimplemented!() 禁止（arc-core：无占位实现、无半成品）'))
      }
      return out
    } },
  { id: 'rs-domain-in-core', rule: '编译器核心禁领域能力', severity: SEV_WARNING, scope: 'rs-core', category: CAT_RS,
    fn: function (msk) {
      const out = []
      const re = /\b(?:Sql|Orm|JsonSerializer|JsonParser|SqlTranslator|Sqlite|Mongo|Postgres|MySql|Redis)\w*/g
      for (let i = 0; i < msk.length; i++) {
        re.lastIndex = 0
        let m
        while ((m = re.exec(msk[i]))) out.push(V(i + 1, '编译器核心 crate 出现领域标识 `' + m[0] + '` —— 架构红线：领域翻译逻辑由 std 以 Arc 语言实现，编译器仅提供通用机制'))
      }
      return out
    } },
  { id: 'ui-deny-raster', rule: '渲染唯一对接 wgpu（禁降级）', severity: SEV_ERROR, scope: 'ui', category: CAT_UI,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        if (/rt_ui_render_to_buffer|rt_ui_raster|text_gdi|GDI\b|软件光栅|软件渲染路径|软渲染/.test(msk[i])) out.push(V(i + 1, '渲染唯一后端 = wgpu：禁止软件光栅/GDI/stub 降级方案（arc-ui 铁律，最高优先级）'))
      }
      return out
    } },
  { id: 'ui-dual-path', rule: '禁止渲染双轨 if-else', severity: SEV_WARNING, scope: 'ui', category: CAT_UI,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        const line = msk[i]
        if (/(?:software|cpu|raster|软件)/.test(line) && /(?:wgpu|gpu)/.test(line)) out.push(V(i + 1, '疑似「软件路径 / wgpu 路径」双轨分支 —— 唯一后端 wgpu，禁止双轨 if-else（arc-ui）'))
      }
      return out
    } },
  { id: 'ui-stub-render', rule: '渲染实现禁 *_stub 占位', severity: SEV_ERROR, scope: 'ui', category: CAT_UI,
    fn: function (msk, cmt, rel, fileName) {
      return /_stub\.(?:c|rs)$/i.test(fileName) ? [V(1, '伪实现占位 stub 禁止作为渲染实现载体（跨平台缺分支可用 stub，但禁止冒充真实渲染实现）')] : []
    } },
  { id: 'docs-todo', rule: '文档无占位/待办表述', severity: SEV_WARNING, scope: 'docs', category: CAT_DOC,
    fn: function (msk) {
      const out = []
      for (let i = 0; i < msk.length; i++) {
        if (/\bTODO\b|\bFIXME\b|\bTBD\b|待补充|待完善|占位|待定/.test(msk[i])) out.push(V(i + 1, '文档不得含占位/待办表述（中文写作：先结论后细节；变更须同步 SUMMARY.md 与各目录 index.md）'))
      }
      return out
    } }
]

// ---------- 规则分派（与插件同步） ----------
function scopeMatch(r, rel, fileName, lower) {
  switch (r.scope) {
    case 'as': return lower.slice(-3) === '.as'
    case 'rs': return lower.slice(-3) === '.rs'
    case 'rs-lib': return fileName === 'lib.rs'
    case 'rs-core': return lower.slice(-3) === '.rs' && /^crates\/(?:ast|parse|hir|typeck|mir|codegen)\//.test(lower)
    case 'ui': return (UI_PATH_RE.test(rel) || lower.slice(-5) === '.arml') && !/\.m[dc]$/.test(lower)
    case 'docs': return lower.slice(0, 5) === 'docs/' && lower.slice(-3) === '.md'
    default: return false
  }
}
function dispatch(rel, text, quick) {
  const out = []
  const fileName = rel.split('/').pop()
  const lower = rel.toLowerCase()
  const msk = maskLines(text)
  const cmt = commentLines(text)
  for (let k = 0; k < RULES.length; k++) {
    const r = RULES[k]
    if (!scopeMatch(r, rel, fileName, lower)) continue
    if (quick && r.severity !== SEV_ERROR) continue
    if (isExempt(r.id, rel)) continue
    const hits = r.fn(msk, cmt, rel, fileName)
    for (let h = 0; h < hits.length; h++) {
      const hit = hits[h]
      out.push({ id: r.id, rule: r.rule, severity: hit.sev || r.severity, category: r.category, file: rel, line: hit.line, message: hit.message })
    }
  }
  return out
}

// ---------- 目录/卫生/依赖/验证矩阵（与插件同步） ----------
function parseDeps(text) {
  const m = /\[dependencies\]([\s\S]*?)(?:\n\[|$)/.exec(text)
  if (!m) return []
  const out = []
  const lines = m[1].split(/\r?\n/)
  for (let i = 0; i < lines.length; i++) {
    const mm = /^\s*([a-zA-Z0-9_-]+)\s*=/.exec(lines[i])
    if (mm) out.push(mm[1])
  }
  return out
}
function cargoChecks(files) {
  const out = []
  for (let f = 0; f < files.length; f++) {
    const rel = files[f]
    const m = /^crates\/([^/]+)\/Cargo\.toml$/.exec(rel)
    if (!m) continue
    const c = m[1]
    const text = readRel(rel)
    if (text === null) continue
    const names = parseDeps(text)
    for (let i = 0; i < names.length; i++) {
      const d = names[i]
      const ci = CORE_CHAIN.indexOf(c)
      const di = CORE_CHAIN.indexOf(d)
      if (ci >= 0 && di > ci) out.push({ id: 'rs-reverse-dep', rule: '依赖单向不可逆', severity: SEV_ERROR, category: CAT_RS, file: rel, line: 0, message: '核心链依赖方向错误：' + c + ' 依赖更下游的 ' + d + '（ast→parse→hir→typeck→mir→codegen→arc，不可逆、无循环依赖）' })
      else if (ci >= 0 && SIDE_CRATES.indexOf(d) >= 0 && c !== 'arc') out.push({ id: 'rs-core-side-dep', rule: '核心 crate 不依赖配套 crate', severity: SEV_INFO, category: CAT_RS, file: rel, line: 0, message: '核心 crate ' + c + ' 依赖配套 crate ' + d + ' —— 如确有必要请先评审（arc-rust crate 布局）' })
    }
  }
  return out
}
function hygieneScan() {
  const out = []
  const junkFile = /^(\.tmp_|\.tmp-|target-|obj-|bin-|arc_crash|regex_e2e|arc_web_staged|winscan|armhdemo)/i
  const junkExt = /\.(?:tmp|log|patch|bak|orig)$/i
  const scriptRe = /\.(?:ps1|cjs)$/i
  const push = function (file, msg) {
    out.push({ id: 'hyg-artifact', rule: '工作区卫生：产物只落 target/ 或 $env:TEMP', severity: SEV_ERROR, category: CAT_HYG, file: file, line: 0, message: msg })
  }
  const entries = listRel('')
  if (entries) {
    for (let i = 0; i < entries.length; i++) {
      const e = entries[i]
      if (e.type === 'file') {
        if (junkFile.test(e.name) || junkExt.test(e.name)) push(e.name, '疑似调试/测试产物散落仓库根 —— 重定向/日志一律落 $env:TEMP 或 target/scratch/（arc-workspace-hygiene）')
        else if (scriptRe.test(e.name)) push(e.name, '脚本必须归入 scripts/（<域>-<用途>.ps1，kebab-case）；禁止散落仓库根')
      } else if (e.type === 'directory' && /^(\.tmp_|\.tmp-|target-|obj-|bin-)/i.test(e.name)) {
        push(e.name, '非规范目录名：允许固定 target/、obj/、bin/；`--obj-dir` 仅 …/obj/<config>/ 或 target/e2e/…/obj/')
      }
    }
  }
  // 递归扫描 SCAN_ROOTS 全树（非仅第一层）：调试/测试产物无论埋多深都被检出
  // （架构级：单一扫描面表 SCAN_ROOTS，新增根只改常量）。
  for (let d = 0; d < SCAN_ROOTS.length; d++) {
    walkJunk(SCAN_ROOTS[d], junkFile, junkExt, push, out)
  }
  return out
}
function walkJunk(rel, junkFile, junkExt, push, out) {
  const entries = listRel(rel)
  if (!entries) return
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i]
    const child = rel + '/' + e.name
    if (e.type === 'directory') {
      if (!SKIP_DIRS.has(e.name)) walkJunk(child, junkFile, junkExt, push, out)
    } else if (e.type === 'file') {
      // 递归扫描仅按扩展名判定（.tmp/.log/.patch/.bak/.orig）：合法源文件不可能
      // 使用这些扩展名，前缀匹配（junkFile）只用于根目录残留判定——前缀会误伤
      // 合法文件（如 tests/regex_e2e.rs 命中 regex_e2e 前缀），不得在源码树递归用。
      if (junkExt.test(e.name)) push(child, '源码树中发现调试/测试产物（递归扫描）—— 中间产物只允许 obj/<config>/，最终产物只允许 bin/<config>/，日志落 $env:TEMP 或 target/scratch/')
    }
  }
}
function collectScripts(rel, out) {
  const entries = listRel(rel)
  if (!entries) return
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i]
    const child = rel + '/' + e.name
    if (e.type === 'directory') {
      if (!SKIP_DIRS.has(e.name)) collectScripts(child, out)
    } else if (e.type === 'file' && /\.(ps1|cjs)$/i.test(e.name)) {
      out.push(child)
    }
  }
}
function layoutScan() {
  const out = []
  const push = function (file, msg, sev) {
    out.push({ id: 'lay-layout', rule: '目录结构规范', severity: sev || SEV_ERROR, category: CAT_LAYOUT, file: file, line: 0, message: msg })
  }
  const kebab = /^[a-z0-9]+(?:-[a-z0-9]+)*$/
  const crates = listRel('crates')
  if (crates) {
    for (let i = 0; i < crates.length; i++) {
      const e = crates[i]
      if (e.type === 'directory' && !kebab.test(e.name)) push('crates/' + e.name, 'crate 目录名须 kebab-case（如 runtime-ui）')
    }
  }
  const stdRoot = listRel('std')
  if (stdRoot) {
    for (let i = 0; i < stdRoot.length; i++) {
      const e = stdRoot[i]
      if (e.type === 'directory' && !/^[A-Z][A-Za-z0-9]*$/.test(e.name)) push('std/' + e.name, 'std/ 命名空间目录须 PascalCase（如 Arc、Orm、UI、Web）')
    }
    for (let i = 0; i < stdRoot.length; i++) {
      const e = stdRoot[i]
      if (e.type !== 'directory') continue
      const sub = listRel('std/' + e.name)
      if (!sub) continue
      let hasImplDir = false
      const implFiles = []
      for (let k = 0; k < sub.length; k++) {
        if (sub[k].type === 'directory' && sub[k].name === 'Impl') hasImplDir = true
        if (sub[k].type === 'file' && /\w+Impl\.as$/i.test(sub[k].name)) implFiles.push(sub[k].name)
      }
      if (!hasImplDir && implFiles.length) push('std/' + e.name, '存在接口实现类 ' + implFiles.slice(0, 3).join(', ') + ' 但缺少 Impl/ 子目录（arc-language：接口实现类归 Impl/）')
    }
  }
  const scriptFiles = []
  collectScripts('scripts', scriptFiles)
  for (let i = 0; i < scriptFiles.length; i++) {
    const name = scriptFiles[i].split('/').pop()
    const stem = name.replace(/\.(ps1|cjs)$/i, '')
    if (!kebab.test(stem)) push(scriptFiles[i], '脚本命名须 kebab-case（<域>-<用途>.ps1；禁下划线/大写/裸名），存量触及处随重构转换', SEV_WARNING)
  }
  const docs = listRel('docs')
  if (docs) {
    const have = {}
    for (let i = 0; i < docs.length; i++) have[docs[i].name] = 1
    const needFiles = ['preface.md', 'SUMMARY.md', 'plan.md']
    for (let i = 0; i < needFiles.length; i++) {
      if (!have[needFiles[i]]) out.push({ id: 'lay-docs-core', rule: '文档结构', severity: SEV_WARNING, category: CAT_LAYOUT, file: 'docs/' + needFiles[i], line: 0, message: 'docs/ 必备文档缺失（' + needFiles[i] + '；arc-docs 结构）' })
    }
    const needDirs = ['rfc', 'user-guide', 'domain', 'white-paper']
    for (let i = 0; i < needDirs.length; i++) {
      if (!have[needDirs[i]]) out.push({ id: 'lay-docs-dir', rule: '文档结构', severity: SEV_WARNING, category: CAT_LAYOUT, file: 'docs/' + needDirs[i], line: 0, message: 'docs/ 必备目录缺失（' + needDirs[i] + '；arc-docs 结构）' })
    }
  }
  return out
}
function verifyPlan(files) {
  const cmds = []
  const add = function (command, reason) {
    if (!cmds.some(function (c) { return c.command === command })) cmds.push({ command: command, reason: reason })
  }
  const crates = new Set()
  for (let i = 0; i < files.length; i++) {
    const m = /^crates\/([^/]+)/.exec(files[i])
    if (m) crates.add(m[1])
  }
  crates.forEach(function (c) {
    if (CORE_CHAIN.indexOf(c) >= 0 || SIDE_CRATES.indexOf(c) >= 0) add('cargo test -p ' + c, '编译器 crate 最低验证（arc-core）')
  })
  let coreTouched = false
  crates.forEach(function (c) { if (CORE_CHAIN.indexOf(c) >= 0) coreTouched = true })
  if (coreTouched) add('cargo test -p arc-tests', '核心链变更 → 端到端管线验证')
  if (files.some(function (p) { return p.indexOf('std/') === 0 || p.slice(-3) === '.as' || p.slice(-5) === '.arml' })) add('cargo test --workspace', 'std / 语言面变更 → 全工作区验证（AGENTS.md）')
  // examples 冒烟：优先标准入口 `<name>/Program.as`（多文件项目单文件 build 无意义）；
  // 无标准入口时才回退到任意首个 examples/*.as（如 CLI 示例单文件）。
  const prog = files.find(function (p) { return /^examples\/[^/]+\/Program\.as$/.test(p) })
  const ex = prog || files.find(function (p) { return p.indexOf('examples/') === 0 && p.slice(-3) === '.as' })
  if (ex) add('cargo run -p arc -- build ' + ex, '端到端冒烟（arc-core 验证矩阵；标准入口 <name>/Program.as）')
  return cmds
}
function gitChanged() {
  try {
    // core.quotepath=false：中文/特殊字符路径输出原始 UTF-8，避免 \346 字节转义被误解析为路径分隔
    const status = execFileSync('git', ['-c', 'core.quotepath=false', 'status', '--porcelain'], { cwd: ROOT, encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] })
    const changed = []
    const stLines = String(status).split(/\r?\n/)
    for (let i = 0; i < stLines.length; i++) {
      // porcelain v1：`XY SP path`（X/Y 为暂存/工作树状态码），路径固定从索引 3 开始；
      // 不可先 trim 行首，否则以 `.` 开头的路径（.github/...）索引错位丢点。
      const t = stLines[i].replace(/\r$/, '')
      if (!t.trim()) continue
      const code = t.slice(0, 2)
      if (code.indexOf('D') >= 0) continue // 已删除文件无需检查
      let p = t.length > 3 ? t.slice(3) : t.slice(2)
      if (p.indexOf(' -> ') >= 0) p = p.split(' -> ').pop()
      p = p.replace(/^"(.*)"$/, '$1').replace(/\\/g, '/').trim()
      if (p) changed.push(p)
    }
    return changed
  } catch (e) { return null }
}

// ---------- 文件收集（与插件同步） ----------
function collectFiles(rel, depth, out, seen, maxFiles, maxDepth) {
  const limit = maxFiles || 400
  const dmax = maxDepth || 6
  if (depth > dmax || out.length >= limit) return
  const entries = listRel(rel)
  if (!entries) return
  for (let i = 0; i < entries.length; i++) {
    if (out.length >= limit) return
    const e = entries[i]
    const child = rel + '/' + e.name
    if (e.type === 'directory') {
      if (!SKIP_DIRS.has(e.name)) collectFiles(child, depth + 1, out, seen, limit, dmax)
    } else if (e.type === 'file' && CHECK_EXTS.indexOf(e.name.toLowerCase().slice(e.name.lastIndexOf('.'))) >= 0) {
      if (!seen[child]) { seen[child] = 1; out.push(child) }
    }
  }
}
function expandPaths(paths) {
  const out = []
  const seen = {}
  for (let i = 0; i < paths.length; i++) {
    const rel = paths[i]
    if (!rel) continue
    const info = statRel(rel)
    if (info && isDir(info)) collectFiles(rel, 0, out, seen)
    else if (!seen[rel]) { seen[rel] = 1; out.push(rel) }
  }
  return out
}

// ---------- 主流程 ----------
function runCheck(opts) {
  const quick = opts.quick === true
  const includeHygiene = opts.includeHygiene !== false
  const includeLayout = opts.includeLayout !== false
  let paths = (opts.paths || []).filter(function (p) { return typeof p === 'string' && p.trim() !== '' })
  let note = ''
  let files = []
  if (opts.all === true) {
    // --all 全库扫描：显式展开全部扫描面（CI 门禁模式，规则引擎真实生效）。
    // CI checkout 后 git 无变更，变更文件模式会退化为仅卫生/布局扫描——此分支
    // 保证编码契约规则（RULES）在 CI 上全库执行。
    const seen = {}
    for (let i = 0; i < SCAN_ROOTS.length; i++) collectFiles(SCAN_ROOTS[i], 0, files, seen, ALL_MAX_FILES, 10)
    note = '--all 全库扫描：' + SCAN_ROOTS.join('/') + ' 全集（' + files.length + ' 文件）——CI 门禁模式，规则引擎全库生效'
  } else {
    files = expandPaths(paths)
    if (paths.length === 0) {
      const g = gitChanged()
      if (g && g.length) {
        files = expandPaths(g)
        note = '未提供 paths：自动采用 git 检出的 ' + g.length + ' 个变更文件'
      } else {
        note = '未提供 paths 且 git 无变更 —— 仅执行卫生/目录结构扫描；可显式传入 paths 或使用 --all 全库扫描'
      }
    }
  }
  const violations = []
  const unreadable = []
  for (let i = 0; i < files.length; i++) {
    const text = readRel(files[i])
    if (text === null) { unreadable.push(files[i]); continue }
    const vs = dispatch(files[i], text, quick)
    for (let k = 0; k < vs.length; k++) violations.push(vs[k])
  }
  const cargo = cargoChecks(files)
  for (let k = 0; k < cargo.length; k++) violations.push(cargo[k])
  if (includeHygiene) {
    const hy = hygieneScan()
    for (let k = 0; k < hy.length; k++) violations.push(hy[k])
  }
  if (includeLayout) {
    const ly = layoutScan()
    for (let k = 0; k < ly.length; k++) violations.push(ly[k])
  }
  const counts = { error: 0, warning: 0, info: 0 }
  for (let i = 0; i < violations.length; i++) counts[violations[i].severity]++
  violations.sort(function (a, b) {
    const x = (a.file || '').localeCompare(b.file || '')
    return x !== 0 ? x : a.line - b.line
  })
  const applied = []
  const seen = {}
  for (let k = 0; k < RULES.length; k++) {
    const r = RULES[k]
    for (let i = 0; i < files.length; i++) {
      const rel = files[i]
      const fileName = rel.split('/').pop()
      if (scopeMatch(r, rel, fileName, rel.toLowerCase()) && !isExempt(r.id, rel)) {
        if (!seen[r.id]) {
          seen[r.id] = 1
          applied.push({ id: r.id, rule: r.rule, severity: r.severity, category: r.category })
        }
        break
      }
    }
  }
  if (includeHygiene) applied.push({ id: 'hyg-scan', rule: '工作区卫生扫描（根目录产物/脚本归属）', severity: SEV_ERROR, category: CAT_HYG })
  if (includeLayout) applied.push({ id: 'lay-scan', rule: '目录结构扫描（crates/std/scripts/docs 布局）', severity: SEV_ERROR, category: CAT_LAYOUT })
  return {
    exitCode: counts.error > 0 ? 1 : 0,
    passed: counts.error === 0,
    counts: counts,
    violations: violations,
    unreadable: unreadable,
    verification: verifyPlan(files),
    appliedRules: applied,
    exemptions: EXEMPTIONS.map(function (e) { return { id: e.id, file: e.file, reason: e.reason } }),
    quick: quick,
    note: note
  }
}

function renderText(v) {
  const lines = []
  lines.push('# Arc 规范守卫（spec-guard）')
  lines.push((v.passed ? '✅ 通过' : '❌ 未通过') + ' —— error=' + v.counts.error + ' · warning=' + v.counts.warning + ' · info=' + v.counts.info + (v.quick ? '（快速门禁：仅 error 级）' : ''))
  if (v.note) lines.push('> ' + v.note)
  if (v.exemptions && v.exemptions.length) lines.push('豁免：' + v.exemptions.map(function (e) { return e.id + '@' + e.file }).join('；'))
  const groups = { error: [], warning: [], info: [] }
  for (let i = 0; i < v.violations.length; i++) groups[v.violations[i].severity].push(v.violations[i])
  const sevs = ['error', 'warning', 'info']
  for (let s = 0; s < sevs.length; s++) {
    const g = groups[sevs[s]]
    if (g.length === 0) continue
    lines.push('')
    lines.push('## ' + sevs[s].toUpperCase() + ' (' + g.length + ')')
    for (let i = 0; i < g.length; i++) {
      const x = g[i]
      lines.push('- [' + x.id + '] ' + x.file + ':' + x.line + ' — ' + x.rule + '：' + x.message)
    }
  }
  if (v.unreadable && v.unreadable.length) {
    lines.push('')
    lines.push('## 无法读取：' + v.unreadable.join(', '))
  }
  if (v.verification && v.verification.length) {
    lines.push('')
    lines.push('## 验证矩阵（arc-core）')
    for (let i = 0; i < v.verification.length; i++) lines.push('- `' + v.verification[i].command + '` — ' + v.verification[i].reason)
  }
  return lines.join('\n')
}

// ---------- CLI ----------
const argv = process.argv.slice(2)
const command = argv[0] || 'check'
if (command === 'rules') {
  for (let i = 0; i < RULES.length; i++) {
    console.log('[' + RULES[i].severity + '] ' + RULES[i].id + ' — ' + RULES[i].rule + '（' + RULES[i].category + '）')
  }
  if (EXEMPTIONS.length) {
    console.log('')
    console.log('豁免：')
    for (let i = 0; i < EXEMPTIONS.length; i++) console.log('- ' + EXEMPTIONS[i].id + '@' + EXEMPTIONS[i].file + '：' + EXEMPTIONS[i].reason)
  }
  process.exit(0)
}
if (command !== 'check') {
  console.error('未知命令：' + command + '（支持 check / rules）')
  process.exit(2)
}
const opts = { paths: [], quick: false, all: false, includeHygiene: true, includeLayout: true, json: false }
for (let i = 1; i < argv.length; i++) {
  const a = argv[i]
  if (a === '--quick') opts.quick = true
  else if (a === '--all') opts.all = true
  else if (a === '--no-hygiene') opts.includeHygiene = false
  else if (a === '--no-layout') opts.includeLayout = false
  else if (a === '--json') opts.json = true
  else opts.paths.push(a)
}
const result = runCheck(opts)
if (opts.json) {
  process.stdout.write(JSON.stringify(result))
} else {
  process.stdout.write(renderText(result) + '\n')
}
process.exitCode = result.exitCode
