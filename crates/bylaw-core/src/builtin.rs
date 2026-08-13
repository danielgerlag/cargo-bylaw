//! Built-in architecture rules expressed through the public rule engine.

use crate::{
    ArchitectureGraph, Candidate, Component, ComponentId, ConditionEvent, DependencyEvidence,
    DependencyScope, DescribedCondition, DescribedSelector, PathPatternError, Rule, RuleMetadata,
    SelectorSpec,
};
use petgraph::algo::kosaraju_scc;
use petgraph::graph::DiGraph;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyScopes {
    Actual,
    Declared,
    #[default]
    Both,
}

impl DependencyScopes {
    pub fn includes(self, scope: DependencyScope) -> bool {
        matches!(
            (self, scope),
            (Self::Both, _)
                | (Self::Actual, DependencyScope::Actual)
                | (Self::Declared, DependencyScope::Declared)
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NamedSelectorSpec {
    pub name: String,
    pub selector: SelectorSpec,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LayerDependencySpec {
    pub from: String,
    pub may_depend_on: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CycleGrouping {
    Components,
    Modules,
    #[default]
    Crates,
    Slices {
        slices: Vec<NamedSelectorSpec>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BuiltInRuleSpec {
    ForbidDependencies {
        metadata: RuleMetadata,
        from: SelectorSpec,
        to: SelectorSpec,
        #[serde(default)]
        scopes: DependencyScopes,
    },
    OnlyDependencies {
        metadata: RuleMetadata,
        from: SelectorSpec,
        allowed: SelectorSpec,
        #[serde(default)]
        scopes: DependencyScopes,
        #[serde(default = "default_true")]
        allow_toolchain: bool,
        #[serde(default = "default_true")]
        allow_self: bool,
    },
    Layers {
        metadata: RuleMetadata,
        layers: Vec<NamedSelectorSpec>,
        dependencies: Vec<LayerDependencySpec>,
        #[serde(default)]
        scopes: DependencyScopes,
    },
    NoCycles {
        metadata: RuleMetadata,
        within: SelectorSpec,
        #[serde(default)]
        grouping: CycleGrouping,
        #[serde(default)]
        scopes: DependencyScopes,
    },
}

fn default_true() -> bool {
    true
}

impl BuiltInRuleSpec {
    pub fn metadata(&self) -> &RuleMetadata {
        match self {
            Self::ForbidDependencies { metadata, .. }
            | Self::OnlyDependencies { metadata, .. }
            | Self::Layers { metadata, .. }
            | Self::NoCycles { metadata, .. } => metadata,
        }
    }

    pub fn metadata_mut(&mut self) -> &mut RuleMetadata {
        match self {
            Self::ForbidDependencies { metadata, .. }
            | Self::OnlyDependencies { metadata, .. }
            | Self::Layers { metadata, .. }
            | Self::NoCycles { metadata, .. } => metadata,
        }
    }

    pub fn compile(&self) -> Result<Rule, RuleBuildError> {
        if self.metadata().id.trim().is_empty() {
            return Err(RuleBuildError::EmptyRuleId);
        }
        match self {
            Self::ForbidDependencies {
                metadata,
                from,
                to,
                scopes,
            } => {
                let origin_selector = from.compile()?;
                let target_selector = to.compile()?;
                Ok(Rule::new(
                    metadata.clone(),
                    origin_selector,
                    forbid_dependencies_condition(target_selector, *scopes),
                ))
            }
            Self::OnlyDependencies {
                metadata,
                from,
                allowed,
                scopes,
                allow_toolchain,
                allow_self,
            } => {
                let origin_selector = from.compile()?;
                let allowed_selector = allowed.compile()?;
                Ok(Rule::new(
                    metadata.clone(),
                    origin_selector,
                    only_dependencies_condition(
                        allowed_selector,
                        *scopes,
                        *allow_toolchain,
                        *allow_self,
                    ),
                ))
            }
            Self::Layers {
                metadata,
                layers,
                dependencies,
                scopes,
            } => {
                let layers = compile_named_selectors(layers)?;
                validate_layer_dependencies(&layers, dependencies)?;
                let dependencies = dependencies
                    .iter()
                    .map(|dependency| {
                        (
                            dependency.from.clone(),
                            dependency.may_depend_on.iter().cloned().collect(),
                        )
                    })
                    .collect::<BTreeMap<String, BTreeSet<String>>>();
                Ok(Rule::new(
                    metadata.clone(),
                    DescribedSelector::all(),
                    layers_condition(layers, dependencies, *scopes),
                ))
            }
            Self::NoCycles {
                metadata,
                within,
                grouping,
                scopes,
            } => {
                let within = within.compile()?;
                let grouping = compile_grouping(grouping)?;
                Ok(Rule::new(
                    metadata.clone(),
                    DescribedSelector::all(),
                    no_cycles_condition(within, grouping, *scopes),
                ))
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum RuleBuildError {
    #[error(transparent)]
    InvalidPattern(#[from] PathPatternError),
    #[error("rule IDs cannot be empty")]
    EmptyRuleId,
    #[error("selector name `{0}` is duplicated")]
    DuplicateSelector(String),
    #[error("selector names cannot be empty")]
    EmptySelectorName,
    #[error("layer dependency references unknown origin layer `{0}`")]
    UnknownOriginLayer(String),
    #[error("layer `{layer}` may depend on unknown layer `{target}`")]
    UnknownTargetLayer { layer: String, target: String },
}

fn compile_named_selectors(
    specs: &[NamedSelectorSpec],
) -> Result<Vec<(String, DescribedSelector)>, RuleBuildError> {
    let mut names = HashSet::new();
    let mut compiled = Vec::with_capacity(specs.len());
    for spec in specs {
        if spec.name.trim().is_empty() {
            return Err(RuleBuildError::EmptySelectorName);
        }
        if !names.insert(spec.name.clone()) {
            return Err(RuleBuildError::DuplicateSelector(spec.name.clone()));
        }
        compiled.push((spec.name.clone(), spec.selector.compile()?));
    }
    Ok(compiled)
}

fn validate_layer_dependencies(
    layers: &[(String, DescribedSelector)],
    dependencies: &[LayerDependencySpec],
) -> Result<(), RuleBuildError> {
    let names = layers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<HashSet<_>>();
    for dependency in dependencies {
        if !names.contains(dependency.from.as_str()) {
            return Err(RuleBuildError::UnknownOriginLayer(dependency.from.clone()));
        }
        for target in &dependency.may_depend_on {
            if !names.contains(target.as_str()) {
                return Err(RuleBuildError::UnknownTargetLayer {
                    layer: dependency.from.clone(),
                    target: target.clone(),
                });
            }
        }
    }
    Ok(())
}

fn forbid_dependencies_condition(
    target_selector: DescribedSelector,
    scopes: DependencyScopes,
) -> DescribedCondition {
    let description = format!(
        "not depend on {} through {scopes:?} dependencies",
        target_selector.description()
    );
    DescribedCondition::new(description, move |graph, selected| {
        let selected = selected.iter().collect::<HashSet<_>>();
        graph
            .edges()
            .filter(|edge| selected.contains(&edge.origin) && scopes.includes(edge.scope))
            .filter_map(|edge| {
                let target = graph.component(&edge.target)?;
                target_selector
                    .matches(Candidate::new(graph, target))
                    .then(|| {
                        let origin_name = graph.component(&edge.origin).map_or_else(
                            || edge.origin.to_string(),
                            |node| node.canonical_name().to_owned(),
                        );
                        ConditionEvent::new(format!(
                            "{origin_name} depends on {}",
                            target.canonical_name()
                        ))
                        .with_edge(
                            edge.origin.clone(),
                            edge.target.clone(),
                            edge.evidence.clone(),
                        )
                    })
            })
            .collect()
    })
}

fn only_dependencies_condition(
    allowed_selector: DescribedSelector,
    scopes: DependencyScopes,
    allow_toolchain: bool,
    allow_self: bool,
) -> DescribedCondition {
    let description = format!(
        "only depend on {} through {scopes:?} dependencies",
        allowed_selector.description()
    );
    DescribedCondition::new(description, move |graph, selected| {
        let selected = selected.iter().collect::<HashSet<_>>();
        graph
            .edges()
            .filter(|edge| selected.contains(&edge.origin) && scopes.includes(edge.scope))
            .filter_map(|edge| {
                let origin = graph.component(&edge.origin)?;
                let target = graph.component(&edge.target)?;
                if edge.origin == edge.target
                    || (allow_self && same_workspace_package(origin, target))
                {
                    return None;
                }
                if allow_toolchain && target.is_toolchain_crate() {
                    return None;
                }
                (!allowed_selector.matches(Candidate::new(graph, target))).then(|| {
                    let origin_name = graph.component(&edge.origin).map_or_else(
                        || edge.origin.to_string(),
                        |node| node.canonical_name().to_owned(),
                    );
                    ConditionEvent::new(format!(
                        "{origin_name} depends on disallowed component {}",
                        target.canonical_name()
                    ))
                    .with_edge(
                        edge.origin.clone(),
                        edge.target.clone(),
                        edge.evidence.clone(),
                    )
                })
            })
            .collect()
    })
}

fn same_workspace_package(origin: &Component, target: &Component) -> bool {
    matches!(origin, Component::Crate(_) | Component::Module(_))
        && matches!(target, Component::Crate(_) | Component::Module(_))
        && origin.package_name() == target.package_name()
}

fn layers_condition(
    layers: Vec<(String, DescribedSelector)>,
    dependencies: BTreeMap<String, BTreeSet<String>>,
    scopes: DependencyScopes,
) -> DescribedCondition {
    DescribedCondition::new("respect configured layer dependencies", move |graph, _| {
        let mut events = Vec::new();
        for edge in graph.edges().filter(|edge| scopes.includes(edge.scope)) {
            let (Some(origin), Some(target)) =
                (graph.component(&edge.origin), graph.component(&edge.target))
            else {
                continue;
            };
            let origin_layers = matching_names(graph, origin, &layers);
            let target_layers = matching_names(graph, target, &layers);

            for origin_layer in &origin_layers {
                for target_layer in &target_layers {
                    if origin_layer == target_layer {
                        continue;
                    }
                    let allowed = dependencies
                        .get(origin_layer)
                        .is_some_and(|targets| targets.contains(target_layer));
                    if !allowed {
                        events.push(
                            ConditionEvent::new(format!(
                                "layer `{origin_layer}` may not depend on layer `{target_layer}`: {} depends on {}",
                                origin.canonical_name(),
                                target.canonical_name()
                            ))
                            .with_edge(
                                edge.origin.clone(),
                                edge.target.clone(),
                                edge.evidence.clone(),
                            ),
                        );
                    }
                }
            }
        }
        events
    })
}

fn matching_names(
    graph: &ArchitectureGraph,
    component: &Component,
    selectors: &[(String, DescribedSelector)],
) -> Vec<String> {
    selectors
        .iter()
        .filter(|(_, selector)| selector.matches(Candidate::new(graph, component)))
        .map(|(name, _)| name.clone())
        .collect()
}

#[derive(Clone)]
enum CompiledGrouping {
    Components,
    Modules,
    Crates,
    Slices(Vec<(String, DescribedSelector)>),
}

fn compile_grouping(grouping: &CycleGrouping) -> Result<CompiledGrouping, RuleBuildError> {
    match grouping {
        CycleGrouping::Components => Ok(CompiledGrouping::Components),
        CycleGrouping::Modules => Ok(CompiledGrouping::Modules),
        CycleGrouping::Crates => Ok(CompiledGrouping::Crates),
        CycleGrouping::Slices { slices } => {
            Ok(CompiledGrouping::Slices(compile_named_selectors(slices)?))
        }
    }
}

fn no_cycles_condition(
    within: DescribedSelector,
    grouping: CompiledGrouping,
    scopes: DependencyScopes,
) -> DescribedCondition {
    DescribedCondition::new("be free of dependency cycles", move |graph, _| {
        let grouped = GroupedGraph::build(graph, &within, &grouping, scopes);
        grouped.cycle_events()
    })
}

#[derive(Default)]
struct GroupedGraph {
    labels: BTreeMap<String, String>,
    representatives: BTreeMap<String, ComponentId>,
    adjacency: BTreeMap<String, BTreeSet<String>>,
    evidence: BTreeMap<(String, String), Vec<DependencyEvidence>>,
}

impl GroupedGraph {
    fn build(
        graph: &ArchitectureGraph,
        within: &DescribedSelector,
        grouping: &CompiledGrouping,
        scopes: DependencyScopes,
    ) -> Self {
        let selected = graph
            .components()
            .filter(|component| within.matches(Candidate::new(graph, component)))
            .map(Component::id)
            .collect::<HashSet<_>>();
        let selected_crates = selected
            .iter()
            .filter_map(|id| graph.component(id)?.containing_crate())
            .collect::<HashSet<_>>();

        let mut grouped = Self::default();
        for edge in graph.edges().filter(|edge| scopes.includes(edge.scope)) {
            let origin_groups =
                groups_for(graph, &edge.origin, grouping, &selected, &selected_crates);
            let target_groups =
                groups_for(graph, &edge.target, grouping, &selected, &selected_crates);
            for origin in &origin_groups {
                for target in &target_groups {
                    if origin.key == target.key {
                        continue;
                    }
                    grouped
                        .labels
                        .entry(origin.key.clone())
                        .or_insert_with(|| origin.label.clone());
                    grouped
                        .labels
                        .entry(target.key.clone())
                        .or_insert_with(|| target.label.clone());
                    grouped
                        .representatives
                        .entry(origin.key.clone())
                        .or_insert_with(|| edge.origin.clone());
                    grouped
                        .representatives
                        .entry(target.key.clone())
                        .or_insert_with(|| edge.target.clone());
                    grouped
                        .adjacency
                        .entry(origin.key.clone())
                        .or_default()
                        .insert(target.key.clone());
                    let evidence = grouped
                        .evidence
                        .entry((origin.key.clone(), target.key.clone()))
                        .or_default();
                    for item in &edge.evidence {
                        if !evidence.contains(item) {
                            evidence.push(item.clone());
                        }
                    }
                }
            }
        }
        grouped
    }

    fn cycle_events(&self) -> Vec<ConditionEvent> {
        let mut graph = DiGraph::<String, ()>::new();
        let mut nodes = BTreeMap::new();
        for key in self.labels.keys() {
            nodes.insert(key.clone(), graph.add_node(key.clone()));
        }
        for (origin, targets) in &self.adjacency {
            for target in targets {
                if let (Some(origin), Some(target)) = (nodes.get(origin), nodes.get(target)) {
                    graph.add_edge(*origin, *target, ());
                }
            }
        }

        let mut strongly_connected = kosaraju_scc(&graph)
            .into_iter()
            .filter(|component| component.len() > 1)
            .map(|component| {
                let mut keys = component
                    .into_iter()
                    .filter_map(|node| graph.node_weight(node).cloned())
                    .collect::<Vec<_>>();
                keys.sort();
                keys
            })
            .collect::<Vec<_>>();
        strongly_connected.sort();

        strongly_connected
            .into_iter()
            .filter_map(|component| self.find_cycle(&component))
            .map(|cycle| {
                let labels = cycle
                    .iter()
                    .map(|key| self.labels.get(key).unwrap_or(key))
                    .cloned()
                    .collect::<Vec<_>>();
                let component_cycle = cycle
                    .iter()
                    .filter_map(|key| self.representatives.get(key).cloned())
                    .collect::<Vec<_>>();
                let evidence = cycle
                    .windows(2)
                    .flat_map(|pair| {
                        self.evidence
                            .get(&(pair[0].clone(), pair[1].clone()))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>();
                let mut event =
                    ConditionEvent::new(format!("dependency cycle: {}", labels.join(" -> ")))
                        .with_cycle(component_cycle);
                if let Some((origin, target)) = cycle.first().zip(cycle.get(1))
                    && let (Some(origin), Some(target)) = (
                        self.representatives.get(origin),
                        self.representatives.get(target),
                    )
                {
                    event = event.with_edge(origin.clone(), target.clone(), evidence);
                }
                event
            })
            .collect()
    }

    fn find_cycle(&self, component: &[String]) -> Option<Vec<String>> {
        let members = component.iter().cloned().collect::<BTreeSet<_>>();
        for start in component {
            let mut path = vec![start.clone()];
            let mut active = BTreeSet::from([start.clone()]);
            if self.find_cycle_from(start, start, &members, &mut active, &mut path) {
                return Some(path);
            }
        }
        None
    }

    fn find_cycle_from(
        &self,
        start: &str,
        current: &str,
        members: &BTreeSet<String>,
        active: &mut BTreeSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        let Some(neighbors) = self.adjacency.get(current) else {
            return false;
        };
        for neighbor in neighbors {
            if !members.contains(neighbor) {
                continue;
            }
            if neighbor == start {
                path.push(start.to_owned());
                return true;
            }
            if active.insert(neighbor.clone()) {
                path.push(neighbor.clone());
                if self.find_cycle_from(start, neighbor, members, active, path) {
                    return true;
                }
                path.pop();
                active.remove(neighbor);
            }
        }
        false
    }
}

#[derive(Clone)]
struct Group {
    key: String,
    label: String,
}

fn groups_for(
    graph: &ArchitectureGraph,
    id: &ComponentId,
    grouping: &CompiledGrouping,
    selected: &HashSet<ComponentId>,
    selected_crates: &HashSet<crate::CrateId>,
) -> Vec<Group> {
    let Some(component) = graph.component(id) else {
        return Vec::new();
    };
    match grouping {
        CompiledGrouping::Components => selected
            .contains(id)
            .then(|| Group {
                key: id.to_string(),
                label: component.canonical_name().to_owned(),
            })
            .into_iter()
            .collect(),
        CompiledGrouping::Modules => {
            if selected.contains(id) && matches!(component, Component::Module(_)) {
                vec![Group {
                    key: id.to_string(),
                    label: component.canonical_name().to_owned(),
                }]
            } else {
                Vec::new()
            }
        }
        CompiledGrouping::Crates => component
            .containing_crate()
            .filter(|crate_id| selected_crates.contains(crate_id))
            .map(|crate_id| {
                let component_id = ComponentId::Crate(crate_id.clone());
                let label = graph.component(&component_id).map_or_else(
                    || crate_id.to_string(),
                    |node| node.canonical_name().to_owned(),
                );
                Group {
                    key: crate_id.to_string(),
                    label,
                }
            })
            .into_iter()
            .collect(),
        CompiledGrouping::Slices(slices) => {
            if !selected.contains(id) {
                return Vec::new();
            }
            slices
                .iter()
                .filter(|(_, selector)| selector.matches(Candidate::new(graph, component)))
                .map(|(name, _)| Group {
                    key: name.clone(),
                    label: name.clone(),
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AnalysisContext, ArchitectureRule, Component, CrateId, CrateNode, DependencyEvidence,
        DependencyKind, GraphBuilder, ModuleId, ModuleNode, Package, PackageId, TargetKind,
    };
    use camino::Utf8PathBuf;

    fn graph_with_module_edges(edges: &[(&str, &str)]) -> ArchitectureGraph {
        graph_with_scoped_module_edges(
            edges
                .iter()
                .map(|(origin, target)| (*origin, *target, DependencyScope::Actual))
                .collect::<Vec<_>>()
                .as_slice(),
        )
    }

    fn graph_with_scoped_module_edges(
        edges: &[(&str, &str, DependencyScope)],
    ) -> ArchitectureGraph {
        let package_id = PackageId::new("shop 0.1.0");
        let crate_id = CrateId::new("shop#lib");
        let mut builder = GraphBuilder::new(AnalysisContext::default());
        builder
            .add_package(Package {
                id: package_id.clone(),
                name: "shop".to_owned(),
                version: Some("0.1.0".to_owned()),
                manifest_path: Utf8PathBuf::from("Cargo.toml"),
            })
            .unwrap();
        builder
            .add_component(Component::Crate(CrateNode {
                id: crate_id.clone(),
                package_id: package_id.clone(),
                package_name: "shop".to_owned(),
                crate_name: "shop".to_owned(),
                target_name: "shop".to_owned(),
                target_kind: TargetKind::Library,
                source_root: Utf8PathBuf::from("src/lib.rs"),
            }))
            .unwrap();

        for name in ["domain", "persistence", "api"] {
            builder
                .add_component(Component::Module(ModuleNode {
                    id: ModuleId::new(format!("shop#lib::{name}")),
                    crate_id: crate_id.clone(),
                    package_id: package_id.clone(),
                    package_name: "shop".to_owned(),
                    crate_name: "shop".to_owned(),
                    path: format!("shop::{name}"),
                    parent: None,
                    source_file: Utf8PathBuf::from(format!("src/{name}.rs")),
                }))
                .unwrap();
        }
        for (origin, target, scope) in edges {
            builder
                .add_dependency(
                    ComponentId::Module(ModuleId::new(format!("shop#lib::{origin}"))),
                    ComponentId::Module(ModuleId::new(format!("shop#lib::{target}"))),
                    *scope,
                    DependencyEvidence::new(DependencyKind::Use),
                )
                .unwrap();
        }
        builder.finish()
    }

    #[test]
    fn forbidden_dependency_reports_edge() {
        let graph = graph_with_module_edges(&[("domain", "persistence")]);
        let rule = BuiltInRuleSpec::ForbidDependencies {
            metadata: RuleMetadata::new("domain-is-internal"),
            from: SelectorSpec::Modules {
                patterns: vec!["shop::domain".to_owned()],
            },
            to: SelectorSpec::Modules {
                patterns: vec!["shop::persistence".to_owned()],
            },
            scopes: DependencyScopes::Actual,
        }
        .compile()
        .unwrap();

        let result = rule.evaluate(&graph);
        assert_eq!(result.violations.len(), 1);
        assert!(result.violations[0].message.contains("persistence"));
    }

    #[test]
    fn dependency_scope_is_respected() {
        let graph =
            graph_with_scoped_module_edges(&[("domain", "persistence", DependencyScope::Declared)]);
        let rule = BuiltInRuleSpec::ForbidDependencies {
            metadata: RuleMetadata::new("actual-only"),
            from: SelectorSpec::Modules {
                patterns: vec!["shop::domain".to_owned()],
            },
            to: SelectorSpec::Modules {
                patterns: vec!["shop::persistence".to_owned()],
            },
            scopes: DependencyScopes::Actual,
        }
        .compile()
        .unwrap();

        assert!(rule.evaluate(&graph).violations.is_empty());
    }

    #[test]
    fn only_dependencies_reports_disallowed_target() {
        let graph = graph_with_module_edges(&[("domain", "persistence")]);
        let rule = BuiltInRuleSpec::OnlyDependencies {
            metadata: RuleMetadata::new("domain-dependencies"),
            from: SelectorSpec::Modules {
                patterns: vec!["shop::domain".to_owned()],
            },
            allowed: SelectorSpec::Modules {
                patterns: vec!["shop::domain::**".to_owned()],
            },
            scopes: DependencyScopes::Actual,
            allow_toolchain: true,
            allow_self: false,
        }
        .compile()
        .unwrap();

        assert_eq!(rule.evaluate(&graph).violations.len(), 1);
    }

    #[test]
    fn only_dependencies_can_allow_same_package_edges() {
        let graph = graph_with_module_edges(&[("domain", "persistence")]);
        let rule = BuiltInRuleSpec::OnlyDependencies {
            metadata: RuleMetadata::new("domain-dependencies"),
            from: SelectorSpec::Modules {
                patterns: vec!["shop::domain".to_owned()],
            },
            allowed: SelectorSpec::Modules {
                patterns: Vec::new(),
            },
            scopes: DependencyScopes::Actual,
            allow_toolchain: true,
            allow_self: true,
        }
        .compile()
        .unwrap();

        assert!(rule.evaluate(&graph).violations.is_empty());
    }

    #[test]
    fn layered_rule_enforces_direction() {
        let graph = graph_with_module_edges(&[("domain", "persistence")]);
        let rule = BuiltInRuleSpec::Layers {
            metadata: RuleMetadata::new("layers"),
            layers: vec![
                NamedSelectorSpec {
                    name: "domain".to_owned(),
                    selector: SelectorSpec::Modules {
                        patterns: vec!["shop::domain".to_owned()],
                    },
                },
                NamedSelectorSpec {
                    name: "persistence".to_owned(),
                    selector: SelectorSpec::Modules {
                        patterns: vec!["shop::persistence".to_owned()],
                    },
                },
            ],
            dependencies: vec![LayerDependencySpec {
                from: "persistence".to_owned(),
                may_depend_on: vec!["domain".to_owned()],
            }],
            scopes: DependencyScopes::Actual,
        }
        .compile()
        .unwrap();

        let result = rule.evaluate(&graph);
        assert_eq!(result.violations.len(), 1);
        assert!(result.violations[0].message.contains("domain"));
    }

    #[test]
    fn cycle_rule_reports_deterministic_path() {
        let graph = graph_with_module_edges(&[
            ("domain", "persistence"),
            ("persistence", "api"),
            ("api", "domain"),
        ]);
        let rule = BuiltInRuleSpec::NoCycles {
            metadata: RuleMetadata::new("modules-are-acyclic"),
            within: SelectorSpec::Modules {
                patterns: vec!["shop::**".to_owned()],
            },
            grouping: CycleGrouping::Modules,
            scopes: DependencyScopes::Actual,
        }
        .compile()
        .unwrap();

        let result = rule.evaluate(&graph);
        assert_eq!(result.violations.len(), 1);
        assert_eq!(result.violations[0].cycle.len(), 4);
    }

    #[test]
    fn named_slices_can_be_checked_for_cycles() {
        let graph = graph_with_module_edges(&[
            ("domain", "persistence"),
            ("persistence", "api"),
            ("api", "domain"),
        ]);
        let slices = ["domain", "persistence", "api"]
            .into_iter()
            .map(|name| NamedSelectorSpec {
                name: name.to_owned(),
                selector: SelectorSpec::Modules {
                    patterns: vec![format!("shop::{name}")],
                },
            })
            .collect();
        let rule = BuiltInRuleSpec::NoCycles {
            metadata: RuleMetadata::new("slices-are-acyclic"),
            within: SelectorSpec::Modules {
                patterns: vec!["shop::**".to_owned()],
            },
            grouping: CycleGrouping::Slices { slices },
            scopes: DependencyScopes::Actual,
        }
        .compile()
        .unwrap();

        assert_eq!(rule.evaluate(&graph).violations.len(), 1);
    }
}
