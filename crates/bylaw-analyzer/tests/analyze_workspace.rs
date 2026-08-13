use bylaw_analyzer::{AnalysisOptions, IncompleteAnalysisPolicy, analyze_workspace};
use bylaw_core::{
    ArchitectureGraph, Component, ComponentId, DependencyKind, DependencyScope, TargetKind,
};
use camino::Utf8PathBuf;
use indexmap::IndexSet;

fn fixture_manifest(name: &str) -> Utf8PathBuf {
    Utf8PathBuf::from(format!(
        "{}/../../fixtures/{name}/Cargo.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn target_kinds(values: impl IntoIterator<Item = TargetKind>) -> IndexSet<TargetKind> {
    values.into_iter().collect()
}

fn find_crate_id(
    graph: &ArchitectureGraph,
    package_name: &str,
    target_kind: TargetKind,
) -> bylaw_core::CrateId {
    graph
        .components()
        .find_map(|component| match component {
            Component::Crate(node)
                if node.package_name == package_name && node.target_kind == target_kind =>
            {
                Some(node.id.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("crate {package_name:?} {target_kind:?} not found"))
}

fn find_module_id(
    graph: &ArchitectureGraph,
    crate_id: &bylaw_core::CrateId,
    path: &str,
) -> bylaw_core::ModuleId {
    graph
        .components()
        .find_map(|component| match component {
            Component::Module(node) if &node.crate_id == crate_id && node.path == path => {
                Some(node.id.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("module {crate_id}::{path} not found"))
}

fn find_workspace_crate_component(
    graph: &ArchitectureGraph,
    package_name: &str,
    target_kind: TargetKind,
) -> ComponentId {
    ComponentId::Crate(find_crate_id(graph, package_name, target_kind))
}

fn find_external_component_id(graph: &ArchitectureGraph, package_name: &str) -> ComponentId {
    graph
        .components()
        .find_map(|component| match component {
            Component::ExternalCrate(node) if node.package_name == package_name => {
                Some(ComponentId::ExternalCrate(node.id.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("external crate {package_name:?} not found"))
}

fn has_toolchain_actual_edge(graph: &ArchitectureGraph, origin: &ComponentId) -> bool {
    graph.edges().any(|edge| {
        &edge.origin == origin
            && edge.scope == DependencyScope::Actual
            && matches!(
                graph.component(&edge.target),
                Some(Component::ExternalCrate(node)) if node.toolchain
            )
    })
}

fn edge_kinds(
    graph: &ArchitectureGraph,
    origin: &ComponentId,
    target: &ComponentId,
    scope: DependencyScope,
) -> Vec<DependencyKind> {
    graph
        .edges()
        .filter(|edge| &edge.origin == origin && &edge.target == target && edge.scope == scope)
        .flat_map(|edge| edge.evidence.iter().map(|evidence| evidence.kind.clone()))
        .collect()
}

fn has_evidence_at_line(
    graph: &ArchitectureGraph,
    origin: &ComponentId,
    target: &ComponentId,
    scope: DependencyScope,
    kind: DependencyKind,
    line: u32,
) -> bool {
    graph.edges().any(|edge| {
        &edge.origin == origin
            && &edge.target == target
            && edge.scope == scope
            && edge.evidence.iter().any(|evidence| {
                evidence.kind == kind
                    && evidence
                        .span
                        .as_ref()
                        .is_some_and(|span| span.start.line == line)
            })
    })
}

#[test]
fn resolves_aliases_reexports_fq_paths_external_crates_and_macro_rules() {
    let mut options = AnalysisOptions::new(fixture_manifest("analyzer-main"));
    options.selected_package_names = vec!["app".to_owned()];
    options.included_target_kinds = target_kinds([TargetKind::Binary]);

    let graph = analyze_workspace(&options).expect("analysis should succeed");
    assert!(
        graph.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        graph.diagnostics()
    );

    let app_crate = find_crate_id(&graph, "app", TargetKind::Binary);
    let app_root = ComponentId::Module(find_module_id(&graph, &app_crate, "app"));
    let helper_nested = ComponentId::Module(find_module_id(
        &graph,
        &find_crate_id(&graph, "helper", TargetKind::Library),
        "helper::nested",
    ));
    let itoa = find_external_component_id(&graph, "itoa");

    assert!(
        edge_kinds(&graph, &app_root, &helper_nested, DependencyScope::Actual)
            .contains(&DependencyKind::Use)
    );
    assert!(has_evidence_at_line(
        &graph,
        &app_root,
        &helper_nested,
        DependencyScope::Actual,
        DependencyKind::ReExport,
        2,
    ));
    assert!(has_evidence_at_line(
        &graph,
        &app_root,
        &helper_nested,
        DependencyScope::Actual,
        DependencyKind::Path,
        14,
    ));
    assert!(has_evidence_at_line(
        &graph,
        &app_root,
        &helper_nested,
        DependencyScope::Actual,
        DependencyKind::Call,
        15,
    ));
    assert!(has_evidence_at_line(
        &graph,
        &app_root,
        &helper_nested,
        DependencyScope::Actual,
        DependencyKind::Call,
        16,
    ));
    assert!(
        edge_kinds(&graph, &app_root, &itoa, DependencyScope::Actual)
            .contains(&DependencyKind::Call)
    );
    assert!(
        has_toolchain_actual_edge(&graph, &app_root),
        "expected a toolchain dependency edge"
    );

    assert!(
        graph.components().all(|component| match component {
            Component::Module(node) => node.path != "helper::hidden_cfg",
            _ => true,
        }),
        "cfg-disabled helper module should be absent"
    );
}

#[test]
fn records_build_and_dev_declared_dependencies() {
    let mut options = AnalysisOptions::new(fixture_manifest("analyzer-main"));
    options.selected_package_names = vec!["app".to_owned()];
    options.included_target_kinds = target_kinds([TargetKind::BuildScript, TargetKind::Test]);

    let graph = analyze_workspace(&options).expect("analysis should succeed");
    assert!(
        graph.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        graph.diagnostics()
    );

    let build_crate = find_workspace_crate_component(&graph, "app", TargetKind::BuildScript);
    let test_crate = find_workspace_crate_component(&graph, "app", TargetKind::Test);
    let build_helper = find_workspace_crate_component(&graph, "build-helper", TargetKind::Library);
    let dev_helper = find_workspace_crate_component(&graph, "dev-helper", TargetKind::Library);

    assert!(
        edge_kinds(
            &graph,
            &build_crate,
            &build_helper,
            DependencyScope::Declared
        )
        .contains(&DependencyKind::CargoBuild)
    );
    assert!(
        edge_kinds(&graph, &test_crate, &dev_helper, DependencyScope::Declared)
            .contains(&DependencyKind::CargoDev)
    );
}

#[test]
fn enables_feature_gated_modules_when_features_are_requested() {
    let mut options = AnalysisOptions::new(fixture_manifest("analyzer-features"));
    options.selected_package_names = vec!["app".to_owned()];
    options.features = vec!["extra".to_owned()];
    options.included_target_kinds = target_kinds([TargetKind::Binary]);

    let graph = analyze_workspace(&options).expect("feature-enabled analysis should succeed");
    assert!(
        graph.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        graph.diagnostics()
    );

    let app_crate = find_crate_id(&graph, "app", TargetKind::Binary);
    let app_root = ComponentId::Module(find_module_id(&graph, &app_crate, "app"));
    let helper_extra = ComponentId::Module(find_module_id(
        &graph,
        &find_crate_id(&graph, "helper", TargetKind::Library),
        "helper::extra_mod",
    ));

    assert!(
        edge_kinds(&graph, &app_root, &helper_extra, DependencyScope::Actual)
            .contains(&DependencyKind::Call)
            || edge_kinds(&graph, &app_root, &helper_extra, DependencyScope::Actual)
                .contains(&DependencyKind::Path)
    );
}

#[test]
fn proc_macro_support_smoke_test_reports_or_resolves_expansion() {
    let mut options = AnalysisOptions::new(fixture_manifest("analyzer-proc-macro"));
    options.selected_package_names = vec!["app".to_owned()];
    options.included_target_kinds = target_kinds([TargetKind::Binary]);
    options.enable_proc_macros = true;
    options.incomplete_policy = IncompleteAnalysisPolicy::Allow;

    let graph = analyze_workspace(&options).expect("proc-macro analysis should not hard fail");
    let proc_macro_unavailable = graph.diagnostics().iter().any(|diagnostic| {
        matches!(
            diagnostic.code.as_str(),
            "analyzer.proc-macro-server-unavailable"
                | "analyzer.unavailable-expansion"
                | "analyzer.unresolved-macro"
        )
    });

    if proc_macro_unavailable {
        return;
    }

    let app_crate = find_crate_id(&graph, "app", TargetKind::Binary);
    let app_root = ComponentId::Module(find_module_id(&graph, &app_crate, "app"));
    let dep_root = ComponentId::Module(find_module_id(
        &graph,
        &find_crate_id(&graph, "dep-lib", TargetKind::Library),
        "dep_lib",
    ));

    assert!(
        !edge_kinds(&graph, &app_root, &dep_root, DependencyScope::Actual).is_empty(),
        "expected proc-macro expansion to resolve to dep-lib when proc macros are available"
    );
}
