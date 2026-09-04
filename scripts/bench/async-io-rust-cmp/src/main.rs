//! async IO 对比 harness：与 Arc `adv_async_io_throughput_e2e` 同构（该 bench 已随 arc-integration 退场，a2627a0f；同 K/ROUNDS/offset 公式）。
//!
//! 镜像 Arc 工作负载：
//!   - 64 MiB 文件，单 fd
//!   - 每轮批量提交 K=4096 个 4KB offset 读 → 全部完成
//!   - ROUNDS=256，总 ops = 1,048,576
//!   - offset(j) = (j*4096) % (64MiB - 4096)，复用 Arc 公式
//!   - 4 轮 untimed warmup，再计时 ROUNDS 轮
//!
//! 惯用路径：Rust 在 Windows 上文件 IO 走阻塞线程池（tokio::fs 同构），无真异步 IOCP。
//! 此处用 std 固定线程池 + `FileExt::seek_read`（Windows 位置无关读），worker-local buffer 复用，
//! 如实度量「线程池并发异步文件读」吞吐。输出 `OK:` 行供 `run-async-io-cmp.ps1` 解析。

use std::fs::File;
use std::io;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const FILE_SIZE: u64 = 64 * 1024 * 1024;
const BUF: usize = 4096;
const K: u32 = 4096;
const ROUNDS: u32 = 256;

fn offset(j: u32) -> u64 {
    ((j as u64) * BUF as u64) % (FILE_SIZE - BUF as u64)
}

fn main() -> io::Result<()> {
    let path = std::env::temp_dir().join("adv_async_io_rust.tmp");

    // 创建 64 MiB 测试文件。
    {
        let mut f = File::create(&path)?;
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..64 {
            use std::io::Write;
            f.write_all(&chunk)?;
        }
    }

    // 以共享读写句柄打开（支持并发位置无关读）。
    let file = Arc::new(
        File::options()
            .read(true)
            .write(true)
            .share_mode(0x1 | 0x2) // FILE_SHARE_READ | FILE_SHARE_WRITE
            .open(&path)?,
    );

    let n_workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8).max(2);

    // 固定线程池：job 通道派发，完成通道回报。worker-local buffer 复用（与 Arc buffer 池同构）。
    let (job_tx, job_rx) = channel::<u32>();
    let (done_tx, done_rx) = channel::<u32>();
    let job_rx = Arc::new(Mutex::new(job_rx));
    for _ in 0..n_workers {
        let file = file.clone();
        let job_rx = job_rx.clone();
        let done_tx = done_tx.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; BUF];
            loop {
                let job = { job_rx.lock().unwrap().recv() };
                match job {
                    Ok(j) => {
                        let _ = file.seek_read(&mut buf, offset(j));
                        let _ = done_tx.send(j);
                    }
                    Err(_) => break,
                }
            }
        });
    }
    drop(done_tx);

    fn run_round(job_tx: &Sender<u32>, done_rx: &Receiver<u32>) {
        for j in 0..K {
            let _ = job_tx.send(j);
        }
        for _ in 0..K {
            let _ = done_rx.recv();
        }
    }

    // warmup（不计时）
    for _ in 0..4 {
        run_round(&job_tx, &done_rx);
    }

    let t0 = Instant::now();
    for _ in 0..ROUNDS {
        run_round(&job_tx, &done_rx);
    }
    let ns = t0.elapsed().as_nanos() as f64;

    drop(job_tx); // 断开 job 通道，worker 退出。

    let ops = (K as f64) * (ROUNDS as f64);
    let ns_op = ns / ops;
    let ops_s = ops * 1e9 / ns;
    println!("OK: async_io_rust ops={ops} ns_total={ns} ns_per_op={ns_op:.2} ops_per_s={ops_s:.0}");
    Ok(())
}
