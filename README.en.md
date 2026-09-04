# Arc

English | [简体中文](README.md)

Arc is a **purely AOT-compiled, systems-level programming language** designed for the era of human-agent collaboration. It takes **idiomatic C# surface syntax** as its baseline, blends in Rust-style memory-safety semantics, and compiles ahead-of-time to native machine code — no stop-the-world garbage collector. Core abstractions (types, LINQ, `Expression<T>`, compile-time metaprogramming) are expanded at **compile time and link time**, never interpreted at runtime.

## Positioning

The four-factor design equation ([RFC 001 Language Charter](docs/rfc/001-language-charter.md)):

```
Arc = Readability × Compile-time Safety × AOT Determinism × Human-Agent Collaboration
```

The four factors **multiply** rather than add — if any one of them is zero, the language's value is zero.

## Five Design Tenets

| Tenet | Meaning | Core Mechanisms |
|-------|---------|-----------------|
| ① Readable means collaborative | Programs are written first for humans and agents to read together | Leading-type declarations, single idiom, deterministic formatting, declarative queries |
| ② Safety is settled at compile time | A runtime crash is a design failure, not a normal state of programs | Resource ownership, borrow constraints, exhaustive matching, explicit error chains |
| ③ Behavior is determined at compile time | Predictability is the bottom line for systems software | Pure AOT, no STW GC, zero-cost abstractions, deterministic output |
| ④ Code is data | Program structure is data that can be analyzed, transformed, and transferred | Expression trees, Provider pattern, dual-path queries |
| ⑤ Built for human-agent co-writing | AI agents are first-class collaborators | Structured diagnostics, local reasoning, explicit capabilities, declarable contracts |

See the [Language Charter](docs/rfc/001-language-charter.md) and the [Language Manifesto](docs/manifesto.md) for details.

## Current Status

**Arc 1.0** (2026-09-04) — the first stable release of the language, compiler, standard library, and runtime: a single `arc` executable, a source-distributed standard library, and runtime C sources shipped with the package (compiled on demand through a content-addressed cache on first build). AOT compilation to native machine code, no JIT runtime. The project is still under active evolution; we do not claim full C# parity. See the [CHANGELOG](CHANGELOG.md) for version history and the [Maturity Charter](docs/rfc/036-maturity.md) for governance.

- **Milestones**: F0–M3 ✅ (assets never rolled back); M4 schedulable, not started; M5–Mn layer-by-layer self-hosting (HIR / typeck / codegen) in progress.
- **Self-hosting**: the compiler is currently a **Rust bootstrap implementation** (`crates/*`); the default CLI remains the Rust compiler until the Arc self-hosted compiler reaches equivalence (Mn).
- **Claim discipline**: the foundation (language core / `rt_*` ABI / `std/Arc` Stable) is frozen by default; breaking changes require an RFC first. No claims without a falsifiable acceptance protocol.

## Quick Start

### Requirements

| Item | Requirement |
|------|-------------|
| Rust | Rust toolchain (`cargo`, stable) |
| LLVM | LLVM 22+ (`clang` ≥ 22.0.0; enforced by `arc doctor`) |

### Build the compiler

```bash
cargo build --release
cargo test --workspace
```

### Usage

```bash
# Show the version
cargo run -p arc -- --version

# Environment self-check (like `rustup doctor`)
cargo run -p arc -- doctor

# Type check (with borrow check, no codegen)
cargo run -p arc -- check examples/CompilerSmoke/Program.as

# Compile to a native binary (host target)
cargo run -p arc -- build examples/CompilerSmoke/Program.as -o hello.exe

# Compile and run
cargo run -p arc -- run examples/CompilerSmoke/Program.as
```

Hello World (`Program.as`):

```as
using Arc;

void Main() {
    Console.WriteLine("Hello, Arc!");
}
```

See [Installation & Quick Start](docs/user-guide/01-getting-started.md) and [Build & Run](docs/user-guide/02-build-run.md) for details.

## Project Layout

```
arc/
├── crates/
│   ├── arc/             # CLI driver (env / doctor / toolchain / component / release / self-update / publish / new / detect / inspect)
│   ├── parse/           # Parsing
│   ├── ast/             # Abstract syntax tree
│   ├── hir/             # High-level IR
│   ├── typeck/          # Type checking
│   ├── mir/             # Mid-level IR
│   ├── codegen/         # LLVM 22 native code generation
│   ├── reachability/    # L2 entry-point reachability analysis
│   ├── arc-server/      # LSP server
│   ├── arcgr/           # .arcgr semantic index
│   ├── arc-ui/          # Declarative UI (.arml parsing / typeck / inspect)
│   ├── arc-ssr/         # SSR template compilation
│   ├── arc-tests/       # Layered tests (L1 fast tests / L2 full-rt gated)
│   ├── runtime/         # Runtime ABI (pure C sources)
│   ├── runtime-crypto/  # vendored mbedTLS
│   ├── runtime-drawing/ # vendored qrcodegen / quirc / stb
│   ├── runtime-iree/    # vendored IREE
│   ├── runtime-onnx/    # vendored ONNX Runtime
│   ├── runtime-quic/    # vendored ngtcp2 + OpenSSL (QUIC/TLS)
│   ├── runtime-sqlite/  # vendored SQLite
│   └── runtime-ui/      # UI runtime ABI + wgpu-native rendering backend
├── std/                 # Standard library (.as)
├── docs/                # The Arc Language Book (white-paper / user-guide / domain / rfc)
└── examples/            # Example solutions (CompilerSmoke / ArmlDemo / ArcAgent / ReviewAgent, etc.)
```

## Documentation

- [The Arc Language Book · Table of Contents](docs/SUMMARY.md)
- [Preface](docs/preface.md)
- [Language Manifesto](docs/manifesto.md)
- [White Paper](docs/white-paper/index.md)
- [User Guide](docs/user-guide/index.md)
- [Domain Libraries](docs/domain/index.md)
- [RFC Design Decisions](docs/rfc/index.md)
- [CHANGELOG](CHANGELOG.md)

## Installation / SDK (release candidate)

Arc ships a CLI tool with a .NET-CLI look and feel plus SDK capabilities, currently a **release candidate** (not an official release):

- `arc env` — environment variables and SDK layout
- `arc doctor` — environment self-check (clang/LLVM 22 baseline)
- `arc toolchain` — toolchain install / management
- `arc component` — component install / management
- `arc release` — signed release manifests (generate / verify / keygen)
- `arc self-update` — self-update distribution
- `arc publish` — package publishing (`.aopkg` source distribution package)
- `arc new` / `arc detect` — scaffolding and project detection

## Author

- **Author**: LUSIDA (Start) — creator of the Arc language
- **Email**: 474309146@qq.com
- **Website**: [www.lusida.net](https://www.lusida.net)
- **Copyright**: Copyright (c) 2026 LUSIDA (Start). All rights reserved.

## License

This project is licensed under the [MIT License](LICENSE).

> Vendored third-party code under `crates/runtime-*/` (SQLite, mbedTLS, qrcodegen / quirc / stb, IREE, ONNX Runtime, ngtcp2 / OpenSSL, wgpu-native) retains its own independent licenses as documented in the respective `NOTICE` / `VENDOR.md` attribution files, and is not covered by this repository's MIT license.
