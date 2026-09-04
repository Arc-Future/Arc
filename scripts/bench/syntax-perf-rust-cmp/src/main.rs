//! 基本语法性能对比 harness：与 Arc `syntax_perf_bench_e2e.rs` 同构（同 N/ops/负载）。
//!
//! 场景：
//!   1. loop_sum            纯算术循环求和 N=5e7（同 Arc/C 公式 s += i ^ (i>>3)）
//!   2. string_replace_long 1MB 文本（'a' 为主 + 16 处 "xyz"）替换 "xyz"→"XYZ" 20 次
//!   3. file_concurrency    8 线程各自 write+read 64KB 文件 50 次（std::thread::scope）
//!
//! 输出 `OK:` 行供 `run-syntax-perf-cmp.ps1` 解析。仅锚点，不作业界领先宣称。

use std::hint::black_box;
use std::time::Instant;

fn report(name: &str, ops: f64, ns: f64) {
    let ns_per_op = ns / ops;
    let ops_s = ops * 1e9 / ns;
    println!("OK: {name} ops={ops} ns_total={ns} ns_per_op={ns_per_op:.2} ops_per_s={ops_s:.0}");
}

fn main() {
    // 1. 纯算术循环
    let n: i64 = 50_000_000;
    let t0 = Instant::now();
    let mut s: i64 = 0;
    for i in 0..n {
        s += i ^ (i >> 3);
    }
    let ns = t0.elapsed().as_nanos() as f64;
    black_box(s);
    report("loop_sum", n as f64, ns);

    // 2. 长文本 replace（1MB + 16 处 "xyz" → "XYZ"，20 次）
    {
        const LEN: usize = 1_048_576;
        const OCC: usize = 16;
        const STEP: usize = 65536;
        const N: usize = 20;
        let mut bytes = vec![b'a'; LEN];
        for i in 0..OCC {
            bytes[i * STEP] = b'x';
            bytes[i * STEP + 1] = b'y';
            bytes[i * STEP + 2] = b'z';
        }
        let mut s = String::from_utf8(bytes).unwrap();
        let t0 = Instant::now();
        for _ in 0..N {
            s = s.replace("xyz", "XYZ");
        }
        let ns = t0.elapsed().as_nanos() as f64;
        assert_eq!(s.len(), LEN);
        black_box(s.len());
        report("string_replace_long", N as f64, ns);
    }

    // 3. 文件操作并发（8 线程 × 50 次 write+read 64KB）
    {
        const T: usize = 8;
        const M: usize = 50;
        const PAYLOAD: usize = 64 * 1024;
        let payload = vec![b'x'; PAYLOAD];
        let dir = std::env::temp_dir();
        let t0 = Instant::now();
        std::thread::scope(|scope| {
            for t in 0..T {
                let path = dir.join(format!("synfc_{t}.tmp"));
                let payload = &payload;
                scope.spawn(move || {
                    for _ in 0..M {
                        std::fs::write(&path, payload).unwrap();
                        let got = std::fs::read(&path).unwrap();
                        assert_eq!(got.len(), payload.len());
                    }
                });
            }
        });
        let ns = t0.elapsed().as_nanos() as f64;
        for t in 0..T {
            let _ = std::fs::remove_file(dir.join(format!("synfc_{t}.tmp")));
        }
        report("file_concurrency", (T * M * 2) as f64, ns);
    }
}
