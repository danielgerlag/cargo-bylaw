# Keeping coding agents aligned

Coding agents are effective at making focused changes, but architecture is a
workspace-wide concern. An agent can solve the immediate task while
accidentally:

- Importing persistence types into the domain model.
- Exposing internal entities through a public API contract.
- Bypassing an application layer to call an adapter directly.
- Adding a convenient dependency that creates a cycle.

Architecture prose helps an agent understand intent, but prose alone is
advisory. Cargo Bylaw turns that intent into executable feedback that applies to
every change, regardless of whether it was written by a person or an agent.

## The agent feedback loop

Keep `bylaw.toml` or Rust architecture tests in the repository:

1. The agent reads the same architecture rules as the team.
2. It makes the requested code change.
3. It runs `cargo bylaw check`.
4. Cargo Bylaw reports the violated rule, dependency direction, and source
   location.
5. The agent revises the change until the architecture check passes.

This gives the agent an objective completion condition instead of relying on it
to remember every boundary across a large codebase.

## Add the requirement to agent instructions

Add guidance like this to `AGENTS.md`, `.github/copilot-instructions.md`, or the
instruction file used by the coding-agent platform:

```markdown
## Architecture

- Treat `bylaw.toml` and the architecture tests as the source of truth for
  dependency boundaries.
- Before completing any code change, run `cargo bylaw check`.
- Fix architecture violations rather than weakening, excluding, or deleting
  rules.
- Do not use `--allow-incomplete` unless the task explicitly requires and
  documents the exception.
```

If the project uses the Rust API instead of `bylaw.toml`, name the architecture
test command explicitly:

```markdown
Before completing a change, run:

cargo test -p architecture-tests
```

## Enforce the same rule in CI

Local agent instructions provide fast feedback. CI makes the boundary
non-optional:

```yaml
- name: Check architecture
  run: cargo bylaw check
```

The default fail-closed behavior is important for automated changes. An
unresolved path or unavailable macro expansion is an analysis failure rather
than a success-shaped result that could let an agent introduce an unseen
dependency.

## Keep rules architectural

Rules are most useful to agents when they describe durable system boundaries,
not temporary implementation details. Prefer rules such as:

- The domain cannot depend on persistence or transport models.
- API handlers cannot access database adapters directly.
- Feature slices must remain acyclic.
- Only the composition root may depend on every adapter.

The [model boundaries example](examples.md) demonstrates these constraints in
both `bylaw.toml` and a normal Rust architecture test.
