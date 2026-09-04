// Arc 性能对照 C++ harness（RFC 099 §2.3 / 08-rfcs V1-SPRINT 轨道 G）。
//
// 与 Arc `std_hotpath_bench_e2e` 同构（该 bench 已随 arc-integration 退场，a2627a0f）：List/Dict/HS/StringBuilder/File IO 五场景，
// 输出 `OK:` 行供 `run_std_hotpath_*` 脚本解析成对比（与 rust-hotpath 同格式）。
//
// 构建：clang++ -O2 -DNDEBUG -o cxx-hotpath.exe main.cpp
// 运行：cxx-hotpath.exe
//
// 注意：C++ 是「对标 C/C++/Rust」的第三个 anchor。与 rust-hotpath 同场景、同 N、
// 同 ops 计数，仅换标准库实现（std::vector / std::unordered_map / std::unordered_set /
// std::string）。仅作同机锚点，不宣称业界领先。

#include <chrono>
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <vector>

// MSVC CRT 将 fopen 标记 deprecated；这里是有意使用可移植 C 流（与 Arc C 基准同构）。
#ifndef _CRT_SECURE_NO_WARNINGS
#define _CRT_SECURE_NO_WARNINGS
#endif

using Clock = std::chrono::steady_clock;

static double now_ns() {
    return (double)std::chrono::duration_cast<std::chrono::nanoseconds>(
               Clock::now().time_since_epoch())
        .count();
}

// 防 DCE：全局可观测副作用（对 volatile 全局累加，与 black_box 作用等价）。
static volatile std::int64_t ARC_SINK = 0;

// 统计化（RFC 013 §1.5 / RFC 044 沿用）：M 轮迭代，取 min 下界，与 Arc 侧同构。
static const int HOT_N_ITERS = 30;

static void report_stat(const char* name, double ops, int iters, double* ns) {
    std::sort(ns, ns + iters);
    double mn = ns[0];
    double p50 = ns[iters / 2];
    int p99i = (iters * 99) / 100;
    if (p99i >= iters) p99i = iters - 1;
    double p99 = ns[p99i];
    double sum = 0.0;
    for (int i = 0; i < iters; i++) sum += ns[i];
    double mean = sum / (double)iters;
    std::printf("OK: %s iters=%d ops=%.0f\n", name, iters, ops);
    std::printf("  min=%.0fns (%.2fns/op) p50=%.2fns/op p99=%.2fns/op mean=%.2fns/op\n",
                mn, mn / ops, p50 / ops, p99 / ops, mean / ops);
    std::printf("  claim: min_per_op=%.2fns (falsifiable lower bound, RFC 013 §1.5)\n",
                mn / ops);
}

int main() {
    // List: Add + index read（与 Arc bench_list_add_get 同构：N=200k，2N ops）
    const std::int32_t N1 = 200000;
    {
        double iters_ns[HOT_N_ITERS];
        for (int it = 0; it < HOT_N_ITERS; it++) {
            std::vector<std::int32_t> v;
            v.reserve(N1);
            double t0 = now_ns();
            for (std::int32_t i = 0; i < N1; i++) v.push_back(i);
            std::int64_t acc = 0;
            for (std::int32_t i = 0; i < N1; i++) acc += v[(std::size_t)i];
            ARC_SINK += acc;
            iters_ns[it] = now_ns() - t0;
        }
        report_stat("list_add_get", (double)N1 * 2.0, HOT_N_ITERS, iters_ns);
    }

    // Dictionary: set + get（与 Arc bench_dict_set_get 同构：N=150k，2N ops）
    const std::int32_t N2 = 150000;
    {
        double iters_ns[HOT_N_ITERS];
        for (int it = 0; it < HOT_N_ITERS; it++) {
            std::unordered_map<std::int64_t, std::int64_t> d;
            d.reserve((std::size_t)N2);
            double t0 = now_ns();
            for (std::int64_t i = 0; i < N2; i++) d.emplace(i, i * 2);
            std::int64_t acc = 0;
            for (std::int64_t i = 0; i < N2; i++) acc += d[i];
            ARC_SINK += acc;
            iters_ns[it] = now_ns() - t0;
        }
        report_stat("dict_set_get", (double)N2 * 2.0, HOT_N_ITERS, iters_ns);
    }

    // HashSet: Add + Contains + duplicate Add（与 Arc bench_hashset_add_contains
    // 同构：N=150k，3N ops）
    const std::int32_t N3 = 150000;
    {
        double iters_ns[HOT_N_ITERS];
        for (int it = 0; it < HOT_N_ITERS; it++) {
            std::unordered_set<std::int64_t> s;
            s.reserve((std::size_t)N3);
            double t0 = now_ns();
            for (std::int64_t i = 0; i < N3; i++) s.insert(i);
            std::int64_t acc = 0;
            for (std::int64_t i = 0; i < N3; i++) acc += s.find(i) != s.end() ? 1 : 0;
            for (std::int64_t i = 0; i < N3; i++) s.insert(i);  // duplicate Add
            ARC_SINK += acc;
            iters_ns[it] = now_ns() - t0;
        }
        report_stat("hashset_add_contains", (double)N3 * 3.0, HOT_N_ITERS, iters_ns);
    }

    // StringBuilder: string push（与 Arc bench_stringbuilder_append 同构：N=100k）
    const std::int32_t N4 = 100000;
    {
        double iters_ns[HOT_N_ITERS];
        for (int it = 0; it < HOT_N_ITERS; it++) {
            std::string sb;
            sb.reserve((std::size_t)N4 * 8);
            double t0 = now_ns();
            for (std::int32_t i = 0; i < N4; i++) sb.push_back('x');
            ARC_SINK += (std::int64_t)sb.size();
            iters_ns[it] = now_ns() - t0;
        }
        report_stat("stringbuilder_append", (double)N4, HOT_N_ITERS, iters_ns);
    }

    // File IO 吞吐：64 KiB 载荷 Write+Read 往返（与 Arc bench_file_io_throughput
    // 同构：N=64，2N ops）
    {
        const std::int32_t N = 64;
        const std::size_t SIZE = 64 * 1024;
        const char* path = "arc_cxx-hotpath_io_big.tmp";
        std::string payload(SIZE, 'x');
        double iters_ns[HOT_N_ITERS];
        for (int it = 0; it < HOT_N_ITERS; it++) {
            double t0 = now_ns();
            for (std::int32_t i = 0; i < N; i++) {
                FILE* f = std::fopen(path, "wb");
                if (!f) { std::fprintf(stderr, "cxx io write open failed\n"); return 1; }
                std::fwrite(payload.data(), 1, SIZE, f);
                std::fclose(f);
                f = std::fopen(path, "rb");
                if (!f) { std::fprintf(stderr, "cxx io read open failed\n"); return 1; }
                // 读入缓冲区并校验长度（防优化；与 Arc 读回校验同构）
                std::string got(SIZE, '\0');
                std::size_t rd = std::fread(&got[0], 1, SIZE, f);
                std::fclose(f);
                if (rd != SIZE) { std::fprintf(stderr, "cxx io read mismatch\n"); return 1; }
            }
            iters_ns[it] = now_ns() - t0;
        }
        std::remove(path);
        report_stat("file_io_throughput", (double)N * 2.0, HOT_N_ITERS, iters_ns);
    }

    return 0;
}