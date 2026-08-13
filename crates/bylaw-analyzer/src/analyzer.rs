use crate::{AnalysisOptions, AnalyzerError, IncompleteAnalysisPolicy};
use bylaw_core::{
    AnalysisContext, AnalysisDiagnostic, ArchitectureGraph, Component, ComponentId, CrateId,
    CrateNode, DependencyEvidence, DependencyKind, DependencyScope, ExternalCrateId,
    ExternalCrateNode, GraphBuilder, ModuleId, ModuleNode, Package, PackageId, SourcePosition,
    SourceSpan, TargetKind,
};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{
    CargoOpt, DependencyKind as CargoDependencyKind, Metadata, MetadataCommand, Node,
    Package as CargoPackage, PackageId as CargoPackageId, Target as CargoTarget,
    TargetKind as CargoTargetKind,
};
use indexmap::IndexSet;
use ra_ap_hir::{
    self as hir, CfgExpr, CfgOptions, Crate as HirCrate, ModuleDef, PathResolution, Semantics,
    attach_db,
};
use ra_ap_ide::RootDatabase;
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::{CargoConfig, CargoFeatures, RustLibSource};
use ra_ap_syntax::{
    AstNode, SyntaxKind, SyntaxNode, TextRange, ast, ast::HasAttrs, ast::HasVisibility,
};
use ra_ap_vfs::{FileId, Vfs};
use std::collections::{HashMap, HashSet};
use std::fs;

pub fn analyze_workspace(options: &AnalysisOptions) -> Result<ArchitectureGraph, AnalyzerError> {
    let resolved = ResolvedOptions::from_options(options)?;
    let metadata = load_metadata(&resolved)?;
    let mut analyzer = Analyzer::new(resolved, metadata)?;
    analyzer.analyze()
}

#[derive(Clone, Debug)]
struct ResolvedOptions {
    manifest_path: Utf8PathBuf,
    selected_package_names: IndexSet<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target_triple: Option<String>,
    included_target_kinds: IndexSet<TargetKind>,
    enable_proc_macros: bool,
    enable_build_scripts: bool,
    incomplete_policy: IncompleteAnalysisPolicy,
}

impl ResolvedOptions {
    fn from_options(options: &AnalysisOptions) -> Result<Self, AnalyzerError> {
        if options.included_target_kinds.is_empty() {
            return Err(AnalyzerError::InvalidOptions(
                "included_target_kinds cannot be empty".to_owned(),
            ));
        }
        if options.all_features && (options.no_default_features || !options.features.is_empty()) {
            return Err(AnalyzerError::InvalidOptions(
                "all_features cannot be combined with features or no_default_features".to_owned(),
            ));
        }

        let manifest_path = normalize_manifest_path(&options.manifest_path)?;
        Ok(Self {
            manifest_path,
            selected_package_names: options.selected_package_names.iter().cloned().collect(),
            features: options.features.clone(),
            all_features: options.all_features,
            no_default_features: options.no_default_features,
            target_triple: options.target_triple.clone(),
            included_target_kinds: options.included_target_kinds.clone(),
            enable_proc_macros: options.enable_proc_macros,
            enable_build_scripts: options.enable_build_scripts,
            incomplete_policy: options.incomplete_policy,
        })
    }
}

struct Analyzer {
    options: ResolvedOptions,
    metadata: Metadata,
    workspace_root: Utf8PathBuf,
    graph: GraphBuilder,
    packages_by_id: HashMap<CargoPackageId, CargoPackage>,
    resolve_nodes_by_id: HashMap<CargoPackageId, Node>,
    workspace_member_ids: HashSet<CargoPackageId>,
    selected_member_ids: HashSet<CargoPackageId>,
    package_component_ids: HashMap<CargoPackageId, PackageId>,
    workspace_targets: Vec<WorkspaceTarget>,
    workspace_targets_by_src_path: HashMap<Utf8PathBuf, Vec<usize>>,
    dependency_target_by_package: HashMap<CargoPackageId, ComponentId>,
    external_target_by_src_path: HashMap<Utf8PathBuf, CargoPackageId>,
    external_component_ids: HashMap<CargoPackageId, ExternalCrateId>,
    toolchain_component_ids: HashMap<String, ExternalCrateId>,
    line_cache: LineCache,
}

impl Analyzer {
    fn new(options: ResolvedOptions, metadata: Metadata) -> Result<Self, AnalyzerError> {
        let workspace_root = metadata.workspace_root.clone();
        let packages_by_id = metadata
            .packages
            .iter()
            .cloned()
            .map(|package| (package.id.clone(), package))
            .collect::<HashMap<_, _>>();
        let resolve =
            metadata
                .resolve
                .clone()
                .ok_or_else(|| AnalyzerError::MissingResolveGraph {
                    manifest_path: options.manifest_path.clone(),
                })?;
        let resolve_nodes_by_id = resolve
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect::<HashMap<_, _>>();
        let workspace_member_ids = metadata
            .workspace_members
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let selected_member_ids =
            resolve_selected_packages(&options, &metadata, &workspace_member_ids)?;
        let graph = GraphBuilder::new(build_context(&options));

        Ok(Self {
            options,
            metadata,
            workspace_root,
            graph,
            packages_by_id,
            resolve_nodes_by_id,
            workspace_member_ids,
            selected_member_ids,
            package_component_ids: HashMap::new(),
            workspace_targets: Vec::new(),
            workspace_targets_by_src_path: HashMap::new(),
            dependency_target_by_package: HashMap::new(),
            external_target_by_src_path: HashMap::new(),
            external_component_ids: HashMap::new(),
            toolchain_component_ids: HashMap::new(),
            line_cache: LineCache::default(),
        })
    }

