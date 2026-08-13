# Configuration

Cargo Bylaw discovers `bylaw.toml` from the current directory upward, or accepts
`--config`. Unknown fields and unsupported configuration versions are errors.

## Analysis

```toml
version = 1

[analysis]
manifest_path = "Cargo.toml"
packages = ["shop-api"]
features = ["postgres"]
target_kinds = ["library", "binary"]
incomplete = "deny"
proc_macros = true
build_scripts = true

[output]
format = "human"
```

Library and binary targets are analyzed by default. Tests, examples, benches,
build scripts, and proc-macro targets must be selected explicitly. `incomplete =
"deny"` is the default; `cargo bylaw check --allow-incomplete` changes
incompleteness to warnings for that invocation.

Output is `human` by default and can be set to `json`; `--format` overrides the
configuration for one invocation. JSON output uses a top-level `version` field
and either a structured `report` or `error` object.

`cargo bylaw check` exits with `0` for a successful check, `1` for architecture
violations, `2` for configuration errors, and `3` for analyzer failures.

## Selectors

```toml
[selectors.domain]
packages = ["shop-domain"]

[selectors.domain-modules]
crates = ["shop"]
modules = ["shop::domain::**"]

[selectors.serialization]
external_crates = ["serde", "serde_json"]
```

Values within a field are alternatives. Different fields are combined, so the
`domain-modules` selector means modules matching the path in the `shop` crate.
Rust path patterns use `*` for one `::` segment and `**` for zero or more.

## Dependency rules

```toml
[[rule]]
id = "domain-is-internal"
kind = "forbid-dependencies"
from = "domain"
to = ["persistence", "api"]
scope = "both"
because = "domain policy must not depend on adapters"

[[rule]]
id = "domain-allowlist"
kind = "only-dependencies"
from = "domain"
allowed = ["domain", "serialization"]
scope = "actual"
allow_toolchain = true
allow_self = true
```

`scope` is `actual`, `declared`, or `both`.

## Layers

```toml
[[rule]]
id = "application-layers"
kind = "layers"
scope = "both"

[[rule.layers]]
name = "domain"
selector = "domain"

[[rule.layers]]
name = "persistence"
selector = "persistence"

[[rule.dependencies]]
from = "persistence"
may_depend_on = ["domain"]
```

Omitting a layer from `may_depend_on` forbids that direction. Dependencies
within the same layer are allowed.

## Cycles

```toml
[[rule]]
id = "model-crates-are-acyclic"
kind = "no-cycles"
within = ["domain", "persistence", "api"]
grouping = "crates"
scope = "actual"
```

Grouping can be `components`, `modules`, `crates`, or named `slices`.
