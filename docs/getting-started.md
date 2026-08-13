# Getting started

## Install the CLI

```console
cargo install cargo-bylaw
```

For architecture rules written as Rust tests, add the library as a development
dependency:

```toml
[dev-dependencies]
bylaw = "0.1"
```

## Configure the CLI

Create `bylaw.toml` beside the workspace `Cargo.toml`:

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
scope = "both"
because = "domain policy must not depend on infrastructure"
```

Run the check:

```console
cargo bylaw check
```

## Write a Rust architecture test

```rust
use bylaw::analyzer::{AnalysisOptions, analyze_workspace};
use bylaw::prelude::*;

#[test]
fn architecture_is_valid() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")).into();
    let graph = analyze_workspace(&AnalysisOptions {
        manifest_path,
        ..AnalysisOptions::default()
    })?;

    rules()
        .forbid_dependencies(
            "domain-is-internal",
            modules(["shop::domain::**"]),
            modules(["shop::persistence::**"]),
        )
        .check(&graph)?
        .assert();

    Ok(())
}
```

The graph is imported once and can be evaluated by any number of built-in or
custom rules.

## Analyze a different build configuration

Architecture depends on the selected features, target, and Cargo targets:

```toml
[analysis]
features = ["postgres"]
target = "x86_64-unknown-linux-gnu"
target_kinds = ["library", "binary"]
```

See [Configuration](configuration.md) for every option.