    fn analyze(&mut self) -> Result<ArchitectureGraph, AnalyzerError> {
        self.add_workspace_packages()?;
        self.add_workspace_crates()?;
        self.index_external_targets();

        let mut session = RaSession::load(&self.options)?;
        if self.options.enable_proc_macros && !session.proc_macros_available {
            self.graph.add_diagnostic(
                self.incomplete_diagnostic(
                    "analyzer.proc-macro-server-unavailable",
                    "proc-macro support was requested but rust-analyzer could not start a proc-macro server",
                )
                .with_help(
                    "install rust-analyzer-proc-macro-srv/rust-src for the active toolchain or rerun with enable_proc_macros = false",
                ),
            );
        }

        session.bind_workspace_components(self)?;
        self.add_declared_dependencies()?;
        self.add_actual_dependencies(&session)?;
        Ok(std::mem::take(&mut self.graph).finish())
    }

    fn add_workspace_packages(&mut self) -> Result<(), AnalyzerError> {
        let mut workspace_packages = self
            .metadata
            .packages
            .iter()
            .filter(|package| self.workspace_member_ids.contains(&package.id))
            .cloned()
            .collect::<Vec<_>>();
        workspace_packages.sort_by(|left, right| left.manifest_path.cmp(&right.manifest_path));

        for package in workspace_packages {
            let id = stable_workspace_package_id(&package, &self.workspace_root);
            self.package_component_ids
                .insert(package.id.clone(), id.clone());
            self.graph.add_package(Package {
                id,
                name: package.name.to_string(),
                version: Some(package.version.to_string()),
                manifest_path: package.manifest_path.clone(),
            })?;
        }
        Ok(())
    }

    fn add_workspace_crates(&mut self) -> Result<(), AnalyzerError> {
        let mut dependency_preferences = HashMap::<CargoPackageId, u8>::new();
        let mut workspace_packages = self
            .metadata
            .packages
            .iter()
            .filter(|package| self.workspace_member_ids.contains(&package.id))
            .cloned()
            .collect::<Vec<_>>();
        workspace_packages.sort_by(|left, right| left.manifest_path.cmp(&right.manifest_path));

        for package in workspace_packages {
            let package_component_id = self
                .package_component_ids
                .get(&package.id)
                .cloned()
                .expect("workspace package ids are registered");

            let mut targets = package.targets.clone();
            targets.sort_by(|left, right| {
                left.src_path
                    .cmp(&right.src_path)
                    .then(left.name.cmp(&right.name))
            });
            for target in targets {
                let target_kind = map_target_kind(&target);
                let crate_id =
                    stable_workspace_crate_id(&package_component_id, &target_kind, &target.name);
                let crate_name = normalize_crate_name(&target.name);
                let origin_selected = self.selected_member_ids.contains(&package.id)
                    && self.options.included_target_kinds.contains(&target_kind);

                self.graph.add_component(Component::Crate(CrateNode {
                    id: crate_id.clone(),
                    package_id: package_component_id.clone(),
                    package_name: package.name.to_string(),
                    crate_name: crate_name.clone(),
                    target_name: target.name.clone(),
                    target_kind: target_kind.clone(),
                    source_root: target.src_path.clone(),
                }))?;

                let entry = WorkspaceTarget {
                    cargo_package_id: package.id.clone(),
                    package_id: package_component_id.clone(),
                    package_name: package.name.to_string(),
                    crate_id: crate_id.clone(),
                    crate_name,
                    target_name: target.name.clone(),
                    target_kind: target_kind.clone(),
                    src_path: target.src_path.clone(),
                    required_features: target.required_features.clone(),
                    origin_selected,
                };
                self.workspace_targets_by_src_path
                    .entry(entry.src_path.clone())
                    .or_default()
                    .push(self.workspace_targets.len());
                self.workspace_targets.push(entry);

                let rank = dependency_target_rank(&target_kind);
                if rank > 0 {
                    let current = dependency_preferences
                        .get(&package.id)
                        .copied()
                        .unwrap_or_default();
                    if rank > current {
                        dependency_preferences.insert(package.id.clone(), rank);
                        self.dependency_target_by_package
                            .insert(package.id.clone(), ComponentId::Crate(crate_id));
                    }
                }
            }
        }
        Ok(())
    }

    fn index_external_targets(&mut self) {
        for package in self
            .metadata
            .packages
            .iter()
            .filter(|package| !self.workspace_member_ids.contains(&package.id))
        {
            if let Some(target) = primary_dependency_target(package) {
                self.external_target_by_src_path
                    .insert(target.src_path.clone(), package.id.clone());
            }
        }
    }

