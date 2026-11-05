//! Protonaut core: configuration, the versioned report model, static
//! build inspection and report rendering. This crate performs no process
//! execution and no network access.

pub mod config;
pub mod inspect;
pub mod model;
pub mod observe;
pub mod report;
pub mod runtime_checks;
pub mod static_checks;

/// JSON Schema (draft 2020-12) for `report.json`.
pub fn report_schema() -> serde_json::Value {
    schema_value(
        schemars::schema_for!(model::Report),
        "report.v1.schema.json",
        "Protonaut report",
    )
}

/// JSON Schema for `protonaut.toml` (expressed over its TOML data model).
pub fn config_schema() -> serde_json::Value {
    schema_value(
        schemars::schema_for!(config::ConfigFile),
        "config.v1.schema.json",
        "Protonaut configuration",
    )
}

fn schema_value(schema: schemars::Schema, file: &str, title: &str) -> serde_json::Value {
    let mut v = schema.to_value();
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "$id".into(),
            format!("https://github.com/Mahan-Imanian/protonaut/blob/main/schemas/{file}")
                .into(),
        );
        obj.insert("title".into(), title.into());
    }
    v
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::path::Path;

    fn committed(name: &str) -> serde_json::Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas")
            .join(name);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "missing {}; run `cargo run -p protonaut -- schema --write`",
                path.display()
            )
        });
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn committed_report_schema_matches_code() {
        assert_eq!(
            committed("report.v1.schema.json"),
            super::report_schema(),
            "run `cargo run -p protonaut -- schema --write`"
        );
    }

    #[test]
    fn committed_config_schema_matches_code() {
        assert_eq!(
            committed("config.v1.schema.json"),
            super::config_schema(),
            "run `cargo run -p protonaut -- schema --write`"
        );
    }
}
