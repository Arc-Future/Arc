# Arc — AOT Systems Programming Language

[English](README.md) | [简体中文](README.zh.md)

**Arc** is a **purely AOT-compiled systems programming language** for human–agent collaboration. It uses **idiomatic C# surface syntax**, Rust-style memory-safety semantics, and ahead-of-time compilation to native code — **no stop-the-world garbage collector**. Core abstractions (types, LINQ, `Expression<T>`, compile-time metaprogramming) expand at **compile time and link time**, never as a runtime interpreter.

> **Not the same as** [Cratis/Arc](https://github.com/Cratis/Arc) (a .NET application framework) or other projects also named “Arc”. This repository is the **Arc language** (`Arc-Future/Arc`): compiler, standard library, and runtime.

## Why Arc

| | Arc | Typical C# | Typical Rust |
|---|-----|------------|--------------|
| Surface | C#-idiomatic (leading types, LINQ, `async`) | C# | Rust |
| Execution | Pure AOT → native | JIT / NativeAOT optional | AOT (LLVM) |
| Memory | Ownership + borrow; no STW GC | GC (STW) | Ownership + borrow |
| Queries / trees | Compile-time LINQ & `Expression<T>` | Often runtime / JIT-shaped | Macros / manual |
| Agents | Structured diagnostics, explicit capabilities | Tooling varies | Tooling varies |

Design equation ([RFC 001](docs/rfc/001-language-charter.md)):

```
Arc = Readability × Compile-time Safety × AOT Determinism × Human-Agent Collaboration
```

The factors **multiply** — if any one is zero, the language’s value is zero. See the [Language Charter](docs/rfc/001-language-charter.md) and [Manifesto](docs/arc/01-introduction/manifesto.md).

## Current status

**Arc 0.1** (pre-1.0 public cut): single `arc` CLI, source-distributed standard library, runtime C sources (content-addressed cache on first build), AOT to native machine code, no JIT. We do **not** claim full C# parity.

- Compiler today: **Rust bootstrap** (`crates/*`); default CLI stays Rust until Arc self-hosting reaches equivalence.
- Foundation (language core / `rt_*` ABI / `std/Arc` Stable) is frozen by default; breaking changes require an RFC.
- Version history: [CHANGELOG](CHANGELOG.md) · maturity governance: [RFC 036](docs/rfc/036-maturity.md).

## Quick start

### Requirements

| Item | Requirement |
|------|-------------|
| Rust | Stable toolchain (`cargo`) |
| LLVM | LLVM 22+ (`clang` ≥ 22.0.0; enforced by `arc doctor`) |

### Build the compiler

```bash
cargo build --release
cargo test --workspace
```

### Usage

```bash
cargo run -p arc -- --version
cargo run -p arc -- doctor
cargo run -p arc -- check examples/CompilerSmoke/Program.as
cargo run -p arc -- build examples/CompilerSmoke/Program.as -o hello.exe
cargo run -p arc -- run examples/CompilerSmoke/Program.as
```

Hello World (`Program.as`):

```as
using Arc;

void Main() {
    Console.WriteLine("Hello, Arc!");
}
```

More: [Getting started](docs/arc/02-quickstart/getting-started.md) · [Build & run](docs/arc/02-quickstart/build-run.md).

## Install / SDK (0.1)

Binaries: [GitHub Releases](https://github.com/Arc-Future/Arc/releases). Public CDN `static.arc.dev` is still a placeholder — set `ARC_RELEASE_BASE` to the Release download root when using self-update.

Useful commands: `arc env`, `arc doctor`, `arc toolchain`, `arc component`, `arc release`, `arc self-update`, `arc publish`, `arc new`, `arc detect`.

## Documentation

| Doc | Purpose |
|-----|---------|
| [llms.txt](llms.txt) | Canonical map for AI agents & crawlers |
| [Arc Developer Handbook](docs/arc/INDEX.md) | **Publishable docs** (Docbit slug `arc`) |
| [Docs navigation](docs/SUMMARY.md) | Handbook + RFC index |
| [Manifesto](docs/arc/01-introduction/manifesto.md) | Design equation & tenets |
| [RFCs](docs/rfc/index.md) | Design authority (not in default publish set) |
| [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) | Contribution & vulnerability reporting |

## Repository layout

```
crates/     # Compiler (Rust) + runtime ABI (C) + vendored natives
std/        # Standard library (.as)
docs/       # arc/ handbook (publishable) + rfc/ design authority
examples/   # Sample solutions (CompilerSmoke, ArmlDemo, ArcAgent, …)
```

## Author

- **Author**: LUSIDA (Start) — creator of the Arc language  
- **Email**: 474309146@qq.com  
- **Website**: [www.lusida.net](https://www.lusida.net)  
- **Copyright**: Copyright (c) 2026 LUSIDA (Start). All rights reserved.

## License

[MIT](LICENSE). Vendored code under `crates/runtime-*/` keeps its own licenses (`NOTICE` / `VENDOR.md`).
