# Cargo Bylaw

Cargo Bylaw is an architecture enforcement framework for Rust. It provides a
Rust test API and a `cargo bylaw` command backed by the same semantic dependency
graph and rule engine.

Rules can constrain dependencies between modules, workspace crates, and external
crates, define allowed layer directions, and reject dependency cycles.

## Install

```console
cargo install cargo-bylaw
cargo bylaw --version
```

Cargo installs subcommands into `$CARGO_HOME/bin`, which defaults to
`~/.cargo/bin`. If Cargo reports `no such command: bylaw`, ensure that directory
is on `PATH` and restart the shell:

```console
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
```

Run architecture checks with the required `check` subcommand:

```console
cargo bylaw check
```

See the [Getting Started guide](docs/getting-started.md) for Windows PATH
instructions and library installation.

## Configure

```toml
version = 1

[selectors.domain]
modules = ["shop::domain::**"]

[selectors.persistence]
modules = ["shop::persistence::**"]

[[rule]]
id = "domain-is-internal"
kind = "forbid-dependencies"
from = "domain"
to = "persistence"
scope = "actual"
```

The same rule model is available from normal Rust tests:

```rust
use bylaw::prelude::*;

#[test]
fn architecture_is_valid() -> Result<(), Box<dyn std::error::Error>> {
    let graph = bylaw::analyzer::analyze_workspace(&Default::default())?;
    rules()
        .forbid_dependencies(
            "domain-is-internal",
            modules(["shop::domain::**"]),
            modules(["shop::persistence::**"]),
        )
        .actual_dependencies()
        .check(&graph)?
        .assert();
    Ok(())
}
```

See [`examples/model-boundaries`](examples/model-boundaries) for a complete
domain, persistence, contract, API, and composition-root example.

## Documentation site

The guide is an mdBook deployed through GitHub Pages. Build it locally with:

```console
cargo install mdbook --version 0.5.4 --locked
mdbook serve --open
```

## Documentation

- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [Custom rules](docs/extensions.md)
- [Semantic analysis](docs/analysis.md)
- [Upgrading rust-analyzer](docs/upgrading-rust-analyzer.md)
- [Releasing](docs/releasing.md)

## License

Licensed under either of:

- [Apache License, Version 2.0](https://github.com/danielgerlag/cargo-bylaw/blob/main/LICENSE-APACHE)
- [MIT License](https://github.com/danielgerlag/cargo-bylaw/blob/main/LICENSE-MIT)

at your option.