    fn add_declared_dependencies(&mut self) -> Result<(), AnalyzerError> {
        let origin_targets = self
            .workspace_targets
            .iter()
            .filter(|target| target.origin_selected)
            .cloned()
            .collect::<Vec<_>>();

        for origin in origin_targets {
            let origin_id = ComponentId::Crate(origin.crate_id.clone());
            let Some(dependencies) = self
                .resolve_nodes_by_id
                .get(&origin.cargo_package_id)
                .map(|node| node.deps.clone())
            else {
                continue;
            };

            for dependency in &dependencies {
                let dep_kind_infos = if dependency.dep_kinds.is_empty() {
                    vec![CargoDependencyKind::Normal]
                } else {
                    dependency.dep_kinds.iter().map(|info| info.kind).collect()
                };

                for dep_kind in dep_kind_infos {
                    let Some(edge_kind) =
                        declared_edge_kind_for_target(&origin.target_kind, dep_kind)
                    else {
                        continue;
                    };
                    let Some(target) = self.component_for_declared_dependency(&dependency.pkg)
                    else {
                        let diagnostic = self
                            .incomplete_diagnostic(
                                "analyzer.unmapped-declared-dependency",
                                format!(
                                    "declared dependency `{}` for `{}` could not be mapped to a graph component",
                                    dependency.name,
                                    origin.crate_id
                                ),
                            )
                            .with_help("ensure the dependency has a library or proc-macro target");
                        self.graph.add_diagnostic(diagnostic);
                        continue;
                    };

                    let mut evidence = DependencyEvidence::new(edge_kind);
                    let description =
                        declared_dependency_description(&origin, dependency, dep_kind);
                    if !description.is_empty() {
                        evidence = evidence.with_description(description);
                    }
                    self.graph.add_dependency(
                        origin_id.clone(),
                        target,
                        DependencyScope::Declared,
                        evidence,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn add_actual_dependencies(&mut self, session: &RaSession) -> Result<(), AnalyzerError> {
        attach_db(&session.db, || -> Result<(), AnalyzerError> {
            let sema = Semantics::new(&session.db);
            let origin_targets = self
                .workspace_targets
                .iter()
                .filter(|target| target.origin_selected)
                .cloned()
                .collect::<Vec<_>>();

            for target in origin_targets {
                let Some(file_ids) = session.crate_source_files.get(&target.crate_id) else {
                    let diagnostic = self
                        .incomplete_diagnostic(
                            "analyzer.skipped-target",
                            format!(
                                "selected target `{}` could not be loaded into rust-analyzer",
                                target.crate_id
                            ),
                        )
                        .with_help("verify the target compiles for the selected features and target triple");
                    self.graph.add_diagnostic(diagnostic);
                    continue;
                };

                let mut visited_expansions = HashSet::new();
                let Some(cfg_options) = session.crate_cfg_options.get(&target.crate_id) else {
                    let diagnostic = self.incomplete_diagnostic(
                        "analyzer.missing-cfg-options",
                        format!(
                            "selected target `{}` has no rust-analyzer cfg options",
                            target.crate_id
                        ),
                    );
                    self.graph.add_diagnostic(diagnostic);
                    continue;
                };
                for file_id in file_ids.iter().copied() {
                    let syntax = sema.parse_guess_edition(file_id).syntax().clone();
                    self.walk_syntax_tree(
                        session,
                        &sema,
                        &target,
                        cfg_options,
                        syntax,
                        &mut visited_expansions,
                    )?;
                }
            }
            Ok(())
        })
    }

    fn walk_syntax_tree(
        &mut self,
        session: &RaSession,
        sema: &Semantics<'_, RootDatabase>,
        target: &WorkspaceTarget,
        cfg_options: &CfgOptions,
        root: SyntaxNode,
        visited_expansions: &mut HashSet<hir::HirFileId>,
    ) -> Result<(), AnalyzerError> {
        for path in std::iter::once(root.clone())
            .chain(root.descendants())
            .filter_map(ast::Path::cast)
            .filter(is_terminal_path)
            .filter(|path| syntax_is_cfg_enabled(path.syntax(), cfg_options))
        {
            if is_macro_path(&path) {
                continue;
            }
            self.record_path_reference(session, sema, target, &path)?;
        }

        for macro_call in std::iter::once(root.clone())
            .chain(root.descendants())
            .filter_map(ast::MacroCall::cast)
            .filter(|macro_call| syntax_is_cfg_enabled(macro_call.syntax(), cfg_options))
        {
            self.record_macro_reference(session, sema, target, &macro_call)?;
            if let Some(expanded) = sema.expand_macro_call(&macro_call) {
                if expanded.value.kind() == SyntaxKind::ERROR
                    || expanded.value.text().to_string().trim().is_empty()
                {
                    let diagnostic = self
                        .incomplete_diagnostic(
                            "analyzer.unavailable-expansion",
                            format!(
                                "macro expansion for `{}` was empty or could not be parsed",
                                macro_call
                                    .path()
                                    .map(|path| path.syntax().text().to_string())
                                    .unwrap_or_else(|| "<unknown-macro>".to_owned())
                            ),
                        )
                        .with_help(
                            "proc-macro expansion may require a running proc-macro server, build-script output, or syntactically valid expanded tokens",
                        );
                    self.add_spanned_diagnostic(session, sema, diagnostic, macro_call.syntax())?;
                    continue;
                }
                if visited_expansions.insert(expanded.file_id) {
                    self.walk_syntax_tree(
                        session,
                        sema,
                        target,
                        cfg_options,
                        expanded.value,
                        visited_expansions,
                    )?;
                }
            } else {
                let diagnostic = self
                    .incomplete_diagnostic(
                        "analyzer.unavailable-expansion",
                        format!(
                            "macro expansion for `{}` was unavailable",
                            macro_call
                                .path()
                                .map(|path| path.syntax().text().to_string())
                                .unwrap_or_else(|| "<unknown-macro>".to_owned())
                        ),
                    )
                    .with_help(
                        "enable proc macros/build scripts or inspect rust-analyzer diagnostics for expansion failures",
                    );
                self.add_spanned_diagnostic(session, sema, diagnostic, macro_call.syntax())?;
            }
        }

        Ok(())
    }

    fn record_path_reference(
        &mut self,
        session: &RaSession,
        sema: &Semantics<'_, RootDatabase>,
        target: &WorkspaceTarget,
        path: &ast::Path,
    ) -> Result<(), AnalyzerError> {
        let Some(edge_kind) = classify_path(path) else {
            return Ok(());
        };
        let Some(origin) = self.origin_component_for_node(session, sema, target, path.syntax())
        else {
            let diagnostic = self.incomplete_diagnostic(
                "analyzer.unmapped-origin",
                format!(
                    "could not determine an owning module for path `{}`",
                    path.syntax().text()
                ),
            );
            self.add_spanned_diagnostic(session, sema, diagnostic, path.syntax())?;
            return Ok(());
        };

        let Some(resolution) = sema.resolve_path(path).or_else(|| {
            sema.scope(path.syntax())
                .and_then(|scope| scope.speculative_resolve(path))
        }) else {
            let diagnostic = self.incomplete_diagnostic(
                "analyzer.unresolved-path",
                format!("could not resolve path `{}`", path.syntax().text()),
            );
            self.add_spanned_diagnostic(session, sema, diagnostic, path.syntax())?;
            return Ok(());
        };

        let Some(target_component) = self.target_component_for_resolution(session, resolution)
        else {
            if !matches!(
                resolution,
                PathResolution::Local(_)
                    | PathResolution::TypeParam(_)
                    | PathResolution::ConstParam(_)
                    | PathResolution::SelfType(_)
                    | PathResolution::BuiltinAttr(_)
                    | PathResolution::ToolModule(_)
                    | PathResolution::DeriveHelper(_)
                    | PathResolution::Def(ModuleDef::BuiltinType(_))
            ) {
                let diagnostic = self.incomplete_diagnostic(
                    "analyzer.unmapped-resolution",
                    format!(
                        "resolved path `{}` but could not map it to a graph component",
                        path.syntax().text()
                    ),
                );
                self.add_spanned_diagnostic(session, sema, diagnostic, path.syntax())?;
            }
            return Ok(());
        };

        if origin == target_component {
            return Ok(());
        }

        let evidence = DependencyEvidence::new(edge_kind)
            .with_span(span_for_syntax(self, session, sema, path.syntax())?)
            .with_description(path.syntax().text().to_string());
        self.graph
            .add_dependency(origin, target_component, DependencyScope::Actual, evidence)?;
        Ok(())
    }

    fn record_macro_reference(
        &mut self,
        session: &RaSession,
        sema: &Semantics<'_, RootDatabase>,
        target: &WorkspaceTarget,
        macro_call: &ast::MacroCall,
    ) -> Result<(), AnalyzerError> {
        let Some(origin) =
            self.origin_component_for_node(session, sema, target, macro_call.syntax())
        else {
            let diagnostic = self.incomplete_diagnostic(
                "analyzer.unmapped-origin",
                format!(
                    "could not determine an owning module for macro `{}`",
                    macro_call
                        .path()
                        .map(|path| path.syntax().text().to_string())
                        .unwrap_or_else(|| "<unknown-macro>".to_owned())
                ),
            );
            self.add_spanned_diagnostic(session, sema, diagnostic, macro_call.syntax())?;
            return Ok(());
        };

        let Some(macro_def) = sema.resolve_macro_call(macro_call) else {
            let diagnostic = self.incomplete_diagnostic(
                "analyzer.unresolved-macro",
                format!(
                    "could not resolve macro `{}`",
                    macro_call
                        .path()
                        .map(|path| path.syntax().text().to_string())
                        .unwrap_or_else(|| "<unknown-macro>".to_owned())
                ),
            );
            self.add_spanned_diagnostic(session, sema, diagnostic, macro_call.syntax())?;
            return Ok(());
        };

        let Some(target_component) =
            self.target_component_for_module(session, macro_def.module(&session.db))
        else {
            let diagnostic = self.incomplete_diagnostic(
                "analyzer.unmapped-macro",
                format!(
                    "resolved macro `{}` but could not map it to a graph component",
                    macro_call
                        .path()
                        .map(|path| path.syntax().text().to_string())
                        .unwrap_or_else(|| "<unknown-macro>".to_owned())
                ),
            );
            self.add_spanned_diagnostic(session, sema, diagnostic, macro_call.syntax())?;
            return Ok(());
        };

        if origin == target_component {
            return Ok(());
        }

        let evidence = DependencyEvidence::new(DependencyKind::Macro)
            .with_span(span_for_syntax(self, session, sema, macro_call.syntax())?)
            .with_description(
                macro_call
                    .path()
                    .map(|path| path.syntax().text().to_string())
                    .unwrap_or_else(|| "<unknown-macro>".to_owned()),
            );
        self.graph
            .add_dependency(origin, target_component, DependencyScope::Actual, evidence)?;
        Ok(())
    }

    fn origin_component_for_node(
        &self,
        session: &RaSession,
        sema: &Semantics<'_, RootDatabase>,
        target: &WorkspaceTarget,
        node: &SyntaxNode,
    ) -> Option<ComponentId> {
        let scope = sema.scope(node)?;
        let module = scope.module().nearest_non_block_module(&session.db);
        session
            .module_component_ids
            .get(&module)
            .cloned()
            .or_else(|| session.ra_crate_component_ids.get(&scope.krate()).cloned())
            .or_else(|| Some(ComponentId::Crate(target.crate_id.clone())))
    }

    fn target_component_for_resolution(
        &self,
        session: &RaSession,
        resolution: PathResolution<'_>,
    ) -> Option<ComponentId> {
        match resolution {
            PathResolution::Def(ModuleDef::Module(module)) => {
                let module = module.nearest_non_block_module(&session.db);
                self.target_component_for_module(session, module)
            }
            PathResolution::Def(def) => {
                let module = def
                    .module(&session.db)?
                    .nearest_non_block_module(&session.db);
                self.target_component_for_module(session, module)
            }
            PathResolution::Local(_)
            | PathResolution::TypeParam(_)
            | PathResolution::ConstParam(_)
            | PathResolution::SelfType(_)
            | PathResolution::BuiltinAttr(_)
            | PathResolution::ToolModule(_)
            | PathResolution::DeriveHelper(_) => None,
        }
    }

    fn target_component_for_module(
        &self,
        session: &RaSession,
        module: hir::Module,
    ) -> Option<ComponentId> {
        session
            .module_component_ids
            .get(&module)
            .cloned()
            .or_else(|| {
                session
                    .ra_crate_component_ids
                    .get(&module.krate(&session.db))
                    .cloned()
            })
    }

    fn component_for_declared_dependency(
        &mut self,
        package_id: &CargoPackageId,
    ) -> Option<ComponentId> {
        if self.workspace_member_ids.contains(package_id) {
            return self.dependency_target_by_package.get(package_id).cloned();
        }
        self.ensure_external_component(package_id)
            .ok()
            .map(ComponentId::ExternalCrate)
    }

    fn ensure_external_component(
        &mut self,
        package_id: &CargoPackageId,
    ) -> Result<ExternalCrateId, AnalyzerError> {
        if let Some(id) = self.external_component_ids.get(package_id) {
            return Ok(id.clone());
        }
        let package = self
            .packages_by_id
            .get(package_id)
            .expect("external package must exist in metadata");
        let id = stable_external_crate_id(package, &self.workspace_root);
        let crate_name = external_crate_name(package);
        self.graph
            .add_component(Component::ExternalCrate(ExternalCrateNode {
                id: id.clone(),
                package_name: package.name.to_string(),
                crate_name,
                version: Some(package.version.to_string()),
                source: Some(external_source_key(package, &self.workspace_root)),
                toolchain: false,
            }))?;
        self.external_component_ids
            .insert(package_id.clone(), id.clone());
        Ok(id)
    }

    fn ensure_toolchain_component(
        &mut self,
        display_name: &str,
        version: Option<String>,
        source_root: &Utf8Path,
    ) -> Result<ExternalCrateId, AnalyzerError> {
        let key = format!(
            "{display_name}@{}",
            version.clone().unwrap_or_else(|| "unknown".to_owned())
        );
        if let Some(id) = self.toolchain_component_ids.get(&key) {
            return Ok(id.clone());
        }

        let id = ExternalCrateId::new(format!("toolchain::{key}"));
        self.graph
            .add_component(Component::ExternalCrate(ExternalCrateNode {
                id: id.clone(),
                package_name: display_name.to_owned(),
                crate_name: display_name.to_owned(),
                version,
                source: Some(source_root.to_string()),
                toolchain: true,
            }))?;
        self.toolchain_component_ids.insert(key, id.clone());
        Ok(id)
    }

    fn add_spanned_diagnostic(
        &mut self,
        session: &RaSession,
        sema: &Semantics<'_, RootDatabase>,
        diagnostic: AnalysisDiagnostic,
        node: &SyntaxNode,
    ) -> Result<(), AnalyzerError> {
        let diagnostic = with_syntax_span(self, session, sema, diagnostic, node)?;
        self.graph.add_diagnostic(diagnostic);
        Ok(())
    }

    fn incomplete_diagnostic(
        &self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> AnalysisDiagnostic {
        match self.options.incomplete_policy {
            IncompleteAnalysisPolicy::Deny => AnalysisDiagnostic::error(code, message),
            IncompleteAnalysisPolicy::Allow => AnalysisDiagnostic::warning(code, message),
        }
    }
}

#[derive(Clone, Debug)]
struct WorkspaceTarget {
    cargo_package_id: CargoPackageId,
    package_id: PackageId,
    package_name: String,
    crate_id: CrateId,
    crate_name: String,
    target_name: String,
    target_kind: TargetKind,
    src_path: Utf8PathBuf,
    required_features: Vec<String>,
    origin_selected: bool,
}

struct RaSession {
    db: RootDatabase,
    vfs: Vfs,
    proc_macros_available: bool,
    ra_crate_component_ids: HashMap<HirCrate, ComponentId>,
    module_component_ids: HashMap<hir::Module, ComponentId>,
    crate_source_files: HashMap<CrateId, IndexSet<FileId>>,
    crate_cfg_options: HashMap<CrateId, CfgOptions>,
}

impl RaSession {
    fn load(options: &ResolvedOptions) -> Result<Self, AnalyzerError> {
        let cargo_config = CargoConfig {
            all_targets: options.included_target_kinds.iter().any(|kind| {
                matches!(
                    kind,
                    TargetKind::Test | TargetKind::Example | TargetKind::Bench
                )
            }),
            features: cargo_features(options),
            target: options.target_triple.clone(),
            sysroot: Some(RustLibSource::Discover),
            set_test: options.included_target_kinds.contains(&TargetKind::Test),
            ..CargoConfig::default()
        };
        let load_config = LoadCargoConfig {
            load_out_dirs_from_check: options.enable_build_scripts,
            with_proc_macro_server: if options.enable_proc_macros {
                ProcMacroServerChoice::Sysroot
            } else {
                ProcMacroServerChoice::None
            },
            prefill_caches: false,
            num_worker_threads: 1,
            proc_macro_processes: 1,
        };
        let (db, vfs, proc_macro_client) = load_workspace_at(
            options.manifest_path.as_std_path(),
            &cargo_config,
            &load_config,
            &|_| {},
        )
        .map_err(|error| AnalyzerError::WorkspaceLoad {
            manifest_path: options.manifest_path.clone(),
            message: error.to_string(),
        })?;

        Ok(Self {
            db,
            vfs,
            proc_macros_available: proc_macro_client.is_some(),
            ra_crate_component_ids: HashMap::new(),
            module_component_ids: HashMap::new(),
            crate_source_files: HashMap::new(),
            crate_cfg_options: HashMap::new(),
        })
    }

    fn bind_workspace_components(&mut self, analyzer: &mut Analyzer) -> Result<(), AnalyzerError> {
        attach_db(&self.db, || -> Result<(), AnalyzerError> {
            let sema = Semantics::new(&self.db);
            let mut added_module_ids = HashSet::new();
            let mut bound_origin_targets = HashSet::new();
            let mut all_crates = HirCrate::all(&self.db);
            all_crates.sort_by_key(|krate| ra_crate_name(*krate, &self.db));

            for krate in all_crates {
                let root_path = file_path_for_id(&self.vfs, krate.root_file(&self.db))?;
                let krate_name = ra_crate_name(krate, &self.db);
                if let Some(target) =
                    analyzer.select_workspace_target_for_ra_crate(&root_path, &krate_name)
                {
                    let crate_component_id = ComponentId::Crate(target.crate_id.clone());
                    self.ra_crate_component_ids
                        .insert(krate, crate_component_id);
                    if target.origin_selected {
                        bound_origin_targets.insert(target.crate_id.clone());
                    }
                    self.crate_cfg_options
                        .entry(target.crate_id.clone())
                        .or_insert_with(|| krate.cfg(&self.db).clone());

                    for module in krate.modules(&self.db) {
                        let module = module.nearest_non_block_module(&self.db);
                        if !module.is_crate_root(&self.db) && module.name(&self.db).is_none() {
                            continue;
                        }

                        let path = module_path(module, &self.db, &target.crate_name);
                        let module_id = stable_module_id(&target.crate_id, &path);
                        let parent = module
                            .parent(&self.db)
                            .map(|parent| parent.nearest_non_block_module(&self.db))
                            .filter(|parent| {
                                !parent.is_crate_root(&self.db) || path != target.crate_name
                            })
                            .map(|parent| {
                                stable_module_id(
                                    &target.crate_id,
                                    &module_path(parent, &self.db, &target.crate_name),
                                )
                            });
                        let source_display = sema.diagnostics_display_range_for_range(
                            module.definition_source_range(&self.db),
                        );
                        let source_file = file_path_for_id(&self.vfs, source_display.file_id)?;

                        if added_module_ids.insert(module_id.clone()) {
                            analyzer.graph.add_component(Component::Module(ModuleNode {
                                id: module_id.clone(),
                                crate_id: target.crate_id.clone(),
                                package_id: target.package_id.clone(),
                                package_name: target.package_name.clone(),
                                crate_name: target.crate_name.clone(),
                                path: path.clone(),
                                parent,
                                source_file: source_file.clone(),
                            }))?;
                        }
                        self.module_component_ids
                            .entry(module)
                            .or_insert_with(|| ComponentId::Module(module_id));
                        self.crate_source_files
                            .entry(target.crate_id.clone())
                            .or_default()
                            .insert(source_display.file_id);
                    }
                    continue;
                }

                if let Some(package_id) = analyzer
                    .external_target_by_src_path
                    .get(&root_path)
                    .cloned()
                {
                    let external_id = analyzer.ensure_external_component(&package_id)?;
                    self.ra_crate_component_ids
                        .insert(krate, ComponentId::ExternalCrate(external_id));
                    continue;
                }

                let toolchain_id = analyzer.ensure_toolchain_component(
                    &ra_crate_name(krate, &self.db),
                    krate.version(&self.db),
                    &root_path,
                )?;
                self.ra_crate_component_ids
                    .insert(krate, ComponentId::ExternalCrate(toolchain_id));
            }

            for target in analyzer
                .workspace_targets
                .iter()
                .filter(|target| target.origin_selected)
            {
                if !bound_origin_targets.contains(&target.crate_id) {
                    let mut diagnostic = analyzer.incomplete_diagnostic(
                        "analyzer.skipped-target",
                        format!(
                            "selected target `{}` was not loaded into rust-analyzer",
                            target.crate_id
                        ),
                    );
                    if !target.required_features.is_empty() {
                        diagnostic = diagnostic.with_help(format!(
                            "enable required features [{}] or adjust included target kinds",
                            target.required_features.join(", ")
                        ));
                    }
                    analyzer.graph.add_diagnostic(diagnostic);
                }
            }

            Ok(())
        })
    }
}

#[derive(Default)]
struct LineCache {
    contents: HashMap<Utf8PathBuf, String>,
}

impl LineCache {
    fn source_span(
        &mut self,
        path: &Utf8Path,
        range: TextRange,
    ) -> Result<SourceSpan, AnalyzerError> {
        let text = if let Some(text) = self.contents.get(path) {
            text
        } else {
            let contents = fs::read_to_string(path).map_err(|source| AnalyzerError::ReadFile {
                path: path.to_path_buf(),
                source,
            })?;
            self.contents.insert(path.to_path_buf(), contents);
            self.contents.get(path).expect("cached contents must exist")
        };

        let start = offset_to_position(text, range.start().into());
        let end = offset_to_position(text, range.end().into());
        Ok(SourceSpan {
            path: path.to_path_buf(),
            start,
            end,
        })
    }
}

fn normalize_manifest_path(path: &Utf8Path) -> Result<Utf8PathBuf, AnalyzerError> {
    let manifest_path = if path.is_dir() {
        path.join("Cargo.toml")
    } else {
        path.to_path_buf()
    };

    if !manifest_path.exists() {
        return Err(AnalyzerError::ManifestPathDoesNotExist(manifest_path));
    }
    if manifest_path.file_name() != Some("Cargo.toml") {
        return Err(AnalyzerError::InvalidManifestPath(manifest_path));
    }
    Ok(manifest_path)
}

fn load_metadata(options: &ResolvedOptions) -> Result<Metadata, AnalyzerError> {
    let mut command = MetadataCommand::new();
    command.manifest_path(&options.manifest_path);
    match cargo_features(options) {
        CargoFeatures::All => {
            command.features(CargoOpt::AllFeatures);
        }
        CargoFeatures::Selected {
            ref features,
            no_default_features,
        } => {
            if no_default_features {
                command.features(CargoOpt::NoDefaultFeatures);
            }
            if !features.is_empty() {
                command.features(CargoOpt::SomeFeatures(features.clone()));
            }
        }
    }
    if let Some(target) = &options.target_triple {
        command.other_options(vec!["--filter-platform".to_owned(), target.clone()]);
    }

    command
        .exec()
        .map_err(|source| AnalyzerError::CargoMetadata {
            manifest_path: options.manifest_path.clone(),
            source,
        })
}

fn build_context(options: &ResolvedOptions) -> AnalysisContext {
    let mut features = options.features.clone();
    if options.all_features {
        features.push("*".to_owned());
    }
    if options.no_default_features {
        features.push("!default".to_owned());
    }

    AnalysisContext {
        manifest_path: Some(options.manifest_path.clone()),
        target_triple: options.target_triple.clone(),
        features,
        target_kinds: options.included_target_kinds.clone(),
    }
}

fn resolve_selected_packages(
    options: &ResolvedOptions,
    metadata: &Metadata,
    workspace_member_ids: &HashSet<CargoPackageId>,
) -> Result<HashSet<CargoPackageId>, AnalyzerError> {
    if options.selected_package_names.is_empty() {
        return Ok(metadata.workspace_members.iter().cloned().collect());
    }

    let mut package_names = HashMap::<String, Vec<CargoPackageId>>::new();
    for package in metadata
        .packages
        .iter()
        .filter(|package| workspace_member_ids.contains(&package.id))
    {
        package_names
            .entry(package.name.to_string())
            .or_default()
            .push(package.id.clone());
    }

    let mut selected = HashSet::new();
    let mut missing = Vec::new();
    for name in &options.selected_package_names {
        match package_names.get(name) {
            Some(ids) if ids.len() == 1 => {
                selected.insert(ids[0].clone());
            }
            Some(ids) => {
                return Err(AnalyzerError::InvalidOptions(format!(
                    "package selection `{name}` is ambiguous across package ids {ids:?}`"
                )));
            }
            None => missing.push(name.clone()),
        }
    }

    if missing.is_empty() {
        Ok(selected)
    } else {
        missing.sort();
        Err(AnalyzerError::UnknownPackages { packages: missing })
    }
}

fn cargo_features(options: &ResolvedOptions) -> CargoFeatures {
    if options.all_features {
        CargoFeatures::All
    } else {
        CargoFeatures::Selected {
            features: options.features.clone(),
            no_default_features: options.no_default_features,
        }
    }
}

fn map_target_kind(target: &CargoTarget) -> TargetKind {
    if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::ProcMacro))
    {
        TargetKind::ProcMacro
    } else if target.kind.iter().any(|kind| {
        matches!(
            kind,
            CargoTargetKind::Lib
                | CargoTargetKind::RLib
                | CargoTargetKind::DyLib
                | CargoTargetKind::CDyLib
                | CargoTargetKind::StaticLib
        )
    }) {
        TargetKind::Library
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::Bin))
    {
        TargetKind::Binary
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::Test))
    {
        TargetKind::Test
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::Example))
    {
        TargetKind::Example
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::Bench))
    {
        TargetKind::Bench
    } else if target
        .kind
        .iter()
        .any(|kind| matches!(kind, CargoTargetKind::CustomBuild))
    {
        TargetKind::BuildScript
    } else {
        TargetKind::Other(
            target
                .kind
                .iter()
                .map(cargo_target_kind_name)
                .collect::<Vec<_>>()
                .join("+"),
        )
    }
}

