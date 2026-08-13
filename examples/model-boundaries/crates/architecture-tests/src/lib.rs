#[cfg(test)]
mod tests {
    use bylaw::analyzer::{AnalysisOptions, analyze_workspace};
    use bylaw::prelude::*;
    use std::error::Error;

    #[test]
    fn model_boundaries_are_enforced() -> Result<(), Box<dyn Error>> {
        let manifest_path = format!("{}/../../Cargo.toml", env!("CARGO_MANIFEST_DIR")).into();
        let graph = analyze_workspace(&AnalysisOptions {
            manifest_path,
            selected_package_names: vec![
                "shop-domain".to_owned(),
                "shop-persistence".to_owned(),
                "shop-contract".to_owned(),
                "shop-api".to_owned(),
                "shop-app".to_owned(),
            ],
            ..AnalysisOptions::default()
        })?;

        let domain = packages(["shop-domain"]);
        let persistence = packages(["shop-persistence"]);
        let contract_model = packages(["shop-contract"]);
        let api = packages(["shop-api"]);
        let app = packages(["shop-app"]);
        let model_crates = domain
            .clone()
            .or(persistence.clone())
            .or(contract_model.clone())
            .or(api.clone());

        rules()
            .forbid_dependencies(
                "domain-is-internal",
                domain.clone(),
                persistence
                    .clone()
                    .or(contract_model.clone())
                    .or(api.clone())
                    .or(app.clone()),
            )
            .because("the internal domain model must not leak boundary representations")
            .forbid_dependencies(
                "contract-is-boundary-only",
                contract_model.clone(),
                domain
                    .clone()
                    .or(persistence.clone())
                    .or(api.clone())
                    .or(app.clone()),
            )
            .forbid_dependencies(
                "persistence-does-not-leak-transport-models",
                persistence.clone(),
                contract_model.clone().or(api.clone()).or(app.clone()),
            )
            .forbid_dependencies(
                "api-does-not-use-persistence-models",
                api.clone(),
                persistence.clone().or(app),
            )
            .no_cycles(
                "model-crates-are-acyclic",
                model_crates,
                CycleGrouping::Crates,
            )
            .layered(
                "model-layers",
                LayeredArchitecture::new()
                    .layer("domain", domain)
                    .layer("persistence", persistence)
                    .layer("contract", contract_model)
                    .layer("api", api)
                    .may_depend_on("persistence", ["domain"])
                    .may_depend_on("api", ["domain", "contract"]),
            )
            .check(&graph)?
            .assert();
        Ok(())
    }
}
