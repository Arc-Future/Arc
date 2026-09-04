//! Arc 性能对照 Rust harness（RFC 099 §2.3 / 08-rfcs V1-SPRINT 轨道 G）。
//!
//! 与 Arc `std_hotpath_bench_e2e` 同构：List/Dict/HS 三场景，输出 `OK:` 行
//! 供 `run_std_hotpath_*` 脚本解析成对比。
//!
//! 构建：`cargo build --release`
//! 运行：`target/release/arc_rust-hotpath.exe`

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::Instant;

fn report(name: &str, ops: f64, ns: u128) {
    let ns_per_op = ns as f64 / ops;
    println!("OK: {name} ops={ops} ns_total={ns} ns_per_op={ns_per_op:.2}");
}

fn main() {
    // List: Add + index read（与 Arc bench_list_add_get 同构：N=200k，2N ops）
    let n: usize = 200_000;
    let t0 = Instant::now();
    let mut v: Vec<i64> = Vec::with_capacity(n);
    for i in 0..n {
        v.push(i as i64);
    }
    let mut acc: i64 = 0;
    for i in 0..n {
        acc += v[i];
    }
    black_box(acc);
    report("list_add_get", (n as f64) * 2.0, t0.elapsed().as_nanos());

    // Dictionary: set + get（与 Arc bench_dict_set_get 同构：N=150k，2N ops）
    let n: usize = 150_000;
    let t0 = Instant::now();
    let mut d: HashMap<i64, i64> = HashMap::with_capacity(n);
    for i in 0..n as i64 {
        d.insert(i, i * 2);
    }
    let mut acc: i64 = 0;
    for i in 0..n as i64 {
        acc += d.get(&i).copied().unwrap_or(0);
    }
    black_box(acc);
    report("dict_set_get", (n as f64) * 2.0, t0.elapsed().as_nanos());

    // HashSet: Add + Contains + duplicate Add（与 Arc bench_hashset_add_contains
    // 同构：N=150k，3N ops）
    let n: usize = 150_000;
    let t0 = Instant::now();
    let mut s: HashSet<i64> = HashSet::with_capacity(n);
    for i in 0..n as i64 {
        s.insert(i);
    }
    let mut acc: i64 = 0;
    for i in 0..n as i64 {
        acc += if s.contains(&i) { 1 } else { 0 };
    }
    // duplicate Add（Arc harness 的第三遍）
    for i in 0..n as i64 {
        s.insert(i);
    }
    black_box(acc);
    report("hashset_add_contains", (n as f64) * 3.0, t0.elapsed().as_nanos());

    // StringBuilder: String push（与 Arc bench_stringbuilder_append 同构：N=100k）
    let n: usize = 100_000;
    let t0 = Instant::now();
    let mut sb: String = String::with_capacity(n * 8);
    for _ in 0..n {
        sb.push('x');
    }
    let len = sb.len();
    black_box(len);
    report("stringbuilder_append", n as f64, t0.elapsed().as_nanos());

    // File IO 吞吐：64 KiB 载荷 Write+Read 往返（与 Arc bench_file_io_throughput 同构：
    // N=64，2N ops）
    {
        use std::fs;
        const N: usize = 64;
        const PAYLOAD_SIZE: usize = 64 * 1024;
        let path = std::env::temp_dir().join("arc_rust-hotpath_io_big.tmp");
        let payload = "x".repeat(PAYLOAD_SIZE);
        let t0 = Instant::now();
        for _ in 0..N {
            fs::write(&path, &payload).unwrap();
            let got = fs::read_to_string(&path).unwrap();
            assert_eq!(got.len(), PAYLOAD_SIZE);
        }
        let _ = fs::remove_file(&path);
        report("file_io_throughput", (N as f64) * 2.0, t0.elapsed().as_nanos());
    }
}
