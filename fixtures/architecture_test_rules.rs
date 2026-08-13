use bylaw::core::CycleGrouping;
use bylaw::prelude::*;

pub fn module_rules(crate_name: &str) -> Rules {
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