fn cargo_target_kind_name(kind: &CargoTargetKind) -> String {
    match kind {
        CargoTargetKind::Bench => "bench".to_owned(),
        CargoTargetKind::Bin => "bin".to_owned(),
        CargoTargetKind::CustomBuild => "custom-build".to_owned(),
        CargoTargetKind::CDyLib => "cdylib".to_owned(),
        CargoTargetKind::DyLib => "dylib".to_owned(),
        CargoTargetKind::Example => "example".to_owned(),
        CargoTargetKind::Lib => "lib".to_owned(),
        CargoTargetKind::ProcMacro => "proc-macro".to_owned(),
        CargoTargetKind::RLib => "rlib".to_owned(),
        CargoTargetKind::StaticLib => "staticlib".to_owned(),
        CargoTargetKind::Test => "test".to_owned(),
        CargoTargetKind::Unknown(value) => value.clone(),
        _ => "unknown".to_owned(),
    }
}

fn dependency_target_rank(target_kind: &TargetKind) -> u8 {
    match target_kind {
        TargetKind::Library => 2,
        TargetKind::ProcMacro => 1,
        _ => 0,
    }
}

fn stable_workspace_package_id(package: &CargoPackage, workspace_root: &Utf8Path) -> PackageId {
    let manifest = relative_or_absolute(&package.manifest_path, workspace_root);
    PackageId::new(format!("{} {} ({manifest})", package.name, package.version))
}

