use bylaw::{
    Rules,
    analyzer::{
        AnalysisOptions, IncompleteAnalysisPolicy as AnalyzerIncompletePolicy, analyze_workspace,
    },
    config::{
        AnalysisConfig, Config, IncompleteAnalysisPolicy as ConfigIncompletePolicy,
        OutputFormatConfig, TargetKindConfig,
    },
    core::{Severity, TargetKind},
    render_human_report,
};
use camino::Utf8PathBuf;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use std::env;
use std::ffi::OsString;
use std::fmt::Write;
use std::process::ExitCode;

const EXIT_VIOLATIONS: u8 = 1;
const EXIT_CONFIGURATION: u8 = 2;
const EXIT_ANALYSIS: u8 = 3;

#[derive(Debug, Parser)]
#[command(
    name = "cargo-bylaw",
    bin_name = "cargo bylaw",
    version,
    about = "Enforce architectural boundaries in Rust projects"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

impl From<OutputFormatConfig> for OutputFormat {
    fn from(value: OutputFormatConfig) -> Self {
        match value {
            OutputFormatConfig::Human => Self::Human,
            OutputFormatConfig::Json => Self::Json,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the configured architecture rules.
    Check(CheckArgs),
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Path to bylaw.toml.
    #[arg(long)]
    config: Option<Utf8PathBuf>,

    /// Path to Cargo.toml.
    #[arg(long)]
    manifest_path: Option<Utf8PathBuf>,

    /// Analyze only the named workspace package. Repeat for multiple packages.
    #[arg(short = 'p', long = "package", action = ArgAction::Append)]
    packages: Vec<String>,

    /// Space- or comma-separated Cargo features.
    #[arg(long, value_delimiter = ',')]
    features: Vec<String>,

    /// Activate all available Cargo features.
    #[arg(long, conflicts_with = "no_default_features")]
    all_features: bool,

    /// Do not activate Cargo default features.
    #[arg(long, conflicts_with = "all_features")]
    no_default_features: bool,

    /// Analyze for the target triple.
    #[arg(long)]
    target: Option<String>,

    /// Replace configured target kinds.
    #[arg(long = "target-kind", value_enum, action = ArgAction::Append)]
    target_kinds: Vec<TargetKindArg>,

    /// Continue with warnings when semantic analysis is incomplete.
    #[arg(long)]
    allow_incomplete: bool,

    /// Output format.
    #[arg(long, value_enum)]
    format: Option<OutputFormat>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum TargetKindArg {
    Library,
    Binary,
    Test,
    Example,
    Bench,
    BuildScript,
    ProcMacro,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    #[default]
    Human,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse_from(normalized_args());
    let explicit_json = matches!(
        &cli.command,
        Command::Check(args) if args.format == Some(OutputFormat::Json)
    );
    match run(cli) {
        Ok(code) => code,
        Err(failure) => {
            if failure.json || explicit_json {
                eprintln!("{}", json_error(failure.kind, &failure.message));
            } else {
                eprintln!("{}: {}", failure.kind, failure.message);
            }
            ExitCode::from(failure.code)
        }
    }
}

fn normalized_args() -> Vec<OsString> {
    normalize_args(env::args_os().collect())
}

fn normalize_args(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.get(1).is_some_and(|argument| argument == "bylaw") {
        args.remove(1);
    }
    args
}

fn run(cli: Cli) -> Result<ExitCode, Failure> {
    match cli.command {
        Command::Check(args) => check(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_cargo_external_subcommand_arguments() {
        let args = ["cargo-bylaw", "bylaw", "check"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let cli = Cli::try_parse_from(normalize_args(args)).unwrap();
        assert!(matches!(cli.command, Command::Check(_)));
    }

    #[test]
    fn accepts_direct_binary_arguments() {
        let args = ["cargo-bylaw", "check"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let cli = Cli::try_parse_from(normalize_args(args)).unwrap();
        assert!(matches!(cli.command, Command::Check(_)));
    }

    #[test]
    fn cargo_feature_lists_accept_spaces_and_commas() {
        assert_eq!(
            normalize_features(&["serde,postgres tracing".to_owned()]),
            ["serde", "postgres", "tracing"]
        );
    }
}

fn check(args: CheckArgs) -> Result<ExitCode, Failure> {
    let config_path = args
        .config
        .clone()
        .map(Ok)
        .unwrap_or_else(discover_config)
        .map_err(Failure::configuration)?;
    let config = Config::load(&config_path)
        .map_err(|error| Failure::configuration(error.render()).with_format(args.format))?;
    let format = args
        .format
        .unwrap_or_else(|| OutputFormat::from(config.output.format));
    let compiled = config
        .compile()
        .map_err(|error| Failure::configuration(error.render()).with_format(Some(format)))?;
    let analysis_options = merge_analysis_options(&compiled.analysis, &args, &config_path)
        .map_err(|failure| failure.with_format(Some(format)))?;
    let graph = analyze_workspace(&analysis_options)
        .map_err(|error| Failure::analysis(error).with_format(Some(format)))?;
    let report = Rules::from_specs(compiled.rules)
        .check(&graph)
        .map_err(|error| Failure::configuration(error).with_format(Some(format)))?
        .into_report();

    match format {
        OutputFormat::Human => println!("{}", render_human_report(&report)),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&json_envelope(
                "report",
                serde_json::to_value(&report).map_err(Failure::analysis)?,
            ))
            .map_err(Failure::analysis)?
        ),
    }

    if report
        .analysis_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        Ok(ExitCode::from(EXIT_ANALYSIS))
    } else if report.is_success() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(EXIT_VIOLATIONS))
    }
}

fn json_error(kind: &str, message: &str) -> serde_json::Value {
    let mut error = serde_json::Map::new();
    error.insert(
        "kind".to_owned(),
        serde_json::Value::String(kind.to_owned()),
    );
    error.insert(
        "message".to_owned(),
        serde_json::Value::String(message.to_owned()),
    );
    json_envelope("error", serde_json::Value::Object(error))
}

fn json_envelope(key: &str, value: serde_json::Value) -> serde_json::Value {
    let mut envelope = serde_json::Map::new();
    envelope.insert(
        "version".to_owned(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    );
    envelope.insert(key.to_owned(), value);
    serde_json::Value::Object(envelope)
}

fn discover_config() -> Result<Utf8PathBuf, String> {
    let current_dir = env::current_dir().map_err(|error| error.to_string())?;
    let current_dir = Utf8PathBuf::from_path_buf(current_dir)
        .map_err(|path| format!("current directory is not valid UTF-8: {}", path.display()))?;
    for directory in current_dir.ancestors() {
        let candidate = directory.join("bylaw.toml");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err("could not find `bylaw.toml`; pass `--config` explicitly".to_owned())
}

fn merge_analysis_options(
    config: &AnalysisConfig,
    args: &CheckArgs,
    config_path: &Utf8PathBuf,
) -> Result<AnalysisOptions, Failure> {
    let config_directory = config_path
        .parent()
        .ok_or_else(|| Failure::configuration("configuration path has no parent directory"))?;
    let manifest_path = if let Some(path) = args.manifest_path.clone() {
        if path.is_absolute() {
            path
        } else {
            current_directory()?.join(path)
        }
    } else {
        config
            .manifest_path
            .clone()
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    config_directory.join(path)
                }
            })
            .unwrap_or_else(|| config_directory.join("Cargo.toml"))
    };

    let packages = if args.packages.is_empty() {
        config.packages.clone()
    } else {
        args.packages.clone()
    };
    let features = if args.features.is_empty() {
        normalize_features(&config.features)
    } else {
        normalize_features(&args.features)
    };
    let all_features = args.all_features || (!args.no_default_features && config.all_features);
    let no_default_features =
        args.no_default_features || (!args.all_features && config.no_default_features);
    if all_features && no_default_features {
        return Err(Failure::configuration(
            "`all_features` and `no_default_features` cannot both be enabled",
        ));
    }
    let target_kinds = if args.target_kinds.is_empty() {
        config.target_kinds.iter().map(config_target_kind).collect()
    } else {
        args.target_kinds
            .iter()
            .copied()
            .map(cli_target_kind)
            .collect()
    };
    let incomplete = if args.allow_incomplete || config.incomplete == ConfigIncompletePolicy::Allow
    {
        AnalyzerIncompletePolicy::Allow
    } else {
        AnalyzerIncompletePolicy::Deny
    };

    Ok(AnalysisOptions {
        manifest_path,
        selected_package_names: packages,
        features,
        all_features,
        no_default_features,
        target_triple: args.target.clone().or_else(|| config.target.clone()),
        included_target_kinds: target_kinds,
        enable_proc_macros: config.proc_macros,
        enable_build_scripts: config.build_scripts,
        incomplete_policy: incomplete,
    })
}

fn current_directory() -> Result<Utf8PathBuf, Failure> {
    let current_dir = env::current_dir().map_err(Failure::configuration)?;
    Utf8PathBuf::from_path_buf(current_dir).map_err(|path| {
        Failure::configuration(format!(
            "current directory is not valid UTF-8: {}",
            path.display()
        ))
    })
}

fn normalize_features(features: &[String]) -> Vec<String> {
    features
        .iter()
        .flat_map(|features| {
            features
                .split(|character: char| character == ',' || character.is_whitespace())
                .filter(|feature| !feature.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn config_target_kind(kind: &TargetKindConfig) -> TargetKind {
    match kind {
        TargetKindConfig::Library => TargetKind::Library,
        TargetKindConfig::Binary => TargetKind::Binary,
        TargetKindConfig::Test => TargetKind::Test,
        TargetKindConfig::Example => TargetKind::Example,
        TargetKindConfig::Bench => TargetKind::Bench,
        TargetKindConfig::BuildScript => TargetKind::BuildScript,
        TargetKindConfig::ProcMacro => TargetKind::ProcMacro,
    }
}

fn cli_target_kind(kind: TargetKindArg) -> TargetKind {
    match kind {
        TargetKindArg::Library => TargetKind::Library,
        TargetKindArg::Binary => TargetKind::Binary,
        TargetKindArg::Test => TargetKind::Test,
        TargetKindArg::Example => TargetKind::Example,
        TargetKindArg::Bench => TargetKind::Bench,
        TargetKindArg::BuildScript => TargetKind::BuildScript,
        TargetKindArg::ProcMacro => TargetKind::ProcMacro,
    }
}

#[derive(Debug)]
struct Failure {
    code: u8,
    kind: &'static str,
    message: String,
    json: bool,
}

impl Failure {
    fn configuration(error: impl std::fmt::Display) -> Self {
        Self::new(EXIT_CONFIGURATION, "configuration", error)
    }

    fn analysis(error: impl std::fmt::Display) -> Self {
        Self::new(EXIT_ANALYSIS, "analysis", error)
    }

    fn new(code: u8, kind: &'static str, error: impl std::fmt::Display) -> Self {
        let mut message = String::new();
        let _ = write!(message, "{error}");
        Self {
            code,
            kind,
            message,
            json: false,
        }
    }

    fn with_format(mut self, format: Option<OutputFormat>) -> Self {
        self.json = format == Some(OutputFormat::Json);
        self
    }
}
