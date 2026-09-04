//! RFC 015 并发性能协议对照（Rust std 基线）。
//!
//! std 无 work-stealing 调度器/无并发集合（rayon/crossbeam 为外部依赖，本对照
//! 遵守「Rust `std::collections` 等」口径，不强引第三方）→ 仅提供：
//!   - parallel_scale：`std::thread` 分块并行求和（与 Arc parallel_for_amdahl
//!     场景同构：串行基线 + 1/2/4/8/16 workers 加速比）
//!   - task_create_1m：无 std task 抽象 → 如实 N/A（打印 note 行）
//!
//! 输出 OK: 行供协议对照脚本解析。

use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Instant;

fn main() {
    const TOTAL: i64 = 10_000_000;

    // 串行基线（black_box 防止常量折叠）
    let t0 = Instant::now();
    let mut serial: i64 = 0;
    for i in 0..TOTAL {
        serial = serial.wrapping_add(black_box(i));
    }
    let serial_ms = t0.elapsed().as_secs_f64() * 1000.0;
    black_box(serial);
    println!("OK: parallel_scale serial_ms={serial_ms:.2} sum={serial}");

    // 1/2/4/8/16 workers：std::thread 分块
    for w in [1usize, 2, 4, 8, 16] {
        let t0 = Instant::now();
        let counter = AtomicU64::new(0);
        let chunk = (TOTAL / w as i64) + 1;
        thread::scope(|s| {
            for t in 0..w {
                let counter = &counter;
                let start = t as i64 * chunk;
                let end = (start + chunk).min(TOTAL);
                s.spawn(move || {
                    let mut acc: i64 = 0;
                    for i in start..end {
                        acc = acc.wrapping_add(black_box(i));
                    }
                    counter.fetch_add(acc as u64, Ordering::Relaxed);
                });
            }
        });
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        black_box(counter.load(Ordering::Relaxed));
        let sp = serial_ms / ms;
        println!("OK: parallel_scale w={w} ms={ms:.2} speedup={sp:.2}");
    }

    // std 无 task 抽象 → 如实 N/A
    println!("OK: task_create_1m n/a std_no_task_abstraction");
    println!("OK: concurrent_collection n/a std_no_concurrent_collection");
}
