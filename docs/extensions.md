# Custom rules

Custom Rust rules use the analyzer-independent `bylaw-core` API. No
rust-analyzer type is exposed.

Most rules use three concepts:

- `DescribedSelector`: chooses graph components and composes with `and`, `or`,
  and `not`.
- `DescribedCondition`: evaluates selected component IDs and emits
  `ConditionEvent` values.
- `Rule`: combines metadata, a selector, and a condition.

```rust
use bylaw::core::{
    Candidate, ConditionEvent, DescribedCondition, DescribedSelector, Rule,
    RuleMetadata,
};

let selector = DescribedSelector::new("workspace modules", |candidate: Candidate<'_>| {
    matches!(candidate.component(), bylaw::core::Component::Module(_))
});

let condition = DescribedCondition::new(
    "have at least one outgoing dependency",
    |graph, selected| {
        selected
            .iter()
            .filter(|id| graph.outgoing(id).next().is_none())
            .map(|id| ConditionEvent::new(format!("{id} has no outgoing dependencies")))
            .collect()
    },
);

let rule = Rule::new(RuleMetadata::new("modules-have-dependencies"), selector, condition);
let report = bylaw::rules().with_custom(rule).check(&graph)?.into_report();
# Ok::<(), bylaw::Error>(())
```

Implement `Selector`, `Condition`, or `ArchitectureRule` directly when a closure
is not sufficient. Public extension traits are `Send + Sync`, allowing rule sets
to remain shareable. Arbitrary custom Rust rules run in a test or purpose-built
binary; the stock TOML CLI supports serializable built-in rules only.
