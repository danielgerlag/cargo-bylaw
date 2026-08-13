use bylaw::analyzer::{AnalysisOptions, analyze_workspace};
use bylaw::config::Config;
use bylaw::core::CycleGrouping;
use bylaw::prelude::*;
use std::error::Error;
use std::sync::OnceLock;

fn fixture_path(name: &str, file: &str) -> String {
    format!(
        "{}/../../fixtures/{name}/{file}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn load_fixture(name: &str) -> ArchitectureGraph {
    analyze_workspace(&AnalysisOptions {
        manifest_path: fixture_path(name, "Cargo.toml").into(),
        ..AnalysisOptions::default()
    })
    .unwrap()
}

fn analyze_fixture(name: &str) -> &'static ArchitectureGraph {
    static VALID: OnceLock<ArchitectureGraph> = OnceLock::new();
    static INVALID: OnceLock<ArchitectureGraph> = OnceLock::new();
    static DECLARED: OnceLock<ArchitectureGraph> = OnceLock::new();
    match name {
        "model-boundaries-modules-valid" => VALID.get_or_init(|| load_fixture(name)),
        "model-boundaries-modules-invalid" => INVALID.get_or_init(|| load_fixture(name)),
        "declared-dependency" => DECLARED.get_or_init(|| load_fixture(name)),
        _ => panic!("unknown fixture `{name}`"),
    }
}

fn module_rules(crate_name: &str) -> Rules {
    let domain = modules([format!("{crate_name}::domain::**")]);
    let persistence = modules([format!("{crate_name}::persistence::**")]);
    let contract_model = modules([format!("{crate_name}::contract::**")]);
    let api = modules([format!("{crate_name}::api::**")]);
    let models = domain
        .clone()
        .or(persistence.clone())
        .or(contract_model.clone())
        .or(api.clone());

    rules()
        .forbid_dependencies(
            "domain-is-internal",
            domain,
            persistence
                .clone()
                .or(contract_model.clone())
                .or(api.clone()),
        )
        .actual_dependencies()
        .forbid_dependencies(
            "persistence-does-not-use-contract",
            persistence.clone(),
            contract_model.or(api.clone()),
        )
        .actual_dependencies()
        .forbid_dependencies("api-does-not-use-persistence", api, persistence)
        .actual_dependencies()
        .no_cycles("model-modules-are-acyclic", models, CycleGrouping::Modules)
        .actual_dependencies()
}

#[test]
fn valid_module_boundaries_pass() -> Result<(), Box<dyn Error>> {
    let graph = analyze_fixture("model-boundaries-modules-valid");
    let report = module_rules("model_boundaries_modules_valid")
        .check(graph)?
        .into_report();

    assert!(
        report.is_success(),
        "{}",
        bylaw::render_human_report(&report)
    );
    Ok(())
}

#[test]
fn invalid_module_boundaries_report_edges_and_cycle() -> Result<(), Box<dyn Error>> {
    let graph = analyze_fixture("model-boundaries-modules-invalid");
    let report = module_rules("model_boundaries_modules_invalid")
        .check(graph)?
        .into_report();

    assert!(
        !report.is_success(),
        "imported components:\n{:#?}\nimported edges:\n{:#?}",
        graph.components().collect::<Vec<_>>(),
        graph.edges().collect::<Vec<_>>()
    );
    let violated_ids = report
        .rule_results
        .iter()
        .filter(|result| !result.violations.is_empty())
        .map(|result| result.rule_id.as_str())
        .collect::<Vec<_>>();
    assert!(violated_ids.contains(&"domain-is-internal"));
    assert!(violated_ids.contains(&"persistence-does-not-use-contract"));
    assert!(violated_ids.contains(&"api-does-not-use-persistence"));
    assert!(violated_ids.contains(&"model-modules-are-acyclic"));
    assert!(
        report
            .violations()
            .flat_map(|violation| &violation.evidence)
            .any(|evidence| evidence.span.is_some())
    );
    Ok(())
}

#[test]
fn toml_and_dsl_rules_produce_equivalent_results() -> Result<(), Box<dyn Error>> {
    let fixture = "model-boundaries-modules-invalid";
    let graph = analyze_fixture(fixture);
    let dsl = module_rules("model_boundaries_modules_invalid")
        .check(graph)?
        .into_report();
    let config = Config::load(fixture_path(fixture, "bylaw.toml"))?.compile()?;
    let configured = Rules::from_specs(config.rules).check(graph)?.into_report();

    let summarize = |report: &bylaw::core::EvaluationReport| {
        report
            .rule_results
            .iter()
            .map(|result| (result.rule_id.clone(), result.violations.len()))
            .collect::<Vec<_>>()
    };
    assert_eq!(summarize(&dsl), summarize(&configured));
    Ok(())
}

#[test]
fn declared_and_actual_dependency_scopes_are_distinct() -> Result<(), Box<dyn Error>> {
    let graph = analyze_fixture("declared-dependency");
    let rule = || {
        rules().forbid_dependencies(
            "domain-does-not-use-persistence",
            packages(["declared-domain"]),
            packages(["declared-persistence"]),
        )
    };

    let actual = rule().actual_dependencies().check(graph)?.into_report();
    let declared = rule().declared_dependencies().check(graph)?.into_report();

    assert!(actual.is_success());
    assert!(!declared.is_success());
    assert_eq!(declared.violations().count(), 1);
    Ok(())
}
