//! Public test API and fluent architecture rule DSL.

use bylaw_core::{
    ArchitectureGraph, ArchitectureRule, BuiltInRuleSpec, ComponentKind, CycleGrouping,
    DependencyScopes, EvaluationReport, LayerDependencySpec, NamedSelectorSpec, RuleBuildError,
    RuleMetadata, RuleSet, SelectorSpec, Severity, TargetKind,
};
use std::collections::HashSet;
use std::fmt::Write;
use std::sync::Arc;
use thiserror::Error;

pub use bylaw_analyzer as analyzer;
pub use bylaw_config as config;
pub use bylaw_core as core;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Elements {
    spec: SelectorSpec,
}

impl Elements {
    pub fn from_spec(spec: SelectorSpec) -> Self {
        Self { spec }
    }

    pub fn into_spec(self) -> SelectorSpec {
        self.spec
    }

    pub fn or(self, other: Self) -> Self {
        Self::from_spec(SelectorSpec::AnyOf {
            selectors: vec![self.spec, other.spec],
        })
    }

    pub fn and(self, other: Self) -> Self {
        Self::from_spec(SelectorSpec::AllOf {
            selectors: vec![self.spec, other.spec],
        })
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        Self::from_spec(SelectorSpec::Not {
            selector: Box::new(self.spec),
        })
    }
}

pub fn all() -> Elements {
    Elements::from_spec(SelectorSpec::All)
}

pub fn packages<I, S>(names: I) -> Elements
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Elements::from_spec(SelectorSpec::Packages {
        names: collect_strings(names),
    })
}

pub fn crates<I, S>(names: I) -> Elements
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Elements::from_spec(SelectorSpec::Crates {
        names: collect_strings(names),
    })
}

pub fn modules<I, S>(patterns: I) -> Elements
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Elements::from_spec(SelectorSpec::Modules {
        patterns: collect_strings(patterns),
    })
}

pub fn external_crates<I, S>(names: I) -> Elements
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Elements::from_spec(SelectorSpec::ExternalCrates {
        names: collect_strings(names),
    })
}

pub fn target_kinds<I>(kinds: I) -> Elements
where
    I: IntoIterator<Item = TargetKind>,
{
    Elements::from_spec(SelectorSpec::TargetKinds {
        kinds: kinds.into_iter().collect(),
    })
}

pub fn component_kinds<I>(kinds: I) -> Elements
where
    I: IntoIterator<Item = ComponentKind>,
{
    Elements::from_spec(SelectorSpec::ComponentKinds {
        kinds: kinds.into_iter().collect(),
    })
}

fn collect_strings<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    values.into_iter().map(Into::into).collect()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LayeredArchitecture {
    layers: Vec<NamedSelectorSpec>,
    dependencies: Vec<LayerDependencySpec>,
}

impl LayeredArchitecture {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layer(mut self, name: impl Into<String>, selector: Elements) -> Self {
        self.layers.push(NamedSelectorSpec {
            name: name.into(),
            selector: selector.into_spec(),
        });
        self
    }

    pub fn may_depend_on<I, S>(mut self, from: impl Into<String>, targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.dependencies.push(LayerDependencySpec {
            from: from.into(),
            may_depend_on: collect_strings(targets),
        });
        self
    }
}

pub fn slice_grouping<I, N>(slices: I) -> CycleGrouping
where
    I: IntoIterator<Item = (N, Elements)>,
    N: Into<String>,
{
    CycleGrouping::Slices {
        slices: slices
            .into_iter()
            .map(|(name, selector)| NamedSelectorSpec {
                name: name.into(),
                selector: selector.into_spec(),
            })
            .collect(),
    }
}

#[derive(Default)]
pub struct Rules {
    specs: Vec<BuiltInRuleSpec>,
    custom: Vec<Arc<dyn ArchitectureRule>>,
    last_builtin: Option<usize>,
}

impl Rules {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_specs(specs: Vec<BuiltInRuleSpec>) -> Self {
        Self {
            last_builtin: specs.len().checked_sub(1),
            specs,
            custom: Vec::new(),
        }
    }

    pub fn forbid_dependencies(
        mut self,
        id: impl Into<String>,
        from: Elements,
        to: Elements,
    ) -> Self {
        self.specs.push(BuiltInRuleSpec::ForbidDependencies {
            metadata: RuleMetadata::new(id),
            from: from.into_spec(),
            to: to.into_spec(),
            scopes: DependencyScopes::Both,
        });
        self.last_builtin = self.specs.len().checked_sub(1);
        self
    }

    pub fn only_dependencies(
        mut self,
        id: impl Into<String>,
        from: Elements,
        allowed: Elements,
    ) -> Self {
        self.specs.push(BuiltInRuleSpec::OnlyDependencies {
            metadata: RuleMetadata::new(id),
            from: from.into_spec(),
            allowed: allowed.into_spec(),
            scopes: DependencyScopes::Both,
            allow_toolchain: true,
            allow_self: true,
        });
        self.last_builtin = self.specs.len().checked_sub(1);
        self
    }

