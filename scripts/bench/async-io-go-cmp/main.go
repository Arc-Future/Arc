// async IO 对比 harness：与 Arc `adv_async_io_throughput_e2e` 同构（该 bench 已随 arc-integration 退场，a2627a0f；同 K/ROUNDS/offset 公式）。
//
// 镜像 Arc 工作负载：
//   - 64 MiB 文件，单 fd
//   - 每轮批量提交 K=4096 个 4KB offset 读 → 全部完成
//   - ROUNDS=256，总 ops = 1,048,576
//   - offset(j) = (j*4096) % (64MiB - 4096)，复用 Arc 公式
//   - 4 轮 untimed warmup，再计时 ROUNDS 轮
//
// 惯用路径：Go 的 os.File 无真异步文件 IO，用 goroutine + `ReadAt`（并发安全，阻塞线程池）。
// 输出 `OK:` 行供 `run-async-io-cmp.ps1` 解析。

package main

import (
	"fmt"
	"os"
	"sync"
	"time"
)

const (
	fileSize = int64(64 * 1024 * 1024)
	bufSize  = 4096
	k        = 4096
	rounds   = 256
)

func offset(j int) int64 {
	return (int64(j) * bufSize) % (fileSize - bufSize)
}

func main() {
	path := os.TempDir() + string(os.PathSeparator) + "adv_async_io_go.tmp"

	// 创建 64 MiB 测试文件。
	{
		f, err := os.Create(path)
		if err != nil {
			panic(err)
		}
		chunk := make([]byte, 1024*1024)
		for i := range chunk {
			chunk[i] = 'x'
		}
		for i := 0; i < 64; i++ {
			if _, err := f.Write(chunk); err != nil {
				panic(err)
			}
		}
		f.Close()
	}

	f, err := os.OpenFile(path, os.O_RDWR, 0644)
	if err != nil {
		panic(err)
	}
	defer f.Close()

	// 预分配 K 个 buffer（各并发读独立，跨轮复用）。
	bufs := make([][]byte, k)
	for j := 0; j < k; j++ {
		bufs[j] = make([]byte, bufSize)
	}

	runRound := func() {
		var wg sync.WaitGroup
		wg.Add(k)
		for j := 0; j < k; j++ {
			go func(j int) {
				defer wg.Done()
				_, _ = f.ReadAt(bufs[j], offset(j))
			}(j)
		}
		wg.Wait()
	}

	// warmup（不计时）
	for w := 0; w < 4; w++ {
		runRound()
	}

	start := time.Now()
	for r := 0; r < rounds; r++ {
		runRound()
	}
	ns := float64(time.Since(start).Nanoseconds())

	ops := float64(k) * float64(rounds)
	nsOp := ns / ops
	opsS := ops * 1e9 / ns
	fmt.Printf("OK: async_io_go ops=%.0f ns_total=%.0f ns_per_op=%.2f ops_per_s=%.0f\n", ops, ns, nsOp, opsS)
}
