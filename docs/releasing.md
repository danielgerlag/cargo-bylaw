# Releasing

Cargo Bylaw publishes its guide to GitHub Pages and its five workspace packages
to crates.io.

## GitHub Pages

The `Docs` workflow builds this mdBook and deploys the generated `book/`
directory with GitHub's official Pages actions.

In the GitHub repository:

1. Open **Settings → Pages**.
2. Set **Source** to **GitHub Actions**.
3. Push to `main` or run the `Docs` workflow manually.

Build the site locally with:

```console
cargo install mdbook --version 0.5.4 --locked
mdbook serve --open
```

## crates.io

Create a crates.io API token and save it as the GitHub Actions repository secret
`CARGO_REGISTRY_TOKEN`.

All workspace crates use the same version. To publish `0.1.0`:

```console
git tag v0.1.0
git push origin v0.1.0
```

The `Publish crates` workflow verifies that the tag matches every workspace
package and publishes in dependency order:

```text
bylaw-core
bylaw-analyzer
bylaw-config
bylaw
cargo-bylaw
```

The script waits for each version to reach the crates.io index before publishing
its dependents. It also skips versions that already exist, so a partially
completed release can be rerun safely.

Before the first release, validate the core package plus every dependent
package's manifest and file set without publishing:

```console
PUBLISH_DRY_RUN=1 bash scripts/publish-crates.sh v0.1.0
```