    pub fn layered(mut self, id: impl Into<String>, architecture: LayeredArchitecture) -> Self {
        self.specs.push(BuiltInRuleSpec::Layers {
            metadata: RuleMetadata::new(id),
            layers: architecture.layers,
            dependencies: architecture.dependencies,
            scopes: DependencyScopes::Both,
        });
        self.last_builtin = self.specs.len().checked_sub(1);
        self
    }

    pub fn no_cycles(
        mut self,
        id: impl Into<String>,
        within: Elements,
        grouping: CycleGrouping,
    ) -> Self {
        self.specs.push(BuiltInRuleSpec::NoCycles {
            metadata: RuleMetadata::new(id),
            within: within.into_spec(),
            grouping,
            scopes: DependencyScopes::Both,
        });
        self.last_builtin = self.specs.len().checked_sub(1);
        self
    }

    pub fn with_custom<R>(mut self, rule: R) -> Self
    where
        R: ArchitectureRule + 'static,
    {
        self.custom.push(Arc::new(rule));
        self.last_builtin = None;
        self
    }

    pub fn because(mut self, rationale: impl Into<String>) -> Self {
        self.last_spec_mut("because").metadata_mut().because = Some(rationale.into());
        self
    }

    pub fn described_as(mut self, description: impl Into<String>) -> Self {
        self.last_spec_mut("described_as")
            .metadata_mut()
            .description = Some(description.into());
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.last_spec_mut("with_severity").metadata_mut().severity = severity;
        self
    }

    pub fn actual_dependencies(mut self) -> Self {
        set_scopes(
            self.last_spec_mut("actual_dependencies"),
            DependencyScopes::Actual,
        );
        self
    }

    pub fn declared_dependencies(mut self) -> Self {
        set_scopes(
            self.last_spec_mut("declared_dependencies"),
            DependencyScopes::Declared,
        );
        self
    }

    pub fn check_self_dependencies(mut self) -> Self {
        match self.last_spec_mut("check_self_dependencies") {
            BuiltInRuleSpec::OnlyDependencies { allow_self, .. } => *allow_self = false,
            _ => panic!("`check_self_dependencies` only applies to an allow-list rule"),
        }
        self
    }

    pub fn check_toolchain_dependencies(mut self) -> Self {
        match self.last_spec_mut("check_toolchain_dependencies") {
            BuiltInRuleSpec::OnlyDependencies {
                allow_toolchain, ..
            } => *allow_toolchain = false,
            _ => panic!("`check_toolchain_dependencies` only applies to an allow-list rule"),
        }
        self
    }

    pub fn compile(&self) -> Result<RuleSet, Error> {
        let mut rules = RuleSet::new();
        let mut ids = HashSet::new();
        for spec in &self.specs {
            let id = spec.metadata().id.clone();
            if id.trim().is_empty() {
                return Err(Error::EmptyRuleId);
            }
            if !ids.insert(id.clone()) {
                return Err(Error::DuplicateRuleId(id));
            }
            rules.push(spec.compile()?);
        }
        for rule in &self.custom {
            let id = rule.metadata().id.clone();
            if id.trim().is_empty() {
                return Err(Error::EmptyRuleId);
            }
            if !ids.insert(id.clone()) {
                return Err(Error::DuplicateRuleId(id));
            }
            rules.push_shared(Arc::clone(rule));
        }
        Ok(rules)
    }

    pub fn check(&self, graph: &ArchitectureGraph) -> Result<Check, Error> {
        Ok(Check {
            report: self.compile()?.evaluate(graph),
        })
    }

    pub fn specs(&self) -> &[BuiltInRuleSpec] {
        &self.specs
    }

    fn last_spec_mut(&mut self, method: &str) -> &mut BuiltInRuleSpec {
        let index = self.last_builtin.unwrap_or_else(|| {
            panic!("`{method}` must immediately follow a built-in rule definition")
        });
        &mut self.specs[index]
    }
}

pub fn rules() -> Rules {
    Rules::new()
}

