# Semantic analysis

Cargo Bylaw uses `cargo metadata` for workspace, target, and declared dependency
information and embedded rust-analyzer libraries for source semantics. It runs
on stable Rust and does not use `rustc_private`, nightly flags, or rustdoc JSON.

The importer resolves aliases, re-exports, qualified paths, active `cfg`
branches, declarative macros, workspace dependencies, renamed dependencies, and
external/toolchain crates where rust-analyzer provides a semantic target.

## Configuration-dependent graphs

Features, target triples, target kinds, build-script output, and proc-macro
availability change the imported graph. Architecture checks should use the same
configuration as the build they protect.

Build-script analysis can execute project build scripts and write normal Cargo
artifacts. Proc-macro analysis can execute project procedural macros through
rust-analyzer's proc-macro server. Disable either behavior in `bylaw.toml` when
that is inappropriate for the environment.

## Incomplete analysis

Enforcement fails closed by default. Any unresolved semantic path, unavailable
macro expansion, or requested target that cannot be loaded is emitted as an
error diagnostic. Use `--allow-incomplete` or `incomplete = "allow"` only when
warnings and possible false negatives are acceptable; every omission remains in
human and JSON reports.

The project pins its `ra_ap_*` dependencies exactly because rust-analyzer's
library APIs are not semver-stable. Upgrades must run the semantic fixture suite
before changing the pin.
