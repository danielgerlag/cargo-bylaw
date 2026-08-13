use crate::{ArchitectureGraph, Component, ComponentKind, TargetKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone, Copy)]
pub struct Candidate<'a> {
    graph: &'a ArchitectureGraph,
    component: &'a Component,
}

impl<'a> Candidate<'a> {
    pub fn new(graph: &'a ArchitectureGraph, component: &'a Component) -> Self {
        Self { graph, component }
    }

    pub fn graph(self) -> &'a ArchitectureGraph {
        self.graph
    }

    pub fn component(self) -> &'a Component {
        self.component
    }
}

pub trait Selector: Send + Sync {
    fn description(&self) -> &str;
    fn matches(&self, candidate: Candidate<'_>) -> bool;
}

#[derive(Clone)]
pub struct DescribedSelector {
    inner: Arc<dyn Selector>,
}

impl fmt::Debug for DescribedSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DescribedSelector")
            .field("description", &self.description())
            .finish_non_exhaustive()
    }
}

impl DescribedSelector {
    pub fn new<F>(description: impl Into<String>, predicate: F) -> Self
    where
        F: for<'a> Fn(Candidate<'a>) -> bool + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(FunctionSelector {
                description: description.into(),
                predicate,
            }),
        }
    }

    pub fn all() -> Self {
        Self::new("all components", |_| true)
    }

    pub fn description(&self) -> &str {
        self.inner.description()
    }

    pub fn matches(&self, candidate: Candidate<'_>) -> bool {
        self.inner.matches(candidate)
    }

    pub fn and(self, other: Self) -> Self {
        let description = format!("{} and {}", self.description(), other.description());
        Self::new(description, move |candidate| {
            self.matches(candidate) && other.matches(candidate)
        })
    }

    pub fn or(self, other: Self) -> Self {
        let description = format!("{} or {}", self.description(), other.description());
        Self::new(description, move |candidate| {
            self.matches(candidate) || other.matches(candidate)
        })
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        let description = format!("not {}", self.description());
        Self::new(description, move |candidate| !self.matches(candidate))
    }
}

impl Selector for DescribedSelector {
    fn description(&self) -> &str {
        self.description()
    }

    fn matches(&self, candidate: Candidate<'_>) -> bool {
        self.matches(candidate)
    }
}

struct FunctionSelector<F> {
    description: String,
    predicate: F,
}