fn stable_workspace_crate_id(
    package_id: &PackageId,
    target_kind: &TargetKind,
    target_name: &str,
) -> CrateId {
    CrateId::new(format!(
        "{}#{}:{}",
        package_id,
        target_kind_key(target_kind),
        target_name
    ))
}

fn stable_module_id(crate_id: &CrateId, path: &str) -> ModuleId {
    ModuleId::new(format!("{}::{path}", crate_id))
}

fn stable_external_crate_id(package: &CargoPackage, workspace_root: &Utf8Path) -> ExternalCrateId {
    ExternalCrateId::new(format!(
        "{} {} [{}]",
        package.name,
        package.version,
        external_source_key(package, workspace_root)
    ))
}

fn target_kind_key(target_kind: &TargetKind) -> String {
    match target_kind {
        TargetKind::Library => "lib".to_owned(),
        TargetKind::Binary => "bin".to_owned(),
        TargetKind::Test => "test".to_owned(),
        TargetKind::Example => "example".to_owned(),
        TargetKind::Bench => "bench".to_owned(),
        TargetKind::BuildScript => "build".to_owned(),
        TargetKind::ProcMacro => "proc-macro".to_owned(),
        TargetKind::Other(value) => format!("other:{value}"),
    }
}

fn relative_or_absolute(path: &Utf8Path, workspace_root: &Utf8Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|relative| relative.to_string())
        .unwrap_or_else(|_| path.to_string())
}

fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

fn external_source_key(package: &CargoPackage, workspace_root: &Utf8Path) -> String {
    package
        .source
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| relative_or_absolute(&package.manifest_path, workspace_root))
}

fn external_crate_name(package: &CargoPackage) -> String {
    primary_dependency_target(package)
        .map(|target| normalize_crate_name(&target.name))
        .unwrap_or_else(|| normalize_crate_name(package.name.as_ref()))
}

fn primary_dependency_target(package: &CargoPackage) -> Option<&CargoTarget> {
    package
        .targets
        .iter()
        .find(|target| map_target_kind(target) == TargetKind::Library)
        .or_else(|| {
            package
                .targets
                .iter()
                .find(|target| map_target_kind(target) == TargetKind::ProcMacro)
        })
        .or_else(|| package.targets.first())
}

fn declared_edge_kind_for_target(
    target_kind: &TargetKind,
    dependency_kind: CargoDependencyKind,
) -> Option<DependencyKind> {
    match target_kind {
        TargetKind::BuildScript => {
            (dependency_kind == CargoDependencyKind::Build).then_some(DependencyKind::CargoBuild)
        }
        TargetKind::Test | TargetKind::Example | TargetKind::Bench => match dependency_kind {
            CargoDependencyKind::Normal => Some(DependencyKind::CargoNormal),
            CargoDependencyKind::Development => Some(DependencyKind::CargoDev),
            CargoDependencyKind::Build => None,
            _ => None,
        },
        _ => match dependency_kind {
            CargoDependencyKind::Normal => Some(DependencyKind::CargoNormal),
            CargoDependencyKind::Build | CargoDependencyKind::Development => None,
            _ => None,
        },
    }
}

