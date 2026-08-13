# Model boundaries example

The repository includes a runnable application with separate domain,
persistence, public contract, API, and composition-root crates:

```text
shop-persistence --> shop-domain
shop-api ---------> shop-domain
shop-api ---------> shop-contract
shop-app ---------> all model and adapter crates
```

From the repository root, run the Rust architecture test:

```console
cargo test \
  --manifest-path examples/model-boundaries/Cargo.toml \
  -p architecture-tests
```

Run the equivalent TOML rules through the CLI:

```console
cargo run -p cargo-bylaw -- check \
  --config examples/model-boundaries/bylaw.toml
```

The expected result is:

```text
architecture checks passed (6 rules)
```

The test suite also contains real passing and intentionally failing Rust
architecture tests:

```console
cargo test -p bylaw --test architecture_test_e2e
```

The parent harness verifies that the passing test succeeds and that the failing
test exits non-zero with the expected boundary and cycle diagnostics.
