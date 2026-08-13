//! Versioned `bylaw.toml` configuration and lowering into core rule specs.

use bylaw_core::{
    BuiltInRuleSpec, ComponentKind, CycleGrouping, DependencyScopes, LayerDependencySpec,
    NamedSelectorSpec, RuleBuildError, RuleMetadata, SelectorSpec, Severity, TargetKind,
};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::ops::Range;
use thiserror::Error;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub selectors: BTreeMap<String, SelectorConfig>,
    #[serde(default, rename = "rule")]
    pub rules: Vec<RuleConfig>,
}

impl Config {
    pub fn from_toml(source: &str) -> Result<Self, ConfigError> {
        let config = toml::from_str::<Self>(source).map_err(|error| {
            let span = error.span();
            let (line, column) = span
                .as_ref()
                .map(|span| line_column(source, span.start))
                .unzip();
            ConfigError::Parse {
                message: error.message().to_owned(),
                span,
                line,
                column,
            }
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: impl AsRef<Utf8Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_toml(&source).map_err(|error| error.with_path(path.to_owned()))
    }

    pub fn compile(&self) -> Result<CompiledConfig, ConfigError> {
        self.validate()?;
        let selectors = self
            .selectors
            .iter()
            .map(|(name, selector)| {
                if name.trim().is_empty() {
                    return Err(ConfigError::EmptySelectorName);
                }
                selector
                    .lower()
                    .map(|selector| (name.clone(), selector))
                    .map_err(|error| ConfigError::InvalidSelector {
                        selector: name.clone(),
                        message: error,
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let mut ids = HashSet::new();
        let mut rules = Vec::with_capacity(self.rules.len());
        for rule in &self.rules {
            let id = rule.common().id.clone();
            if id.trim().is_empty() {
                return Err(ConfigError::EmptyRuleId);
            }
            if !ids.insert(id.clone()) {
                return Err(ConfigError::DuplicateRuleId(id));
            }
            let spec = rule.lower(&selectors)?;
            spec.compile()
                .map_err(|source| ConfigError::InvalidRule { rule: id, source })?;
            rules.push(spec);
        }

        Ok(CompiledConfig {
            analysis: self.analysis.clone(),
            output: self.output,
            rules,
        })
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: self.version,
                supported: CONFIG_VERSION,
            });
        }
        if self.analysis.all_features && self.analysis.no_default_features {
            return Err(ConfigError::ConflictingFeatureOptions);
        }
        if self.analysis.target_kinds.is_empty() {
            return Err(ConfigError::EmptyTargetKinds);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledConfig {
    pub analysis: AnalysisConfig,
    pub output: OutputConfig,
    pub rules: Vec<BuiltInRuleSpec>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnalysisConfig {
    pub manifest_path: Option<Utf8PathBuf>,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub no_default_features: bool,
    pub target: Option<String>,
    #[serde(default = "default_target_kinds")]
    pub target_kinds: Vec<TargetKindConfig>,
    #[serde(default)]
    pub incomplete: IncompleteAnalysisPolicy,
    #[serde(default = "default_true")]
    pub proc_macros: bool,
    #[serde(default = "default_true")]
    pub build_scripts: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            manifest_path: None,
            packages: Vec::new(),
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target: None,
            target_kinds: default_target_kinds(),
            incomplete: IncompleteAnalysisPolicy::Deny,
            proc_macros: true,
            build_scripts: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    #[serde(default)]
    pub format: OutputFormatConfig,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormatConfig {
    #[default]
    Human,
    Json,
}

fn default_true() -> bool {
    true
}

fn default_target_kinds() -> Vec<TargetKindConfig> {
    vec![TargetKindConfig::Library, TargetKindConfig::Binary]
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum IncompleteAnalysisPolicy {
    #[default]
    Deny,
    Allow,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKindConfig {
    Library,
    Binary,
    Test,
    Example,
    Bench,
    BuildScript,
    ProcMacro,
}

impl From<&TargetKindConfig> for TargetKind {
    fn from(value: &TargetKindConfig) -> Self {
        match value {
            TargetKindConfig::Library => Self::Library,
            TargetKindConfig::Binary => Self::Binary,
            TargetKindConfig::Test => Self::Test,
            TargetKindConfig::Example => Self::Example,
            TargetKindConfig::Bench => Self::Bench,
            TargetKindConfig::BuildScript => Self::BuildScript,
            TargetKindConfig::ProcMacro => Self::ProcMacro,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SelectorConfig {
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub crates: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default)]
    pub external_crates: Vec<String>,
    #[serde(default)]
    pub target_kinds: Vec<TargetKindConfig>,
    #[serde(default)]
    pub component_kinds: Vec<ComponentKindConfig>,
}

impl SelectorConfig {
    fn lower(&self) -> Result<SelectorSpec, String> {
        if self
            .packages
            .iter()
            .chain(&self.crates)
            .chain(&self.modules)
            .chain(&self.external_crates)
            .any(|value| value.trim().is_empty())
        {
            return Err("selector matcher values cannot be empty".to_owned());
        }
        let mut selectors = Vec::new();
        if !self.packages.is_empty() {
            selectors.push(SelectorSpec::Packages {
                names: self.packages.clone(),
            });
        }
        if !self.crates.is_empty() {
            selectors.push(SelectorSpec::Crates {
                names: self.crates.clone(),
            });
        }
        if !self.modules.is_empty() {
            selectors.push(SelectorSpec::Modules {
                patterns: self.modules.clone(),
            });
        }
        if !self.external_crates.is_empty() {
            selectors.push(SelectorSpec::ExternalCrates {
                names: self.external_crates.clone(),
            });
        }
        if !self.target_kinds.is_empty() {
            selectors.push(SelectorSpec::TargetKinds {
                kinds: self.target_kinds.iter().map(TargetKind::from).collect(),
            });
        }
        if !self.component_kinds.is_empty() {
            selectors.push(SelectorSpec::ComponentKinds {
                kinds: self
                    .component_kinds
                    .iter()
                    .map(ComponentKind::from)
                    .collect(),
            });
        }

        match selectors.len() {
            0 => Err("selector must define at least one matcher".to_owned()),
            1 => Ok(selectors.pop().expect("length was checked")),
            _ => Ok(SelectorSpec::AllOf { selectors }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKindConfig {
    Crate,
    Module,
    ExternalCrate,
}

impl From<&ComponentKindConfig> for ComponentKind {
    fn from(value: &ComponentKindConfig) -> Self {
        match value {
            ComponentKindConfig::Crate => Self::Crate,
            ComponentKindConfig::Module => Self::Module,
            ComponentKindConfig::ExternalCrate => Self::ExternalCrate,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum SelectorReference {
    One(String),
    Many(Vec<String>),
}

impl SelectorReference {
    fn names(&self) -> &[String] {
        match self {
            Self::One(name) => std::slice::from_ref(name),
            Self::Many(names) => names,
        }
    }

    fn resolve(
        &self,
        selectors: &BTreeMap<String, SelectorSpec>,
    ) -> Result<SelectorSpec, ConfigError> {
        let mut resolved = self
            .names()
            .iter()
            .map(|name| {
                selectors
                    .get(name)
                    .cloned()
                    .ok_or_else(|| ConfigError::UnknownSelector(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        match resolved.len() {
            0 => Err(ConfigError::EmptySelectorReference),
            1 => Ok(resolved.pop().expect("length was checked")),
            _ => Ok(SelectorSpec::AnyOf {
                selectors: resolved,
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RuleConfig {
    ForbidDependencies {
        #[serde(flatten)]
        common: RuleCommon,
        from: SelectorReference,
        to: SelectorReference,
        #[serde(default)]
        scope: DependencyScopes,
    },
    OnlyDependencies {
        #[serde(flatten)]
        common: RuleCommon,
        from: SelectorReference,
        allowed: SelectorReference,
        #[serde(default)]
        scope: DependencyScopes,
        #[serde(default = "default_true")]
        allow_toolchain: bool,
        #[serde(default = "default_true")]
        allow_self: bool,
    },
    Layers {
        #[serde(flatten)]
        common: RuleCommon,
        layers: Vec<LayerConfig>,
        #[serde(default)]
        dependencies: Vec<LayerDependencyConfig>,
        #[serde(default)]
        scope: DependencyScopes,
    },
    NoCycles {
        #[serde(flatten)]
        common: RuleCommon,
        within: SelectorReference,
        #[serde(default)]
        grouping: CycleGroupingConfig,
        #[serde(default)]
        slices: Vec<LayerConfig>,
        #[serde(default)]
        scope: DependencyScopes,
    },
}

impl RuleConfig {
    fn common(&self) -> &RuleCommon {
        match self {
            Self::ForbidDependencies { common, .. }
            | Self::OnlyDependencies { common, .. }
            | Self::Layers { common, .. }
            | Self::NoCycles { common, .. } => common,
        }
    }

    fn lower(
        &self,
        selectors: &BTreeMap<String, SelectorSpec>,
    ) -> Result<BuiltInRuleSpec, ConfigError> {
        match self {
            Self::ForbidDependencies {
                common,
                from,
                to,
                scope,
            } => Ok(BuiltInRuleSpec::ForbidDependencies {
                metadata: common.metadata(),
                from: from.resolve(selectors)?,
                to: to.resolve(selectors)?,
                scopes: *scope,
            }),
            Self::OnlyDependencies {
                common,
                from,
                allowed,
                scope,
                allow_toolchain,
                allow_self,
            } => Ok(BuiltInRuleSpec::OnlyDependencies {
                metadata: common.metadata(),
                from: from.resolve(selectors)?,
                allowed: allowed.resolve(selectors)?,
                scopes: *scope,
                allow_toolchain: *allow_toolchain,
                allow_self: *allow_self,
            }),
            Self::Layers {
                common,
                layers,
                dependencies,
                scope,
            } => Ok(BuiltInRuleSpec::Layers {
                metadata: common.metadata(),
                layers: lower_named_selectors(layers, selectors)?,
                dependencies: dependencies
                    .iter()
                    .map(|dependency| LayerDependencySpec {
                        from: dependency.from.clone(),
                        may_depend_on: dependency.may_depend_on.clone(),
                    })
                    .collect(),
                scopes: *scope,
            }),
            Self::NoCycles {
                common,
                within,
                grouping,
                slices,
                scope,
            } => {
                let grouping = match grouping {
                    CycleGroupingConfig::Components => CycleGrouping::Components,
                    CycleGroupingConfig::Modules => CycleGrouping::Modules,
                    CycleGroupingConfig::Crates => CycleGrouping::Crates,
                    CycleGroupingConfig::Slices => CycleGrouping::Slices {
                        slices: lower_named_selectors(slices, selectors)?,
                    },
                };
                if matches!(grouping, CycleGrouping::Slices { ref slices } if slices.is_empty()) {
                    return Err(ConfigError::MissingSlices(common.id.clone()));
                }
                Ok(BuiltInRuleSpec::NoCycles {
                    metadata: common.metadata(),
                    within: within.resolve(selectors)?,
                    grouping,
                    scopes: *scope,
                })
            }
        }
    }
}

fn lower_named_selectors(
    layers: &[LayerConfig],
    selectors: &BTreeMap<String, SelectorSpec>,
) -> Result<Vec<NamedSelectorSpec>, ConfigError> {
    layers
        .iter()
        .map(|layer| {
            if layer.name.trim().is_empty() {
                return Err(ConfigError::EmptyLayerName);
            }
            Ok(NamedSelectorSpec {
                name: layer.name.clone(),
                selector: SelectorReference::One(layer.selector.clone()).resolve(selectors)?,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuleCommon {
    pub id: String,
    pub description: Option<String>,
    pub because: Option<String>,
    #[serde(default)]
    pub severity: Severity,
}

impl RuleCommon {
    fn metadata(&self) -> RuleMetadata {
        RuleMetadata {
            id: self.id.clone(),
            description: self.description.clone(),
            because: self.because.clone(),
            severity: self.severity,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayerConfig {
    pub name: String,
    pub selector: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayerDependencyConfig {
    pub from: String,
    #[serde(default)]
    pub may_depend_on: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CycleGroupingConfig {
    Components,
    Modules,
    #[default]
    Crates,
    Slices,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read configuration `{path}`: {source}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration: {message}")]
    Parse {
        message: String,
        span: Option<Range<usize>>,
        line: Option<usize>,
        column: Option<usize>,
    },
    #[error("invalid configuration `{path}`: {message}")]
    ParseFile {
        path: Utf8PathBuf,
        message: String,
        span: Option<Range<usize>>,
        line: Option<usize>,
        column: Option<usize>,
    },
    #[error("unsupported configuration version {found}; supported version is {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("`all_features` and `no_default_features` cannot both be enabled")]
    ConflictingFeatureOptions,
    #[error("analysis must include at least one target kind")]
    EmptyTargetKinds,
    #[error("selector `{selector}` is invalid: {message}")]
    InvalidSelector { selector: String, message: String },
    #[error("selector names cannot be empty")]
    EmptySelectorName,
    #[error("rule IDs cannot be empty")]
    EmptyRuleId,
    #[error("layer names cannot be empty")]
    EmptyLayerName,
    #[error("rule ID `{0}` is duplicated")]
    DuplicateRuleId(String),
    #[error("unknown selector `{0}`")]
    UnknownSelector(String),
    #[error("selector reference cannot be empty")]
    EmptySelectorReference,
    #[error("cycle rule `{0}` uses slice grouping but defines no slices")]
    MissingSlices(String),
    #[error("rule `{rule}` is invalid: {source}")]
    InvalidRule {
        rule: String,
        #[source]
        source: RuleBuildError,
    },
}

impl ConfigError {
    fn with_path(self, path: Utf8PathBuf) -> Self {
        match self {
            Self::Parse {
                message,
                span,
                line,
                column,
            } => Self::ParseFile {
                path,
                message,
                span,
                line,
                column,
            },
            error => error,
        }
    }

    pub fn span(&self) -> Option<Range<usize>> {
        match self {
            Self::Parse { span, .. } | Self::ParseFile { span, .. } => span.clone(),
            _ => None,
        }
    }

    pub fn render(&self) -> String {
        match self {
            Self::Parse {
                message,
                line: Some(line),
                column: Some(column),
                ..
            } => format!("invalid configuration at {line}:{column}: {message}"),
            Self::ParseFile {
                path,
                message,
                line: Some(line),
                column: Some(column),
                ..
            } => format!("invalid configuration `{path}` at {line}:{column}: {message}"),
            _ => self.to_string(),
        }
    }
}

fn line_column(source: &str, byte_offset: usize) -> (usize, usize) {
    let prefix = &source[..byte_offset.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL_BOUNDARIES: &str = r#"
version = 1

[selectors.domain]
packages = ["shop-domain"]

[selectors.persistence]
packages = ["shop-persistence"]

[selectors.contract]
packages = ["shop-contract"]

[selectors.api]
packages = ["shop-api"]

[[rule]]
id = "domain-is-internal"
kind = "forbid-dependencies"
from = "domain"
to = ["persistence", "contract", "api"]
scope = "both"
because = "the internal domain model must not leak boundary representations"

[[rule]]
id = "model-crates-are-acyclic"
kind = "no-cycles"
within = ["domain", "persistence", "contract", "api"]
grouping = "crates"
"#;

    #[test]
    fn parses_and_lowers_model_boundary_rules() {
        let config = Config::from_toml(MODEL_BOUNDARIES).unwrap();
        let compiled = config.compile().unwrap();
        assert_eq!(compiled.rules.len(), 2);
        assert_eq!(
            compiled.analysis.target_kinds,
            vec![TargetKindConfig::Library, TargetKindConfig::Binary]
        );
        assert_eq!(compiled.output.format, OutputFormatConfig::Human);
    }

    #[test]
    fn repository_example_is_valid_configuration() {
        let source = include_str!("../../../examples/model-boundaries/bylaw.toml");
        let compiled = Config::from_toml(source).unwrap().compile().unwrap();
        assert_eq!(compiled.rules.len(), 6);
    }

    #[test]
    fn repository_self_enforcement_is_valid_configuration() {
        let source = include_str!("../../../bylaw.toml");
        let compiled = Config::from_toml(source).unwrap().compile().unwrap();
        assert_eq!(compiled.rules.len(), 5);
    }

    #[test]
    fn module_fixture_configurations_are_valid() {
        for source in [
            include_str!("../../../fixtures/model-boundaries-modules-valid/bylaw.toml"),
            include_str!("../../../fixtures/model-boundaries-modules-invalid/bylaw.toml"),
        ] {
            let compiled = Config::from_toml(source).unwrap().compile().unwrap();
            assert_eq!(compiled.rules.len(), 4);
        }
    }

    #[test]
    fn rejects_unknown_selector_references() {
        let config = Config::from_toml(
            r#"
version = 1

[[rule]]
id = "broken"
kind = "forbid-dependencies"
from = "missing"
to = "also-missing"
"#,
        )
        .unwrap();

        assert!(matches!(
            config.compile(),
            Err(ConfigError::UnknownSelector(name)) if name == "missing"
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = Config::from_toml(
            r#"
version = 1
surprise = true
"#,
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::Parse { .. }));
        assert!(error.span().is_some());
        assert!(error.render().contains("at 3:1"));
    }

    #[test]
    fn rejects_unknown_rule_fields() {
        let error = Config::from_toml(
            r#"
version = 1

[selectors.domain]
packages = ["domain"]

[[rule]]
id = "broken"
kind = "forbid-dependencies"
from = "domain"
to = "domain"
surprise = true
"#,
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::Parse { .. }));
    }

    #[test]
    fn rejects_an_empty_target_set() {
        let error = Config::from_toml(
            r#"
version = 1

[analysis]
target_kinds = []
"#,
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::EmptyTargetKinds));
    }
}
