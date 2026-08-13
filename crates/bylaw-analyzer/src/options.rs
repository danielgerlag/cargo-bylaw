use bylaw_core::TargetKind;
use camino::Utf8PathBuf;
use indexmap::IndexSet;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum IncompleteAnalysisPolicy {
    #[default]
    Deny,
    Allow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisOptions {
    pub manifest_path: Utf8PathBuf,
    pub selected_package_names: Vec<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target_triple: Option<String>,
    pub included_target_kinds: IndexSet<TargetKind>,
    pub enable_proc_macros: bool,
    pub enable_build_scripts: bool,
    pub incomplete_policy: IncompleteAnalysisPolicy,
}

impl AnalysisOptions {
    pub fn new(manifest_path: impl Into<Utf8PathBuf>) -> Self {
        Self {
            manifest_path: manifest_path.into(),
            ..Self::default()
        }
    }
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            manifest_path: Utf8PathBuf::from("Cargo.toml"),
            selected_package_names: Vec::new(),
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target_triple: None,
            included_target_kinds: IndexSet::from([TargetKind::Library, TargetKind::Binary]),
            enable_proc_macros: true,
            enable_build_scripts: true,
            incomplete_policy: IncompleteAnalysisPolicy::Deny,
        }
    }
}