fn declared_dependency_description(
    origin: &WorkspaceTarget,
    dependency: &cargo_metadata::NodeDep,
    dependency_kind: CargoDependencyKind,
) -> String {
    let mut description = match dependency_kind {
        CargoDependencyKind::Normal => "declared normal dependency".to_owned(),
        CargoDependencyKind::Build => "declared build dependency".to_owned(),
        CargoDependencyKind::Development => "declared dev dependency".to_owned(),
        _ => "declared dependency".to_owned(),
    };

    if dependency.name != origin.crate_name {
        description.push_str(&format!(" via `{}`", dependency.name));
    }
    description
}

impl Analyzer {
    fn select_workspace_target_for_ra_crate(
        &self,
        root_path: &Utf8Path,
        ra_crate_name: &str,
    ) -> Option<WorkspaceTarget> {
        let candidates = self.workspace_targets_by_src_path.get(root_path)?;
        if candidates.len() == 1 {
            return Some(self.workspace_targets[candidates[0]].clone());
        }

        let crate_name = normalize_crate_name(ra_crate_name);
        candidates
            .iter()
            .find_map(|index| {
                let target = &self.workspace_targets[*index];
                (target.crate_name == crate_name
                    || normalize_crate_name(&target.target_name) == crate_name)
                    .then(|| target.clone())
            })
            .or_else(|| {
                candidates
                    .first()
                    .map(|index| self.workspace_targets[*index].clone())
            })
    }
}

