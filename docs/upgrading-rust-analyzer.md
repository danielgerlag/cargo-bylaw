# Upgrading rust-analyzer

Cargo Bylaw embeds rust-analyzer through exactly pinned `ra_ap_*` crates. Those
library APIs do not provide normal semver compatibility, so update every
`ra_ap_*` crate in one change.

1. Change all pins to the same release.
2. Keep all adaptation inside `bylaw-analyzer`; no `ra_ap_*` type may appear in
   another crate's public or private API.
3. Compile the focused analyzer fixtures first.
4. Run the module-boundary end-to-end tests, the model-boundaries example, and
   the root self-enforcement check.
5. Compare human and JSON diagnostics for source-span or canonical-name changes.
6. Confirm the new crates' MSRV before changing the workspace `rust-version`.

Required validation:

```console
cargo test -p bylaw-analyzer
cargo test -p bylaw --test model_boundaries
cargo test -p bylaw --test architecture_test_e2e
cargo test -p cargo-bylaw --test cli
cargo test --manifest-path examples/model-boundaries/Cargo.toml
cargo run -p cargo-bylaw -- check
```

Do not replace a failed semantic resolution with a silent syntax-only fallback.
Add a completeness diagnostic or reject the workspace instead.
