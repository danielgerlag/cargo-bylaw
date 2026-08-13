use crate::{ComponentId, DependencyEvidence, SourceSpan};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    #[default]
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AnalysisDiagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub help: Option<String>,
}

impl AnalysisDiagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            span: None,
            help: None,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            span: None,
            help: None,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConditionEvent {
    pub message: String,
    pub origin: Option<ComponentId>,
    pub target: Option<ComponentId>,
    pub evidence: Vec<DependencyEvidence>,
    pub cycle: Vec<ComponentId>,
    pub help: Option<String>,
}

impl ConditionEvent {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }

    pub fn with_edge(
        mut self,
        origin: ComponentId,
        target: ComponentId,
        evidence: Vec<DependencyEvidence>,
    ) -> Self {
        self.origin = Some(origin);
        self.target = Some(target);
        self.evidence = evidence;
        self
    }

    pub fn with_cycle(mut self, cycle: Vec<ComponentId>) -> Self {
        self.cycle = cycle;
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Violation {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub origin: Option<ComponentId>,
    pub target: Option<ComponentId>,
    pub evidence: Vec<DependencyEvidence>,
    pub cycle: Vec<ComponentId>,
    pub help: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuleResult {
    pub rule_id: String,
    pub description: String,
    pub because: Option<String>,
    pub severity: Severity,
    pub violations: Vec<Violation>,
}

impl RuleResult {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvaluationReport {
    pub analysis_diagnostics: Vec<AnalysisDiagnostic>,
    pub rule_results: Vec<RuleResult>,
}

impl EvaluationReport {
    pub fn is_success(&self) -> bool {
        let analysis_failed = self
            .analysis_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error);
        let rule_failed = self
            .rule_results
            .iter()
            .any(|result| result.severity == Severity::Error && !result.violations.is_empty());
        !analysis_failed && !rule_failed
    }

    pub fn violations(&self) -> impl Iterator<Item = &Violation> {
        self.rule_results
            .iter()
            .flat_map(|result| result.violations.iter())
    }

    pub fn sort_deterministically(&mut self) {
        self.analysis_diagnostics.sort_by(|left, right| {
            (
                &left.code,
                left.span.as_ref().map(|span| span.path.as_str()),
                &left.message,
            )
                .cmp(&(
                    &right.code,
                    right.span.as_ref().map(|span| span.path.as_str()),
                    &right.message,
                ))
        });
        self.rule_results
            .sort_by(|left, right| left.rule_id.cmp(&right.rule_id));
        for result in &mut self.rule_results {
            result.violations.sort_by(|left, right| {
                (
                    &left.message,
                    left.origin.as_ref(),
                    left.target.as_ref(),
                    left.evidence
                        .first()
                        .and_then(|evidence| evidence.span.as_ref())
                        .map(|span| (span.path.as_str(), span.start.line, span.start.column)),
                )
                    .cmp(&(
                        &right.message,
                        right.origin.as_ref(),
                        right.target.as_ref(),
                        right
                            .evidence
                            .first()
                            .and_then(|evidence| evidence.span.as_ref())
                            .map(|span| (span.path.as_str(), span.start.line, span.start.column)),
                    ))
            });
        }
    }
}
