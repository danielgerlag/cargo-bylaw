# Cargo Bylaw

Cargo Bylaw is an architecture enforcement framework for Rust, inspired by
ArchUnit. It builds a semantic dependency graph with rust-analyzer and checks
rules from either normal Rust tests or `bylaw.toml`.

```console
cargo install cargo-bylaw
cargo bylaw check
```

If Cargo cannot find the installed subcommand, follow the
[PATH troubleshooting steps](getting-started.md#install-the-cli).

The initial rule library supports:

- Forbidden and allow-listed dependencies.
- Layered architecture.
- Cycle detection across modules, crates, or named slices.
- Actual source references and Cargo manifest declarations.
- Workspace modules, workspace crates, external crates, and toolchain crates.

Analysis runs on stable Rust and fails closed when semantic information is
incomplete unless the user explicitly allows warnings.

## Guardrails for coding agents

Cargo Bylaw gives coding agents an executable definition of the architecture.
Agents can run the same check as CI, receive source-level dependency violations,
and revise their changes instead of relying on architecture prose alone.

See [Keeping coding agents aligned](coding-agents.md) for a recommended agent
instruction, workflow, and CI gate.

## Documentation

- Start with [Getting started](getting-started.md).
- Add [guardrails for coding agents](coding-agents.md).
- See the runnable [model boundaries example](examples.md).
- Read [Semantic analysis](analysis.md) for cfg, macro, and build-script behavior.
- Extend the framework with [Custom rules](extensions.md).

## API documentation

Published crates receive API documentation from docs.rs:

- [`bylaw`](https://docs.rs/bylaw)
- [`bylaw-core`](https://docs.rs/bylaw-core)
- [`bylaw-analyzer`](https://docs.rs/bylaw-analyzer)
- [`bylaw-config`](https://docs.rs/bylaw-config)
- [`cargo-bylaw`](https://docs.rs/cargo-bylaw)
