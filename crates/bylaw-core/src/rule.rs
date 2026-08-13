use crate::{
    ArchitectureGraph, ComponentId, ConditionEvent, DescribedSelector, EvaluationReport,
    RuleResult, Severity, Violation,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

pub trait Condition: Send + Sync {
    fn description(&self) -> &str;
    fn evaluate(&self, graph: &ArchitectureGraph, selected: &[ComponentId]) -> Vec<ConditionEvent>;
}

#[derive(Clone)]
pub struct DescribedCondition {
    inner: Arc<dyn Condition>,
}

impl fmt::Debug for DescribedCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DescribedCondition")
            .field("description", &self.description())
            .finish_non_exhaustive()
    }
}

impl DescribedCondition {
    pub fn new<F>(description: impl Into<String>, evaluator: F) -> Self
    where
        F: Fn(&ArchitectureGraph, &[ComponentId]) -> Vec<ConditionEvent> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(FunctionCondition {
                description: description.into(),
                evaluator,
            }),
        }
    }

    pub fn description(&self) -> &str {
        self.inner.description()
    }

    pub fn evaluate(
        &self,
        graph: &ArchitectureGraph,
        selected: &[ComponentId],
    ) -> Vec<ConditionEvent> {
        self.inner.evaluate(graph, selected)
    }
}

impl Condition for DescribedCondition {
    fn description(&self) -> &str {
        self.description()
    }

    fn evaluate(&self, graph: &ArchitectureGraph, selected: &[ComponentId]) -> Vec<ConditionEvent> {
        self.evaluate(graph, selected)
    }
}

struct FunctionCondition<F> {
    description: String,
    evaluator: F,
}

impl<F> Condition for FunctionCondition<F>
where
    F: Fn(&ArchitectureGraph, &[ComponentId]) -> Vec<ConditionEvent> + Send + Sync,
{
    fn description(&self) -> &str {
        &self.description
    }

    fn evaluate(&self, graph: &ArchitectureGraph, selected: &[ComponentId]) -> Vec<ConditionEvent> {
        (self.evaluator)(graph, selected)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuleMetadata {
    pub id: String,
    pub description: Option<String>,
    pub because: Option<String>,
    pub severity: Severity,
}

impl RuleMetadata {
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        assert!(!id.trim().is_empty(), "rule IDs cannot be empty");
        Self {
            id,
            description: None,
            because: None,
            severity: Severity::Error,
        }
    }

    pub fn described_as(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn because(mut self, rationale: impl Into<String>) -> Self {
        self.because = Some(rationale.into());
        self
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }
}

pub trait ArchitectureRule: Send + Sync {
    fn metadata(&self) -> &RuleMetadata;
    fn evaluate(&self, graph: &ArchitectureGraph) -> RuleResult;
}

#[derive(Clone, Debug)]
pub struct Rule {
    metadata: RuleMetadata,
    selector: DescribedSelector,
    condition: DescribedCondition,
}

impl Rule {
    pub fn new(
        metadata: RuleMetadata,
        selector: DescribedSelector,
        condition: DescribedCondition,
    ) -> Self {
        Self {
            metadata,
            selector,
            condition,
        }
    }
}

impl ArchitectureRule for Rule {
    fn metadata(&self) -> &RuleMetadata {
        &self.metadata
    }

    fn evaluate(&self, graph: &ArchitectureGraph) -> RuleResult {
        let selected = graph
            .components()
            .filter(|component| {
                self.selector
                    .matches(crate::Candidate::new(graph, component))
            })
            .map(crate::Component::id)
            .collect::<Vec<_>>();

        let description = self.metadata.description.clone().unwrap_or_else(|| {
            format!(
                "{} should {}",
                self.selector.description(),
                self.condition.description()
            )
        });
        let mut violations = self
            .condition
            .evaluate(graph, &selected)
            .into_iter()
            .map(|event| Violation {
                rule_id: self.metadata.id.clone(),
                severity: self.metadata.severity,
                message: event.message,
                origin: event.origin,
                target: event.target,
                evidence: event.evidence,
                cycle: event.cycle,
                help: event.help,
            })
            .collect::<Vec<_>>();
        violations.sort_by(|left, right| {
            (&left.message, &left.origin, &left.target).cmp(&(
                &right.message,
                &right.origin,
                &right.target,
            ))
        });

        RuleResult {
            rule_id: self.metadata.id.clone(),
            description,
            because: self.metadata.because.clone(),
            severity: self.metadata.severity,
            violations,
        }
    }
}

#[derive(Default)]
pub struct RuleSet {
    rules: Vec<Arc<dyn ArchitectureRule>>,
}

impl fmt::Debug for RuleSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuleSet")
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

impl RuleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rule<R>(mut self, rule: R) -> Self
    where
        R: ArchitectureRule + 'static,
    {
        self.rules.push(Arc::new(rule));
        self
    }

    pub fn push<R>(&mut self, rule: R)
    where
        R: ArchitectureRule + 'static,
    {
        self.rules.push(Arc::new(rule));
    }

    pub fn push_shared(&mut self, rule: Arc<dyn ArchitectureRule>) {
        self.rules.push(rule);
    }

    pub fn evaluate(&self, graph: &ArchitectureGraph) -> EvaluationReport {
        let mut report = EvaluationReport {
            analysis_diagnostics: graph.diagnostics().to_vec(),
            rule_results: self.rules.iter().map(|rule| rule.evaluate(graph)).collect(),
        };
        report.sort_deterministically();
        report
    }
}
