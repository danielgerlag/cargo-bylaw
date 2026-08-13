mod analyzer;
mod error;
mod options;

pub use analyzer::analyze_workspace;
pub use error::AnalyzerError;
pub use options::{AnalysisOptions, IncompleteAnalysisPolicy};