impl<F> Selector for FunctionSelector<F>
where
    F: for<'a> Fn(Candidate<'a>) -> bool + Send + Sync,
{
    fn description(&self) -> &str {
        &self.description
    }

    fn matches(&self, candidate: Candidate<'_>) -> bool {
        (self.predicate)(candidate)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SelectorSpec {
    All,
    Packages { names: Vec<String> },
    Crates { names: Vec<String> },
    Modules { patterns: Vec<String> },
    ExternalCrates { names: Vec<String> },
    TargetKinds { kinds: Vec<TargetKind> },
    ComponentKinds { kinds: Vec<ComponentKind> },
    AnyOf { selectors: Vec<SelectorSpec> },
    AllOf { selectors: Vec<SelectorSpec> },
    Not { selector: Box<SelectorSpec> },
}

impl SelectorSpec {
    pub fn compile(&self) -> Result<DescribedSelector, PathPatternError> {
        match self {
            Self::All => Ok(DescribedSelector::all()),
            Self::Packages { names } => {
                let names = names.clone();
                let description = format!("packages [{}]", names.join(", "));
                Ok(DescribedSelector::new(description, move |candidate| {
                    matches!(
                        candidate.component(),
                        Component::Crate(_) | Component::Module(_)
                    ) && names
                        .iter()
                        .any(|name| name == candidate.component().package_name())
                }))
            }
            Self::Crates { names } => {
                let names = names.clone();
                let description = format!("crates [{}]", names.join(", "));
                Ok(DescribedSelector::new(description, move |candidate| {
                    matches!(
                        candidate.component(),
                        Component::Crate(_) | Component::Module(_)
                    ) && names
                        .iter()
                        .any(|name| name == candidate.component().crate_name())
                }))
            }
            Self::Modules { patterns } => {
                let compiled = patterns
                    .iter()
                    .map(PathPattern::new)
                    .collect::<Result<Vec<_>, _>>()?;
                let description = format!("modules [{}]", patterns.join(", "));
                Ok(DescribedSelector::new(description, move |candidate| {
                    let Component::Module(module) = candidate.component() else {
                        return false;
                    };
                    compiled.iter().any(|pattern| pattern.matches(&module.path))
                }))
            }
            Self::ExternalCrates { names } => {
                let names = names.clone();
                let description = format!("external crates [{}]", names.join(", "));
                Ok(DescribedSelector::new(description, move |candidate| {
                    matches!(candidate.component(), Component::ExternalCrate(_))
                        && names
                            .iter()
                            .any(|name| name == candidate.component().crate_name())
                }))
            }
            Self::TargetKinds { kinds } => {
                let kinds = kinds.clone();
                let description = format!(
                    "target kinds [{}]",
                    kinds
                        .iter()
                        .map(|kind| format!("{kind:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                Ok(DescribedSelector::new(description, move |candidate| {
                    let component = candidate.component();
                    let direct = component.target_kind();
                    let inherited = component.containing_crate().and_then(|crate_id| {
                        candidate
                            .graph()
                            .component(&crate::ComponentId::Crate(crate_id))
                            .and_then(Component::target_kind)
                    });
                    direct
                        .or(inherited)
                        .is_some_and(|kind| kinds.contains(kind))
                }))
            }
            Self::ComponentKinds { kinds } => {
                let kinds = kinds.clone();
                let description = format!(
                    "component kinds [{}]",
                    kinds
                        .iter()
                        .map(|kind| format!("{kind:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                Ok(DescribedSelector::new(description, move |candidate| {
                    kinds.contains(&candidate.component().kind())
                }))
            }
            Self::AnyOf { selectors } => {
                let mut selectors = selectors.iter();
                let Some(first) = selectors.next() else {
                    return Ok(DescribedSelector::new("no components", |_| false));
                };
                selectors.try_fold(first.compile()?, |combined, selector| {
                    Ok(combined.or(selector.compile()?))
                })
            }
            Self::AllOf { selectors } => {
                let mut selectors = selectors.iter();
                let Some(first) = selectors.next() else {
                    return Ok(DescribedSelector::all());
                };
                selectors.try_fold(first.compile()?, |combined, selector| {
                    Ok(combined.and(selector.compile()?))
                })
            }
            Self::Not { selector } => Ok(selector.compile()?.not()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PatternSegment {
    Exact(String),
    One,
    Many,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathPattern {
    source: String,
    segments: Vec<PatternSegment>,
}

impl PathPattern {
    pub fn new(pattern: impl Into<String>) -> Result<Self, PathPatternError> {
        let source = pattern.into();
        if source.is_empty() {
            return Err(PathPatternError::Empty);
        }
        let mut segments = Vec::new();
        for segment in source.split("::") {
            if segment.is_empty() {
                return Err(PathPatternError::EmptySegment(source));
            }
            let segment = match segment {
                "*" => PatternSegment::One,
                "**" => PatternSegment::Many,
                value if value.contains('*') => {
                    return Err(PathPatternError::InvalidWildcard(value.to_owned()));
                }
                value => PatternSegment::Exact(value.to_owned()),
            };
            segments.push(segment);
        }
        Ok(Self { source, segments })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    pub fn matches(&self, path: &str) -> bool {
        let path = path.split("::").collect::<Vec<_>>();
        let mut memo = HashMap::new();
        self.matches_from(0, 0, &path, &mut memo)
    }

    fn matches_from(
        &self,
        pattern_index: usize,
        path_index: usize,
        path: &[&str],
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(result) = memo.get(&(pattern_index, path_index)) {
            return *result;
        }

        let result = match self.segments.get(pattern_index) {
            None => path_index == path.len(),
            Some(PatternSegment::Exact(expected)) => {
                path.get(path_index)
                    .is_some_and(|actual| *actual == expected)
                    && self.matches_from(pattern_index + 1, path_index + 1, path, memo)
            }
            Some(PatternSegment::One) => {
                path_index < path.len()
                    && self.matches_from(pattern_index + 1, path_index + 1, path, memo)
            }
            Some(PatternSegment::Many) => {
                self.matches_from(pattern_index + 1, path_index, path, memo)
                    || (path_index < path.len()
                        && self.matches_from(pattern_index, path_index + 1, path, memo))
            }
        };
        memo.insert((pattern_index, path_index), result);
        result
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PathPatternError {
    #[error("path pattern cannot be empty")]
    Empty,
    #[error("path pattern `{0}` contains an empty segment")]
    EmptySegment(String),
    #[error("wildcards must occupy an entire path segment, found `{0}`")]
    InvalidWildcard(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExternalCrateId, ExternalCrateNode};

    #[test]
    fn path_pattern_matches_rust_segments() {
        let pattern = PathPattern::new("shop::domain::**").unwrap();
        assert!(pattern.matches("shop::domain"));
        assert!(pattern.matches("shop::domain::order"));
        assert!(!pattern.matches("shop::api::domain"));

        let one = PathPattern::new("shop::*::model").unwrap();
        assert!(one.matches("shop::domain::model"));
        assert!(!one.matches("shop::deep::domain::model"));
    }

    #[test]
    fn package_selectors_do_not_match_external_crates() {
        let graph = ArchitectureGraph::default();
        let external = Component::ExternalCrate(ExternalCrateNode {
            id: ExternalCrateId::new("serde@1"),
            package_name: "serde".to_owned(),
            crate_name: "serde".to_owned(),
            version: Some("1".to_owned()),
            source: None,
            toolchain: false,
        });
        let selector = SelectorSpec::Packages {
            names: vec!["serde".to_owned()],
        }
        .compile()
        .unwrap();

        assert!(!selector.matches(Candidate::new(&graph, &external)));
    }
}
