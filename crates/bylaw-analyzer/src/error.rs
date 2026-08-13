use bylaw_core::GraphBuildError;
use camino::Utf8PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("manifest path `{0}` does not exist")]
    ManifestPathDoesNotExist(Utf8PathBuf),
    #[error("manifest path `{0}` must point to a Cargo.toml file")]
    InvalidManifestPath(Utf8PathBuf),
    #[error("invalid analysis options: {0}")]
    InvalidOptions(String),
    #[error("requested packages were not found in the workspace: {packages:?}")]
    UnknownPackages { packages: Vec<String> },
    #[error("cargo metadata failed for `{manifest_path}`: {source}")]
    CargoMetadata {
        manifest_path: Utf8PathBuf,
        #[source]
        source: cargo_metadata::Error,
    },
    #[error("cargo metadata for `{manifest_path}` did not include a resolve graph")]
    MissingResolveGraph { manifest_path: Utf8PathBuf },
    #[error("rust-analyzer workspace load failed for `{manifest_path}`: {message}")]
    WorkspaceLoad {
        manifest_path: Utf8PathBuf,
        message: String,
    },
    #[error("failed to read `{path}`: {source}")]
    ReadFile {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    GraphBuild(#[from] GraphBuildError),
}
