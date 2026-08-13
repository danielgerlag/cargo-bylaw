use crate::AnalysisDiagnostic;
use camino::Utf8PathBuf;
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;
use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(PackageId);
string_id!(CrateId);
string_id!(ModuleId);
string_id!(ExternalCrateId);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "kebab-case")]
pub enum ComponentId {
    Crate(CrateId),
    Module(ModuleId),
    ExternalCrate(ExternalCrateId),
}

impl fmt::Display for ComponentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crate(id) => id.fmt(formatter),
            Self::Module(id) => id.fmt(formatter),
            Self::ExternalCrate(id) => id.fmt(formatter),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKind {
    Crate,
    Module,
    ExternalCrate,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    Library,
    Binary,
    Test,
    Example,
    Bench,
    BuildScript,
    ProcMacro,
    Other(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Package {
    pub id: PackageId,
    pub name: String,
    pub version: Option<String>,
    pub manifest_path: Utf8PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CrateNode {
    pub id: CrateId,
    pub package_id: PackageId,
    pub package_name: String,
    pub crate_name: String,
    pub target_name: String,
    pub target_kind: TargetKind,
    pub source_root: Utf8PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModuleNode {
    pub id: ModuleId,
    pub crate_id: CrateId,
    pub package_id: PackageId,
    pub package_name: String,
    pub crate_name: String,
    pub path: String,
    pub parent: Option<ModuleId>,
    pub source_file: Utf8PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExternalCrateNode {
    pub id: ExternalCrateId,
    pub package_name: String,
    pub crate_name: String,
    pub version: Option<String>,
    pub source: Option<String>,
    pub toolchain: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Component {
    Crate(CrateNode),
    Module(ModuleNode),
    ExternalCrate(ExternalCrateNode),
}

impl Component {
    pub fn id(&self) -> ComponentId {
        match self {
            Self::Crate(node) => ComponentId::Crate(node.id.clone()),
            Self::Module(node) => ComponentId::Module(node.id.clone()),
            Self::ExternalCrate(node) => ComponentId::ExternalCrate(node.id.clone()),
        }
    }

    pub fn kind(&self) -> ComponentKind {
        match self {
            Self::Crate(_) => ComponentKind::Crate,
            Self::Module(_) => ComponentKind::Module,
            Self::ExternalCrate(_) => ComponentKind::ExternalCrate,
        }
    }

    pub fn canonical_name(&self) -> &str {
        match self {
            Self::Crate(node) => node.id.as_str(),
            Self::Module(node) => &node.path,
            Self::ExternalCrate(node) => node.id.as_str(),
        }
    }

    pub fn package_name(&self) -> &str {
        match self {
            Self::Crate(node) => &node.package_name,
            Self::Module(node) => &node.package_name,
            Self::ExternalCrate(node) => &node.package_name,
        }
    }

    pub fn crate_name(&self) -> &str {
        match self {
            Self::Crate(node) => &node.crate_name,
            Self::Module(node) => &node.crate_name,
            Self::ExternalCrate(node) => &node.crate_name,
        }
    }

    pub fn target_kind(&self) -> Option<&TargetKind> {
        match self {
            Self::Crate(node) => Some(&node.target_kind),
            Self::Module(_) | Self::ExternalCrate(_) => None,
        }
    }

    pub fn containing_crate(&self) -> Option<CrateId> {
        match self {
            Self::Crate(node) => Some(node.id.clone()),
            Self::Module(node) => Some(node.crate_id.clone()),
            Self::ExternalCrate(_) => None,
        }
    }

    pub fn source_file(&self) -> Option<&Utf8PathBuf> {
        match self {
            Self::Crate(node) => Some(&node.source_root),
            Self::Module(node) => Some(&node.source_file),
            Self::ExternalCrate(_) => None,
        }
    }

    pub fn is_toolchain_crate(&self) -> bool {
        matches!(self, Self::ExternalCrate(node) if node.toolchain)
    }
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct SourceSpan {
    pub path: Utf8PathBuf,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyScope {
    Actual,
    Declared,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyKind {
    Use,
    Path,
    Type,
    Call,
    Macro,
    ReExport,
    CargoNormal,
    CargoBuild,
    CargoDev,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct DependencyEvidence {
    pub kind: DependencyKind,
    pub span: Option<SourceSpan>,
    pub description: Option<String>,
}

impl DependencyEvidence {
    pub fn new(kind: DependencyKind) -> Self {
        Self {
            kind,
            span: None,
            description: None,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DependencyEdge {
    pub origin: ComponentId,
    pub target: ComponentId,
    pub scope: DependencyScope,
    pub evidence: Vec<DependencyEvidence>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AnalysisContext {
    pub manifest_path: Option<Utf8PathBuf>,
    pub target_triple: Option<String>,
    pub features: Vec<String>,
    pub target_kinds: IndexSet<TargetKind>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ArchitectureGraph {
    context: AnalysisContext,
    packages: IndexMap<PackageId, Package>,
    components: IndexMap<ComponentId, Component>,
    edges: Vec<DependencyEdge>,
    diagnostics: Vec<AnalysisDiagnostic>,
}

impl ArchitectureGraph {
    pub fn context(&self) -> &AnalysisContext {
        &self.context
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.values()
    }

    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.components.values()
    }

    pub fn edges(&self) -> impl Iterator<Item = &DependencyEdge> {
        self.edges.iter()
    }

    pub fn diagnostics(&self) -> &[AnalysisDiagnostic] {
        &self.diagnostics
    }

    pub fn package(&self, id: &PackageId) -> Option<&Package> {
        self.packages.get(id)
    }

    pub fn component(&self, id: &ComponentId) -> Option<&Component> {
        self.components.get(id)
    }

    pub fn outgoing(&self, id: &ComponentId) -> impl Iterator<Item = &DependencyEdge> {
        self.edges.iter().filter(move |edge| &edge.origin == id)
    }

    pub fn incoming(&self, id: &ComponentId) -> impl Iterator<Item = &DependencyEdge> {
        self.edges.iter().filter(move |edge| &edge.target == id)
    }
}

#[derive(Debug, Error)]
pub enum GraphBuildError {
    #[error("package `{0}` is already present")]
    DuplicatePackage(PackageId),
    #[error("component `{0}` is already present")]
    DuplicateComponent(ComponentId),
    #[error("component `{component}` references unknown package `{package}`")]
    UnknownPackage {
        component: ComponentId,
        package: PackageId,
    },
    #[error("module `{module}` references unknown crate `{crate_id}`")]
    UnknownCrate { module: ModuleId, crate_id: CrateId },
    #[error("dependency references unknown {role} component `{component}`")]
    UnknownDependencyComponent {
        role: &'static str,
        component: ComponentId,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct EdgeKey {
    origin: ComponentId,
    target: ComponentId,
    scope: DependencyScope,
}

#[derive(Clone, Debug, Default)]
pub struct GraphBuilder {
    context: AnalysisContext,
    packages: IndexMap<PackageId, Package>,
    components: IndexMap<ComponentId, Component>,
    edges: IndexMap<EdgeKey, DependencyEdge>,
    diagnostics: Vec<AnalysisDiagnostic>,
}

impl GraphBuilder {
    pub fn new(context: AnalysisContext) -> Self {
        Self {
            context,
            ..Self::default()
        }
    }

    pub fn add_package(&mut self, package: Package) -> Result<(), GraphBuildError> {
        if self.packages.contains_key(&package.id) {
            return Err(GraphBuildError::DuplicatePackage(package.id));
        }
        self.packages.insert(package.id.clone(), package);
        Ok(())
    }

    pub fn add_component(&mut self, component: Component) -> Result<(), GraphBuildError> {
        let id = component.id();
        if self.components.contains_key(&id) {
            return Err(GraphBuildError::DuplicateComponent(id));
        }

        match &component {
            Component::Crate(node) => {
                if !self.packages.contains_key(&node.package_id) {
                    return Err(GraphBuildError::UnknownPackage {
                        component: id,
                        package: node.package_id.clone(),
                    });
                }
            }
            Component::Module(node) => {
                if !self.packages.contains_key(&node.package_id) {
                    return Err(GraphBuildError::UnknownPackage {
                        component: id,
                        package: node.package_id.clone(),
                    });
                }
                let crate_id = ComponentId::Crate(node.crate_id.clone());
                if !self.components.contains_key(&crate_id) {
                    return Err(GraphBuildError::UnknownCrate {
                        module: node.id.clone(),
                        crate_id: node.crate_id.clone(),
                    });
                }
            }
            Component::ExternalCrate(_) => {}
        }

        self.components.insert(id, component);
        Ok(())
    }

    pub fn add_dependency(
        &mut self,
        origin: ComponentId,
        target: ComponentId,
        scope: DependencyScope,
        evidence: DependencyEvidence,
    ) -> Result<(), GraphBuildError> {
        if !self.components.contains_key(&origin) {
            return Err(GraphBuildError::UnknownDependencyComponent {
                role: "origin",
                component: origin,
            });
        }
        if !self.components.contains_key(&target) {
            return Err(GraphBuildError::UnknownDependencyComponent {
                role: "target",
                component: target,
            });
        }

        let key = EdgeKey {
            origin: origin.clone(),
            target: target.clone(),
            scope,
        };
        let edge = self.edges.entry(key).or_insert_with(|| DependencyEdge {
            origin,
            target,
            scope,
            evidence: Vec::new(),
        });
        if !edge.evidence.contains(&evidence) {
            edge.evidence.push(evidence);
        }
        Ok(())
    }

    pub fn add_diagnostic(&mut self, diagnostic: AnalysisDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn finish(self) -> ArchitectureGraph {
        let mut packages = self.packages;
        packages.sort_keys();
        let mut components = self.components;
        components.sort_keys();
        let mut edges = self.edges.into_values().collect::<Vec<_>>();
        for edge in &mut edges {
            edge.evidence.sort_by(|left, right| {
                (
                    &left.kind,
                    left.span.as_ref().map(|span| {
                        (
                            span.path.as_str(),
                            span.start.line,
                            span.start.column,
                            span.end.line,
                            span.end.column,
                        )
                    }),
                    &left.description,
                )
                    .cmp(&(
                        &right.kind,
                        right.span.as_ref().map(|span| {
                            (
                                span.path.as_str(),
                                span.start.line,
                                span.start.column,
                                span.end.line,
                                span.end.column,
                            )
                        }),
                        &right.description,
                    ))
            });
        }
        edges.sort_by(|left, right| {
            (&left.origin, &left.target, left.scope).cmp(&(
                &right.origin,
                &right.target,
                right.scope,
            ))
        });
        ArchitectureGraph {
            context: self.context,
            packages,
            components,
            edges,
            diagnostics: self.diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> Package {
        Package {
            id: PackageId::new("shop 0.1.0"),
            name: "shop".to_owned(),
            version: Some("0.1.0".to_owned()),
            manifest_path: Utf8PathBuf::from("Cargo.toml"),
        }
    }

    fn crate_node(package_id: PackageId) -> CrateNode {
        CrateNode {
            id: CrateId::new("shop#lib"),
            package_id,
            package_name: "shop".to_owned(),
            crate_name: "shop".to_owned(),
            target_name: "shop".to_owned(),
            target_kind: TargetKind::Library,
            source_root: Utf8PathBuf::from("src/lib.rs"),
        }
    }

    #[test]
    fn aggregates_duplicate_edges_and_evidence() {
        let package = package();
        let crate_node = crate_node(package.id.clone());
        let crate_id = ComponentId::Crate(crate_node.id.clone());
        let external = ExternalCrateNode {
            id: ExternalCrateId::new("serde@1"),
            package_name: "serde".to_owned(),
            crate_name: "serde".to_owned(),
            version: Some("1".to_owned()),
            source: None,
            toolchain: false,
        };
        let external_id = ComponentId::ExternalCrate(external.id.clone());

        let mut builder = GraphBuilder::default();
        builder.add_package(package).unwrap();
        builder.add_component(Component::Crate(crate_node)).unwrap();
        builder
            .add_component(Component::ExternalCrate(external))
            .unwrap();

        let evidence = DependencyEvidence::new(DependencyKind::CargoNormal);
        builder
            .add_dependency(
                crate_id.clone(),
                external_id.clone(),
                DependencyScope::Declared,
                evidence.clone(),
            )
            .unwrap();
        builder
            .add_dependency(crate_id, external_id, DependencyScope::Declared, evidence)
            .unwrap();

        let graph = builder.finish();
        let edge = graph.edges().next().unwrap();
        assert_eq!(edge.evidence.len(), 1);
    }
}
