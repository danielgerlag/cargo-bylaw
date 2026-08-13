# Model boundaries example

This workspace keeps four representations separate:

- `shop-domain`: internal business entities and behavior.
- `shop-persistence`: database records and mappings to the domain.
- `shop-contract`: public request and response types.
- `shop-api`: maps contract types to and from domain operations.
- `shop-app`: the only composition root allowed to wire all adapters.

Run the same policy through the CLI:

```console
cargo run --manifest-path ../../crates/cargo-bylaw/Cargo.toml -- check
```

Or through a normal Rust architecture test:

```console
cargo test -p architecture-tests
```

The permitted model dependencies are:

```text
shop-persistence --> shop-domain
shop-api ---------> shop-domain
shop-api ---------> shop-contract
shop-app ---------> every model and adapter crate
```

The checked-in rules evaluate both source references and Cargo manifest
dependencies, so an unused forbidden dependency is still rejected.
