//! L2 批量运行时测试：Arc.Net 纯解析行为验证（Uri userinfo/IPv6 端口、
//! CookieContainer domain 分桶隔离）。无网络 I/O，纯本地解析。
//!
//! 通过 `build_and_run_batch` 合并多个 case 为一次编译 + 一次运行。
//! 每个 case 自行输出 `ARC_CASE:{name}:PASS/FAIL` 标记。
//! 需 `--features full-rt` 门控，默认 `cargo test` 不触发。

#![cfg(feature = "full-rt")]

use arc_tests::batch::{batch_case_result, build_and_run_batch_with_deps, BatchCase};

// 批依赖 Arc.Net（Net/Core）→ Arc.Security 传递符号使产物隐式导入 vendored
// crypto_native.dll——beside-exe 复制由 codegen 的 copy_crypto_native_dll_if_needed
// 统一负责（std P3 门卫修复后进程内编译路径同样生效），测试侧不再兜底。
#[test]
fn net_uri_parse_batch() {
    // Uri.Parse：authority 边界先行（query 中 @ 不污染）、userinfo 取 LAST @、
    // IPv6 端口取 "]" 之后的冒号；含普通 host:port / 默认端口 / fragment 回归保护。
    let results = build_and_run_batch_with_deps(
        "net_uri_parse",
        &[
            BatchCase {
                name: "uri_userinfo_basic",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://a@b.com/x");
    if (u.UserInfo != "a") { Console.WriteLine("ARC_CASE:uri_userinfo_basic:FAIL:userinfo"); return; }
    if (u.Host != "b.com") { Console.WriteLine("ARC_CASE:uri_userinfo_basic:FAIL:host"); return; }
    Console.WriteLine("ARC_CASE:uri_userinfo_basic:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_query_at_not_polluted",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://x.com/?email=a@b.com");
    if (u.UserInfo != "") { Console.WriteLine("ARC_CASE:uri_query_at_not_polluted:FAIL:userinfo"); return; }
    if (u.Host != "x.com") { Console.WriteLine("ARC_CASE:uri_query_at_not_polluted:FAIL:host"); return; }
    if (u.Query != "?email=a@b.com") { Console.WriteLine("ARC_CASE:uri_query_at_not_polluted:FAIL:query"); return; }
    Console.WriteLine("ARC_CASE:uri_query_at_not_polluted:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_ipv6_port",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://[::1]:8080/");
    if (u.Host != "::1") { Console.WriteLine("ARC_CASE:uri_ipv6_port:FAIL:host"); return; }
    if (u.Port != 8080) { Console.WriteLine("ARC_CASE:uri_ipv6_port:FAIL:port"); return; }
    Console.WriteLine("ARC_CASE:uri_ipv6_port:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_ipv6_no_port",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://[::1]/");
    if (u.Host != "::1") { Console.WriteLine("ARC_CASE:uri_ipv6_no_port:FAIL:host"); return; }
    if (u.Port != 80) { Console.WriteLine("ARC_CASE:uri_ipv6_no_port:FAIL:port"); return; }
    Console.WriteLine("ARC_CASE:uri_ipv6_no_port:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_last_at_wins",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    // userinfo 自身可含 @：取 LAST @ 之后的为 host
    Uri u = new Uri("http://user:p@ss@host/");
    if (u.UserInfo != "user:p@ss") { Console.WriteLine("ARC_CASE:uri_last_at_wins:FAIL:userinfo"); return; }
    if (u.Host != "host") { Console.WriteLine("ARC_CASE:uri_last_at_wins:FAIL:host"); return; }
    Console.WriteLine("ARC_CASE:uri_last_at_wins:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_plain_host_port",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://h:8080/p");
    if (u.Host != "h") { Console.WriteLine("ARC_CASE:uri_plain_host_port:FAIL:host"); return; }
    if (u.Port != 8080) { Console.WriteLine("ARC_CASE:uri_plain_host_port:FAIL:port"); return; }
    if (u.AbsolutePath != "/p") { Console.WriteLine("ARC_CASE:uri_plain_host_port:FAIL:path"); return; }
    Console.WriteLine("ARC_CASE:uri_plain_host_port:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_https_default",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("https://h/p");
    if (u.Scheme != "https") { Console.WriteLine("ARC_CASE:uri_https_default:FAIL:scheme"); return; }
    if (u.Port != 443) { Console.WriteLine("ARC_CASE:uri_https_default:FAIL:port"); return; }
    Console.WriteLine("ARC_CASE:uri_https_default:PASS");
}
"#,
            },
            BatchCase {
                name: "uri_fragment",
                src: r##"using Arc;
using Arc.Net;

void Main() {
    Uri u = new Uri("http://h/p#frag");
    if (u.AbsolutePath != "/p") { Console.WriteLine("ARC_CASE:uri_fragment:FAIL:path"); return; }
    if (u.Fragment != "#frag") { Console.WriteLine("ARC_CASE:uri_fragment:FAIL:fragment"); return; }
    if (u.Query != "") { Console.WriteLine("ARC_CASE:uri_fragment:FAIL:query"); return; }
    Console.WriteLine("ARC_CASE:uri_fragment:PASS");
}
"##,
            },
        ],
        &[("Arc.Net", "Net/Core")],
    );

    for name in [
        "uri_userinfo_basic",
        "uri_query_at_not_polluted",
        "uri_ipv6_port",
        "uri_ipv6_no_port",
        "uri_last_at_wins",
        "uri_plain_host_port",
        "uri_https_default",
        "uri_fragment",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "{} failed: {:?} stdout: {}",
            name, r.error, r.stdout
        );
    }
}

#[test]
fn net_cookie_container_batch() {
    // CookieContainer：按 Host 分桶——跨域隔离、同名覆盖、异名共存、Clear。
    let results = build_and_run_batch_with_deps(
        "net_cookie_container",
        &[
            BatchCase {
                name: "cookie_domain_isolation",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u1 = new Uri("http://a.com/");
    Uri u2 = new Uri("http://b.com/");
    Uri u3 = new Uri("http://c.com/");
    CookieContainer jar = new CookieContainer();
    jar.Add(u1, "a=1");
    jar.Add(u2, "b=2");
    if (jar.GetCookieHeader(u1) != "a=1") { Console.WriteLine("ARC_CASE:cookie_domain_isolation:FAIL:u1"); return; }
    if (jar.GetCookieHeader(u2) != "b=2") { Console.WriteLine("ARC_CASE:cookie_domain_isolation:FAIL:u2"); return; }
    if (jar.GetCookieHeader(u3) != "") { Console.WriteLine("ARC_CASE:cookie_domain_isolation:FAIL:u3"); return; }
    Console.WriteLine("ARC_CASE:cookie_domain_isolation:PASS");
}
"#,
            },
            BatchCase {
                name: "cookie_same_host_update",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u1 = new Uri("http://a.com/");
    CookieContainer jar = new CookieContainer();
    jar.Add(u1, "a=1");
    jar.Add(u1, "a=2");
    jar.Add(u1, "b=3");
    if (jar.GetCookieHeader(u1) != "a=2; b=3") { Console.WriteLine("ARC_CASE:cookie_same_host_update:FAIL:value"); return; }
    Console.WriteLine("ARC_CASE:cookie_same_host_update:PASS");
}
"#,
            },
            BatchCase {
                name: "cookie_clear",
                src: r#"using Arc;
using Arc.Net;

void Main() {
    Uri u1 = new Uri("http://a.com/");
    CookieContainer jar = new CookieContainer();
    jar.Add(u1, "a=1");
    jar.Clear();
    if (jar.GetCookieHeader(u1) != "") { Console.WriteLine("ARC_CASE:cookie_clear:FAIL:value"); return; }
    Console.WriteLine("ARC_CASE:cookie_clear:PASS");
}
"#,
            },
        ],
        &[("Arc.Net", "Net/Core")],
    );

    for name in [
        "cookie_domain_isolation",
        "cookie_same_host_update",
        "cookie_clear",
    ] {
        let r = batch_case_result(&results, name);
        assert!(
            r.passed,
            "{} failed: {:?} stdout: {}",
            name, r.error, r.stdout
        );
    }
}