fn set_scopes(spec: &mut BuiltInRuleSpec, scopes: DependencyScopes) {
    match spec {
        BuiltInRuleSpec::ForbidDependencies {
            scopes: current, ..
        }
        | BuiltInRuleSpec::OnlyDependencies {
            scopes: current, ..
        }
        | BuiltInRuleSpec::Layers {
            scopes: current, ..
        }
        | BuiltInRuleSpec::NoCycles {
            scopes: current, ..
        } => *current = scopes,
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    report: EvaluationReport,
}

impl Check {
    pub fn report(&self) -> &EvaluationReport {
        &self.report
    }

    pub fn into_report(self) -> EvaluationReport {
        self.report
    }

    #[track_caller]
    pub fn assert(self) {
        assert!(
            self.report.is_success(),
            "{}",
            render_human_report(&self.report)
        );
    }
}

pub fn render_human_report(report: &EvaluationReport) -> String {
    if report.is_success()
        && report.analysis_diagnostics.is_empty()
        && report.violations().next().is_none()
    {
        return format!(
            "architecture checks passed ({} rules)",
            report.rule_results.len()
        );
    }

    let mut output = String::new();
    for diagnostic in &report.analysis_diagnostics {
        let _ = writeln!(
            output,
            "{:?}[{}]: {}",
            diagnostic.severity, diagnostic.code, diagnostic.message
        );
        if let Some(span) = &diagnostic.span {
            let _ = writeln!(
                output,
                "  at {}:{}:{}",
                span.path, span.start.line, span.start.column
            );
        }
        if let Some(help) = &diagnostic.help {
            let _ = writeln!(output, "  help: {help}");
        }
    }

    for result in &report.rule_results {
        if result.violations.is_empty() {
            continue;
        }
        let _ = writeln!(
            output,
            "{:?}[{}]: {}",
            result.severity, result.rule_id, result.description
        );
        if let Some(because) = &result.because {
            let _ = writeln!(output, "  because: {because}");
        }
        for violation in &result.violations {
            let _ = writeln!(output, "  - {}", violation.message);
            for evidence in &violation.evidence {
                if let Some(span) = &evidence.span {
                    let _ = writeln!(
                        output,
                        "    at {}:{}:{} ({:?})",
                        span.path, span.start.line, span.start.column, evidence.kind
                    );
                }
            }
            if let Some(help) = &violation.help {
                let _ = writeln!(output, "    help: {help}");
            }
        }
    }
    output.trim_end().to_owned()
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    InvalidRule(#[from] RuleBuildError),
    #[error("rule ID `{0}` is duplicated")]
    DuplicateRuleId(String),
    #[error("rule IDs cannot be empty")]
    EmptyRuleId,
}

pub mod prelude {
    pub use crate::{
        Elements, LayeredArchitecture, Rules, all, component_kinds, crates, external_crates,
        modules, packages, rules, slice_grouping, target_kinds,
    };
    pub use bylaw_core::{
        ArchitectureGraph, ArchitectureRule, Candidate, Component, Condition, CycleGrouping,
        DescribedCondition, DescribedSelector, Rule, RuleMetadata, Severity,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use bylaw_core::{
        AnalysisContext, Component, ComponentId, ConditionEvent, CrateId, CrateNode,
        DependencyEvidence, DependencyKind, DependencyScope, DescribedCondition, DescribedSelector,
        GraphBuilder, ModuleId, ModuleNode, Package, PackageId, Rule, SourcePosition, SourceSpan,
        TargetKind,
    };
    use camino::Utf8PathBuf;

    fn graph() -> ArchitectureGraph {
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
        for name in ["domain", "persistence"] {
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
        builder
            .add_dependency(
                ComponentId::Module(ModuleId::new("shop#lib::domain")),
                ComponentId::Module(ModuleId::new("shop#lib::persistence")),
                DependencyScope::Actual,
                DependencyEvidence::new(DependencyKind::Use).with_span(SourceSpan {
                    path: Utf8PathBuf::from("src/domain.rs"),
                    start: SourcePosition { line: 4, column: 5 },
                    end: SourcePosition {
                        line: 4,
                        column: 28,
                    },
                }),
            )
            .unwrap();
        builder.finish()
    }

    #[test]
    fn fluent_rules_evaluate_shared_core_specs() {
        let check = rules()
            .forbid_dependencies(
                "domain-is-internal",
                modules(["shop::domain"]),
                modules(["shop::persistence"]),
            )
            .because("domain is internal")
            .actual_dependencies()
            .check(&graph())
            .unwrap();

        assert!(!check.report().is_success());
        let output = render_human_report(check.report());
        assert!(output.contains("Error[domain-is-internal]"));
        assert!(output.contains("at src/domain.rs:4:5 (Use)"));
    }

    #[test]
    fn custom_conditions_use_only_public_graph_types() {
        let selector = DescribedSelector::new("modules", |candidate| {
            matches!(candidate.component(), Component::Module(_))
        });
        let condition = DescribedCondition::new("have no dependencies", |graph, selected| {
            selected
                .iter()
                .flat_map(|origin| {
                    graph.outgoing(origin).map(|edge| {
                        ConditionEvent::new("module has an outgoing dependency").with_edge(
                            edge.origin.clone(),
                            edge.target.clone(),
                            edge.evidence.clone(),
                        )
                    })
                })
                .collect()
        });
        let custom = Rule::new(RuleMetadata::new("custom-module-rule"), selector, condition);

        let report = rules()
            .with_custom(custom)
            .check(&graph())
            .unwrap()
            .into_report();
        assert_eq!(report.violations().count(), 1);
    }
}
