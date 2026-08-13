# Architecture

Cargo Bylaw separates stable public contracts from the rust-analyzer integration:

```text
bylaw-core <--- bylaw-config
     ^              ^
     |              |
bylaw-analyzer      |
     ^              |
      \            /
          bylaw
            ^
            |
       cargo-bylaw
```

`bylaw-core` owns the immutable architecture graph, selectors, extension traits,
built-in rule specifications, and structured reports. It has no dependency on
Cargo or rust-analyzer.

`bylaw-analyzer` imports a Cargo workspace into that graph. All `ra_ap_*` types
remain private so a rust-analyzer upgrade cannot break downstream rule code.

`bylaw-config` strictly parses versioned `bylaw.toml` files and lowers built-in
rules into the same `BuiltInRuleSpec` values used by the Rust DSL.

`bylaw` provides the fluent test API, assertion integration, report formatting,
and re-exports needed by custom rules. `cargo-bylaw` is the stock CLI.

## Graph model

Components are workspace crate targets, Rust modules, or external/toolchain
crates. Canonical IDs distinguish Cargo package names, Rust crate names, and
target kinds.

Edges have one of two scopes:

- `actual`: a semantic source reference resolved by rust-analyzer.
- `declared`: a Cargo manifest dependency.

An edge retains all known evidence spans instead of duplicating graph edges.
Rules can evaluate actual dependencies, declared dependencies, or both.

Analysis completeness is part of the graph. Unresolved paths, unavailable macro
expansions, and skipped requested targets are diagnostics rather than silent
omissions.
