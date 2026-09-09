# SpectraLang

> A Rust-implemented language and toolchain for AI/ML workloads and first-class API services.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org)
[![Status: Active Development](https://img.shields.io/badge/Status-Active%20Development-blue.svg)](#project-status)
[![Platforms](https://img.shields.io/badge/Platforms-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#prerequisites)

SpectraLang is an language and toolchain implemented in Rust, organized around two complementary workstreams: an **AI/ML core** (tensors, autodiff, numerics, ML framework) and an **API platform** (async/await, `spectra.api` package, HTTP server and routing, JSON, middleware, TLS, drivers, observability). The repository contains the front-end compiler, semantic analysis, SSA-based midend, Cranelift backend, runtime, CLI, LSP, examples, and installer assets.

The project is in **active development**. The language surface grows continuously, with new features landing in the compiler, standard library, and tooling on a regular cadence. See [Project Status](#project-status) for a snapshot of what works today.

---

## Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Tech Stack](#tech-stack)
- [Repository Layout](#repository-layout)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [CLI Essentials](#cli-essentials)
- [Project Status](#project-status)
- [Roadmap & Planning](#roadmap--planning)
- [Contributing](#contributing)
- [License](#license)
- [Acknowledgments](#acknowledgments)

---

## Overview

SpectraLang targets two production workstreams that are first-class in the language and the standard library:

- **AI/ML core** — A tensor-first language and runtime with a scientific type system, shape-aware operations, reverse-mode autodiff, an ML framework layer (modules, losses, optimizers, datasets), accelerator backends, and interop with NumPy/ONNX. The AI track is the project's historical and primary direction.
- **API platform** — First-class async/await, typed HTTP primitives (`Request`, `Response`, `Method`, `Status`, `Header`), routing, middleware, JSON, TLS, and database drivers, delivered through the `spectra.api` package. The API workstream is in active development on top of the async core.

Both workstreams share the same compiler, runtime, and tooling. A single `spectralang` binary is the entry point for compile, check, run, lint, bench, format, REPL, project scaffolding, and package management.

---

## Key Features

### Compiler & Language

- Lexer, parser, AST, semantic analysis, and linting
- Stable core control flow: `if`, `unless`, `while`, `while let`, `do-while`, `for`, `loop`, `switch`, `match`
- First-class tensors with shape and dtype constraints (`Tensor<T, R>`)
- Differentiable regions and gradient contracts as language-level constructs
- Generic types, traits, closures, destructuring, and pattern matching
- Stable, machine-readable diagnostics with error codes
- Source-level stack traces and AOT debug map emission

### AI/ML Core

- SSA-based intermediate representation with optimization passes
- Cranelift-based JIT execution and object file emission
- First-class tensors, autodiff, and a graph IR for fusion and accelerator lowering
- CPU kernel library with vectorization and BLAS integration
- ML framework layer: layers, losses, optimizers, datasets, dataloaders
- ONNX import/export, NumPy `.npy` interchange, and Python/FFI bridges
- Experiment tracking, distributed-training foundations, and reproducibility tooling
- Inference serving foundations and a local toy inference server

### API Platform (in active development)

- `spectra.api` package: HTTP/1.1 server and client, JSON, routing, middleware
- Typed HTTP primitives: `Request`, `Response`, `Method`, `Status`, `Header`, `Cookie`
- Async/await first-class in the language and runtime, with a platform-aware reactor
- TLS, authentication, validation, and structured error handling on the roadmap
- Database drivers for PostgreSQL, SQLite, and Redis (in development)
- Observability: OpenTelemetry tracing, Prometheus metrics, audit logs, health checks (planned)

### Developer Experience

- `spectralang` CLI: `compile`, `check`, `run`, `lint`, `bench`, `repl`, `new`, `fmt`, `package`, `release-info`
- Language server (`spectra-lsp`) with hover, go-to-definition, references, rename, completion, semantic tokens
- Built-in formatter (`spectralang fmt`)
- Project scaffolder (`spectralang new`)
- Package manager with lockfiles, Git sources, catalogs, and offline mode
- Windows installer assets

---

## Tech Stack

| Area | Tool / Crate | Role |
|---|---|---|
| Implementation | Rust (stable) | Compiler, runtime, tooling |
| Build | Cargo (workspace) | Build, test, benchmark |
| IR | Custom SSA IR | Midend, optimization, graph IR |
| Codegen | Cranelift | JIT and AOT object emission |
| Async runtime | Custom reactor (epoll / IOCP / kqueue) | Async/await host |
| Numerics | Custom kernel library, BLAS integration | Tensor CPU kernels |
| Interop | C/Rust FFI, Python bindings, ONNX, NumPy | Ecosystem bridge |
| Package | Git-backed catalogs, SHA-256 checksums, lockfile v2 | Distribution |
| Language server | `spectra-lsp` | Editor integration |
| Installer | PowerShell-based Windows installer assets | Distribution |

---

## Repository Layout

| Path | Purpose |
|---|---|
| `compiler/` | Lexer, parser, AST, semantic analysis, linting, compilation pipeline |
| `midend/` | IR lowering, validation, optimization passes, graph IR |
| `backend/` | JIT and AOT code generation using Cranelift |
| `runtime/` | Runtime services, memory management, FFI, stdlib hooks, async reactor |
| `runtime/src/api/` | HTTP parser, server, client, JSON, TLS, routing (the `spectra.api` surface) |
| `runtime/src/reactor/` | Platform-specific event loop (`epoll` / `IOCP` / `kqueue`) |
| `tools/spectra-cli/` | `spectralang` command-line interface |
| `tools/spectra-lsp/` | Language server implementation |
| `tools/spectra-interop/` | Language interop helpers |
| `examples/` | Sample Spectra programs (basic, AI, API, projects) |
| `tests/` | Language, semantic, CLI, and project tests |
| `docs/` | Language docs, project docs, implementation planning docs |
| `docs/api/` | API library reference (under active development) |
| `roadmap/` | Machine-readable roadmap tracking (`roadmap.toml`) |
| `packages/spectra-api/` | The `spectra.api` package that delivers the API platform surface (in development) |
| `installer/` | Installer scripts and packaging assets |
| `python/` | Python interop demo and helpers |
| `fuzz/` | Fuzz targets for compiler stages |
| `scripts/` | Validation, benchmark, and reporting scripts |
| `AGENTS.md` | Repository-specific instructions for coding agents |
| `ARCHITECTURE.md` | High-level architecture notes |

---

## Prerequisites

- **Rust** stable toolchain (install via [rustup](https://rustup.rs))
- **Cargo** (bundled with Rust)
- **Git**
- On **Windows**: MSVC build tools (Visual Studio Build Tools with the "Desktop development with C++" workload) if you intend to build native artifacts or installer assets
- On **Linux**: a C/C++ toolchain (`build-essential` or equivalent) for Cranelift and BLAS
- On **macOS**: Xcode Command Line Tools (`xcode-select --install`)

---

## Installation

### 1. Clone the repository

```bash
git clone https://github.com/Estevaobonatto/SpectraLang.git
cd SpectraLang
```

### 2. Build the full workspace

```bash
cargo build
```

This builds the compiler, midend, backend, runtime, CLI, LSP, interop, and all dependencies.

### Optional: build a subset

```bash
# CLI only
cargo build -p spectra-cli

# Language server only
cargo build -p spectra-lsp

# Interop helpers only
cargo build -p spectra-interop
```

### Optional: install the CLI locally

```bash
cargo install --path tools/spectra-cli
```

After this, `spectralang` is available on your `PATH`.

### Optional: Windows installer

See the scripts and assets under `installer/` for packaging the CLI for Windows. The installer assets are maintained alongside the project and are not part of the default `cargo build` flow.

---

## Quick Start

### Hello, Spectra — a basic program

Create a file `hello.spectra`:

```spectra
module hello;

pub fn main() {
    let x = 10;
    let y = 20;
    let sum = x + y;
    return;
}
```

Run it with the JIT:

```bash
spectralang run hello.spectra
```

### Run a bundled example

The repository ships with a small, self-contained starter program:

```bash
spectralang run examples/basic.spectra
```

### Build a tiny HTTP service (API platform)

The `examples/api/00_hello_http.spectra` example exercises the `spectra.api` server surface (routing, handlers, requests, responses, listen/shutdown):

```bash
spectralang run examples/api/00_hello_http.spectra
```

The first few lines of that file illustrate the API surface — a `Router`, a registered `Handler`, and a `Server` you `listen` on and `serve` until `shutdown`:

```spectra
module hello_http;

import { Server, new, listen, serve, shutdown } from std.api.server;
import { Router, Route, router, get, route_id } from std.api.routing;
import { HandlerHandle, text, with_header, register_sync, dispatch_sync } from std.api.handler;
import { Request, Response, method_get, request, response_status } from std.api.http;
```

The full, runnable example (with handler registration, request dispatch, listen, serve, and shutdown) lives at [`examples/api/00_hello_http.spectra`](examples/api/00_hello_http.spectra). For a CRUD-style variant, see [`examples/api/01_rest_crud.spectra`](examples/api/01_rest_crud.spectra).

### Run a tensor graph (AI core)

The `examples/ai/tensor_graph_elementwise_fusion.spectra` example uses the tensor runtime and exercises the graph IR's elementwise fusion path:

```bash
spectralang run examples/ai/tensor_graph_elementwise_fusion.spectra
```

The example:

```spectra
module ai_tensor_graph_elementwise_fusion;

import std.tensor as tensor;

pub fn main() -> int {
    tensor.free_all();
    tensor.reset_stats();

    let input = tensor.full_f(8, 4.0);
    let activated = tensor.relu(input);
    let normalized = tensor.sqrt_f(activated);
    let projected = tensor.tanh_f(normalized);

    if tensor.len(projected) != 8 { return tensor.len(projected); }
    if tensor.get_f(projected, 0) <= 0.0 { return 1; }
    if tensor.stats_kernel_ops() < 3 { return 2; }

    tensor.free_all();
    return 0;
}
```

### More examples

```bash
ls examples/
# basic, control flow, methods, algorithms, math, fibonacci, calculator, ...

ls examples/ai/
# mlp, cnn, transformer, onnx, rag, distributed training, monitoring, ...

ls examples/projects/
# boletim, complex_demo, multi_file, poliedro, spectra_academy, test_corpus
```

---

## CLI Essentials

The CLI is invoked as `spectralang <COMMAND> [OPTIONS] <paths>...`. Run `spectralang help` for the full reference. The most common commands:

| Command | Description |
|---|---|
| `spectralang compile <paths>` | Compile Spectra modules (default). |
| `spectralang check <paths>` | Type-check modules and report diagnostics without executing. |
| `spectralang run <paths>` | Compile modules and execute the entry point via JIT. |
| `spectralang lint <paths>` | Run lint checks and report warnings or denied rules. |
| `spectralang bench <paths>` | Compile with timing metrics and optional JSON report. |
| `spectralang fmt <paths>` | Format Spectra source files. |
| `spectralang repl` | Start an interactive Spectra prompt. |
| `spectralang new <path>` | Scaffold a new Spectra project with a starter module and manifest. |
| `spectralang package <action>` | Resolve, lock, build, publish, and consume packages. |
| `spectralang release-info` | Report CLI and package release channel metadata. |

Examples:

```bash
spectralang check examples/
spectralang lint src/
spectralang compile --dump-ir project/
spectralang run -O3 app.spectra
spectralang bench --bench-json target/bench.json src/
spectralang fmt src/
spectralang fmt --stdin < file.spectra
spectralang new my-project
spectralang package add math --path ../math
spectralang release-info --json --root .
```

Exit codes:

| Code | Meaning |
|---|---|
| `0` | Success |
| `64` | Usage error (invalid flags, missing inputs) |
| `65` | Compilation failed |
| `74` | I/O failure while reading or writing files |

---

## Project Status

SpectraLang is **not yet a stable production language**. The current state, honestly:

- The language reference is being built out continuously as the language grows; some constructs still evolve between releases.
- Core control flow (`switch`, `unless`, `do-while`, `loop`) is stable and parses unconditionally.
- The AI/ML core (tensors, autodiff, ML framework, ONNX, NumPy interop, GPU backend baseline) is implemented and exercised by the bundled examples.
- The API platform (`spectra.api`) is in active development. The HTTP server, routing, and handler surfaces are exercised by `examples/api/00_hello_http.spectra` and `examples/api/01_rest_crud.spectra`, but middleware, TLS, database drivers, and observability are still landing.
- Cross-file and cross-module linkage has improved substantially (strings, type/method resolution, aggregate codegen), but the boundaries are still being hardened.
- Standalone executable generation is not yet fully integrated end-to-end; the JIT path is the primary execution mode.
- The standard library is still incomplete relative to the long-term plan.

The full list of completed, in-progress, and planned work lives in the [roadmap](#roadmap--planning).

---

## Roadmap & Planning

SpectraLang tracks its work in three canonical planning artifacts. **The README does not duplicate their content** — refer to them for details, status, and acceptance criteria:

- **Strategic plan:** [`docs/production-ai-implementation-plan.md`](docs/production-ai-implementation-plan.md) — long-term vision, workstream decomposition, production-gap coverage, and acceptance criteria by phase.
- **Human-readable backlog:** [`docs/roadmap-backlog.md`](docs/roadmap-backlog.md) — issue-ready work breakdown, prioritization, and recommended execution ordering.
- **Structured roadmap:** [`roadmap/roadmap.toml`](roadmap/roadmap.toml) — machine-readable project planning for automation, reporting, and dependency tracking.

The project targets 30+ phases covering compiler productionization, scientific types, the tensor core, numerics, autodiff, ML framework, accelerators, interop, packaging, tooling, concurrency, security, docs, AI language core, numerical performance, accelerator compilation, data and experiment platform, model ecosystem, AI ops, production certification, async core, the `spectra.api` library, middleware and security, advanced API features, persistence, API tooling, observability, API conformance, and production reality / ML systems gap closure.

---

## Contributing

Contributions are welcome. SpectraLang uses a planning-driven workflow:

1. **Read the planning files first.** Skim `AGENTS.md`, then `docs/roadmap-backlog.md` to find work that matches your interest, and the relevant item in `roadmap/roadmap.toml` for the canonical ID (`R-####`), owner, and acceptance criteria.
2. **Pick or open a roadmap item.** Small, well-scoped items are the easiest path. Larger items should reference the roadmap ID and the strategic plan section.
3. **Follow the standard GitHub flow.**
   ```bash
   # fork the repository, then:
   git checkout -b feature/r-XXXX-short-description
   # ... implement, add tests ...
   git commit -m "feat: short summary referencing R-XXXX"
   git push origin feature/r-XXXX-short-description
   # open a pull request against the main branch
   ```
4. **Match the repository's standards.**
   - Follow the patterns established in neighboring crates and tests.
   - For core language changes (variables, control flow, methods, traits, closures, return paths), add or update at least one regression in `tests/validation/` or `tests/errors/` and run the frontend and midend tests.
   - For lowering/JIT/AOT/runtime changes, run a `spectralang run` validation end-to-end.
   - Keep changes narrow, do not introduce unrelated refactors, and do not mark items complete without validated evidence.
5. **Update planning files when relevant.** If your change moves a tracked item forward, update `roadmap/roadmap.toml` (status, dependencies) and `docs/roadmap-backlog.md` (notes) in the same change. Only update `docs/production-ai-implementation-plan.md` if strategy or architecture changed.
6. **Be honest about the current state.** Do not claim production readiness, performance wins, or "complete" without checked-in evidence and validation commands.

Issues, bug reports, and documentation fixes are valuable contributions on their own.

---

## License

This repository is licensed under the **MIT License**. See [LICENSE](LICENSE) for the full text.

```
MIT License
Copyright (c) 2026 SpectraLang Project
```

The MIT License is a permissive open-source license: anyone may use, copy, modify, merge, publish, distribute, sublicense, and sell copies of the software, provided that the copyright notice and permission notice are preserved. The SpectraLang Project retains copyright.

---

## Acknowledgments

- **[Rust](https://www.rust-lang.org/)** — the implementation language for the entire toolchain.
- **[Cranelift](https://github.com/bytecodealliance/wasmtime/tree/main/cranelift)** — code generator for JIT and AOT execution.
- The **NumPy**, **ONNX**, **BLAS**, and broader open-source ML ecosystems — for the interop targets that SpectraLang integrates with.
- The SpectraLang **contributors and early adopters** who run the toolchain, report issues, and shape the language and standard library.
