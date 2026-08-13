//! Analyzer-independent architecture graph and rule engine.

mod diagnostic;
mod graph;
mod rule;
mod selector;

pub mod builtin;

pub use builtin::{
    BuiltInRuleSpec, CycleGrouping, DependencyScopes, LayerDependencySpec, NamedSelectorSpec,
    RuleBuildError,
};
pub use diagnostic::{
    AnalysisDiagnostic, ConditionEvent, EvaluationReport, RuleResult, Severity, Violation,
};
pub use graph::{
    AnalysisContext, ArchitectureGraph, Component, ComponentId, ComponentKind, CrateId, CrateNode,
    DependencyEdge, DependencyEvidence, DependencyKind, DependencyScope, ExternalCrateId,
    ExternalCrateNode, GraphBuildError, GraphBuilder, ModuleId, ModuleNode, Package, PackageId,
    SourcePosition, SourceSpan, TargetKind,
};
pub use rule::{ArchitectureRule, Condition, DescribedCondition, Rule, RuleMetadata, RuleSet};
pub use selector::{
    Candidate, DescribedSelector, PathPattern, PathPatternError, Selector, SelectorSpec,
};