fn ra_crate_name(krate: HirCrate, db: &RootDatabase) -> String {
    krate
        .display_name(db)
        .map(|name| name.canonical_name().to_string())
        .unwrap_or_else(|| "<anonymous>".to_owned())
}

fn module_path(module: hir::Module, db: &RootDatabase, crate_name: &str) -> String {
    let segments = module
        .path_segments(db)
        .map(|name| name.display_no_db(module.krate(db).edition(db)).to_string())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        crate_name.to_owned()
    } else {
        format!("{crate_name}::{}", segments.join("::"))
    }
}

fn file_path_for_id(vfs: &Vfs, file_id: FileId) -> Result<Utf8PathBuf, AnalyzerError> {
    vfs.file_path(file_id)
        .as_path()
        .map(|path| Utf8PathBuf::from(path.to_string()))
        .ok_or_else(|| {
            AnalyzerError::InvalidOptions(format!(
                "file id {:?} is not a real filesystem path",
                file_id
            ))
        })
}

fn span_for_syntax(
    analyzer: &mut Analyzer,
    session: &RaSession,
    sema: &Semantics<'_, RootDatabase>,
    node: &SyntaxNode,
) -> Result<SourceSpan, AnalyzerError> {
    let file_range = sema.original_range(node);
    let file_id = file_range.file_id.file_id(&session.db);
    let path = file_path_for_id(&session.vfs, file_id)?;
    analyzer.line_cache.source_span(&path, file_range.range)
}

fn with_syntax_span(
    analyzer: &mut Analyzer,
    session: &RaSession,
    sema: &Semantics<'_, RootDatabase>,
    diagnostic: AnalysisDiagnostic,
    node: &SyntaxNode,
) -> Result<AnalysisDiagnostic, AnalyzerError> {
    Ok(diagnostic.with_span(span_for_syntax(analyzer, session, sema, node)?))
}

fn offset_to_position(text: &str, offset: usize) -> SourcePosition {
    let mut line = 1u32;
    let mut column = 1u32;
    for (index, ch) in text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    SourcePosition { line, column }
}

fn is_terminal_path(path: &ast::Path) -> bool {
    !path
        .syntax()
        .ancestors()
        .skip(1)
        .any(|ancestor| ast::Path::can_cast(ancestor.kind()))
}

fn is_macro_path(path: &ast::Path) -> bool {
    path.syntax()
        .ancestors()
        .find_map(ast::MacroCall::cast)
        .and_then(|macro_call| macro_call.path())
        .is_some_and(|macro_path| macro_path.syntax().text_range() == path.syntax().text_range())
}

fn syntax_is_cfg_enabled(node: &SyntaxNode, cfg_options: &CfgOptions) -> bool {
    node.ancestors()
        .filter_map(ast::AnyHasAttrs::cast)
        .flat_map(|owner| owner.attrs())
        .all(|attr| {
            attr.meta()
                .is_none_or(|meta| meta_is_cfg_enabled(meta, cfg_options))
        })
}

fn meta_is_cfg_enabled(meta: ast::Meta, cfg_options: &CfgOptions) -> bool {
    match meta {
        ast::Meta::CfgMeta(cfg) => cfg
            .cfg_predicate()
            .map(CfgExpr::parse_from_ast)
            .and_then(|expression| cfg_options.check(&expression))
            .unwrap_or(true),
        ast::Meta::CfgAttrMeta(cfg_attr) => {
            let applies = cfg_attr
                .cfg_predicate()
                .map(CfgExpr::parse_from_ast)
                .and_then(|expression| cfg_options.check(&expression));
            applies != Some(true)
                || cfg_attr
                    .metas()
                    .all(|meta| meta_is_cfg_enabled(meta, cfg_options))
        }
        _ => true,
    }
}

fn classify_path(path: &ast::Path) -> Option<DependencyKind> {
    let syntax = path.syntax();
    if syntax
        .ancestors()
        .any(|ancestor| ast::Attr::can_cast(ancestor.kind()))
    {
        return None;
    }

    if syntax.ancestors().find_map(ast::Use::cast).is_some() {
        let use_item = syntax.ancestors().find_map(ast::Use::cast)?;
        return Some(if use_item.visibility().is_some() {
            DependencyKind::ReExport
        } else {
            DependencyKind::Use
        });
    }

    if let Some(path_expr) = syntax.parent().and_then(ast::PathExpr::cast) {
        if let Some(call_expr) = path_expr.syntax().parent().and_then(ast::CallExpr::cast)
            && call_expr
                .expr()
                .is_some_and(|expr| expr.syntax().text_range() == path_expr.syntax().text_range())
        {
            return Some(DependencyKind::Call);
        }
        return Some(DependencyKind::Path);
    }

    if syntax.ancestors().any(|ancestor| {
        ast::PathType::can_cast(ancestor.kind()) || ast::TypeBound::can_cast(ancestor.kind())
    }) {
        return Some(DependencyKind::Type);
    }

    Some(DependencyKind::Path)
}
